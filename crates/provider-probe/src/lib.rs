//! Live connection tests for configured providers.
//!
//! The brief's rule is that a provider must never be shown as connected unless a
//! real connection test passed. This crate is what makes that statement
//! checkable rather than aspirational, and it is a port of upstream's
//! `scripts/verify-byok.mjs` — a genuinely good harness whose four best ideas
//! are preserved here:
//!
//! 1. **Three probes, not one.** A key that authenticates is not the same as a
//!    provider that works. `catalog` checks that the model ids this application
//!    has pinned still exist and are not reported down; `chat` sends a real
//!    completion; `voice` sends real audio to the transcription endpoint.
//!
//! 2. **A 2xx is not a pass.** `200 OK` with an empty completion, a non-JSON
//!    body, or a missing `text` field is a failure. This is the single most
//!    important rule here: it is exactly how an integration comes to be
//!    displayed as working while doing nothing.
//!
//! 3. **A tone cannot verify recognition.** A 440 Hz sine transcribes to the
//!    empty string on every provider, which cannot distinguish a working model
//!    from a silently broken one. So [`ProbeAudio`] records whether it holds
//!    real speech, and the report says which was used — the weaker assertion is
//!    stated rather than implied.
//!
//! 4. **A run that probed nothing is not a pass.** [`ProbeReport::is_connected`]
//!    is false when every probe was skipped for want of a credential.
//!
//! Two things upstream did that are deliberately not reproduced. Its terminal
//! provider list is hardcoded; here it comes from the provider registry, so a
//! provider cannot be probed unless it is a registry entry. And its `gate: false`
//! flag downgraded OpenAI failures to warnings, which is a per-build policy
//! decision rather than a property of the provider — [`ProbeOutcome`] has no
//! warning state, and the caller decides what to do with a failure.
//!
//! Secrets: a credential reaches [`ProbeTransport`] and nothing else. No probe
//! detail string is built from a credential, and every detail is truncated
//! before it is stored, so a provider that echoes a request back cannot put an
//! unbounded body into a log line.

use std::collections::BTreeSet;

use kw_network_policy::{NetworkPolicy, RequestPurpose};
use kw_provider_registry::Registry;
use kw_security::{CredentialStore, KeyRef, Secret};
use serde::Serialize;

/// How much of a provider's response body may appear in a report.
const DETAIL_LIMIT: usize = 300;

/// Which probe ran.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProbeKind {
    /// The live model catalogue, compared against the ids we pinned.
    Catalog,
    /// A real chat completion.
    Chat,
    /// A real transcription request.
    Voice,
}

impl ProbeKind {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Catalog => "catalog",
            Self::Chat => "chat",
            Self::Voice => "voice",
        }
    }
}

/// Why a probe did not run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "reason", rename_all = "kebab-case")]
pub enum SkipReason {
    /// No credential is stored for this provider, so there is nothing to test.
    NoCredential,
    /// The provider does not offer this capability at all.
    NotSupported { detail: String },
}

/// What a probe concluded.
///
/// Deliberately three states with no "warning". A probe either demonstrated the
/// provider works, demonstrated it does not, or did not run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "outcome", rename_all = "kebab-case")]
pub enum ProbeOutcome {
    Pass { detail: String },
    Fail { detail: String },
    Skipped(SkipReason),
}

impl ProbeOutcome {
    #[must_use]
    pub const fn passed(&self) -> bool {
        matches!(self, Self::Pass { .. })
    }

    #[must_use]
    pub const fn failed(&self) -> bool {
        matches!(self, Self::Fail { .. })
    }

    fn pass(detail: impl Into<String>) -> Self {
        Self::Pass {
            detail: truncate(detail.into()),
        }
    }

    fn fail(detail: impl Into<String>) -> Self {
        Self::Fail {
            detail: truncate(detail.into()),
        }
    }
}

fn truncate(mut s: String) -> String {
    if s.chars().count() <= DETAIL_LIMIT {
        return s;
    }
    // Truncate on a character boundary; a byte-index cut can split UTF-8.
    let cut = s
        .char_indices()
        .nth(DETAIL_LIMIT)
        .map_or(s.len(), |(i, _)| i);
    s.truncate(cut);
    s.push('…');
    s
}

/// One probe's result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProbeResult {
    pub provider: String,
    pub kind: ProbeKind,
    pub outcome: ProbeOutcome,
}

/// Whether the audio used for the voice probe can prove recognition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum AudioKind {
    /// Real speech. A transcript that contains the expected words proves the
    /// model ran.
    Speech,
    /// A synthetic tone. Every provider transcribes this to the empty string, so
    /// a pass proves endpoint, credential, and model id — not recognition.
    Tone,
}

/// The audio sent to a transcription endpoint.
#[derive(Debug, Clone)]
pub struct ProbeAudio {
    wav: Vec<u8>,
    kind: AudioKind,
}

