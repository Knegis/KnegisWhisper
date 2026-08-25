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

/// How a provider expects keyterms on the wire.
///
/// The differences are not cosmetic: each needs different serialisation, and one
/// of them silently truncates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "kebab-case")]
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

/// Every provider this application has a client for.
///
/// The allowlist is **derived from this table** — see [`is_allowed`]. Upstream
/// derives its `ALLOWED_MEETING_PROVIDERS` from its client registry for exactly
/// this reason, with a comment worth preserving: a provider that is permitted
/// but has no client must never fall through to the OpenAI default.
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
        name: "Local streaming model",
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

/// Capabilities for `id`, or `None` if there is no client for it.
#[must_use]
pub fn capabilities(id: &str) -> Option<&'static MeetingProviderCapabilities> {
    PROVIDERS.iter().find(|p| p.id == id)
}

/// Whether `id` may be used.
///
/// Derived from [`PROVIDERS`] rather than kept as a separate list, so a provider
/// cannot be permitted without a client. That is the property upstream's comment
/// is protecting: the alternative is a permitted-but-clientless provider falling
/// through to whatever the default happens to be.
#[must_use]
pub fn is_allowed(id: &str) -> bool {
    capabilities(id).is_some()
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
        // through to a default.
        for provider in PROVIDERS {
            assert!(
                is_allowed(provider.id),
                "{} has a client but is not allowed",
                provider.id
            );
        }
        for bogus in ["openai", "assemblyai", "deepgram", "", "OPENAI-REALTIME"] {
            assert!(!is_allowed(bogus), "{bogus:?} should not be allowed");
        }
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
