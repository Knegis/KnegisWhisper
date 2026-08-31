//! Meeting transcription: the provider contract and what each provider can do.
//!
//! ## What is here, and what is not
//!
//! This crate holds the *contract* — the trait a streaming client implements and
//! the capability model that says what each provider honours — and the clients
//! that exist. The capability model is new work rather than a port: upstream has
//! no capability object at all, and it is what lets calling code ask a question
//! like "will this provider honour my keyterms?" instead of having to know.
//!
//! **[`is_allowed`] is derived from [`client`], not from [`PROVIDERS`].** That
//! sentence is the correction of a real defect: it used to read
//! `capabilities(id).is_some()`, so all six described providers were permitted
//! while no client existed for any of them — under a module doc that claimed
//! exactly the opposite. Describing a provider and being able to use it are two
//! different facts and the code now keeps them apart, which is also what lets
//! the settings screen list a provider it cannot offer and say why.
//!
//! Of the six, `local`, `openai-realtime`, `assemblyai-realtime`,
//! `deepgram-realtime` and `corti-realtime` have clients — Corti's is the
//! most involved of the four cloud ones, because it authenticates with an
//! OAuth2 exchange before either socket opens. `tinfoil-realtime` is
//! [`declined`] rather than pending — see [`availability`]. None of the
//! four cloud clients has been run against a live service; each needs a key
//! or a tenant this project does not have.
//!
//! See `docs/PORTING_MATRIX.md` for the per-provider wire details each client
//! will need.

pub mod assemblyai;
pub mod assemblyai_socket;
pub mod capabilities;
pub mod corti;
pub mod corti_socket;
pub mod deepgram;
pub mod deepgram_socket;
pub mod fault;
pub mod local;
pub mod openai;
pub mod openai_socket;

pub use fault::{ends_the_session, FaultKind, SessionFault, SocketFault};

pub use capabilities::{
    availability, capabilities, client, credential_slot, declined, is_allowed, listing,
    listing_with_credentials, Client, ClientAvailability, KeytermSupport, KeytermWire,
    MeetingProviderCapabilities, MeetingProviderSummary, PROVIDERS,
};

use kw_security::KeyRef;
use serde::Serialize;

/// A chunk of audio to transcribe.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioChunk {
    /// 16-bit little-endian PCM, mono, at the provider's declared sample rate.
    pub pcm: Vec<u8>,
    /// Where it came from, so a transcript can attribute it when the provider
    /// does not label speakers.
    pub source: AudioSource,
}

/// Which capture a chunk came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ts_rs::TS)]
#[serde(rename_all = "kebab-case")]
pub enum AudioSource {
    /// The user's microphone.
    Mic,
    /// The system's output — everyone else in the meeting.
    System,
}

impl AudioSource {
    /// What to call this capture in a sentence a person reads.
    ///
    /// Used where a fault has to say which of a session's two sockets it came
    /// from — see [`fault::SessionFault`], which holds one cell for both.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Mic => "microphone",
            Self::System => "system audio",
        }
    }
}

/// How to connect.
///
/// Carries a credential *reference*, never a credential, for the same reason the
/// transcription route does: only the client resolves it, at the moment of use.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MeetingProviderConfig {
    pub provider: String,
    pub model: Option<String>,
    /// A concrete language, where the provider needs one.
    pub language: Option<String>,
    /// Terms to bias recognition towards. How many are used depends on the
    /// provider — see [`KeytermSupport::effective_count`].
    pub keyterms: Vec<String>,
    pub key_ref: Option<KeyRef>,
    pub sample_rate: u32,
}

/// One piece of transcript from a provider.
#[derive(Debug, Clone, PartialEq, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptEvent {
    pub text: String,
    /// False while the provider may still revise this text.
    pub is_final: bool,
    /// Seconds from the start of the session.
    pub timestamp: f64,
    pub speaker: Option<String>,
    pub source: AudioSource,
}

/// A finished meeting transcript.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MeetingTranscript {
    pub events: Vec<TranscriptEvent>,
    pub duration_seconds: f64,
    /// How many times the session had to reconnect. Surfaced because a
    /// transcript with gaps should not look complete.
    pub reconnects: u32,
    /// How many stretches of audio were never transcribed at all.
    ///
    /// A separate count from `reconnects` because the causes differ and so does
    /// what a user would do about them: a reconnect is the network, and a gap is
    /// the transcriber failing to keep up with the meeting. Both leave a hole,
    /// and a transcript with a hole in it must not look complete.
    pub gaps: u32,
}

/// Why a meeting session failed.
#[derive(Debug, thiserror::Error)]
pub enum MeetingError {
    #[error("{id:?} is not a meeting provider this application has a client for")]
    UnknownProvider { id: String },

    #[error("no credential is stored for {slot}. Add one in settings.")]
    MissingCredential { slot: String },

