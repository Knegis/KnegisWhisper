//! Local transcription against `sherpa-onnx-offline`.
//!
//! A second [`LocalTranscriber`], alongside [`crate::local::WhisperServer`].
//! `route::Transport::Local` already says engine selection stays with the
//! local manager, so this needs no new `Transport` variant -- it is a second
//! implementation slotted into the same seam.
//!
//! The shape here is deliberately not a copy of `WhisperServer`'s: sherpa-onnx
//! ships `sherpa-onnx-offline` as a one-shot CLI that reads one WAV file and
//! prints one transcript, not an HTTP server on a port. So there is no
//! `ServerOptions`, no port, and no persistent process -- [`SherpaOffline`]
//! spawns a fresh child for every [`LocalTranscriber::transcribe`] call.
//!
//! ## What is measured here, not guessed
//!
//! `docs/BUILD_PLAN.md`'s chunk B4 recorded that an extracted sherpa model is
//! a directory of encoder/decoder/joiner/tokens files whose layout differs
//! between the Parakeet and Nemotron families, and that the CLI flags follow
//! the layout -- something that "cannot be written correctly from a release
//! page". [`discover_transducer_files`] and [`build_args`] are the answer,
//! and both were checked against the real thing with
//! `examples/sherpa_probe.rs`, run against both pinned Parakeet archives
//! (`parakeet-unified-en-0.6b` and `parakeet-tdt-0.6b-v3`):
//!
//! - Both extract to a flat directory holding `encoder.int8.onnx`,
//!   `decoder.int8.onnx`, `joiner.int8.onnx` and `tokens.txt` at the top
//!   level, plus a `test_wavs/` subdirectory of sample audio that is not part
//!   of the model.
//! - `sherpa-onnx-offline --help`'s own "(1) Transducer from icefall" usage
//!   block names exactly `--tokens`, `--encoder`, `--decoder`, `--joiner` and
//!   `--num-threads`, followed by one or more WAV paths -- which is the
//!   invocation both probe runs actually used.
//! - A clean run prints one JSON object to stdout per input file, of the
//!   shape `{"text": "...", ...}` among other fields this build does not use.
//!
//! Both runs transcribed `tests/fixtures/speech-16k-mono.wav` -- the same
//! fixture `tests/local_dictation.rs` uses for whisper -- to "The quick brown
//! fox jumps over the lazy dog. This is a test of local transcription.",
//! matching whisper's own transcript of the same file up to capitalisation.
//! See `docs/PLATFORM_SUPPORT.md` for the full record.
//!
//! The two Nemotron models are streaming, over sherpa's own WebSocket
//! protocol rather than this offline CLI, and were not exercised by any of
//! this -- they stay unreached by this module on purpose.

use std::path::{Path, PathBuf};
use std::process::Stdio;

use serde::Deserialize;

use crate::local::{Inference, LocalTranscriber, TimedSegment};

/// The four files sherpa-onnx-offline's transducer model type needs, resolved
/// to real paths inside one extracted model directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransducerFiles {
    pub encoder: PathBuf,
    pub decoder: PathBuf,
    pub joiner: PathBuf,
    pub tokens: PathBuf,
}

/// Why a model directory did not match the measured transducer layout.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LayoutError {
    #[error(
        "{dir}: no {what} file was found among its top-level files. Either the model was not \
         fully extracted, or its layout is not the Parakeet transducer shape \
         (encoder/decoder/joiner/tokens) this build knows how to drive."
    )]
    Missing { dir: PathBuf, what: &'static str },

    #[error(
        "{dir}: more than one file could be the {what} ({found:?}). Discovery expects exactly \
         one and refuses to guess which is right."
    )]
    Ambiguous {
        dir: PathBuf,
        what: &'static str,
        found: Vec<PathBuf>,
    },

    #[error("could not read {dir}: {detail}")]
    Unreadable { dir: PathBuf, detail: String },
}

/// Find the measured Parakeet transducer files directly inside `dir`.
///
/// Not recursive: the one subdirectory the pinned archives ship,
/// `test_wavs/`, holds sample audio rather than model files, and a recursive
/// search would risk mistaking a sample for the model.
pub fn discover_transducer_files(dir: &Path) -> Result<TransducerFiles, LayoutError> {
    Ok(TransducerFiles {
        encoder: find_one(dir, "encoder", |name| {
            name.contains("encoder") && name.ends_with(".onnx")
        })?,
        decoder: find_one(dir, "decoder", |name| {
            name.contains("decoder") && name.ends_with(".onnx")
        })?,
        joiner: find_one(dir, "joiner", |name| {
            name.contains("joiner") && name.ends_with(".onnx")
        })?,
        tokens: find_one(dir, "tokens.txt", |name| name == "tokens.txt")?,
    })
}

