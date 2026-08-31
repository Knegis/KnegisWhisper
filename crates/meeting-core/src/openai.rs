//! OpenAI Realtime transcription over a WebSocket.
//!
//! # Three things that are not like the others
//!
//! **24 kHz only.** Every other provider here takes 16. The capability table
//! says so and [`crate::validate`] enforces it, and the meeting pipeline
//! resamples per session because of this provider — it used to hardcode 16 kHz,
//! which meant this client could never have connected at all.
//!
//! **A sixty-minute session cap.** The socket is closed by the server at the
//! hour, so a meeting longer than that must reconnect, and the transcript must
//! say it did. [`MeetingTranscript::reconnects`] is that count. This is the only
//! client where that path is reachable, which makes it the only one where a live
//! test can prove the reconnect works.
//!
//! **Keyterms are accepted and ignored.** Upstream includes them in this
//! provider's connection cache key, which implies they matter. They do not, so
//! nothing here sends them and the capability entry says `Unsupported`. Sending
//! them anyway would look like support to anybody reading the wire.
//!
//! # What is verified
//!
//! Request shaping, the event grammar, and the reconnect arithmetic, against the
//! fixtures below. **No live call in the test suite** — that needs a key, and
//! `docs/PLATFORM_SUPPORT.md` records whether one has been used by hand.
//!
//! # The wire, briefly
//!
//! Connect to `wss://api.openai.com/v1/realtime?intent=transcription` with a
//! bearer token and no beta header -- see [`BETA_HEADER`]. Send a
//! `session.update` naming the model and the audio format, then
//! `input_audio_buffer.append` frames carrying base64 PCM16. Transcript arrives
//! as `conversation.item.input_audio_transcription.delta` (partial) and
//! `.completed` (final).

use std::sync::Arc;
use std::time::{Duration, Instant};

use base64::Engine as _;
use kw_network_policy::RequestPurpose;
use kw_security::{CredentialStore, KeyRef};
use serde_json::{json, Value};

use crate::capabilities::{capabilities, MeetingProviderCapabilities};
use crate::{
    AudioChunk, AudioSource, MeetingError, MeetingProviderConfig, MeetingTranscript,
    TranscriptEvent,
};

/// The provider id and its capability entry.
pub const PROVIDER_ID: &str = "openai-realtime";

/// Where the socket goes.
///
/// `intent=transcription` selects the transcription-only session, which is a
/// different thing from the conversational Realtime API: no model responses, no
/// audio out, and no risk of being billed for generation nobody asked for.
pub const ENDPOINT: &str = "wss://api.openai.com/v1/realtime?intent=transcription";

/// The default transcription model.
pub const DEFAULT_MODEL: &str = "gpt-4o-transcribe";

/// The purpose this client's traffic travels under.
///
/// Meeting audio to a provider the user configured. The brief fixes the number of
/// purposes at seven, and a socket does not make this a different reason.
pub const PURPOSE: RequestPurpose = RequestPurpose::UserConfiguredTranscriptionProvider;

/// The beta opt-in header this client used to send, kept only to say why it
/// does not any more.
///
/// Written from documentation that said the header was required and that
/// omitting it produced a 400 with no useful body. That stopped being true.
/// The first live use of this client, on a real meeting on 2026-08-28, was
/// closed by the server immediately on both sockets:
///
/// ```text
/// The Realtime Beta API is no longer supported. Please use /v1/realtime for
/// the GA API.
/// invalid_request_error.beta_api_shape_disabled (code 4000)
/// ```
///
/// The endpoint was already `/v1/realtime`, so the header was the thing
/// selecting the retired shape. It is no longer sent.
///
/// **Removing it was right, and confirmed live minutes later**: the socket
/// opened and stayed open. It did rename `transcription_session.update`, which
/// the same attempt reported as
/// `Invalid value: 'tra...ate'. Supported values are: 'session.update', ...` --
/// fixed in [`session_update`], along with `session.type`, which GA requires on
/// the session object even though the endpoint already carries
/// `intent=transcription`. Each of those arrived as a named parameter in the
/// server's own words, one live attempt at a time.
pub const BETA_HEADER: (&str, &str) = ("OpenAI-Beta", "realtime=v1");

