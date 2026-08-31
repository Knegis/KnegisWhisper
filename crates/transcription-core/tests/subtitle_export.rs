//! A long recording, through the real server, out as a real subtitle file.
//!
//! `#[ignore]`d, because it downloads about 160 MB and starts a real server:
//!
//! ```text
//! cargo test -p kw-transcription-core --test subtitle_export -- --ignored --nocapture
//! ```
//!
//! # What this exists to catch
//!
//! A 3,401-second import exported as **one** subtitle cue at `00:00:00`. The
//! cause was measured on 2026-08-31: the server ran with `--no-timestamps`, so
//! `verbose_json` returned a single segment carrying neither `start` nor `end`,
//! and there was nothing to divide. `2fa5331` replaced that flag with
//! [`kw_transcription_core::local::MAX_SEGMENT_CHARS`].
//!
//! Six unit tests cover [`kw_transcription_core::local::interpret`] against the
//! two measured response bodies. None of them starts a server, and none of them
//! writes a subtitle file — so none could tell you whether the flags this build
//! passes still produce timings, or whether those timings survive
//! `kw_notes_core::transcript::export`. Both are only answerable against the
//! real thing, and the second one nearly did not survive: `merge` overwrites a
//! segment's `end_timestamp` with its own start, and `export_cues` takes every
//! cue's end from the *next* cue's start, which is what rescues it.
//!
//! # The recording
//!
//! Built at run time from the same 6.3-second fixture `local_dictation.rs` uses,
//! repeated with silence between, rather than committing a two-minute WAV.
//! Deterministic, carries nobody's voice, and the expected cue count is known
//! from the construction rather than from whatever the model felt like doing.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::collections::BTreeMap;
use std::time::Duration;

use kw_network_policy::NetworkPolicy;
use kw_notes_core::transcript::{export, Format, NoteMeta, Segment, SpeakerLabels};
use kw_sidecar::{pid::PidStore, port, process, Catalog, HttpHealth, Launch, StartTimings};
use kw_transcription_core::local::{
    build_args, Inference, LocalTranscriber, ServerOptions, WhisperServer, HEALTH_PATH,
};
use kw_transcription_core::models;

const SPEECH: &[u8] = include_bytes!("fixtures/speech-16k-mono.wav");

/// How many times the fixture is repeated.
const UTTERANCES: usize = 10;

/// Silence between repeats, in seconds.
///
/// Well above `kw_notes_core::transcript`'s two-second merge window, which
/// compares one segment's start to the previous segment's start. The fixture is
/// 6.3 s long, so even a zero-length gap would clear it -- but a real pause is
/// what makes whisper end a segment in the first place, and this test is about
/// segment boundaries.
const GAP_SECONDS: f64 = 1.5;

/// 16 kHz mono 16-bit, which is what the fixture is and what whisper wants.
const RATE: usize = 16_000;

