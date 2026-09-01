//! The rules half of the sherpa-onnx streaming client: what to send, what a
//! frame means, and how a cumulative transcript becomes events.
//!
//! [`crate::nemotron_socket`] is the plumbing. Everything here is pure and
//! covered by fixtures, because everything here is a rule that can be wrong in a
//! way a running server would hide.
//!
//! # The protocol was measured, not read
//!
//! `sherpa-onnx-online-websocket-server` v1.13.4 running
//! `nemotron-speech-streaming-en-0.6b`, on 2026-09-01, through
//! `crates/meeting-core/examples/sherpa_ws_probe.rs`. Both directions, both
//! against the real binaries, with the reference client reproducing the model's
//! own `test_wavs/trans.txt` first as the control.
//!
//! Three of the four things below are **not** in the client's `--help`, and one
//! of them contradicts it:
//!
//! | | measured |
//! | --- | --- |
//! | sample format | **`f32` little-endian** |
//! | frame | BINARY, 8000 samples, then a short tail |
//! | client end marker | TEXT `Done` |
//! | server end marker | TEXT `Done!`, and it never closes the socket |
//!
//! `sherpa-onnx-online-websocket-client --help` says *"16-bit samples"*. It is
//! wrong, or at least it describes the file it reads rather than the wire: as
//! `f32` the values are speech inside ±1, and as `i16` they are noise. This is
//! the single fact a client written from the documentation would get wrong, and
//! it would produce a socket that connects, sends, and transcribes nothing.
//!
//! # `text` is cumulative, which is the rule with teeth
//!
//! Every update repeats the whole segment so far, growing. A client that
//! appended would produce *"After After early After early nightfall"*. See
//! [`Segments`], which is the whole of that rule and is tested against the real
//! thirteen-update sequence the probe recorded.
//!
//! # There is no error frame, and that decides how faults are reported
//!
//! Measured deliberately, because [`crate::fault`] exists on account of four
//! clients that swallowed exactly that. `--fault` sent an unparseable TEXT frame
//! and a three-byte BINARY frame before the audio: **the server answered
//! neither, and the transcript came back byte-identical to the clean run.**
//!
//! So this provider has no channel through which to say something is wrong.
//! [`crate::fault::FaultKind::Reported`] is unreachable here and a client
//! claiming to surface "what the provider said" would be describing something
//! that does not exist. A [`crate::fault::SessionFault`] from this client can
//! only ever be [`crate::fault::FaultKind::Transport`] — a refused connect, a
//! dead socket, a read error — or [`crate::fault::FaultKind::Unreadable`], and
//! `no_error_frame_is_reachable_from_this_client` asserts it.
//!
//! # Audio does not leave the machine
//!
//! The server is a child process this application started, bound to loopback by
//! `kw_sidecar::port`. `NetworkPolicy::loopback_websocket` builds the address
//! from a port and nothing else, so there is no way to aim this client at
//! another host — the same argument, and the same signature-level guarantee,
//! that `NetworkPolicy::loopback` already makes for the whisper server.

use std::time::Duration;

use serde_json::Value;

use crate::capabilities::MeetingProviderCapabilities;
use crate::{
    AudioChunk, AudioSource, MeetingError, MeetingProviderConfig, MeetingTranscript,
    TranscriptEvent,
};

/// The provider id. Distinct from `local`, which is the windowed whisper pass.
pub const PROVIDER_ID: &str = "local-streaming";

/// The one rate sherpa's online models are trained at.
pub const SAMPLE_RATE: u32 = 16_000;

/// Samples per binary frame.
///
/// What the reference client was measured sending: 75 frames of exactly this,
/// then a 640-sample tail. Frame boundaries almost certainly do not matter to a
/// server that is accumulating a stream — but "almost certainly" is a belief,
/// and matching the measured size costs nothing, so the only frame this client
/// sends that was never observed on the wire is the final short one, which was.
pub const SAMPLES_PER_FRAME: usize = 8_000;