/// How long before the server's cap to reconnect on purpose.
///
/// Two minutes. Reconnecting *before* being closed means the gap falls where
/// this client chooses rather than mid-sentence, and two minutes is enough slack
/// that a slow handshake does not overrun the cap anyway.
const RECONNECT_MARGIN: Duration = Duration::from_secs(120);

/// The session configuration message, sent once per connection.
///
/// `pcm16` at the rate agreed in `connect`. OpenAI documents 24 kHz for this
/// format and the capability table declares only that, so a mismatch is a bug
/// here rather than something to negotiate.
#[must_use]
pub fn session_update(model: &str, language: Option<&str>, sample_rate: u32) -> Value {
    // Built as a map rather than indexed into, because this workspace denies
    // indexing in library code -- `Value`'s Index panics on a type mismatch, and
    // "it is an object, I just made it" is exactly the reasoning that lint exists
    // to refuse.
    let mut transcription = serde_json::Map::new();
    transcription.insert("model".to_owned(), json!(model));
    // Sent only when the user chose one. This provider detects language, and
    // pinning it to the wrong one is worse than letting it decide.
    if let Some(language) = language.filter(|l| !l.trim().is_empty()) {
        transcription.insert("language".to_owned(), json!(language));
    }
    let transcription = Value::Object(transcription);

    // The GA session payload, arrived at one server message at a time. Four
    // live attempts named, in order: the beta header, the frame type, the
    // missing `session.type`, and then
    // `Unknown parameter: 'session.input_audio_format'.`
    //
    // That last one is why the three audio fields move together rather than one
    // per round trip. Up to that point the server had objected only to names,
    // so changing one thing at a time was both cheapest and most readable. It
    // then said a field that had always been at the top of `session` is not
    // known there at all, which is a different kind of answer: the family has
    // moved, and `input_audio_transcription` and `turn_detection` are its
    // siblings. Relocating one and leaving two behind would buy two more
    // meetings' worth of round trips to learn what this one already implies.
    //
    // **Inferred as a group, and not verified.** `audio.input` with `format`,
    // `transcription` and `turn_detection` under it is the shape this is
    // written against. If any part is wrong the server names the parameter, as
    // it has every time so far.
    json!({
        "type": "session.update",
        "session": {
            // Required by the GA API, which answered the version without it
            // with `Missing required parameter: 'session.type'.` The endpoint
            // already carries `intent=transcription`; GA wants it named on the
            // session object as well, and the two must agree -- this is the
            // transcription-only session, not the conversational one.
            "type": "transcription",
            "audio": {
                "input": {
                    // The rate is the session's own, already checked against
                    // the capability table by `validate`, rather than a
                    // constant that could disagree with the audio being sent.
                    "format": { "type": "audio/pcm", "rate": sample_rate },
                    "transcription": transcription,
                    // Server-side voice activity detection, so the server
                    // decides where an utterance ends. The alternative is
                    // committing buffers from here, which would put a second
                    // opinion about sentence boundaries in a client that cannot
                    // hear.
                    "turn_detection": { "type": "server_vad" },
                }
            }
        }
    })
}

/// One frame of audio, base64 as the protocol requires.
#[must_use]
pub fn append_audio(pcm: &[u8]) -> Value {
    json!({
        "type": "input_audio_buffer.append",
        "audio": base64::engine::general_purpose::STANDARD.encode(pcm),
    })
}

/// What a server message means to us.
#[derive(Debug, Clone, PartialEq)]
pub enum Incoming {
    /// Transcript, with whether the server considers it settled.
    Transcript { text: String, is_final: bool },
    /// The session is configured and audio may flow.
    Ready,
    /// The server reported an error. Carried rather than logged, because a
    /// meeting silently producing no transcript is the failure to avoid.
    Failed { message: String },
    /// Something this client does not act on. Named rather than ignored so a new
    /// event type shows up in a log instead of vanishing.
    Other { kind: String },
}