fn find_one(
    dir: &Path,
    what: &'static str,
    is_match: impl Fn(&str) -> bool,
) -> Result<PathBuf, LayoutError> {
    let entries = std::fs::read_dir(dir).map_err(|err| LayoutError::Unreadable {
        dir: dir.to_path_buf(),
        detail: err.to_string(),
    })?;

    let mut found = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Some(name) = path.file_name().and_then(std::ffi::OsStr::to_str) else {
            continue;
        };
        if is_match(&name.to_lowercase()) {
            found.push(path);
        }
    }

    match found.len() {
        0 => Err(LayoutError::Missing {
            dir: dir.to_path_buf(),
            what,
        }),
        1 => Ok(found.remove(0)),
        _ => Err(LayoutError::Ambiguous {
            dir: dir.to_path_buf(),
            what,
            found,
        }),
    }
}

/// The command-line arguments for one transcription request.
///
/// Built fresh per request rather than once at start-up: sherpa-onnx-offline
/// is a one-shot CLI, so there is no persistent process to hold options for
/// the way `local::build_args` holds them for a `whisper-server` that starts
/// once and serves many requests. Measured against the real `--help` text and
/// two real model directories -- see the module comment.
#[must_use]
pub fn build_args(
    files: &TransducerFiles,
    wav_path: &Path,
    num_threads: Option<u32>,
) -> Vec<String> {
    let mut args = vec![
        format!("--tokens={}", files.tokens.display()),
        format!("--encoder={}", files.encoder.display()),
        format!("--decoder={}", files.decoder.display()),
        format!("--joiner={}", files.joiner.display()),
    ];

    // Omitted rather than defaulted, the same reasoning as
    // `local::build_args`'s `threads` field: sherpa-onnx-offline's own
    // default (2) is a choice about the machine that this build is not
    // better placed to make than the CLI itself.
    if let Some(threads) = num_threads {
        args.push(format!("--num-threads={threads}"));
    }

    args.push(wav_path.display().to_string());
    args
}

/// What one line of sherpa-onnx-offline's stdout carries.
///
/// Measured on 2026-08-31 against `parakeet-unified-en-0.6b-int8`, 13.5 s of
/// speech, the same audio the whisper timing measurement used:
///
/// ```text
/// {"lang": "", "emotion": "", "event": "",
///  "text": "The quick brown fox jumps over the lazy dog. Second sentence, ...",
///  "timestamps": [0.64, 0.72, 0.72, 0.88, ...],           // 64 entries
///  "durations": [],
///  "tokens": [" The", " qu", "ick", " br", "own", ...],   // 64 entries
///  "ys_log_probs": [...],                                 // 64 entries
///  "words": []}
/// ```
///
/// Three things that settles, none of which the field names alone said:
///
/// * **`timestamps` is per token, not per segment** -- 64 of them for 64
///   tokens, and the tokens are subword pieces (`" qu"`, `"ick"`). There is no
///   segmentation in this output at all.
/// * **`words` and `durations` come back empty** for a transducer model.
///   `--whisper-enable-segment-timestamps` and
///   `--whisper-enable-token-timestamps` exist but are Whisper-family options;
///   Parakeet is a transducer and neither applies. So there is no flag that
///   fills them in and no offsets to be had -- only onsets.
/// * `ys_log_probs` is per token as well. Unread: nothing here reports
///   confidence, and a threshold nobody measured would be worse than silence.
#[derive(Debug, Deserialize)]
struct OfflineResult {
    text: Option<String>,
    /// One onset per token, in seconds from the start of the recording.
    #[serde(default)]
    timestamps: Vec<f64>,
    /// Subword pieces. A leading space marks the start of a word.
    #[serde(default)]
    tokens: Vec<String>,
}

/// The longest cue [`cues`] will build before splitting, in characters.
///
/// A backstop, not the rule that does the work -- sentence-final punctuation is.
/// It exists because a transducer has no decode window to fall back on the way
/// whisper does: whisper cannot emit a segment longer than 30 seconds of audio
/// whatever we ask for, and `sherpa-onnx-offline` would happily return a
/// 57-minute recording with no punctuation in it as one cue.
///
/// **A judgement, not a measurement.** 200 characters is generous for one
/// spoken sentence and short enough that a run of unpunctuated speech still
/// produces readable cues. Nothing was measured to choose it, and the split it
/// causes still lands on a real token onset and a word boundary, so being
/// wrong about the number costs readability and never correctness.
const MAX_CUE_CHARS: usize = 200;

/// How much of stderr is worth repeating in a refusal.
const STDERR_LIMIT: usize = 400;

