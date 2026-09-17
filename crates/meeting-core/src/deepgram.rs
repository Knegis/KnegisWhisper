//! Deepgram Realtime transcription over a WebSocket.
//!
//! # Two sockets, one per capture — the same shape as OpenAI Realtime and AssemblyAI
//!
//! [`crate::TranscriptEvent::source`] is not optional, and `Settings > Meetings`
//! renders "You" and "Everyone else" from it. A speaker label says *who within a
//! stream*, never *which stream* — so one socket carrying both the microphone and
//! the system output would make `source` a guess on every event. Deepgram's
//! diarization is additional information within a capture, not a replacement for
//! knowing which capture spoke: a label read from this provider's wire is carried
//! into [`crate::TranscriptEvent::speaker`] as an addition to `source`, never a
//! replacement for it. See [`crate::deepgram_socket`] for where the two
//! connections live.
//!
//! # Keyterms are a repeated query parameter
//!
//! `keyterm=a&keyterm=b`, built with [`url::Url::query_pairs_mut`] rather than
//! string concatenation, so a term containing a space, an ampersand, an equals
//! sign or a quote cannot alter the query or inject a parameter of its own. The
//! application limits vocabulary to 100 terms and 400 UTF-8 bytes. Shared
//! preparation applies these limits and omits hints for incompatible models.
//!
//! # Token rotation: declared, and honestly not implemented
//!
//! `token_rotation: true` is the only such entry in
//! [`crate::capabilities::PROVIDERS`], and it describes upstream's practice, not
//! this client's. Upstream minted short-lived temporary tokens from its own hosted
//! token-issuance service and refreshed them mid-session because a minted token
//! expires. This application never mints one: [`token`] resolves the user's own
//! stored API key, unchanged, at the moment a socket is opened, and there is
//! nothing about a stored API key that expires on a schedule. Consequently there is
//! no timer, clock, or background task anywhere in this module or
//! [`crate::deepgram_socket`] that re-reads or refreshes it — see
//! [`crate::deepgram_socket`]'s module documentation and the tests at the bottom of
//! this file and that one for the tripwire that keeps it that way. Implementing a
//! refresh loop that re-reads an unchanging key would be ceremony that looks like a
//! safeguard and is not one.
//!
//! # What is verified
//!
//! The credential slot name (`deepgram`, in `config/provider-registry.json`), the
//! three declared sample rates, and the `RepeatedQueryParam` keyterm wire shape
//! come from this project's own capability table
//! ([`crate::capabilities::PROVIDERS`]), written before this client and independent
//! of it. The query construction is tested against that contract below.
//!
//! # What is not verified
//!
//! No Deepgram key exists on this development machine, no request has been made,
//! and no vendor source for this wire exists anywhere in this repository to check
//! against — this is a clean-room client built from public documentation, the same
//! position [`crate::assemblyai`] was in. `docs/PLATFORM_SUPPORT.md` records that
//! plainly. Everything below this line is **believed, not confirmed**, and each
//! item is marked again at its use so a reader does not have to hold this list in
//! their head:
//!
//! - The endpoint path, `wss://api.deepgram.com/v1/listen`.
//! - The query parameter names `encoding` (value `linear16` for PCM16
//!   little-endian), `sample_rate`, `channels`, `language`, `detect_language` and
//!   `diarize`, and the repeated `keyterm` parameter itself.
//! - That the credential travels as `Token <key>` in an `Authorization` header —
//!   Deepgram's documented convention for its REST and streaming APIs alike,
//!   assumed here to carry over unchanged to this endpoint.
//! - The message envelope: a `type` field naming `"Results"`, `"Metadata"` or
//!   `"Error"`, the `is_final` field on a `Results` message, and the transcript
//!   text living at `channel.alternatives[0].transcript`.
//! - That a `Results` message with no `Metadata` message preceding it is normal —
//!   unlike AssemblyAI's `Begin` and OpenAI's `transcription_session.created`,
//!   Deepgram is believed to need no explicit "ready" message before audio may
//!   flow, so [`Incoming::Ready`] is wired but nothing in
//!   [`crate::deepgram_socket`] waits for it before returning from `connect`.
//! - Diarization is per-word, at `channel.alternatives[0].words[0].speaker`, as a
//!   number rather than a letter the way AssemblyAI's is assumed to be. This client
//!   reads the *first* word's speaker as representative of the whole piece of
//!   transcript in that message; a message whose words span two speakers — a very
//!   short interruption caught mid-utterance — would report only the first
//!   speaker, and [`AudioSource`] still attributes the event to the right capture
//!   either way, so a wrong guess here degrades exactly the way it does for
//!   AssemblyAI: never to a misattributed transcript, only to a possibly-imprecise
//!   speaker label.
//! - The shape of an error frame, `{"type":"Error","message":"..."}` (falling back
//!   to a `description` field if `message` is absent). If Deepgram instead signals
//!   an error only through a WebSocket close code, this client will read that as an
//!   ordinary close rather than as a distinct failure — see
//!   [`crate::deepgram_socket`].
//! - The graceful-termination message, `{"type":"CloseStream"}`, documented as the
//!   way to tell Deepgram no more audio is coming so it can flush and send a final
//!   `Metadata` message before the socket closes.
//!
//! Audio leaves as raw binary WebSocket frames, not JSON-plus-base64 the way
//! OpenAI Realtime's does — matching AssemblyAI's assumed convention and Deepgram's
//! documented one for raw linear PCM, and believed with more confidence than the
//! rest of this list for that reason.

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
pub const PROVIDER_ID: &str = "deepgram-realtime";

