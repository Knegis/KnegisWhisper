//! A long, noisy recording through the real server and the real speech
//! finder (KNEG-404).
//!
//! `#[ignore]`d, because it downloads about 160 MB and starts a real server:
//!
//! ```text
//! cargo test -p kw-transcription-core --test speech_first -- --ignored --nocapture
//! ```
//!
//! # What this exists to catch
//!
//! Sent whole, a recording with noise between its sentences came back with
//! rows stamped seconds from their speech, and noise alone came back as
//! invented text. Finding the speech first and sending it in chunks is meant
//! to put every row on its own speech by construction (`kw_transcription_core::
//! speech`). The unit tests pin the order of requests against a scripted
//! server and a scripted finder. Only the real tool, the bundled model and the
//! real server can say whether the rows actually land where the speech is, so
//! this asks them, through `WhisperServer` as the application builds it.
//!
//! # The recordings
//!
//! Built at run time from the fixture `subtitle_export.rs` uses: the fixture
//! four times, with silences of 20, 30, 2, 15 and 10 seconds around the
//! repeats, under a fixed white-noise floor. The fixture's two sentences were
//! measured at 0.11-2.56 s and 3.49-5.50 s, so every sentence's true start and
//! end is known from the construction. The second recording is the noise
//! floor alone, 40 seconds of it.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use std::sync::Arc;
use std::time::{Duration, Instant};

use kw_audio_core::vad::VadConfig;
use kw_network_policy::NetworkPolicy;
use kw_sidecar::{pid::PidStore, port, process, Catalog, HttpHealth, Launch, StartTimings};
use kw_transcription_core::local::{
    build_args, Inference, LocalTranscriber, ServerOptions, TimedSegment, WhisperServer,
    HEALTH_PATH,
};
use kw_transcription_core::models;
use kw_transcription_core::speech::SpeechFinder;

const SPEECH: &[u8] = include_bytes!("fixtures/speech-16k-mono.wav");

/// The Silero model the desktop application bundles.
const VAD_MODEL: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../apps/desktop/src-tauri/resources/ggml-silero-v5.1.2.bin"
);

const RATE: usize = 16_000;

/// Silence before each repeat of the fixture, and after the last.
const GAPS: [f64; 5] = [20.0, 30.0, 2.0, 15.0, 10.0];

/// The fixture's two sentences, in seconds from its start.
const SENTENCES: [(f64, f64); 2] = [(0.11, 2.56), (3.49, 5.50)];

/// How far a row may start from its sentence, and end past the last sentence
/// it holds. What "on its own speech" means here: the measured prototype
/// managed 0.94 s and 1.38 s on a harder recording.
const START_SLACK: f64 = 1.0;
const END_SLACK: f64 = 1.5;

/// Noise, about 2% of full scale, from a linear congruential generator rather
/// than a crate, so it is the same on every run and every machine.
fn add_noise(samples: &mut [f32]) {
    let mut state: u32 = 0x2545_f491;
    for sample in samples {
        state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let noise = (f64::from(state >> 8) / f64::from(1_u32 << 24)).mul_add(2.0, -1.0) * 0.02;
        *sample = (*sample + noise as f32).clamp(-1.0, 1.0);
    }
}

/// The recording, and every sentence's true start and end in it.
fn noisy_recording() -> (Vec<u8>, Vec<(f64, f64)>) {
    let speech = kw_audio_core::wav::from_pcm(SPEECH.get(44..).expect("a WAV header"));
    let mut out: Vec<f32> = Vec::new();
    let mut truth = Vec::new();
    for (index, gap) in GAPS.iter().enumerate() {
        out.extend(vec![0.0_f32; (gap * RATE as f64) as usize]);
        if index + 1 < GAPS.len() {
            let offset = out.len() as f64 / RATE as f64;
            truth.extend(SENTENCES.iter().map(|(s, e)| (offset + s, offset + e)));
            out.extend_from_slice(&speech);
        }
    }
    add_noise(&mut out);
    println!(
        "built a {:.1} s recording with {} sentences",
        out.len() as f64 / RATE as f64,
        truth.len()
    );
    (kw_audio_core::wav::encode_wav(&out, RATE as u32), truth)
}

