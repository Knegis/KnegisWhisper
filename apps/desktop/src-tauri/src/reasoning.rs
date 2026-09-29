//! Reasoning configuration: what tidies a dictation, and where that request goes.
//!
//! `crates/reasoning-core` has done the work since phase 3 — route resolution
//! derived from the registry, per-model parameter shaping, and a cleanup pass
//! that returns the original transcript on any failure. None of it was
//! configurable. `DictationRuntime::build` was handed `None` for cleanup with a
//! comment explaining that a half-configured cleanup is worse than none, and
//! nothing could ever make it a `Some`.
//!
//! This module is what makes it a `Some`.
//!
//! # Why cleanup is off by default and stays that way
//!
//! A raw transcript is a *correct answer*. Cleanup is an improvement on it, and
//! an improvement that needs a cloud provider, a credential, and a model that
//! all agree. Defaulting it on would mean a first dictation that either fails
//! for a reason the user did not ask about, or silently sends their speech to a
//! provider they never chose. So: off until switched on, and the switch names
//! exactly where the text will go.

use std::sync::{Arc, RwLock};

use kw_provider_registry::Registry;
use kw_reasoning_core::enterprise::EnterpriseConfig;
use kw_reasoning_core::{ChatRoute, ChatSettings};
use serde::{Deserialize, Serialize};
use tauri::State;
use ts_rs::TS;

use crate::dictation::DictationRuntime;

/// Where the cleanup configuration lives.
pub const CLEANUP_SETTINGS_KEY: &str = "reasoning.cleanup";

/// The live cleanup destination, shared with `ModelCleanup`.
///
/// `None` means off. `ModelCleanup` reads it on every dictation, so a change
/// here takes effect on the next one without restarting anything.
pub type CleanupSlot = Arc<RwLock<Option<(ChatRoute, String)>>>;

/// What the user configured, as stored.
///
/// Kept separate from [`ChatSettings`] rather than reusing it, because the
/// reasoning crate's own comment is worth honouring: chat settings must never be
/// read as cleanup settings. Two structs cannot be confused for each other; one
/// struct with a mode field can.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(optional_fields = nullable)]
pub struct CleanupSettings {
    /// Off unless explicitly switched on.
    pub enabled: bool,
    /// A cloud or enterprise provider id from the registry.
    pub provider: Option<String>,
    /// The model to send to. Required: there is no safe default across seven
    /// providers, and `params::shape_for` needs the real id to pick the right
    /// token parameter.
    pub model: Option<String>,
    /// An endpoint the user runs. Wins over `provider`, as in `resolve_chat`.
    pub self_hosted_url: Option<String>,
    /// Whether `provider` names an enterprise connector rather than a cloud one.
    pub enterprise: bool,
}

impl CleanupSettings {
    /// The chat settings this resolves through.
    fn chat(&self) -> ChatSettings {
        ChatSettings {
            provider: self.provider.clone(),
            self_hosted_url: self.self_hosted_url.clone(),
            enterprise: self.enterprise,
        }
    }

    /// The route and model to hand `ModelCleanup`, or why there is none.
    ///
    /// Returns `Ok(None)` for "switched off", which is different from an error:
    /// off is a working configuration.
    fn resolve(&self, registry: &Registry) -> Result<Option<(ChatRoute, String)>, String> {
        if !self.enabled {
            return Ok(None);
        }

        let model = self
            .model
            .as_deref()
            .map(str::trim)
            .filter(|m| !m.is_empty())
            .ok_or_else(|| {
                "cleanup needs a model. Providers disagree about parameter names, so there \
                 is no safe default to fall back on."
                    .to_owned()
            })?;

        let route =
            kw_reasoning_core::resolve_chat(registry, &self.chat()).map_err(|e| e.to_string())?;

        Ok(Some((route, model.to_owned())))
    }
}