/// Interpret a finished `sherpa-onnx-offline` run.
///
/// Pure, so every shape is covered without a process. Mirrors
/// `local::interpret`'s rule: a clean exit with no usable `text` field is a
/// failure, not an empty transcript -- the difference matters when someone is
/// trying to work out why dictation produced nothing.
#[must_use]
pub fn interpret(exit_code: Option<i32>, stdout: &[u8], stderr: &str) -> Inference {
    let Some(code) = exit_code else {
        // Killed by a signal: there is no exit code to report. `status` on
        // `Inference::Rejected` normally carries an HTTP status; reused here
        // as a process-exit signal, with 1 standing in for "abnormal".
        return Inference::Rejected {
            status: 1,
            body: truncate(stderr),
        };
    };

    if code != 0 {
        return Inference::Rejected {
            status: u16::try_from(code).unwrap_or(1),
            body: format!(
                "Parakeet exited with code {code} (0x{:08X}). {}",
                u32::from_ne_bytes(code.to_ne_bytes()),
                stderr_tail(stderr)
            ),
        };
    }

    let parsed = std::str::from_utf8(stdout)
        .ok()
        .and_then(|s| serde_json::from_str::<OfflineResult>(s.trim()).ok());

    match parsed {
        Some(parsed) => {
            let Some(text) = parsed.text else {
                return Inference::Rejected {
                    status: 0,
                    body: truncate(stderr),
                };
            };
            let trimmed = text.trim();
            if trimmed.is_empty() {
                return Inference::Empty;
            }
            let segments = cues(&parsed.tokens, &parsed.timestamps);
            if segments.is_empty() {
                Inference::Text(trimmed.to_owned())
            } else {
                Inference::Timed {
                    text: trimmed.to_owned(),
                    segments,
                }
            }
        }
        // A clean exit that did not print the shape this build expects. Real
        // examples: a `--help`-shaped invocation, or a future sherpa-onnx
        // release that changes its stdout contract.
        None => Inference::Rejected {
            status: 0,
            body: truncate(stderr),
        },
    }
}

/// Group per-token onsets into subtitle cues.
///
/// `sherpa-onnx-offline` reports no segmentation, so this builds it -- which is
/// the same job whisper.cpp's *server* does for us from its own decoded
/// timestamp tokens, not a capability being invented. What matters is the rule
/// it follows:
///
/// **Every boundary is a measured token onset.** A cue starts at the onset of
/// its first token and ends at the onset of its last. Nothing is interpolated,
/// averaged, or derived from the audio length.
///
/// **A cue ends at its last *spoken* token, and a trailing punctuation mark is
/// not one.** That is not tidiness; it is what the measurement forced. This
/// model timestamps a sentence-final `"."` at the end of the pause that follows
/// it rather than where the speech stopped:
///
/// ```text
/// token   'y'   ' do'  'g'    '.'    ' Se'  'cond'
/// onset   2.32  2.64   2.72   4.08   4.24   4.32
/// ```
///
/// The `"."` sits 1.36 s after `"g"` and 0.16 s before the next word, because
/// the decoder only emits it once it has seen the silence. Ending a cue on it
/// would hold every cue on screen through the whole pause -- 1.2 s of dead air
/// per cue, measured. The last token carrying a letter or digit is the last
/// thing actually said, so that is the end.
///
/// The cost of the remaining honesty, stated rather than hidden: a cue ends
/// slightly **early**, by the length of that final spoken token, because an
/// onset is not an offset. `durations` comes back empty for a transducer model
/// (see [`OfflineResult`]), so a token's extent is not something this output
/// reports and there is no better end to be had. Against the same audio,
/// whisper's own segmenter said `0.00-3.00 / 3.68-9.44 / 10.08-12.88` and this
/// rule gives `0.64-2.72 / 4.24-9.12 / 10.56-12.40`: the same three utterances,
/// each a fraction of a second tighter.
///
/// Splits happen after sentence-final punctuation, or when [`MAX_CUE_CHARS`]
/// would be exceeded, and **only ever at the start of a word** -- a token with
/// no leading space continues the current one. That is the same protection
/// `--split-on-word` gives whisper, except here it is unconditional rather than
/// a flag, because breaking `"ick"` off `" qu"` could never be wanted.
///
/// Returns empty -- meaning *no timings were reported* -- unless there is
/// exactly one onset per token and at least one of each. All-or-nothing, the
/// rule `crate::local::interpret` follows for the same reason: a partly timed
/// transcript would place some cues correctly and the rest by fabrication.
#[must_use]
pub fn cues(tokens: &[String], timestamps: &[f64]) -> Vec<TimedSegment> {
    if tokens.is_empty() || tokens.len() != timestamps.len() {
        return Vec::new();
    }

    let mut out: Vec<TimedSegment> = Vec::new();
    // The cue being accumulated: its text, its start, and the onset of the last
    // *spoken* token put into it -- which becomes its end. See the doc comment
    // on why a trailing punctuation token's onset is not an end.
    let mut text = String::new();
    let mut start = 0.0_f64;
    let mut last = 0.0_f64;
    // Set once a cue has reached a point it may be closed at. Acted on at the
    // next word start rather than immediately, so `"."` stays with the word it
    // punctuates instead of opening the following cue.
    let mut ready = false;

    for (token, &onset) in tokens.iter().zip(timestamps) {
        let starts_word = token.starts_with(' ');
        let ends_sentence = token
            .trim_end()
            .chars()
            .next_back()
            .is_some_and(|c| matches!(c, '.' | '?' | '!' | '。' | '？' | '！'));

        if starts_word && ready && !text.trim().is_empty() {
            out.push(TimedSegment {
                text: text.trim().to_owned(),
                start,
                // Never before the start: a cue whose only token carried no
                // letter or digit would otherwise report the previous cue's
                // end. Not reachable from any measured output -- every cue here
                // begins with a word -- but the clamp costs nothing and the
                // alternative is a cue that runs backwards.
                end: last.max(start),
            });
            text.clear();
            ready = false;
        }

        if text.is_empty() {
            start = onset;
        }
        text.push_str(token);
        if token.chars().any(char::is_alphanumeric) {
            last = onset;
        }

        // The cap is checked against what the cue already holds, so the token
        // that crosses it opens the next cue rather than being stranded.
        ready = ready || ends_sentence || text.chars().count() >= MAX_CUE_CHARS;
    }

    if !text.trim().is_empty() {
        out.push(TimedSegment {
            text: text.trim().to_owned(),
            start,
            end: last.max(start),
        });
    }

    out
}

