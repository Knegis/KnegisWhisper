//! AssemblyAI Realtime transcription over a WebSocket.
//!
//! # Two sockets, one per capture — the same shape as OpenAI Realtime
//!
//! The plan this project started from argued for one socket, on the theory
//! that AssemblyAI's speaker labels make a second connection pointless. That
//! reasoning is overridden: [`crate::TranscriptEvent::source`] is not
//! optional, and `Settings > Meetings` renders "You" and "Everyone else" from
//! it. A speaker label says *who within a stream*, never *which stream* — so
//! one socket carrying both the microphone and the system output would make
//! `source` a guess on every event, which is the precise defect
//! [`crate::openai_socket`]'s two sockets exist to avoid. A label read from
//! this provider's wire is carried into [`crate::TranscriptEvent::speaker`] as
//! an addition to `source`, never a replacement for it. See
//! [`crate::assemblyai_socket`] for where the two connections live.
//!
//! # What is verified
//!
//! `keyterms_prompt` as a JSON-encoded array in a query parameter, and the
//! hundred-term cap, come from this project's own audit
//! (`docs/INTEGRATION_PARITY.md`) and from the `assemblyai-realtime` row of
//! [`crate::capabilities::PROVIDERS`], both written before this client and
//! independent of it. The query construction and the cap are tested against
//! that contract below.
//!
//! # What is not verified
//!
//! No AssemblyAI key exists on this development machine, no request has been
//! made, and no vendor source for this wire exists anywhere in this
//! repository to check against — this is a clean-room client built from
//! public documentation. `docs/PLATFORM_SUPPORT.md` records that plainly.
//! Everything below this line is **believed, not confirmed**, and each item is
//! marked again at its use so a reader does not have to hold this list in
//! their head:
//!
//! - The endpoint path, `wss://streaming.assemblyai.com/v3/ws`.
//! - The query parameter names `sample_rate`, `encoding` and `language_code`,
//!   and the encoding value `pcm_s16le`.
//! - That the credential travels as the raw API key in an `Authorization`
//!   header with no `Bearer` prefix — the convention AssemblyAI's REST API is
//!   known to use, assumed here to carry over to this endpoint.
//! - The message envelope: a `type` field taking `"Begin"`, `"Turn"`,
//!   `"Termination"` or `"Error"`.
//! - The per-turn field names `transcript`, `end_of_turn` and `speaker` —
//!   including whether a streaming turn carries a speaker label **at all**.
//!   The capability table declares `speaker_labels: true` for this provider;
//!   this client reads a `speaker` field if the wire carries one and leaves
//!   [`crate::TranscriptEvent::speaker`] `None` if it does not. Both outcomes
//!   are safe: a meeting is still attributed correctly by
//!   [`crate::AudioSource`] either way, so a wrong guess here degrades to
//!   exactly what OpenAI Realtime already reports, never to a misattributed
//!   transcript.
//! - The graceful-termination message `{"type":"Terminate"}`, and the shape of
//!   an error frame, `{"type":"Error","error":"..."}`. If AssemblyAI instead
//!   signals an error only through a WebSocket close code, this client will
//!   read that as an ordinary close rather than as a distinct failure — see
//!   [`crate::assemblyai_socket`].
//!
//! Audio leaves as raw binary WebSocket frames, not JSON-plus-base64 the way
//! OpenAI Realtime's does — the one piece of this wire believed with more
//! confidence than the rest, since it matches AssemblyAI's documented batch
//! upload convention and is the kind of asymmetry `docs/PORTING_MATRIX.md`
//! exists to keep visible rather than flatten into a uniform shape.

use std::sync::Arc;

use kw_network_policy::RequestPurpose;
use kw_security::{CredentialStore, KeyRef};
use serde_json::{json, Value};
use url::Url;

use crate::capabilities::{capabilities, MeetingProviderCapabilities};
use crate::{
    AudioChunk, AudioSource, MeetingError, MeetingProviderConfig, MeetingTranscript,
    TranscriptEvent,
};

/// The provider id and its capability entry.
pub const PROVIDER_ID: &str = "assemblyai-realtime";