/// A long recording: the fixture, repeated, with silence between.
fn long_recording() -> Vec<u8> {
    // The fixture is a canonical 44-byte-header WAV written by
    // `kw_audio_core::wav::encode_wav`. Everything after that is samples.
    let pcm = SPEECH.get(44..).expect("the fixture has a WAV header");
    let samples = kw_audio_core::wav::from_pcm(pcm);
    assert!(!samples.is_empty(), "the fixture decoded to nothing");

    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let gap = vec![0.0_f32; (GAP_SECONDS * RATE as f64) as usize];

    let mut out = Vec::with_capacity((samples.len() + gap.len()) * UTTERANCES);
    for index in 0..UTTERANCES {
        if index > 0 {
            out.extend_from_slice(&gap);
        }
        out.extend_from_slice(&samples);
    }

    #[allow(clippy::cast_precision_loss)]
    let seconds = out.len() as f64 / RATE as f64;
    println!("built a {seconds:.1} s recording from {UTTERANCES} repeats");
    assert!(seconds > 60.0, "the point is that it is long: {seconds}");

    kw_audio_core::wav::encode_wav(&out, u32::try_from(RATE).unwrap())
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "downloads ~160 MB and starts a real server; run with --ignored"]
async fn a_long_recording_exports_as_many_cues_not_one() {
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
    let options = ServerOptions {
        model_path: model.path.display().to_string(),
        port: chosen,
        language: None,
        threads: None,
        gpu_device: None,
        vad: None,
    };

    // The shipped arguments, not a hand-written list. That is the whole point:
    // if `--max-len` ever goes back to `--no-timestamps`, this test is what
    // notices, and it can only notice by asking `build_args`.
    let args = build_args(&options);
    assert!(
        !args.iter().any(|a| a == "--no-timestamps"),
        "the flag that removed every timing is back: {args:?}"
    );

    let running = process::start(
        spec,
        &Launch {
            executable: installed.executable.clone(),
            args,
            port: chosen,
        },
        &PidStore::new(root.path()),
        &HttpHealth::new(policy.clone(), HEALTH_PATH),
        StartTimings {
            timeout: Duration::from_secs(90),
            poll_interval: Duration::from_millis(100),
        },
    )
    .await
    .expect("whisper-server should start and answer");

    // `.timed()`: the request media import makes, and the only one that asks
    // for `verbose_json`.
    let server = WhisperServer::new(policy.clone(), chosen, Some("tiny".to_owned())).timed();
    let result = server
        .transcribe(long_recording(), None, None)
        .await
        .expect("the request should reach the server");

    let segments = match result {
        Inference::Timed { segments, text } => {
            println!("{} segments for {} characters", segments.len(), text.len());
            for segment in &segments {
                println!(
                    "  {:7.2} -> {:7.2}  {}",
                    segment.start, segment.end, segment.text
                );
            }
            segments
        }
        // The defect, as a failure. Before `2fa5331` this was `Text`, because
        // the server had been told not to produce timings.
        other => panic!("expected timings from a `.timed()` request, got {other:?}"),
    };

    assert!(
        segments.len() >= UTTERANCES,
        "{UTTERANCES} spoken utterances should not collapse into {} segments",
        segments.len()
    );

    // Ascending, and each end at or after its own start.
    for pair in segments.windows(2) {
        assert!(
            pair[1].start >= pair[0].start,
            "segments out of order: {pair:?}"
        );
    }
    for segment in &segments {
        assert!(segment.end >= segment.start, "backwards: {segment:?}");
    }

    // The timings must span the recording rather than bunch at the start, which
    // is what one cue at `00:00:00` looked like.
    let last = segments.last().unwrap();
    assert!(
        last.start > 60.0,
        "the last segment starts at {:.2} s in a recording over a minute long",
        last.start
    );

    /* --------------------------------------------- and out as a subtitle file */

    let note = NoteMeta {
        title: "A long import".to_owned(),
        created_at: None,
        participants: None,
    };
    let as_segments: Vec<Segment> = segments
        .iter()
        .map(|s| Segment {
            text: s.text.clone(),
            timestamp: Some(s.start),
            end_timestamp: Some(s.end),
            ..Segment::default()
        })
        .collect();

    let srt = export(
        Format::Srt,
        &note,
        &as_segments,
        &BTreeMap::new(),
        &SpeakerLabels::default(),
    );
    println!("--- SRT ---\n{srt}");

    // Cue numbering: "1" through at least UTTERANCES, each on its own line.
    let cue_count = srt.lines().filter(|line| line.contains(" --> ")).count();
    assert!(
        cue_count >= UTTERANCES,
        "expected at least {UTTERANCES} cues, got {cue_count}:\n{srt}"
    );

    // No zero-length cue. `merge` overwrites `end_timestamp` with the segment's
    // own start, so every cue's end has to come from the next cue's start --
    // and the last one from a fixed tail. A cue reading
    // `00:00:00,000 --> 00:00:00,000` is that going wrong.
    for line in srt.lines().filter(|line| line.contains(" --> ")) {
        let (start, end) = line.split_once(" --> ").unwrap();
        assert_ne!(start.trim(), end.trim(), "a zero-length cue in:\n{srt}");
    }

    // And the file must reach the end of the recording, not stop near zero.
    let last_cue = srt
        .lines()
        .filter(|line| line.contains(" --> "))
        .next_back()
        .unwrap();
    assert!(
        !last_cue.starts_with("00:00:0"),
        "the last cue starts in the first ten seconds: {last_cue}"
    );

    running.stop(&PidStore::new(root.path())).await.ok();
}

