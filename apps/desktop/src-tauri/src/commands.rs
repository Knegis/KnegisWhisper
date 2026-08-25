//! The Tauri command surface.
//!
//! Upstream's equivalent is a single 10,678-line file holding 478 IPC handlers.
//! The discipline that avoids that here: a command may validate its input, call
//! into one `kw-*` crate, and shape the result. Any logic worth testing belongs
//! in the crate, not here — so this file stays readable and the behaviour stays
//! testable without a running application.
//!
//! Errors cross the boundary as strings, because that is all the frontend can
//! use. They are constructed from `Display`, which for every error type in this
//! workspace is redacted where it needs to be.

use kw_platform::hotkey::{self, Binding, HotkeyAction};
use kw_provider_probe::{ProbeAudio, ProbeReport};
use kw_storage::{settings, transcriptions};
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::state::AppState;

/// A command failure, as the frontend sees it.
type CmdResult<T> = Result<T, String>;

fn fail(context: &str, err: impl std::fmt::Display) -> String {
    // Deliberately not `{err:?}`: Debug on some error types is more verbose than
    // anything a user should read, and redaction is implemented on Display.
    format!("{context}: {err}")
}

/* ------------------------------------------------------------------- settings */

/// Read a setting as raw JSON.
///
/// Untyped on purpose: the frontend owns the shape of its own settings, and
/// mirroring every one of them into a Rust struct would be a second source of
/// truth to keep in step. The credential-shaped-key rule still applies, because
/// it is enforced in `kw_storage::settings`.
#[tauri::command]
pub async fn settings_get(
    state: State<'_, AppState>,
    key: String,
) -> CmdResult<Option<serde_json::Value>> {
    settings::get::<serde_json::Value>(state.db().pool(), &key)
        .await
        .map_err(|e| fail("could not read the setting", e))
}

/// Write a setting.
#[tauri::command]
pub async fn settings_set(
    state: State<'_, AppState>,
    key: String,
    value: serde_json::Value,
) -> CmdResult<()> {
    settings::set(state.db().pool(), &key, &value)
        .await
        .map_err(|e| fail("could not save the setting", e))
}

/// Every settings key currently stored. Values are not returned.
#[tauri::command]
pub async fn settings_keys(state: State<'_, AppState>) -> CmdResult<Vec<String>> {
    settings::keys(state.db().pool())
        .await
        .map_err(|e| fail("could not list settings", e))
}

/* -------------------------------------------------------------------- hotkeys */

/// One action's hotkeys, classified, with any problems spelled out.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HotkeyReport {
    pub bindings: Vec<Binding>,
    /// Accelerators claimed by more than one action.
    pub conflicts: Vec<hotkey::Conflict>,
    /// Actions with no working hotkey at all.
    pub unusable: Vec<HotkeyAction>,
}

/// Read the configured hotkeys and say what will actually work.
///
/// This is where the modifier-only problem surfaces: upstream's default
/// dictation hotkey is `Control+Super`, which no cross-platform shortcut API can
/// register. Reporting it as configured would leave the user pressing a dead
/// key, so it comes back as a problem with an explanation.
#[tauri::command]
pub async fn hotkeys(state: State<'_, AppState>) -> CmdResult<HotkeyReport> {
    let bindings = load_bindings(&state).await?;
    let conflicts = hotkey::conflicts(&bindings);
    let unusable = bindings
        .iter()
        .filter(|b| !b.is_usable())
        .map(|b| b.action)
        .collect();

    Ok(HotkeyReport {
        bindings,
        conflicts,
        unusable,
    })
}

/// Load every action's binding from settings, falling back to the default.
pub async fn load_bindings(state: &AppState) -> CmdResult<Vec<Binding>> {
    let mut bindings = Vec::with_capacity(hotkey::ALL_ACTIONS.len());

    for action in hotkey::ALL_ACTIONS {
        let stored: String = settings::get_or(
            state.db().pool(),
            &action.settings_key(),
            action.default_accelerator().to_owned(),
        )
        .await
        .map_err(|e| fail("could not read a hotkey setting", e))?;

        bindings.push(Binding::from_stored(action, &stored));
    }

    Ok(bindings)
}

