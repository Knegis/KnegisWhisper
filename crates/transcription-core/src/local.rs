//! Local transcription against a managed `whisper-server`.
//!
//! The server is a sidecar this application installed and started, bound to
//! loopback. Audio is posted to `/inference` as multipart and comes back as
//! `{"text": "..."}`. No audio leaves the machine on this path, which is the
//! whole point of it.
//!
//! Two things are ported from upstream deliberately and should not be
//! "simplified" away, because both were measured rather than guessed:
//!
//! - The decoder thresholds in [`DECODER_FIELDS`]. whisper.cpp's defaults let a
//!   mostly-silent thirty-second window pass the repetition check and emit
//!   training-data boilerplate — "Thank you for watching", and its equivalents
//!   in other languages — as if it were speech.
//! - `--no-timestamps`. whisper.cpp turned token timestamps on for every
//!   request, which enables the server's sixty-character segment wrap.
//!   `split_on_word` is off, so the wrap lands on a token boundary and breaks
//!   words in half. We join segments into one string and never read per-token
//!   timings, so turning timestamps off removes the wrap with them.
//!
//! What is *not* ported: upstream converts every input with ffmpeg before
//! sending, because it accepts arbitrary media. `kw_audio_core` produces 16 kHz
//! mono WAV at the point of capture, which is exactly what whisper.cpp wants, so
//! there is no transcoding step here and no ffmpeg dependency on the dictation
//! path.

use std::path::Path;

use kw_network_policy::NetworkPolicy;
use serde::Deserialize;

/// Decoder thresholds sent with every `/inference` request.
///
/// Upstream measured these over 4,814 real dictations: the hallucinated-tail
/// rate went from 2.25% to 0.06%. whisper.cpp's own defaults are entropy 2.4 and
/// logprob -1.0.
pub const DECODER_FIELDS: [(&str, &str); 2] =
    [("entropy_thold", "2.8"), ("logprob_thold", "-1.25")];

/// The path `whisper-server` serves inference on. Its own default, restated so
/// the argument list and the request cannot drift apart.
pub const INFERENCE_PATH: &str = "/inference";

/// The path used for health checks.
///
/// `GET /` returns the server's static index page. Any answer means it is
/// listening and speaking HTTP, which is the question being asked.
pub const HEALTH_PATH: &str = "/";

/// Voice-activity settings, when the caller wants the server to do the gating.
#[derive(Debug, Clone, PartialEq)]
pub struct VadSettings {
    pub model_path: String,
    pub threshold: f32,
    pub min_speech_ms: u32,
    pub min_silence_ms: u32,
    pub max_speech_seconds: f32,
    pub speech_pad_ms: u32,
    pub samples_overlap: f32,
}

/// How to start `whisper-server`.
#[derive(Debug, Clone, PartialEq)]
pub struct ServerOptions {
    /// The GGML model file.
    pub model_path: String,
    pub port: u16,
    /// A concrete language, or `None` for detection.
    ///
    /// `None` becomes `auto` rather than being omitted: whisper.cpp defaults to
    /// English when `--language` is absent, so leaving it out would silently
    /// transcribe every other language as though it were English.
    pub language: Option<String>,
    pub threads: Option<u32>,
    /// Index of the GPU device to pin to, where one was chosen.
    pub gpu_device: Option<u32>,
    pub vad: Option<VadSettings>,
}