/// Where the socket goes.
///
/// UNVERIFIED — see the module documentation.
pub const ENDPOINT: &str = "wss://api.deepgram.com/v1/listen";

/// The purpose this client's traffic travels under.
pub const PURPOSE: RequestPurpose = RequestPurpose::UserConfiguredTranscriptionProvider;

/// The header the handshake carries the credential in.
pub const AUTH_HEADER: &str = "Authorization";

/// The scheme prefix Deepgram's `Authorization` header is believed to use —
/// `Token <key>`, not a bearer token. UNVERIFIED, see the module documentation.
pub const AUTH_SCHEME: &str = "Token";

/// Linear PCM16 little-endian: the only encoding this client asks for, matching
/// the format [`AudioChunk::pcm`] already documents.
///
/// UNVERIFIED — see the module documentation.
const ENCODING: &str = "linear16";

/// Build the connection URL: encoding, sample rate, channel count, a language or a
/// request to detect one, diarization, interim results, and — if any exist —
/// keyterms as repeated query parameters.
///
/// Everything the session needs travels here rather than in a message sent after
/// the handshake, the same choice [`crate::assemblyai::endpoint`] makes and for the
/// same reason: this is the part with a rule in it, and it is fully covered by
/// fixtures below without a socket.
///
/// Every parameter here is entailed by a fact this client claims elsewhere rather
/// than invented for its own sake: `encoding`/`sample_rate`/`channels` because raw
/// PCM needs them, `language`/`detect_language` because
/// `language_detection: true` is declared, `diarize` because `speaker_labels: true`
/// is declared, and `keyterm` because `keyterms` is declared
/// [`crate::KeytermSupport::Supported`]. Nothing is sent that is not backed by a
/// row in [`crate::capabilities::PROVIDERS`].
pub fn endpoint(
    sample_rate: u32,
    language: Option<&str>,
    keyterms: &[String],
    caps: &MeetingProviderCapabilities,
) -> Result<String, MeetingError> {
    endpoint_with_model(sample_rate, language, keyterms, caps, None)
}

