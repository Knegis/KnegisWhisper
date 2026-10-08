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

use std::collections::VecDeque;
use std::path::{Path, PathBuf};

use kw_notes_core::{
    notes, recordings as note_recordings, speakers, transcript, NoteRecording, TranscriptRow,
};
use kw_platform::hotkey::{self, Binding, HotkeyAction, HotkeyMode};
use kw_provider_probe::{ProbeAudio, ProbeReport};
use kw_storage::{settings, transcriptions};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, State};
use ts_rs::TS;

use crate::state::AppState;

/// A command failure, as the frontend sees it.
type CmdResult<T> = Result<T, String>;

fn fail(context: &str, err: impl std::fmt::Display) -> String {
    // Deliberately not `{err:?}`: Debug on some error types is more verbose than
    // anything a user should read, and redaction is implemented on Display.
    format!("{context}: {err}")
}

#[cfg(all(test, feature = "tauri-test"))]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod speaker_ipc_tests {
    use super::*;
    use serde_json::{json, Value};
    use tauri::WebviewWindow;

    fn invoke(
        window: &WebviewWindow<tauri::test::MockRuntime>,
        command: &str,
        body: Value,
    ) -> Result<Value, Value> {
        tauri::test::get_ipc_response(
            window,
            tauri::webview::InvokeRequest {
                cmd: command.into(),
                callback: tauri::ipc::CallbackFn(0),
                error: tauri::ipc::CallbackFn(1),
                url: window.url().unwrap(),
                body: tauri::ipc::InvokeBody::Json(body),
                headers: Default::default(),
                invoke_key: tauri::test::INVOKE_KEY.into(),
            },
        )
        .map(|response| response.deserialize().unwrap())
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn renaming_a_speaker_updates_rows_exports_and_suggestions_over_ipc() {
        let state = AppState::in_memory().await.unwrap();
        let db = state.db().clone();
        let transcript = json!([
            {"text":"Hello", "timestamp":0.0, "speaker":"speaker_0", "speakerName":"Speaker 1", "speakerIsPlaceholder":true},
            {"text":"Hi", "timestamp":3.0, "speaker":"speaker_1", "speakerName":"Speaker 2", "speakerIsPlaceholder":true}
        ]).to_string();
        let id = notes::create(
            db.pool(),
            &notes::NewNote {
                title: "Meeting".into(),
                transcript: Some(transcript),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let second = notes::create(
            db.pool(),
            &notes::NewNote {
                title: "Second".into(),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let app = tauri::test::mock_builder()
            .manage(state)
            .invoke_handler(tauri::generate_handler![
                note_transcript_rows,
                note_speaker_rename,
                note_speaker_suggestions,
                note_export
            ])
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .unwrap();
        let window = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
            .build()
            .unwrap();

        let rows = invoke(&window, "note_transcript_rows", json!({"id":id})).unwrap();
        assert_eq!(rows[0]["speaker"], "Speaker 1");
        assert_eq!(rows[0]["speakerId"], "speaker_0");
        invoke(
            &window,
            "note_speaker_rename",
            json!({"id":id,"speakerId":"speaker_0","name":"Axel"}),
        )
        .unwrap();
        let rows = invoke(&window, "note_transcript_rows", json!({"id":id})).unwrap();
        assert_eq!(rows[0]["speaker"], "Axel");
        let exported = invoke(&window, "note_export", json!({"id":id,"format":"txt"})).unwrap();
        assert!(exported["content"].as_str().unwrap().contains("Axel"));
        assert!(!exported["content"].as_str().unwrap().contains("Speaker 1"));
        let suggestions =
            invoke(&window, "note_speaker_suggestions", json!({"id":second})).unwrap();
        assert_eq!(suggestions[0], "Axel");
        invoke(
            &window,
            "note_speaker_rename",
            json!({"id":id,"speakerId":"speaker_0","name":" "}),
        )
        .unwrap();
        let rows = invoke(&window, "note_transcript_rows", json!({"id":id})).unwrap();
        assert_eq!(rows[0]["speaker"], "Speaker 1");
        let missing = invoke(
            &window,
            "note_speaker_rename",
            json!({"id":id+100,"speakerId":"speaker_0","name":"Axel"}),
        )
        .unwrap_err();
        assert!(missing.to_string().contains("no note with that id"));
    }
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

/// An action nothing in the application handles, and the sentence saying so.
///
/// Reported per action rather than per accelerator because that is the shape of
/// the fact. Nothing about the accelerator is wrong; there is no handler behind
/// it, and typing a different chord would not change that.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct UnhandledAction {
    pub action: HotkeyAction,
    pub reason: String,
}

/// One action's hotkeys, classified, with any problems spelled out.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct HotkeyReport {
    pub registrations: Vec<crate::shortcuts::RegistrationReport>,
    pub portal_supported: bool,
    pub portal_enabled: bool,
    pub bindings: Vec<Binding>,
    /// Accelerators claimed by more than one action.
    pub conflicts: Vec<hotkey::Conflict>,
    /// Actions that are dead because nothing handles them, with the reason.
    /// Registration outcome is reported separately by
    /// [`crate::shortcuts::RegistrationReport::is_live`].
    pub unhandled: Vec<UnhandledAction>,
    /// Hotkeys that work and also fire when the user meant something else.
    ///
    /// A caution means the hotkey works and also fires at times nobody asked
    /// for. Today the sources are AltGr and modifier-only prefix chords.
    pub cautions: Vec<hotkey::Caution>,
}

/// Save the opt-in Linux backend choice. Applied only at the next startup.
#[tauri::command]
pub async fn hotkey_set_portal(state: State<'_, AppState>, enabled: bool) -> CmdResult<()> {
    if !cfg!(target_os = "linux") {
        return Err("Desktop portal shortcuts are only available on Linux.".into());
    }
    settings::set(state.db().pool(), "hotkey.usePortal", &enabled)
        .await
        .map_err(|e| fail("could not save the shortcut backend", e))
}

/// Read the configured hotkeys and say what will actually work.
///
/// This is where the modifier-only problem surfaces: upstream's default
/// dictation hotkey is `Control+Super`, which no cross-platform shortcut API can
/// register. Reporting it as configured would leave the user pressing a dead
/// key, so it comes back as a problem with an explanation.
#[tauri::command]
pub async fn hotkeys(
    state: State<'_, AppState>,
    hooks: State<'_, std::sync::Arc<crate::shortcuts::Hooks>>,
) -> CmdResult<HotkeyReport> {
    let mut report = report_for(load_bindings(&state).await?);
    report.registrations = hooks.reports();
    report.portal_enabled = hooks.portal_enabled;
    Ok(report)
}

/// Build the report from bindings that have already been loaded.
///
/// Split from the command so a test can reach it. The command takes Tauri state;
/// the interesting part — which actions are dead, and whether the reason is the
/// user's to fix — has nothing to do with Tauri, and leaving it inside the
/// command is what made it untested.
fn report_for(bindings: Vec<Binding>) -> HotkeyReport {
    report_with_layout(bindings, &kw_platform::keyboard_layout::altgr())
}

/// [`report_for`], with the keyboard layout supplied.
///
/// Injected for the reason the parity gate injects `readComponent`: the
/// interesting half is which accelerators an AltGr layout makes dangerous, and
/// a test that could only assert it on a machine whose layout happens to have
/// AltGr would pass here and prove nothing in CI, which is Linux.
fn report_with_layout(
    bindings: Vec<Binding>,
    altgr: &kw_platform::keyboard_layout::AltGr,
) -> HotkeyReport {
    let conflicts = hotkey::conflicts(&bindings);
    // Two kinds, kept separate because they are different facts about the same
    // chord and either can hold without the other: AltGr depends on the layout,
    // and being a prefix depends only on the chord having two keys.
    let mut cautions = hotkey::cautions(&bindings, altgr);
    cautions.extend(hotkey::prefix_cautions(&bindings));

    // Asked of the registration module rather than recomputed, because that is
    // the module that decides. `Binding::has_registrable` can only speak about
    // the accelerator: it says chat agent's `Control+Shift+Space` is
    // registrable, and it is right about the accelerator and says nothing about
    // the hotkey.
    let unhandled: Vec<UnhandledAction> = hotkey::ALL_ACTIONS
        .into_iter()
        .filter_map(|action| {
            crate::shortcuts::unhandled_reason(action).map(|reason| UnhandledAction {
                action,
                reason: reason.to_owned(),
            })
        })
        .collect();

    HotkeyReport {
        registrations: Vec::new(),
        portal_supported: cfg!(target_os = "linux"),
        portal_enabled: false,
        bindings,
        conflicts,
        unhandled,
        cautions,
    }
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

        // The shipped default, spelled by the type rather than written out
        // here, so the one place that decides what ships is `HotkeyMode`'s
        // `Default` and the reasoning next to it.
        let mode: String = settings::get_or(
            state.db().pool(),
            &action.mode_settings_key(),
            HotkeyMode::default().id().to_owned(),
        )
        .await
        .map_err(|e| fail("could not read a hotkey mode setting", e))?;

        bindings.push(Binding::from_stored(
            action,
            &stored,
            HotkeyMode::from_stored(&mode),
        ));
    }

    Ok(bindings)
}

/// What changing one action's hotkey actually achieved.
///
/// `binding` is what was stored and how it classifies; `registrations` is what
/// the platform said when asked to take it. Both, because they fail
/// independently: a well-formed accelerator can still be refused because another
/// application owns it, and reporting "saved" for that is the kind of confident
/// success this project treats as a defect.
#[derive(Debug, Clone, serde::Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct HotkeyChange {
    pub binding: Binding,
    pub registrations: Vec<crate::shortcuts::Registration>,
    /// True when at least one accelerator for this action is now live.
    pub live: bool,
    /// What this hotkey will *also* do. Returned here as well as from
    /// [`hotkeys`] because the moment to learn that `Control+Alt` is AltGr is
    /// when you press Set, not the next time the screen is opened.
    pub cautions: Vec<hotkey::Caution>,
}

/// Change one action's hotkey.
///
/// Returns the classification of what was saved *and* whether the platform took
/// it, so the UI can immediately distinguish a modifier-only accelerator from
/// one another application already owns.
#[tauri::command]
pub async fn hotkey_set(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    action: String,
    accelerators: String,
    hooks: State<'_, std::sync::Arc<crate::shortcuts::Hooks>>,
) -> CmdResult<HotkeyChange> {
    // Fail closed on an unknown action id rather than defaulting to dictation.
    if crate::shortcuts::capture_active(&app) {
        return Err("Finish recording the shortcut before saving it.".into());
    }
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
    reapply(&app, &state, &hooks, action).await
}

/// Change whether one action is toggled or held.
///
/// A separate command from [`hotkey_set`] because it writes a separate key, and
/// the two change for different reasons — rebinding a key is not choosing
/// push-to-talk. It re-registers everything for the same reason `hotkey_set`
/// does: the mode is read at registration time by both the hook path and the
/// plugin path, so a mode saved and not applied until the next launch would be a
/// setting that lies about having taken effect.
#[tauri::command]
pub async fn hotkey_set_mode(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    action: String,
    mode: String,
    hooks: State<'_, std::sync::Arc<crate::shortcuts::Hooks>>,
) -> CmdResult<HotkeyChange> {
    if crate::shortcuts::capture_active(&app) {
        return Err("Finish recording the shortcut before changing its mode.".into());
    }
    let action = HotkeyAction::from_id(&action)
        .ok_or_else(|| format!("{action:?} is not a hotkey action"))?;

    // Refused rather than defaulted. `HotkeyMode::from_stored` falls back for a
    // value already on disk, which is recovery; a value arriving from the
    // renderer is a request, and answering an unrecognised request with "toggle,
    // then" would report success for something that did not happen.
    let mode =
        HotkeyMode::from_id(mode.trim()).ok_or_else(|| format!("{mode:?} is not a hotkey mode"))?;

    settings::set(state.db().pool(), &action.mode_settings_key(), &mode.id())
        .await
        .map_err(|e| fail("could not save the hotkey mode", e))?;

    reapply(&app, &state, &hooks, action).await
}

/// Re-register every hotkey and report what became of `action`'s.
///
/// Shared by the two writers above so they cannot come to disagree about what a
/// change achieved. Every action is re-registered rather than just this one,
/// because `unregister_all` is the only way to drop the previous binding for one
/// action and it drops the others with it.
async fn reapply(
    app: &tauri::AppHandle,
    state: &AppState,
    hooks: &std::sync::Arc<crate::shortcuts::Hooks>,
    action: HotkeyAction,
) -> CmdResult<HotkeyChange> {
    let bindings = load_bindings(state).await?;
    let reports = crate::shortcuts::register_all(app, &bindings, hooks);
    let live = reports.iter().filter(|r| r.is_live()).count();
    tracing::info!(live, total = reports.len(), "hotkeys re-registered");

    // This action's own outcome, pulled out of the whole-set re-registration.
    // Absent only if `ALL_ACTIONS` and the stored bindings ever disagree, which
    // would be a bug rather than a state to render, so it reports as not live
    // with no results rather than inventing a success.
    let mine = reports.into_iter().find(|r| r.action == action);

    // Read back out of what was just loaded rather than rebuilt from the
    // argument. Rebuilding was fine while the only thing a writer changed was
    // the accelerator; with two keys behind one binding, rebuilding from one of
    // them means the returned binding disagrees with the one that was
    // registered about the other.
    let binding = bindings
        .into_iter()
        .find(|b| b.action == action)
        .unwrap_or_else(|| {
            Binding::from_stored(action, action.default_accelerator(), HotkeyMode::default())
        });

    let mut cautions = hotkey::cautions(
        std::slice::from_ref(&binding),
        &kw_platform::keyboard_layout::altgr(),
    );
    cautions.extend(hotkey::prefix_cautions(std::slice::from_ref(&binding)));

    Ok(HotkeyChange {
        binding,
        registrations: mine.as_ref().map(|r| r.results.clone()).unwrap_or_default(),
        live: mine.is_some_and(|r| r.is_live()),
        cautions,
    })
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

/// Whether dictation history is being written at all.
///
/// Read from settings rather than from the running session, for the same reason
/// as `history_keep_raw_text` below: `DbHistory::enabled` reads it per
/// dictation, so there is no cached copy that could disagree with the stored
/// one. The default here is `true` because the default there is `true` -- two
/// defaults for one key is how a screen comes to report the opposite of what the
/// sink is doing.
#[tauri::command]
pub async fn history_enabled(state: State<'_, AppState>) -> CmdResult<bool> {
    Ok(kw_storage::settings::get_or(
        state.db().pool(),
        crate::dictation::HISTORY_ENABLED_KEY,
        true,
    )
    .await
    .unwrap_or(true))
}

/// Start, or stop, writing dictation history.
///
/// Stopping is not deleting. Rows already written stay exactly where they are;
/// `history_clear` above is the only thing that removes them. The two controls
/// sit next to each other on the Dictation screen and sound alike, so the screen
/// spells the difference out rather than leaving it to be found out.
///
/// Takes effect on the next dictation, not after a relaunch: the sink reads the
/// setting each time it is about to write a row.
#[tauri::command]
pub async fn history_set_enabled(state: State<'_, AppState>, enabled: bool) -> CmdResult<()> {
    kw_storage::settings::set(
        state.db().pool(),
        crate::dictation::HISTORY_ENABLED_KEY,
        &enabled,
    )
    .await
    .map_err(|e| fail("could not save the setting", e))
}

/// Whether the pre-cleanup transcript is kept alongside the cleaned one.
///
/// Read from settings rather than from the running session, because
/// `DbHistory::keep_raw_text` reads it per dictation and there is no cached copy
/// that could disagree with the stored one.
#[tauri::command]
pub async fn history_keep_raw_text(state: State<'_, AppState>) -> CmdResult<bool> {
    Ok(kw_storage::settings::get_or(
        state.db().pool(),
        crate::dictation::KEEP_RAW_TEXT_KEY,
        false,
    )
    .await
    .unwrap_or(false))
}

/// Keep, or stop keeping, the pre-cleanup transcript.
///
/// Takes effect on the next dictation, not after a relaunch: the sink reads the
/// setting each time it writes a row.
#[tauri::command]
pub async fn history_set_keep_raw_text(state: State<'_, AppState>, enabled: bool) -> CmdResult<()> {
    kw_storage::settings::set(
        state.db().pool(),
        crate::dictation::KEEP_RAW_TEXT_KEY,
        &enabled,
    )
    .await
    .map_err(|e| fail("could not save the setting", e))
}

/* ------------------------------------------------------------------ providers */

/// What would happen if dictation were pointed at this provider right now.
///
/// Three states rather than a boolean, because "we have not written the client"
/// and "you have not filled in the settings this provider needs" are different
/// problems with different fixes, and neither is "it works".
///
/// Derived by asking `route::resolve` rather than by listing provider ids here.
/// A hand-written list of which providers dispatch would be a second copy of the
/// match in `crates/transcription-core/src/route.rs`, and the two would part
/// company the first time a client was written.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(
    tag = "state",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum Dispatch {
    /// A transport exists. Dictation would make a real request.
    Ready,
    /// A provider this build will not send to, with the reason.
    ///
    /// Not always "unwritten": Tinfoil is refused *deliberately*, because an
    /// unattested request would present as Tinfoil while providing none of the
    /// confidentiality the name means. The reason comes from the client rather
    /// than being composed here, so it says which case this is.
    NeedsClient { reason: String },
    /// The route cannot be resolved at all with what is configured.
    Unresolvable { reason: String },
}

/// A provider and whether it has a credential stored — *not* whether it works.
///
/// The distinction is the point. `configured` means a key is present;
/// `connected` requires a passing connection test, which only
/// [`probe_providers`] can establish; and `dispatch` is a third thing again —
/// whether this build can even send the request.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ProviderSummary {
    pub id: String,
    pub name: String,
    pub credential_slot: String,
    pub configured: bool,
    pub model_count: usize,
    /// The models on offer, so a picker never writes a model id by hand.
    pub models: Vec<String>,
    pub dispatch: Dispatch,
}

/// Ask the real router what it would do with this provider.
fn dispatch_for(registry: &kw_provider_registry::Registry, provider_id: &str) -> Dispatch {
    let settings = kw_transcription_core::RouteSettings {
        transcription_mode: Some("providers".to_owned()),
        cloud_transcription_provider: Some(provider_id.to_owned()),
        ..Default::default()
    };

    match kw_transcription_core::route::resolve(
        registry,
        &settings,
        &kw_transcription_core::RouteRequest::default(),
    ) {
        Ok(kw_transcription_core::Transport::ProviderClient {
            provider,
            model,
            language,
            corti,
            ..
        }) => {
            // Asked, not listed. Three of the four have clients now, and a
            // hand-written set of which would have been a second copy of the
            // match in `clients::shape` — parting company the first time one
            // was added, which is exactly what just happened.
            match kw_transcription_core::clients::shape(
                provider,
                model.as_deref(),
                language.as_deref(),
                corti.as_ref(),
            ) {
                Ok(_) => Dispatch::Ready,
                Err(err) => Dispatch::NeedsClient {
                    reason: err.to_string(),
                },
            }
        }
        Ok(_) => Dispatch::Ready,
        Err(err) => Dispatch::Unresolvable {
            reason: err.to_string(),
        },
    }
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
                models: p.models.iter().map(|m| m.id.clone()).collect(),
                dispatch: dispatch_for(registry, &p.id),
            }
        })
        .collect())
}

