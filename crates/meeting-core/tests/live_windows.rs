//! The live `local` meeting view against the real server and the real speech
//! finder (KNEG-410).
//!
//! `#[ignore]`d, because it downloads about 160 MB and starts a real server:
//!
//! ```text
//! cargo test -p kw-meeting-core --test live_windows -- --ignored --nocapture
//! ```
//!
//! # What this exists to catch
//!
//! `LocalWindowProvider` sends a five-second window every three seconds, and
//! now searches each for speech first, skipping those with none. The unit
//! tests pin the provider's decisions against a scripted finder; only the real
//! tool, the bundled model and the real server can say whether the windows it
//! skips are the right ones, and whether speech at a window's edge survives.
//!
//! # What it found, 2026-10-07
//!
//! Speech at a window's edge was found from 0.25 s of it; 0.1 s was not,
//! which the overlap covers (`local.rs`'s module doc). Every one of 33
//! windows was found to have speech exactly when it did, at 51 ms median.
//! The live pass took 17.9 s instead of 44.3 s, with the same twelve events.
//! Sent every window, whisper invented nothing in the 21 silent ones: the
//! invented text that 40 s of noise produces when sent whole does not happen
//! on five seconds of it, so the "before" pass is printed, not asserted on.
//!
//! # The recording
//!
//! Built as `kw-transcription-core`'s `speech_first.rs` builds it: the test
//! fixture four times, with silences of 20, 30, 2, 15 and 10 seconds around
//! the repeats, under a fixed white-noise floor, so every sentence's true
//! start and end is known.

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

use kw_audio_core::meeting::WindowFeed;
use kw_audio_core::vad::VadConfig;
use kw_audio_core::wav;
use kw_meeting_core::local::LocalWindowProvider;
use kw_meeting_core::{
    AudioChunk, AudioSource, MeetingProviderConfig, MeetingTranscriptionProvider, TranscriptEvent,
};
use kw_network_policy::NetworkPolicy;
use kw_sidecar::{pid::PidStore, port, process, Catalog, HttpHealth, Launch, StartTimings};
use kw_transcription_core::local::{build_args, ServerOptions, WhisperServer, HEALTH_PATH};
use kw_transcription_core::models;
use kw_transcription_core::speech::{FindSpeech, SpeechFinder};

const SPEECH: &[u8] = include_bytes!("../../transcription-core/tests/fixtures/speech-16k-mono.wav");

/// The Silero model the desktop application bundles.
const VAD_MODEL: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../apps/desktop/src-tauri/resources/ggml-silero-v5.1.2.bin"
);

const RATE: usize = 16_000;

/// The shipped windowing.
const WINDOW: f64 = 5.0;

/// Silence before each repeat of the fixture, and after the last.
const GAPS: [f64; 5] = [20.0, 30.0, 2.0, 15.0, 10.0];

/// The fixture's two sentences, in seconds from its start.
const SENTENCES: [(f64, f64); 2] = [(0.11, 2.56), (3.49, 5.50)];

/// How much of a sentence at a window's edge is tried: seconds of speech
/// inside an otherwise silent window.
const EDGES: [f64; 6] = [0.1, 0.25, 0.5, 1.0, 1.5, 2.0];

/// Noise, about 2% of full scale, from a linear congruential generator rather
/// than a crate, so it is the same on every run and every machine.
fn add_noise(samples: &mut [f32], seed: u32) {
    let mut state: u32 = seed;
    for sample in samples {
        state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let noise = (f64::from(state >> 8) / f64::from(1_u32 << 24)).mul_add(2.0, -1.0) * 0.02;
        *sample = (*sample + noise as f32).clamp(-1.0, 1.0);
    }
}

fn fixture() -> Vec<f32> {
    wav::from_pcm(SPEECH.get(44..).expect("a WAV header"))
}

fn at(seconds: f64) -> usize {
    (seconds * RATE as f64).round() as usize
}

