//! Corti Realtime transcription over a WebSocket, authenticated first.
//!
//! # Two boundary crossings, not one
//!
//! Every client before this one crosses the network once, at the socket.
//! Corti crosses it twice: an OAuth2 client-credentials exchange over HTTPS
//! first, through `kw_network_policy::NetworkPolicy`, and then the socket
//! itself through [`kw_network_policy::validate_websocket`]. Both travel under
//! [`RequestPurpose::UserConfiguredTranscriptionProvider`] -- one action the
//! user configured, not two, and not an eighth purpose invented for the
//! occasion. See [`crate::corti_socket`] for where the exchange actually runs.
//!
//! # The token exchange is reused, not re-derived
//!
//! `kw_transcription_core::clients::corti_token_endpoint`,
//! `corti_token_request` and `interpret_token` already exist, are already
//! tested, and already do exactly this for batch Corti transcription
//! (`crates/transcription-core/src/executor.rs`'s `Executor::corti_token`).
//! [`token_request`] and [`token_endpoint`] here are thin wrappers around the
//! first two, not a second implementation. This module's only original
//! contribution to the exchange is *which* stored value plays which OAuth2
//! role, and that mapping is copied from the one place it is already
//! exercised: **the tenant name is the client id**, and the secret stored
//! under the `corti` credential slot is the client secret. Distinguish this
//! from everything under "What is not verified" below -- the exchange's
//! shape is reused from working, tested code, not believed from public
//! documentation.
//!
//! # One exchange, shared by both sockets
//!
//! [`crate::corti_socket::CortiRealtime::connect`] exchanges a token exactly
//! once, before either socket opens, and both the microphone and the system
//! socket carry the same access token. An OAuth2 client-credentials token is,
//! by design, valid for a bounded lifetime (`expires_in`) and meant to be
//! presented on more than one request; nothing in this project's own audit of
//! Corti (`docs/BUILD_PLAN.md`, `route.rs`, `clients.rs`) suggests a token is
//! scoped to a single connection the way Deepgram's minted temporary tokens
//! are documented to be. Exchanging twice would spend a second round trip on
//! a token the first one already produced, for no benefit this project has
//! found.
//!
//! # Environment and tenant are reused, not duplicated
//!
//! Both are properties of the Corti tenancy the user already configured for
//! batch transcription (`Settings > Transcription`,
//! `crates/transcription-core/src/route.rs`'s `corti_environment` /
//! `corti_tenant`), not of the meeting feature. A second pair of meeting-only
//! settings would be two answers to one question, so
//! `apps/desktop/src-tauri/src/meeting.rs` reads the same stored values
//! rather than asking for them again. [`scope`] is the refusal: it names
//! which of the two is missing, exactly like `route.rs`'s `required` does --
//! deliberately, because that precedent exists to fix a real defect. Before
//! it, an unconfigured Corti route defaulted to `us`/`base`, composing a live
//! endpoint under a tenancy nobody chose and sending a user's audio and
//! credential there. Guessing here would be the same mistake in a second
//! place.
//!
//! # Two sockets, one per capture -- the same shape as every client before it
//!
//! [`crate::TranscriptEvent::source`] is not optional, and `Settings >
//! Meetings` renders "You" and "Everyone else" from it. Corti's speaker
//! labels, if the wire carries them, say who *within* a stream, never *which*
//! stream. See [`crate::corti_socket`].
//!
//! # `KeytermWire::SessionConfig` -- the third distinct wire shape
//!
//! Deepgram repeats a query parameter and AssemblyAI JSON-encodes one into a
//! query parameter; Corti's keyterms travel inside a JSON message sent after
//! the handshake, never in the query string. [`session_config`] builds that
//! message. The key path, `configuration.keyterms`, comes from this
//! project's own audit (`docs/INTEGRATION_PARITY.md`), written before this
//! client and independent of it -- the rest of the message is this module's
//! own construction, marked below.
//!
//! # `language_detection: false`
//!
//! Corti needs a concrete primary language and cannot detect one, so
//! [`crate::validate`] reports `language_required` whenever none is set, and
//! the meeting runtime refuses before connecting rather than guessing. Every
//! other client this project has built so far declares detection, so this is
//! the first one that actually reaches that refusal in practice; a test in
//! `apps/desktop/src-tauri/src/meeting.rs` (written before this client, for
//! this reason) proves it fires.
//!
//! # What is verified
//!
//! The credential slot (`corti`, in `config/provider-registry.json`), the one
//! declared sample rate, `language_detection: false`, and the
//! `KeytermWire::SessionConfig` wire shape come from this project's own
//! capability table ([`crate::capabilities::PROVIDERS`]), written before this
//! client and independent of it. The OAuth2 exchange's request and response
//! shapes are reused from `kw_transcription_core::clients`, which is tested
//! there against a real client-credentials form -- see above.
//!
//! # What is not verified
//!
//! No Corti credential exists on this development machine, no request has
//! been made, and no vendor source for the *streaming* wire exists anywhere
//! in this repository to check against. Unlike the token exchange, everything
//! below this line is genuinely **believed, not confirmed** -- a clean-room
//! construction from the shape of the batch client and public convention, the
//! same position [`crate::deepgram`] and [`crate::assemblyai`] are in for
//! their sockets. Each item is marked again at its point of use so a reader
//! does not have to hold this list in their head:
//!
//! - The endpoint path, `wss://api.{environment}.corti.app/audio-bridge/v1/streams`.
//!   The host template -- `api.{environment}.corti.app` -- is reused from the
//!   tested batch endpoint; the path after it, `/audio-bridge/v1/streams`, is
//!   not.
//! - That the access token travels as `Authorization: Bearer <token>`. This is
//!   the one item believed with more confidence than the rest: it is the
//!   standard OAuth2 bearer convention (RFC 6750) for presenting an
//!   `access_token`, not a guess specific to Corti.
//! - That the tenant travels on the socket handshake as a `Tenant-Name`
//!   header -- the same header name the tested batch request carries, assumed
//!   here to apply to a WebSocket handshake, which it has never been tried
//!   against.
//! - The whole shape of [`session_config`] beyond the `configuration.keyterms`
//!   path noted above: whether an envelope is needed at all, whether
//!   `primaryLanguage` (reused from the batch field name) is the field this
//!   endpoint actually reads, and the `encoding`/`sampleRate`/`channels`
//!   fields, none of which appear anywhere in this project's own audit.
//! - The incoming message envelope entirely: a `type` field naming
//!   `"ready"`, `"transcript"` or `"error"`, and the per-message fields
//!   `text`, `isFinal` and `speaker`. This is the lowest-confidence area in
//!   this module, exactly as it is for Deepgram's and AssemblyAI's message
//!   envelopes -- there is no textual anchor for it anywhere in this
//!   repository, unlike `configuration.keyterms`.
//! - The graceful-termination frame, `{"type":"end"}`.
//! - Audio leaving as raw binary WebSocket frames, matching the convention
//!   believed for AssemblyAI and Deepgram. Held at the same confidence as the
//!   rest of this list, not more: unlike Deepgram there is no documented raw
//!   PCM streaming convention for Corti anywhere in this repository to anchor
//!   it to, only the multipart file upload the batch client uses.

