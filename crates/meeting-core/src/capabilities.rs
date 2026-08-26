//! What each meeting transcription provider can actually do.
//!
//! This is new work rather than a port. Upstream has no capability object at
//! all: capability is implied by a `streaming: true` flag in the registry, by
//! per-provider wire-shape differences buried in each client, and by behavioural
//! facts that live only in comments. The result is that the calling code cannot
//! ask a question like "will this provider honour my keyterms?" — it has to know.
//!
//! The design rule here is that the type must be able to express the asymmetries
//! the audit actually found, not a uniform shape that flatters the providers.
//! Two examples make the point:
//!
//! * **AssemblyAI caps keyterms at 100.** Deepgram sends them as repeated query
//!   parameters, Corti as a JSON configuration block, AssemblyAI as a JSON query
//!   parameter. Three wire shapes, one of them with a limit.
//!
//! * **OpenAI Realtime and Tinfoil ignore keyterms entirely.** They accept them
//!   without complaint and do nothing with them. Upstream even includes keyterms
//!   in the connection cache key for these providers, which implies they matter.
//!   Modelling that as "supported" would be exactly the fake integration success
//!   the brief forbids, so [`KeytermSupport::Unsupported`] is a state a provider
//!   can be in and the UI is expected to say so.
//!
//! Nothing here is a promise about a provider we have not implemented. A
//! capability set describes what the client in this repository does.

use std::time::Duration;

use serde::Serialize;
use ts_rs::TS;

/// How a provider expects keyterms on the wire.
///
/// The differences are not cosmetic: each needs different serialisation, and one
/// of them silently truncates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum KeytermWire {
    /// A single JSON-encoded array in a query parameter
    /// (`keyterms_prompt=["..."]`). AssemblyAI.
    JsonQueryParam,
    /// The parameter repeated once per term (`keyterm=a&keyterm=b`). Deepgram.
    RepeatedQueryParam,
    /// A field inside the session configuration message. Corti.
    SessionConfig,
}

/// Whether a provider does anything with keyterms.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(
    tag = "state",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum KeytermSupport {
    /// The provider accepts them and ignores them. OpenAI Realtime and Tinfoil
    /// both do this, and it must not be reported as support.
    Unsupported,
    Supported {
        /// `None` means no documented limit.
        max: Option<usize>,
        wire: KeytermWire,
    },
}

impl KeytermSupport {
    #[must_use]
    pub const fn is_supported(self) -> bool {
        matches!(self, Self::Supported { .. })
    }

    /// How many of `requested` terms will actually be used.
    ///
    /// Returns zero for an unsupported provider, which is the honest answer and
    /// lets a UI say "this provider ignores keyterms" rather than showing a
    /// field that does nothing.
    #[must_use]
    pub const fn effective_count(self, requested: usize) -> usize {
        match self {
            Self::Unsupported => 0,
            Self::Supported { max: Some(max), .. } => {
                if requested < max {
                    requested
                } else {
                    max
                }
            }
            Self::Supported { max: None, .. } => requested,
        }
    }
}

/// What a provider can do.
///
/// Every field is here because a caller has to branch on it. Nothing is included
/// for symmetry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MeetingProviderCapabilities {
    /// The provider's id.
    pub id: &'static str,
    /// Name for the UI.
    pub name: &'static str,

    /// Whether audio leaves the machine. `false` only for a local model.
    ///
    /// First field a privacy-conscious user asks about, so it is not buried.
    pub audio_leaves_device: bool,

    pub keyterms: KeytermSupport,

    /// A hard session limit the client must reconnect around.
    pub session_limit: Option<Duration>,

    /// Whether the provider issues short-lived tokens that must be refreshed
    /// mid-session. Deepgram is the only one.
    pub token_rotation: bool,

    /// Sample rates the provider accepts, in Hz.
    pub sample_rates: &'static [u32],

    /// Whether the provider labels speakers.
    pub speaker_labels: bool,

    /// Whether interim (unstable) results arrive.
    pub partials: bool,

    /// Whether the provider detects the language itself.
    pub language_detection: bool,

    /// Whether a dropped connection can be resumed without losing the session.
    pub reconnect: bool,

    /// Whether a batch endpoint exists to fall back to if streaming fails.
    pub batch_fallback: bool,
}