/// Where the socket goes.
///
/// UNVERIFIED — see the module documentation.
pub const ENDPOINT: &str = "wss://streaming.assemblyai.com/v3/ws";

/// The purpose this client's traffic travels under.
pub const PURPOSE: RequestPurpose = RequestPurpose::UserConfiguredTranscriptionProvider;

/// The header the handshake carries the credential in.
///
/// UNVERIFIED — the raw API key with no scheme prefix, not a bearer token.
/// See the module documentation.
pub const AUTH_HEADER: &str = "Authorization";

/// PCM16 little-endian: the only encoding this client asks for, matching the
/// format [`AudioChunk::pcm`] already documents.
///
/// UNVERIFIED — see the module documentation.
const ENCODING: &str = "pcm_s16le";

/// Build the connection URL: sample rate, encoding, an optional language, and
/// — if any survive the provider's cap — keyterms as a JSON-encoded array, all
/// as query parameters.
///
/// Everything the session needs travels here rather than in a message sent
/// after the handshake the way OpenAI Realtime's `session_update` is, which is
/// itself an assumption — see the module documentation.
///
/// The keyterm cap is read from `caps` rather than a second constant in this
/// file, so there is exactly one place that number is declared: the same
/// capability table [`crate::ConfigReview`] already uses to predict this same
/// truncation, so the screen's claim and the wire stay in agreement.
pub fn endpoint(
    sample_rate: u32,
    language: Option<&str>,
    keyterms: &[String],
    caps: &MeetingProviderCapabilities,
) -> Result<String, MeetingError> {
    let mut url = Url::parse(ENDPOINT).map_err(|err| MeetingError::Rejected {
        provider: PROVIDER_ID.to_owned(),
        detail: format!("the endpoint template did not parse: {err}"),
    })?;

    {
        let mut query = url.query_pairs_mut();
        query.append_pair("sample_rate", &sample_rate.to_string());
        query.append_pair("encoding", ENCODING);
    }

    // Sent only when the user chose one. This provider cannot detect a
    // language on its own -- `language_detection: false` in the capability
    // table -- and `validate` refuses to start a session with neither, so by
    // the time this runs `language` should be concrete; this still guards
    // against a blank value rather than sending one.
    if let Some(language) = language.filter(|l| !l.trim().is_empty()) {
        url.query_pairs_mut().append_pair("language_code", language);
    }

    let cap = caps.keyterms.effective_count(keyterms.len());
    let capped: Vec<&str> = keyterms.iter().take(cap).map(String::as_str).collect();
    if !capped.is_empty() {
        // `unwrap_or_default` rather than `unwrap`: a `Vec<&str>` cannot fail
        // to serialise, but this workspace denies unwrapping in library code
        // regardless of how sure a call site is.
        let prompt = serde_json::to_string(&capped).unwrap_or_default();
        url.query_pairs_mut()
            .append_pair("keyterms_prompt", &prompt);
    }

    Ok(url.to_string())
}

/// What a server message means to us.
#[derive(Debug, Clone, PartialEq)]
pub enum Incoming {
    /// The session is open and audio may flow.
    Ready,
    /// Transcript for one turn: whether the provider considers the turn ended,
    /// and who the provider says is speaking, if the wire carried a label.
    Transcript {
        text: String,
        is_final: bool,
        speaker: Option<String>,
    },
    /// The server reported an error. Carried rather than logged, because a
    /// meeting silently producing no transcript is the failure to avoid.
    Failed { message: String },
    /// Something this client does not act on. Named rather than ignored so a
    /// new event type shows up in a log instead of vanishing.
    Other { kind: String },
}

