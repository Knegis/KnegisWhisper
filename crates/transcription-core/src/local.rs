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
//!   because it carries per-token timings and probabilities, and only media
//!   import and the meeting's whole-recording pass ask for it -- see
//!   [`WhisperServer::timed`]. Both keep those timings as words (KNEG-408, see
//!   [`TimedSegment::words`]).
//!
//! What is *not* ported: upstream converts every input with ffmpeg before
//! sending, because it accepts arbitrary media. `kw_audio_core` produces 16 kHz
//! mono WAV at the point of capture, which is exactly what whisper.cpp wants, so
//! there is no transcoding step here and no ffmpeg dependency on the dictation
//! path.

use std::path::Path;
use std::sync::Arc;

use kw_network_policy::NetworkPolicy;
use serde::Deserialize;

use crate::speech::{self, FindSpeech};

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

/// How to start `whisper-server`.
///
/// There is no VAD option. The server's own VAD (`--vad`, `--vad-model`) was
/// measured on 2026-10-06 putting row boundaries up to 30 s from their speech,
/// and started with `--vad` it would apply to every request, dictation's
/// included. Long recordings skip silence by finding the speech first
/// instead; see [`crate::speech`].
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
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TimedSegment {
    pub text: String,
    pub start: f64,
    pub end: f64,
    /// The utterance word by word, when whisper timed every word of it
    /// (KNEG-408). Empty -- and left out of the JSON -- otherwise: every
    /// engine but the whisper server, and any utterance one of whose words
    /// came back without a time. See [`merge_tokens`].
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub words: Vec<TimedWord>,
}

/// One word of a [`TimedSegment`], on the same clock.
///
/// Whisper times *tokens*, not words: " Stock" and "holm" are two. A word is
/// the tokens from one that starts with a space up to the next, so its start
/// is its first token's and its end its last's. Rounded to 0.01 s, which is
/// the server's own resolution (it reports centiseconds times 0.01).
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct TimedWord {
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
    /// The segment's tokens, each `{"word", "start", "end", ...}`, as
    /// whisper.cpp's server writes them for `verbose_json`: read in its
    /// source at b4938, and seen from the pinned server on 2026-10-08
    /// (`tests/word_timings.rs`). The key is `word` although the value is a
    /// token. Kept as raw JSON and read leniently: words are extra, and a
    /// token in a shape this does not expect must cost the words, never the
    /// transcript.
    #[serde(default)]
    words: serde_json::Value,
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
        let mut words = merge_tokens(body.words.as_array().map_or(&[], Vec::as_slice));
        // Words are shown in place of the row's text, so words that no
        // longer spell it -- a token can be half a character, which the
        // server writes as U+FFFD -- are not kept.
        if !spells(&words, &text) {
            tracing::warn!(
                discarded_words = words.len(),
                reason = "word-text-mismatch",
                "discarded unusable word timings"
            );
            words.clear();
        }
        out.push(TimedSegment {
            text,
            start,
            end,
            words,
        });
    }
    out
}