impl MeetingProviderCapabilities {
    /// A short, honest sentence for the UI. Never claims an unsupported feature.
    #[must_use]
    pub fn summary(&self) -> String {
        let mut parts = Vec::new();

        parts.push(if self.audio_leaves_device {
            "audio is sent to the provider".to_owned()
        } else {
            "audio stays on this machine".to_owned()
        });

        parts.push(match self.keyterms {
            KeytermSupport::Unsupported => "keyterms are ignored".to_owned(),
            KeytermSupport::Supported { max: Some(max), .. } => {
                format!("up to {max} keyterms")
            }
            KeytermSupport::Supported { max: None, .. } => "keyterms supported".to_owned(),
        });

        if let Some(limit) = self.session_limit {
            parts.push(format!(
                "sessions end after {} minutes",
                limit.as_secs() / 60
            ));
        }

        if self.speaker_labels {
            parts.push("speakers are labelled".to_owned());
        }

        parts.join("; ")
    }
}

/// Every provider this application *describes*.
///
/// Not the allowlist. Being in this table means the wire details are known well
/// enough to say what the provider would do; it does not mean a client exists.
/// [`is_allowed`] is derived from [`client`] instead, and this table is what the
/// settings screen renders so that a provider with no client can be listed and
/// explained rather than silently missing.
///
/// That separation is the whole point, and it was not here at first: `is_allowed`
/// used to be `capabilities(id).is_some()`, which returned `true` for all six of
/// these while no client existed for any of them — under a module doc claiming
/// the opposite. Upstream derives its `ALLOWED_MEETING_PROVIDERS` from its client
/// registry, with a comment worth preserving: a provider that is permitted but
/// has no client must never fall through to the OpenAI default.
pub const PROVIDERS: &[MeetingProviderCapabilities] = &[
    MeetingProviderCapabilities {
        id: "openai-realtime",
        name: "OpenAI Realtime",
        audio_leaves_device: true,
        // Accepted and ignored. Upstream includes keyterms in this provider's
        // connection cache key, which implies otherwise.
        keyterms: KeytermSupport::Unsupported,
        // A hard cap, so the client has to reconnect mid-meeting.
        session_limit: Some(Duration::from_secs(60 * 60)),
        token_rotation: false,
        sample_rates: &[24_000],
        speaker_labels: false,
        partials: true,
        language_detection: true,
        reconnect: true,
        batch_fallback: true,
    },
    MeetingProviderCapabilities {
        id: "assemblyai-realtime",
        name: "AssemblyAI Realtime",
        audio_leaves_device: true,
        keyterms: KeytermSupport::Supported {
            // Documented cap. Sending more is silently truncated, so the caller
            // needs to know before it decides which terms matter.
            max: Some(100),
            wire: KeytermWire::JsonQueryParam,
        },
        session_limit: None,
        token_rotation: false,
        sample_rates: &[16_000],
        speaker_labels: true,
        partials: true,
        language_detection: false,
        reconnect: true,
        batch_fallback: true,
    },
    MeetingProviderCapabilities {
        id: "deepgram-realtime",
        name: "Deepgram Realtime",
        audio_leaves_device: true,
        keyterms: KeytermSupport::Supported {
            max: None,
            wire: KeytermWire::RepeatedQueryParam,
        },
        session_limit: None,
        // The only provider that needs proactive token refresh mid-session.
        token_rotation: true,
        sample_rates: &[16_000, 24_000, 48_000],
        speaker_labels: true,
        partials: true,
        language_detection: true,
        reconnect: true,
        batch_fallback: true,
    },
    MeetingProviderCapabilities {
        id: "corti-realtime",
        name: "Corti Realtime",
        audio_leaves_device: true,
        keyterms: KeytermSupport::Supported {
            max: None,
            wire: KeytermWire::SessionConfig,
        },
        session_limit: None,
        token_rotation: false,
        sample_rates: &[16_000],
        speaker_labels: true,
        partials: true,
        // Corti requires a concrete primary language rather than detecting one.
        language_detection: false,
        reconnect: true,
        batch_fallback: true,
    },
    MeetingProviderCapabilities {
        id: "tinfoil-realtime",
        name: "Tinfoil Realtime",
        // Audio leaves the machine, into an attested enclave. Better than most,
        // but "leaves the device" is still the truthful answer.
        audio_leaves_device: true,
        keyterms: KeytermSupport::Unsupported,
        session_limit: None,
        token_rotation: false,
        sample_rates: &[16_000],
        speaker_labels: false,
        partials: true,
        language_detection: true,
        // The attested socket cannot be resumed; a drop restarts attestation.
        reconnect: false,
        batch_fallback: true,
    },
    MeetingProviderCapabilities {
        id: "local",
        // Not "streaming": it is repeated short passes over a local
        // `whisper-server`, about three seconds behind the room. Upstream's local
        // provider is a sherpa-onnx WebSocket server that does emit words as they
        // are recognised, and that runtime is blocked -- see chunk B4. Borrowing
        // its name would be the overclaim this project keeps finding upstream.
        name: "Local windowed model",
        // The only provider for which this is false, and the reason someone
        // would choose it.
        audio_leaves_device: false,
        keyterms: KeytermSupport::Unsupported,
        session_limit: None,
        token_rotation: false,
        sample_rates: &[16_000],
        speaker_labels: false,
        partials: true,
        language_detection: false,
        reconnect: true,
        batch_fallback: true,
    },
];