/// Whether a reasoning provider can actually be used for a request today.
///
/// The same three-state shape as `commands::Dispatch`, and a separate type
/// because the reasons differ: a transcription provider is blocked on a client,
/// a reasoning provider on a request *shape* or on a sidecar.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(
    tag = "state",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum ReasoningDispatch {
    /// OpenAI-compatible. A request would really be sent.
    Ready,
    /// Bedrock, Azure or Vertex. The request shape and the signing exist; what
    /// is missing is the organisation's own credentials, which are multi-field
    /// and have their own form.
    NeedsConfig { reason: String },
    /// A llama.cpp server this application would manage, and does not yet.
    NeedsSidecar { reason: String },
    /// The route cannot be resolved at all.
    Unresolvable { reason: String },
}

/// One model a reasoning provider offers.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ReasoningModelSummary {
    pub id: String,
    pub name: String,
    /// `max_tokens` or `max_completion_tokens`. Shown because getting it wrong
    /// is a 400 that reads as a broken integration rather than a wrong field.
    pub token_param: String,
    pub supports_thinking: bool,
    pub supports_vision: bool,
}

/// One reasoning provider, and whether it could be used.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ReasoningProviderSummary {
    pub id: String,
    pub name: String,
    /// `None` for enterprise connectors, whose credentials are multi-field.
    pub credential_slot: Option<String>,
    /// A credential is stored. Not the same as the provider working.
    pub configured: bool,
    pub enterprise: bool,
    pub dispatch: ReasoningDispatch,
    pub models: Vec<ReasoningModelSummary>,
}

/// Where a cleanup request would go right now.
///
/// The *resolved* destination rather than the stored setting, for the same
/// reason `dictation::RouteReport` is: a provider with no credential resolves
/// differently from what the setting says, and showing the setting would be
/// showing an intention.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(
    tag = "state",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum CleanupRoute {
    /// Switched off. The raw transcript is what gets inserted.
    Off,
    /// Configured and resolvable.
    Ready {
        /// A short phrase for the UI: `openai`, `self-hosted`, and so on.
        destination: String,
        model: String,
        /// Whether the transcript leaves this machine on this route.
        leaves_device: bool,
        /// Set when the destination resolves but could not actually be used —
        /// no credential stored, or a shape that is not implemented.
        caveat: Option<String>,
    },
    /// Switched on and unusable, with the reason.
    Problem { reason: String },
}

/// One enterprise connector's configuration, as the screen sees it.
///
/// Field *names* and whether each is filled, never a value. There is no command
/// that reads an enterprise configuration back, for the same reason there is
/// none for an API key: a screen that cannot display a secret cannot leak one.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct EnterpriseField {
    pub name: String,
    pub filled: bool,
}

/// What is configured for one enterprise connector.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct EnterpriseStatus {
    pub provider: String,
    pub name: String,
    /// Whether anything is stored at all.
    pub configured: bool,
    /// Present when something is stored: which fields it has.
    pub fields: Vec<EnterpriseField>,
    /// Present when something is stored and cannot be read, or is incomplete.
    pub problem: Option<String>,
}

/// Everything the Intelligence screen shows.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ReasoningStatus {
    pub settings: CleanupSettings,
    pub route: CleanupRoute,
    pub providers: Vec<ReasoningProviderSummary>,
    pub enterprise: Vec<EnterpriseStatus>,
}

/// What would happen if a request were sent to this provider.
///
/// `local_running` is the port of a running llama.cpp server, or `None`. Passed
/// in rather than looked up, so this stays a pure function of what is true.
fn dispatch_for(
    registry: &Registry,
    id: &str,
    enterprise: bool,
    local_running: Option<u16>,
) -> ReasoningDispatch {
    let settings = ChatSettings {
        provider: Some(id.to_owned()),
        self_hosted_url: None,
        enterprise,
    };

    match kw_reasoning_core::resolve_chat(registry, &settings) {
        Ok(ChatRoute::Enterprise { id }) => ReasoningDispatch::NeedsConfig {
            reason: format!(
                "{id} needs its organisation credentials, set under Enterprise connectors below"
            ),
        },
        Ok(ChatRoute::Local) => {
            if local_running.is_some() {
                ReasoningDispatch::Ready
            } else {
                // The server exists in this build; it just is not running. A
                // request would be refused rather than sent to a dead port, and
                // saying "not running" points at the screen that starts it.
                ReasoningDispatch::NeedsSidecar {
                    reason: "no local model is running. Start one under Settings > Local text models."
                        .to_owned(),
                }
            }
        }
        Ok(_) => ReasoningDispatch::Ready,
        Err(err) => ReasoningDispatch::Unresolvable {
            reason: err.to_string(),
        },
    }
}

