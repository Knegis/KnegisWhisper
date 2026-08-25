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

use kw_notes_core::{notes, transcript};
use kw_platform::hotkey::{self, Binding, HotkeyAction};
use kw_provider_probe::{ProbeAudio, ProbeReport};
use kw_storage::{settings, transcriptions};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, State};

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

/// What changing one action's hotkey actually achieved.
///
/// `binding` is what was stored and how it classifies; `registrations` is what
/// the platform said when asked to take it. Both, because they fail
/// independently: a well-formed accelerator can still be refused because another
/// application owns it, and reporting "saved" for that is the kind of confident
/// success this project treats as a defect.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HotkeyChange {
    pub binding: Binding,
    pub registrations: Vec<crate::shortcuts::Registration>,
    /// True when at least one accelerator for this action is now live.
    pub live: bool,
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
) -> CmdResult<HotkeyChange> {
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

    // This action's own outcome, pulled out of the whole-set re-registration.
    // Absent only if `ALL_ACTIONS` and the stored bindings ever disagree, which
    // would be a bug rather than a state to render, so it reports as not live
    // with no results rather than inventing a success.
    let mine = reports.into_iter().find(|r| r.action == action);

    Ok(HotkeyChange {
        binding: Binding::from_stored(action, &canonical),
        registrations: mine.as_ref().map(|r| r.results.clone()).unwrap_or_default(),
        live: mine.is_some_and(|r| r.is_live()),
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
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
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
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
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
            title: request.title,
            content: request.content,
            note_type: request.note_type,
            folder_id: request.folder_id,
            transcript: request.transcript,
            participants: request.participants,
            audio_duration_seconds: request.audio_duration_seconds,
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

/// Delete a note. Returns whether it existed.
#[tauri::command]
pub async fn notes_delete(state: State<'_, AppState>, id: i64) -> CmdResult<bool> {
    notes::delete(state.db().pool(), id)
        .await
        .map_err(|e| fail("could not delete the note", e))
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
#[derive(Debug, Clone, Serialize)]
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
fn export_filename(title: &str, format: transcript::Format) -> String {
    let mut stem = String::new();
    let mut pending_hyphen = false;

    for ch in title.chars() {
        if ch.is_ascii_alphanumeric() {
            if pending_hyphen && !stem.is_empty() {
                stem.push('-');
            }
            pending_hyphen = false;
            stem.push(ch);
        } else {
            pending_hyphen = true;
        }
    }

    if stem.is_empty() {
        stem.push_str("transcript");
    }
    stem.truncate(80);

    format!("{stem}.{}", format.extension())
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

    // A note with no transcript exports as an empty one rather than failing:
    // a typed note is a legitimate thing to export, and `segment_count` tells
    // the caller which case it got.
    let segments: Vec<transcript::Segment> = match note.transcript.as_deref() {
        Some(raw) if !raw.trim().is_empty() => serde_json::from_str(raw)
            .map_err(|e| fail("the note's transcript is not a list of segments", e))?,
        _ => Vec::new(),
    };

    let meta = transcript::NoteMeta {
        title: note.title.clone(),
        created_at: note.created_at.clone(),
        participants: note.participants.clone(),
    };

    // Speaker names live in `speaker_mappings`, which nothing writes yet because
    // there is no diarisation. An empty map is the honest input rather than a
    // missing one: `transcript::speaker_name` then falls back to the segment's
    // label or capture source, which is exactly what it should do.
    let mappings = std::collections::BTreeMap::new();

    let content = transcript::export(
        format,
        &meta,
        &segments,
        &mappings,
        &transcript::SpeakerLabels::default(),
    );

    Ok(ExportedTranscript {
        format,
        filename: export_filename(&note.title, format),
        content,
        segment_count: segments.len(),
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

    /* ------------------------------------------------------------------ notes */

    // These assert the exact JSON keys crossing the boundary, because the one
    // recurring defect in this codebase is a frontend type describing the other
    // side of the boundary that was written rather than derived — see the
    // completion plan's defects 2, 3 and 4. `apps/desktop/src/lib/ipc.ts` must
    // match what these tests print.

    #[test]
    fn a_note_crosses_the_boundary_in_camel_case() {
        let note = notes::Note {
            id: 1,
            title: "Standup".to_owned(),
            content: "body".to_owned(),
            note_type: "personal".to_owned(),
            enhanced_content: None,
            folder_id: Some(2),
            transcript: None,
            participants: None,
            audio_duration_seconds: Some(12.5),
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
                "title",
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
        })
        .unwrap();
        let mut hit_keys: Vec<&str> = hit
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        hit_keys.sort_unstable();
        assert_eq!(hit_keys, ["id", "snippet", "title"]);
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