/// The recording's samples, and every sentence's true start and end in it.
fn noisy_recording() -> (Vec<f32>, Vec<(f64, f64)>) {
    let speech = fixture();
    let mut out: Vec<f32> = Vec::new();
    let mut truth = Vec::new();
    for (index, gap) in GAPS.iter().enumerate() {
        out.extend(vec![0.0_f32; at(*gap)]);
        if index + 1 < GAPS.len() {
            let offset = out.len() as f64 / RATE as f64;
            truth.extend(SENTENCES.iter().map(|(s, e)| (offset + s, offset + e)));
            out.extend_from_slice(&speech);
        }
    }
    add_noise(&mut out, 0x2545_f491);
    (out, truth)
}

/// Seconds of `window` (start, end) that are speech.
fn speech_inside(start: f64, end: f64, truth: &[(f64, f64)]) -> f64 {
    truth
        .iter()
        .map(|(s, e)| (e.min(end) - s.max(start)).max(0.0))
        .sum()
}

/// A silent five-second window holding `seconds` of a sentence at one edge.
fn edge_window(seconds: f64, speech_at_end: bool) -> Vec<f32> {
    let speech = fixture();
    let silence = vec![0.0_f32; at(WINDOW - seconds)];
    let mut window = if speech_at_end {
        // The opening of the first sentence, just begun as the window ends.
        let start = at(SENTENCES[0].0);
        let mut window = silence;
        window.extend_from_slice(&speech[start..start + at(seconds)]);
        window
    } else {
        // The close of the second sentence, just ended as the window began.
        let end = at(SENTENCES[1].1);
        let mut window = speech[end - at(seconds)..end].to_vec();
        window.extend(silence);
        window
    };
    add_noise(&mut window, 0x0bad_5eed);
    window
}

async fn find(finder: &SpeechFinder, samples: &[f32]) -> (bool, Duration) {
    let started = Instant::now();
    let spans = finder
        .find(&wav::encode_wav(samples, RATE as u32))
        .await
        .expect("the speech finder runs");
    (!spans.is_empty(), started.elapsed())
}

/// The meeting as the capture would send it, one second at a time.
async fn live(
    provider: &mut LocalWindowProvider,
    samples: &[f32],
) -> (Vec<TranscriptEvent>, Duration) {
    provider
        .connect(MeetingProviderConfig {
            provider: "local".to_owned(),
            model: None,
            language: None,
            keyterms: Vec::new(),
            key_ref: None,
            sample_rate: RATE as u32,
        })
        .await
        .unwrap();
    let started = Instant::now();
    for second in samples.chunks(RATE) {
        let pcm = wav::encode_wav(second, RATE as u32)[44..].to_vec();
        provider
            .send_audio(AudioChunk {
                pcm,
                source: AudioSource::Mic,
            })
            .await
            .unwrap();
    }
    let transcript = provider.finish().await.unwrap();
    assert_eq!(transcript.gaps, 0, "a burst is not falling behind");
    (transcript.events, started.elapsed())
}

/// Events from windows with no speech in them: what whisper invented.
fn invented<'a>(events: &'a [TranscriptEvent], truth: &[(f64, f64)]) -> Vec<&'a TranscriptEvent> {
    events
        .iter()
        .filter(|event| speech_inside(event.timestamp, event.timestamp + WINDOW, truth) == 0.0)
        .collect()
}