/// A segment's tokens, joined into timed words (KNEG-408).
///
/// A token that starts with a space starts a word, and any other token --
/// "holm" after " Stock", or "," -- ends up in the word before it. A token
/// with no text is skipped, and so are special tokens such as `[_BEG_]` and
/// `<|endoftext|>`; whisper.cpp's server already leaves those out, so this
/// only keeps one that slipped through out of a note.
///
/// All or nothing per segment, the rule [`timed_segments`] follows for
/// segments: one token with text and no time -- missing, negative or not a
/// number -- and the segment has no words, rather than words highlighted at
/// a time nobody reported. Times are rounded to 0.01 s.
fn merge_tokens(tokens: &[serde_json::Value]) -> Vec<TimedWord> {
    let mut words: Vec<TimedWord> = Vec::new();
    // Whether the next token starts a word even without a leading space:
    // the first one does, and so does one after a token of spaces alone.
    let mut breaks = true;
    for token in tokens {
        let Some(raw) = token.get("word").and_then(serde_json::Value::as_str) else {
            continue;
        };
        if raw.trim().is_empty() {
            breaks = breaks || !raw.is_empty();
            continue;
        }
        if is_special(raw.trim()) {
            continue;
        }
        let (Some(start), Some(end)) = (token_time(token, "start"), token_time(token, "end"))
        else {
            tracing::warn!(
                discarded_tokens = tokens.len(),
                reason = "missing-or-invalid-word-time",
                "discarded unusable word timings"
            );
            return Vec::new();
        };
        let starts_word = breaks || raw.starts_with(char::is_whitespace);
        breaks = raw.ends_with(char::is_whitespace);
        match words.last_mut() {
            Some(word) if !starts_word => {
                word.text.push_str(raw.trim_end());
                word.end = word.end.max(end);
            }
            _ => words.push(TimedWord {
                text: raw.trim().to_owned(),
                start,
                end: end.max(start),
            }),
        }
    }
    for word in &mut words {
        word.start = hundredths(word.start);
        word.end = hundredths(word.end).max(word.start);
    }
    words
}

/// `[_BEG_]`, `[_TT_150]`, `<|endoftext|>`, `<|sv|>`: tokens that are not
/// speech.
fn is_special(token: &str) -> bool {
    (token.starts_with("[_") && token.ends_with(']'))
        || (token.starts_with("<|") && token.ends_with("|>"))
}

/// A token's time, if it has a real one.
fn token_time(token: &serde_json::Value, key: &str) -> Option<f64> {
    token
        .get(key)
        .and_then(serde_json::Value::as_f64)
        .filter(|seconds| seconds.is_finite() && *seconds >= 0.0)
}

/// `seconds` to the nearest hundredth.
pub(crate) fn hundredths(seconds: f64) -> f64 {
    (seconds * 100.0).round() / 100.0
}

/// Whether `words` spell `text`, spaces aside. True for no words.
fn spells(words: &[TimedWord], text: &str) -> bool {
    words.is_empty()
        || kw_transcript_text::words_match_text(words.iter().map(|word| word.text.as_str()), text)
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
    /// Where to look for the speech first. See [`Self::speech_first`].
    speech: Option<Arc<dyn FindSpeech>>,
}