/// Read one server message.
///
/// Pure, so every shape is covered without a socket.
#[must_use]
pub fn interpret(frame: &str) -> Incoming {
    let Ok(value) = serde_json::from_str::<Value>(frame) else {
        return Incoming::Failed {
            message: "the server sent something that is not JSON".to_owned(),
        };
    };

    let kind = value
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned();

    match kind.as_str() {
        "conversation.item.input_audio_transcription.delta" => {
            // `delta` on a partial, `transcript` on a completed one. Two field
            // names for one idea, which is the sort of thing a wire fixture is
            // for.
            let text = value
                .get("delta")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned();
            Incoming::Transcript {
                text,
                is_final: false,
            }
        }
        "conversation.item.input_audio_transcription.completed" => Incoming::Transcript {
            text: value
                .get("transcript")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
            is_final: true,
        },
        // Both spellings. The GA API renamed the session events along with the
        // update frame; accepting the beta pair as well costs nothing and means
        // a server that still speaks either is understood rather than having
        // its readiness fall through to `Other`.
        "session.created"
        | "session.updated"
        | "transcription_session.created"
        | "transcription_session.updated" => Incoming::Ready,
        "error" => Incoming::Failed {
            message: value
                .get("error")
                .and_then(|e| e.get("message"))
                .and_then(Value::as_str)
                .unwrap_or("the server reported an error with no message")
                .to_owned(),
        },
        other => Incoming::Other {
            kind: other.to_owned(),
        },
    }
}

/// Session bookkeeping: when to reconnect, and how often it has happened.
///
/// Separated from the socket for the usual reason — this is the part with a rule
/// in it, and a rule that only runs against a live sixty-minute call is a rule
/// nobody checks.
#[derive(Debug)]
pub struct SessionClock {
    limit: Duration,
    opened: Instant,
    reconnects: u32,
}

impl SessionClock {
    /// Start the clock for a provider with `limit`.
    #[must_use]
    pub fn new(limit: Option<Duration>, now: Instant) -> Self {
        Self {
            // No cap declared means never reconnect on a timer.
            limit: limit.unwrap_or(Duration::MAX),
            opened: now,
            reconnects: 0,
        }
    }

    /// Whether the socket should be replaced before the server closes it.
    #[must_use]
    pub fn should_reconnect(&self, now: Instant) -> bool {
        if self.limit == Duration::MAX {
            return false;
        }
        now.duration_since(self.opened) + RECONNECT_MARGIN >= self.limit
    }

    /// Record that a new socket was opened.
    pub fn reconnected(&mut self, now: Instant) {
        self.opened = now;
        self.reconnects = self.reconnects.saturating_add(1);
    }

    /// How many times the session has been rebuilt.
    #[must_use]
    pub const fn reconnects(&self) -> u32 {
        self.reconnects
    }
}

/// Resolve the credential for this provider.
///
/// Returns the secret only at the moment of use, from the store, so no part of
/// the meeting configuration holds one — the same discipline the transcription
/// route follows.
pub fn token(
    store: &Arc<dyn CredentialStore>,
    key_ref: Option<&KeyRef>,
) -> Result<kw_security::Secret, MeetingError> {
    let key_ref = key_ref.ok_or_else(|| MeetingError::MissingCredential {
        slot: "openai".to_owned(),
    })?;

    store
        .get(key_ref)
        .map_err(|_| MeetingError::MissingCredential {
            slot: key_ref.slot().to_owned(),
        })
}

/// Check a configuration before anything is opened.
pub fn prepare(
    config: &MeetingProviderConfig,
) -> Result<&'static MeetingProviderCapabilities, MeetingError> {
    if config.provider != PROVIDER_ID {
        return Err(MeetingError::UnknownProvider {
            id: config.provider.clone(),
        });
    }

    let caps = capabilities(PROVIDER_ID).ok_or_else(|| MeetingError::UnknownProvider {
        id: PROVIDER_ID.to_owned(),
    })?;

    if !caps.sample_rates.contains(&config.sample_rate) {
        return Err(MeetingError::SampleRate {
            provider: PROVIDER_ID.to_owned(),
            offered: config.sample_rate,
            accepted: caps.sample_rates,
        });
    }

    Ok(caps)
}

/// Turn a chunk into the frame that carries it.
#[must_use]
pub fn frame_for(chunk: &AudioChunk) -> Value {
    append_audio(&chunk.pcm)
}