/// The command-line arguments for a `whisper-server` matching `options`.
///
/// Pure, so the argument list is checked without starting anything.
#[must_use]
pub fn build_args(options: &ServerOptions) -> Vec<String> {
    let mut args = vec![
        "--model".to_owned(),
        options.model_path.clone(),
        // Never 0.0.0.0. The server holds a transcription endpoint with no
        // authentication of any kind; binding it to every interface would put
        // that on the network.
        "--host".to_owned(),
        "127.0.0.1".to_owned(),
        "--port".to_owned(),
        options.port.to_string(),
    ];

    if let Some(threads) = options.threads {
        args.push("--threads".to_owned());
        args.push(threads.to_string());
    }

    // `--device` counts the logical GPU devices ggml registers, which are the
    // indices whisper-server prints as "ggml_vulkan: N = ...". Not the same as
    // GGML_VK_VISIBLE_DEVICES, which takes raw physical enumeration indices and
    // diverges whenever a device is filtered out.
    if let Some(device) = options.gpu_device {
        args.push("--device".to_owned());
        args.push(device.to_string());
    }

    args.push("--language".to_owned());
    args.push(
        options
            .language
            .clone()
            .unwrap_or_else(|| "auto".to_owned()),
    );

    // See the module comment: this is about mid-word breaks, not about
    // timestamps being unwanted.
    args.push("--no-timestamps".to_owned());

    if let Some(vad) = &options.vad {
        args.push("--vad".to_owned());
        args.push("--vad-model".to_owned());
        args.push(vad.model_path.clone());
        args.push("--vad-threshold".to_owned());
        args.push(vad.threshold.to_string());
        args.push("--vad-min-speech-duration-ms".to_owned());
        args.push(vad.min_speech_ms.to_string());
        args.push("--vad-min-silence-duration-ms".to_owned());
        args.push(vad.min_silence_ms.to_string());
        args.push("--vad-max-speech-duration-s".to_owned());
        args.push(vad.max_speech_seconds.to_string());
        args.push("--vad-speech-pad-ms".to_owned());
        args.push(vad.speech_pad_ms.to_string());
        args.push("--vad-samples-overlap".to_owned());
        args.push(vad.samples_overlap.to_string());
    }

    args
}

/// Whether a model file is where it is supposed to be.
///
/// Checked before starting, because whisper-server's own failure for a missing
/// model is an exit during startup with a message that has to be read out of
/// captured stderr. Saying so up front is cheaper and clearer.
#[must_use]
pub fn model_is_present(model_path: &str) -> bool {
    Path::new(model_path).is_file()
}

/// What `/inference` said.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Inference {
    /// A transcript. Already trimmed.
    Text(String),
    /// The request succeeded and produced nothing usable.
    Empty,
    /// The server rejected it.
    Rejected { status: u16, body: String },
}

#[derive(Debug, Deserialize)]
struct InferenceBody {
    text: Option<String>,
}

/// How much of an error body is worth repeating.
const BODY_LIMIT: usize = 400;

/// Interpret a response from `/inference`.
///
/// Pure, so every shape is covered without a server. A 2xx carrying no `text`
/// field is a failure, not an empty transcript: the difference between "the
/// model heard nothing" and "the response was not what we expected" matters when
/// someone is trying to work out why dictation produced nothing.
#[must_use]
pub fn interpret(status: u16, body: &str) -> Inference {
    if !(200..300).contains(&status) {
        return Inference::Rejected {
            status,
            body: truncate(body),
        };
    }

    match serde_json::from_str::<InferenceBody>(body) {
        Ok(parsed) => match parsed.text {
            // Leading space and trailing newline are how whisper.cpp returns
            // every transcript, so trimming is not optional tidying.
            Some(text) => {
                let trimmed = text.trim();
                if trimmed.is_empty() {
                    Inference::Empty
                } else {
                    Inference::Text(trimmed.to_owned())
                }
            }
            None => Inference::Rejected {
                status,
                body: truncate(body),
            },
        },
        Err(_) => Inference::Rejected {
            status,
            body: truncate(body),
        },
    }
}

fn truncate(body: &str) -> String {
    if body.chars().count() <= BODY_LIMIT {
        return body.to_owned();
    }
    let cut = body
        .char_indices()
        .nth(BODY_LIMIT)
        .map_or(body.len(), |(index, _)| index);
    let mut out = body.get(..cut).unwrap_or(body).to_owned();
    out.push('…');
    out
}

/// Something that can transcribe locally.
///
/// A trait so the executor can be tested, and so a future local runtime
/// (sherpa-onnx) slots in without the executor knowing which one it has.
#[async_trait::async_trait]
pub trait LocalTranscriber: Send + Sync + std::fmt::Debug {
    /// Transcribe 16 kHz mono WAV.
    ///
    /// `prompt` biases recognition — the custom dictionary goes here.
    async fn transcribe(
        &self,
        wav: Vec<u8>,
        language: Option<&str>,
        prompt: Option<&str>,
    ) -> Result<Inference, String>;

    /// A name for the history row.
    fn model_name(&self) -> Option<String> {
        None
    }
}

/// A running `whisper-server` on loopback.
#[derive(Debug, Clone)]
pub struct WhisperServer {
    policy: NetworkPolicy,
    port: u16,
    model: Option<String>,
}