impl WhisperServer {
    #[must_use]
    pub const fn new(policy: NetworkPolicy, port: u16, model: Option<String>) -> Self {
        Self {
            policy,
            port,
            model,
            timed: false,
            speech: None,
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
    /// otherwise. Dictation wants one line of text. Media import and the
    /// meeting's whole-recording pass want cues -- subtitles for the one,
    /// speaker labels and timed rows for the other -- and pay for them. The
    /// measurement is in the module doc.
    ///
    /// This doc said "dictation and meetings want one line of text" until
    /// 2026-09-26, and the meeting pass was built bare to match. It had needed
    /// timings since speaker labels landed on 2026-09-01, and on a whisper
    /// engine they never attached (KNEG-326).
    #[must_use]
    pub const fn timed(mut self) -> Self {
        self.timed = true;
        self
    }

    /// Whether requests will ask for per-utterance timings.
    ///
    /// Read-only, for a caller's test to pin what it asked for. The meeting
    /// pass needs it: nothing in a running application can be asked whether it
    /// sent `verbose_json`, and building the server bare produced a note that
    /// said the pass "returned no timings" for every meeting on a whisper
    /// engine before anyone noticed.
    #[must_use]
    pub const fn asks_for_timings(&self) -> bool {
        self.timed
    }

    #[must_use]
    pub const fn port(&self) -> u16 {
        self.port
    }

    /// Find the speech first, and transcribe only that, in chunks (KNEG-404).
    ///
    /// For long recordings: media import and the meeting's whole-recording
    /// pass. A builder for the reason [`Self::timed`] is one -- it is a
    /// property of what the caller is transcribing. Never for dictation, where
    /// a pause-heavy sentence stripped to nothing leaves whisper the
    /// vocabulary prompt to repeat back (`kw_audio_core::vad`). The design and
    /// its measurements are in [`crate::speech`].
    #[must_use]
    pub fn speech_first(mut self, finder: Arc<dyn FindSpeech>) -> Self {
        self.speech = Some(finder);
        self
    }

    /// Whether long recordings will have their speech found first.
    ///
    /// Read-only, for a caller's test to pin what it built, as
    /// [`Self::asks_for_timings`] is.
    #[must_use]
    pub const fn finds_speech_first(&self) -> bool {
        self.speech.is_some()
    }

    /// One `/inference` request for `wav`, as every path sends it.
    async fn request(
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

    /// The whole recording in one request, as every recording went before
    /// [`Self::speech_first`], and as one still goes when its speech cannot
    /// be found first.
    async fn whole(
        &self,
        wav: Vec<u8>,
        choice: Choice<'_>,
        prompt: Option<&str>,
    ) -> Result<Inference, String> {
        match choice {
            Choice::One(language) => self.request(wav, language, prompt).await,
            Choice::Between(languages) => {
                let body = self.language_probabilities(wav.clone()).await?;
                let language = selected_language(&body, languages)?;
                self.request(wav, Some(&language), prompt).await
            }
        }
    }

    /// Find the speech, detect the language once, and send each chunk.
    /// See [`crate::speech`].
    async fn speech_chunks(
        &self,
        finder: &dyn FindSpeech,
        wav: Vec<u8>,
        choice: Choice<'_>,
        prompt: Option<&str>,
    ) -> Result<Inference, String> {
        let Some(audio) = speech::Pcm::parse(&wav) else {
            tracing::warn!("the recording is not 16 kHz mono PCM; transcribing it whole");
            return self.whole(wav, choice, prompt).await;
        };
        let started = std::time::Instant::now();
        let spans = match finder.find(&wav).await {
            Ok(spans) => spans,
            Err(detail) => {
                tracing::warn!(%detail, "could not find the speech first; transcribing the recording whole");
                return self.whole(wav, choice, prompt).await;
            }
        };
        let chunks = speech::plan(&spans, audio.duration());
        tracing::info!(
            seconds = audio.duration(),
            speech = spans.len(),
            chunks = chunks.len(),
            search_ms = started.elapsed().as_millis(),
            "found the speech first"
        );
        if chunks.is_empty() {
            return Ok(Inference::Empty);
        }

        let sample = || audio.join(&speech::detection_sample(&chunks));
        let language = match choice {
            Choice::One(Some(language)) => Some(language.to_owned()),
            Choice::One(None) => {
                match self
                    .language_probabilities(sample())
                    .await
                    .and_then(|body| top_language(&body))
                {
                    Ok(language) => Some(language),
                    Err(detail) => {
                        tracing::warn!(%detail, "could not detect the language once; each chunk will detect its own");
                        None
                    }
                }
            }
            Choice::Between(languages) => {
                let body = self.language_probabilities(sample()).await?;
                Some(selected_language(&body, languages)?)
            }
        };

        let mut parts = Vec::with_capacity(chunks.len());
        for chunk in chunks {
            let answer = self
                .request(audio.slice(chunk), language.as_deref(), prompt)
                .await?;
            if matches!(answer, Inference::Rejected { .. }) {
                return Ok(answer);
            }
            parts.push((chunk, answer));
        }
        Ok(speech::stitch(parts))
    }

    /// Ask the server only which language `wav` is in.
    async fn language_probabilities(&self, wav: Vec<u8>) -> Result<serde_json::Value, String> {
        let part = reqwest::multipart::Part::bytes(wav)
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
        response.json().await.map_err(|e| e.to_string())
    }
}

/// Which language a recording is sent in.
#[derive(Debug, Clone, Copy)]
enum Choice<'a> {
    /// This one, or `None` for whichever whisper hears.
    One(Option<&'a str>),
    /// Whichever of the two dictation languages it sounds more like.
    Between(&'a [String]),
}

#[async_trait::async_trait]
impl LocalTranscriber for WhisperServer {
    async fn transcribe(
        &self,
        wav: Vec<u8>,
        language: Option<&str>,
        prompt: Option<&str>,
    ) -> Result<Inference, String> {
        match &self.speech {
            Some(finder) => {
                self.speech_chunks(finder.as_ref(), wav, Choice::One(language), prompt)
                    .await
            }
            None => self.request(wav, language, prompt).await,
        }
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
        let choice = if languages.len() == 2 {
            Choice::Between(languages)
        } else {
            Choice::One(languages.first().map(String::as_str))
        };
        match &self.speech {
            Some(finder) => {
                self.speech_chunks(finder.as_ref(), wav, choice, prompt)
                    .await
            }
            None => self.whole(wav, choice, prompt).await,
        }
    }

    fn model_name(&self) -> Option<String> {
        self.model.clone()
    }
}

/// The likeliest language of all, from a detection answer.
fn top_language(body: &serde_json::Value) -> Result<String, String> {
    body.get("language_probabilities")
        .and_then(serde_json::Value::as_object)
        .into_iter()
        .flatten()
        .filter_map(|(language, probability)| {
            let probability = probability.as_f64()?;
            (probability.is_finite() && probability > 0.0).then_some((language, probability))
        })
        .max_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(language, _)| language.clone())
        .ok_or_else(|| "the server returned no language probabilities".to_owned())
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
    /// 2026-08-31 against `ggml-tiny`. Trimmed to the fields this code read
    /// then; the real response also carries `tokens`, `words` (read since
    /// KNEG-408, see `WORDS_BODY`), `temperature`, `avg_logprob` and
    /// `no_speech_prob` per segment.
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

    /* --------------------------------------------- words (KNEG-408) */

    /// `merge_tokens` over a JSON list of tokens, as the server sends them.
    fn merged(tokens: &str) -> Vec<(String, f64, f64)> {
        let tokens: Vec<serde_json::Value> = serde_json::from_str(tokens).unwrap();
        merge_tokens(&tokens)
            .into_iter()
            .map(|word| (word.text, word.start, word.end))
            .collect()
    }

    fn word(text: &str, start: f64, end: f64) -> (String, f64, f64) {
        (text.to_owned(), start, end)
    }

    #[test]
    fn a_token_without_a_leading_space_is_part_of_the_word_before_it() {
        // Whisper's tokens are not words: " Stock" and "holm" are one.
        assert_eq!(
            merged(
                r#"[{"word":" i","start":0.5,"end":0.62},
                    {"word":" Stock","start":0.62,"end":0.9},
                    {"word":"holm","start":0.9,"end":1.24}]"#
            ),
            [word("i", 0.5, 0.62), word("Stockholm", 0.62, 1.24)]
        );
    }

    #[test]
    fn punctuation_joins_the_word_it_follows() {
        assert_eq!(
            merged(
                r#"[{"word":" Hej","start":0.0,"end":0.4},
                    {"word":",","start":0.4,"end":0.4},
                    {"word":" då","start":0.5,"end":0.8},
                    {"word":".","start":0.8,"end":0.82}]"#
            ),
            [word("Hej,", 0.0, 0.4), word("då.", 0.5, 0.82)]
        );
    }

    #[test]
    fn special_tokens_and_tokens_with_no_text_are_dropped() {
        // whisper.cpp's server leaves out every token from end-of-text on,
        // which is all of these; they are dropped anyway, so a server that
        // sent one would not put "[_BEG_]" in a note.
        assert_eq!(
            merged(
                r#"[{"word":"[_BEG_]","start":0.0,"end":0.0},
                    {"word":"<|sv|>","start":0.0,"end":0.0},
                    {"word":" Ja","start":0.1,"end":0.3},
                    {"word":"","start":0.3,"end":0.3},
                    {"start":0.3,"end":0.3},
                    {"word":"[_TT_150]"},
                    {"word":"<|endoftext|>","start":1.5,"end":1.5}]"#
            ),
            [word("Ja", 0.1, 0.3)]
        );
    }