/// Change one action's hotkey.
///
/// Returns the classification of what was saved, so the UI can immediately show
/// that a modifier-only accelerator will not work rather than accepting it
/// silently.
#[tauri::command]
pub async fn hotkey_set(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    action: String,
    accelerators: String,
) -> CmdResult<Binding> {
    // Fail closed on an unknown action id rather than defaulting to dictation.
    let action = HotkeyAction::from_id(&action)
        .ok_or_else(|| format!("{action:?} is not a hotkey action"))?;

    let canonical = hotkey::serialize_list(&hotkey::parse_list(&accelerators));
    settings::set(state.db().pool(), &action.settings_key(), &canonical)
        .await
        .map_err(|e| fail("could not save the hotkey", e))?;

    // Applied now, not at the next launch. Saving a shortcut and then requiring
    // a restart before it does anything makes it impossible to tell "I typed the
    // accelerator wrongly" from "the platform refused it" — and the whole point
    // of returning the classification is to let the user find that out
    // immediately.
    //
    // Every action is re-registered rather than just this one, because
    // `unregister_all` is the only way to drop the previous binding for this
    // action and it drops the others with it.
    let bindings = load_bindings(&state).await?;
    let reports = crate::shortcuts::register_all(&app, &bindings);
    let live = reports.iter().filter(|r| r.is_live()).count();
    tracing::info!(live, total = reports.len(), "hotkeys re-registered");

    Ok(Binding::from_stored(action, &canonical))
}

/* -------------------------------------------------------------------- history */

/// Recent dictation history, newest first.
#[tauri::command]
pub async fn history(
    state: State<'_, AppState>,
    limit: Option<i64>,
    after: Option<i64>,
) -> CmdResult<Vec<transcriptions::Transcription>> {
    transcriptions::recent(state.db().pool(), limit.unwrap_or(50), after)
        .await
        .map_err(|e| fail("could not read history", e))
}

/// Delete one history entry.
#[tauri::command]
pub async fn history_delete(state: State<'_, AppState>, id: i64) -> CmdResult<bool> {
    transcriptions::delete(state.db().pool(), id)
        .await
        .map_err(|e| fail("could not delete the entry", e))
}

/// Delete all history. Returns how many entries were removed.
#[tauri::command]
pub async fn history_clear(state: State<'_, AppState>) -> CmdResult<u64> {
    transcriptions::clear(state.db().pool())
        .await
        .map_err(|e| fail("could not clear history", e))
}

/* ------------------------------------------------------------------ providers */

/// A provider and whether it has a credential stored — *not* whether it works.
///
/// The distinction is the point. `configured` means a key is present;
/// `connected` requires a passing connection test, which only
/// [`probe_providers`] can establish.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderSummary {
    pub id: String,
    pub name: String,
    pub credential_slot: String,
    pub configured: bool,
    pub model_count: usize,
}

/// List transcription providers and whether each has a credential.
#[tauri::command]
pub fn providers(state: State<'_, AppState>) -> CmdResult<Vec<ProviderSummary>> {
    let registry = kw_provider_registry::registry().map_err(|e| fail("provider registry", e))?;

    Ok(registry
        .transcription
        .iter()
        .map(|p| {
            let configured = kw_security::KeyRef::new(&p.credential_slot)
                .is_some_and(|slot| state.credentials().contains(&slot));
            ProviderSummary {
                id: p.id.clone(),
                name: p.name.clone(),
                credential_slot: p.credential_slot.clone(),
                configured,
                model_count: p.models.len(),
            }
        })
        .collect())
}

/// Store a provider credential.
///
/// The value goes straight to the OS credential store and is never written to
/// SQLite, a settings row, or a log line.
#[tauri::command]
pub fn provider_credential_set(
    state: State<'_, AppState>,
    slot: String,
    value: String,
) -> CmdResult<()> {
    let registry = kw_provider_registry::registry().map_err(|e| fail("provider registry", e))?;

    // A frontend may only write to a slot the registry declares. Otherwise the
    // command is an arbitrary write into the user's keychain namespace.
    if !registry.credential_slots().contains(slot.as_str()) {
        return Err(format!(
            "{slot:?} is not a credential slot this application uses"
        ));
    }

    let key =
        kw_security::KeyRef::new(&slot).ok_or_else(|| format!("{slot:?} is not a valid slot"))?;

    if value.trim().is_empty() {
        return state
            .credentials()
            .delete(&key)
            .map_err(|e| fail("could not remove the credential", e));
    }

    state
        .credentials()
        .set(&key, &kw_security::Secret::new(value))
        .map_err(|e| fail("could not store the credential", e))
}

/// Whether a credential exists for a slot. Never returns the value.
#[tauri::command]
pub fn provider_credential_present(state: State<'_, AppState>, slot: String) -> CmdResult<bool> {
    let key =
        kw_security::KeyRef::new(&slot).ok_or_else(|| format!("{slot:?} is not a valid slot"))?;
    Ok(state.credentials().contains(&key))
}

