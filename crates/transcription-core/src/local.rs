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
//! - The segment length cap, [`MAX_SEGMENT_CHARS`]. This replaced
//!   `--no-timestamps` on 2026-08-31 and the reason is measured -- see
//!   **Timings** below.
//!
//! # Timings, and why `--no-timestamps` went
//!
//! `--no-timestamps` was ported from upstream because whisper.cpp's server wraps
//! segments at sixty characters when timestamps are on, and with `split_on_word`
//! off that wrap lands on a token boundary and breaks words in half. Turning
//! timestamps off removed the wrap.
//!
//! It also removed every timing, which is not what anybody wanted. Measured on
//! this machine against `ggml-tiny` on 2026-08-31: with `--no-timestamps`,
//! `response_format=verbose_json` returns **one** segment for the whole
//! recording carrying no `start` and no `end` at all. That is why a 57-minute
//! import exported as a single subtitle cue at `00:00:00` -- not because the
//! import failed to divide it, but because there was nothing to divide.
//!
//! The cure is not `split_on_word`. Neither it nor `max_len` is honoured as a
//! request field -- all three shapes came back identical -- and `--max-len 0`
//! changes nothing either, because the server reads 0 as 60. An explicit
//! [`MAX_SEGMENT_CHARS`] does what was wanted:
//!
//! | server flags | `json` | `verbose_json` |
//! | --- | --- | --- |
//! | `--no-timestamps` | 0.63-0.65 s | one segment, no timings |
//! | `--max-len 1000` | 0.66-0.70 s | 0.95-1.01 s, one segment per utterance with real `start`/`end` |
//!
//! Three runs each over 13.5 s of speech. Two things that decides:
//!
//! * **The wrap is gone, so nothing needs splitting on words.** A cap far above
//!   what a 30-second window can hold means the segment boundaries fall where
//!   whisper's own timestamps fall -- at utterance ends. The `\n` that remains in
//!   `text` is a paragraph break between utterances, not a break inside one, and
//!   [`interpret`] collapses it. The result is **byte-identical** to what
//!   `--no-timestamps` produced, which was checked rather than assumed.
//! * **The cost lands only where it is asked for.** Timestamps cost 3-7% on the
//!   `json` path, which dictation uses. `verbose_json` costs about 45% more,
//!   because it carries per-word timings and probabilities, and only media
//!   import asks for it -- see [`WhisperServer::timed`].
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

/// The longest segment `whisper-server` may emit, in characters.
///
/// Deliberately far above what one 30-second decode window can hold -- roughly
/// 450 characters of ordinary speech, and under 800 even for a dense language --
/// so it never actually cuts anything. It is set only because the server treats
/// `0` as 60 and would otherwise wrap mid-utterance; see the module doc.
///
/// Not a per-request field. It and `split_on_word` are start-time flags, which
/// was measured by sending both as form fields and getting a byte-identical
/// response back.
pub const MAX_SEGMENT_CHARS: u32 = 1000;

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

    // See the module comment. This used to be `--no-timestamps`, which removed
    // the mid-utterance wrap and every timing with it; capping the segment
    // length removes the wrap and keeps the timings.
    args.push("--max-len".to_owned());
    args.push(MAX_SEGMENT_CHARS.to_string());

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
#[derive(Debug, Clone, PartialEq)]
pub enum Inference {
    /// A transcript with no timings. Already trimmed.
    Text(String),
    /// A transcript the engine divided into timed utterances.
    ///
    /// Produced only by `response_format=verbose_json` against a server that
    /// was started with timestamps on, and asked for only by the paths that can
    /// use it -- see [`WhisperServer::timed`]. Never synthesised: a response
    /// whose segments carry no `start` and `end` comes back as [`Self::Text`],
    /// so this variant cannot claim a timing that did not arrive.
    Timed {
        /// The whole transcript, collapsed to one line as [`Self::Text`] is.
        text: String,
        /// In order, and never empty.
        segments: Vec<TimedSegment>,
    },
    /// The request succeeded and produced nothing usable.
    Empty,
    /// The server rejected it.
    Rejected { status: u16, body: String },
}

/// One timed utterance of a local transcript.
///
/// Seconds from the start of the recording, which is what
/// `kw_notes_core::transcript::Segment` wants and what SRT and WebVTT need.
///
/// `Serialize` only. This crate has no `ts-rs` dependency and should not gain
/// one for a type the desktop crate can name in its own IPC struct.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TimedSegment {
    pub text: String,
    pub start: f64,
    pub end: f64,
}

#[derive(Debug, Deserialize)]
struct InferenceBody {
    text: Option<String>,
    /// Present for `verbose_json` only, and then whether or not the server was
    /// started with timestamps on.
    #[serde(default)]
    segments: Vec<SegmentBody>,
}