    #[error("{provider} refused the connection: {detail}")]
    Rejected { provider: String, detail: String },

    #[error("the connection to {provider} was lost and could not be resumed: {detail}")]
    Disconnected { provider: String, detail: String },

    /// The provider took the connection and then said something was wrong.
    ///
    /// Neither of the two variants above fits: `Rejected` says the connection
    /// was refused, and it was not; `Disconnected` says it was lost, and it may
    /// still be open. The gap between them is exactly where a provider-reported
    /// error frame used to fall through to a `tracing::warn!` and nothing else,
    /// leaving a meeting that transcribed nothing looking like a quiet room. See
    /// [`fault`].
    ///
    /// `detail` reaches the person in the room through
    /// `MeetingOutcome::Failed`, so it carries the provider's own sentence and
    /// which capture it came from rather than a code.
    #[error("{provider} accepted the connection and then stopped transcribing: {detail}")]
    ProviderFault { provider: String, detail: String },

    #[error(
        "{provider} accepts {accepted:?} Hz audio and was offered {offered} Hz. Resample \
         before sending, or choose a provider that accepts this rate."
    )]
    SampleRate {
        provider: String,
        offered: u32,
        accepted: &'static [u32],
    },

    #[error("no client is implemented for {provider} yet")]
    NotImplemented { provider: String },
}

/// A streaming meeting transcription provider.
///
/// The lifecycle is deliberately explicit — `pause` and `resume` exist because a
/// meeting gets interrupted and a session that has to be torn down and rebuilt
/// loses its context and its speaker labels.
#[async_trait::async_trait]
pub trait MeetingTranscriptionProvider: Send + Sync {
    /// Open the session.
    async fn connect(&mut self, config: MeetingProviderConfig) -> Result<(), MeetingError>;

    /// Send audio. Chunks must be at the sample rate agreed in `connect`.
    async fn send_audio(&mut self, chunk: AudioChunk) -> Result<(), MeetingError>;

    /// Stop sending without closing the session.
    async fn pause(&mut self) -> Result<(), MeetingError>;

    async fn resume(&mut self) -> Result<(), MeetingError>;

    /// Close the session and return everything transcribed.
    async fn finish(&mut self) -> Result<MeetingTranscript, MeetingError>;

    /// Abandon the session and discard the transcript.
    async fn cancel(&mut self) -> Result<(), MeetingError>;

    /// What this provider honours. Never a promise about a provider in general —
    /// a description of what this client does.
    fn capabilities(&self) -> &'static MeetingProviderCapabilities;
}

/// Check a configuration against a provider's declared capabilities.
///
/// Called before connecting, so a mismatch is an error the user can act on
/// rather than a silently degraded session. In particular it reports keyterm
/// truncation, which is otherwise invisible.
pub fn validate(config: &MeetingProviderConfig) -> Result<ConfigReview, MeetingError> {
    let caps = capabilities(&config.provider).ok_or_else(|| MeetingError::UnknownProvider {
        id: config.provider.clone(),
    })?;

    if !caps.sample_rates.contains(&config.sample_rate) {
        return Err(MeetingError::SampleRate {
            provider: config.provider.clone(),
            offered: config.sample_rate,
            accepted: caps.sample_rates,
        });
    }

    let requested = config.keyterms.len();
    let effective = caps.keyterms.effective_count(requested);

    Ok(ConfigReview {
        keyterms_requested: requested,
        keyterms_used: effective,
        keyterms_dropped: requested.saturating_sub(effective),
        language_required: !caps.language_detection && config.language.is_none(),
        session_limit_seconds: caps.session_limit.map(|d| d.as_secs()),
    })
}

/// What a configuration will actually do, once the provider's limits apply.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct ConfigReview {
    pub keyterms_requested: usize,
    pub keyterms_used: usize,
    /// Non-zero when terms will be silently discarded. Worth telling the user.
    pub keyterms_dropped: usize,
    /// True when the provider cannot detect a language and none was given.
    pub language_required: bool,
    pub session_limit_seconds: Option<u64>,
}

