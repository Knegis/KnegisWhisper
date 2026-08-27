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

use crate::local::{Inference, LocalTranscriber};

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

/// What one line of sherpa-onnx-offline's stdout carries. Only the field this
/// build reads; every other field the real output carries (`timestamps`,
/// `tokens`, `ys_log_probs`, ...) is left unparsed.
#[derive(Debug, Deserialize)]
struct OfflineResult {
    text: Option<String>,
}

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
            body: truncate(stderr),
        };
    }

    let text = std::str::from_utf8(stdout)
        .ok()
        .and_then(|s| serde_json::from_str::<OfflineResult>(s.trim()).ok())
        .and_then(|r| r.text);

    match text {
        Some(text) => {
            let trimmed = text.trim();
            if trimmed.is_empty() {
                Inference::Empty
            } else {
                Inference::Text(trimmed.to_owned())
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
        tokio::fs::write(&wav_path, &wav)
            .await
            .map_err(|err| err.to_string())?;

        let args = build_args(&self.files, &wav_path, self.num_threads);
        let output = tokio::process::Command::new(&self.executable)
            .args(&args)
            .stdin(Stdio::null())
            .output()
            .await;

        // Best-effort: the temp file is scratch, and a failed cleanup must
        // not hide the real result of the transcription attempt.
        let _ = tokio::fs::remove_file(&wav_path).await;

        let output = output.map_err(|err| err.to_string())?;
        let stderr = String::from_utf8_lossy(&output.stderr);
        Ok(interpret(output.status.code(), &output.stdout, &stderr))
    }

    fn model_name(&self) -> Option<String> {
        self.model_name.clone()
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
                assert_eq!(body, "model file not found");
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
                assert!(body.ends_with('…'));
                assert_eq!(body.chars().count(), STDERR_LIMIT + 1);
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

    #[tokio::test]
    async fn the_staged_wav_is_cleaned_up_even_when_the_process_cannot_start() {
        let dir = tempfile::tempdir().unwrap();
        layout(dir.path());
        let files = discover_transducer_files(dir.path()).unwrap();
        let temp = tempfile::tempdir().unwrap();

        let sherpa = SherpaOffline::new(
            temp.path().join("not-a-real-binary"),
            files,
            None,
            temp.path().to_path_buf(),
            None,
        );
        let _ = sherpa.transcribe(vec![0_u8; 16], None, None).await;

        let leftover: Vec<_> = std::fs::read_dir(temp.path())
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().starts_with("sherpa-input-"))
            .collect();
        assert!(leftover.is_empty(), "a staged wav was left behind");
    }
}