/// Read one server message.
///
/// Pure, so every shape is covered without a socket. The envelope itself (a
/// `type` field naming `Begin`/`Turn`/`Termination`/`Error`) and the `speaker`
/// field are unverified — see the module documentation.
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
        "Begin" => Incoming::Ready,
        "Turn" => {
            let text = value
                .get("transcript")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned();
            let is_final = value
                .get("end_of_turn")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            // Read if the wire carries one, `None` if it does not -- both are
            // safe, because `AudioSource` attributes the event either way. See
            // the module documentation on why this field's presence at all is
            // unverified.
            let speaker = value
                .get("speaker")
                .and_then(Value::as_str)
                .map(str::to_owned);
            Incoming::Transcript {
                text,
                is_final,
                speaker,
            }
        }
        "Error" => Incoming::Failed {
            message: value
                .get("error")
                .and_then(Value::as_str)
                .unwrap_or("the server reported an error with no message")
                .to_owned(),
        },
        other => Incoming::Other {
            kind: other.to_owned(),
        },
    }
}

/// The message that ends a session gracefully, sent before the closing frame
/// so the server can report final usage rather than seeing a plain drop.
///
/// UNVERIFIED — see the module documentation. Sending it costs nothing if the
/// server does not recognise it: the close frame that follows ends the
/// session either way.
#[must_use]
pub fn terminate() -> Value {
    json!({ "type": "Terminate" })
}

