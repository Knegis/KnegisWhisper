//! The whole sherpa local dictation path, for real: install the CLI sidecar,
//! install a Parakeet model, transcribe speech, read the words back.
//!
//! `#[ignore]`d, because it downloads the sherpa-onnx CLI (~25 MB on Windows)
//! plus one Parakeet model archive (~630-680 MB). Run it deliberately:
//!
//! ```text
//! cargo test -p kw-transcription-core --test sherpa_dictation -- --ignored --nocapture
//! ```
//!
//! Mirrors `local_dictation.rs`'s reasoning exactly: fixtures and fakes can
//! prove the argument-building logic, but they cannot tell you whether the
//! flags this build derived are flags the real `sherpa-onnx-offline` binary
//! accepts, or whether a real extracted model has the layout
//! `sherpa::discover_transducer_files` expects. `examples/sherpa_probe.rs`
//! found that out first, by hand; this pins the same finding behind a test
//! that exercises the production `SherpaOffline` type itself, not a
//! throwaway script, so it stays true as the pinned sherpa release moves.
//!
//! The fixture is the same one `local_dictation.rs` uses for whisper: 6.3s of
//! synthesised speech at 16 kHz mono, generated with the Windows speech
//! synthesiser. Both `local_dictation.rs` and `examples/sherpa_probe.rs`
//! transcribed it in the course of this work; whisper (tiny) said "The Quick
//! Brown Fox jumps over the lazy dog. This is a test of local
//! transcription." and sherpa (both pinned Parakeet models) said "The quick
//! brown fox jumps over the lazy dog. This is a test of local
//! transcription." -- the same words, differing only in whisper's mid-sentence
//! capitalisation.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use kw_network_policy::NetworkPolicy;
use kw_sidecar::Catalog;
use kw_transcription_core::local::{Inference, LocalTranscriber};
use kw_transcription_core::sherpa::SherpaOffline;
use kw_transcription_core::sherpa_models;

const SPEECH: &[u8] = include_bytes!("fixtures/speech-16k-mono.wav");

/// Which model to prove the path with. Parakeet Unified EN 0.6B: English
/// only, non-streaming, and the smaller of the two pinned Parakeet archives.
const MODEL_ID: &str = "parakeet-unified-en-0.6b";

#[tokio::test(flavor = "multi_thread")]
#[ignore = "downloads the sherpa CLI and a ~630 MB model archive; run with --ignored"]
async fn speech_goes_in_and_the_right_words_come_out() {
    let catalog = Catalog::shipped().unwrap();
    let (spec, asset) = match catalog.resolve("sherpa") {
        Ok(pair) => pair,
        Err(err) => {
            // An honest refusal on a platform sherpa is not published for,
            // not a failure of this test.
            println!("skipped: {err}");
            return;
        }
    };

    let root = tempfile::tempdir().unwrap();
    let policy = NetworkPolicy::new().unwrap();

    // 1. The CLI.
    let installed = kw_sidecar::install(&policy, spec, asset, root.path(), &mut |_| {})
        .await
        .expect("the pinned sherpa-onnx CLI should install");
    println!("installed {} sidecar file(s)", installed.files);

    // 2. The model.
    let registry = kw_provider_registry::registry().unwrap();
    let model = registry
        .local_asr
        .sherpa
        .iter()
        .find(|m| m.id == MODEL_ID)
        .unwrap();

    let models_dir = root.path().join("models");
    let installed_model = sherpa_models::install(&policy, model, &models_dir, &mut |_| {})
        .await
        .expect("the pinned parakeet model should download and extract");
    println!(
        "installed model at {} ({} bytes downloaded)",
        installed_model.dir.display(),
        installed_model.bytes,
    );

    // 3. `sherpa_models::install` already discovered the layout (that is the
    // real check it holds the install to -- see its module comment), so the
    // files it found are used directly rather than rediscovered.
    let files = installed_model.files;

    // 4. Transcribe.
    let temp_dir = root.path().join("audio-temp");
    let transcriber = SherpaOffline::new(
        installed.executable.clone(),
        files,
        Some(model.id.clone()),
        temp_dir,
        None,
    );

    let result = transcriber
        .transcribe(SPEECH.to_vec(), None, None)
        .await
        .expect("the transcription should run");

    match &result {
        Inference::Text(text) => {
            println!("transcript: {text}");
            let lowered = text.to_lowercase();
            assert!(
                lowered.contains("quick brown fox"),
                "expected the fixture's words, got {text:?}"
            );
            assert!(
                lowered.contains("lazy dog"),
                "expected the fixture's words, got {text:?}"
            );
            assert_eq!(
                text.trim(),
                text,
                "the transcript should already be trimmed"
            );
        }
        other => panic!("expected a transcript, got {other:?}"),
    }

    assert_eq!(
        transcriber.model_name(),
        Some(model.id.clone()),
        "the history row should be able to name which model produced this"
    );
}