/// One credential slot and whether a secret is stored in it.
///
/// `stored` is deliberately the only thing said about the value. There is no
/// command that reads a credential back, so a screen cannot display one and a
/// compromised renderer cannot exfiltrate one.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct CredentialSlotStatus {
    pub slot: String,
    pub name: String,
    pub kind: kw_provider_registry::CredentialKind,
    /// Which parts of the application use this key, from the registry.
    pub used_for: Vec<String>,
    /// Which feature a custom endpoint belongs to; `None` for a provider, and
    /// for `custom`, your own server's key, which cleanup and summaries share.
    pub feature: Option<String>,
    /// A secret is present. This is NOT the same as the provider working.
    pub stored: bool,
}

/// Every credential slot, in registry order, with whether each is filled.
///
/// One call rather than one `provider_credential_present` per slot: there are
/// twenty-one, the screen re-reads after every save, and the list itself has to
/// come from the registry anyway so that no screen writes provider ids by hand.
#[tauri::command]
pub fn credential_slots(state: State<'_, AppState>) -> CmdResult<Vec<CredentialSlotStatus>> {
    let registry = kw_provider_registry::registry().map_err(|e| fail("provider registry", e))?;

    Ok(registry
        .credentials
        .iter()
        .map(|c| CredentialSlotStatus {
            slot: c.slot.clone(),
            name: c.name.clone(),
            kind: c.kind,
            used_for: c.used_for.clone(),
            feature: c.feature.clone(),
            stored: kw_security::KeyRef::new(&c.slot)
                .is_some_and(|key| state.credentials().contains(&key)),
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

/// How many dialog-returned paths [`DialogGrants`] remembers at once.
///
/// `Notes > Upload` holds one picked path at a time, so 32 is ample for any
/// amount of re-picking a person does. The bound is here so a renderer calling
/// `pick_import_file` in a loop cannot grow the set without limit.
pub(crate) const MAX_DIALOG_GRANTS: usize = 32;

/// Every command that takes a filesystem path from the renderer, with the
/// dotted path to that argument inside the command's JSON body.
///
/// **One list, so the boundary test cannot fall behind the application.** The
/// doc on [`DialogGrants`] below says these are "the only three commands that
/// take a path, so the surface is closed". Until this list existed that was
/// prose with nothing behind it: `media.rs`'s IPC test named the three in its
/// own `generate_handler!` and again in its own loop, so a fourth path-taking
/// command was covered by neither, and removing one from the application left
/// the test green (KNEG-151).
///
/// Three things read this, which is what turns that sentence into a check:
///
///   * `media.rs`'s `every_import_command_enforces_the_dialog_grant_at_the_ipc_boundary`
///     drives **every entry** over real IPC, so a new one is asserted by being added.
///   * [`tests::every_path_taking_command_is_registered`] ties the list to
///     `lib.rs`'s `generate_handler!`, so dropping one from the application fails.
///   * `tools/command-audit/scan.mjs` fails when a registered command takes a
///     renderer-supplied path and is **not** listed here, which is the direction
///     neither test can see.
///
/// The second element is dotted because the body shapes differ:
/// `import_openwhispr` takes an `ImportRequest` struct, the other two take the
/// string directly. It is the JSON shape, not the Rust one.
#[cfg(test)]
pub(crate) const PATH_TAKING_COMMANDS: [(&str, &str); 3] = [
    ("media_import_audio", "path"),
    ("media_import_video", "path"),
    ("import_openwhispr", "request.path"),
];

/// What an import command says when it was handed a path no dialog produced.
///
/// One sentence for two cases on purpose — see [`DialogGrants::authorise`].
const UNGRANTED_PATH: &str = "this file was not chosen through the import dialog, or is no \
                              longer where it was. Choose it again.";

/// The paths a real OS file dialog has returned this session.
///
/// **An import command reads a path only if this set holds it.** The renderer
/// has no independent way to *obtain* a path — `capabilities/default.json`
/// grants exactly `["core:default"]`, so there is no filesystem, dialog, shell
/// or HTTP capability to get one from — but nothing used to *check* that, and
/// a compromised renderer (which `docs/SECURITY.md`'s threat model includes)
/// could call `media_import_audio` with any local path at all. With a cloud
/// transcription provider configured, that file's contents then left the
/// machine. This set is what closes that: [`pick_import_file`] is the only
/// thing that puts a path in it, and [`media::media_import_audio`],
/// [`media::media_import_video`] and [`import_openwhispr`] are the only three
/// commands that take a path, so the surface is closed.
///
/// [`media::media_import_audio`]: crate::media::media_import_audio
/// [`media::media_import_video`]: crate::media::media_import_video
///
/// **Session-lifetime, and bounded by count rather than by time.** A grant
/// lasts until the application exits. Single-use would break
/// retry-after-failure, which exists today — `NotesUpload.tsx`'s `catch`
/// leaves the picked path in state, so the same file can be tried again after
/// a failed conversion. A time bound would refuse a file the user really did
/// pick if they left the review step open, and they realistically will: that
/// screen can sit on a picked path across an ffmpeg install of tens of
/// megabytes. This is the same argument `meeting::Watcher::offered` makes for
/// keying on the provider rather than on a timestamp — read that field's doc
/// comment; a time-tuned rule gets one of its two cases wrong whichever way it
/// is set, and identity gets both right.
///
/// **The key is a canonicalised [`PathBuf`], never the string that crossed the
/// boundary.** [`pick_import_file`] hands back `path.display().to_string()`,
/// which is lossy. The consequence, stated plainly: a path whose name does not
/// survive `display()` cannot be imported. That is not a regression — today
/// `media::check_importable_file`'s `is_file()` already fails on such a string
/// — but it becomes a deliberate refusal rather than an accident.
#[derive(Debug, Default)]
pub struct DialogGrants(std::sync::Mutex<VecDeque<PathBuf>>);

impl DialogGrants {
    /// The one canonical form both the grant and the check go through.
    ///
    /// `std::fs::canonicalize` on Windows returns a verbatim `\\?\C:\...`
    /// path. Both sides must come through here, or a canonical path would be
    /// compared against a raw one and never match.
    fn key(path: &Path) -> std::io::Result<PathBuf> {
        std::fs::canonicalize(path)
    }

    /// Remember `path` as dialog-returned, and answer with its canonical form.
    pub(crate) fn grant(&self, path: &Path) -> std::io::Result<PathBuf> {
        let key = Self::key(path)?;

        let mut held = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        // Re-picking a path moves it to the newest end rather than leaving it
        // where it was. The file just chosen is the one about to be imported,
        // so it must not be the one the bound evicts.
        held.retain(|existing| existing != &key);
        held.push_back(key.clone());
        while held.len() > MAX_DIALOG_GRANTS {
            held.pop_front();
        }

        Ok(key)
    }

    /// The canonical path to import, or a refusal.
    ///
    /// The whole decision, and pure apart from the filesystem lookup
    /// `canonicalize` needs: it takes no `AppHandle`, so it is unit-testable on
    /// its own. That is the same reason `media::check_importable_file` and
    /// [`import_pick_dialog`] are separate functions rather than inline
    /// branches. The exported command boundary is covered separately with
    /// Tauri's mock webview runtime.
    ///
    /// A path that cannot be canonicalised and a path nobody granted get the
    /// *same* refusal, deliberately. Telling them apart would answer "does this
    /// file exist" for any path a compromised renderer cared to name, and a
    /// path that will not canonicalise cannot be in the set in any case, since
    /// every member of it came out of `canonicalize`.
    pub fn authorise(&self, path: &str) -> CmdResult<PathBuf> {
        let Ok(key) = Self::key(Path::new(path)) else {
            return Err(UNGRANTED_PATH.to_owned());
        };

        if self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .contains(&key)
        {
            Ok(key)
        } else {
            Err(UNGRANTED_PATH.to_owned())
        }
    }
}

/// Import an existing OpenWhispr database.
#[derive(Debug, Clone, Deserialize)]
pub struct ImportRequest {
    /// Absolute path to the source `transcriptions.db`, as
    /// [`pick_import_file`] returned it.
    pub path: String,
}

/// Import an OpenWhispr database, read-only, into this one.
///
/// **The path must be one a real dialog returned** — see [`DialogGrants`].
/// `kw_storage::import::from_openwhispr` checks `exists()` rather than
/// `is_file()`, so the grant is the only thing standing between this command
/// and any readable path the caller names.
#[tauri::command]
pub async fn import_openwhispr(
    state: State<'_, AppState>,
    grants: State<'_, DialogGrants>,
    request: ImportRequest,
) -> CmdResult<kw_storage::import::ImportReport> {
    let path = grants.authorise(&request.path)?;
    kw_storage::import::from_openwhispr(state.db(), &path, state.credentials())
        .await
        .map_err(|e| fail("import", e))
}

/// Which file `pick_import_file` is being asked to find.
///
/// A closed enum rather than a filter typed by the frontend, the same
/// reasoning as `media::MediaSidecar`: the title and extension list are fixed
/// by this command, not invented by whoever calls it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum ImportPickKind {
    Audio,
    Video,
    OpenWhisprDatabase,
}

/// The dialog title, filter name and extensions for one [`ImportPickKind`].
///
/// Pure, so it is unit-testable without an `AppHandle` — see the module doc's
/// note on why a Tauri-dependent function cannot be named as a value in a
/// test.
fn import_pick_dialog(
    kind: ImportPickKind,
) -> (&'static str, &'static str, &'static [&'static str]) {
    match kind {
        ImportPickKind::Audio => (
            "Choose an audio file",
            "Audio",
            &[
                "mp3", "wav", "m4a", "aac", "flac", "ogg", "opus", "wma", "aiff",
            ],
        ),
        ImportPickKind::Video => (
            "Choose a video file",
            "Video",
            &["mp4", "mov", "mkv", "avi", "webm", "m4v"],
        ),
        ImportPickKind::OpenWhisprDatabase => (
            "Choose transcriptions.db",
            "SQLite database",
            &["db", "sqlite", "sqlite3"],
        ),
    }
}

/// Open a native "choose a file" dialog and return the path picked, or `None`
/// if the user cancelled.
///
/// **What this guarantees, and what it does not.** The renderer has no
/// independent way to *obtain* a path: there is no filesystem or dialog
/// capability in `capabilities/default.json`, so the only path a caller can
/// ever have seen is one a real OS dialog produced through this command.
///
/// **It is not the shape `note_export_to_file` uses, and it is confirmed
/// rather than assumed.** That command fuses picking and writing into one
/// atomic call, so the frontend never sees or supplies a path at all. Fusing
/// pick and import the same way would fight `Notes > Upload`'s
/// pick-then-review-then-confirm flow, which a single-click save does not
/// need, so this command still hands the chosen path *back* to the renderer,
/// which holds it in state and passes it as a plain string to a separate
/// command (`media_import_audio`, `media_import_video`, `import_openwhispr`)
/// on a later click. What makes that safe is [`DialogGrants`]: the canonical
/// form of every path this command returns is recorded, and those three
/// commands authorise against that set before reading anything. A path no
/// dialog produced is refused.
///
/// The pure policy tests are backed by a mock-webview IPC test which invokes
/// all three import commands by their exported names. A real picker check still
/// belongs in the bench sequence because only the OS dialog can prove the
/// person-facing flow; see `docs/BENCH_CHECK.md`'s Task 12 entry.
#[tauri::command]
pub async fn pick_import_file(
    app: AppHandle,
    grants: State<'_, DialogGrants>,
    kind: ImportPickKind,
) -> CmdResult<Option<String>> {
    use tauri_plugin_dialog::DialogExt as _;

    let (title, filter_name, extensions) = import_pick_dialog(kind);

    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog()
        .file()
        .set_title(title)
        .add_filter(filter_name, extensions)
        .pick_file(move |chosen| {
            // The receiver is dropped only if this command was cancelled, in
            // which case there is nobody left to tell.
            let _ = tx.send(chosen);
        });

    let Some(chosen) = rx
        .await
        .map_err(|e| fail("the open dialog closed unexpectedly", e))?
    else {
        return Ok(None);
    };

    let path = chosen
        .into_path()
        .map_err(|e| fail("could not resolve the chosen path", e))?;

    // Granted before the path is handed over, so the renderer never holds a
    // string no import command will accept.
    grants
        .grant(&path)
        .map_err(|e| fail("could not resolve the chosen path", e))?;

    // The *display* string, not the canonical one the grant is keyed on: the
    // screen shows this path to the person who picked it and derives the note
    // title from its file name, and Windows' verbatim `\\?\C:\...` form is not
    // something to put in front of anyone. `authorise` canonicalises again on
    // the way back in, so the two still meet.
    Ok(Some(path.display().to_string()))
}

/* ---------------------------------------------------------------------- notes */

// The local API serves this same data over HTTP (`crates/local-api/src/server.rs`),
// and the two surfaces read one database, so they must not disagree about what
// is allowed. Every judgement below is the one already made there, referenced
// rather than re-derived.

/// The default number of rows a list command returns, matching the local API's
/// `wire::DEFAULT_LIMIT`.
const NOTES_DEFAULT_LIMIT: i64 = 100;

/// The most rows any list command will return, matching `wire::MAX_LIMIT`.
/// A larger request is clamped rather than refused — the caller gets fewer rows
/// than it asked for, never an error it has to handle.
const NOTES_MAX_LIMIT: i64 = 500;

fn notes_limit(requested: Option<i64>) -> CmdResult<i64> {
    match requested {
        None => Ok(NOTES_DEFAULT_LIMIT),
        Some(0) => Err("a limit of 0 returns nothing; omit it for the default".to_owned()),
        Some(n) if n < 0 => Err("a limit must be positive".to_owned()),
        Some(n) => Ok(n.min(NOTES_MAX_LIMIT)),
    }
}

fn positive_id(value: Option<i64>, field: &str) -> CmdResult<Option<i64>> {
    match value {
        Some(id) if id <= 0 => Err(format!("{field:?} must be a positive id")),
        other => Ok(other),
    }
}

/// A new note, as the renderer sends it.
///
/// A separate type from `notes::NewNote`, which is not `Deserialize`. That is
/// the right boundary: this is where an untrusted payload is validated, and the
/// domain type stays free of serialization concerns.
///
/// `deny_unknown_fields` is deliberate. A mistyped field name would otherwise be
/// dropped in silence and the note saved without it — the same shape of defect
/// as a frontend type declaring a field the backend never sends.
#[derive(Debug, Clone, Default, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
// serde defaults an absent `Option<T>` to `None`, so these keys may be omitted
// as well as sent as null. The declaration says both rather than only one.
#[ts(optional_fields = nullable)]
pub struct NewNoteRequest {
    pub title: String,
    pub content: String,
    pub note_type: Option<String>,
    pub folder_id: Option<i64>,
    /// A JSON array of transcript segments, as stored.
    pub transcript: Option<String>,
    /// A JSON array of `{displayName, email}`, as stored.
    pub participants: Option<String>,
    pub audio_duration_seconds: Option<f64>,
}

/// The fields of a note to change. Absent means leave it alone.
#[derive(Debug, Clone, Default, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
// serde defaults an absent `Option<T>` to `None`, so these keys may be omitted
// as well as sent as null. The declaration says both rather than only one.
#[ts(optional_fields = nullable)]
pub struct NoteUpdateRequest {
    pub title: Option<String>,
    pub content: Option<String>,
    pub folder_id: Option<i64>,
}

impl NoteUpdateRequest {
    /// Refuse an update that would change nothing.
    ///
    /// Same rule as the API's `NotePatch::validate` (`crates/local-api/src/wire.rs:247`):
    /// an empty patch is almost always a caller bug, and answering "saved" to it
    /// hides one.
    fn validate(&self) -> CmdResult<notes::NoteUpdate> {
        if self.title.is_none() && self.content.is_none() && self.folder_id.is_none() {
            return Err(
                "nothing to change: send at least one of 'title', 'content', or 'folderId'"
                    .to_owned(),
            );
        }
        positive_id(self.folder_id, "folderId")?;
        if let Some(title) = &self.title {
            if title.len() > 1000 {
                return Err("'title' is too long".to_owned());
            }
        }

        Ok(notes::NoteUpdate {
            title: self.title.clone(),
            content: self.content.clone(),
            folder_id: self.folder_id,
        })
    }
}

/// Notes, most recently updated first.
///
/// There is no cursor, and that is not an omission. Notes are ordered by
/// `updated_at`, so an id cursor would skip or repeat rows as notes are edited.
/// The local API refuses a cursor here explicitly for the same reason
/// (`crates/local-api/src/server.rs:219`), and `notes::list` records the
/// reasoning at `crates/notes-core/src/notes.rs:104`.
#[tauri::command]
pub async fn notes_list(
    state: State<'_, AppState>,
    note_type: Option<String>,
    folder_id: Option<i64>,
    limit: Option<i64>,
) -> CmdResult<Vec<notes::Note>> {
    let folder_id = positive_id(folder_id, "folderId")?;
    let limit = notes_limit(limit)?;

    notes::list(state.db().pool(), note_type.as_deref(), folder_id, limit)
        .await
        .map_err(|e| fail("could not list notes", e))
}

/// Full-text search across notes.
///
/// The query is escaped into FTS5 syntax by `notes::escape_query` rather than
/// passed through, because users type prose and FTS5 reads expressions. A query
/// with nothing searchable in it returns no rows rather than every note.
#[tauri::command]
pub async fn notes_search(
    state: State<'_, AppState>,
    query: String,
    limit: Option<i64>,
) -> CmdResult<Vec<notes::SearchHit>> {
    if query.trim().is_empty() {
        return Err("a search needs something to search for".to_owned());
    }
    let limit = notes_limit(limit)?;

    notes::search(state.db().pool(), &query, limit)
        .await
        .map_err(|e| fail("could not search notes", e))
}

/// One note, or `None` if there is no such id.
#[tauri::command]
pub async fn notes_get(state: State<'_, AppState>, id: i64) -> CmdResult<Option<notes::Note>> {
    notes::get(state.db().pool(), id)
        .await
        .map_err(|e| fail("could not read the note", e))
}

/// Create a note, returning its id.
#[tauri::command]
pub async fn notes_create(state: State<'_, AppState>, request: NewNoteRequest) -> CmdResult<i64> {
    positive_id(request.folder_id, "folderId")?;

    notes::create(
        state.db().pool(),
        &notes::NewNote {
            recording_details: None,
            title: request.title,
            content: request.content,
            note_type: request.note_type,
            folder_id: request.folder_id,
            transcript: request.transcript,
            participants: request.participants,
            audio_duration_seconds: request.audio_duration_seconds,
            // Not the caller's to say: only the app's own saves know where a
            // transcript was made (KNEG-368).
            transcribed_by: None,
        },
    )
    .await
    .map_err(|e| fail("could not create the note", e))
}

/// Apply a partial update. Returns whether the note existed.
#[tauri::command]
pub async fn notes_update(
    state: State<'_, AppState>,
    id: i64,
    request: NoteUpdateRequest,
) -> CmdResult<bool> {
    let change = request.validate()?;

    notes::update(state.db().pool(), id, &change)
        .await
        .map_err(|e| fail("could not save the note", e))
}

/// Delete a note and its application-owned recording. Returns whether the note
/// existed.
///
/// Both desktop and local HTTP API use the shared transactional deletion path.
#[tauri::command]
pub async fn notes_delete(state: State<'_, AppState>, id: i64) -> CmdResult<bool> {
    note_recordings::delete_with_recording(state.db().pool(), id, state.recording_directory())
        .await
        .map_err(|e| fail("could not delete the note", e))
}

/// Move a note into a folder, or out of every folder with `folderId: null`.
///
/// `notes_update` cannot express "no folder": its `folderId` is absent or
/// set, as a partial update's should be. This is the one call that unfiles.
#[tauri::command]
pub async fn notes_set_folder(
    state: State<'_, AppState>,
    id: i64,
    folder_id: Option<i64>,
) -> CmdResult<bool> {
    notes::set_folder(state.db().pool(), id, folder_id)
        .await
        .map_err(|e| fail("could not move the note", e))
}

/// A note's speaker name is one edit for every segment with that raw label.
#[tauri::command]
pub async fn note_speaker_rename(
    state: State<'_, AppState>,
    id: i64,
    speaker_id: String,
    name: String,
) -> CmdResult<()> {
    if speaker_id.trim().is_empty() {
        return Err("a speaker id cannot be blank".to_owned());
    }
    speakers::rename(state.db().pool(), id, &speaker_id, &name)
        .await
        .map_err(|e| fail("could not rename the speaker", e))?
        .then_some(())
        .ok_or_else(|| "there is no note with that id".to_owned())
}

/// Offer note participants and previously used names while naming a speaker.
#[tauri::command]
pub async fn note_speaker_suggestions(
    state: State<'_, AppState>,
    id: i64,
) -> CmdResult<Vec<String>> {
    speakers::suggestions(state.db().pool(), id)
        .await
        .map_err(|e| fail("could not suggest speaker names", e))
}

/// The recording attached to a note, if it has one.
#[tauri::command]
pub async fn note_recording_get(
    state: State<'_, AppState>,
    id: i64,
) -> CmdResult<Option<NoteRecording>> {
    note_recordings::get(state.db().pool(), id)
        .await
        .map_err(|e| fail("could not read the note's recording", e))
}

/// Pick and attach in Rust: no filesystem path crosses this IPC boundary.
#[tauri::command]
pub async fn note_recording_attach_from_dialog(
    app: AppHandle,
    state: State<'_, AppState>,
    id: i64,
) -> CmdResult<Option<NoteRecording>> {
    use tauri_plugin_dialog::DialogExt as _;
    crate::recording_attach::ensure_available(state.db(), id).await?;
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog()
        .file()
        .set_title("Attach original recording")
        .add_filter("WAV recording", &["wav"])
        .pick_file(move |chosen| {
            let _ = tx.send(chosen);
        });
    let selected = rx
        .await
        .map_err(|e| fail("the open dialog closed unexpectedly", e))?
        .map(|chosen| {
            chosen
                .into_path()
                .map_err(|e| fail("could not resolve the chosen recording", e))
        })
        .transpose()?;
    crate::recording_attach::from_selection(
        state.db().clone(),
        id,
        selected,
        state.recording_directory().to_owned(),
    )
    .await
}

/// Folders in display order.
#[tauri::command]
pub async fn folders_list(state: State<'_, AppState>) -> CmdResult<Vec<notes::Folder>> {
    notes::folders(state.db().pool())
        .await
        .map_err(|e| fail("could not list folders", e))
}

/// Create a folder, returning its id.
#[tauri::command]
pub async fn folder_create(state: State<'_, AppState>, name: String) -> CmdResult<i64> {
    if name.trim().is_empty() {
        return Err("a folder needs a name".to_owned());
    }

    notes::create_folder(state.db().pool(), &name)
        .await
        .map_err(|e| fail("could not create the folder", e))
}

/// Delete a folder. Its notes survive, with no folder.
#[tauri::command]
pub async fn folder_delete(state: State<'_, AppState>, id: i64) -> CmdResult<bool> {
    notes::delete_folder(state.db().pool(), id)
        .await
        .map_err(|e| fail("could not delete the folder", e))
}

/* ------------------------------------------------------------ transcript export */

/// A rendered transcript, ready to preview or write.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ExportedTranscript {
    pub format: transcript::Format,
    /// A suggested filename, including the extension.
    pub filename: String,
    pub content: String,
    /// How many segments the transcript held. Zero is a legitimate answer for a
    /// note that was typed rather than recorded, and the UI should be able to
    /// say so rather than showing an export that looks empty for no reason.
    pub segment_count: usize,
}

/// Turn a note title into a filename stem.
///
/// Everything outside `[A-Za-z0-9]` becomes a hyphen, because the export is
/// written to a path the user picked on a filesystem we know nothing about, and
/// a title is free text that can hold separators, colons, and newlines.
#[cfg(test)]
fn export_filename(title: &str, format: transcript::Format) -> String {
    kw_notes_core::document::filename(title, format.extension())
}

/// Parse the single stored transcript representation shared by export and
/// playback rows.
fn transcript_segments(note: &notes::Note) -> CmdResult<Vec<transcript::Segment>> {
    // A note with no transcript is legitimate: typed notes export and render as
    // an empty transcript rather than failing. Search reads it the same way, so
    // a hit's row is this row (`transcript::parse_stored`, KNEG-406).
    transcript::parse_stored(note.transcript.as_deref())
        .map_err(|e| fail("the note's transcript is not a list of segments", e))
}

fn transcript_meta(note: &notes::Note) -> transcript::NoteMeta {
    transcript::NoteMeta {
        title: note.title.clone(),
        created_at: note.created_at.clone(),
        participants: note.participants.clone(),
    }
}

/// The Transcript tab's rename writes these names (KNEG-318). An unnamed
/// speaker still falls through to the segment's placeholder or capture source.
async fn transcript_mappings(
    db: &kw_storage::Db,
    id: i64,
) -> CmdResult<std::collections::BTreeMap<String, String>> {
    speakers::mappings(db.pool(), id)
        .await
        .map_err(|e| fail("could not read the note's speaker names", e))
}

/// Load one note's transcript as playable, unmerged rows.
#[tauri::command]
pub async fn note_transcript_rows(
    state: State<'_, AppState>,
    id: i64,
) -> CmdResult<Vec<TranscriptRow>> {
    let note = notes::get(state.db().pool(), id)
        .await
        .map_err(|e| fail("could not read the note", e))?
        .ok_or_else(|| "there is no note with that id".to_owned())?;
    let segments = transcript_segments(&note)?;
    let mappings = transcript_mappings(state.db(), id).await?;

    Ok(transcript::rows(
        &segments,
        &transcript_meta(&note),
        &mappings,
        &transcript::SpeakerLabels::default(),
    ))
}

/// Load a note and render its transcript.
async fn render_export(
    state: &AppState,
    id: i64,
    format: transcript::Format,
) -> CmdResult<ExportedTranscript> {
    let note = notes::get(state.db().pool(), id)
        .await
        .map_err(|e| fail("could not read the note", e))?
        .ok_or_else(|| "there is no note with that id".to_owned())?;

    let mappings = transcript_mappings(state.db(), id).await?;
    let document = kw_notes_core::document::render(&note, format, true, &mappings)
        .map_err(|e| fail("the note's transcript is not a list of segments", e))?;

    Ok(ExportedTranscript {
        format,
        filename: document.filename,
        content: document.content,
        segment_count: document.segment_count,
    })
}

/// Render a note's transcript without writing anything.
#[tauri::command]
pub async fn note_export(
    state: State<'_, AppState>,
    id: i64,
    format: transcript::Format,
) -> CmdResult<ExportedTranscript> {
    render_export(&state, id, format).await
}

/// Render a note's transcript and write it to a file the user picks.
///
/// Returns the path written, or `None` if the user cancelled.
///
/// **The save dialog is opened from Rust, not from the renderer.** The frontend
/// never sees or supplies a path, so there is no path to traverse — which is
/// why this needs no filesystem or dialog capability in
/// `capabilities/default.json`. `docs/SECURITY.md` names path traversal through
/// export as a threat; this is the shape that answers it.
#[tauri::command]
pub async fn note_export_to_file(
    app: AppHandle,
    state: State<'_, AppState>,
    id: i64,
    format: transcript::Format,
) -> CmdResult<Option<String>> {
    use tauri_plugin_dialog::DialogExt as _;

    // Rendered before the dialog opens, so a note that cannot be exported says
    // so instead of asking the user where to put nothing.
    let exported = render_export(&state, id, format).await?;

    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog()
        .file()
        .set_title("Export transcript")
        .set_file_name(&exported.filename)
        .add_filter(format.extension().to_uppercase(), &[format.extension()])
        .save_file(move |chosen| {
            // The receiver is dropped only if this command was cancelled, in
            // which case there is nobody left to tell.
            let _ = tx.send(chosen);
        });

    let Some(chosen) = rx
        .await
        .map_err(|e| fail("the save dialog closed unexpectedly", e))?
    else {
        return Ok(None);
    };

    let path = chosen
        .into_path()
        .map_err(|e| fail("could not resolve the chosen path", e))?;

    tokio::fs::write(&path, exported.content.as_bytes())
        .await
        .map_err(|e| fail("could not write the export", e))?;

    Ok(Some(path.display().to_string()))
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

    /// Every entry in [`PATH_TAKING_COMMANDS`] is registered by the application.
    ///
    /// **The mutation this exists for:** delete `media::media_import_video` from
    /// `lib.rs`'s `generate_handler!`. Before this test that was invisible — the
    /// IPC boundary test invokes a handler list it builds itself, so the
    /// application could stop exposing a command while every assertion about
    /// that command stayed green (KNEG-151).
    ///
    /// Reading `lib.rs` as text is the only way in: Rust has no reflection over
    /// `#[tauri::command]` registration, and `generate_handler!` needs literal
    /// paths, so the list cannot be generated from a const. The same parse is
    /// already done one language over by `tools/settings-audit/scan.mjs`.
    #[test]
    fn every_path_taking_command_is_registered() {
        const OPEN: &str = "generate_handler![";
        let lib = include_str!("lib.rs");
        let at = lib
            .find(OPEN)
            .expect("lib.rs registers with generate_handler!");
        let open = at + OPEN.len();
        let close = open + lib[open..].find("])").expect("the handler list is closed");
        let registered = &lib[open..close];

        // Controls, and the reason they are not obvious: the assertions below are
        // `contains` over a slice, so a parse that returned the *whole file* would
        // satisfy every one of them while proving nothing at all. These pin both
        // ends of the slice to the handler list itself.
        assert!(
            registered.contains("diagnostics::diagnostics"),
            "the parse did not reach the first registered command"
        );
        assert!(
            !registered.contains("prevent_close"),
            "the parse ran past the handler list into the window handlers"
        );

        for (command, _) in PATH_TAKING_COMMANDS {
            assert!(registered.contains(command), "{command} takes a renderer path and is not registered in lib.rs; register it, or drop it from PATH_TAKING_COMMANDS");
        }
    }

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

        // Upstream's default is Control+Super, which is modifier-only. The
        // cross-platform shortcut register still cannot take it —
        // `has_registrable` is about that register and only that — so it comes
        // back with an explanation rather than looking fine.
        assert!(!dictation.has_registrable());
        assert_eq!(dictation.problems().len(), 1);

        // What the explanation *says* now depends on the platform, because the
        // answer does. Windows has the keyboard hook and the shortcut works;
        // elsewhere it does nothing and the advice is to add a regular key.
        let problem = &dictation.problems()[0];
        if kw_platform::keyhook::support().is_available() {
            assert!(problem.contains("keyboard hook"), "{problem}");
            assert!(
                problem.contains("still works"),
                "a platform with the hook must not say the shortcut is dead: {problem}"
            );
        } else {
            assert!(problem.contains("does nothing"), "{problem}");
        }
    }

    #[tokio::test]
    async fn an_action_nothing_handles_is_reported_dead_with_the_reason() {
        let s = state().await;
        // A faultless accelerator for an action with no handler. Before this,
        // `Control+Shift+Space` registered with the platform, reported success,
        // took the chord away from every other application, and did nothing.
        settings::set(
            s.db().pool(),
            &HotkeyAction::ChatAgent.settings_key(),
            &"Control+Shift+Space",
        )
        .await
        .unwrap();

        let report = report_for(load_bindings(&s).await.unwrap());

        let chat = report
            .bindings
            .iter()
            .find(|b| b.action == HotkeyAction::ChatAgent)
            .unwrap();
        // The accelerator itself is still fine, and the report still says so.
        // The two facts are separate and both are reported.
        assert!(chat.has_registrable(), "the accelerator is not the problem");
        assert!(chat.problems().is_empty());

        let reason = report
            .unhandled
            .iter()
            .find(|u| u.action == HotkeyAction::ChatAgent)
            .map(|u| u.reason.clone())
            .expect("chat agent has no handler, so it must appear here");
        assert!(reason.contains("not built yet"), "{reason}");
    }

    /// The report carries the AltGr caution, and does it without asking the
    /// machine it happens to be running on.
    ///
    /// The layout is injected rather than read, which is the point: CI is Linux
    /// and would answer `Unknown`, and a developer on a US layout would get
    /// `Absent`. A test whose result depends on the tester's keyboard is a test
    /// that will be quietly wrong for someone.
    #[tokio::test]
    async fn a_hotkey_that_altgr_also_fires_is_reported_as_a_caution() {
        let s = state().await;
        settings::set(
            s.db().pool(),
            &HotkeyAction::Dictation.settings_key(),
            &"Control+Alt",
        )
        .await
        .unwrap();

        let bindings = load_bindings(&s).await.unwrap();
        let swedish = kw_platform::keyboard_layout::AltGr::Present { example: '@' };
        let report = report_with_layout(bindings.clone(), &swedish);

        // Two, and they are different facts about the same chord: AltGr depends
        // on the layout, being a prefix of a longer shortcut depends only on
        // the chord having two keys. Either can hold without the other, so they
        // are separate sentences rather than one merged one -- and asserting
        // "exactly one caution" was what this used to do, which would have
        // hidden the second the moment it arrived (KNEG-115).
        assert_eq!(report.cautions.len(), 2, "{:?}", report.cautions);
        assert!(report
            .cautions
            .iter()
            .all(|c| c.action == HotkeyAction::Dictation));
        assert!(
            report.cautions.iter().any(|c| c.reason.contains("AltGr")),
            "{:?}",
            report.cautions
        );
        assert!(
            report
                .cautions
                .iter()
                .any(|c| c.reason.contains("Ctrl+Alt+Delete")),
            "{:?}",
            report.cautions
        );

        // And it is a caution rather than a problem: the hotkey is fine. Saying
        // otherwise would send someone looking for a typo in an accelerator
        // that is spelled perfectly and works exactly as written.
        let dictation = report
            .bindings
            .iter()
            .find(|b| b.action == HotkeyAction::Dictation)
            .unwrap();
        assert!(report.conflicts.is_empty());
        assert!(
            dictation.problems().iter().all(|p| !p.contains("AltGr")),
            "the caution must not also be reported as a problem: {:?}",
            dictation.problems()
        );
        // The same bindings on a layout with no AltGr lose the AltGr caution and
        // keep the other one. That is the separation earning its place: a US
        // keyboard cannot press AltGr, and it can press Ctrl+Alt+Delete.
        let us = kw_platform::keyboard_layout::AltGr::Absent;
        let on_us = report_with_layout(bindings, &us).cautions;
        assert_eq!(on_us.len(), 1, "{on_us:?}");
        assert!(!on_us[0].reason.contains("AltGr"), "{:?}", on_us[0]);
        assert!(
            on_us[0].reason.contains("Ctrl+Alt+Delete"),
            "{:?}",
            on_us[0]
        );
    }

    #[tokio::test]
    async fn only_dictation_is_missing_from_the_unhandled_list() {
        let s = state().await;
        let report = report_for(load_bindings(&s).await.unwrap());

        let unhandled: Vec<HotkeyAction> = report.unhandled.iter().map(|u| u.action).collect();
        assert!(!unhandled.contains(&HotkeyAction::Dictation));
        assert_eq!(unhandled.len(), hotkey::ALL_ACTIONS.len() - 1);
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
        assert!(dictation.has_registrable());
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

    /// Four of the seven transcription providers cannot be dispatched.
    ///
    /// Derived by asking the real router rather than by listing ids, so this
    /// test also fails if a provider silently starts resolving to something —
    /// which is what a fallback to OpenAI would look like from here.
    #[test]
    fn a_provider_without_a_client_says_so_rather_than_reading_as_available() {
        let registry = kw_provider_registry::registry().expect("registry");

        for id in ["openai", "groq", "berget"] {
            assert!(
                matches!(dispatch_for(registry, id), Dispatch::Ready),
                "{id} has an HttpBatch transport and should be usable"
            );
        }

        // Three of the four have clients now, so they dispatch — even though
        // they still need a key, which `configured` says separately.
        for id in ["xai", "mistral"] {
            assert!(
                matches!(dispatch_for(registry, id), Dispatch::Ready),
                "{id} has a client and should dispatch"
            );
        }

        // Corti has a client, and an unconfigured Corti does not resolve at all:
        // the route refuses to guess an environment or a tenant. `Unresolvable`
        // rather than `NeedsClient`, because the gap is configuration.
        match dispatch_for(registry, "corti") {
            Dispatch::Unresolvable { reason } => {
                assert!(reason.contains("environment"), "{reason}");
                assert!(reason.contains("Nothing was sent"), "{reason}");
            }
            other => panic!("unconfigured corti should not be ready, got {other:?}"),
        }

        // Tinfoil is refused on purpose, and the reason says so rather than
        // reading as an unfinished feature.
        match dispatch_for(registry, "tinfoil") {
            Dispatch::NeedsClient { reason } => {
                assert!(reason.contains("attestation"), "{reason}");
            }
            other => panic!("tinfoil should be refused, got {other:?}"),
        }
    }

    /// An id the registry does not know is refused, never defaulted.
    ///
    /// Upstream's terminal fallback returned OpenAI for anything unrecognised,
    /// which meant a typo sent the user's audio to a provider they never chose.
    /// The refusal here does mention OpenAI — to say what it is *not* doing —
    /// so what this asserts is the state, and that the message says nothing was
    /// sent rather than leaving that to be assumed.
    #[test]
    fn an_unknown_provider_is_unresolvable_rather_than_openai() {
        let registry = kw_provider_registry::registry().expect("registry");
        match dispatch_for(registry, "not-a-provider") {
            Dispatch::Unresolvable { reason } => {
                assert!(
                    reason.contains("Nothing was sent"),
                    "a refusal has to say the audio stayed here: {reason}"
                );
            }
            other => panic!("expected a refusal, got {other:?}"),
        }
    }

    /// The dispatch state crosses the boundary as a discriminant a UI can
    /// switch on, with its fields in the same camelCase as everything else.
    #[test]
    fn dispatch_crosses_the_boundary_tagged() {
        let json = serde_json::to_value(Dispatch::NeedsClient {
            reason: "no client".to_owned(),
        })
        .expect("serialize");
        assert_eq!(json["state"], "needs-client");
        assert_eq!(json["reason"], "no client");

        let ready = serde_json::to_value(Dispatch::Ready).expect("serialize");
        assert_eq!(ready["state"], "ready");
    }

    #[test]
    fn the_error_helper_does_not_use_debug_formatting() {
        // Display is where redaction lives; Debug on a credential error would be
        // fine today but this keeps the habit.
        let rendered = fail("context", "detail");
        assert_eq!(rendered, "context: detail");
    }

    /* ------------------------------------------------------------------ notes */

    // These assert the exact JSON keys crossing the boundary, because the one
    // recurring defect in this codebase is a frontend type describing the other
    // side of the boundary that was written rather than derived — see the
    // completion plan's defects 2, 3 and 4. `apps/desktop/src/lib/ipc.ts` must
    // match what these tests print.

    #[test]
    fn a_note_crosses_the_boundary_in_camel_case() {
        let note = notes::Note {
            recording_details: None,
            id: 1,
            title: "Standup".to_owned(),
            content: "body".to_owned(),
            note_type: "personal".to_owned(),
            enhanced_content: None,
            folder_id: Some(2),
            transcript: None,
            participants: None,
            audio_duration_seconds: Some(12.5),
            transcribed_by: None,
            created_at: Some("2026-08-25 10:00:00".to_owned()),
            updated_at: Some("2026-08-25 10:05:00".to_owned()),
        };

        let json = serde_json::to_value(&note).unwrap();
        let mut keys: Vec<&str> = json
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();

        assert_eq!(
            keys,
            [
                "audioDurationSeconds",
                "content",
                "createdAt",
                "enhancedContent",
                "folderId",
                "id",
                "noteType",
                "participants",
                "recordingDetails",
                "title",
                "transcribedBy",
                "transcript",
                "updatedAt",
            ]
        );
    }

    #[test]
    fn a_folder_and_a_search_hit_cross_the_boundary_in_camel_case() {
        let folder = serde_json::to_value(notes::Folder {
            id: 1,
            name: "Work".to_owned(),
            is_default: true,
            sort_order: 0,
        })
        .unwrap();
        let mut folder_keys: Vec<&str> = folder
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        folder_keys.sort_unstable();
        assert_eq!(folder_keys, ["id", "isDefault", "name", "sortOrder"]);

        let hit = serde_json::to_value(notes::SearchHit {
            id: 1,
            title: "Standup".to_owned(),
            snippet: "a [match] here".to_owned(),
            transcript_row: None,
        })
        .unwrap();
        let mut hit_keys: Vec<&str> = hit
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        hit_keys.sort_unstable();
        assert_eq!(hit_keys, ["id", "snippet", "title", "transcriptRow"]);
    }

    #[test]
    fn every_export_format_has_the_wire_spelling_the_frontend_sends() {
        // The picker offers one option per format, so a variant renamed on the
        // Rust side without the frontend following would silently stop matching.
        let spellings: Vec<String> = transcript::Format::all()
            .iter()
            .map(|f| serde_json::to_string(f).unwrap())
            .collect();

        assert_eq!(
            spellings,
            [r#""txt""#, r#""md""#, r#""json""#, r#""srt""#, r#""vtt""#]
        );

        // And they round-trip, so `note_export` accepts what `Format::all` emits.
        for format in transcript::Format::all() {
            let sent = serde_json::to_string(&format).unwrap();
            let received: transcript::Format = serde_json::from_str(&sent).unwrap();
            assert_eq!(received, format);
        }
    }

    #[test]
    fn an_exported_transcript_crosses_the_boundary_in_camel_case() {
        let json = serde_json::to_value(ExportedTranscript {
            format: transcript::Format::Md,
            filename: "standup.md".to_owned(),
            content: "# Standup".to_owned(),
            segment_count: 0,
        })
        .unwrap();

        let mut keys: Vec<&str> = json
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        assert_eq!(keys, ["content", "filename", "format", "segmentCount"]);
    }

    #[test]
    fn a_request_with_a_mistyped_field_is_refused_rather_than_ignored() {
        // `folder` is not `folderId`. Without deny_unknown_fields this would
        // deserialize happily and create the note outside the folder the user
        // chose, which is the same class of silent-drop defect as a frontend
        // type declaring a field the backend never sends.
        let err = serde_json::from_value::<NewNoteRequest>(serde_json::json!({
            "title": "Standup",
            "content": "",
            "folder": 3
        }))
        .unwrap_err();

        assert!(err.to_string().contains("folder"), "{err}");
    }

    #[test]
    fn an_update_that_would_change_nothing_is_refused() {
        let err = NoteUpdateRequest::default().validate().unwrap_err();
        assert!(err.contains("nothing to change"), "{err}");
    }

    #[test]
    fn an_update_validates_the_folder_id_and_the_title_length() {
        let bad_folder = NoteUpdateRequest {
            folder_id: Some(0),
            ..Default::default()
        };
        assert!(bad_folder.validate().unwrap_err().contains("positive id"));

        let long_title = NoteUpdateRequest {
            title: Some("x".repeat(1001)),
            ..Default::default()
        };
        assert!(long_title.validate().unwrap_err().contains("too long"));
    }

    #[test]
    fn a_limit_is_clamped_upward_and_refused_at_zero() {
        assert_eq!(notes_limit(None).unwrap(), NOTES_DEFAULT_LIMIT);
        assert_eq!(notes_limit(Some(10)).unwrap(), 10);
        // Clamped, not refused: a caller asking for too much gets fewer rows
        // rather than an error, matching the local API.
        assert_eq!(notes_limit(Some(100_000)).unwrap(), NOTES_MAX_LIMIT);
        assert!(notes_limit(Some(0)).is_err());
        assert!(notes_limit(Some(-1)).is_err());
    }

    #[test]
    fn an_export_filename_survives_a_hostile_title() {
        // A title is free text. Path separators, colons and newlines in it must
        // not reach the filesystem.
        assert_eq!(
            export_filename("../../etc/passwd", transcript::Format::Txt),
            "etc-passwd.txt"
        );
        assert_eq!(
            export_filename("Q3: Review\nnotes", transcript::Format::Md),
            "Q3-Review-notes.md"
        );
        assert_eq!(
            export_filename("Standup", transcript::Format::Srt),
            "Standup.srt"
        );
        // Nothing usable in the title still produces a valid filename.
        assert_eq!(
            export_filename("///", transcript::Format::Vtt),
            "transcript.vtt"
        );
        assert_eq!(
            export_filename("", transcript::Format::Json),
            "transcript.json"
        );
    }

    #[test]
    fn every_import_pick_kind_has_a_title_and_a_nonempty_filter() {
        // `import_pick_dialog` is the pure decision `pick_import_file` makes
        // before it ever touches an `AppHandle` -- see the module doc on why a
        // Tauri-dependent command cannot be named as a value in a test.
        for kind in [
            ImportPickKind::Audio,
            ImportPickKind::Video,
            ImportPickKind::OpenWhisprDatabase,
        ] {
            let (title, filter_name, extensions) = import_pick_dialog(kind);
            assert!(!title.is_empty(), "{kind:?} has no dialog title");
            assert!(!filter_name.is_empty(), "{kind:?} has no filter name");
            assert!(!extensions.is_empty(), "{kind:?} offers no extensions");
        }
    }

    #[test]
    fn import_pick_kind_crosses_the_boundary_in_kebab_case() {
        // The frontend sends one of these three strings; a rename here without
        // a matching change in `lib/ipc.ts` would fail silently as an unknown
        // variant instead of a compile error, so the wire spelling is pinned.
        assert_eq!(
            serde_json::to_value(ImportPickKind::Audio).unwrap(),
            serde_json::json!("audio")
        );
        assert_eq!(
            serde_json::to_value(ImportPickKind::Video).unwrap(),
            serde_json::json!("video")
        );
        assert_eq!(
            serde_json::to_value(ImportPickKind::OpenWhisprDatabase).unwrap(),
            serde_json::json!("open-whispr-database")
        );
    }

    #[tokio::test]
    async fn notes_round_trip_through_the_command_layer() {
        let s = state().await;

        let id = notes::create(
            s.db().pool(),
            &notes::NewNote {
                title: "Standup".to_owned(),
                content: "we shipped the parity gate".to_owned(),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let listed = notes::list(s.db().pool(), None, None, NOTES_DEFAULT_LIMIT)
            .await
            .unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, id);

        // The search box takes prose, not FTS5 syntax.
        let hits = notes::search(s.db().pool(), "parity gate", 10)
            .await
            .unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].id, id);

        assert!(notes::delete(s.db().pool(), id).await.unwrap());
        assert!(notes::get(s.db().pool(), id).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn a_note_with_no_transcript_exports_as_an_empty_one() {
        let s = state().await;
        let id = notes::create(
            s.db().pool(),
            &notes::NewNote {
                title: "Typed, not spoken".to_owned(),
                content: "body".to_owned(),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let exported = render_export(&s, id, transcript::Format::Txt)
            .await
            .unwrap();

        // Zero segments is a fact the caller can act on, not a failure.
        assert_eq!(exported.segment_count, 0);
        assert_eq!(exported.filename, "Typed-not-spoken.txt");
        assert!(exported.content.contains("Typed, not spoken"));
    }

    #[tokio::test]
    async fn a_recorded_note_exports_its_segments() {
        let s = state().await;
        let transcript_json = serde_json::json!([
            { "text": "first thing", "timestamp": 0.0, "endTimestamp": 2.0 },
            { "text": "second thing", "timestamp": 30.0, "endTimestamp": 32.0 },
        ])
        .to_string();

        let id = notes::create(
            s.db().pool(),
            &notes::NewNote {
                title: "Standup".to_owned(),
                content: String::new(),
                transcript: Some(transcript_json),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let exported = render_export(&s, id, transcript::Format::Srt)
            .await
            .unwrap();
        assert_eq!(exported.segment_count, 2);
        assert!(exported.content.contains("first thing"));
        assert!(exported.content.contains("second thing"));
        // SRT cue timing, which is the whole reason the format exists.
        assert!(
            exported.content.contains("00:00:30"),
            "{}",
            exported.content
        );
    }

    #[tokio::test]
    async fn a_transcript_that_is_not_a_segment_list_fails_with_a_readable_reason() {
        let s = state().await;
        let id = notes::create(
            s.db().pool(),
            &notes::NewNote {
                title: "Broken".to_owned(),
                content: String::new(),
                transcript: Some("not json at all".to_owned()),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let err = render_export(&s, id, transcript::Format::Txt)
            .await
            .unwrap_err();
        assert!(err.contains("not a list of segments"), "{err}");
    }

    #[tokio::test]
    async fn exporting_a_note_that_does_not_exist_says_so() {
        let s = state().await;
        let err = render_export(&s, 9999, transcript::Format::Txt)
            .await
            .unwrap_err();
        assert!(err.contains("no note with that id"), "{err}");
    }

    #[tokio::test]
    async fn deleting_a_folder_leaves_its_notes_behind() {
        let s = state().await;
        let folder = notes::create_folder(s.db().pool(), "Work").await.unwrap();
        let note = notes::create(
            s.db().pool(),
            &notes::NewNote {
                title: "In a folder".to_owned(),
                folder_id: Some(folder),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        assert!(notes::delete_folder(s.db().pool(), folder).await.unwrap());

        let survivor = notes::get(s.db().pool(), note).await.unwrap().unwrap();
        assert_eq!(survivor.folder_id, None);
    }
}