fn stderr_tail(stderr: &str) -> String {
    let count = stderr.chars().count();
    stderr
        .chars()
        .skip(count.saturating_sub(STDERR_LIMIT))
        .collect()
}

fn truncate(s: &str) -> String {
    if s.chars().count() <= STDERR_LIMIT {
        return s.to_owned();
    }
    let cut = s
        .char_indices()
        .nth(STDERR_LIMIT)
        .map_or(s.len(), |(index, _)| index);
    let mut out = s.get(..cut).unwrap_or(s).to_owned();
    out.push('…');
    out
}

/// A unique path for one request's input WAV, inside `dir`.
///
/// `sherpa-onnx-offline` takes a file path, not stdin, so every request needs
/// one. Named uniquely rather than reused so two overlapping calls (a retry
/// racing a cancellation, say) cannot clobber each other's input.
fn unique_wav_path(dir: &Path) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    dir.join(format!("sherpa-input-{}-{nanos}.wav", std::process::id()))
}

/// A Parakeet transducer model driven through `sherpa-onnx-offline`.
///
/// Unlike [`crate::local::WhisperServer`], there is no running server to hold
/// a port for: each [`LocalTranscriber::transcribe`] call writes the audio to
/// a temporary file, spawns the CLI against it, and reads back one line of
/// JSON.
#[derive(Debug, Clone)]
pub struct SherpaOffline {
    executable: PathBuf,
    files: TransducerFiles,
    model_name: Option<String>,
    /// Where one request's WAV is staged before the CLI reads it.
    ///
    /// Supplied by the caller rather than resolved here: this crate does not
    /// depend on `kw_storage`, and the caller already knows where the
    /// ASCII-safe scratch directory the native binaries need is (see
    /// `kw_storage::paths::audio_temp_dir`).
    temp_dir: PathBuf,
    num_threads: Option<u32>,
}

impl SherpaOffline {
    #[must_use]
    pub const fn new(
        executable: PathBuf,
        files: TransducerFiles,
        model_name: Option<String>,
        temp_dir: PathBuf,
        num_threads: Option<u32>,
    ) -> Self {
        Self {
            executable,
            files,
            model_name,
            temp_dir,
            num_threads,
        }
    }
}

#[async_trait::async_trait]
impl LocalTranscriber for SherpaOffline {
    async fn transcribe(
        &self,
        wav: Vec<u8>,
        // Measured: the transducer invocation this was proven against takes
        // neither a language nor a prompt flag -- `--whisper-language` is a
        // Whisper-family option this model does not use, and hotwords
        // (`--hotwords-file`) is a different mechanism from a free-text
        // prompt that was not exercised here. Accepted, to honour the trait
        // honestly, and deliberately unused.
        _language: Option<&str>,
        _prompt: Option<&str>,
    ) -> Result<Inference, String> {
        tokio::fs::create_dir_all(&self.temp_dir)
            .await
            .map_err(|err| err.to_string())?;
        let wav_path = unique_wav_path(&self.temp_dir);
        let _scratch = ScratchWav(wav_path.clone());
        tokio::fs::write(&wav_path, &wav)
            .await
            .map_err(|err| err.to_string())?;

        let args = build_args(&self.files, &wav_path, self.num_threads);
        let output = tokio::process::Command::new(&self.executable)
            .args(&args)
            .stdin(Stdio::null())
            .kill_on_drop(true)
            .output()
            .await;

        // Best-effort: the temp file is scratch, and a failed cleanup must
        // not hide the real result of the transcription attempt.
        // ScratchWav also covers cancellation while awaiting the child.

        let output = output.map_err(|err| err.to_string())?;
        let stderr = String::from_utf8_lossy(&output.stderr);
        Ok(interpret(output.status.code(), &output.stdout, &stderr))
    }

    fn model_name(&self) -> Option<String> {
        self.model_name.clone()
    }
}