impl WhisperServer {
    #[must_use]
    pub const fn new(policy: NetworkPolicy, port: u16, model: Option<String>) -> Self {
        Self {
            policy,
            port,
            model,
        }
    }

    #[must_use]
    pub const fn port(&self) -> u16 {
        self.port
    }
}

#[async_trait::async_trait]
impl LocalTranscriber for WhisperServer {
    async fn transcribe(
        &self,
        wav: Vec<u8>,
        language: Option<&str>,
        prompt: Option<&str>,
    ) -> Result<Inference, String> {
        let part = reqwest::multipart::Part::bytes(wav)
            .file_name("audio.wav")
            .mime_str("audio/wav")
            .map_err(|err| err.to_string())?;

        let mut form = reqwest::multipart::Form::new()
            .part("file", part)
            .text("language", language.unwrap_or("auto").to_owned())
            .text("response_format", "json");

        for (name, value) in DECODER_FIELDS {
            form = form.text(name, value);
        }

        if let Some(prompt) = prompt.filter(|p| !p.trim().is_empty()) {
            form = form.text("prompt", prompt.to_owned());
        }

        let response = self
            .policy
            // Not one of the seven egress purposes: this is a process we
            // started, on 127.0.0.1, and nothing leaves the machine.
            .loopback(reqwest::Method::POST, self.port, INFERENCE_PATH)
            .map_err(|err| err.to_string())?
            .multipart(form)
            .send()
            .await
            .map_err(|err| err.to_string())?;

        let status = response.status().as_u16();
        let body = response.text().await.map_err(|err| err.to_string())?;
        Ok(interpret(status, &body))
    }