/// What this provider is described by, whether or not it can be used.
#[must_use]
pub fn capabilities(id: &str) -> Option<&'static MeetingProviderCapabilities> {
    PROVIDERS.iter().find(|p| p.id == id)
}

/// Which client implements a provider.
///
/// A name, not a constructor. Two reasons, and both were paid for.
///
/// The first is that an exhaustive `match` on this at the point where a session
/// is built is a coupling the compiler enforces: adding a variant stops every
/// call site compiling until it says what to do. A `fn` pointer or a
/// `Box<dyn …>` factory would move that from compile time to whatever the
/// factory happened to return.
///
/// The second is measured. Holding a function pointer to Tauri-dependent code
/// keeps the Tauri runtime alive in the test binary, and a `cargo test`
/// executable has no WebView2 beside it — the whole binary then dies at
/// `STATUS_ENTRYPOINT_NOT_FOUND` before a single test runs. See the 2026-08-26
/// entry in `docs/ENGINEERING_LOG.md`. Naming the client keeps the decision
/// testable and leaves the construction at the one site that has the arguments
/// for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Client {
    /// Repeated short passes over a local `whisper-server`. Not word-by-word
    /// streaming, and named so it cannot be mistaken for it.
    LocalWindow,
}

/// The client for `id`, or `None` when there is none.
///
/// This is the allowlist. A provider reaches it by having an implementation
/// here, so [`is_allowed`] cannot drift from what exists.
#[must_use]
pub fn client(id: &str) -> Option<Client> {
    match id {
        "local" => Some(Client::LocalWindow),
        _ => None,
    }
}

/// Why a provider is *refused* rather than merely unbuilt.
///
/// The distinction matters to the person reading the screen: one of these will
/// be built and the other will not, and "coming soon" over a deliberate refusal
/// is the kind of quiet dishonesty this project keeps finding in the upstream it
/// came from.
#[must_use]
pub fn declined(id: &str) -> Option<&'static str> {
    match id {
        // The same reason the batch Tinfoil client is refused, deliberately word
        // for word: a streaming socket carries more audio, not less.
        "tinfoil-realtime" => Some(
            "Tinfoil requires verifying an enclave attestation before any audio is sent. An              unattested WebSocket would transcribe and would provide none of the confidentiality              the name implies, so it is refused rather than approximated.",
        ),
        _ => None,
    }
}

/// Whether `id` may be used.
///
/// Derived from [`client`], so a provider cannot be permitted without an
/// implementation. That is the property upstream's comment is protecting: the
/// alternative is a permitted-but-clientless provider falling through to
/// whatever the default happens to be.
#[must_use]
pub fn is_allowed(id: &str) -> bool {
    client(id).is_some()
}