struct ScratchWav(PathBuf);
impl Drop for ScratchWav {
    fn drop(&mut self) {
        let path = self.0.clone();
        match std::fs::remove_file(&path) {
            Ok(()) => return,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return,
            Err(_) => {}
        }
        // A cancelled Windows child may still be releasing its file handle.
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            runtime.spawn(async move {
                for _ in 0..5 {
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                    match tokio::fs::remove_file(&path).await {
                        Ok(()) => return,
                        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return,
                        Err(_) => {}
                    }
                }
                tracing::warn!("could not remove Parakeet scratch audio after process cleanup");
            });
        } else {
            tracing::warn!("could not remove Parakeet scratch audio without an active runtime");
        }
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

    #[test]
    fn failure_keeps_native_exit_code_and_final_diagnostics() {
        let stderr = format!("{}final native failure", "configuration ".repeat(100));
        let result = interpret(Some(-1073740791), b"", &stderr);
        let Inference::Rejected { body, .. } = result else {
            panic!("must reject");
        };
        assert!(body.contains("0xC0000409"));
        assert!(body.ends_with("final native failure"));
        assert!(body.len() < 600);
    }

    #[tokio::test]
    async fn cancelling_an_operation_removes_its_scratch_wav() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cancelled.wav");
        std::fs::write(&path, b"fixture").unwrap();
        let (ready, receiver) = tokio::sync::oneshot::channel();
        let owned = path.clone();
        let task = tokio::spawn(async move {
            let _scratch = ScratchWav(owned);
            let _ = ready.send(());
            std::future::pending::<()>().await;
        });
        receiver.await.unwrap();
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
        assert!(!path.exists());
    }

    /* ---------------------------------------------- the measured output */

    /// The real stdout of one `sherpa-onnx-offline` run, measured on 2026-08-31
    /// against `parakeet-unified-en-0.6b-int8` over the same 13.5 s of speech
    /// the whisper timing measurement used.
    ///
    /// Verbatim except that `ys_log_probs` is dropped -- 64 floats this build
    /// does not parse. `text`, `timestamps` and `tokens` are exactly as
    /// printed, including the empty `words` and `durations` that made a cue
    /// builder necessary in the first place.
    const REAL_STDOUT: &str = r##"{"lang": "", "emotion": "", "event": "", "text": "The quick brown fox jumps over the lazy dog. Second sentence, spoken after a pause, so the server has somewhere to cut. Third and last sentence of the probe recording.", "timestamps": [0.64, 0.72, 0.72, 0.88, 0.96, 1.2, 1.28, 1.52, 1.6, 1.6, 1.92, 1.92, 2.16, 2.24, 2.24, 2.32, 2.64, 2.72, 4.08, 4.24, 4.32, 4.56, 4.64, 4.64, 5.52, 5.68, 5.76, 5.76, 6.16, 6.24, 6.48, 6.56, 6.64, 6.64, 7.52, 7.68, 7.84, 7.92, 7.92, 8.32, 8.56, 8.56, 8.64, 8.64, 8.96, 9.12, 9.12, 10.4, 10.56, 10.56, 10.8, 10.88, 10.88, 11.2, 11.2, 11.28, 11.76, 11.84, 11.92, 12.0, 12.24, 12.32, 12.4, 13.52], "durations": [], "tokens": [" The", " qu", "ick", " br", "own", " fo", "x", " j", "um", "ps", " o", "ver", " the", " la", "z", "y", " do", "g", ".", " Se", "cond", " s", "ent", "ence", ",", " sp", "ok", "en", " a", "fter", " a", " p", "a", "use", ",", " so", " the", " ser", "ver", " has", " s", "ome", "wh", "ere", " to", " c", "ut", ".", " Th", "ird", " and", " l", "ast", " s", "ent", "ence", " of", " the", " pro", "be", " rec", "ord", "ing", "."], "words": []}"##;

    #[test]
    fn the_measured_output_becomes_three_cues() {
        // The whole point. Against this audio whisper's own segmenter said
        // 0.00-3.00 / 3.68-9.44 / 10.08-12.88; this rule finds the same three
        // utterances from per-token onsets alone, each a fraction tighter
        // because a cue ends at its last token's onset and `durations` is empty.
        match interpret(Some(0), REAL_STDOUT.as_bytes(), "") {
            Inference::Timed { segments, text } => {
                assert_eq!(segments.len(), 3, "{segments:#?}");

                assert_eq!(
                    segments[0].text,
                    "The quick brown fox jumps over the lazy dog."
                );
                assert!((segments[0].start - 0.64).abs() < 1e-9);
                // The onset of "g", not of the "." that follows it 1.36 s
                // later. See `cues`.
                assert!((segments[0].end - 2.72).abs() < 1e-9, "{:?}", segments[0]);

                assert!(segments[1].text.starts_with("Second sentence,"));
                assert!(segments[1].text.ends_with("somewhere to cut."));
                assert!((segments[1].start - 4.24).abs() < 1e-9, "{:?}", segments[1]);
                assert!((segments[1].end - 9.12).abs() < 1e-9, "{:?}", segments[1]);

                assert_eq!(
                    segments[2].text,
                    "Third and last sentence of the probe recording."
                );
                assert!(
                    (segments[2].start - 10.56).abs() < 1e-9,
                    "{:?}",
                    segments[2]
                );
                assert!((segments[2].end - 12.40).abs() < 1e-9, "{:?}", segments[2]);

                // No cue may outlast the one after it, which is what a
                // punctuation onset used to cause.
                for pair in segments.windows(2) {
                    assert!(pair[0].end <= pair[1].start, "{pair:?}");
                }

                // The cues reconstruct the transcript, so nothing was dropped
                // between them.
                let joined = segments
                    .iter()
                    .map(|s| s.text.as_str())
                    .collect::<Vec<_>>()
                    .join(" ");
                assert_eq!(joined, text);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn cues_never_start_inside_a_word() {
        // The protection `--split-on-word` gives whisper, unconditional here:
        // splitting `"ick"` off `" qu"` could never be wanted. Every cue must
        // begin at a token that began a word, which after trimming means no cue
        // starts with a subword fragment of the previous one.
        let segments = match interpret(Some(0), REAL_STDOUT.as_bytes(), "") {
            Inference::Timed { segments, .. } => segments,
            other => panic!("{other:?}"),
        };
        for segment in &segments {
            let first = segment.text.split_whitespace().next().unwrap_or_default();
            assert!(
                first.chars().next().is_some_and(char::is_alphanumeric),
                "cue starts mid-word: {segment:?}"
            );
        }
    }

    #[test]
    fn every_boundary_is_a_reported_onset() {
        // The rule that makes these cues honest. Nothing is interpolated, so
        // every start and every end must appear verbatim in `timestamps`.
        let onsets: Vec<f64> = serde_json::from_str::<serde_json::Value>(REAL_STDOUT)
            .unwrap()
            .get("timestamps")
            .unwrap()
            .as_array()
            .unwrap()
            .iter()
            .filter_map(serde_json::Value::as_f64)
            .collect();

        let segments = match interpret(Some(0), REAL_STDOUT.as_bytes(), "") {
            Inference::Timed { segments, .. } => segments,
            other => panic!("{other:?}"),
        };
        for segment in &segments {
            assert!(
                onsets.iter().any(|o| (o - segment.start).abs() < 1e-9),
                "invented start: {segment:?}"
            );
            assert!(
                onsets.iter().any(|o| (o - segment.end).abs() < 1e-9),
                "invented end: {segment:?}"
            );
        }
    }

    #[test]
    fn a_run_with_no_timings_is_still_a_transcript() {
        // A model family that prints no `timestamps` at all, or a future
        // release that renames the field. The transcript is still good; there
        // are simply no cues, and `Text` is how that is said.
        let body = r#"{"text": "hello there", "timestamps": [], "tokens": []}"#;
        match interpret(Some(0), body.as_bytes(), "") {
            Inference::Text(text) => assert_eq!(text, "hello there"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_mismatched_count_is_refused_rather_than_zipped() {
        // Three tokens, two onsets. `zip` would silently drop the third and
        // report two cues that looked fine, which is the failure mode the
        // all-or-nothing rule exists to prevent.
        assert!(cues(
            &[" a".to_owned(), " b".to_owned(), " c".to_owned()],
            &[0.0, 1.0]
        )
        .is_empty());
    }

    #[test]
    fn an_unpunctuated_run_is_split_by_the_backstop() {
        // A transducer has no decode window, so without the cap a 57-minute
        // recording with no punctuation in it would come back as one cue --
        // exactly the defect this work started from.
        let word = " abcdefghij".to_owned();
        let tokens: Vec<String> = (0..60).map(|_| word.clone()).collect();
        #[allow(clippy::cast_precision_loss)]
        let onsets: Vec<f64> = (0..60).map(|i| f64::from(i) * 0.5).collect();

        let built = cues(&tokens, &onsets);
        assert!(built.len() > 1, "{built:#?}");
        for segment in &built {
            // The cap is checked against what a cue already holds, so one word
            // may cross it; nothing may be far past it.
            assert!(
                segment.text.chars().count() <= MAX_CUE_CHARS + word.len(),
                "{segment:?}"
            );
        }
    }

    fn layout(dir: &Path) {
        std::fs::write(dir.join("encoder.int8.onnx"), b"e").unwrap();
        std::fs::write(dir.join("decoder.int8.onnx"), b"d").unwrap();
        std::fs::write(dir.join("joiner.int8.onnx"), b"j").unwrap();
        std::fs::write(dir.join("tokens.txt"), b"t").unwrap();
    }

    // -------------------------------------------------------------- discovery

    #[test]
    fn the_measured_parakeet_layout_is_discovered() {
        // The real shape of both pinned Parakeet archives, measured with
        // examples/sherpa_probe.rs: encoder/decoder/joiner/tokens directly at
        // the model root.
        let dir = tempfile::tempdir().unwrap();
        layout(dir.path());
        // The real archives also carry a test_wavs/ subdirectory of sample
        // audio, which must not be mistaken for anything discovery looks for.
        std::fs::create_dir_all(dir.path().join("test_wavs")).unwrap();
        std::fs::write(dir.path().join("test_wavs").join("0.wav"), b"w").unwrap();

        let files = discover_transducer_files(dir.path()).unwrap();
        assert_eq!(files.encoder, dir.path().join("encoder.int8.onnx"));
        assert_eq!(files.decoder, dir.path().join("decoder.int8.onnx"));
        assert_eq!(files.joiner, dir.path().join("joiner.int8.onnx"));
        assert_eq!(files.tokens, dir.path().join("tokens.txt"));
    }

    #[test]
    fn a_missing_file_is_reported_by_name() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("decoder.int8.onnx"), b"d").unwrap();
        std::fs::write(dir.path().join("joiner.int8.onnx"), b"j").unwrap();
        std::fs::write(dir.path().join("tokens.txt"), b"t").unwrap();
        // No encoder.

        match discover_transducer_files(dir.path()) {
            Err(LayoutError::Missing { what, .. }) => assert_eq!(what, "encoder"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_subdirectory_of_sample_audio_is_not_recursed_into() {
        // If a nemotron layout ever put its encoder one level down, this
        // must fail closed rather than silently pick it up from somewhere
        // unmeasured.
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("decoder.int8.onnx"), b"d").unwrap();
        std::fs::write(dir.path().join("joiner.int8.onnx"), b"j").unwrap();
        std::fs::write(dir.path().join("tokens.txt"), b"t").unwrap();
        let nested = dir.path().join("nested");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(nested.join("encoder.int8.onnx"), b"e").unwrap();

        assert!(matches!(
            discover_transducer_files(dir.path()),
            Err(LayoutError::Missing {
                what: "encoder",
                ..
            })
        ));
    }

    #[test]
    fn two_candidates_for_one_role_are_ambiguous_rather_than_guessed_at() {
        let dir = tempfile::tempdir().unwrap();
        layout(dir.path());
        std::fs::write(dir.path().join("encoder.fp32.onnx"), b"e2").unwrap();

        match discover_transducer_files(dir.path()) {
            Err(LayoutError::Ambiguous { what, found, .. }) => {
                assert_eq!(what, "encoder");
                assert_eq!(found.len(), 2);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn an_unreadable_directory_is_reported_rather_than_treated_as_empty() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("does-not-exist");
        assert!(matches!(
            discover_transducer_files(&missing),
            Err(LayoutError::Unreadable { .. })
        ));
    }

    // ------------------------------------------------------------------ args

    fn files() -> TransducerFiles {
        TransducerFiles {
            encoder: PathBuf::from("/models/p/encoder.int8.onnx"),
            decoder: PathBuf::from("/models/p/decoder.int8.onnx"),
            joiner: PathBuf::from("/models/p/joiner.int8.onnx"),
            tokens: PathBuf::from("/models/p/tokens.txt"),
        }
    }

    fn find(args: &[String], flag: &str) -> Option<String> {
        args.iter()
            .find_map(|a| a.strip_prefix(&format!("{flag}=")))
            .map(str::to_owned)
    }

    #[test]
    fn the_four_measured_flags_carry_the_right_paths() {
        let args = build_args(&files(), Path::new("/tmp/in.wav"), None);
        assert_eq!(
            find(&args, "--tokens").as_deref(),
            Some("/models/p/tokens.txt")
        );
        assert_eq!(
            find(&args, "--encoder").as_deref(),
            Some("/models/p/encoder.int8.onnx")
        );
        assert_eq!(
            find(&args, "--decoder").as_deref(),
            Some("/models/p/decoder.int8.onnx")
        );
        assert_eq!(
            find(&args, "--joiner").as_deref(),
            Some("/models/p/joiner.int8.onnx")
        );
    }

    #[test]
    fn the_wav_path_is_the_last_positional_argument() {
        let args = build_args(&files(), Path::new("/tmp/in.wav"), None);
        assert_eq!(args.last().map(String::as_str), Some("/tmp/in.wav"));
    }

    #[test]
    fn num_threads_is_omitted_unless_chosen() {
        // Passing a default would override sherpa-onnx-offline's own, which
        // is the same reasoning `local::build_args` uses for whisper.
        let args = build_args(&files(), Path::new("/tmp/in.wav"), None);
        assert!(!args.iter().any(|a| a.starts_with("--num-threads")));
    }

    #[test]
    fn num_threads_is_passed_when_chosen() {
        let args = build_args(&files(), Path::new("/tmp/in.wav"), Some(4));
        assert_eq!(find(&args, "--num-threads").as_deref(), Some("4"));
    }

    // ------------------------------------------------------------- responses

    #[test]
    fn the_real_measured_stdout_shape_is_parsed() {
        // The literal stdout sherpa-onnx-offline v1.13.4 printed for
        // tests/fixtures/speech-16k-mono.wav against both pinned Parakeet
        // archives, captured by examples/sherpa_probe.rs.
        let stdout = br#"{"lang": "", "emotion": "", "event": "", "text": "The quick brown fox jumps over the lazy dog. This is a test of local transcription.", "timestamps": [], "durations": [], "tokens":[], "ys_log_probs": [], "words": []}"#;
        match interpret(Some(0), stdout, "") {
            Inference::Text(text) => assert_eq!(
                text,
                "The quick brown fox jumps over the lazy dog. This is a test of local \
                 transcription."
            ),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn an_empty_text_field_is_empty_not_rejected() {
        let stdout = br#"{"text": "   "}"#;
        assert_eq!(interpret(Some(0), stdout, ""), Inference::Empty);
    }

    #[test]
    fn a_clean_exit_with_unparseable_stdout_is_rejected_not_empty() {
        // The model heard nothing and the CLI worked are different claims
        // from a stdout that is not the shape this build expects at all --
        // the same distinction `local::interpret` draws for whisper-server.
        assert!(matches!(
            interpret(Some(0), b"not json at all", "diagnostic line"),
            Inference::Rejected { status: 0, .. }
        ));
    }

    #[test]
    fn a_nonzero_exit_is_rejected_with_the_code_and_stderr() {
        match interpret(Some(2), b"", "model file not found") {
            Inference::Rejected { status, body } => {
                assert_eq!(status, 2);
                assert_eq!(
                    body,
                    "Parakeet exited with code 2 (0x00000002). model file not found"
                );
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_signal_kill_is_rejected_rather_than_panicking_on_a_missing_code() {
        assert!(matches!(
            interpret(None, b"", "killed"),
            Inference::Rejected { status: 1, .. }
        ));
    }

    #[test]
    fn a_long_stderr_is_truncated_on_a_character_boundary() {
        let stderr = "å".repeat(STDERR_LIMIT + 50);
        match interpret(Some(1), b"", &stderr) {
            Inference::Rejected { body, .. } => {
                assert!(body.ends_with(&"å".repeat(STDERR_LIMIT)));
                assert_eq!(
                    body.chars().count(),
                    "Parakeet exited with code 1 (0x00000001). ".len() + STDERR_LIMIT
                );
            }
            other => panic!("{other:?}"),
        }
    }

    // -------------------------------------------------------- the whole path

    #[tokio::test]
    async fn model_name_is_reported_for_the_history_row() {
        let dir = tempfile::tempdir().unwrap();
        layout(dir.path());
        let files = discover_transducer_files(dir.path()).unwrap();
        let temp = tempfile::tempdir().unwrap();

        let sherpa = SherpaOffline::new(
            PathBuf::from("sherpa-onnx-offline"),
            files,
            Some("parakeet-unified-en-0.6b".to_owned()),
            temp.path().to_path_buf(),
            None,
        );
        assert_eq!(
            sherpa.model_name(),
            Some("parakeet-unified-en-0.6b".to_owned())
        );
    }

    #[tokio::test]
    async fn a_missing_executable_is_reported_rather_than_panicking() {
        let dir = tempfile::tempdir().unwrap();
        layout(dir.path());
        let files = discover_transducer_files(dir.path()).unwrap();
        let temp = tempfile::tempdir().unwrap();

        let sherpa = SherpaOffline::new(
            temp.path().join("definitely-not-a-real-binary"),
            files,
            None,
            temp.path().to_path_buf(),
            None,
        );

        let err = sherpa.transcribe(vec![0_u8; 16], None, None).await;
        assert!(err.is_err());
    }

    /// The staged WAV is removed even when the process cannot start.
    ///
    /// **The absence needs a control, and this had none.** "No file matching
    /// `sherpa-input-` exists" is trivially true of a run that never staged one,
    /// so a `transcribe` that failed before writing anything — or a rename of
    /// the prefix — would leave this passing while proving nothing. That shape
    /// cost four hundred runs to find once already (KNEG-88), and this is a
    /// claim about raw audio not being left on disk, which is worth more than
    /// most.
    ///
    /// The control is the scratch directory. It does not exist beforehand, so
    /// `transcribe` creating it proves the function reached `create_dir_all` —
    /// and the write of the WAV is the very next statement.
    #[tokio::test]
    async fn the_staged_wav_is_cleaned_up_even_when_the_process_cannot_start() {
        let dir = tempfile::tempdir().unwrap();
        layout(dir.path());
        let files = discover_transducer_files(dir.path()).unwrap();
        let temp = tempfile::tempdir().unwrap();
        let scratch = temp.path().join("scratch");
        assert!(
            !scratch.exists(),
            "premise: the scratch directory is not there yet"
        );

        let sherpa = SherpaOffline::new(
            temp.path().join("not-a-real-binary"),
            files,
            None,
            scratch.clone(),
            None,
        );
        let result = sherpa.transcribe(vec![0_u8; 16], None, None).await;

        assert!(
            result.is_err(),
            "premise: a binary that is not there cannot have transcribed anything"
        );
        assert!(
            scratch.is_dir(),
            "the staging directory was never created, so nothing was ever staged and the \
             assertion below would pass for the wrong reason"
        );

        let leftover: Vec<_> = std::fs::read_dir(&scratch)
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().starts_with("sherpa-input-"))
            .collect();
        assert!(leftover.is_empty(), "a staged wav was left behind");
    }
}