/// Use the selected model; unsupported models retain it and omit keyterms.
pub fn endpoint_with_model(
    sample_rate: u32,
    language: Option<&str>,
    keyterms: &[String],
    caps: &MeetingProviderCapabilities,
    model: Option<&str>,
) -> Result<String, MeetingError> {
    let mut url = Url::parse(ENDPOINT).map_err(|err| MeetingError::Rejected {
        provider: PROVIDER_ID.to_owned(),
        detail: format!("the endpoint template did not parse: {err}"),
    })?;

    {
        let mut query = url.query_pairs_mut();
        query.append_pair("encoding", ENCODING);
        query.append_pair("sample_rate", &sample_rate.to_string());
        // `AudioChunk::pcm` documents mono audio; this is that fact stated on
        // the wire rather than left for the server to assume.
        query.append_pair("channels", "1");
        // `speaker_labels: true` in the capability table -- requested
        // unconditionally, because nothing in `MeetingProviderConfig` lets a
        // caller turn this client's diarization off. UNVERIFIED param name
        // and value; see the module documentation.
        query.append_pair("diarize", "true");
        // `partials: true` in the capability table -- without this, believed
        // (but unverified) to default to final-only results, which would
        // make that claim false rather than merely unconfirmed.
        query.append_pair("interim_results", "true");
    }

    let prepared = kw_transcription_core::vocabulary::prepare(PROVIDER_ID, model, keyterms);
    if let Some(model) = model.or_else(|| (!keyterms.is_empty()).then_some("nova-3")) {
        url.query_pairs_mut().append_pair("model", model);
    }
    match language.filter(|l| !l.trim().is_empty()) {
        Some(language) => {
            url.query_pairs_mut().append_pair("language", language);
        }
        // `language_detection: true` is declared, so `validate` does not
        // require a language here -- this asks the provider to detect one
        // rather than silently falling back to whatever its own default
        // model language is. UNVERIFIED param name and value.
        None => {
            if !prepared.supported || (model.is_none() && keyterms.is_empty()) {
                url.query_pairs_mut().append_pair("detect_language", "true");
            } else {
                url.query_pairs_mut().append_pair("language", "multi");
            }
        }
    }

    let keyterms = &prepared.included;
    let cap = caps.keyterms.effective_count(keyterms.len());
    if cap > 0 {
        let mut query = url.query_pairs_mut();
        // Repeated once per term, through `query_pairs_mut` rather than
        // string concatenation, so a term containing a space, an ampersand,
        // an equals sign or a quote is percent-encoded rather than altering
        // the query or injecting a parameter of its own. See
        // `the_keyterm_wire_escapes_characters_that_could_alter_the_query`
        // below.
        for term in keyterms.iter().take(cap) {
            query.append_pair("keyterm", term);
        }
    }

    Ok(url.to_string())
}

/// What a server message means to us.
#[derive(Debug, Clone, PartialEq)]
pub enum Incoming {
    /// The session is open and audio may flow. Wired but not waited on — see
    /// the module documentation on why Deepgram is believed to need no
    /// explicit readiness signal before audio may flow.
    Ready,
    /// Transcript for one message: whether the provider considers it settled,
    /// and who the provider says is speaking, if the wire carried a label.
    Transcript {
        text: String,
        is_final: bool,
        speaker: Option<String>,
    },
    /// The server reported an error. Carried rather than logged, because a
    /// meeting silently producing no transcript is the failure to avoid — a
    /// promise the socket layer broke until 2026-08-31, logging this and
    /// carrying on. It now records into [`crate::fault::SessionFault`] and the
    /// session ends.
    Failed { message: String },
    /// A frame this client could not parse at all.
    ///
    /// Separate from [`Incoming::Failed`] on purpose. That one means the
    /// provider *named* a fault; this one means we could not read what the
    /// provider said. `interpret` used to answer both with `Failed`, so an HTML
    /// error body and a text keepalive this parser happens not to know were the
    /// same thing to every caller. [`crate::fault::ends_the_session`] is where
    /// the difference is spent, and says why.
    Unreadable { preview: String },
    /// Something this client does not act on. Named rather than ignored so a
    /// new event type shows up in a log instead of vanishing.
    Other { kind: String },
}

/// The transcript text of the first alternative, if the shape is what is
/// believed. A private helper purely to avoid indexing into [`Value`], which
/// this workspace's lints forbid.
fn first_alternative_transcript(value: &Value) -> Option<&str> {
    value
        .get("channel")?
        .get("alternatives")?
        .get(0)?
        .get("transcript")?
        .as_str()
}

/// The first word's speaker, if diarization put one there. UNVERIFIED — see
/// the module documentation on reading the first word as representative of
/// the whole message, and on the field being a number rather than a letter.
///
/// Degrades to `None` for a message with no words, a `speaker` field that is
/// absent, explicitly `null`, or not a number -- never a panic and never a
/// guessed label.
fn first_word_speaker(value: &Value) -> Option<String> {
    let speaker = value
        .get("channel")?
        .get("alternatives")?
        .get(0)?
        .get("words")?
        .get(0)?
        .get("speaker")?
        .as_u64()?;
    Some(speaker.to_string())
}