/// Describe the configured route, including why it would not work.
fn describe(
    registry: &Registry,
    settings: &CleanupSettings,
    credentials: &dyn kw_security::CredentialStore,
    local_running: Option<u16>,
) -> CleanupRoute {
    match settings.resolve(registry) {
        Ok(None) => CleanupRoute::Off,
        Err(reason) => CleanupRoute::Problem { reason },
        Ok(Some((route, model))) => {
            let (destination, leaves_device, caveat) = match &route {
                ChatRoute::SelfHosted { base_url, .. } => (
                    format!("self-hosted at {base_url}"),
                    // A loopback or LAN address does not leave the machine in
                    // the sense that matters; anything else does.
                    !base_url.contains("127.0.0.1") && !base_url.contains("localhost"),
                    None,
                ),
                ChatRoute::Local => (
                    "local model".to_owned(),
                    false,
                    if local_running.is_some() {
                        None
                    } else {
                        Some(
                            "no local model is running, so cleanup will leave the transcript as \
                             it is until one is started under Settings > Local text models"
                                .to_owned(),
                        )
                    },
                ),
                ChatRoute::Provider { id, key_ref, .. } => (
                    id.clone(),
                    true,
                    if credentials.contains(key_ref) {
                        None
                    } else {
                        Some(format!(
                            "no key is stored for {}. Cleanup will leave the transcript as it is \
                             until one is, on the Providers screen.",
                            key_ref.slot()
                        ))
                    },
                ),
                ChatRoute::Enterprise { id } => {
                    let configured = kw_security::KeyRef::new(id)
                        .is_some_and(|slot| credentials.contains(&slot));
                    (
                        id.clone(),
                        true,
                        if configured {
                            None
                        } else {
                            Some(format!(
                                "{id} has no configuration stored, so cleanup will leave the \
                                 transcript as it is until one is set"
                            ))
                        },
                    )
                }
            };

            CleanupRoute::Ready {
                destination,
                model,
                leaves_device,
                caveat,
            }
        }
    }
}

/// Read the stored cleanup settings.
async fn stored(db: &kw_storage::Db) -> CleanupSettings {
    kw_storage::settings::get(db.pool(), CLEANUP_SETTINGS_KEY)
        .await
        .ok()
        .flatten()
        .unwrap_or_default()
}