fn noise_only() -> Vec<u8> {
    let mut out = vec![0.0_f32; 40 * RATE];
    add_noise(&mut out);
    kw_audio_core::wav::encode_wav(&out, RATE as u32)
}

fn rows(inference: &Inference) -> Vec<TimedSegment> {
    match inference {
        Inference::Timed { segments, .. } => segments.clone(),
        other => panic!("expected timed rows, got {other:?}"),
    }
}

fn print(label: &str, rows: &[TimedSegment], took: Duration) {
    println!(
        "{label}: {} rows in {:.2} s",
        rows.len(),
        took.as_secs_f64()
    );
    for row in rows {
        println!("  {:7.2} - {:7.2}  {}", row.start, row.end, row.text);
    }
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "downloads ~160 MB and starts a real server; run with --ignored"]
async fn every_row_sits_on_its_own_speech_and_noise_alone_is_nothing() {
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

    // The finder ships in the same archive as the server.
    let tool = installed.executable.with_file_name(format!(
        "whisper-vad-speech-segments{}",
        std::env::consts::EXE_SUFFIX
    ));
    if !tool.is_file() {
        if cfg!(windows) {
            panic!(
                "the pinned Windows archive carries {}; it is gone",
                tool.display()
            );
        }
        println!("skipped: this platform's archive has no {}", tool.display());
        return;
    }

    let registry = kw_provider_registry::registry().unwrap();
    let base = registry
        .local_asr
        .whisper
        .iter()
        .find(|m| m.id == "base")
        .unwrap();
    let model = models::install(&policy, base, &root.path().join("models"), &mut |_| {})
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

    let finder = SpeechFinder::new(
        tool,
        VAD_MODEL.into(),
        VadConfig::default(),
        root.path().join("scratch"),
    );
    let (wav, truth) = noisy_recording();

    // For the record, the recording sent whole, as before. Not asserted on:
    // it is what this change moves away from.
    let whole = WhisperServer::new(policy.clone(), chosen, None).timed();
    let started = Instant::now();
    let before = whole.transcribe(wav.clone(), None, None).await.unwrap();
    print("sent whole", &rows(&before), started.elapsed());

    let server = WhisperServer::new(policy.clone(), chosen, None)
        .timed()
        .speech_first(Arc::new(finder));
    let started = Instant::now();
    let after = rows(&server.transcribe(wav, None, None).await.unwrap());
    print("speech first", &after, started.elapsed());

    let noise = server.transcribe(noise_only(), None, None).await.unwrap();
    println!("noise alone: {noise:?}");

    running.stop(&PidStore::new(root.path())).await.unwrap();

    let mut worst_start = 0.0_f64;
    let mut worst_end = 0.0_f64;
    let mut heard = 0;
    for row in &after {
        let nearest = truth
            .iter()
            .map(|(start, _)| (row.start - start).abs())
            .fold(f64::INFINITY, f64::min);
        let last_end = truth
            .iter()
            .filter(|(start, _)| *start >= row.start - START_SLACK && *start < row.end)
            .map(|(_, end)| *end)
            .fold(f64::NEG_INFINITY, f64::max);
        worst_start = worst_start.max(nearest);
        worst_end = worst_end.max(row.end - last_end);
        assert!(
            nearest <= START_SLACK,
            "row at {:.2} starts {nearest:.2} s from any sentence",
            row.start
        );
        assert!(
            row.end <= last_end + END_SLACK,
            "row at {:.2} ends at {:.2}, {:.2} s after its speech",
            row.start,
            row.end,
            row.end - last_end
        );
        heard += row.text.to_lowercase().matches("quick brown fox").count();
    }
    println!("worst start {worst_start:.2} s from its sentence, worst end {worst_end:.2} s past");
    assert_eq!(heard, 4, "every repeat of the fixture was transcribed");
    assert_eq!(
        noise,
        Inference::Empty,
        "noise alone is not sent to whisper at all"
    );
}