/// Read one server message.
///
/// Pure, so every shape is covered without a socket. The envelope itself (a
/// `type` field naming `Results`/`Metadata`/`Error`) and every field read from
/// a `Results` message are unverified — see the module documentation.
#[must_use]
pub fn interpret(frame: &str) -> Incoming {
    let Ok(value) = serde_json::from_str::<Value>(frame) else {
        // Not `Failed`. An HTML error body and a keepalive this parser does not
        // recognise both land here, and only one of them should end a meeting;
        // the caller decides which, with `crate::fault::ends_the_session`. The
        // frame itself is repeated back — bounded and flattened — because "not
        // JSON" is worth very little next to what the provider actually sent.
        return Incoming::Unreadable {
            preview: crate::fault::preview(frame),
        };
    };

    let kind = value
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned();

    match kind.as_str() {
        "Results" => {
            let text = first_alternative_transcript(&value)
                .unwrap_or_default()
                .to_owned();
            let is_final = value
                .get("is_final")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let speaker = first_word_speaker(&value);
            Incoming::Transcript {
                text,
                is_final,
                speaker,
            }
        }
        "Metadata" => Incoming::Ready,
        "Error" => Incoming::Failed {
            message: value
                .get("message")
                .and_then(Value::as_str)
                .or_else(|| value.get("description").and_then(Value::as_str))
                .unwrap_or("the server reported an error with no message")
                .to_owned(),
        },
        other => Incoming::Other {
            kind: other.to_owned(),
        },
    }
}

/// The message that ends a session gracefully, sent before the closing frame so
/// the server can flush and reply with a final `Metadata` message rather than
/// seeing a plain drop.
///
/// UNVERIFIED — see the module documentation. Sending it costs nothing if the
/// server does not recognise it: the close frame that follows ends the session
/// either way.
#[must_use]
pub fn terminate() -> Value {
    json!({ "type": "CloseStream" })
}