/// What the client sends to say the audio is over.
pub const CLIENT_DONE: &str = "Done";

/// What the server sends to say it has finished.
///
/// **Not the same string**, and the reference client prints `Done!` as its own
/// last log line too, so it is easy to read this as an echo of the log. It is
/// not; it comes off the wire.
pub const SERVER_DONE: &str = "Done!";

/// Bytes in one `f32`.
const SAMPLE_BYTES: usize = 4;

/// How long to wait for `Done!` after sending `Done`.
///
/// The server does not close the socket, so there is no end-of-stream to wait
/// on: without a deadline `finish` would block for as long as the meeting was
/// long. Sixty seconds is generous for a model that keeps up with real time and
/// is bounded, which is the property that matters.
pub const FINISH_TIMEOUT: Duration = Duration::from_secs(60);

/// 16-bit little-endian PCM as `f32` in [-1, 1).
///
/// The conversion the whole client turns on. A trailing odd byte is dropped
/// rather than padded: half a sample is not a sample, and padding it would put
/// a click in the audio at every chunk boundary.
///
/// That is guarded twice, deliberately -- `chunks_exact` never yields the odd
/// byte, and the `try_from` would reject it if it did. Worth knowing when
/// reading the mutation log: no single-line edit defeats both, so the mutation
/// that proves the test bites has to replace the whole expression.
#[must_use]
pub fn samples(pcm: &[u8]) -> Vec<f32> {
    pcm.chunks_exact(2)
        .filter_map(|pair| <[u8; 2]>::try_from(pair).ok())
        .map(|pair| f32::from(i16::from_le_bytes(pair)) / 32_768.0)
        .collect()
}

/// One binary frame's bytes: `f32` little-endian, no header.
///
/// No header was measured, and none is added. The server reads a stream of
/// floats and the frame is only how they arrive.
#[must_use]
pub fn block_bytes(block: &[f32]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(block.len() * SAMPLE_BYTES);
    for sample in block {
        bytes.extend_from_slice(&sample.to_le_bytes());
    }
    bytes
}

/// Audio waiting for a full frame.
///
/// Holds up to [`SAMPLES_PER_FRAME`] - 1 samples — half a second at 16 kHz —
/// so that every frame but the last is the size that was measured. The buffer
/// is the price of that, and [`Self::drain`] is what stops it becoming a lost
/// half-second at the end of every meeting.
#[derive(Debug, Default)]
pub struct Pending {
    samples: Vec<f32>,
}

impl Pending {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            samples: Vec::new(),
        }
    }

    /// Add audio and take every whole frame it completes.
    pub fn push(&mut self, incoming: &[f32]) -> Vec<Vec<u8>> {
        self.samples.extend_from_slice(incoming);

        let whole = self.samples.len() / SAMPLES_PER_FRAME;
        if whole == 0 {
            return Vec::new();
        }

        let taken = whole * SAMPLES_PER_FRAME;
        let rest = self.samples.split_off(taken);
        let frames = self
            .samples
            .chunks(SAMPLES_PER_FRAME)
            .map(block_bytes)
            .collect();
        self.samples = rest;
        frames
    }

    /// Everything held back, as one short final frame.
    ///
    /// Empty when nothing is waiting, so a caller can send it unconditionally
    /// without checking — an empty binary frame would be a frame this client
    /// has never seen the server given.
    pub fn drain(&mut self) -> Option<Vec<u8>> {
        (!self.samples.is_empty()).then(|| {
            let bytes = block_bytes(&self.samples);
            self.samples.clear();
            bytes
        })
    }

    /// How many samples are waiting. For tests and for a bound to assert.
    #[must_use]
    pub fn held(&self) -> usize {
        self.samples.len()
    }
}