use std::sync::Arc;

use kw_network_policy::RequestPurpose;
use kw_security::{CredentialStore, KeyRef};
use serde_json::{json, Value};

use crate::capabilities::{capabilities, MeetingProviderCapabilities};
use crate::{
    AudioChunk, AudioSource, MeetingError, MeetingProviderConfig, MeetingTranscript,
    TranscriptEvent,
};

/// The provider id and its capability entry.
pub const PROVIDER_ID: &str = "corti-realtime";

/// The purpose both of this client's requests travel under: the token
/// exchange and the socket alike. One action the user configured, not two.
pub const PURPOSE: RequestPurpose = RequestPurpose::UserConfiguredTranscriptionProvider;

/// The header the socket handshake carries the access token in.
///
/// `Bearer <token>` -- believed with more confidence than the rest of this
/// module's socket wire, since it is the OAuth2 convention (RFC 6750) for
/// presenting an `access_token`, not a guess specific to Corti. See the
/// module documentation.
pub const AUTH_HEADER: &str = "Authorization";

/// The header the socket handshake carries the tenant in.
///
/// The same header name the tested batch request uses
/// (`crates/transcription-core/src/clients.rs::shape`), assumed here to apply
/// to a WebSocket handshake too -- UNVERIFIED, see the module documentation.
pub const TENANT_HEADER: &str = "Tenant-Name";