/// Resolve the credential for this provider.
///
/// Returns the secret only at the moment of use, from the store, so no part of
/// the meeting configuration holds one. `token_rotation: true` describes
/// upstream's minted temporary tokens, not this function -- see the module
/// documentation. Calling this repeatedly for the same key returns the same
/// value every time, because nothing here treats a stored API key as expiring.
pub fn token(
    store: &Arc<dyn CredentialStore>,
    key_ref: Option<&KeyRef>,
) -> Result<kw_security::Secret, MeetingError> {
    let key_ref = key_ref.ok_or_else(|| MeetingError::MissingCredential {
        slot: "deepgram".to_owned(),
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
/// WebSocket message rather than wrapped in JSON. See the module documentation
/// on why that is believed rather than confirmed.
#[must_use]
pub fn frame_bytes(chunk: &AudioChunk) -> &[u8] {
    &chunk.pcm
}

/// Assemble the transcript from what the socket produced.
///
/// `reconnects` is always zero: this provider declares no `session_limit`, so
/// there is no clock deciding to rebuild the sockets, and no `SessionClock`
/// exists in this module for that reason -- the same shape as
/// [`crate::assemblyai::transcript`].
///
/// `gaps` is `TranscriptBudget::gaps` from the socket layer: how many stretches
/// of audio went untranscribed because the session's retention ceiling was
/// full. It is a parameter rather than a hardcoded `0` as of KNEG-109 — bounding
/// untrusted provider output is deliberate, and so is saying what the bound
/// cost. `keep` in `meeting.rs` turns a non-zero count into a paragraph on the
/// saved note, which is the whole reason `MeetingTranscript::gaps` exists.
///
/// Nothing is dropped on the way *out*: audio either reaches the socket or the
/// session ends. Three things end it and none of them skips ahead — a send that
/// fails, an error frame the provider sent, and a first frame this client could
/// not read. See `crate::fault`. So every gap this can report came from the
/// ceiling on the way in.
#[must_use]
pub fn transcript(
    events: Vec<TranscriptEvent>,
    duration_seconds: f64,
    gaps: u32,
) -> MeetingTranscript {
    MeetingTranscript {
        events,
        duration_seconds,
        reconnects: 0,
        gaps,
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
            key_ref: KeyRef::new("deepgram"),
            sample_rate: rate,
        }
    }

    /// The query parameters of a built endpoint, as a multimap, so a test can
    /// ask for every value of a repeated parameter rather than only the last
    /// one a plain map would keep.
    fn query_pairs(url: &str) -> Vec<(String, String)> {
        Url::parse(url)
            .unwrap()
            .query_pairs()
            .map(|(k, v)| (k.into_owned(), v.into_owned()))
            .collect()
    }

    fn query_of(url: &str) -> HashMap<String, String> {
        query_pairs(url).into_iter().collect()
    }

    /* ------------------------------------------------------ what is sent */

    #[test]
    fn explicit_incompatible_model_is_preserved_without_sending_hints() {
        let url =
            endpoint_with_model(16_000, None, &["Dyve".into()], caps(), Some("nova-2")).unwrap();
        let pairs = query_pairs(&url);
        assert!(pairs.contains(&("model".into(), "nova-2".into())));
        assert!(pairs.contains(&("detect_language".into(), "true".into())));
        assert!(!pairs.iter().any(|(key, _)| key == "keyterm"));
    }

    #[test]
    fn the_endpoint_carries_the_wire_facts_entailed_by_the_capability_table_and_passes_the_boundary(
    ) {
        let url = endpoint(16_000, Some("en"), &[], caps()).unwrap();
        assert!(kw_network_policy::validate_websocket(&url, PURPOSE).is_ok());

        let query = query_of(&url);
        assert_eq!(query.get("encoding").map(String::as_str), Some("linear16"));
        assert_eq!(query.get("sample_rate").map(String::as_str), Some("16000"));
        assert_eq!(query.get("channels").map(String::as_str), Some("1"));
        assert_eq!(query.get("diarize").map(String::as_str), Some("true"));
        assert_eq!(
            query.get("interim_results").map(String::as_str),
            Some("true")
        );
    }

    #[test]
    fn a_chosen_language_is_sent_and_detection_is_not_requested() {
        let url = endpoint(16_000, Some("sv"), &[], caps()).unwrap();
        let query = query_of(&url);
        assert_eq!(query.get("language").map(String::as_str), Some("sv"));
        assert!(!query.contains_key("detect_language"));
    }

    #[test]
    fn no_language_asks_the_provider_to_detect_one_rather_than_guessing_here() {
        // `language_detection: true` is declared, so `validate` does not
        // require a language -- this is what happens instead of silently
        // sending nothing and hoping the server's default matches the room.
        for absent in [None, Some(""), Some("   ")] {
            let url = endpoint(16_000, absent, &[], caps()).unwrap();
            let query = query_of(&url);
            assert!(
                !query.contains_key("language"),
                "{absent:?} should not send a concrete language"
            );
            assert_eq!(
                query.get("detect_language").map(String::as_str),
                Some("true"),
                "{absent:?}"
            );
        }
    }

    #[test]
    fn no_keyterms_means_no_query_parameter_at_all() {
        let url = endpoint(16_000, None, &[], caps()).unwrap();
        assert!(!query_pairs(&url).iter().any(|(k, _)| k == "keyterm"));
    }

    #[test]
    fn keyterms_are_sent_as_one_repeated_query_parameter_each() {
        let terms = vec!["Knegis".to_owned(), "sprint review".to_owned()];
        let url = endpoint(16_000, None, &terms, caps()).unwrap();

        let sent: Vec<String> = query_pairs(&url)
            .into_iter()
            .filter(|(k, _)| k == "keyterm")
            .map(|(_, v)| v)
            .collect();
        assert_eq!(sent, terms);
    }

    #[test]
    fn application_byte_budget_bounds_the_sent_keyterms() {
        // The application byte budget is stricter than the provider term cap.
        let terms: Vec<String> = (0..250).map(|i| format!("term{i}")).collect();
        let url = endpoint(16_000, None, &terms, caps()).unwrap();

        let sent: Vec<String> = query_pairs(&url)
            .into_iter()
            .filter(|(k, _)| k == "keyterm")
            .map(|(_, v)| v)
            .collect();
        let review = kw_transcription_core::vocabulary::prepare(PROVIDER_ID, None, &terms);
        assert_eq!(sent, review.included);
        assert!(sent.join(", ").len() <= 400);
        assert!(!review.omitted.is_empty());
        assert!(query_pairs(&url).contains(&("model".into(), "nova-3".into())));
    }

    #[test]
    fn the_keyterm_wire_escapes_characters_that_could_alter_the_query() {
        // Carried forward from Task 3's review: no keyterm test used a term
        // containing characters that could alter a query, so the escaping
        // rested on library behaviour rather than on a test. For Deepgram
        // that is not a nicety -- repeated query parameters are the wire
        // format itself, so this is where correctness actually lives.
        // Asserted on the *parsed* query pairs, not a substring of the URL,
        // so this fails if `&`/`=`/a space/a quote ever manage to split into
        // a second parameter rather than staying inside this one's value.
        let hostile = r#"a&b=c "quoted""#.to_owned();
        let terms = vec![hostile.clone(), "second term".to_owned()];
        let url = endpoint(16_000, None, &terms, caps()).unwrap();

        let sent: Vec<(String, String)> = query_pairs(&url)
            .into_iter()
            .filter(|(k, _)| k == "keyterm")
            .collect();
        assert_eq!(
            sent,
            vec![
                ("keyterm".to_owned(), hostile.clone()),
                ("keyterm".to_owned(), "second term".to_owned()),
            ],
            "each hostile term must survive intact, as its own keyterm value"
        );

        // And nothing the term contains introduced a parameter of its own --
        // `b`, `c`, `quoted` and a bare `a` must none of them appear as query
        // parameter names.
        let names: Vec<String> = query_pairs(&url).into_iter().map(|(k, _)| k).collect();
        for injected in ["a", "b", "c", "quoted"] {
            assert!(
                !names.iter().any(|k| k == injected),
                "{injected:?} must not have become a query parameter name; got {names:?}"
            );
        }
    }

    #[test]
    fn the_boundary_still_blocks_a_vendor_host_under_this_providers_purpose() {
        // Belt and suspenders: this client's own PURPOSE constant is not a way
        // to sidestep the boundary. `validate_websocket` runs on every socket
        // open (see `deepgram_socket::DeepgramRealtime::open`), not once at
        // startup, so a blocked host must not become reachable by
        // reconnecting -- proved here directly against the purpose this
        // module actually uses, the same host the network-policy crate's own
        // suite already exercises.
        // domain-scanner:allow-next-line asserting the runtime blocklist refuses a vendor host under this provider's purpose
        let blocked = "wss://api.openwhispr.com/v1/listen";
        assert!(kw_network_policy::validate_websocket(blocked, PURPOSE).is_err());
    }

    #[test]
    fn a_chunk_becomes_the_raw_bytes_it_carries() {
        // No JSON, no base64 -- the same convention this client believes
        // AssemblyAI's uses, and the opposite of OpenAI Realtime's.
        let chunk = AudioChunk {
            pcm: vec![0x10, 0x20, 0xff, 0x00],
            source: AudioSource::Mic,
        };
        assert_eq!(frame_bytes(&chunk), &[0x10, 0x20, 0xff, 0x00]);
    }

    /* ---------------------------------------------------- what is read */

    #[test]
    fn a_results_message_is_recognised_as_a_transcript() {
        let partial = interpret(
            r#"{"type":"Results","is_final":false,"channel":{"alternatives":[{"transcript":"the quick"}]}}"#,
        );
        assert_eq!(
            partial,
            Incoming::Transcript {
                text: "the quick".to_owned(),
                is_final: false,
                speaker: None,
            }
        );

        let complete = interpret(
            r#"{"type":"Results","is_final":true,"channel":{"alternatives":[{"transcript":"the quick brown fox"}]}}"#,
        );
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
    fn a_speaker_on_the_first_word_is_carried_through_as_a_string() {
        let incoming = interpret(
            r#"{"type":"Results","is_final":true,"channel":{"alternatives":[
                {"transcript":"hello","words":[{"word":"hello","speaker":2}]}
            ]}}"#,
        );
        match incoming {
            Incoming::Transcript { speaker, .. } => assert_eq!(speaker, Some("2".to_owned())),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_speaker_field_that_is_absent_null_or_the_wrong_type_degrades_to_none() {
        // Left untested for AssemblyAI's speaker field in Task 3; for
        // Deepgram the field is nested three levels deep, which is exactly
        // where a missing link in the chain must degrade rather than panic.
        for frame in [
            r#"{"type":"Results","is_final":true,"channel":{"alternatives":[
                {"transcript":"hello","words":[{"word":"hello"}]}
            ]}}"#,
            r#"{"type":"Results","is_final":true,"channel":{"alternatives":[
                {"transcript":"hello","words":[{"word":"hello","speaker":null}]}
            ]}}"#,
            r#"{"type":"Results","is_final":true,"channel":{"alternatives":[
                {"transcript":"hello","words":[{"word":"hello","speaker":"A"}]}
            ]}}"#,
            r#"{"type":"Results","is_final":true,"channel":{"alternatives":[
                {"transcript":"hello","words":[]}
            ]}}"#,
            r#"{"type":"Results","is_final":true,"channel":{"alternatives":[
                {"transcript":"hello"}
            ]}}"#,
            r#"{"type":"Results","is_final":true}"#,
        ] {
            match interpret(frame) {
                Incoming::Transcript { speaker, .. } => {
                    assert_eq!(speaker, None, "frame: {frame}");
                }
                other => panic!("{frame}: {other:?}"),
            }
        }
    }

    #[test]
    fn a_metadata_message_is_recognised_as_ready() {
        assert_eq!(
            interpret(r#"{"type":"Metadata","request_id":"abc","duration":12.3}"#),
            Incoming::Ready
        );
    }

    #[test]
    fn a_server_error_is_carried_rather_than_swallowed() {
        let incoming = interpret(r#"{"type":"Error","message":"invalid sample rate"}"#);
        match incoming {
            Incoming::Failed { message } => assert_eq!(message, "invalid sample rate"),
            other => panic!("{other:?}"),
        }

        // `description` as a fallback for `message`.
        let incoming = interpret(r#"{"type":"Error","description":"bad request"}"#);
        match incoming {
            Incoming::Failed { message } => assert_eq!(message, "bad request"),
            other => panic!("{other:?}"),
        }

        match interpret(r#"{"type":"Error"}"#) {
            Incoming::Failed { message } => assert!(!message.is_empty()),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_frame_this_client_does_not_act_on_lands_in_other_rather_than_a_transcript() {
        match interpret(r#"{"type":"Warning","warn_msg":"deprecated parameter"}"#) {
            Incoming::Other { kind } => assert_eq!(kind, "Warning"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_frame_that_is_not_json_is_unreadable_and_not_a_transcript() {
        // `Unreadable` rather than `Failed`: whether this ends the session
        // depends on when it arrived, and `crate::fault::ends_the_session` holds
        // that rule. The frame is repeated back rather than replaced with "not
        // JSON" — see `crate::fault::preview` for why, and for the bound on it.
        match interpret("<html>502 Bad Gateway</html>") {
            Incoming::Unreadable { preview } => {
                assert_eq!(preview, "<html>502 Bad Gateway</html>");
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_results_message_with_no_transcript_field_is_empty_rather_than_a_panic() {
        assert_eq!(
            interpret(r#"{"type":"Results","is_final":false}"#),
            Incoming::Transcript {
                text: String::new(),
                is_final: false,
                speaker: None,
            }
        );
    }

    /* ----------------------------------------------------- configuration */

    #[test]
    fn all_three_declared_rates_are_accepted() {
        for rate in [16_000, 24_000, 48_000] {
            assert!(prepare(&config(rate)).is_ok(), "{rate} Hz");
        }

        match prepare(&config(8_000)).unwrap_err() {
            MeetingError::SampleRate {
                offered, accepted, ..
            } => {
                assert_eq!(offered, 8_000);
                assert_eq!(accepted, &[16_000, 24_000, 48_000]);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn another_providers_configuration_is_refused() {
        // Fail closed. AssemblyAI's configuration must not be sent to
        // Deepgram just because this is a client that exists.
        let mut cfg = config(16_000);
        cfg.provider = "assemblyai-realtime".to_owned();
        match prepare(&cfg).unwrap_err() {
            MeetingError::UnknownProvider { id } => assert_eq!(id, "assemblyai-realtime"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_missing_credential_names_the_slot_rather_than_failing_obscurely() {
        let store: Arc<dyn CredentialStore> = Arc::new(kw_security::MemoryCredentialStore::new());

        match token(&store, None).unwrap_err() {
            MeetingError::MissingCredential { slot } => assert_eq!(slot, "deepgram"),
            other => panic!("{other:?}"),
        }

        let key = KeyRef::new("deepgram").unwrap();
        match token(&store, Some(&key)).unwrap_err() {
            MeetingError::MissingCredential { slot } => assert_eq!(slot, "deepgram"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_stored_credential_is_resolved_only_at_the_point_of_use() {
        use kw_security::Secret;

        let store: Arc<dyn CredentialStore> = Arc::new(kw_security::MemoryCredentialStore::new());
        let key = KeyRef::new("deepgram").unwrap();
        store
            .set(&key, &Secret::new("dg-not-a-real-key".to_owned()))
            .unwrap();

        let resolved = token(&store, Some(&key)).unwrap();
        assert_eq!(resolved.expose(), "dg-not-a-real-key");

        // And the configuration that reached this point never held it.
        let rendered = format!("{:?}", config(16_000));
        assert!(
            rendered.contains("deepgram"),
            "the slot name is safe to show"
        );
        assert!(!rendered.contains("dg-"));
    }

    #[test]
    fn token_rotation_is_declared_for_upstreams_temporary_tokens_not_for_a_stored_key() {
        use kw_security::Secret;
        // `token_rotation: true` describes Deepgram's own hosted service
        // minting short-lived temporary tokens that must be refreshed
        // mid-session -- see the module documentation. This application
        // never mints one: `token` resolves the user's stored API key and
        // nothing about it expires or needs to be re-read on a timer, which
        // is why there is no `SessionClock`-shaped struct anywhere in this
        // module or in `deepgram_socket` for that reason. The observable
        // proof is that reading it repeatedly, with real work done between
        // reads, returns byte-for-byte the same secret every time -- nothing
        // here treats it as expiring or swaps it out from under a session.
        assert!(
            caps().token_rotation,
            "the capability table still claims rotation for upstream's temporary tokens"
        );

        let store: Arc<dyn CredentialStore> = Arc::new(kw_security::MemoryCredentialStore::new());
        let key = KeyRef::new("deepgram").unwrap();
        store
            .set(&key, &Secret::new("dg-not-a-real-key".to_owned()))
            .unwrap();

        let readings: Vec<String> = (0..5)
            .map(|_| token(&store, Some(&key)).unwrap().expose().to_owned())
            .collect();
        assert!(
            readings.iter().all(|r| r == "dg-not-a-real-key"),
            "no refresh mutated or rotated the stored key between reads: {readings:?}"
        );
    }

    #[test]
    fn the_transcript_reports_no_reconnects_and_the_gaps_it_is_given() {
        let events = vec![event(
            "hello".to_owned(),
            true,
            Some("0".to_owned()),
            1.5,
            AudioSource::System,
        )];
        let transcript = transcript(events, 90.0, 0);

        assert_eq!(
            transcript.reconnects, 0,
            "no session_limit means no clock deciding to reconnect"
        );
        assert_eq!(
            transcript.gaps, 0,
            "nothing hit the retention ceiling in this session"
        );
        assert_eq!(transcript.events[0].source, AudioSource::System);
        assert_eq!(transcript.events[0].speaker.as_deref(), Some("0"));
    }

    #[test]
    fn a_gap_the_socket_counted_reaches_the_saved_transcript() {
        // The whole of KNEG-109 in one assertion. `keep` in `meeting.rs`
        // appends "N stretch(es) of audio were never transcribed" when this is
        // non-zero, so a hardcoded `0` here was a hole the note denied having.
        let transcript = transcript(Vec::new(), 90.0, 2);
        assert_eq!(transcript.gaps, 2);
    }

    #[test]
    fn the_capability_entry_agrees_with_this_module() {
        let caps = caps();
        assert!(caps.audio_leaves_device);
        assert_eq!(caps.sample_rates, &[16_000, 24_000, 48_000]);
        assert!(caps.speaker_labels, "the wire may carry a speaker field");
        assert!(
            caps.language_detection,
            "detect_language is sent when no language is chosen"
        );
        assert_eq!(
            caps.session_limit, None,
            "no clock to reconnect around, and no SessionClock in this module"
        );
        assert!(caps.token_rotation);
        match caps.keyterms {
            KeytermSupport::Supported { max, wire } => {
                assert_eq!(max, Some(100));
                assert_eq!(wire, KeytermWire::RepeatedQueryParam);
            }
            other => panic!("{other:?}"),
        }
    }
}
