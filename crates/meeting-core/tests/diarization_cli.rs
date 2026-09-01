//! The diarization rules against the real CLI.
//!
//! Everything in `diarization.rs`'s own tests is a fixture recorded off a real
//! run, so it proves the rules read that output correctly and nothing about
//! whether the arguments this build constructs are ones the CLI accepts. This
//! spawns `sherpa-onnx-offline-speaker-diarization` with exactly
//! [`kw_meeting_core::diarization::build_args`]' output and reads the result
//! back through [`kw_meeting_core::diarization::interpret`].
//!
//! The same shape and the same reason as `nemotron_stream.rs` and
//! `crates/transcription-core/tests/sherpa_dictation.rs`: `--ignored`, because
//! it needs ~37 MB of model and a binary on disk, and committed rather than run
//! once by hand, so it stays true as the pinned sherpa release moves rather
//! than being true only on the day the probe ran.
//!
//! ```text
//! KW_SHERPA_DIARIZE=<...>/sherpa-onnx-offline-speaker-diarization.exe \
//! KW_DIARIZE_SEGMENTATION=<...>/sherpa-onnx-pyannote-segmentation-3-0 \
//! KW_DIARIZE_EMBEDDING=<...>/3dspeaker_speech_campplus_sv_en_voxceleb_16k.onnx \
//!   cargo test -p kw-meeting-core --test diarization_cli -- --ignored --nocapture
//! ```
//!
//! # The control is a recording whose answer is already known
//!
//! `crates/transcription-core/tests/fixtures/speech-16k-mono.wav` is one person
//! reading one sentence — the same fixture whisper and Parakeet are both
//! checked against. One voice is the answer, and it is an answer this test can
//! assert without a human listening to anything.
//!
//! # What it proves, and what it does not
//!
//! It proves the flags are accepted, the exit status means what this build
//! thinks it means, and one speaker comes back as `Speaker 1` rather than
//! `Speaker 2` — the renumbering, end to end, against the process that
//! produced the off-by-one in the first place. **It says nothing about whether
//! speaker turns are right in a real meeting**: one voice cannot be confused
//! with another, so the hard half of diarization is untested here and is a
//! by-hand check (KNEG-73a).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::path::PathBuf;

use kw_meeting_core::diarization::{build_args, interpret, DiarizeError, Models, Tuning};

fn from_env(name: &str) -> PathBuf {
    PathBuf::from(
        std::env::var(name)
            .unwrap_or_else(|_| panic!("{name} must point at the real thing for this test")),
    )
}

/// The one-speaker fixture, reached from this crate rather than copied into it.
fn control_wav() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../transcription-core/tests/fixtures/speech-16k-mono.wav")
}

fn models() -> Models {
    Models::locate(
        &from_env("KW_DIARIZE_SEGMENTATION"),
        &from_env("KW_DIARIZE_EMBEDDING"),
    )
    .expect("both models are on disk")
}

/// Spawn with exactly the arguments the shipped code builds, and read the
/// result the way the shipped code reads it.
fn run(args: &[String]) -> Result<Vec<kw_meeting_core::diarization::Turn>, DiarizeError> {
    let output = std::process::Command::new(from_env("KW_SHERPA_DIARIZE"))
        .args(args)
        .output()
        .expect("the diarization CLI starts");
    interpret(
        &String::from_utf8_lossy(&output.stdout),
        &String::from_utf8_lossy(&output.stderr),
        output.status.success(),
    )
}

#[test]
#[ignore = "needs the sherpa diarization CLI and both models on disk"]
fn the_shipped_arguments_are_ones_the_cli_accepts() {
    let turns = run(&build_args(&models(), &control_wav(), &Tuning::default()))
        .expect("a clean run over a one-speaker recording");

    assert_eq!(turns.len(), 1, "{turns:#?}");
    // The renumbering, against the process that made it necessary: sherpa
    // labels this recording's only voice `speaker_01`, and a build that read
    // the index straight through would say "Speaker 2".
    assert_eq!(turns[0].speaker, 0);
    assert_eq!(turns[0].label(), "Speaker 1");
    assert!(turns[0].end_seconds > turns[0].start_seconds);
}

#[test]
#[ignore = "needs the sherpa diarization CLI and both models on disk"]
fn a_wav_that_is_not_there_reaches_the_caller_as_a_failure() {
    // The failure mode this module is most exposed to, because stdout for a
    // failure is byte-identical to stdout for a recording nobody spoke in. If
    // the status is ever dropped, this comes back as `Ok(vec![])` and a broken
    // diarization becomes a meeting with no speakers in it.
    let mut args = build_args(&models(), &control_wav(), &Tuning::default());
    let last = args.len().saturating_sub(1);
    args[last] = "./no-such-recording.wav".to_owned();

    match run(&args) {
        Err(DiarizeError::Failed { detail }) => {
            assert!(!detail.is_empty(), "the CLI's own sentence is quoted back");
        }
        other => panic!("{other:#?}"),
    }
}