/// Everything the Intelligence screen needs, with no network access.
#[tauri::command]
pub async fn reasoning_status(
    state: State<'_, crate::state::AppState>,
    llm: State<'_, Arc<crate::local_llm::LocalLlm>>,
) -> Result<ReasoningStatus, String> {
    let registry = kw_provider_registry::registry().map_err(|e| e.to_string())?;
    let settings = stored(state.db()).await;
    // Read without recording a use: a settings window left open must not keep
    // the idle timer from ever firing.
    let local_running = llm.running_port();

    let mut providers: Vec<ReasoningProviderSummary> = registry
        .reasoning
        .iter()
        .map(|p| ReasoningProviderSummary {
            id: p.id.clone(),
            name: p.name.clone(),
            credential_slot: Some(p.credential_slot.clone()),
            configured: kw_security::KeyRef::new(&p.credential_slot)
                .is_some_and(|slot| state.credentials().contains(&slot)),
            enterprise: false,
            dispatch: dispatch_for(registry, &p.id, false, local_running),
            models: p
                .models
                .iter()
                .map(|m| ReasoningModelSummary {
                    id: m.id.clone(),
                    name: m.name.clone(),
                    token_param: m.token_param.clone(),
                    supports_thinking: m.supports_thinking,
                    supports_vision: m.supports_vision,
                })
                .collect(),
        })
        .collect();

    // Enterprise connectors are listed alongside rather than hidden. They are
    // real registry entries, and a screen that omits them says nothing about
    // why — whereas listing them with `needs-shape` says exactly why.
    providers.extend(registry.enterprise.iter().map(|p| {
        ReasoningProviderSummary {
            id: p.id.clone(),
            name: p.name.clone(),
            // Multi-field credentials: region and profile, or endpoint and
            // deployment, or a service-account JSON. There is no single slot.
            credential_slot: None,
            configured: false,
            enterprise: true,
            dispatch: dispatch_for(registry, &p.id, true, local_running),
            models: p
                .models
                .iter()
                .map(|m| ReasoningModelSummary {
                    id: m.id.clone(),
                    name: m.name.clone(),
                    // Not in the registry for enterprise models; the OpenAI default
                    // is what `shape_for` would pick.
                    token_param: "max_tokens".to_owned(),
                    supports_thinking: false,
                    supports_vision: false,
                })
                .collect(),
        }
    }));

    let enterprise = registry
        .enterprise
        .iter()
        .map(|p| enterprise_status(&p.id, &p.name, state.credentials()))
        .collect();

    // The local route is a real destination and is not in either registry list,
    // so it is added here. Without it the screen offers ten cloud providers and
    // no way to point cleanup at a model on this machine -- which is the one
    // route that needs no key and sends nothing anywhere.
    providers.insert(
        0,
        ReasoningProviderSummary {
            id: "local".to_owned(),
            name: "On this machine (llama.cpp)".to_owned(),
            credential_slot: None,
            configured: local_running.is_some(),
            enterprise: false,
            dispatch: dispatch_for(registry, "local", false, local_running),
            models: registry
                .local_llm
                .iter()
                .flat_map(|p| {
                    p.models.iter().map(|m| ReasoningModelSummary {
                        id: m.id.clone(),
                        name: m.name.clone(),
                        // llama.cpp speaks the OpenAI shape and takes
                        // `max_tokens`; there is no second spelling to worry
                        // about on a server we run ourselves.
                        token_param: "max_tokens".to_owned(),
                        supports_thinking: m.supports_thinking,
                        supports_vision: false,
                    })
                })
                .collect(),
        },
    );

    Ok(ReasoningStatus {
        route: describe(registry, &settings, state.credentials(), local_running),
        settings,
        providers,
        enterprise,
    })
}

/// Read one connector's stored configuration and report its shape.
fn enterprise_status(
    id: &str,
    name: &str,
    credentials: &dyn kw_security::CredentialStore,
) -> EnterpriseStatus {
    let empty = EnterpriseStatus {
        provider: id.to_owned(),
        name: name.to_owned(),
        configured: false,
        fields: Vec::new(),
        problem: None,
    };

    let Some(slot) = kw_security::KeyRef::new(id) else {
        return empty;
    };
    let Some(stored) = credentials.get(&slot).ok().filter(|s| !s.is_empty()) else {
        return empty;
    };

    match serde_json::from_str::<EnterpriseConfig>(stored.expose()) {
        Ok(config) => {
            // A stored configuration that no longer validates is worse than
            // none: it reads as configured and fails at send time. Say so here.
            let problem = config.validate().err();
            EnterpriseStatus {
                configured: true,
                fields: config
                    .filled_fields()
                    .into_iter()
                    .map(|(name, filled)| EnterpriseField {
                        name: name.to_owned(),
                        filled,
                    })
                    .collect(),
                problem,
                ..empty
            }
        }
        Err(err) => EnterpriseStatus {
            configured: true,
            problem: Some(format!(
                "something is stored for {id} and it could not be read: {err}"
            )),
            ..empty
        },
    }
}

/// Store an enterprise connector's configuration.
///
/// Validated before it is stored, so an incomplete configuration is refused
/// where the user is looking at it rather than at the end of their next
/// dictation. The whole document goes into one OS-credential-store slot; nothing
/// enterprise-shaped is written to SQLite.
#[tauri::command]
pub fn enterprise_config_set(
    state: State<'_, crate::state::AppState>,
    config: EnterpriseConfig,
) -> Result<(), String> {
    config.validate()?;

    let id = config.provider();
    let slot = kw_security::KeyRef::new(id).ok_or_else(|| format!("{id} is not a valid slot"))?;
    let json = serde_json::to_string(&config).map_err(|e| e.to_string())?;

    state
        .credentials()
        .set(&slot, &kw_security::Secret::new(json))
        .map_err(|e| format!("could not store the {id} configuration: {e}"))
}