/// Whether a provider can be used, and if not, why not.
///
/// Three states rather than a boolean, because the three need different words on
/// the screen and lead to different actions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(
    tag = "state",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum Availability {
    /// A client exists. This provider can be selected.
    Available,
    /// No client is written yet. Not a promise that one will be.
    NoClient { reason: &'static str },
    /// Deliberately refused, and it stays refused until `reason` stops being
    /// true.
    Declined { reason: &'static str },
}

impl Availability {
    #[must_use]
    pub const fn is_available(self) -> bool {
        matches!(self, Self::Available)
    }
}

/// What can be said about `id` today.
#[must_use]
pub fn availability(id: &str) -> Availability {
    if is_allowed(id) {
        return Availability::Available;
    }
    match declined(id) {
        Some(reason) => Availability::Declined { reason },
        None => Availability::NoClient {
            reason: "No streaming client for this provider has been written yet.",
        },
    }
}

/// One provider as the settings screen needs it.
///
/// A flattened view rather than [`MeetingProviderCapabilities`] itself: that type
/// carries a `Duration`, which serialises as `{secs, nanos}` and is not what a
/// screen wants, and `&'static [u32]`. Named `Meeting…` because
/// `provider-registry` already exports a `ProviderSummary` and two Rust types
/// cannot share one TypeScript name — `ipc_types.rs` asserts on exactly that.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct MeetingProviderSummary {
    pub id: &'static str,
    pub name: &'static str,
    /// First, because it is the first thing a privacy-conscious user asks.
    pub audio_leaves_device: bool,
    pub keyterms: KeytermSupport,
    pub session_limit_seconds: Option<u64>,
    pub token_rotation: bool,
    pub sample_rates: Vec<u32>,
    pub speaker_labels: bool,
    pub partials: bool,
    pub language_detection: bool,
    pub reconnect: bool,
    pub batch_fallback: bool,
    pub availability: Availability,
    /// The same honest sentence [`MeetingProviderCapabilities::summary`] builds,
    /// rendered once here rather than reassembled in TypeScript.
    pub summary: String,
}

/// Every described provider, with whether it can actually be used.
///
/// Order follows [`PROVIDERS`] rather than sorting the usable ones first: the
/// screen should show the same list every time, and a provider moving position
/// when a client lands is a worse surprise than one being unavailable.
#[must_use]
pub fn listing() -> Vec<MeetingProviderSummary> {
    PROVIDERS
        .iter()
        .map(|p| MeetingProviderSummary {
            id: p.id,
            name: p.name,
            audio_leaves_device: p.audio_leaves_device,
            keyterms: p.keyterms,
            session_limit_seconds: p.session_limit.map(|d| d.as_secs()),
            token_rotation: p.token_rotation,
            sample_rates: p.sample_rates.to_vec(),
            speaker_labels: p.speaker_labels,
            partials: p.partials,
            language_detection: p.language_detection,
            reconnect: p.reconnect,
            batch_fallback: p.batch_fallback,
            availability: availability(p.id),
            summary: p.summary(),
        })
        .collect()
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

    #[test]
    fn the_allowlist_is_exactly_the_set_of_providers_with_clients() {
        // The invariant that stops a permitted-but-clientless provider falling
        // through to a default. It used to be checked against `PROVIDERS`, which
        // made it vacuous: `is_allowed` *was* `capabilities(id).is_some()`, so
        // the assertion could not fail and all six providers were permitted with
        // no client behind any of them.
        for provider in PROVIDERS {
            assert_eq!(
                is_allowed(provider.id),
                client(provider.id).is_some(),
                "{} is allowed iff it has a client",
                provider.id
            );
        }
        for bogus in [
            "openai",
            "assemblyai",
            "deepgram",
            "",
            "OPENAI-REALTIME",
            "LOCAL",
        ] {
            assert!(!is_allowed(bogus), "{bogus:?} should not be allowed");
        }
    }

    #[test]
    fn only_the_local_client_exists_so_far() {
        // A tripwire. When a cloud client lands this fails, which is the prompt
        // to flip its manifest entry and its PLATFORM_SUPPORT.md row in the same
        // commit rather than leaving them describing a state that has ended.
        let with_clients: Vec<&str> = PROVIDERS
            .iter()
            .map(|p| p.id)
            .filter(|id| client(id).is_some())
            .collect();
        assert_eq!(with_clients, vec!["local"]);
    }

    #[test]
    fn a_described_provider_with_no_client_is_listed_and_explained() {
        // The reason `PROVIDERS` and the allowlist are separate lists at all: a
        // provider the screen cannot offer still has to be visible, with a
        // reason, rather than absent.
        let listed = listing();
        assert_eq!(listed.len(), PROVIDERS.len());

        let deepgram = listed.iter().find(|p| p.id == "deepgram-realtime").unwrap();
        match deepgram.availability {
            Availability::NoClient { reason } => {
                assert!(reason.contains("written yet"), "{reason}")
            }
            other => panic!("{other:?}"),
        }
        assert!(!deepgram.availability.is_available());
    }

    #[test]
    fn tinfoil_is_declined_rather_than_merely_unbuilt() {
        // Two different futures, so two different words. "Coming soon" over a
        // deliberate refusal is the quiet dishonesty this project keeps finding
        // upstream.
        let listed = listing();
        let tinfoil = listed.iter().find(|p| p.id == "tinfoil-realtime").unwrap();
        match tinfoil.availability {
            Availability::Declined { reason } => {
                assert!(reason.contains("attestation"), "{reason}");
                assert!(reason.contains("refused"), "{reason}");
            }
            other => panic!("{other:?}"),
        }
        assert!(declined("deepgram-realtime").is_none());
    }

    #[test]
    fn the_summary_keeps_the_duration_out_of_the_wire() {
        // `Duration` serialises as {secs, nanos}, which is not something a screen
        // should have to know.
        let listed = listing();
        let openai = listed.iter().find(|p| p.id == "openai-realtime").unwrap();
        assert_eq!(openai.session_limit_seconds, Some(3_600));

        let json = serde_json::to_value(openai).unwrap();
        assert_eq!(json["sessionLimitSeconds"], 3_600);
        assert_eq!(json["audioLeavesDevice"], true);
        assert_eq!(json["availability"]["state"], "no-client");
    }

    #[test]
    fn the_local_provider_is_the_only_one_that_keeps_audio_on_the_machine() {
        let listed = listing();
        let local: Vec<&str> = listed
            .iter()
            .filter(|p| !p.audio_leaves_device)
            .map(|p| p.id)
            .collect();
        assert_eq!(local, vec!["local"]);

        // And it is the one that is available, which is not a coincidence worth
        // hiding: it is the only client that could be verified without a key.
        let local = listed.iter().find(|p| p.id == "local").unwrap();
        assert!(local.availability.is_available());
    }

    #[test]
    fn provider_ids_are_unique() {
        let mut ids: Vec<&str> = PROVIDERS.iter().map(|p| p.id).collect();
        let count = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), count, "duplicate provider id");
    }

    #[test]
    fn the_two_providers_that_ignore_keyterms_say_so() {
        // The specific asymmetry a uniform capability shape would hide, and the
        // one upstream's cache key implies is not there.
        for id in ["openai-realtime", "tinfoil-realtime"] {
            let caps = capabilities(id).unwrap();
            assert_eq!(caps.keyterms, KeytermSupport::Unsupported, "{id}");
            assert!(!caps.keyterms.is_supported());
            assert_eq!(
                caps.keyterms.effective_count(50),
                0,
                "{id} must report that none of the terms will be used"
            );
            assert!(
                caps.summary().contains("keyterms are ignored"),
                "{}",
                caps.summary()
            );
        }
    }

    #[test]
    fn assemblyai_reports_its_hundred_term_cap() {
        let caps = capabilities("assemblyai-realtime").unwrap();
        match caps.keyterms {
            KeytermSupport::Supported { max, wire } => {
                assert_eq!(max, Some(100));
                assert_eq!(wire, KeytermWire::JsonQueryParam);
            }
            other => panic!("{other:?}"),
        }

        // Under, at, and over the cap.
        assert_eq!(caps.keyterms.effective_count(40), 40);
        assert_eq!(caps.keyterms.effective_count(100), 100);
        assert_eq!(
            caps.keyterms.effective_count(250),
            100,
            "over the cap must truncate"
        );
        assert!(caps.summary().contains("up to 100 keyterms"));
    }

    #[test]
    fn the_three_keyterm_wire_shapes_are_all_represented() {
        // If these collapsed into one, a client would be serialising them wrong.
        let wires: Vec<KeytermWire> = PROVIDERS
            .iter()
            .filter_map(|p| match p.keyterms {
                KeytermSupport::Supported { wire, .. } => Some(wire),
                KeytermSupport::Unsupported => None,
            })
            .collect();

        assert!(wires.contains(&KeytermWire::JsonQueryParam));
        assert!(wires.contains(&KeytermWire::RepeatedQueryParam));
        assert!(wires.contains(&KeytermWire::SessionConfig));
    }

    #[test]
    fn openai_realtime_declares_its_session_cap_and_the_others_do_not() {
        let openai = capabilities("openai-realtime").unwrap();
        assert_eq!(openai.session_limit, Some(Duration::from_secs(3_600)));
        assert!(
            openai.summary().contains("60 minutes"),
            "{}",
            openai.summary()
        );

        for other in PROVIDERS.iter().filter(|p| p.id != "openai-realtime") {
            assert_eq!(other.session_limit, None, "{}", other.id);
        }
    }

    #[test]
    fn deepgram_is_the_only_provider_needing_token_rotation() {
        let rotating: Vec<&str> = PROVIDERS
            .iter()
            .filter(|p| p.token_rotation)
            .map(|p| p.id)
            .collect();
        assert_eq!(rotating, vec!["deepgram-realtime"]);
    }

    #[test]
    fn only_the_local_provider_keeps_audio_on_the_machine() {
        // The reason someone chooses it, so it must not be overstated for
        // anything else -- including Tinfoil, whose enclave is better than most
        // but is still off-device.
        let local: Vec<&str> = PROVIDERS
            .iter()
            .filter(|p| !p.audio_leaves_device)
            .map(|p| p.id)
            .collect();
        assert_eq!(local, vec!["local"]);

        let tinfoil = capabilities("tinfoil-realtime").unwrap();
        assert!(
            tinfoil.audio_leaves_device,
            "an attested enclave is still not this machine"
        );
        assert!(tinfoil.summary().contains("audio is sent to the provider"));
    }

    #[test]
    fn tinfoil_cannot_be_resumed_after_a_drop() {
        // Its attested socket has to redo attestation, so a reconnect is a new
        // session rather than a resumption.
        assert!(!capabilities("tinfoil-realtime").unwrap().reconnect);
    }

    #[test]
    fn every_provider_declares_at_least_one_sample_rate() {
        for provider in PROVIDERS {
            assert!(
                !provider.sample_rates.is_empty(),
                "{} declares no sample rate, so a caller cannot resample for it",
                provider.id
            );
            assert!(provider.sample_rates.iter().all(|r| *r >= 8_000));
        }
    }

    #[test]
    fn a_summary_never_claims_something_the_capabilities_deny() {
        for provider in PROVIDERS {
            let summary = provider.summary();
            if !provider.keyterms.is_supported() {
                assert!(
                    !summary.contains("up to") && !summary.contains("keyterms supported"),
                    "{}: {summary}",
                    provider.id
                );
            }
            if !provider.speaker_labels {
                assert!(
                    !summary.contains("speakers are labelled"),
                    "{}: {summary}",
                    provider.id
                );
            }
            if !provider.audio_leaves_device {
                assert!(
                    summary.contains("stays on this machine"),
                    "{}: {summary}",
                    provider.id
                );
            }
        }
    }

    #[test]
    fn capabilities_serialize_with_the_keys_the_frontend_expects() {
        let json = serde_json::to_value(capabilities("deepgram-realtime").unwrap()).unwrap();
        for key in [
            "id",
            "name",
            "audioLeavesDevice",
            "keyterms",
            "tokenRotation",
            "sampleRates",
            "speakerLabels",
            "partials",
            "languageDetection",
            "reconnect",
            "batchFallback",
        ] {
            assert!(json.get(key).is_some(), "missing {key}");
        }
        assert_eq!(json["keyterms"]["state"], "supported");

        let ignored = serde_json::to_value(capabilities("openai-realtime").unwrap()).unwrap();
        assert_eq!(ignored["keyterms"]["state"], "unsupported");
    }

    #[test]
    fn effective_count_handles_the_edges() {
        let capped = KeytermSupport::Supported {
            max: Some(3),
            wire: KeytermWire::JsonQueryParam,
        };
        assert_eq!(capped.effective_count(0), 0);
        assert_eq!(capped.effective_count(3), 3);
        assert_eq!(capped.effective_count(4), 3);

        let uncapped = KeytermSupport::Supported {
            max: None,
            wire: KeytermWire::RepeatedQueryParam,
        };
        assert_eq!(uncapped.effective_count(10_000), 10_000);
    }
}