/// What one frame from the server means.
///
/// **There is no `Failed` variant, and that is a measurement rather than an
/// omission.** The other four clients have one because their providers send
/// error frames; this server sends nothing at all for a frame it cannot parse.
/// Adding a variant nothing can produce would suggest a channel exists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Incoming {
    /// A transcript update. `text` is the whole of `segment` so far.
    Update {
        segment: u32,
        text: String,
        /// True on the last update of the stream, where `is_final` and `is_eof`
        /// both turn true together.
        is_final: bool,
    },
    /// `Done!` — the server has finished with everything it was sent.
    Finished,
    /// A frame this client could not read.
    Unreadable { preview: String },
}

/// Read one text frame.
///
/// The JSON carries `text`, `tokens[]`, `timestamps[]`, `ys_probs[]`,
/// `lm_probs[]`, `context_scores[]`, `segment`, `words[]`, `start_time`,
/// `is_final` and `is_eof`. Only four of those are read: the rest are the
/// model's own confidence data, and reading a field this client does not use
/// would be inventing a dependency on it.
#[must_use]
pub fn interpret(frame: &str) -> Incoming {
    if frame.trim() == SERVER_DONE {
        return Incoming::Finished;
    }

    let Ok(value) = serde_json::from_str::<Value>(frame) else {
        return Incoming::Unreadable {
            preview: crate::fault::preview(frame),
        };
    };

    let Some(text) = value.get("text").and_then(Value::as_str) else {
        // Parsed as JSON and carries no transcript. Not a keepalive this client
        // knows and not an error either -- the server has no error frame -- so
        // it is reported as unreadable and `fault::ends_the_session` decides
        // whether that matters, which depends on whether anything has been
        // understood yet.
        return Incoming::Unreadable {
            preview: crate::fault::preview(frame),
        };
    };

    // `segment` numbers the utterances within a stream. Absent is segment zero
    // rather than an error: a server that stopped sending it would still be
    // giving us a transcript, and refusing one over a missing index would be
    // the kind of strictness that loses a meeting.
    let segment = value
        .get("segment")
        .and_then(Value::as_u64)
        .and_then(|n| u32::try_from(n).ok())
        .unwrap_or(0);

    // `is_eof` as well as `is_final`: both turn true on the same frame, and
    // reading only one would depend on which of them the server means.
    let is_final = value
        .get("is_final")
        .and_then(Value::as_bool)
        .unwrap_or(false)
        || value
            .get("is_eof")
            .and_then(Value::as_bool)
            .unwrap_or(false);

    Incoming::Update {
        segment,
        text: text.to_owned(),
        is_final,
    }
}

/// What to do with an update, given everything before it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// Nothing worth showing: the server sent an empty transcript, which is
    /// most of the first frames of an utterance.
    Idle,
    /// A new segment. Whatever event was open before it is settled.
    Begin(String),
    /// The same segment, with more words. **Replace, never append.**
    Revise(String),
}

/// Cumulative updates in, one event per segment out.
///
/// The rule the whole protocol turns on, kept here where it can be tested
/// against the recorded sequence rather than against a live model.
#[derive(Debug, Default)]
pub struct Segments {
    /// The segment the last update belonged to.
    segment: Option<u32>,
    /// Whether an event has been started for it. Separate from `segment`
    /// because a segment's first updates are usually empty, and a `Revise` with
    /// nothing to revise is the bug that separation prevents.
    open: bool,
}

impl Segments {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            segment: None,
            open: false,
        }
    }

    /// Take one update.
    pub fn step(&mut self, segment: u32, text: &str) -> Step {
        if self.segment != Some(segment) {
            self.segment = Some(segment);
            self.open = false;
        }

        let text = text.trim();
        if text.is_empty() {
            return Step::Idle;
        }

        if self.open {
            Step::Revise(text.to_owned())
        } else {
            self.open = true;
            Step::Begin(text.to_owned())
        }
    }
}

/// Check a configuration and answer with what this client honours.
#[must_use = "the capabilities say what the session will actually do"]
pub fn prepare(
    config: &MeetingProviderConfig,
) -> Result<&'static MeetingProviderCapabilities, MeetingError> {
    crate::validate(config)?;
    crate::capabilities(PROVIDER_ID).ok_or_else(|| MeetingError::UnknownProvider {
        id: PROVIDER_ID.to_owned(),
    })
}