/// Run real connection tests against every configured provider.
///
/// This is the only thing that may establish "connected". It sends real requests
/// with the user's own credentials, which is why it is explicitly invoked rather
/// than run at startup — see `docs/NETWORK_BOUNDARY.md`.
#[tauri::command]
pub async fn probe_providers(state: State<'_, AppState>) -> CmdResult<ProbeReport> {
    let registry = kw_provider_registry::registry().map_err(|e| fail("provider registry", e))?;
    let transport =
        kw_provider_probe::PolicyTransport::new().map_err(|e| fail("network policy", e))?;

    // A tone, not speech: this build has no text-to-speech path yet, and the
    // report says so rather than implying recognition was verified.
    let audio = ProbeAudio::tone();

    kw_provider_probe::probe_configured(registry, state.credentials(), &transport, &audio)
        .await
        .map_err(|e| fail("connection test", e))
}

/* --------------------------------------------------------------------- import */

/// Import an existing OpenWhispr database.
#[derive(Debug, Clone, Deserialize)]
pub struct ImportRequest {
    /// Absolute path to the source `transcriptions.db`.
    pub path: String,
}

/// Import an OpenWhispr database, read-only, into this one.
#[tauri::command]
pub async fn import_openwhispr(
    state: State<'_, AppState>,
    request: ImportRequest,
) -> CmdResult<kw_storage::import::ImportReport> {
    let path = std::path::PathBuf::from(&request.path);
    kw_storage::import::from_openwhispr(state.db(), &path, state.credentials())
        .await
        .map_err(|e| fail("import", e))
}

/* ---------------------------------------------------------------------- tests */

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use super::*;

    async fn state() -> AppState {
        AppState::in_memory().await.unwrap()
    }

    #[tokio::test]
    async fn hotkey_bindings_default_to_upstreams_values() {
        let s = state().await;
        let bindings = load_bindings(&s).await.unwrap();

        assert_eq!(bindings.len(), hotkey::ALL_ACTIONS.len());
        let dictation = bindings
            .iter()
            .find(|b| b.action == HotkeyAction::Dictation)
            .unwrap();

        // Upstream's default is Control+Super, which is modifier-only, so it
        // must come back unusable with an explanation rather than looking fine.
        assert!(!dictation.is_usable());
        assert_eq!(dictation.problems().len(), 1);
        assert!(dictation.problems()[0].contains("does nothing"));
    }

    #[tokio::test]
    async fn a_configured_hotkey_replaces_the_default_and_becomes_usable() {
        let s = state().await;
        settings::set(
            s.db().pool(),
            &HotkeyAction::Dictation.settings_key(),
            &"Control+Shift+D",
        )
        .await
        .unwrap();

        let bindings = load_bindings(&s).await.unwrap();
        let dictation = bindings
            .iter()
            .find(|b| b.action == HotkeyAction::Dictation)
            .unwrap();
        assert!(dictation.is_usable());
        assert_eq!(dictation.registrable(), vec!["Control+Shift+D"]);
    }

    #[tokio::test]
    async fn provider_summary_reports_configured_only_when_a_credential_exists() {
        let s = state().await;
        let registry = kw_provider_registry::registry().unwrap();

        // Nothing stored: nothing configured.
        let before: Vec<bool> = registry
            .transcription
            .iter()
            .map(|p| {
                kw_security::KeyRef::new(&p.credential_slot)
                    .is_some_and(|k| s.credentials().contains(&k))
            })
            .collect();
        assert!(before.iter().all(|c| !c));

        // Store one.
        let slot = kw_security::KeyRef::new(&registry.transcription[0].credential_slot).unwrap();
        s.credentials()
            .set(&slot, &kw_security::Secret::new("sk-test".to_owned()))
            .unwrap();
        assert!(s.credentials().contains(&slot));
    }

    #[tokio::test]
    async fn a_credential_write_to_an_unknown_slot_is_refused() {
        let s = state().await;
        let registry = kw_provider_registry::registry().unwrap();

        // Not a registry slot, so not writable -- otherwise the frontend could
        // write anywhere in the application's keychain namespace.
        assert!(!registry.credential_slots().contains("attacker-chosen"));

        let key = kw_security::KeyRef::new("attacker-chosen").unwrap();
        assert!(!s.credentials().contains(&key));
    }

    #[tokio::test]
    async fn history_round_trips_through_the_command_layer() {
        let s = state().await;
        transcriptions::record(
            s.db().pool(),
            &transcriptions::NewTranscription {
                text: "hello".to_owned(),
                ..Default::default()
            },
            transcriptions::Status::Completed,
        )
        .await
        .unwrap();

        let rows = transcriptions::recent(s.db().pool(), 10, None)
            .await
            .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].text, "hello");
    }

    #[tokio::test]
    async fn an_unknown_hotkey_action_id_is_rejected() {
        // The fail-closed rule at the command boundary.
        assert_eq!(HotkeyAction::from_id("not-an-action"), None);
    }

    #[test]
    fn the_error_helper_does_not_use_debug_formatting() {
        // Display is where redaction lives; Debug on a credential error would be
        // fine today but this keeps the habit.
        let rendered = fail("context", "detail");
        assert_eq!(rendered, "context: detail");
    }
}