fn print(label: &str, events: &[TranscriptEvent], truth: &[(f64, f64)], took: Duration) {
    println!(
        "{label}: {} events in {:.2} s",
        events.len(),
        took.as_secs_f64()
    );
    for event in events {
        let speech = speech_inside(event.timestamp, event.timestamp + WINDOW, truth);
        println!(
            "  {:6.1} s  speech {speech:4.2} s  {}{}",
            event.timestamp,
            if event.is_final { "(final) " } else { "" },
            event.text.trim()
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "downloads ~160 MB and starts a real server; run with --ignored"]
async fn silent_windows_are_skipped_and_every_sentence_is_still_heard() {
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
    let finder = SpeechFinder::new(
        tool,
        VAD_MODEL.into(),
        VadConfig::default(),
        root.path().join("scratch"),
    );

    // 1. Speech at a window's edge, in an otherwise silent window.
    println!("speech at a window's edge (seconds of it inside: found at the end, at the start):");
    let mut edges = Vec::new();
    for seconds in EDGES {
        let (at_end, _) = find(&finder, &edge_window(seconds, true)).await;
        let (at_start, _) = find(&finder, &edge_window(seconds, false)).await;
        println!("  {seconds:4.2} s: {at_end}, {at_start}");
        edges.push((seconds, at_end, at_start));
    }

    // 2. Every window of the recording, as the provider cuts it.
    let (recording, truth) = noisy_recording();
    println!(
        "a {:.1} s recording with {} sentences",
        recording.len() as f64 / RATE as f64,
        truth.len()
    );
    let mut feed = WindowFeed::standard();
    feed.push(&recording);
    let mut windows = Vec::new();
    let mut costs = Vec::new();
    while let Some(window) = feed.next_window() {
        let (found, took) = find(&finder, &window.samples).await;
        let start = window.start_seconds;
        windows.push((start, speech_inside(start, start + WINDOW, &truth), found));
        costs.push(took);
    }
    costs.sort();
    println!(
        "{} windows searched: {:.0} ms median, {:.0} ms slowest",
        costs.len(),
        costs[costs.len() / 2].as_secs_f64() * 1000.0,
        costs[costs.len() - 1].as_secs_f64() * 1000.0
    );
    for (start, speech, found) in &windows {
        println!("  {start:6.1} s  speech {speech:4.2} s  found {found}");
    }

    // 3. The live view, through the real server, as the application builds it.
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

    let server = || Arc::new(WhisperServer::new(policy.clone(), chosen, None));

    // For the record, every window sent, as before. Not asserted on: it is
    // what this change moves away from.
    let (before, took) = live(&mut LocalWindowProvider::new(server()), &recording).await;
    print("every window sent", &before, &truth, took);
    println!(
        "  {} of them from windows with no speech",
        invented(&before, &truth).len()
    );

    let mut searching = LocalWindowProvider::new(server()).with_speech_finder(Arc::new(finder));
    let (after, took) = live(&mut searching, &recording).await;
    print("searched first", &after, &truth, took);

    running.stop(&PidStore::new(root.path())).await.unwrap();

    // The overlap means speech missed at one window's edge is inside the
    // next (`local.rs`'s module doc), so nothing is lost; but a finder blind at
    // the edges would show speech a window late. Half a second of a sentence
    // alone at either edge is found; a quarter was, when this was measured.
    for (seconds, at_end, at_start) in edges {
        if seconds >= 0.5 {
            assert!(
                at_end && at_start,
                "{seconds} s of a sentence at a window's edge was not found"
            );
        }
    }
    for (start, speech, found) in &windows {
        if *speech == 0.0 {
            assert!(
                !found,
                "the window at {start} s has no speech and was found to"
            );
        }
    }
    // Speech up to two seconds long lies whole inside some window, and longer
    // speech has at least two seconds inside one.
    for (index, (s, e)) in truth.iter().enumerate() {
        let needed = (e - s).min(2.0) - 0.01;
        assert!(
            windows.iter().any(|(start, _, found)| {
                *found && speech_inside(*start, start + WINDOW, &[(*s, *e)]) >= needed
            }),
            "sentence {index} ({s:.2}-{e:.2} s) has no window found to have speech that holds {needed:.2} s of it"
        );
    }
    let slowest = costs[costs.len() - 1];
    assert!(
        slowest < Duration::from_secs(1),
        "a search took {slowest:?}, against a three-second hop"
    );

    assert!(
        invented(&after, &truth).is_empty(),
        "an event came from a window with no speech in it"
    );
    for (repeat, sentence) in truth.chunks(2).enumerate() {
        let (start, end) = (sentence[0].0, sentence[1].1);
        assert!(
            after.iter().any(|event| {
                event.timestamp < end
                    && event.timestamp + WINDOW > start
                    && event.text.to_lowercase().contains("quick brown fox")
            }),
            "repeat {repeat} ({start:.1}-{end:.1} s) was not heard"
        );
    }
}
