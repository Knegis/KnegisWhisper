//! The whole local dictation path, for real: install the sidecar, install the
//! model, start the server, transcribe speech, read the words back.
//!
//! `#[ignore]`d, because it downloads about 86 MB and takes a few seconds. Run
//! it deliberately:
//!
//! ```text
//! cargo test -p kw-transcription-core --test local_dictation -- --ignored --nocapture
//! ```
//!
//! Everything else about local transcription is tested against fixtures and
//! fakes, which is the right way round — but fixtures cannot tell you whether
//! the arguments we pass are arguments this build of `whisper-server` accepts,
//! whether it answers on the port we chose, or whether the multipart form we
//! send is the one it wants. Those only fail against the real thing.
//!
//! The fixture is 6.3 seconds of synthesised speech at 16 kHz mono, generated
//! with the Windows speech synthesiser. Synthetic on purpose: it is
//! reproducible, it carries nobody's voice, and the expected words are known
//! exactly.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::time::Duration;

use kw_network_policy::NetworkPolicy;
use kw_sidecar::{pid::PidStore, port, process, Catalog, HttpHealth, Launch, StartTimings};
use kw_transcription_core::local::{
    build_args, Inference, LocalTranscriber, ServerOptions, WhisperServer, HEALTH_PATH,
};
use kw_transcription_core::models;

const SPEECH: &[u8] = include_bytes!("fixtures/speech-16k-mono.wav");

#[tokio::test(flavor = "multi_thread")]
#[ignore = "downloads ~86 MB and starts a real server; run with --ignored"]
async fn speech_goes_in_and_the_right_words_come_out() {
    let catalog = Catalog::shipped().unwrap();
    let (spec, asset) = match catalog.resolve("whisper") {
        Ok(pair) => pair,
        Err(err) => {
            // The honest refusal on macOS, not a failure.
            println!("skipped: {err}");
            return;
        }
    };

    let root = tempfile::tempdir().unwrap();
    let policy = NetworkPolicy::new().unwrap();

    // 1. The server binary.
    let installed = kw_sidecar::install(&policy, spec, asset, root.path(), &mut |_| {})
        .await
        .expect("the pinned whisper-server installs");
    println!("installed {} files", installed.files);

    // 2. The smallest model, which is also the pinned one.
    let registry = kw_provider_registry::registry().unwrap();
    let tiny = registry
        .local_asr
        .whisper
        .iter()
        .find(|m| m.id == "tiny")
        .unwrap();

    let model_dir = root.path().join("models");
    let model = models::install(&policy, tiny, &model_dir, &mut |_| {})
        .await
        .expect("the model installs");
    assert_eq!(
        model.verification,
        models::Verification::DigestMatched,
        "tiny is pinned, so this must be a digest match and not a size guess"
    );

    // 3. A free loopback port from the sidecar's declared range.
    let range = spec
        .port_range()
        .expect("whisper is a loopback server, so it declares a range");
    let chosen = port::allocate(range, &port::BindProbe).expect("a free port in the range");

    // 4. Start it.
    let options = ServerOptions {
        model_path: model.path.display().to_string(),
        port: chosen,
        language: None,
        threads: None,
        gpu_device: None,
        vad: None,
    };
    let store = PidStore::new(root.path());
    let health = HttpHealth::new(policy.clone(), HEALTH_PATH);

    let launch = Launch {
        executable: installed.executable.clone(),
        args: build_args(&options),
        port: chosen,
    };

    let running = process::start(
        spec,
        &launch,
        &store,
        &health,
        StartTimings {
            timeout: Duration::from_secs(90),
            poll_interval: Duration::from_millis(100),
        },
    )
    .await
    .expect("whisper-server should start and answer");

    // The PID record exists while it runs, which is what lets a later launch
    // clean up after a crash.
    assert!(store.read("whisper").unwrap().is_some());

    // 5. Transcribe.
    let server = WhisperServer::new(policy.clone(), chosen, Some("tiny".to_owned()));
    let result = server
        .transcribe(SPEECH.to_vec(), None, None)
        .await
        .expect("the request should reach the server");

    match &result {
        Inference::Text(text) => {
            println!("transcript: {text}");
            let lowered = text.to_lowercase();
            // Asserting on content, not just shape: a passing test that only
            // checked "some text came back" would not notice the decoder
            // arguments being wrong.
            assert!(
                lowered.contains("quick brown fox"),
                "expected the fixture's words, got {text:?}"
            );
            assert!(
                lowered.contains("lazy dog"),
                "expected the fixture's words, got {text:?}"
            );
            // The trailing newline and leading space whisper.cpp always returns
            // must have been trimmed.
            assert_eq!(text.trim(), text);
        }
        other => panic!("expected a transcript, got {other:?}"),
    }

    // 6. A custom-dictionary prompt is accepted rather than rejected.
    let prompted = server
        .transcribe(SPEECH.to_vec(), Some("en"), Some("KnegisWhisper, Knegis"))
        .await
        .expect("a prompted request should also work");
    assert!(matches!(prompted, Inference::Text(_)), "{prompted:?}");

    // 7. Stop it and check the record goes with it.
    running.stop(&store).await.unwrap();
    assert_eq!(store.read("whisper").unwrap(), None);
}