/// Resolve the credential for this provider.
///
/// Returns the secret only at the moment of use, from the store, so no part
/// of the meeting configuration holds one.
pub fn token(
    store: &Arc<dyn CredentialStore>,
    key_ref: Option<&KeyRef>,
) -> Result<kw_security::Secret, MeetingError> {
    let key_ref = key_ref.ok_or_else(|| MeetingError::MissingCredential {
        slot: "assemblyai".to_owned(),
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

/// The frame that carries one chunk of audio: the raw bytes, sent as a binary
/// WebSocket message rather than wrapped in JSON. See the module
/// documentation on why that is believed rather than confirmed.
#[must_use]
pub fn frame_bytes(chunk: &AudioChunk) -> &[u8] {
    &chunk.pcm
}

/// Assemble the transcript from what the socket produced.
///
/// `reconnects` is always zero: this provider declares no `session_limit`, so
/// there is no clock deciding to rebuild the sockets, and no `SessionClock`
/// exists in this module for that reason.
#[must_use]
pub fn transcript(events: Vec<TranscriptEvent>, duration_seconds: f64) -> MeetingTranscript {
    MeetingTranscript {
        events,
        duration_seconds,
        reconnects: 0,
        // Nothing is dropped by this client: the socket either carries a
        // frame or the send fails and the session ends.
        gaps: 0,
    }
}

/// One transcript event, at `timestamp` seconds into the session.
#[must_use]
pub fn event(
    text: String,
    is_final: bool,
    speaker: Option<String>,
    timestamp: f64,
    source: AudioSource,
) -> TranscriptEvent {
    TranscriptEvent {
        text,
        is_final,
        timestamp,
        speaker,
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
    use std::collections::HashMap;

    use super::*;
    use crate::{KeytermSupport, KeytermWire};

    fn caps() -> &'static MeetingProviderCapabilities {
        capabilities(PROVIDER_ID).unwrap()
    }

    fn config(rate: u32) -> MeetingProviderConfig {
        MeetingProviderConfig {
            provider: PROVIDER_ID.to_owned(),
            model: None,
            language: Some("en".to_owned()),
            keyterms: vec!["Knegis".to_owned()],
            key_ref: KeyRef::new("assemblyai"),
            sample_rate: rate,
        }
    }

    /// The query parameters of a built endpoint, as a map, so a test can ask
    /// for one by name instead of parsing the string itself.
    fn query_of(url: &str) -> HashMap<String, String> {
        Url::parse(url)
            .unwrap()
            .query_pairs()
            .map(|(k, v)| (k.into_owned(), v.into_owned()))
            .collect()
    }

    /* ------------------------------------------------------ what is sent */

    #[test]
    fn the_endpoint_carries_the_sample_rate_and_encoding_and_passes_the_boundary() {
        let url = endpoint(16_000, None, &[], caps()).unwrap();
        assert!(kw_network_policy::validate_websocket(&url, PURPOSE).is_ok());

        let query = query_of(&url);
        assert_eq!(query.get("sample_rate").map(String::as_str), Some("16000"));
        assert_eq!(query.get("encoding").map(String::as_str), Some("pcm_s16le"));
    }

    #[test]
    fn no_language_is_sent_unless_one_was_chosen() {
        for absent in [None, Some(""), Some("   ")] {
            let url = endpoint(16_000, absent, &[], caps()).unwrap();
            assert!(
                !query_of(&url).contains_key("language_code"),
                "{absent:?} should not send a language"
            );
        }

        let url = endpoint(16_000, Some("sv"), &[], caps()).unwrap();
        assert_eq!(
            query_of(&url).get("language_code").map(String::as_str),
            Some("sv")
        );
    }

    #[test]
    fn no_keyterms_means_no_query_parameter_at_all() {
        let url = endpoint(16_000, None, &[], caps()).unwrap();
        assert!(!query_of(&url).contains_key("keyterms_prompt"));
    }

    #[test]
    fn keyterms_within_the_cap_are_sent_in_full() {
        let terms = vec!["Knegis".to_owned(), "sprint review".to_owned()];
        let url = endpoint(16_000, None, &terms, caps()).unwrap();
        let prompt = query_of(&url).remove("keyterms_prompt").unwrap();
        let sent: Vec<String> = serde_json::from_str(&prompt).unwrap();
        assert_eq!(sent, terms);
    }

    #[test]
    fn keyterms_over_the_cap_are_truncated_to_match_what_config_review_predicts() {
        // The capability table caps this provider at 100 and truncates
        // silently on the wire -- `ConfigReview` already predicts this
        // truncation, and the client must match it rather than send more or
        // fewer.
        let terms: Vec<String> = (0..150).map(|i| format!("term{i}")).collect();
        let url = endpoint(16_000, None, &terms, caps()).unwrap();
        let prompt = query_of(&url).remove("keyterms_prompt").unwrap();
        let sent: Vec<String> = serde_json::from_str(&prompt).unwrap();

        assert_eq!(sent.len(), 100, "matches caps.keyterms.effective_count");
        assert_eq!(sent[0], "term0");
        assert_eq!(sent[99], "term99");
    }

    #[test]
    fn a_chunk_becomes_the_raw_bytes_it_carries() {
        // No JSON, no base64 -- the opposite convention from OpenAI Realtime.
        let chunk = AudioChunk {
            pcm: vec![0x10, 0x20, 0xff, 0x00],
            source: AudioSource::Mic,
        };
        assert_eq!(frame_bytes(&chunk), &[0x10, 0x20, 0xff, 0x00]);
    }

    /* ---------------------------------------------------- what is read */

    #[test]
    fn the_session_being_ready_is_recognised() {
        assert_eq!(
            interpret(r#"{"type":"Begin","id":"sess-1","expires_at":1234}"#),
            Incoming::Ready
        );
    }

    #[test]
    fn a_partial_and_a_final_turn_are_told_apart_by_end_of_turn() {
        let partial = interpret(r#"{"type":"Turn","transcript":"the quick","end_of_turn":false}"#);
        assert_eq!(
            partial,
            Incoming::Transcript {
                text: "the quick".to_owned(),
                is_final: false,
                speaker: None,
            }
        );

        let complete =
            interpret(r#"{"type":"Turn","transcript":"the quick brown fox","end_of_turn":true}"#);
        assert_eq!(
            complete,
            Incoming::Transcript {
                text: "the quick brown fox".to_owned(),
                is_final: true,
                speaker: None,
            }
        );
    }

    #[test]
    fn a_speaker_label_on_a_turn_is_carried_through() {
        let incoming =
            interpret(r#"{"type":"Turn","transcript":"hello","end_of_turn":true,"speaker":"A"}"#);
        match incoming {
            Incoming::Transcript { speaker, .. } => assert_eq!(speaker, Some("A".to_owned())),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_turn_with_no_speaker_field_leaves_it_none_rather_than_guessing() {
        let incoming = interpret(r#"{"type":"Turn","transcript":"hello","end_of_turn":true}"#);
        match incoming {
            Incoming::Transcript { speaker, .. } => assert_eq!(speaker, None),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_server_error_is_carried_rather_than_swallowed() {
        let incoming = interpret(r#"{"type":"Error","error":"invalid sample rate"}"#);
        match incoming {
            Incoming::Failed { message } => assert_eq!(message, "invalid sample rate"),
            other => panic!("{other:?}"),
        }

        match interpret(r#"{"type":"Error"}"#) {
            Incoming::Failed { message } => assert!(!message.is_empty()),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_frame_this_client_does_not_act_on_lands_in_other_rather_than_a_transcript() {
        // `Termination` is a real message this provider is believed to send at
        // the end of a session -- see the module documentation -- and is
        // exactly the shape that must not be mistaken for a transcript.
        match interpret(r#"{"type":"Termination","audio_duration_seconds":12.3}"#) {
            Incoming::Other { kind } => assert_eq!(kind, "Termination"),
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
    fn a_turn_with_no_transcript_field_is_empty_rather_than_a_panic() {
        assert_eq!(
            interpret(r#"{"type":"Turn","end_of_turn":false}"#),
            Incoming::Transcript {
                text: String::new(),
                is_final: false,
                speaker: None,
            }
        );
    }

    /* ----------------------------------------------------- configuration */

    #[test]
    fn sixteen_kilohertz_is_the_only_rate_it_takes() {
        assert!(prepare(&config(16_000)).is_ok());

        match prepare(&config(48_000)).unwrap_err() {
            MeetingError::SampleRate {
                offered, accepted, ..
            } => {
                assert_eq!(offered, 48_000);
                assert_eq!(accepted, &[16_000]);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn another_providers_configuration_is_refused() {
        // Fail closed. Deepgram's configuration must not be sent to
        // AssemblyAI just because this is a client that exists.
        let mut cfg = config(16_000);
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
            MeetingError::MissingCredential { slot } => assert_eq!(slot, "assemblyai"),
            other => panic!("{other:?}"),
        }

        let key = KeyRef::new("assemblyai").unwrap();
        match token(&store, Some(&key)).unwrap_err() {
            MeetingError::MissingCredential { slot } => assert_eq!(slot, "assemblyai"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_stored_credential_is_resolved_only_at_the_point_of_use() {
        use kw_security::Secret;

        let store: Arc<dyn CredentialStore> = Arc::new(kw_security::MemoryCredentialStore::new());
        let key = KeyRef::new("assemblyai").unwrap();
        store
            .set(&key, &Secret::new("aai-not-a-real-key".to_owned()))
            .unwrap();

        let resolved = token(&store, Some(&key)).unwrap();
        assert_eq!(resolved.expose(), "aai-not-a-real-key");

        // And the configuration that reached this point never held it.
        let rendered = format!("{:?}", config(16_000));
        assert!(
            rendered.contains("assemblyai"),
            "the slot name is safe to show"
        );
        assert!(!rendered.contains("aai-"));
    }

    #[test]
    fn the_transcript_reports_no_reconnects_and_no_gaps() {
        let events = vec![event(
            "hello".to_owned(),
            true,
            Some("A".to_owned()),
            1.5,
            AudioSource::System,
        )];
        let transcript = transcript(events, 90.0);

        assert_eq!(
            transcript.reconnects, 0,
            "no session_limit means no clock deciding to reconnect"
        );
        assert_eq!(
            transcript.gaps, 0,
            "this client either sends a frame or the session ends"
        );
        assert_eq!(transcript.events[0].source, AudioSource::System);
        assert_eq!(transcript.events[0].speaker.as_deref(), Some("A"));
    }

    #[test]
    fn the_capability_entry_agrees_with_this_module() {
        let caps = caps();
        assert!(caps.audio_leaves_device);
        assert_eq!(caps.sample_rates, &[16_000]);
        assert!(caps.speaker_labels, "the wire may carry a speaker field");
        assert!(
            !caps.language_detection,
            "a concrete language must be given"
        );
        assert_eq!(
            caps.session_limit, None,
            "no clock to reconnect around, and no SessionClock in this module"
        );
        match caps.keyterms {
            KeytermSupport::Supported { max, wire } => {
                assert_eq!(max, Some(100));
                assert_eq!(wire, KeytermWire::JsonQueryParam);
            }
            other => panic!("{other:?}"),
        }
    }
}