/// One transcript event.
#[must_use]
pub fn event(text: String, is_final: bool, timestamp: f64, source: AudioSource) -> TranscriptEvent {
    TranscriptEvent {
        text,
        is_final,
        timestamp,
        // The server labels no speakers. `AudioSource` is what attributes a
        // meeting here, which is why there are two sockets.
        speaker: None,
        source,
    }
}

/// The finished transcript.
///
/// `reconnects` is always zero and `gaps` is always zero, and both are stated
/// rather than left to a default. There is no session limit to reconnect around
/// — the server is a local process — and nothing in this client drops audio: a
/// frame either goes into [`Pending`] or onto the wire.
#[must_use]
pub fn transcript(events: Vec<TranscriptEvent>, duration_seconds: f64) -> MeetingTranscript {
    MeetingTranscript {
        events,
        duration_seconds,
        reconnects: 0,
        gaps: 0,
    }
}

/// The audio for one chunk, ready to buffer.
#[must_use]
pub fn chunk_samples(chunk: &AudioChunk) -> Vec<f32> {
    samples(&chunk.pcm)
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

    /// The first six updates of the real thirteen the probe recorded, verbatim
    /// apart from the arrays this client does not read.
    const RECORDED: [&str; 6] = [
        r#"{"text":"","segment":0,"start_time":0.0,"is_final":false,"is_eof":false}"#,
        r#"{"text":"AFTER","segment":0,"start_time":0.0,"is_final":false,"is_eof":false}"#,
        r#"{"text":"AFTER EARLY","segment":0,"start_time":0.0,"is_final":false,"is_eof":false}"#,
        r#"{"text":"AFTER EARLY NIGHTFALL","segment":0,"start_time":0.0,"is_final":false,"is_eof":false}"#,
        r#"{"text":"AFTER EARLY NIGHTFALL THE YELLOW LAMPS","segment":0,"start_time":0.0,"is_final":false,"is_eof":false}"#,
        r#"{"text":"AFTER EARLY NIGHTFALL THE YELLOW LAMPS WOULD LIGHT UP","segment":0,"start_time":0.0,"is_final":true,"is_eof":true}"#,
    ];

    #[test]
    fn pcm_becomes_f32_in_range() {
        // The conversion the client turns on, and the one the reference
        // client's own --help gets wrong.
        let pcm: Vec<u8> = [0_i16, 32_767, -32_768, -1]
            .iter()
            .flat_map(|s| s.to_le_bytes())
            .collect();
        let converted = samples(&pcm);
        assert_eq!(converted.len(), 4);
        assert!((converted[0] - 0.0).abs() < f32::EPSILON);
        assert!(converted[1] > 0.999 && converted[1] < 1.0);
        assert!((converted[2] + 1.0).abs() < f32::EPSILON);
        assert!(converted.iter().all(|s| (-1.0..1.0).contains(s)));
    }

    #[test]
    fn an_odd_trailing_byte_is_dropped_rather_than_padded() {
        // Padding would put a click at the boundary of every chunk whose length
        // happened to be odd.
        assert_eq!(samples(&[0, 0, 7]).len(), 1);
        assert!(samples(&[7]).is_empty());
    }

    #[test]
    fn a_block_is_little_endian_f32_with_no_header() {
        let bytes = block_bytes(&[1.0, -1.0]);
        assert_eq!(bytes.len(), 8, "no header, four bytes a sample");
        assert_eq!(&bytes[..4], &1.0_f32.to_le_bytes());
        assert_eq!(&bytes[4..], &(-1.0_f32).to_le_bytes());
    }

    #[test]
    fn frames_are_the_measured_size_and_nothing_is_lost() {
        let mut pending = Pending::new();

        // Two and a bit frames' worth, arriving in awkward pieces.
        let mut sent = 0_usize;
        let mut frames: Vec<Vec<u8>> = Vec::new();
        for _ in 0..25 {
            let piece = vec![0.5_f32; 700];
            sent += piece.len();
            frames.extend(pending.push(&piece));
        }
        assert_eq!(sent, 17_500);
        assert_eq!(frames.len(), 2, "two whole frames, not three");
        assert!(
            frames
                .iter()
                .all(|f| f.len() == SAMPLES_PER_FRAME * SAMPLE_BYTES),
            "every frame is the measured size"
        );
        assert_eq!(pending.held(), 1_500);

        let tail = pending.drain().expect("the remainder is not dropped");
        assert_eq!(tail.len(), 1_500 * SAMPLE_BYTES);
        assert_eq!(pending.held(), 0);
        assert_eq!(
            frames.iter().map(Vec::len).sum::<usize>() + tail.len(),
            sent * SAMPLE_BYTES,
            "every sample handed in reached the wire"
        );
    }

    #[test]
    fn nothing_pending_drains_to_nothing() {
        // An empty binary frame is a frame this client has never seen the
        // server given, so `drain` must not produce one.
        let mut pending = Pending::new();
        assert!(pending.drain().is_none());
        assert!(pending.push(&[]).is_empty());
    }

    #[test]
    fn the_buffer_never_holds_more_than_one_frame() {
        // The bound, asserted rather than argued: a chunk far larger than a
        // frame must leave less than a frame behind, not accumulate.
        let mut pending = Pending::new();
        for _ in 0..5 {
            pending.push(&vec![0.1_f32; SAMPLES_PER_FRAME * 4 + 13]);
            assert!(
                pending.held() < SAMPLES_PER_FRAME,
                "held {} samples",
                pending.held()
            );
        }
    }

    #[test]
    fn the_servers_end_marker_is_read_and_it_is_not_the_clients() {
        assert_eq!(interpret(SERVER_DONE), Incoming::Finished);
        assert_eq!(interpret("Done!\n"), Incoming::Finished);
        // The client's own marker is a different string, and reading it as the
        // server's would end a session the moment it started.
        assert_ne!(interpret(CLIENT_DONE), Incoming::Finished);
    }

    #[test]
    fn a_recorded_update_is_read() {
        match interpret(RECORDED[1]) {
            Incoming::Update {
                segment,
                text,
                is_final,
            } => {
                assert_eq!(segment, 0);
                assert_eq!(text, "AFTER");
                assert!(!is_final);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn the_last_update_is_final() {
        match interpret(RECORDED[5]) {
            Incoming::Update { is_final, .. } => assert!(is_final),
            other => panic!("{other:?}"),
        }
        // Either flag alone is enough. They were measured turning true
        // together, and depending on which one the server means would be a
        // guess about a thing that was never separated.
        let eof_only = r#"{"text":"x","segment":0,"is_final":false,"is_eof":true}"#;
        match interpret(eof_only) {
            Incoming::Update { is_final, .. } => assert!(is_final),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_frame_this_client_cannot_read_says_so_and_does_not_claim_the_server_complained() {
        // The distinction `fault::FaultKind` draws, and the reason there is no
        // `Failed` variant here at all.
        match interpret("{ not json") {
            Incoming::Unreadable { preview } => assert!(preview.contains("not json"), "{preview}"),
            other => panic!("{other:?}"),
        }
        // Valid JSON with no transcript in it is also unreadable rather than an
        // error: this server has no way to report one.
        match interpret(r#"{"hello":"world"}"#) {
            Incoming::Unreadable { .. } => {}
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn cumulative_text_replaces_and_never_appends() {
        // The whole recorded sequence, and the assertion is that thirteen
        // updates produce one event rather than thirteen.
        let mut segments = Segments::new();
        let mut steps = Vec::new();
        for frame in RECORDED {
            match interpret(frame) {
                Incoming::Update { segment, text, .. } => {
                    steps.push(segments.step(segment, &text));
                }
                other => panic!("{other:?}"),
            }
        }

        assert_eq!(steps[0], Step::Idle, "the empty first update shows nothing");
        assert_eq!(steps[1], Step::Begin("AFTER".to_owned()));
        assert!(
            steps[2..].iter().all(|s| matches!(s, Step::Revise { .. })),
            "{steps:?}"
        );

        let Step::Revise(last) = &steps[5] else {
            panic!("{:?}", steps[5]);
        };
        assert_eq!(
            last,
            "AFTER EARLY NIGHTFALL THE YELLOW LAMPS WOULD LIGHT UP"
        );
        assert!(
            !last.contains("AFTER EARLY AFTER"),
            "appending would produce a stutter, and this is the test that catches it"
        );
    }

    #[test]
    fn a_new_segment_begins_a_new_event() {
        let mut segments = Segments::new();
        assert_eq!(segments.step(0, "one"), Step::Begin("one".to_owned()));
        assert_eq!(
            segments.step(0, "one two"),
            Step::Revise("one two".to_owned())
        );
        assert_eq!(segments.step(1, "three"), Step::Begin("three".to_owned()));
        assert_eq!(
            segments.step(1, "three four"),
            Step::Revise("three four".to_owned())
        );
    }

    #[test]
    fn an_empty_update_never_opens_an_event_to_revise() {
        // The separation between `segment` and `open` earns its keep here: a
        // segment whose first updates are empty must still `Begin` when words
        // arrive, or the socket would be told to revise an event that was never
        // pushed.
        let mut segments = Segments::new();
        assert_eq!(segments.step(3, ""), Step::Idle);
        assert_eq!(segments.step(3, "   "), Step::Idle);
        assert_eq!(segments.step(3, "words"), Step::Begin("words".to_owned()));
    }

    #[test]
    fn a_segment_that_goes_backwards_is_still_a_new_segment() {
        // Never observed. Handled as a new segment rather than ignored,
        // because the alternative is a `Revise` aimed at an event that belongs
        // to a different utterance.
        let mut segments = Segments::new();
        assert_eq!(segments.step(4, "later"), Step::Begin("later".to_owned()));
        assert_eq!(
            segments.step(1, "earlier"),
            Step::Begin("earlier".to_owned())
        );
    }

    #[test]
    fn no_error_frame_is_reachable_from_this_client() {
        // The measurement, asserted as a property of the type. `--fault` sent
        // an unparseable TEXT frame and a three-byte BINARY frame and the
        // server answered neither, so nothing this parser can return may mean
        // "the provider said something was wrong".
        //
        // If a future sherpa release grows an error frame, this test is what
        // has to be deleted deliberately -- rather than a `Failed` arm quietly
        // never firing.
        for frame in [
            "{ not json",
            r#"{"error":"anything"}"#,
            r#"{"code":500,"message":"boom"}"#,
            "",
        ] {
            match interpret(frame) {
                Incoming::Unreadable { .. } => {}
                other => panic!("{frame:?} produced {other:?}"),
            }
        }
    }

    #[test]
    fn the_transcript_states_its_zeroes_rather_than_defaulting_them() {
        let out = transcript(Vec::new(), 12.5);
        assert_eq!(
            out.reconnects, 0,
            "there is no session limit to rebuild around"
        );
        assert_eq!(out.gaps, 0, "nothing in this client drops audio");
        assert!((out.duration_seconds - 12.5).abs() < f64::EPSILON);
    }

    #[test]
    fn a_config_at_the_wrong_rate_is_refused_before_a_socket_opens() {
        let mut config = MeetingProviderConfig {
            provider: PROVIDER_ID.to_owned(),
            model: None,
            language: None,
            keyterms: Vec::new(),
            key_ref: None,
            sample_rate: 48_000,
        };
        match prepare(&config) {
            Err(MeetingError::SampleRate { accepted, .. }) => {
                assert_eq!(accepted, &[SAMPLE_RATE]);
            }
            other => panic!("{other:?}"),
        }

        config.sample_rate = SAMPLE_RATE;
        let caps = prepare(&config).expect("the declared rate is accepted");
        assert!(!caps.audio_leaves_device, "the server is a local process");
    }
}