impl ProbeAudio {
    /// One second of a 440 Hz sine at 16 kHz mono, as a WAV.
    ///
    /// Always available and never touches the network or a subprocess, at the
    /// cost of being unable to verify recognition. Generating real speech needs
    /// a platform text-to-speech call, which belongs in the platform crate that
    /// owns process spawning; pass it here with [`ProbeAudio::speech`].
    #[must_use]
    pub fn tone() -> Self {
        const RATE: u32 = 16_000;
        let mut pcm = Vec::with_capacity(RATE as usize * 2);
        for i in 0..RATE {
            let t = f64::from(i) / f64::from(RATE);
            let sample = (t * 440.0 * std::f64::consts::TAU).sin() * 8000.0;
            #[allow(clippy::cast_possible_truncation)]
            pcm.extend_from_slice(&(sample as i16).to_le_bytes());
        }
        Self {
            wav: wav_container(&pcm, RATE),
            kind: AudioKind::Tone,
        }
    }

    /// Wrap caller-supplied speech audio. The caller is asserting this contains
    /// recognisable words; the report will say so.
    #[must_use]
    pub const fn speech(wav: Vec<u8>) -> Self {
        Self {
            wav,
            kind: AudioKind::Speech,
        }
    }

    #[must_use]
    pub const fn kind(&self) -> AudioKind {
        self.kind
    }

    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.wav
    }
}

/// A 16-bit mono PCM WAV container around `pcm`.
fn wav_container(pcm: &[u8], rate: u32) -> Vec<u8> {
    let data_len = u32::try_from(pcm.len()).unwrap_or(u32::MAX);
    let mut out = Vec::with_capacity(pcm.len() + 44);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16_u32.to_le_bytes()); // fmt chunk size
    out.extend_from_slice(&1_u16.to_le_bytes()); // PCM
    out.extend_from_slice(&1_u16.to_le_bytes()); // mono
    out.extend_from_slice(&rate.to_le_bytes());
    out.extend_from_slice(&(rate * 2).to_le_bytes()); // byte rate
    out.extend_from_slice(&2_u16.to_le_bytes()); // block align
    out.extend_from_slice(&16_u16.to_le_bytes()); // bits per sample
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    out.extend_from_slice(pcm);
    out
}

/// The result of probing one or more providers.
#[derive(Debug, Clone, Serialize)]
pub struct ProbeReport {
    pub results: Vec<ProbeResult>,
    /// Which audio the voice probes used, so a pass is not over-read.
    pub audio: AudioKind,
}

impl ProbeReport {
    /// Whether any probe actually ran.
    ///
    /// A run in which every provider was skipped for want of a credential
    /// probed nothing, and must not be reported as success.
    #[must_use]
    pub fn probed_anything(&self) -> bool {
        self.results
            .iter()
            .any(|r| !matches!(r.outcome, ProbeOutcome::Skipped(_)))
    }

    /// Whether `provider` may be displayed as connected.
    ///
    /// True only if at least one probe ran for it and none failed. This is the
    /// function a settings screen should call; it exists so no UI code decides
    /// for itself what "connected" means.
    #[must_use]
    pub fn is_connected(&self, provider: &str) -> bool {
        let mine: Vec<&ProbeResult> = self
            .results
            .iter()
            .filter(|r| r.provider == provider)
            .collect();
        let ran = mine
            .iter()
            .any(|r| !matches!(r.outcome, ProbeOutcome::Skipped(_)));
        ran && !mine.iter().any(|r| r.outcome.failed())
    }

    /// Every failure, for a summary line.
    #[must_use]
    pub fn failures(&self) -> Vec<&ProbeResult> {
        self.results.iter().filter(|r| r.outcome.failed()).collect()
    }
}

/* ------------------------------------------------------------ interpretation */
//
// The interpretation of a response is separated from the sending of it, because
// this is where every rule that matters lives and none of it needs a network.