/// Linear PCM16 little-endian: the only encoding this client asks for,
/// matching the format [`AudioChunk::pcm`] already documents.
///
/// UNVERIFIED field name and value -- see the module documentation.
const ENCODING: &str = "linear16";

/// Where the socket goes for one environment.
///
/// The host template is reused from the tested batch endpoint
/// (`kw_transcription_core::clients::corti_token_endpoint` and
/// `route.rs`'s own `https://api.{environment}.corti.app/v2/transcribe`); the
/// path after it is not -- UNVERIFIED, see the module documentation.
///
/// Carries no query parameters: unlike Deepgram and AssemblyAI, every
/// session-scoped fact this client sends -- audio format, language, keyterms
/// -- travels in [`session_config`] after the handshake, so there is exactly
/// one invented wire shape here rather than two.
pub fn endpoint(environment: &str) -> Result<String, MeetingError> {
    let trimmed = environment.trim();
    let url = format!("wss://api.{trimmed}.corti.app/audio-bridge/v1/streams");

    // Round-tripped through `Url` so a malformed environment fails here, as a
    // named refusal, rather than being handed to `connect_async` to fail
    // however tokio-tungstenite's own parser fails.
    url::Url::parse(&url)
        .map(|parsed| parsed.to_string())
        .map_err(|err| MeetingError::Rejected {
            provider: PROVIDER_ID.to_owned(),
            detail: format!("the endpoint template did not parse: {err}"),
        })
}

/// The session configuration message: audio format, the primary language, and
/// -- if any survive the provider's cap -- keyterms. Sent once, immediately
/// after the handshake, before any audio.
///
/// Everything here is entailed by a fact this client claims elsewhere rather
/// than invented for its own sake: `channels: 1` because [`AudioChunk::pcm`]
/// documents mono audio, `primaryLanguage` because `language_detection: false`
/// is declared and [`crate::validate`] already refuses to reach this point
/// with none, and `keyterms` because `keyterms` is declared
/// [`crate::KeytermSupport::Supported`]. The keyterm cap is read from `caps`
/// rather than a second constant here, the same reason
/// [`crate::assemblyai::endpoint`] reads it from `caps`: there is exactly one
/// place that number is declared, the table [`crate::ConfigReview`] already
/// uses to predict this same truncation.
///
/// UNVERIFIED beyond `configuration.keyterms` itself -- see the module
/// documentation.
pub fn session_config(
    sample_rate: u32,
    language: Option<&str>,
    keyterms: &[String],
    caps: &MeetingProviderCapabilities,
) -> Value {
    let mut configuration = json!({
        "encoding": ENCODING,
        "sampleRate": sample_rate,
        "channels": 1,
    });

    if let Some(map) = configuration.as_object_mut() {
        if let Some(language) = language.filter(|l| !l.trim().is_empty()) {
            // Reused from the batch client's own field name for the same
            // value (`clients.rs::shape`, tested there) rather than invented
            // for this endpoint -- UNVERIFIED that the streaming endpoint
            // reads the same name. See the module documentation.
            map.insert(
                "primaryLanguage".to_owned(),
                Value::String(language.to_owned()),
            );
        }

        let prepared = kw_transcription_core::vocabulary::prepare("corti-realtime", None, keyterms);
        let keyterms = &prepared.included;
        let cap = caps.keyterms.effective_count(keyterms.len());
        if cap > 0 {
            // `configuration.keyterms` -- from this project's own audit
            // (`docs/INTEGRATION_PARITY.md`), written before this client and
            // independent of it.
            let terms: Vec<Value> = keyterms
                .iter()
                .take(cap)
                .map(|term| json!({ "term": term }))
                .collect();
            map.insert("keyterms".to_owned(), json!({ "terms": terms }));
        }
    }

    json!({ "type": "config", "configuration": configuration })
}