    #[test]
    fn a_token_without_a_time_leaves_the_segment_with_no_words() {
        // All or nothing, for the reason `timed_segments` gives: words timed
        // for some of the row and not the rest would be highlighted wrongly.
        for untimed in [
            r#"{"word":" b"}"#,
            r#"{"word":" b","start":1.0}"#,
            r#"{"word":" b","start":1.0,"end":null}"#,
            r#"{"word":" b","start":-0.01,"end":-0.01}"#,
            r#"{"word":" b","start":"1.0","end":1.2}"#,
        ] {
            let tokens = format!(r#"[{{"word":" a","start":0.0,"end":0.5}},{untimed}]"#);
            assert!(merged(&tokens).is_empty(), "{untimed}");
        }
    }

    #[test]
    fn word_times_are_rounded_to_a_hundredth() {
        // The server multiplies centiseconds by 0.01, which is not exact in
        // binary; a hundredth is all the resolution there is.
        assert_eq!(
            merged(
                r#"[{"word":" x","start":0.30000000000000004,"end":1.2349},
                    {"word":" y","start":1.2351,"end":1.239}]"#
            ),
            [word("x", 0.3, 1.23), word("y", 1.24, 1.24)]
        );
    }

    /// `verbose_json` as whisper.cpp's server writes a segment's tokens,
    /// trimmed to the fields read and `probability`: the shape read in its
    /// source at b4938, not captured from a run.
    const WORDS_BODY: &str = r#"{
      "text":" Hej från Stockholm.\n Ja.\n",
      "segments":[
        {"id":0,"text":" Hej från Stockholm.","start":0.0,"end":2.0,"words":[
          {"word":" Hej","start":0.12,"end":0.48,"t_dtw":-1,"probability":0.98},
          {"word":" från","start":0.48,"end":0.8,"t_dtw":-1,"probability":0.97},
          {"word":" Stock","start":0.9,"end":1.3,"t_dtw":-1,"probability":0.91},
          {"word":"holm","start":1.3,"end":1.7,"t_dtw":-1,"probability":0.99},
          {"word":".","start":1.7,"end":1.71,"t_dtw":-1,"probability":0.8}]},
        {"id":1,"text":" Ja.","start":2.5,"end":3.0,"words":[
          {"word":" Ja","start":2.5,"probability":0.9},
          {"word":".","start":2.9,"end":3.0,"probability":0.9}]}
      ]}"#;

    #[test]
    fn a_timed_response_keeps_each_segments_words() {
        let Inference::Timed { segments, .. } = interpret(200, WORDS_BODY) else {
            panic!("expected timed segments");
        };
        let words: Vec<_> = segments[0]
            .words
            .iter()
            .map(|w| (w.text.clone(), w.start, w.end))
            .collect();
        assert_eq!(
            words,
            [
                word("Hej", 0.12, 0.48),
                word("från", 0.48, 0.8),
                word("Stockholm.", 0.9, 1.71)
            ]
        );
        // The second segment's first token came back without an end: the
        // segment keeps its row and loses only its words.
        assert_eq!(segments[1].text, "Ja.");
        assert!(segments[1].words.is_empty());
    }

    #[test]
    fn words_that_do_not_spell_their_segment_are_not_kept() {
        // A token can be half a character -- the server replaces a split
        // UTF-8 sequence with U+FFFD -- and words that no longer spell their
        // row's text could never be shown against it.
        let body = r#"{"text":" på","segments":[{"text":" på","start":0.0,"end":1.0,"words":[
            {"word":" p","start":0.0,"end":0.3},
            {"word":"�","start":0.3,"end":0.5},
            {"word":"�","start":0.5,"end":0.6}]}]}"#;
        let Inference::Timed { segments, .. } = interpret(200, body) else {
            panic!("expected timed segments");
        };
        assert_eq!(segments[0].text, "på");
        assert!(segments[0].words.is_empty());
    }

    #[test]
    fn a_segment_without_words_serializes_as_it_did_before() {
        // Sherpa's segments, and whisper's when a word went untimed.
        let bare = TimedSegment {
            text: "Hej.".to_owned(),
            start: 0.5,
            end: 1.5,
            words: Vec::new(),
        };
        assert_eq!(
            serde_json::to_string(&bare).unwrap(),
            r#"{"text":"Hej.","start":0.5,"end":1.5}"#
        );
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
    fn the_server_never_skips_silence_itself() {
        // Its own VAD puts row boundaries up to 30 s from their speech, and
        // started with `--vad` it would apply to dictation too. Silence is
        // skipped by finding the speech first instead (`crate::speech`).
        let args = build_args(&options());
        assert!(!args.iter().any(|a| a.starts_with("--vad")), "{args:?}");
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

    /* ------------------------------------------ finding the speech first */

    use crate::speech::{FindSpeech, Span};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    /// A speech finder that answers the same thing every time, and counts.
    #[derive(Debug)]
    struct Found {
        answer: Result<Vec<Span>, String>,
        calls: AtomicUsize,
    }

    impl Found {
        fn at(spans: &[(f64, f64)]) -> Arc<Self> {
            Arc::new(Self {
                answer: Ok(spans.iter().map(|(s, e)| Span::new(*s, *e)).collect()),
                calls: AtomicUsize::new(0),
            })
        }

        fn failing() -> Arc<Self> {
            Arc::new(Self {
                answer: Err("the speech finder exited with 2".to_owned()),
                calls: AtomicUsize::new(0),
            })
        }

        fn calls(&self) -> usize {
            self.calls.load(Ordering::SeqCst)
        }
    }

    #[async_trait::async_trait]
    impl FindSpeech for Found {
        async fn find(&self, _wav: &[u8]) -> Result<Vec<Span>, String> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.answer.clone()
        }
    }

    /// A loopback server answering `replies` in order, one per connection,
    /// that hands back every request it read.
    async fn answering(
        replies: Vec<(u16, &'static str)>,
    ) -> (u16, tokio::task::JoinHandle<Vec<Vec<u8>>>) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let task = tokio::spawn(async move {
            let mut seen = Vec::new();
            for (status, reply) in replies {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut bytes = Vec::new();
                loop {
                    let mut chunk = [0; 16_384];
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
                seen.push(bytes);
                socket
                    .write_all(
                        format!(
                            "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\n\
                             Content-Length: {}\r\nConnection: close\r\n\r\n{reply}",
                            reply.len()
                        )
                        .as_bytes(),
                    )
                    .await
                    .unwrap();
            }
            seen
        });
        (port, task)
    }

    /// What `answering` read, failing rather than hanging when it was asked
    /// fewer times than it had replies for.
    async fn served(task: tokio::task::JoinHandle<Vec<Vec<u8>>>) -> Vec<Vec<u8>> {
        tokio::time::timeout(std::time::Duration::from_secs(10), task)
            .await
            .expect("the server was asked fewer times than it had replies for")
            .unwrap()
    }

    /// A multipart text field of `request`.
    fn field(request: &[u8], name: &str) -> Option<String> {
        let text = String::from_utf8_lossy(request);
        let marker = format!("name=\"{name}\"\r\n\r\n");
        let (_, rest) = text.split_once(&marker)?;
        Some(rest.split("\r\n").next()?.to_owned())
    }

    /// How long the WAV `request` carries is, from its header.
    fn seconds_sent(request: &[u8]) -> f64 {
        let at = request.windows(4).position(|w| w == b"RIFF").unwrap();
        let size = u32::from_le_bytes(request[at + 40..at + 44].try_into().unwrap());
        f64::from(size) / 32_000.0
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-6
    }

    /// A minute of 16 kHz mono silence; the finder decides where the speech is.
    fn minute() -> Vec<u8> {
        kw_audio_core::wav::encode_wav(&vec![0.0; 60 * 16_000], 16_000)
    }

    fn policy() -> kw_network_policy::NetworkPolicy {
        kw_network_policy::NetworkPolicy::new().unwrap()
    }

    const HEJ: &str = r#"{"text":" Hej.","segments":[{"text":" Hej.","start":0.5,"end":1.5}]}"#;
    const DA: &str =
        r#"{"text":" Hej då.","segments":[{"text":" Hej då.","start":0.2,"end":0.9}]}"#;

    /// Speech at 2-5 s and 30-33 s: two chunks, 1.8-5.2 and 29.8-33.2.
    const TWO_CHUNKS: [(f64, f64); 2] = [(2.0, 5.0), (30.0, 33.0)];

    fn rows(inference: &Inference) -> Vec<(String, f64, f64)> {
        match inference {
            Inference::Timed { segments, .. } => segments
                .iter()
                .map(|s| (s.text.clone(), s.start, s.end))
                .collect(),
            other => panic!("expected timed rows, got {other:?}"),
        }
    }

    #[test]
    fn only_a_server_told_to_finds_the_speech_first() {
        // Dictation builds its server bare: it must never be handed a finder,
        // because a pause-heavy sentence stripped to nothing leaves whisper the
        // vocabulary prompt to repeat (`kw_audio_core::vad`).
        let bare = WhisperServer::new(policy(), 8178, None).timed();
        assert!(!bare.finds_speech_first());
        assert!(bare.speech_first(Found::at(&[])).finds_speech_first());
    }

    #[tokio::test]
    async fn the_language_is_detected_once_on_the_speech_and_every_chunk_is_sent_with_it() {
        let (port, server) = answering(vec![
            (200, r#"{"language_probabilities":{"sv":0.9,"en":0.1}}"#),
            (200, HEJ),
            (200, DA),
        ])
        .await;
        let found = Found::at(&TWO_CHUNKS);
        let engine = WhisperServer::new(policy(), port, None)
            .timed()
            .speech_first(found.clone());

        let result = engine
            .transcribe(minute(), None, Some("Dyve"))
            .await
            .unwrap();
        let requests = served(server).await;

        assert_eq!(found.calls(), 1);
        assert_eq!(requests.len(), 3);
        // Detection hears the two chunks' audio and nothing else, and no
        // vocabulary prompt.
        assert_eq!(
            field(&requests[0], "detect_language").as_deref(),
            Some("true")
        );
        assert!(field(&requests[0], "prompt").is_none());
        assert!(close(seconds_sent(&requests[0]), 6.8));
        for request in &requests[1..] {
            assert_eq!(field(request, "language").as_deref(), Some("sv"));
            assert_eq!(field(request, "prompt").as_deref(), Some("Dyve"));
            assert_eq!(
                field(request, "response_format").as_deref(),
                Some("verbose_json")
            );
            assert!(field(request, "detect_language").is_none());
            assert!(
                close(seconds_sent(request), 3.4),
                "each chunk is its own audio"
            );
        }

        let rows = rows(&result);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].0, "Hej.");
        assert!(close(rows[0].1, 2.3) && close(rows[0].2, 3.3), "{rows:?}");
        assert_eq!(rows[1].0, "Hej då.");
        assert!(close(rows[1].1, 30.0) && close(rows[1].2, 30.7), "{rows:?}");
        assert!(matches!(&result, Inference::Timed { text, .. } if text == "Hej. Hej då."));
    }

    #[tokio::test]
    async fn a_chosen_language_is_not_detected_again() {
        let (port, server) = answering(vec![(200, HEJ), (200, DA)]).await;
        let engine = WhisperServer::new(policy(), port, None)
            .timed()
            .speech_first(Found::at(&TWO_CHUNKS));

        engine.transcribe(minute(), Some("en"), None).await.unwrap();
        let requests = served(server).await;

        assert_eq!(requests.len(), 2);
        for request in &requests {
            assert_eq!(field(request, "language").as_deref(), Some("en"));
            assert!(field(request, "detect_language").is_none());
        }
    }

    #[tokio::test]
    async fn two_languages_are_chosen_between_once() {
        let (port, server) = answering(vec![
            (
                200,
                r#"{"language_probabilities":{"de":0.7,"en":0.2,"sv":0.1}}"#,
            ),
            (200, HEJ),
            (200, DA),
        ])
        .await;
        let engine = WhisperServer::new(policy(), port, None)
            .timed()
            .speech_first(Found::at(&TWO_CHUNKS));

        engine
            .transcribe_languages(minute(), &["sv".into(), "en".into()], None)
            .await
            .unwrap();
        let requests = served(server).await;

        assert_eq!(requests.len(), 3);
        assert_eq!(
            field(&requests[0], "detect_language").as_deref(),
            Some("true")
        );
        assert_eq!(field(&requests[1], "language").as_deref(), Some("en"));
        assert_eq!(field(&requests[2], "language").as_deref(), Some("en"));
    }

    #[tokio::test]
    async fn a_language_that_cannot_be_detected_leaves_each_chunk_to_detect_its_own() {
        let (port, server) = answering(vec![(200, r#"{"text":""}"#), (200, HEJ), (200, DA)]).await;
        let engine = WhisperServer::new(policy(), port, None)
            .timed()
            .speech_first(Found::at(&TWO_CHUNKS));

        let result = engine.transcribe(minute(), None, None).await.unwrap();
        let requests = served(server).await;

        assert_eq!(field(&requests[1], "language").as_deref(), Some("auto"));
        assert_eq!(field(&requests[2], "language").as_deref(), Some("auto"));
        assert_eq!(rows(&result).len(), 2);
    }

    #[tokio::test]
    async fn a_recording_with_no_speech_asks_whisper_nothing() {
        // Nothing is listening once `answering` has served its empty list, so
        // any request would fail rather than come back empty.
        let (port, server) = answering(Vec::new()).await;
        served(server).await;
        let engine = WhisperServer::new(policy(), port, None)
            .timed()
            .speech_first(Found::at(&[]));

        assert_eq!(
            engine
                .transcribe(minute(), None, Some("Dyve"))
                .await
                .unwrap(),
            Inference::Empty
        );
    }

    #[tokio::test]
    async fn when_the_speech_cannot_be_found_the_recording_goes_whole_as_before() {
        let (port, server) = answering(vec![(200, HEJ)]).await;
        let found = Found::failing();
        let engine = WhisperServer::new(policy(), port, None)
            .timed()
            .speech_first(found.clone());

        let result = engine
            .transcribe(minute(), None, Some("Dyve"))
            .await
            .unwrap();
        let requests = served(server).await;

        assert_eq!(found.calls(), 1);
        assert_eq!(requests.len(), 1);
        assert!(close(seconds_sent(&requests[0]), 60.0));
        assert_eq!(field(&requests[0], "language").as_deref(), Some("auto"));
        assert_eq!(field(&requests[0], "prompt").as_deref(), Some("Dyve"));
        assert_eq!(rows(&result), vec![("Hej.".to_owned(), 0.5, 1.5)]);
    }

    #[tokio::test]
    async fn a_recording_that_is_not_16k_mono_pcm_goes_whole_without_looking() {
        let (port, server) = answering(vec![(200, HEJ)]).await;
        let found = Found::at(&TWO_CHUNKS);
        let engine = WhisperServer::new(policy(), port, None)
            .timed()
            .speech_first(found.clone());
        let stereo = kw_audio_core::wav::wrap_pcm(&[0; 64_000], 16_000, 2);

        engine.transcribe(stereo, None, None).await.unwrap();

        assert_eq!(found.calls(), 0);
        assert_eq!(served(server).await.len(), 1);
    }

    #[tokio::test]
    async fn a_refused_chunk_ends_the_transcription_with_the_refusal() {
        // Nothing listens after the one reply, so a further chunk would fail
        // the call instead of returning the refusal.
        let (port, server) = answering(vec![(500, "failed to process audio")]).await;
        let engine = WhisperServer::new(policy(), port, None)
            .timed()
            .speech_first(Found::at(&TWO_CHUNKS));

        let result = engine.transcribe(minute(), Some("sv"), None).await.unwrap();

        assert_eq!(served(server).await.len(), 1);
        assert!(
            matches!(result, Inference::Rejected { status: 500, .. }),
            "{result:?}"
        );
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