#[derive(Debug, Deserialize)]
struct SegmentBody {
    text: Option<String>,
    /// Absent when the server ran with `--no-timestamps`, which is the whole
    /// reason these are `Option`: a missing timing must not become `0.0`.
    start: Option<f64>,
    end: Option<f64>,
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

    let rejected = || Inference::Rejected {
        status,
        body: truncate(body),
    };

    let Ok(parsed) = serde_json::from_str::<InferenceBody>(body) else {
        return rejected();
    };
    let Some(text) = parsed.text else {
        return rejected();
    };

    // Leading space and trailing newline are how whisper.cpp returns every
    // transcript, so trimming is not optional tidying. The interior newlines are
    // new: they mark the utterance boundaries that came back with the timings,
    // and collapsing them is what keeps this byte-identical to what
    // `--no-timestamps` used to return.
    let text = collapse(&text);
    if text.is_empty() {
        return Inference::Empty;
    }

    let segments = timed_segments(&parsed.segments);
    if segments.is_empty() {
        return Inference::Text(text);
    }
    Inference::Timed { text, segments }
}

/// Whitespace runs to single spaces, then trimmed.
fn collapse(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The segments that carry a real timing, in order.
///
/// Empty unless **every** segment has both a `start` and an `end`. All-or-
/// nothing on purpose: a response where some utterances are timed and others
/// are not would produce a transcript whose later cues are silently wrong,
/// which is worse than one with no cues at all. `--no-timestamps` returns one
/// segment with neither field, so that shape lands here and is discarded.
fn timed_segments(bodies: &[SegmentBody]) -> Vec<TimedSegment> {
    let mut out = Vec::with_capacity(bodies.len());
    for body in bodies {
        let (Some(start), Some(end)) = (body.start, body.end) else {
            return Vec::new();
        };
        let Some(text) = body.text.as_deref().map(collapse) else {
            return Vec::new();
        };
        if text.is_empty() {
            continue;
        }
        out.push(TimedSegment { text, start, end });
    }
    out
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

    /// Engines without language detection retain their own language behavior.
    async fn transcribe_languages(
        &self,
        wav: Vec<u8>,
        languages: &[String],
        prompt: Option<&str>,
    ) -> Result<Inference, String> {
        self.transcribe(
            wav,
            if languages.len() == 1 {
                languages.first().map(String::as_str)
            } else {
                None
            },
            prompt,
        )
        .await
    }

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
    /// Whether to ask for per-utterance timings. See [`Self::timed`].
    timed: bool,
}

impl WhisperServer {
    #[must_use]
    pub const fn new(policy: NetworkPolicy, port: u16, model: Option<String>) -> Self {
        Self {
            policy,
            port,
            model,
            timed: false,
        }
    }

    /// Ask for per-utterance timings, at about 45% more wall clock.
    ///
    /// A builder on the server rather than a parameter on
    /// [`LocalTranscriber::transcribe`], because whether timings are wanted is a
    /// property of what the caller is building -- a subtitle track or a line of
    /// dictated text -- and not of one request. Adding a parameter would have
    /// put the question to five implementors, four of which cannot answer it.
    ///
    /// Off by default, so a caller gets the cheap answer unless it says
    /// otherwise. Dictation and meetings want one line of text; media import
    /// wants cues, and pays for them. The measurement is in the module doc.
    #[must_use]
    pub const fn timed(mut self) -> Self {
        self.timed = true;
        self
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
            .text(
                "response_format",
                if self.timed { "verbose_json" } else { "json" },
            );

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
            .map_err(|err| describe_send_failure(self.port, &err))?;

        let status = response.status().as_u16();
        let body = response.text().await.map_err(|err| err.to_string())?;
        Ok(interpret(status, &body))
    }

    async fn transcribe_languages(
        &self,
        wav: Vec<u8>,
        languages: &[String],
        prompt: Option<&str>,
    ) -> Result<Inference, String> {
        if languages.len() > 2 {
            return Err("Choose at most two dictation languages".into());
        }
        if languages.len() < 2 {
            return self
                .transcribe(wav, languages.first().map(String::as_str), prompt)
                .await;
        }
        let part = reqwest::multipart::Part::bytes(wav.clone())
            .file_name("audio.wav")
            .mime_str("audio/wav")
            .map_err(|e| e.to_string())?;
        let form = reqwest::multipart::Form::new()
            .part("file", part)
            .text("language", "auto")
            .text("detect_language", "true")
            .text("response_format", "verbose_json")
            .text("no_language_probabilities", "false");
        let response = self
            .policy
            .loopback(reqwest::Method::POST, self.port, INFERENCE_PATH)
            .map_err(|e| e.to_string())?
            .multipart(form)
            .send()
            .await
            .map_err(|e| describe_send_failure(self.port, &e))?;
        let response = response.error_for_status().map_err(|e| e.to_string())?;
        let body: serde_json::Value = response.json().await.map_err(|e| e.to_string())?;
        let language = selected_language(&body, languages)?;
        self.transcribe(wav, Some(&language), prompt).await
    }

    fn model_name(&self) -> Option<String> {
        self.model.clone()
    }
}

fn selected_language(body: &serde_json::Value, languages: &[String]) -> Result<String, String> {
    languages.iter().filter_map(|language| {
        let probability = body.get("language_probabilities")?.get(language)?.as_f64()?;
        (probability.is_finite() && probability > 0.0).then_some((language, probability))
    }).max_by(|a, b| a.1.total_cmp(&b.1)).map(|(language, _)| language.clone())
      .ok_or_else(|| "Could not detect either selected language. Choose automatic detection or one language and try again.".to_owned())
}

/// Say which failure this was, because the two need different fixes.
///
/// A connect error means nothing was listening -- the server exited, or never
/// came up -- and the person needs to start it. Anything else means something
/// answered and went wrong, and the detail is the useful part. Reporting both
/// as "the local model server refused the request" sent a bench check on
/// 2026-08-28 looking for a server that was refusing when there was no server
/// at all: nothing on the port, no process, and the application still holding
/// the port from one that had gone.
fn describe_send_failure(port: u16, err: &reqwest::Error) -> String {
    if err.is_connect() {
        return format!(
            "nothing is listening on 127.0.0.1:{port}. The local model server is not \
             running -- start it again under Settings > Transcription > Local."
        );
    }
    if err.is_timeout() {
        return format!("the local model server on 127.0.0.1:{port} did not answer in time");
    }
    err.to_string()
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    #[test]
    fn pair_detection_only_chooses_selected_languages_and_rejects_missing_scores() {
        let candidates = vec!["sv".to_owned(), "en".to_owned()];
        let body = serde_json::json!({"language_probabilities":{"de":0.8,"sv":0.1,"en":0.09}});
        assert_eq!(super::selected_language(&body, &candidates).unwrap(), "sv");
        assert!(super::selected_language(
            &serde_json::json!({"language_probabilities":{"de":1.0}}),
            &candidates
        )
        .is_err());
    }

    #[tokio::test]
    async fn two_languages_detect_then_send_the_selected_language_for_transcription() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = tokio::spawn(async move {
            for phase in 0..2 {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut bytes = Vec::new();
                loop {
                    let mut chunk = [0; 4096];
                    let count = socket.read(&mut chunk).await.unwrap();
                    assert!(count > 0);
                    bytes.extend_from_slice(&chunk[..count]);
                    if let Some(end) = bytes.windows(4).position(|p| p == b"\r\n\r\n") {
                        let headers = String::from_utf8_lossy(&bytes[..end]).to_lowercase();
                        let length: usize = headers
                            .lines()
                            .find_map(|l| l.strip_prefix("content-length:"))
                            .unwrap()
                            .trim()
                            .parse()
                            .unwrap();
                        if bytes.len() >= end + 4 + length {
                            break;
                        }
                    }
                }
                let request = String::from_utf8_lossy(&bytes);
                let body = if phase == 0 {
                    assert!(request.contains("name=\"detect_language\"\r\n\r\ntrue"));
                    assert!(!request.contains("name=\"prompt\""));
                    r#"{"language_probabilities":{"de":0.7,"en":0.2,"sv":0.1}}"#
                } else {
                    assert!(request.contains("name=\"language\"\r\n\r\nen"));
                    assert!(request.contains("name=\"prompt\"\r\n\r\nDyve, \u{00c5}sa Karlsson"));
                    assert!(!request.contains("name=\"detect_language\""));
                    r#"{"text":"Hello"}"#
                };
                socket.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body).as_bytes()).await.unwrap();
            }
        });
        let engine =
            super::WhisperServer::new(kw_network_policy::NetworkPolicy::new().unwrap(), port, None);
        let result = super::LocalTranscriber::transcribe_languages(
            &engine,
            vec![0, 1],
            &["sv".into(), "en".into()],
            Some("Dyve, \u{00c5}sa Karlsson"),
        )
        .await
        .unwrap();
        assert!(matches!(result, super::Inference::Text(text) if text == "Hello"));
        server.await.unwrap();
    }

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
    fn the_segment_length_is_capped_rather_than_timestamps_turned_off() {
        // The cap is what removes the server's 60-character mid-utterance wrap
        // now. `--no-timestamps` also removed it and took every timing with it,
        // which is why a 57-minute import exported as one subtitle cue.
        let args = build_args(&options());
        assert!(!args.iter().any(|a| a == "--no-timestamps"));
        assert_eq!(
            find(&args, "--max-len").as_deref(),
            Some(MAX_SEGMENT_CHARS.to_string().as_str())
        );
    }

    /* ------------------------------------- what the server actually answers */

    /// `verbose_json` from a server started with the cap, measured on
    /// 2026-08-31 against `ggml-tiny`. Trimmed to the fields this code reads;
    /// the real response also carries `tokens`, `words`, `temperature`,
    /// `avg_logprob` and `no_speech_prob` per segment.
    const TIMED_BODY: &str = r#"{
      "task":"transcribe","language":"english","duration":13.524999618530273,
      "text":" The quick brown fox jumps over the lazy dog.\n Second sentence, spoken after a pause, so the server has somewhere to cut.\n Third and last sentence of the probe recording.\n",
      "segments":[
        {"id":0,"text":" The quick brown fox jumps over the lazy dog.","start":0.0,"end":3.0},
        {"id":1,"text":" Second sentence, spoken after a pause, so the server has somewhere to cut.","start":3.68,"end":9.44},
        {"id":2,"text":" Third and last sentence of the probe recording.","start":10.08,"end":12.88}
      ]}"#;

    /// The same request against a server started with `--no-timestamps`: one
    /// segment for the whole recording, carrying neither `start` nor `end`.
    /// Measured in the same session, and the shape that produced the defect.
    const UNTIMED_BODY: &str = r#"{
      "task":"transcribe","language":"english","duration":13.524999618530273,
      "text":" The quick brown fox jumps over the lazy dog. Second sentence, spoken after a pause, so the server has somewhere to cut. Third and last sentence of the probe recording.\n",
      "segments":[
        {"id":0,"text":" The quick brown fox jumps over the lazy dog. Second sentence, spoken after a pause, so the server has somewhere to cut. Third and last sentence of the probe recording."}
      ]}"#;

    #[test]
    fn a_timed_response_is_read_as_timed() {
        match interpret(200, TIMED_BODY) {
            Inference::Timed { text, segments } => {
                assert_eq!(segments.len(), 3);
                assert_eq!(segments[0].start, 0.0);
                assert_eq!(segments[0].end, 3.0);
                assert_eq!(segments[2].start, 10.08);
                assert_eq!(
                    segments[1].text,
                    "Second sentence, spoken after a pause, so the server has somewhere to cut."
                );
                // The interior newlines are utterance boundaries and are
                // collapsed, so a caller that only wants the prose gets exactly
                // what it got before timings existed.
                assert!(!text.contains('\n'));
                assert!(text.starts_with("The quick brown fox"));
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn an_untimed_response_is_not_dressed_up_as_one_segment() {
        // The defect, as a test. A single segment spanning the whole recording
        // with no `start` and no `end` is not a timing, and turning it into
        // `0.0 -> duration` is what made a 57-minute import export as one cue.
        // It must come back as plain text so nothing downstream believes there
        // are cues to write.
        match interpret(200, UNTIMED_BODY) {
            Inference::Text(text) => {
                assert!(text.starts_with("The quick brown fox"));
                assert!(text.ends_with("probe recording."));
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn the_two_shapes_agree_on_the_prose() {
        // Measured as byte-identical against the real server, and asserted here
        // so a future change to `collapse` cannot quietly alter dictation.
        let timed = match interpret(200, TIMED_BODY) {
            Inference::Timed { text, .. } => text,
            other => panic!("{other:?}"),
        };
        let untimed = match interpret(200, UNTIMED_BODY) {
            Inference::Text(text) => text,
            other => panic!("{other:?}"),
        };
        assert_eq!(timed, untimed);
    }

    #[test]
    fn a_partly_timed_response_is_refused_wholesale() {
        // Half a set of cues is worse than none: the untimed ones would have to
        // be placed somewhere, and any choice is a fabricated timestamp.
        let body = r#"{"text":"a b","segments":[
            {"text":"a","start":0.0,"end":1.0},
            {"text":"b"}
        ]}"#;
        assert!(matches!(interpret(200, body), Inference::Text(_)));
    }

    #[test]
    fn timings_are_only_asked_for_when_wanted() {
        // The request field is the only difference, and it costs about 45% more
        // wall clock -- so the default must be the cheap one.
        let policy = kw_network_policy::NetworkPolicy::new().unwrap();
        assert!(!WhisperServer::new(policy.clone(), 8178, None).timed);
        assert!(WhisperServer::new(policy, 8178, None).timed().timed);
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