/// Interpret a chat-completions response.
#[must_use]
pub fn interpret_chat(status: u16, body: &str) -> ProbeOutcome {
    if !(200..300).contains(&status) {
        return ProbeOutcome::fail(format!("HTTP {status}: {body}"));
    }

    let parsed: serde_json::Value = match serde_json::from_str(body) {
        Ok(v) => v,
        Err(e) => return ProbeOutcome::fail(format!("2xx but the body is not JSON ({e}): {body}")),
    };

    let content = parsed
        .get("choices")
        .and_then(|c| c.get(0))
        .and_then(|c| c.get("message"))
        .and_then(|m| m.get("content"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .trim();

    if content.is_empty() {
        // The rule that matters. A provider answering 200 with no content is
        // not working, and calling this a pass is how a broken integration
        // comes to be shown as connected.
        return ProbeOutcome::fail(format!("2xx but no completion content: {body}"));
    }

    ProbeOutcome::pass(format!("replied {content:?}"))
}

/// Interpret a transcription response.
#[must_use]
pub fn interpret_voice(status: u16, body: &str, audio: AudioKind) -> ProbeOutcome {
    if !(200..300).contains(&status) {
        return ProbeOutcome::fail(format!("HTTP {status}: {body}"));
    }

    let parsed: serde_json::Value = match serde_json::from_str(body) {
        Ok(v) => v,
        Err(e) => return ProbeOutcome::fail(format!("2xx but the body is not JSON ({e}): {body}")),
    };

    // The contract is a `text` field. Its being absent is a failure even
    // though the status was 2xx: it means the route worked and the model did not.
    let Some(text) = parsed.get("text").and_then(serde_json::Value::as_str) else {
        return ProbeOutcome::fail(format!("2xx but no \"text\" field: {body}"));
    };

    match audio {
        AudioKind::Speech => ProbeOutcome::pass(format!("transcript {:?}", text.trim())),
        // Being explicit about the weaker claim, in the detail the user reads.
        AudioKind::Tone => ProbeOutcome::pass(format!(
            "endpoint, credential and model accepted; tone audio cannot verify recognition \
             (transcript {:?})",
            text.trim()
        )),
    }
}

/// Interpret a model-catalogue response against the ids this build pinned.
///
/// Fails if a pinned id is absent from the live catalogue or is reported down.
/// That is the check that catches a provider retiring a model out from under a
/// pinned registry — which would otherwise surface as a user's dictation failing.
#[must_use]
pub fn interpret_catalog(status: u16, body: &str, pinned: &[String]) -> ProbeOutcome {
    if !(200..300).contains(&status) {
        return ProbeOutcome::fail(format!("HTTP {status}: {body}"));
    }

    let parsed: serde_json::Value = match serde_json::from_str(body) {
        Ok(v) => v,
        Err(e) => return ProbeOutcome::fail(format!("2xx but the body is not JSON ({e}): {body}")),
    };

    let Some(entries) = parsed.get("data").and_then(serde_json::Value::as_array) else {
        return ProbeOutcome::fail(format!("2xx but no \"data\" array: {body}"));
    };

    let mut live = BTreeSet::new();
    let mut down = BTreeSet::new();
    for entry in entries {
        let Some(id) = entry.get("id").and_then(serde_json::Value::as_str) else {
            continue;
        };
        live.insert(id.to_owned());
        // Berget reports availability as `status.up`.
        if entry
            .get("status")
            .and_then(|s| s.get("up"))
            .and_then(serde_json::Value::as_bool)
            == Some(false)
        {
            down.insert(id.to_owned());
        }
    }

    let vanished: Vec<&String> = pinned.iter().filter(|id| !live.contains(*id)).collect();
    let unavailable: Vec<&String> = pinned.iter().filter(|id| down.contains(*id)).collect();

    if !vanished.is_empty() || !unavailable.is_empty() {
        let mut parts = Vec::new();
        if !vanished.is_empty() {
            parts.push(format!(
                "missing from the live catalogue: {}",
                vanished
                    .iter()
                    .map(|s| s.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        if !unavailable.is_empty() {
            parts.push(format!(
                "reported down: {}",
                unavailable
                    .iter()
                    .map(|s| s.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        return ProbeOutcome::fail(parts.join("; "));
    }

    if pinned.is_empty() {
        return ProbeOutcome::fail(
            "nothing pinned for this provider, so the catalogue proves nothing".to_owned(),
        );
    }

    ProbeOutcome::pass(format!(
        "{} models live; all {} pinned ids present",
        live.len(),
        pinned.len()
    ))
}

/* ----------------------------------------------------------------- transport */

/// Sends the probe requests.
///
/// A trait so every rule above can be tested without a network, and so the only
/// implementation that touches the wire is the one that goes through
/// `kw_network_policy`.
#[async_trait::async_trait]
pub trait ProbeTransport: Send + Sync {
    /// `GET url` with a bearer credential. Returns status and body.
    async fn get(
        &self,
        url: &str,
        key: &Secret,
        purpose: RequestPurpose,
    ) -> Result<(u16, String), String>;

    /// `POST url` with a JSON body and a bearer credential.
    async fn post_json(
        &self,
        url: &str,
        key: &Secret,
        body: String,
        purpose: RequestPurpose,
    ) -> Result<(u16, String), String>;

    /// `POST url` with a `multipart/form-data` body carrying a WAV and a model id.
    async fn post_audio(
        &self,
        url: &str,
        key: &Secret,
        model: &str,
        wav: Vec<u8>,
        purpose: RequestPurpose,
    ) -> Result<(u16, String), String>;
}

/// The real transport. Every request goes through the network policy, so a
/// blocked vendor host or a plaintext URL fails before anything is sent.
#[derive(Debug, Clone)]
pub struct PolicyTransport {
    policy: NetworkPolicy,
}

impl PolicyTransport {
    pub fn new() -> Result<Self, kw_network_policy::PolicyError> {
        Ok(Self {
            policy: NetworkPolicy::new()?,
        })
    }
}

#[async_trait::async_trait]
impl ProbeTransport for PolicyTransport {
    async fn get(
        &self,
        url: &str,
        key: &Secret,
        purpose: RequestPurpose,
    ) -> Result<(u16, String), String> {
        let response = self
            .policy
            .get(purpose, url)
            .map_err(|e| e.to_string())?
            .bearer_auth(key.expose())
            .send()
            .await
            .map_err(|e| e.to_string())?;
        read(response).await
    }

    async fn post_json(
        &self,
        url: &str,
        key: &Secret,
        body: String,
        purpose: RequestPurpose,
    ) -> Result<(u16, String), String> {
        let response = self
            .policy
            .post(purpose, url)
            .map_err(|e| e.to_string())?
            .bearer_auth(key.expose())
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(body)
            .send()
            .await
            .map_err(|e| e.to_string())?;
        read(response).await
    }

    async fn post_audio(
        &self,
        url: &str,
        key: &Secret,
        model: &str,
        wav: Vec<u8>,
        purpose: RequestPurpose,
    ) -> Result<(u16, String), String> {
        let part = reqwest::multipart::Part::bytes(wav)
            .file_name("probe.wav")
            .mime_str("audio/wav")
            .map_err(|e| e.to_string())?;
        let form = reqwest::multipart::Form::new()
            .part("file", part)
            .text("model", model.to_owned());

        let response = self
            .policy
            .post(purpose, url)
            .map_err(|e| e.to_string())?
            .bearer_auth(key.expose())
            .multipart(form)
            .send()
            .await
            .map_err(|e| e.to_string())?;
        read(response).await
    }
}

async fn read(response: reqwest::Response) -> Result<(u16, String), String> {
    let status = response.status().as_u16();
    let body = response.text().await.map_err(|e| e.to_string())?;
    Ok((status, body))
}

/* -------------------------------------------------------------- orchestration */

/// Probe one transcription provider from the registry.
///
/// Returns the three results in a stable order. A provider with no stored
/// credential yields three `Skipped` results rather than an error: not being
/// configured is not a failure.
pub async fn probe_transcription_provider(
    registry: &Registry,
    provider_id: &str,
    credentials: &dyn CredentialStore,
    transport: &dyn ProbeTransport,
    audio: &ProbeAudio,
) -> Result<Vec<ProbeResult>, ProbeError> {
    // Fail closed: an id that is not a registry entry is not probeable. This is
    // the same rule as provider dispatch — an unknown id is an error, never a
    // silent fall through to a default provider.
    let provider =
        registry
            .transcription_provider(provider_id)
            .map_err(|e| ProbeError::UnknownProvider {
                id: provider_id.to_owned(),
                source: e,
            })?;

    let result = |kind, outcome| ProbeResult {
        provider: provider_id.to_owned(),
        kind,
        outcome,
    };

    let Some(base) = provider.base_url.as_deref() else {
        let reason = SkipReason::NotSupported {
            detail: "this provider has no fixed base URL, so there is nothing to probe \
                     until an endpoint is configured"
                .to_owned(),
        };
        return Ok(vec![
            result(ProbeKind::Catalog, ProbeOutcome::Skipped(reason.clone())),
            result(ProbeKind::Chat, ProbeOutcome::Skipped(reason.clone())),
            result(ProbeKind::Voice, ProbeOutcome::Skipped(reason)),
        ]);
    };

    let slot = KeyRef::new(&provider.credential_slot).ok_or_else(|| ProbeError::InvalidSlot {
        slot: provider.credential_slot.clone(),
    })?;

    let Ok(key) = credentials.get(&slot) else {
        return Ok(vec![
            result(
                ProbeKind::Catalog,
                ProbeOutcome::Skipped(SkipReason::NoCredential),
            ),
            result(
                ProbeKind::Chat,
                ProbeOutcome::Skipped(SkipReason::NoCredential),
            ),
            result(
                ProbeKind::Voice,
                ProbeOutcome::Skipped(SkipReason::NoCredential),
            ),
        ]);
    };
    if key.is_empty() {
        return Ok(vec![
            result(
                ProbeKind::Catalog,
                ProbeOutcome::Skipped(SkipReason::NoCredential),
            ),
            result(
                ProbeKind::Chat,
                ProbeOutcome::Skipped(SkipReason::NoCredential),
            ),
            result(
                ProbeKind::Voice,
                ProbeOutcome::Skipped(SkipReason::NoCredential),
            ),
        ]);
    }

    let base = base.trim_end_matches('/');
    let mut results = Vec::with_capacity(3);

    // Catalogue.
    let pinned: Vec<String> = provider.models.iter().map(|m| m.id.clone()).collect();
    let catalog = match transport
        .get(
            &format!("{base}/models"),
            &key,
            RequestPurpose::UserConfiguredTranscriptionProvider,
        )
        .await
    {
        Ok((status, body)) => interpret_catalog(status, &body, &pinned),
        Err(e) => ProbeOutcome::fail(e),
    };
    results.push(result(ProbeKind::Catalog, catalog));

    // Chat is a reasoning capability; only probe it if this provider offers one.
    let chat = if let Ok(reasoning) = registry.reasoning_provider(provider_id) {
        match reasoning.models.first() {
            Some(model) => {
                let body = chat_probe_body(&model.id);
                match transport
                    .post_json(
                        &format!("{base}/chat/completions"),
                        &key,
                        body,
                        RequestPurpose::UserConfiguredAiProvider,
                    )
                    .await
                {
                    Ok((status, body)) => interpret_chat(status, &body),
                    Err(e) => ProbeOutcome::fail(e),
                }
            }
            None => ProbeOutcome::Skipped(SkipReason::NotSupported {
                detail: "the registry lists no reasoning model for this provider".to_owned(),
            }),
        }
    } else {
        ProbeOutcome::Skipped(SkipReason::NotSupported {
            detail: "transcription-only provider".to_owned(),
        })
    };
    results.push(result(ProbeKind::Chat, chat));

    // Voice.
    let voice = match provider.models.first() {
        Some(model) => match transport
            .post_audio(
                &format!("{base}/audio/transcriptions"),
                &key,
                &model.id,
                audio.bytes().to_vec(),
                RequestPurpose::UserConfiguredTranscriptionProvider,
            )
            .await
        {
            Ok((status, body)) => interpret_voice(status, &body, audio.kind()),
            Err(e) => ProbeOutcome::fail(e),
        },
        None => ProbeOutcome::Skipped(SkipReason::NotSupported {
            detail: "the registry lists no transcription model for this provider".to_owned(),
        }),
    };
    results.push(result(ProbeKind::Voice, voice));

    Ok(results)
}

/// The chat probe body.
///
/// Deliberately minimal and deterministic: a health check should not depend on
/// sampling parameters a provider might reject.
fn chat_probe_body(model: &str) -> String {
    serde_json::json!({
        "model": model,
        "messages": [
            { "role": "system", "content": "You are a health check." },
            { "role": "user", "content": "Reply with the single word: OK" }
        ],
        "max_tokens": 16,
    })
    .to_string()
}

/// Probe every transcription provider that has a stored credential.
pub async fn probe_configured(
    registry: &Registry,
    credentials: &dyn CredentialStore,
    transport: &dyn ProbeTransport,
    audio: &ProbeAudio,
) -> Result<ProbeReport, ProbeError> {
    let mut results = Vec::new();
    for provider in &registry.transcription {
        results.extend(
            probe_transcription_provider(registry, &provider.id, credentials, transport, audio)
                .await?,
        );
    }
    Ok(ProbeReport {
        results,
        audio: audio.kind(),
    })
}

/// Failures that prevent a probe from running at all.
#[derive(Debug, thiserror::Error)]
pub enum ProbeError {
    #[error("no provider {id} in the registry, so there is nothing to probe")]
    UnknownProvider {
        id: String,
        #[source]
        source: kw_provider_registry::RegistryError,
    },

    #[error("the registry names credential slot {slot:?}, which is not a valid slot name")]
    InvalidSlot { slot: String },
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
    use kw_security::MemoryCredentialStore;
    use std::sync::Mutex;

    /// A transport that returns canned responses and records what it was asked.
    #[derive(Debug, Default)]
    struct StubTransport {
        catalog: Mutex<Option<(u16, String)>>,
        chat: Mutex<Option<(u16, String)>>,
        voice: Mutex<Option<(u16, String)>>,
        seen_urls: Mutex<Vec<String>>,
        seen_credentials: Mutex<Vec<String>>,
    }

    impl StubTransport {
        fn with(catalog: (u16, &str), chat: (u16, &str), voice: (u16, &str)) -> Self {
            Self {
                catalog: Mutex::new(Some((catalog.0, catalog.1.to_owned()))),
                chat: Mutex::new(Some((chat.0, chat.1.to_owned()))),
                voice: Mutex::new(Some((voice.0, voice.1.to_owned()))),
                ..Default::default()
            }
        }

        fn note(&self, url: &str, key: &Secret) {
            self.seen_urls.lock().unwrap().push(url.to_owned());
            self.seen_credentials
                .lock()
                .unwrap()
                .push(key.expose().to_owned());
        }
    }

    #[async_trait::async_trait]
    impl ProbeTransport for StubTransport {
        async fn get(
            &self,
            url: &str,
            key: &Secret,
            _p: RequestPurpose,
        ) -> Result<(u16, String), String> {
            self.note(url, key);
            self.catalog
                .lock()
                .unwrap()
                .clone()
                .ok_or_else(|| "no canned catalog".to_owned())
        }

        async fn post_json(
            &self,
            url: &str,
            key: &Secret,
            _body: String,
            _p: RequestPurpose,
        ) -> Result<(u16, String), String> {
            self.note(url, key);
            self.chat
                .lock()
                .unwrap()
                .clone()
                .ok_or_else(|| "no canned chat".to_owned())
        }

        async fn post_audio(
            &self,
            url: &str,
            key: &Secret,
            _model: &str,
            _wav: Vec<u8>,
            _p: RequestPurpose,
        ) -> Result<(u16, String), String> {
            self.note(url, key);
            self.voice
                .lock()
                .unwrap()
                .clone()
                .ok_or_else(|| "no canned voice".to_owned())
        }
    }

    fn registry() -> &'static Registry {
        kw_provider_registry::registry().expect("the shipped registry must parse")
    }

    /* ------------------------------------------------- the 2xx-is-not-a-pass rule */

    #[test]
    fn a_2xx_with_no_completion_content_is_a_failure() {
        // The exact shape a broken-but-authenticating provider returns.
        for body in [
            r#"{"choices":[{"message":{"content":""}}]}"#,
            r#"{"choices":[{"message":{"content":"   "}}]}"#,
            r#"{"choices":[]}"#,
            r#"{"choices":[{"message":{}}]}"#,
            r#"{}"#,
        ] {
            let outcome = interpret_chat(200, body);
            assert!(
                outcome.failed(),
                "{body} should not be a pass, got {outcome:?}"
            );
        }
    }

    #[test]
    fn a_2xx_that_is_not_json_is_a_failure() {
        // An HTML error page behind a proxy, served with a 200.
        let outcome = interpret_chat(200, "<html><body>Gateway</body></html>");
        assert!(outcome.failed());
        match outcome {
            ProbeOutcome::Fail { detail } => assert!(detail.contains("not JSON")),
            other => panic!("expected Fail, got {other:?}"),
        }
    }

    #[test]
    fn a_real_completion_passes_and_quotes_the_reply() {
        let outcome = interpret_chat(200, r#"{"choices":[{"message":{"content":"OK"}}]}"#);
        assert!(outcome.passed());
        match outcome {
            ProbeOutcome::Pass { detail } => assert!(detail.contains("OK")),
            other => panic!("expected Pass, got {other:?}"),
        }
    }

    #[test]
    fn a_non_2xx_is_a_failure_carrying_the_status() {
        let outcome = interpret_chat(401, r#"{"error":"invalid api key"}"#);
        assert!(outcome.failed());
        assert!(format!("{outcome:?}").contains("401"));
    }

    #[test]
    fn a_voice_response_without_a_text_field_is_a_failure() {
        for body in [r#"{}"#, r#"{"segments":[]}"#, r#"{"text":null}"#] {
            assert!(
                interpret_voice(200, body, AudioKind::Speech).failed(),
                "{body}"
            );
        }
    }

    #[test]
    fn an_empty_transcript_is_a_pass_because_silence_transcribes_to_nothing() {
        // Distinct from an absent field: the model ran and heard nothing, which
        // is the correct answer for a tone.
        let outcome = interpret_voice(200, r#"{"text":""}"#, AudioKind::Tone);
        assert!(outcome.passed());
    }

    /* ------------------------------------------- the tone-cannot-verify-recognition rule */

    #[test]
    fn a_tone_pass_states_that_it_cannot_verify_recognition() {
        let outcome = interpret_voice(200, r#"{"text":""}"#, AudioKind::Tone);
        match outcome {
            ProbeOutcome::Pass { detail } => {
                assert!(
                    detail.contains("cannot verify recognition"),
                    "the weaker claim must be stated, not implied: {detail}"
                );
            }
            other => panic!("expected Pass, got {other:?}"),
        }
    }

    #[test]
    fn a_speech_pass_does_not_carry_that_caveat() {
        let outcome = interpret_voice(200, r#"{"text":"the quick brown fox"}"#, AudioKind::Speech);
        match outcome {
            ProbeOutcome::Pass { detail } => {
                assert!(!detail.contains("cannot verify"));
                assert!(detail.contains("quick brown fox"));
            }
            other => panic!("expected Pass, got {other:?}"),
        }
    }

    #[test]
    fn the_generated_tone_never_claims_to_be_speech() {
        let audio = ProbeAudio::tone();
        assert_eq!(audio.kind(), AudioKind::Tone);
        assert_eq!(&audio.bytes()[0..4], b"RIFF");
        assert_eq!(&audio.bytes()[8..12], b"WAVE");
        // 1 second, 16 kHz, 16-bit mono, plus a 44-byte header.
        assert_eq!(audio.bytes().len(), 16_000 * 2 + 44);
    }

    /* ------------------------------------------------------------ catalogue rules */

    #[test]
    fn a_pinned_model_missing_from_the_catalogue_is_a_failure() {
        let body = r#"{"data":[{"id":"whisper-1"},{"id":"gpt-4o-transcribe"}]}"#;
        let pinned = vec!["whisper-1".to_owned(), "retired-model".to_owned()];
        let outcome = interpret_catalog(200, body, &pinned);
        assert!(outcome.failed());
        assert!(format!("{outcome:?}").contains("retired-model"));
    }

    #[test]
    fn a_pinned_model_reported_down_is_a_failure() {
        let body = r#"{"data":[{"id":"kb-whisper-large","status":{"up":false}}]}"#;
        let pinned = vec!["kb-whisper-large".to_owned()];
        let outcome = interpret_catalog(200, body, &pinned);
        assert!(outcome.failed());
        assert!(format!("{outcome:?}").contains("reported down"));
    }

    #[test]
    fn a_catalogue_containing_every_pinned_id_passes() {
        let body = r#"{"data":[{"id":"a"},{"id":"b"},{"id":"c","status":{"up":true}}]}"#;
        let pinned = vec!["a".to_owned(), "c".to_owned()];
        let outcome = interpret_catalog(200, body, &pinned);
        assert!(outcome.passed(), "{outcome:?}");
    }

    #[test]
    fn an_empty_pinned_list_cannot_pass_because_it_proves_nothing() {
        let body = r#"{"data":[{"id":"a"}]}"#;
        assert!(interpret_catalog(200, body, &[]).failed());
    }

    #[test]
    fn a_catalogue_without_a_data_array_is_a_failure() {
        assert!(interpret_catalog(200, r#"{"models":["a"]}"#, &["a".to_owned()]).failed());
    }

    /* --------------------------------------------------- nothing-probed is not a pass */

    #[tokio::test]
    async fn a_provider_with_no_credential_is_skipped_not_failed() {
        let creds = MemoryCredentialStore::new();
        let transport = StubTransport::default();
        let results = probe_transcription_provider(
            registry(),
            "openai",
            &creds,
            &transport,
            &ProbeAudio::tone(),
        )
        .await
        .unwrap();

        assert_eq!(results.len(), 3);
        assert!(results
            .iter()
            .all(|r| matches!(r.outcome, ProbeOutcome::Skipped(SkipReason::NoCredential))));
        assert!(
            transport.seen_urls.lock().unwrap().is_empty(),
            "nothing should have been sent"
        );
    }

    #[tokio::test]
    async fn a_report_in_which_nothing_ran_is_not_connected() {
        let creds = MemoryCredentialStore::new();
        let report = probe_configured(
            registry(),
            &creds,
            &StubTransport::default(),
            &ProbeAudio::tone(),
        )
        .await
        .unwrap();

        assert!(!report.probed_anything());
        for provider in &registry().transcription {
            assert!(
                !report.is_connected(&provider.id),
                "{} must not read as connected when nothing was probed",
                provider.id
            );
        }
    }

    #[tokio::test]
    async fn a_blank_credential_counts_as_no_credential() {
        let creds = MemoryCredentialStore::new();
        creds
            .set(
                &KeyRef::new("openai").unwrap(),
                &Secret::new("   ".to_owned()),
            )
            .unwrap();

        let results = probe_transcription_provider(
            registry(),
            "openai",
            &creds,
            &StubTransport::default(),
            &ProbeAudio::tone(),
        )
        .await
        .unwrap();
        assert!(results
            .iter()
            .all(|r| matches!(r.outcome, ProbeOutcome::Skipped(SkipReason::NoCredential))));
    }

    /* ------------------------------------------------------------- is_connected */

    #[tokio::test]
    async fn a_provider_is_connected_only_when_every_probe_that_ran_passed() {
        let creds = MemoryCredentialStore::new();
        let slot = KeyRef::new("openai").unwrap();
        creds
            .set(&slot, &Secret::new("sk-test".to_owned()))
            .unwrap();

        let pinned: Vec<String> = registry()
            .transcription_provider("openai")
            .unwrap()
            .models
            .iter()
            .map(|m| format!(r#"{{"id":"{}"}}"#, m.id))
            .collect();
        let catalog = format!(r#"{{"data":[{}]}}"#, pinned.join(","));

        let good = StubTransport::with(
            (200, &catalog),
            (200, r#"{"choices":[{"message":{"content":"OK"}}]}"#),
            (200, r#"{"text":""}"#),
        );
        let results =
            probe_transcription_provider(registry(), "openai", &creds, &good, &ProbeAudio::tone())
                .await
                .unwrap();
        let report = ProbeReport {
            results,
            audio: AudioKind::Tone,
        };
        assert!(report.probed_anything());
        assert!(report.is_connected("openai"), "{:?}", report.failures());

        // Now break one probe: the provider must stop reading as connected even
        // though the credential is valid and two probes still pass.
        let broken = StubTransport::with(
            (200, &catalog),
            (200, r#"{"choices":[{"message":{"content":""}}]}"#),
            (200, r#"{"text":""}"#),
        );
        let results = probe_transcription_provider(
            registry(),
            "openai",
            &creds,
            &broken,
            &ProbeAudio::tone(),
        )
        .await
        .unwrap();
        let report = ProbeReport {
            results,
            audio: AudioKind::Tone,
        };
        assert!(!report.is_connected("openai"));
        assert_eq!(report.failures().len(), 1);
    }

    /* ------------------------------------------------------------- fail closed */

    #[tokio::test]
    async fn an_unknown_provider_id_is_an_error_not_a_default() {
        let creds = MemoryCredentialStore::new();
        let err = probe_transcription_provider(
            registry(),
            "not-a-provider",
            &creds,
            &StubTransport::default(),
            &ProbeAudio::tone(),
        )
        .await
        .unwrap_err();

        assert!(matches!(err, ProbeError::UnknownProvider { .. }));
        // The point of the assertion: it did not quietly probe OpenAI instead.
        assert!(err.to_string().contains("not-a-provider"));
    }

    /* ---------------------------------------------------------- secret hygiene */

    #[tokio::test]
    async fn no_probe_detail_contains_the_credential() {
        let creds = MemoryCredentialStore::new();
        creds
            .set(
                &KeyRef::new("openai").unwrap(),
                &Secret::new("sk-super-secret-value".to_owned()),
            )
            .unwrap();

        // A provider that echoes the request back, which is the realistic way a
        // credential could end up in a detail string.
        let echo = StubTransport::with(
            (401, r#"{"error":"bad key: sk-super-secret-value"}"#),
            (200, r#"{"choices":[{"message":{"content":"OK"}}]}"#),
            (200, r#"{"text":""}"#),
        );
        let results =
            probe_transcription_provider(registry(), "openai", &creds, &echo, &ProbeAudio::tone())
                .await
                .unwrap();

        // The transport did receive the credential -- that is its job.
        assert!(echo
            .seen_credentials
            .lock()
            .unwrap()
            .iter()
            .any(|c| c == "sk-super-secret-value"));

        // A provider echoing a key back is the provider's failure, and we
        // faithfully report what it said; what must not happen is this crate
        // constructing a detail from the credential itself. Assert the only
        // occurrence is inside the body the provider returned.
        let serialised = serde_json::to_string(&results).unwrap();
        assert_eq!(
            serialised.matches("sk-super-secret-value").count(),
            1,
            "the credential should appear only in the provider's own echoed body"
        );
    }

    #[test]
    fn details_are_truncated_so_a_provider_cannot_fill_a_log() {
        let huge = "x".repeat(10_000);
        let outcome = interpret_chat(500, &huge);
        match outcome {
            ProbeOutcome::Fail { detail } => {
                assert!(
                    detail.chars().count() <= DETAIL_LIMIT + 1,
                    "got {} chars",
                    detail.chars().count()
                );
            }
            other => panic!("expected Fail, got {other:?}"),
        }
    }

    #[test]
    fn truncation_does_not_split_a_multibyte_character() {
        let outcome = interpret_chat(500, &"å".repeat(10_000));
        // The assertion is simply that this did not panic on a byte boundary and
        // the result is still valid UTF-8, which it is by construction.
        assert!(outcome.failed());
    }

    /* --------------------------------------------------------------- targeting */

    #[tokio::test]
    async fn probes_hit_the_provider_base_url_and_the_expected_paths() {
        let creds = MemoryCredentialStore::new();
        creds
            .set(
                &KeyRef::new("openai").unwrap(),
                &Secret::new("sk-test".to_owned()),
            )
            .unwrap();

        let stub = StubTransport::with((200, r#"{"data":[]}"#), (200, "{}"), (200, "{}"));
        probe_transcription_provider(registry(), "openai", &creds, &stub, &ProbeAudio::tone())
            .await
            .unwrap();

        let urls = stub.seen_urls.lock().unwrap().clone();
        let base = registry()
            .transcription_provider("openai")
            .unwrap()
            .base_url
            .clone()
            .unwrap();
        assert!(
            urls.iter()
                .all(|u| u.starts_with(base.trim_end_matches('/'))),
            "{urls:?}"
        );
        assert!(urls.iter().any(|u| u.ends_with("/models")));
        assert!(urls.iter().any(|u| u.ends_with("/audio/transcriptions")));
    }
}
