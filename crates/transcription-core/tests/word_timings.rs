//! Word timings from the real server (KNEG-408).
//!
//! `#[ignore]`d, because it downloads about 80 MB and starts a real server:
//!
//! ```text
//! cargo test -p kw-transcription-core --test word_timings -- --ignored --nocapture
//! ```
//!
//! # What this exists to catch
//!
//! The words a note highlights come from the per-token times whisper.cpp's
//! server puts in `verbose_json`, under `segments[].words[]` with the token's
//! text in `word`. That shape was read in the server's source at b4938, and
//! `local.rs`'s unit tests pin how it is read from a body written by hand.
//! Only the pinned server can say whether it sends those times at all for the
//! request `WhisperServer::timed` makes, and whether they are times rather
//! than the -1 the server holds when it computed none. This asks it, with the
//! shipped arguments, through `WhisperServer` as the application builds it.
//!
//! It verifies the link and nothing further: that words arrive, spell their
//! rows and run forward in time. How close a word's time is to when it was
//! said, on a real recording, is a judgement by ear (KNEG-378).
//!
//! # The recording
//!
//! The 6.3-second fixture `subtitle_export.rs` uses, whose two sentences were
//! measured at 0.11-2.56 s and 3.49-5.50 s (`speech_first.rs`).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::time::Duration;

use kw_network_policy::NetworkPolicy;
use kw_sidecar::{pid::PidStore, port, process, Catalog, HttpHealth, Launch, StartTimings};
use kw_transcription_core::local::{
    build_args, Inference, LocalTranscriber, ServerOptions, WhisperServer, HEALTH_PATH,
};
use kw_transcription_core::models;

const SPEECH: &[u8] = include_bytes!("fixtures/speech-16k-mono.wav");

/// How far a word may sit outside its row: one of whisper's 20 ms steps
/// either way, and rounding.
const SLACK: f64 = 0.05;

#[tokio::test(flavor = "multi_thread")]
#[ignore = "downloads ~80 MB and starts a real server; run with --ignored"]
async fn the_pinned_server_times_every_word_of_a_timed_request() {
    let catalog = Catalog::shipped().unwrap();
    let (spec, asset) = match catalog.resolve("whisper") {
        Ok(pair) => pair,
        Err(err) => {
            println!("skipped: {err}");
            return;
        }
    };

    let root = tempfile::tempdir().unwrap();
    let policy = NetworkPolicy::new().unwrap();
    let installed = kw_sidecar::install(&policy, spec, asset, root.path(), &mut |_| {})
        .await
        .expect("the pinned whisper-server installs");

    let registry = kw_provider_registry::registry().unwrap();
    let tiny = registry
        .local_asr
        .whisper
        .iter()
        .find(|m| m.id == "tiny")
        .unwrap();
    let model = models::install(&policy, tiny, &root.path().join("models"), &mut |_| {})
        .await
        .expect("the model installs");

    let chosen = port::allocate(spec.port_range().unwrap(), &port::BindProbe).unwrap();
    let running = process::start(
        spec,
        &Launch {
            executable: installed.executable.clone(),
            args: build_args(&ServerOptions {
                model_path: model.path.display().to_string(),
                port: chosen,
                language: None,
                threads: None,
                gpu_device: None,
            }),
            port: chosen,
        },
        &PidStore::new(root.path()),
        &HttpHealth::new(policy.clone(), HEALTH_PATH),
        StartTimings {
            timeout: Duration::from_secs(90),
            poll_interval: Duration::from_millis(100),
            ..StartTimings::default()
        },
    )
    .await
    .expect("whisper-server should start and answer");

    let server = WhisperServer::new(policy.clone(), chosen, Some("tiny".to_owned())).timed();
    let result = server.transcribe(SPEECH.to_vec(), None, None).await;
    running.stop(&PidStore::new(root.path())).await.unwrap();

    let Inference::Timed { segments, .. } = result.expect("the request reaches the server") else {
        panic!("expected timings from a `.timed()` request");
    };
    for segment in &segments {
        println!(
            "{:6.2} - {:6.2}  {}",
            segment.start, segment.end, segment.text
        );
        for word in &segment.words {
            println!("    {:6.2} - {:6.2}  {}", word.start, word.end, word.text);
        }
    }

    assert!(!segments.is_empty());
    for segment in &segments {
        assert!(
            !segment.words.is_empty(),
            "a row came back without words: {segment:?}"
        );
        let spelled: String = segment.words.iter().map(|w| w.text.as_str()).collect();
        assert_eq!(
            spelled.replace(char::is_whitespace, ""),
            segment.text.replace(char::is_whitespace, ""),
            "the words spell their row"
        );
        for word in &segment.words {
            assert!(word.end >= word.start, "backwards: {word:?}");
            assert!(
                word.start >= segment.start - SLACK && word.end <= segment.end + SLACK,
                "{word:?} is outside its row {:.2}-{:.2}",
                segment.start,
                segment.end
            );
        }
        for pair in segment.words.windows(2) {
            assert!(pair[1].start >= pair[0].start, "out of order: {pair:?}");
        }
    }
}