/// What a server message means to us.
#[derive(Debug, Clone, PartialEq)]
pub enum Incoming {
    /// The session is open and audio may flow. Wired but not waited on --
    /// UNVERIFIED that Corti sends an explicit readiness signal at all. See
    /// the module documentation.
    Ready,
    /// Transcript for one message: whether the provider considers it
    /// settled, and who the provider says is speaking, if the wire carried a
    /// label.
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

/// Read one server message.
///
/// Pure, so every shape is covered without a socket. The envelope itself (a
/// `type` field naming `ready`/`transcript`/`error`) and every field read
/// from a `transcript` message are the lowest-confidence part of this whole
/// module -- see the module documentation.
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
        "ready" => Incoming::Ready,
        "transcript" => {
            let text = value
                .get("text")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned();
            let is_final = value
                .get("isFinal")
                .and_then(Value::as_bool)
                .unwrap_or(false);
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
        "error" => Incoming::Failed {
            message: value
                .get("message")
                .and_then(Value::as_str)
                .or_else(|| value.get("error").and_then(Value::as_str))
                .unwrap_or("the server reported an error with no message")
                .to_owned(),
        },
        other => Incoming::Other {
            kind: other.to_owned(),
        },
    }
}

/// The message that ends a session gracefully, sent before the closing frame.
///
/// UNVERIFIED -- see the module documentation. Sending it costs nothing if the
/// server does not recognise it: the close frame that follows ends the
/// session either way.
#[must_use]
pub fn terminate() -> Value {
    json!({ "type": "end" })
}

/// One of Corti's two scoping values, or a refusal naming which is missing.
///
/// The pure half of the same rule `route.rs`'s `required` enforces for batch
/// transcription -- copied rather than imported, because that helper returns
/// `RouteError`, a batch-only type from a crate this one already depends on
/// for the token functions but should not grow a second reason to depend on
/// for a four-line rule. The wording matches deliberately: this is the same
/// defect that rule fixes, reached from a second caller.
fn require(value: &str, field: &'static str) -> Result<String, MeetingError> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(MeetingError::Rejected {
            provider: PROVIDER_ID.to_owned(),
            detail: format!(
                "Corti has no {field} configured, and there is no safe default for one. The \
                 environment is part of the hostname and the tenant scopes the request to an \
                 account, so guessing either would send your audio to a real endpoint under a \
                 tenancy you did not choose. Nothing was sent. Add both under Settings > \
                 Transcription, next to \"Use for dictation\"."
            ),
        });
    }
    Ok(trimmed.to_owned())
}

/// Corti's environment and tenant, trimmed, or a refusal naming which is
/// missing.
///
/// See the module documentation on why these are read from the batch Corti
/// settings rather than a second pair of meeting-only ones.
pub fn scope(environment: &str, tenant: &str) -> Result<(String, String), MeetingError> {
    let environment = require(environment, "environment")?;
    let tenant = require(tenant, "tenant")?;
    Ok((environment, tenant))
}

/// Corti's OAuth2 token endpoint for one environment.
///
/// A thin wrapper: `kw_transcription_core::clients::corti_token_endpoint`
/// already does this and is already tested. See the module documentation.
#[must_use]
pub fn token_endpoint(environment: &str) -> String {
    kw_transcription_core::clients::corti_token_endpoint(environment)
}

/// The OAuth2 client-credentials form body, with the tenant as the client id.
///
/// `kw_transcription_core::clients::corti_token_request` already builds the
/// form and is already tested against a real client-credentials shape; what
/// this module owns is the one decision that function leaves to its caller --
/// *which* configured value becomes the client id. Copied from the only place
/// that decision is already exercised
/// (`crates/transcription-core/src/executor.rs`'s `Executor::corti_token`):
/// the tenant name, because that is the identity Corti authenticates a client
/// as. See the module documentation.
#[must_use]
pub fn token_request(tenant: &str, client_secret: &kw_security::Secret) -> kw_security::Secret {
    kw_transcription_core::clients::corti_token_request(tenant, client_secret)
}