/// Assemble the transcript from what the socket produced.
///
/// `source` is carried from the chunk that was being sent, because this provider
/// does not label speakers — `speaker_labels` is false in the table — so
/// attribution comes from which capture the audio came from and nowhere else.
#[must_use]
pub fn transcript(
    events: Vec<TranscriptEvent>,
    duration_seconds: f64,
    reconnects: u32,
) -> MeetingTranscript {
    MeetingTranscript {
        events,
        duration_seconds,
        reconnects,
        // Nothing is dropped by this client: the socket either carries a frame
        // or the send fails and the session ends.
        gaps: 0,
    }
}

/// One transcript event, at `timestamp` seconds into the session.
#[must_use]
pub fn event(text: String, is_final: bool, timestamp: f64, source: AudioSource) -> TranscriptEvent {
    TranscriptEvent {
        text,
        is_final,
        timestamp,
        speaker: None,
        source,
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

    fn config(rate: u32) -> MeetingProviderConfig {
        MeetingProviderConfig {
            provider: PROVIDER_ID.to_owned(),
            model: None,
            language: None,
            keyterms: vec!["Knegis".to_owned()],
            key_ref: KeyRef::new("openai"),
            sample_rate: rate,
        }
    }

    /* ------------------------------------------------------ what is sent */

    #[test]
    fn the_endpoint_asks_for_a_transcription_session_and_passes_the_boundary() {
        // `intent=transcription` is not decoration: without it this is the
        // conversational Realtime API, which answers back and bills for
        // generation nobody asked for.
        assert!(ENDPOINT.contains("intent=transcription"));
        assert!(kw_network_policy::validate_websocket(ENDPOINT, PURPOSE).is_ok());
    }

    /// The one rate this provider declares, so the fixtures agree with the
    /// capability table rather than inventing a second answer.
    const RATE: u32 = 24_000;

    #[test]
    fn the_session_update_names_the_format_and_the_model() {
        let message = session_update(DEFAULT_MODEL, None, RATE);
        assert_eq!(message["type"], "session.update");
        // Required, and separate from the endpoint's `intent=transcription`.
        assert_eq!(message["session"]["type"], "transcription");

        let input = &message["session"]["audio"]["input"];
        assert_eq!(input["format"]["type"], "audio/pcm");
        assert_eq!(input["format"]["rate"], RATE);
        assert_eq!(input["transcription"]["model"], DEFAULT_MODEL);
        assert_eq!(input["turn_detection"]["type"], "server_vad");

        // The beta spellings are gone rather than sent alongside. GA answered
        // `Unknown parameter: 'session.input_audio_format'`, so leaving it in
        // would be sending a field the server has said it does not know.
        assert!(message["session"].get("input_audio_format").is_none());
        assert!(message["session"]
            .get("input_audio_transcription")
            .is_none());
        assert!(message["session"].get("turn_detection").is_none());
    }

    #[test]
    fn the_rate_sent_is_the_session_rate_not_a_constant() {
        // A format that disagrees with the audio is worse than one that is
        // merely wrong: the server would decode it and produce nonsense.
        for rate in [16_000, 24_000, 48_000] {
            let message = session_update(DEFAULT_MODEL, None, rate);
            assert_eq!(message["session"]["audio"]["input"]["format"]["rate"], rate);
        }
    }

    #[test]
    fn no_language_is_sent_unless_one_was_chosen() {
        // This provider detects language. Pinning it to the wrong one is worse
        // than letting it decide, so an absent or blank choice sends nothing.
        for absent in [None, Some(""), Some("   ")] {
            let message = session_update(DEFAULT_MODEL, absent, RATE);
            assert!(
                message["session"]["audio"]["input"]["transcription"]
                    .get("language")
                    .is_none(),
                "{absent:?} should not send a language"
            );
        }

        let message = session_update(DEFAULT_MODEL, Some("sv"), RATE);
        assert_eq!(
            message["session"]["audio"]["input"]["transcription"]["language"],
            "sv"
        );
    }

    #[test]
    fn keyterms_are_never_sent_because_this_provider_ignores_them() {
        // The capability table says Unsupported. Sending them anyway would look
        // like support to anybody reading the wire, and upstream putting them in
        // this provider's cache key is exactly that mistake.
        let rendered = serde_json::to_string(&session_update(DEFAULT_MODEL, None, RATE)).unwrap();
        assert!(!rendered.contains("Knegis"));
        assert!(!rendered.contains("keyterm"));
        assert!(!rendered.contains("prompt"));
    }

    #[test]
    fn audio_is_base64_pcm_under_the_documented_field() {
        let frame = append_audio(&[0x01, 0x02, 0xff, 0xfe]);
        assert_eq!(frame["type"], "input_audio_buffer.append");
        assert_eq!(frame["audio"], "AQL//g==");
    }

    #[test]
    fn a_chunk_becomes_the_frame_that_carries_it() {
        let chunk = AudioChunk {
            pcm: vec![0x10, 0x20],
            source: AudioSource::Mic,
        };
        assert_eq!(frame_for(&chunk), append_audio(&[0x10, 0x20]));
    }

    /* ---------------------------------------------------- what is read */

    #[test]
    fn a_partial_and_a_final_use_different_field_names() {
        // Two names for one idea, which is what a fixture is for.
        let partial = interpret(
            r#"{"type":"conversation.item.input_audio_transcription.delta","delta":"the quick"}"#,
        );
        assert_eq!(
            partial,
            Incoming::Transcript {
                text: "the quick".to_owned(),
                is_final: false
            }
        );

        let complete = interpret(
            r#"{"type":"conversation.item.input_audio_transcription.completed",
                "transcript":"the quick brown fox"}"#,
        );
        assert_eq!(
            complete,
            Incoming::Transcript {
                text: "the quick brown fox".to_owned(),
                is_final: true
            }
        );
    }

    #[test]
    fn the_session_being_ready_is_recognised() {
        for frame in [
            r#"{"type":"session.created"}"#,
            r#"{"type":"session.updated"}"#,
            r#"{"type":"transcription_session.created"}"#,
            r#"{"type":"transcription_session.updated"}"#,
        ] {
            assert_eq!(interpret(frame), Incoming::Ready, "{frame}");
        }
    }

    #[test]
    fn a_server_error_is_carried_rather_than_swallowed() {
        // A meeting silently producing no transcript is the failure to avoid.
        let incoming = interpret(
            r#"{"type":"error","error":{"type":"invalid_request_error",
                "message":"Invalid audio format"}}"#,
        );
        match incoming {
            Incoming::Failed { message } => assert_eq!(message, "Invalid audio format"),
            other => panic!("{other:?}"),
        }

        // And an error with no message still reports as an error.
        match interpret(r#"{"type":"error"}"#) {
            Incoming::Failed { message } => assert!(!message.is_empty()),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn something_unrecognised_is_named_rather_than_ignored() {
        // So a protocol change shows up in a log instead of vanishing.
        match interpret(r#"{"type":"rate_limits.updated","rate_limits":[]}"#) {
            Incoming::Other { kind } => assert_eq!(kind, "rate_limits.updated"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_frame_that_is_not_json_is_a_failure_and_not_a_transcript() {
        match interpret("<html>502 Bad Gateway</html>") {
            Incoming::Failed { message } => assert!(message.contains("not JSON")),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_transcript_with_no_text_field_is_empty_rather_than_a_panic() {
        assert_eq!(
            interpret(r#"{"type":"conversation.item.input_audio_transcription.delta"}"#),
            Incoming::Transcript {
                text: String::new(),
                is_final: false
            }
        );
    }

    /* -------------------------------------------------- the session cap */

    #[test]
    fn the_socket_is_replaced_before_the_server_closes_it() {
        // Reconnecting first means the gap falls where this client chooses rather
        // than mid-sentence. The only client where this path is reachable, which
        // is why it is arithmetic and not a comment.
        let start = Instant::now();
        let hour = Duration::from_secs(3_600);
        let clock = SessionClock::new(Some(hour), start);

        assert!(!clock.should_reconnect(start));
        assert!(!clock.should_reconnect(start + Duration::from_secs(3_000)));
        // 58 minutes: inside the two-minute margin.
        assert!(clock.should_reconnect(start + Duration::from_secs(3_480)));
        assert!(clock.should_reconnect(start + hour));
    }

    #[test]
    fn reconnecting_restarts_the_clock_and_is_counted() {
        // The count reaches MeetingTranscript::reconnects, because a transcript
        // with a gap in it must not look complete.
        let start = Instant::now();
        let mut clock = SessionClock::new(Some(Duration::from_secs(3_600)), start);
        assert_eq!(clock.reconnects(), 0);

        let later = start + Duration::from_secs(3_500);
        assert!(clock.should_reconnect(later));
        clock.reconnected(later);

        assert_eq!(clock.reconnects(), 1);
        assert!(!clock.should_reconnect(later), "the clock restarted");
    }

    #[test]
    fn a_provider_with_no_cap_never_reconnects_on_a_timer() {
        let start = Instant::now();
        let clock = SessionClock::new(None, start);
        assert!(!clock.should_reconnect(start + Duration::from_secs(86_400)));
    }

    /* ----------------------------------------------------- configuration */

    #[test]
    fn twenty_four_kilohertz_is_the_only_rate_it_takes() {
        // The reason the meeting pipeline resamples per session. It used to
        // hardcode 16 kHz, so this client could never have connected at all.
        assert!(prepare(&config(24_000)).is_ok());

        match prepare(&config(16_000)).unwrap_err() {
            MeetingError::SampleRate {
                offered, accepted, ..
            } => {
                assert_eq!(offered, 16_000);
                assert_eq!(accepted, &[24_000]);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn another_providers_configuration_is_refused() {
        // Fail closed. Deepgram's configuration must not be sent to OpenAI just
        // because this is the client that exists.
        let mut cfg = config(24_000);
        cfg.provider = "deepgram-realtime".to_owned();
        match prepare(&cfg).unwrap_err() {
            MeetingError::UnknownProvider { id } => assert_eq!(id, "deepgram-realtime"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_missing_credential_names_the_slot_rather_than_failing_obscurely() {
        let store: Arc<dyn CredentialStore> = Arc::new(kw_security::MemoryCredentialStore::new());

        match token(&store, None).unwrap_err() {
            MeetingError::MissingCredential { slot } => assert_eq!(slot, "openai"),
            other => panic!("{other:?}"),
        }

        let key = KeyRef::new("openai").unwrap();
        match token(&store, Some(&key)).unwrap_err() {
            MeetingError::MissingCredential { slot } => assert_eq!(slot, "openai"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_stored_credential_is_resolved_only_at_the_point_of_use() {
        use kw_security::Secret;

        let store: Arc<dyn CredentialStore> = Arc::new(kw_security::MemoryCredentialStore::new());
        let key = KeyRef::new("openai").unwrap();
        store
            .set(&key, &Secret::new("sk-not-a-real-key".to_owned()))
            .unwrap();

        let resolved = token(&store, Some(&key)).unwrap();
        assert_eq!(resolved.expose(), "sk-not-a-real-key");

        // And the configuration that reached this point never held it.
        let rendered = format!("{:?}", config(24_000));
        assert!(rendered.contains("openai"), "the slot name is safe to show");
        assert!(!rendered.contains("sk-"));
    }

    #[test]
    fn the_transcript_reports_reconnects_and_no_gaps() {
        let events = vec![event("hello".to_owned(), true, 1.5, AudioSource::System)];
        let transcript = transcript(events, 90.0, 2);

        assert_eq!(transcript.reconnects, 2);
        assert_eq!(
            transcript.gaps, 0,
            "this client either sends a frame or the session ends"
        );
        assert_eq!(transcript.events[0].source, AudioSource::System);
        assert!(
            transcript.events[0].speaker.is_none(),
            "it labels no speakers"
        );
    }

    #[test]
    fn the_capability_entry_agrees_with_this_module() {
        let caps = capabilities(PROVIDER_ID).unwrap();
        assert!(caps.audio_leaves_device);
        assert!(!caps.keyterms.is_supported(), "and none are sent");
        assert_eq!(caps.session_limit, Some(Duration::from_secs(3_600)));
        assert!(!caps.token_rotation, "the bearer token does not rotate");
        assert!(!caps.speaker_labels);
    }
}