    fn model_name(&self) -> Option<String> {
        self.model.clone()
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use super::*;

    fn options() -> ServerOptions {
        ServerOptions {
            model_path: "C:/kw/models/ggml-base.bin".to_owned(),
            port: 8178,
            language: None,
            threads: None,
            gpu_device: None,
            vad: None,
        }
    }

    fn find(args: &[String], flag: &str) -> Option<String> {
        args.iter()
            .position(|arg| arg == flag)
            .and_then(|index| args.get(index + 1))
            .cloned()
    }

    // ---------------------------------------------------------------- arguments

    #[test]
    fn the_server_is_told_to_bind_loopback() {
        // An unauthenticated transcription endpoint on 0.0.0.0 would be a
        // transcription service for the whole network.
        let args = build_args(&options());
        assert_eq!(find(&args, "--host").as_deref(), Some("127.0.0.1"));
        assert!(!args.iter().any(|arg| arg.contains("0.0.0.0")));
    }

    #[test]
    fn the_model_and_port_are_passed() {
        let args = build_args(&options());
        assert_eq!(
            find(&args, "--model").as_deref(),
            Some("C:/kw/models/ggml-base.bin")
        );
        assert_eq!(find(&args, "--port").as_deref(), Some("8178"));
    }

    #[test]
    fn no_language_becomes_auto_rather_than_being_omitted() {
        // whisper.cpp defaults to English when --language is absent, so omitting
        // it would transcribe every other language as though it were English.
        let args = build_args(&options());
        assert_eq!(find(&args, "--language").as_deref(), Some("auto"));
    }

    #[test]
    fn a_chosen_language_is_passed_through() {
        let mut options = options();
        options.language = Some("sv".to_owned());
        assert_eq!(
            find(&build_args(&options), "--language").as_deref(),
            Some("sv")
        );
    }

    #[test]
    fn timestamps_are_turned_off() {
        // Not cosmetic: with timestamps on, the server's 60-character wrap lands
        // on a token boundary and breaks words in half.
        assert!(build_args(&options())
            .iter()
            .any(|a| a == "--no-timestamps"));
    }

    #[test]
    fn threads_and_gpu_device_are_omitted_when_not_chosen() {
        // Passing a default would override whisper.cpp's own, which is better
        // informed about the machine than we are.
        let args = build_args(&options());
        assert!(!args.iter().any(|a| a == "--threads"));
        assert!(!args.iter().any(|a| a == "--device"));
    }

    #[test]
    fn threads_and_gpu_device_are_passed_when_chosen() {
        let mut options = options();
        options.threads = Some(8);
        options.gpu_device = Some(1);
        let args = build_args(&options);
        assert_eq!(find(&args, "--threads").as_deref(), Some("8"));
        assert_eq!(find(&args, "--device").as_deref(), Some("1"));
    }

    #[test]
    fn vad_is_off_unless_configured() {
        assert!(!build_args(&options()).iter().any(|a| a == "--vad"));
    }

    #[test]
    fn vad_settings_are_all_passed_when_configured() {
        let mut options = options();
        options.vad = Some(VadSettings {
            model_path: "C:/kw/models/silero.onnx".to_owned(),
            threshold: 0.5,
            min_speech_ms: 250,
            min_silence_ms: 100,
            max_speech_seconds: 30.0,
            speech_pad_ms: 30,
            samples_overlap: 0.1,
        });
        let args = build_args(&options);

        assert!(args.iter().any(|a| a == "--vad"));
        assert_eq!(
            find(&args, "--vad-model").as_deref(),
            Some("C:/kw/models/silero.onnx")
        );
        for flag in [
            "--vad-threshold",
            "--vad-min-speech-duration-ms",
            "--vad-min-silence-duration-ms",
            "--vad-max-speech-duration-s",
            "--vad-speech-pad-ms",
            "--vad-samples-overlap",
        ] {
            assert!(find(&args, flag).is_some(), "{flag} should have a value");
        }
    }

    #[test]
    fn the_decoder_thresholds_are_the_measured_ones() {
        // whisper.cpp's defaults are 2.4 and -1.0. These were measured over
        // 4,814 dictations; a well-meaning "tidy up to the defaults" would
        // reintroduce the hallucinated tails.
        assert_eq!(
            DECODER_FIELDS,
            [("entropy_thold", "2.8"), ("logprob_thold", "-1.25")]
        );
    }

    // -------------------------------------------------------------- responses

    #[test]
    fn a_transcript_is_trimmed() {
        // The exact shape whisper-server returned in the run this was written
        // against: a leading space and a trailing newline.
        let body = r#"{"text":" The quick brown fox jumps over the lazy dog.\n"}"#;
        assert_eq!(
            interpret(200, body),
            Inference::Text("The quick brown fox jumps over the lazy dog.".to_owned())
        );
    }

    #[test]
    fn silence_produces_an_empty_result_rather_than_an_error() {
        // The model heard nothing. Distinct from a broken response, and the
        // caller shows a different message.
        assert_eq!(interpret(200, r#"{"text":"   \n"}"#), Inference::Empty);
        assert_eq!(interpret(200, r#"{"text":""}"#), Inference::Empty);
    }

    #[test]
    fn a_success_with_no_text_field_is_a_failure_not_an_empty_transcript() {
        // "The model heard nothing" and "the response was not what we expected"
        // lead to different investigations.
        match interpret(200, r#"{"segments":[]}"#) {
            Inference::Rejected { status, body } => {
                assert_eq!(status, 200);
                assert!(body.contains("segments"));
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_non_json_success_body_is_a_failure() {
        // An HTML error page served with a 200, which is what a misconfigured
        // proxy in front of the port would produce.
        assert!(matches!(
            interpret(200, "<html>Bad Gateway</html>"),
            Inference::Rejected { .. }
        ));
    }

    #[test]
    fn a_server_error_is_reported_with_its_status_and_body() {
        match interpret(500, "model not loaded") {
            Inference::Rejected { status, body } => {
                assert_eq!(status, 500);
                assert_eq!(body, "model not loaded");
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_long_error_body_is_truncated_on_a_character_boundary() {
        let body = "å".repeat(BODY_LIMIT + 50);
        match interpret(500, &body) {
            Inference::Rejected { body, .. } => {
                assert!(body.ends_with('…'));
                assert_eq!(body.chars().count(), BODY_LIMIT + 1);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_missing_model_file_is_noticed_before_starting() {
        assert!(!model_is_present("C:/kw/models/definitely-not-here.bin"));

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ggml-tiny.bin");
        std::fs::write(&path, b"not really a model").unwrap();
        assert!(model_is_present(&path.display().to_string()));
    }

    #[test]
    fn the_inference_and_health_paths_are_loopback_paths() {
        // Both are handed to NetworkPolicy::loopback, which requires a leading
        // slash and refuses anything carrying a scheme.
        for path in [INFERENCE_PATH, HEALTH_PATH] {
            assert!(path.starts_with('/'));
            assert!(!path.contains("://"));
        }
    }
}