/// Resolve the client secret this provider's OAuth2 exchange needs.
///
/// Named `client_secret` rather than `token`, unlike the sibling clients: what
/// is stored under this credential slot is never sent to a socket directly --
/// it travels only into [`token_request`]'s body, and the *access token* that
/// exchange returns is what actually reaches a socket. See
/// [`crate::corti_socket`].
pub fn client_secret(
    store: &Arc<dyn CredentialStore>,
    key_ref: Option<&KeyRef>,
) -> Result<kw_security::Secret, MeetingError> {
    let key_ref = key_ref.ok_or_else(|| MeetingError::MissingCredential {
        slot: "corti".to_owned(),
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
/// exists in this module for that reason -- the same shape as
/// [`crate::deepgram::transcript`].
#[must_use]
pub fn transcript(events: Vec<TranscriptEvent>, duration_seconds: f64) -> MeetingTranscript {
    MeetingTranscript {
        events,
        duration_seconds,
        reconnects: 0,
        // Nothing is dropped by this client: audio either reaches the socket
        // or the session ends. Three things end it and none of them skips
        // ahead — a send that fails, an error frame the provider sent, and a
        // first frame this client could not read. See `crate::fault`.
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
            key_ref: KeyRef::new("corti"),
            sample_rate: rate,
        }
    }

    /* ------------------------------------------------------------ endpoint */

    #[test]
    fn the_endpoint_is_scoped_to_the_environment_and_passes_the_boundary() {
        let url = endpoint("eu").unwrap();
        assert_eq!(url, "wss://api.eu.corti.app/audio-bridge/v1/streams");
        assert!(kw_network_policy::validate_websocket(&url, PURPOSE).is_ok());
    }

    #[test]
    fn the_endpoint_trims_whitespace_around_the_environment() {
        assert_eq!(
            endpoint(" eu ").unwrap(),
            "wss://api.eu.corti.app/audio-bridge/v1/streams"
        );
    }

    #[test]
    fn the_boundary_still_blocks_a_vendor_host_under_this_providers_purpose() {
        // Belt and suspenders: this client's own PURPOSE constant is not a way
        // to sidestep the boundary. `validate_websocket` runs on every socket
        // open (see `corti_socket::CortiRealtime::open`), not once at startup,
        // so a blocked host must not become reachable by reconnecting.
        // domain-scanner:allow-next-line asserting the runtime blocklist refuses a vendor host under this provider's purpose
        let blocked = "wss://api.openwhispr.com/audio-bridge/v1/streams";
        assert!(kw_network_policy::validate_websocket(blocked, PURPOSE).is_err());
    }

    /* ------------------------------------------------------- session config */

    #[test]
    fn the_session_config_carries_the_language_and_audio_format_under_one_configuration_key() {
        let value = session_config(16_000, Some("sv"), &[], caps());
        let configuration = value
            .get("configuration")
            .and_then(Value::as_object)
            .unwrap();

        assert_eq!(
            configuration.get("primaryLanguage").and_then(Value::as_str),
            Some("sv")
        );
        assert_eq!(
            configuration.get("sampleRate").and_then(Value::as_u64),
            Some(16_000)
        );
        assert_eq!(
            configuration.get("encoding").and_then(Value::as_str),
            Some("linear16")
        );
        assert_eq!(
            configuration.get("channels").and_then(Value::as_u64),
            Some(1)
        );
    }

    #[test]
    fn no_language_is_sent_unless_one_was_chosen() {
        for absent in [None, Some(""), Some("   ")] {
            let value = session_config(16_000, absent, &[], caps());
            let configuration = value
                .get("configuration")
                .and_then(Value::as_object)
                .unwrap();
            assert!(
                !configuration.contains_key("primaryLanguage"),
                "{absent:?} should not send a language"
            );
        }
    }

    #[test]
    fn no_keyterms_means_no_keyterms_key_at_all() {
        let value = session_config(16_000, Some("en"), &[], caps());
        let configuration = value
            .get("configuration")
            .and_then(Value::as_object)
            .unwrap();
        assert!(!configuration.contains_key("keyterms"));
    }

    #[test]
    fn keyterms_travel_inside_the_configuration_key_not_the_query_string() {
        // The one property `KeytermWire::SessionConfig` exists to name: this is
        // *not* a query parameter, unlike Deepgram's and AssemblyAI's.
        let terms = vec!["Knegis".to_owned(), "sprint review".to_owned()];
        let value = session_config(16_000, None, &terms, caps());

        let configuration = value
            .get("configuration")
            .and_then(Value::as_object)
            .unwrap();
        let sent: Vec<&str> = configuration
            .get("keyterms")
            .and_then(|value| value.get("terms"))
            .and_then(Value::as_array)
            .unwrap()
            .iter()
            .filter_map(|value| value.get("term").and_then(Value::as_str))
            .collect();
        assert_eq!(sent, terms.iter().map(String::as_str).collect::<Vec<_>>());
    }

    #[test]
    fn terms_within_the_provider_cap_are_sent() {
        // Corti accepts up to 1,000 terms subject to per-term limits.
        let terms: Vec<String> = (0..250).map(|i| format!("term{i}")).collect();
        let value = session_config(16_000, None, &terms, caps());
        let configuration = value
            .get("configuration")
            .and_then(Value::as_object)
            .unwrap();
        let sent = configuration
            .get("keyterms")
            .and_then(|value| value.get("terms"))
            .and_then(Value::as_array)
            .unwrap();
        assert_eq!(sent.len(), 250);
    }

    /* ---------------------------------------------------- what is read */

    #[test]
    fn a_transcript_message_is_recognised() {
        let partial = interpret(r#"{"type":"transcript","text":"the quick","isFinal":false}"#);
        assert_eq!(
            partial,
            Incoming::Transcript {
                text: "the quick".to_owned(),
                is_final: false,
                speaker: None,
            }
        );

        let complete = interpret(
            r#"{"type":"transcript","text":"the quick brown fox","isFinal":true,"speaker":"A"}"#,
        );
        assert_eq!(
            complete,
            Incoming::Transcript {
                text: "the quick brown fox".to_owned(),
                is_final: true,
                speaker: Some("A".to_owned()),
            }
        );
    }

    #[test]
    fn a_transcript_with_no_speaker_field_leaves_it_none_rather_than_guessing() {
        let incoming = interpret(r#"{"type":"transcript","text":"hello","isFinal":true}"#);
        match incoming {
            Incoming::Transcript { speaker, .. } => assert_eq!(speaker, None),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_ready_message_is_recognised() {
        assert_eq!(interpret(r#"{"type":"ready"}"#), Incoming::Ready);
    }

    #[test]
    fn a_server_error_is_carried_rather_than_swallowed() {
        let incoming = interpret(r#"{"type":"error","message":"invalid sample rate"}"#);
        match incoming {
            Incoming::Failed { message } => assert_eq!(message, "invalid sample rate"),
            other => panic!("{other:?}"),
        }

        // `error` as a fallback for `message`.
        let incoming = interpret(r#"{"type":"error","error":"bad request"}"#);
        match incoming {
            Incoming::Failed { message } => assert_eq!(message, "bad request"),
            other => panic!("{other:?}"),
        }

        match interpret(r#"{"type":"error"}"#) {
            Incoming::Failed { message } => assert!(!message.is_empty()),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_frame_this_client_does_not_act_on_lands_in_other_rather_than_a_transcript() {
        match interpret(r#"{"type":"metadata","durationSeconds":12.3}"#) {
            Incoming::Other { kind } => assert_eq!(kind, "metadata"),
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
    fn a_transcript_message_with_no_text_field_is_empty_rather_than_a_panic() {
        assert_eq!(
            interpret(r#"{"type":"transcript","isFinal":false}"#),
            Incoming::Transcript {
                text: String::new(),
                is_final: false,
                speaker: None,
            }
        );
    }

    /* --------------------------------------------------------------- scope */

    #[test]
    fn a_missing_environment_is_refused_by_name() {
        let err = scope("", "acme").unwrap_err();
        assert!(err.to_string().contains("environment"), "{err}");
    }

    #[test]
    fn a_missing_tenant_is_refused_by_name() {
        let err = scope("eu", "").unwrap_err();
        assert!(err.to_string().contains("tenant"), "{err}");
    }

    #[test]
    fn blank_counts_as_missing_for_either_field() {
        assert!(scope("   ", "acme").is_err());
        assert!(scope("eu", "   ").is_err());
        assert!(scope("", "").is_err());
    }

    #[test]
    fn both_present_resolves_and_trims() {
        let (environment, tenant) = scope(" eu ", " acme ").unwrap();
        assert_eq!(environment, "eu");
        assert_eq!(tenant, "acme");
    }

    /* ------------------------------------------------------- token exchange */

    #[test]
    fn the_token_endpoint_is_scoped_to_the_environment() {
        assert_eq!(
            token_endpoint("eu"),
            "https://auth.eu.corti.app/realms/eu/protocol/openid-connect/token"
        );
    }

    #[test]
    fn the_token_request_uses_the_tenant_as_the_client_id() {
        // The mapping this module owns: which of Corti's two configured
        // values plays which OAuth2 role. `kw_transcription_core::clients`
        // already tests the form-encoding itself against a real
        // client-credentials shape for batch transcription -- this only
        // tests the decision that is this module's own.
        let secret = kw_security::Secret::new("shh".to_owned());
        let form = token_request("my-clinic", &secret);
        assert!(form.expose().contains("grant_type=client_credentials"));
        assert!(form.expose().contains("client_id=my-clinic"));
        assert!(form.expose().contains("client_secret=shh"));
    }

    #[test]
    fn the_token_request_form_does_not_print_itself() {
        let secret = kw_security::Secret::new("shh".to_owned());
        let form = token_request("my-clinic", &secret);
        assert!(!format!("{form:?}").contains("shh"), "{form:?}");
    }

    /* ---------------------------------------------------------- credential */

    #[test]
    fn a_missing_credential_names_the_slot_rather_than_failing_obscurely() {
        let store: Arc<dyn CredentialStore> = Arc::new(kw_security::MemoryCredentialStore::new());

        match client_secret(&store, None).unwrap_err() {
            MeetingError::MissingCredential { slot } => assert_eq!(slot, "corti"),
            other => panic!("{other:?}"),
        }

        let key = KeyRef::new("corti").unwrap();
        match client_secret(&store, Some(&key)).unwrap_err() {
            MeetingError::MissingCredential { slot } => assert_eq!(slot, "corti"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_stored_credential_is_resolved_only_at_the_point_of_use() {
        use kw_security::Secret;

        let store: Arc<dyn CredentialStore> = Arc::new(kw_security::MemoryCredentialStore::new());
        let key = KeyRef::new("corti").unwrap();
        store
            .set(&key, &Secret::new("corti-not-a-real-secret".to_owned()))
            .unwrap();

        let resolved = client_secret(&store, Some(&key)).unwrap();
        assert_eq!(resolved.expose(), "corti-not-a-real-secret");

        // And the configuration that reached this point never held it.
        let rendered = format!("{:?}", config(16_000));
        assert!(rendered.contains("corti"), "the slot name is safe to show");
        assert!(!rendered.contains("corti-not-a-real-secret"));
    }

    /* ----------------------------------------------------------- prepare */

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
        // Fail closed. Deepgram's configuration must not be sent to Corti just
        // because this is a client that exists.
        let mut cfg = config(16_000);
        cfg.provider = "deepgram-realtime".to_owned();
        match prepare(&cfg).unwrap_err() {
            MeetingError::UnknownProvider { id } => assert_eq!(id, "deepgram-realtime"),
            other => panic!("{other:?}"),
        }
    }

    /* -------------------------------------------------------------- other */

    #[test]
    fn a_chunk_becomes_the_raw_bytes_it_carries() {
        let chunk = AudioChunk {
            pcm: vec![0x10, 0x20, 0xff, 0x00],
            source: AudioSource::Mic,
        };
        assert_eq!(frame_bytes(&chunk), &[0x10, 0x20, 0xff, 0x00]);
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
            "a concrete primary language must be given -- this is the client that first \
             reaches MeetingRuntime::start's language_required refusal"
        );
        assert_eq!(
            caps.session_limit, None,
            "no clock to reconnect around, and no SessionClock in this module"
        );
        assert!(!caps.token_rotation);
        match caps.keyterms {
            KeytermSupport::Supported { max, wire } => {
                assert_eq!(max, Some(1000));
                assert_eq!(wire, KeytermWire::SessionConfig);
            }
            other => panic!("{other:?}"),
        }
    }
}