/// Remove an enterprise connector's configuration.
#[tauri::command]
pub fn enterprise_config_clear(
    state: State<'_, crate::state::AppState>,
    provider: String,
) -> Result<(), String> {
    let slot = kw_security::KeyRef::new(&provider)
        .ok_or_else(|| format!("{provider} is not a valid slot"))?;
    state
        .credentials()
        .delete(&slot)
        .map_err(|e| format!("could not remove the {provider} configuration: {e}"))
}

/// Store a cleanup configuration and apply it to the running session.
///
/// Both, in one command, because they fail independently and the user needs the
/// second one: a setting that is stored and not applied is a setting that will
/// work after a restart, which is not what "Save" means.
///
/// Refuses a configuration that cannot resolve rather than storing it. A stored
/// setting that silently does nothing is the shape of defect this whole screen
/// exists to remove.
#[tauri::command]
pub async fn reasoning_set_cleanup(
    state: State<'_, crate::state::AppState>,
    runtime: State<'_, Arc<DictationRuntime>>,
    llm: State<'_, Arc<crate::local_llm::LocalLlm>>,
    settings: CleanupSettings,
) -> Result<CleanupRoute, String> {
    let registry = kw_provider_registry::registry().map_err(|e| e.to_string())?;

    // Resolve before storing. An unresolvable configuration is refused here,
    // where the user is looking at it.
    let resolved = settings.resolve(registry)?;

    kw_storage::settings::set(state.db().pool(), CLEANUP_SETTINGS_KEY, &settings)
        .await
        .map_err(|e| e.to_string())?;

    if let Ok(mut slot) = runtime.cleanup_slot().write() {
        *slot = resolved;
    }

    Ok(describe(
        registry,
        &settings,
        state.credentials(),
        llm.running_port(),
    ))
}