/// The same claim for the other local engine, against an installed model.
///
/// Skipped unless `KW_SHERPA_MODEL_DIR` points at an extracted Parakeet model,
/// because installing one is a 478 MB download and this is the one test where
/// paying it would buy nothing: `sherpa::cues` is already covered by six unit
/// tests over the real captured stdout of a live run. What those cannot tell
/// you is whether a *long* recording still produces sane cue boundaries -- the
/// 13.5 s measurement they are built from has three utterances, and the
/// 200-character backstop never fires in it.
///
/// ```text
/// KW_SHERPA_MODEL_DIR=".../sherpa-onnx-nemo-parakeet-unified-en-0.6b-int8-non-streaming" ///   cargo test -p kw-transcription-core --test subtitle_export -- --ignored --nocapture
/// ```
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs an installed Parakeet model; run with --ignored"]
async fn a_long_recording_exports_as_many_cues_through_parakeet() {
    let Ok(model_dir) = std::env::var("KW_SHERPA_MODEL_DIR") else {
        println!("skipped: set KW_SHERPA_MODEL_DIR to an extracted Parakeet model directory");
        return;
    };
    let Ok(executable) = std::env::var("KW_SHERPA_EXECUTABLE") else {
        println!("skipped: set KW_SHERPA_EXECUTABLE to sherpa-onnx-offline");
        return;
    };

    let model_dir = std::path::PathBuf::from(model_dir);
    let files = kw_transcription_core::sherpa::discover_transducer_files(&model_dir)
        .expect("the measured Parakeet layout");

    let temp = tempfile::tempdir().unwrap();
    let engine = kw_transcription_core::sherpa::SherpaOffline::new(
        std::path::PathBuf::from(executable),
        files,
        Some("parakeet-unified-en-0.6b".to_owned()),
        temp.path().to_path_buf(),
        None,
    );

    let result = engine
        .transcribe(long_recording(), None, None)
        .await
        .expect("the CLI should run");

    let segments = match result {
        Inference::Timed { segments, .. } => {
            println!("{} segments", segments.len());
            for segment in &segments {
                println!(
                    "  {:7.2} -> {:7.2}  {}",
                    segment.start, segment.end, segment.text
                );
            }
            segments
        }
        other => panic!("expected timings from per-token onsets, got {other:?}"),
    };

    assert!(
        segments.len() >= UTTERANCES,
        "{UTTERANCES} spoken utterances should not collapse into {} segments",
        segments.len()
    );
    for segment in &segments {
        assert!(segment.end >= segment.start, "backwards: {segment:?}");
        assert!(
            segment.text.chars().count() <= 260,
            "a cue far past the backstop: {segment:?}"
        );
    }
    for pair in segments.windows(2) {
        assert!(
            pair[1].start >= pair[0].end,
            "cues overlap, which the punctuation-onset rule was meant to stop: {pair:?}"
        );
    }

    let last = segments.last().unwrap();
    assert!(
        last.start > 60.0,
        "the last segment starts at {:.2} s in a recording over a minute long",
        last.start
    );

    let note = NoteMeta {
        title: "A long Parakeet import".to_owned(),
        created_at: None,
        participants: None,
    };
    let as_segments: Vec<Segment> = segments
        .iter()
        .map(|s| Segment {
            text: s.text.clone(),
            timestamp: Some(s.start),
            end_timestamp: Some(s.end),
            ..Segment::default()
        })
        .collect();

    let srt = export(
        Format::Srt,
        &note,
        &as_segments,
        &BTreeMap::new(),
        &SpeakerLabels::default(),
    );
    println!(
        "--- SRT ---
{srt}"
    );

    let cue_count = srt.lines().filter(|line| line.contains(" --> ")).count();
    assert!(
        cue_count >= UTTERANCES,
        "expected at least {UTTERANCES} cues, got {cue_count}:
{srt}"
    );
}