impl ConfigReview {
    /// Whether anything about this configuration is worth warning about.
    #[must_use]
    pub const fn has_warnings(&self) -> bool {
        self.keyterms_dropped > 0 || self.language_required
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

    fn config(provider: &str, rate: u32, keyterms: usize) -> MeetingProviderConfig {
        MeetingProviderConfig {
            provider: provider.to_owned(),
            model: None,
            language: Some("en".to_owned()),
            keyterms: (0..keyterms).map(|i| format!("term{i}")).collect(),
            key_ref: KeyRef::new("openai"),
            sample_rate: rate,
        }
    }

    #[test]
    fn an_unknown_provider_is_refused_before_connecting() {
        let err = validate(&config("deepgram", 16_000, 0)).unwrap_err();
        match err {
            MeetingError::UnknownProvider { id } => assert_eq!(id, "deepgram"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_sample_rate_the_provider_cannot_take_is_an_error_with_the_alternatives() {
        // OpenAI Realtime takes 24 kHz only, so sending 16 kHz would produce a
        // transcript of chipmunks rather than an error.
        let err = validate(&config("openai-realtime", 16_000, 0)).unwrap_err();
        match err {
            MeetingError::SampleRate {
                offered, accepted, ..
            } => {
                assert_eq!(offered, 16_000);
                assert_eq!(accepted, &[24_000]);
            }
            other => panic!("{other:?}"),
        }

        // And the right rate passes.
        assert!(validate(&config("openai-realtime", 24_000, 0)).is_ok());
    }

    #[test]
    fn keyterm_truncation_is_reported_rather_than_silent() {
        // AssemblyAI caps at 100 and truncates without complaint, so the user
        // needs to know which of their terms are being dropped.
        let review = validate(&config("assemblyai-realtime", 16_000, 150)).unwrap();
        assert_eq!(review.keyterms_requested, 150);
        assert_eq!(review.keyterms_used, 100);
        assert_eq!(review.keyterms_dropped, 50);
        assert!(review.has_warnings());
    }

    #[test]
    fn a_provider_that_ignores_keyterms_reports_all_of_them_as_dropped() {
        let review = validate(&config("openai-realtime", 24_000, 20)).unwrap();
        assert_eq!(review.keyterms_used, 0);
        assert_eq!(review.keyterms_dropped, 20);
        assert!(
            review.has_warnings(),
            "the user should be told these do nothing"
        );
    }

    #[test]
    fn keyterms_within_a_cap_produce_no_warning() {
        let review = validate(&config("assemblyai-realtime", 16_000, 10)).unwrap();
        assert_eq!(review.keyterms_dropped, 0);
        assert!(!review.has_warnings());
    }

    #[test]
    fn a_provider_that_cannot_detect_language_requires_one() {
        let mut cfg = config("corti-realtime", 16_000, 0);
        cfg.language = None;
        let review = validate(&cfg).unwrap();
        assert!(review.language_required);
        assert!(review.has_warnings());

        cfg.language = Some("sv".to_owned());
        assert!(!validate(&cfg).unwrap().language_required);
    }

    #[test]
    fn a_provider_that_detects_language_does_not_require_one() {
        let mut cfg = config("deepgram-realtime", 16_000, 0);
        cfg.language = None;
        assert!(!validate(&cfg).unwrap().language_required);
    }

    #[test]
    fn the_local_provider_does_not_require_a_language_either() {
        // `WhisperServer::transcribe` sends "auto" whenever no language is
        // given (crates/transcription-core/src/local.rs), which is real
        // per-request detection. The one provider that needs no credentials
        // and no network must not start refusing to run the moment something
        // finally calls `validate` before connecting it.
        let mut cfg = config("local", 16_000, 0);
        cfg.language = None;
        assert!(!validate(&cfg).unwrap().language_required);
    }

    #[test]
    fn the_session_limit_is_surfaced_so_a_caller_can_plan_a_reconnect() {
        let review = validate(&config("openai-realtime", 24_000, 0)).unwrap();
        assert_eq!(review.session_limit_seconds, Some(3_600));

        let review = validate(&config("deepgram-realtime", 16_000, 0)).unwrap();
        assert_eq!(review.session_limit_seconds, None);
    }

    #[test]
    fn every_provider_validates_at_its_own_first_declared_rate() {
        // Guards against a capability table that offers a rate validate rejects.
        for provider in PROVIDERS {
            let rate = provider.sample_rates[0];
            let result = validate(&config(provider.id, rate, 0));
            assert!(result.is_ok(), "{} at {rate} Hz: {result:?}", provider.id);
        }
    }

    #[test]
    fn a_config_carries_a_slot_and_never_a_credential() {
        let cfg = config("deepgram-realtime", 16_000, 0);
        let rendered = format!("{cfg:?}");
        assert!(rendered.contains("openai"), "the slot name is safe to show");
        assert!(!rendered.contains("sk-"));
    }

    #[test]
    fn a_transcript_reports_reconnects_so_gaps_are_visible() {
        // A transcript with gaps must not look complete.
        let transcript = MeetingTranscript {
            events: vec![TranscriptEvent {
                text: "hello".to_owned(),
                is_final: true,
                timestamp: 0.0,
                speaker: None,
                source: AudioSource::Mic,
            }],
            duration_seconds: 12.0,
            reconnects: 2,
            gaps: 1,
        };
        let json = serde_json::to_value(&transcript).unwrap();
        assert_eq!(json["reconnects"], 2);
        assert_eq!(
            json["gaps"], 1,
            "a gap is not the same failure as a reconnect"
        );
        assert_eq!(json["events"][0]["isFinal"], true);
        assert_eq!(json["events"][0]["source"], "mic");
    }
}