/// Apply the stored cleanup configuration at startup.
///
/// Spawned rather than awaited: the window should not wait on a settings read,
/// and cleanup being off for the first second after launch is indistinguishable
/// from cleanup being off, which is the default anyway.
///
/// A configuration that no longer resolves — a provider removed from the
/// registry, say — leaves cleanup off and logs why, rather than failing startup.
/// The raw transcript is a correct answer.
pub fn apply_at_startup(runtime: &Arc<DictationRuntime>, db: &kw_storage::Db) {
    let runtime = Arc::clone(runtime);
    let db = db.clone();

    tauri::async_runtime::spawn(async move {
        let settings = stored(&db).await;
        if !settings.enabled {
            return;
        }

        let Ok(registry) = kw_provider_registry::registry() else {
            return;
        };

        match settings.resolve(registry) {
            Ok(resolved) => {
                if let Ok(mut slot) = runtime.cleanup_slot().write() {
                    *slot = resolved;
                }
                tracing::info!(
                    provider = settings.provider.as_deref().unwrap_or("self-hosted"),
                    "dictation cleanup is on"
                );
            }
            Err(reason) => tracing::warn!(
                %reason,
                "dictation cleanup is switched on and cannot be resolved; \
                 transcripts will be inserted as they are"
            ),
        }
    });
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
    use kw_security::CredentialStore as _;

    fn reg() -> &'static Registry {
        kw_provider_registry::registry().expect("registry")
    }

    /// Off resolves to nothing, and that is not an error.
    #[test]
    fn off_is_a_working_configuration() {
        let settings = CleanupSettings::default();
        assert_eq!(settings.resolve(reg()), Ok(None));
    }

    /// A model is required, because there is no safe default across providers.
    #[test]
    fn switching_it_on_without_a_model_is_refused_rather_than_guessed() {
        let settings = CleanupSettings {
            enabled: true,
            provider: Some("openai".to_owned()),
            ..CleanupSettings::default()
        };
        let err = settings.resolve(reg()).expect_err("must refuse");
        assert!(err.contains("model"), "{err}");

        // And a blank one is not a model.
        let blank = CleanupSettings {
            model: Some("   ".to_owned()),
            ..settings
        };
        assert!(blank.resolve(reg()).is_err());
    }

    /// An unknown provider is refused, not classified as local.
    ///
    /// This is the upstream bug the reasoning crate was written around: anything
    /// outside a hardcoded set became a *local* model, so a request went to
    /// loopback and failed while blaming the local model.
    #[test]
    fn an_unknown_provider_is_refused_rather_than_treated_as_local() {
        let settings = CleanupSettings {
            enabled: true,
            provider: Some("not-a-provider".to_owned()),
            model: Some("some-model".to_owned()),
            ..CleanupSettings::default()
        };
        let err = settings.resolve(reg()).expect_err("must refuse");
        assert!(err.contains("not-a-provider"), "{err}");
    }

    /// The three enterprise connectors report the configuration gap.
    ///
    /// It used to be a *shape* gap, which was true until `enterprise.rs` wrote
    /// the three request shapes. Reporting a missing feature for what is now a
    /// missing setting would send someone looking in the wrong place.
    #[test]
    fn enterprise_connectors_say_they_need_their_organisation_credentials() {
        for id in ["bedrock", "azure", "vertex"] {
            match dispatch_for(reg(), id, true, None) {
                ReasoningDispatch::NeedsConfig { reason } => {
                    assert!(reason.contains(id), "{id}: {reason}");
                }
                other => panic!("{id} should need a configuration, got {other:?}"),
            }
        }
    }

    /// A stored configuration is reported by shape, never by value.
    #[test]
    fn an_enterprise_status_carries_field_names_and_no_values() {
        let store = kw_security::MemoryCredentialStore::default();
        let slot = kw_security::KeyRef::new("bedrock").expect("slot");
        store
            .set(
                &slot,
                &kw_security::Secret::new(
                    serde_json::to_string(&EnterpriseConfig::Bedrock {
                        region: "eu-west-1".to_owned(),
                        access_key_id: "AKIDEXAMPLE".to_owned(),
                        secret_access_key: "a-real-secret".to_owned(),
                        session_token: None,
                    })
                    .expect("serialize"),
                ),
            )
            .expect("store");

        let status = enterprise_status("bedrock", "AWS Bedrock", &store);
        assert!(status.configured);
        assert!(status.problem.is_none());

        let printed = format!("{status:?}");
        assert!(!printed.contains("a-real-secret"), "{printed}");
        assert!(!printed.contains("AKIDEXAMPLE"), "{printed}");
        assert!(
            status
                .fields
                .iter()
                .any(|f| f.name == "secretAccessKey" && f.filled),
            "{:?}",
            status.fields
        );
    }

    /// A stored blob that is not a configuration reads as a problem, not as
    /// configured and working.
    #[test]
    fn an_unreadable_stored_configuration_is_reported_as_a_problem() {
        let store = kw_security::MemoryCredentialStore::default();
        let slot = kw_security::KeyRef::new("azure").expect("slot");
        store
            .set(&slot, &kw_security::Secret::new("not json".to_owned()))
            .expect("store");

        let status = enterprise_status("azure", "Azure OpenAI", &store);
        assert!(status.configured);
        let problem = status.problem.expect("a problem");
        assert!(problem.contains("could not be read"), "{problem}");
    }

    /// Nothing stored is nothing stored, not an empty configuration.
    #[test]
    fn an_unconfigured_connector_reports_nothing_rather_than_empty_fields() {
        let store = kw_security::MemoryCredentialStore::default();
        let status = enterprise_status("vertex", "GCP Vertex AI", &store);
        assert!(!status.configured);
        assert!(status.fields.is_empty());
        assert!(status.problem.is_none());
    }

    /// The local route is ready when a server is running and not before.
    ///
    /// Reporting it ready with nothing running would be a selectable option that
    /// fails at the end of the next dictation, which is the exact shape of
    /// defect the three-state dispatch exists to prevent.
    #[test]
    fn the_local_route_is_ready_only_while_a_server_runs() {
        assert!(matches!(
            dispatch_for(reg(), "local", false, None),
            ReasoningDispatch::NeedsSidecar { .. }
        ));
        assert!(matches!(
            dispatch_for(reg(), "local", false, Some(8221)),
            ReasoningDispatch::Ready
        ));
    }

    /// A local cleanup route never says the transcript leaves the machine.
    #[test]
    fn a_local_route_keeps_the_transcript_here() {
        let settings = CleanupSettings {
            enabled: true,
            provider: Some("local".to_owned()),
            model: Some("qwen3.5-9b-q4_k_m".to_owned()),
            ..CleanupSettings::default()
        };
        let store = kw_security::MemoryCredentialStore::default();

        match describe(reg(), &settings, &store, Some(8221)) {
            CleanupRoute::Ready {
                destination,
                leaves_device,
                caveat,
                ..
            } => {
                assert_eq!(destination, "local model");
                assert!(!leaves_device);
                assert!(caveat.is_none(), "{caveat:?}");
            }
            other => panic!("expected a resolved route, got {other:?}"),
        }

        // And with nothing running, the caveat says so rather than the screen
        // reading as configured and working.
        match describe(reg(), &settings, &store, None) {
            CleanupRoute::Ready { caveat, .. } => {
                let caveat = caveat.expect("a caveat when nothing is running");
                assert!(caveat.contains("no local model is running"), "{caveat}");
            }
            other => panic!("expected a resolved route, got {other:?}"),
        }
    }

    /// Every cloud reasoning provider in the registry can be routed.
    ///
    /// If one cannot, the screen would offer a provider that fails at send time,
    /// which is the mismatch `cloud_chat_providers` exists to prevent.
    #[test]
    fn every_cloud_provider_the_registry_lists_is_routable() {
        for p in &reg().reasoning {
            assert!(
                matches!(
                    dispatch_for(reg(), &p.id, false, None),
                    ReasoningDispatch::Ready
                ),
                "{} is in the registry and cannot be routed",
                p.id
            );
        }
    }

    /// A configured route says whether the transcript leaves the machine.
    #[test]
    fn a_cloud_route_is_reported_as_leaving_the_machine() {
        let settings = CleanupSettings {
            enabled: true,
            provider: Some("openai".to_owned()),
            model: Some("gpt-4o-mini".to_owned()),
            ..CleanupSettings::default()
        };
        let store = kw_security::MemoryCredentialStore::default();

        match describe(reg(), &settings, &store, None) {
            CleanupRoute::Ready {
                destination,
                leaves_device,
                caveat,
                ..
            } => {
                assert_eq!(destination, "openai");
                assert!(leaves_device);
                // No credential stored, so the caveat has to say so rather than
                // letting the screen read as configured and working.
                let caveat = caveat.expect("an unconfigured provider needs a caveat");
                assert!(caveat.contains("openai"), "{caveat}");
            }
            other => panic!("expected a resolved route, got {other:?}"),
        }
    }

    /// A loopback self-hosted endpoint does not leave the machine.
    #[test]
    fn a_loopback_endpoint_does_not_leave_the_machine() {
        let settings = CleanupSettings {
            enabled: true,
            model: Some("whatever".to_owned()),
            self_hosted_url: Some("http://127.0.0.1:11434/v1".to_owned()),
            ..CleanupSettings::default()
        };
        let store = kw_security::MemoryCredentialStore::default();

        match describe(reg(), &settings, &store, None) {
            CleanupRoute::Ready { leaves_device, .. } => assert!(!leaves_device),
            other => panic!("expected a resolved route, got {other:?}"),
        }
    }

    /// The states cross the boundary as discriminants a UI can switch on.
    #[test]
    fn the_states_serialize_with_a_discriminant() {
        let json = serde_json::to_value(CleanupRoute::Off).unwrap();
        assert_eq!(json["state"], "off");

        let json = serde_json::to_value(ReasoningDispatch::NeedsSidecar {
            reason: "no server".to_owned(),
        })
        .unwrap();
        assert_eq!(json["state"], "needs-sidecar");
        assert_eq!(json["reason"], "no server");

        let json = serde_json::to_value(CleanupRoute::Ready {
            destination: "openai".to_owned(),
            model: "m".to_owned(),
            leaves_device: true,
            caveat: None,
        })
        .unwrap();
        // camelCase inside a tagged variant, like everywhere else.
        assert_eq!(json["leavesDevice"], true);
        assert!(json.get("leaves_device").is_none());
    }
}
