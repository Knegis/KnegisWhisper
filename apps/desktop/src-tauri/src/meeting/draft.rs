//! Typed meeting notes live in memory and beside the WAV for crash durability.
//!
//! An interrupted meeting's `.notes.md` sidecar can be recovered from the
//! Meetings page with its orphan WAV. It is session data, not a settings key.
//! So is the `.meeting.json` beside it, the name and folder chosen before
//! Start (KNEG-365) and whether the recording is deleted once saved (KNEG-409).

use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
use tauri::{Runtime, State, WebviewWindow};

use super::MeetingPlan;

/// The notes typed in the meeting panel while a meeting runs.
#[derive(Debug, Default)]
pub struct NotesDraft {
    inner: Mutex<Inner>,
    writes: tokio::sync::Mutex<()>,
}

#[derive(Debug, Default)]
struct Inner {
    open: bool,
    markdown: String,
    sidecar: Option<PathBuf>,
}

impl NotesDraft {
    /// Start a fresh session and remove an obsolete sidecar for its WAV name.
    pub fn begin(&self, recording: &Path) {
        let sidecar = sidecar_path(recording);
        remove_sidecar(recording);
        if let Ok(mut inner) = self.inner.lock() {
            inner.open = true;
            inner.markdown.clear();
            inner.sidecar = Some(sidecar);
        }
    }

    /// Update memory before atomically replacing the sidecar.
    pub async fn update(&self, markdown: String) -> Result<(), String> {
        let _guard = self.writes.lock().await;
        let sidecar = {
            let mut inner = self.inner.lock().map_err(|e| e.to_string())?;
            if !inner.open {
                return Err("No meeting is open".into());
            }
            inner.markdown = markdown.clone();
            inner
                .sidecar
                .clone()
                .ok_or("Meeting notes path unavailable")?
        };
        let mut tmp_name = sidecar.as_os_str().to_os_string();
        tmp_name.push(".tmp");
        let tmp = PathBuf::from(tmp_name);
        tokio::fs::write(&tmp, markdown)
            .await
            .map_err(|e| e.to_string())?;
        tokio::fs::rename(&tmp, &sidecar)
            .await
            .map_err(|e| e.to_string())
    }

    /// Read the current session text, or an empty string when closed.
    pub fn text(&self) -> String {
        self.inner
            .lock()
            .ok()
            .filter(|inner| inner.open)
            .map(|inner| inner.markdown.clone())
            .unwrap_or_default()
    }

    /// Close the session and return its text; the sidecar remains until removed.
    pub fn take(&self) -> String {
        match self.inner.lock() {
            Ok(mut inner) if inner.open => {
                inner.open = false;
                std::mem::take(&mut inner.markdown)
            }
            _ => String::new(),
        }
    }

    /// Wait for any update that wrote memory before `take` to finish its sidecar.
    pub async fn flush(&self) {
        let _guard = self.writes.lock().await;
    }
}

/// `x.wav` becomes `x.notes.md` beside the recording.
pub fn sidecar_path(recording: &Path) -> PathBuf {
    recording.with_extension("notes.md")
}

/// `x.wav` becomes `x.meeting.json`: the meeting's plan, its name and folder
/// (KNEG-365) and its delete choice (KNEG-409), beside the recording for the
/// same reason the notes are.
pub fn plan_path(recording: &Path) -> PathBuf {
    recording.with_extension("meeting.json")
}

/// Write the meeting's plan beside its recording: to a temporary file, then
/// renamed over the plan, so a crash mid-write never leaves half of one.
pub fn write_plan(recording: &Path, plan: &MeetingPlan) -> Result<(), String> {
    let target = plan_path(recording);
    let mut tmp_name = target.as_os_str().to_os_string();
    tmp_name.push(".tmp");
    let tmp = PathBuf::from(tmp_name);
    let json = serde_json::to_vec(plan).map_err(|e| e.to_string())?;
    std::fs::write(&tmp, json).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &target).map_err(|e| e.to_string())
}

/// The plan beside `recording`, or `None` when there is none to read: a
/// recording made before plans existed, a plan whose write failed, or one
/// that is not a regular file or not a plan, which is also logged.
pub fn read_plan(recording: &Path) -> Option<MeetingPlan> {
    let path = plan_path(recording);
    match std::fs::symlink_metadata(&path) {
        Ok(metadata) if metadata.is_file() => {}
        Ok(_) => {
            tracing::warn!(path = %path.display(), "the meeting plan is not a regular file; ignoring it");
            return None;
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return None,
        Err(error) => {
            tracing::warn!(%error, path = %path.display(), "could not inspect the meeting plan");
            return None;
        }
    }
    let read = std::fs::read(&path)
        .map_err(|error| error.to_string())
        .and_then(|bytes| serde_json::from_slice(&bytes).map_err(|error| error.to_string()));
    match read {
        Ok(plan) => Some(plan),
        Err(error) => {
            tracing::warn!(%error, path = %path.display(), "could not read the meeting plan; ignoring it");
            None
        }
    }
}

/// `x.wav` becomes `x.unfinished`: an empty file that is there from Start
/// until the meeting ends in any way the application saw -- saved, nothing
/// transcribed, failed, discarded.
///
/// A recording with no note is offered for recovery whatever left it so, and
/// most are left on purpose: a quiet room keeps its recording. This is what
/// tells those from a meeting the application never finished, because it
/// crashed or was closed, so only that one is raised from every screen.
pub fn unfinished_path(recording: &Path) -> PathBuf {
    recording.with_extension("unfinished")
}

/// Mark `recording` as a meeting still running. A failure costs only the
/// notice: the recording is offered under Interrupted either way.
pub fn mark_unfinished(recording: &Path) {
    if let Err(error) = std::fs::write(unfinished_path(recording), b"") {
        tracing::warn!(%error, "could not mark the meeting as running");
    }
}

/// The meeting ended where the application could see it.
pub fn clear_unfinished(recording: &Path) {
    remove_quietly(&unfinished_path(recording), "running-meeting marker");
}

/// Whether `recording` is of a meeting the application never saw end.
pub fn is_unfinished(recording: &Path) -> bool {
    std::fs::symlink_metadata(unfinished_path(recording)).is_ok_and(|metadata| metadata.is_file())
}

/// Remove the typed notes' sidecar once the notes are safely saved or have
/// nothing in them. The plan stays: see [`remove_sidecars`].
pub fn remove_sidecar(recording: &Path) {
    remove_quietly(&sidecar_path(recording), "meeting notes sidecar");
}

/// Remove the typed notes, allowing deletion to stop if they remain on disk.
pub fn try_remove_sidecar(recording: &Path) -> std::io::Result<()> {
    match std::fs::remove_file(sidecar_path(recording)) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        result => result,
    }
}

/// Remove both sidecars, the notes and the plan, once the recording no longer
/// needs them: it belongs to a saved note, or it was deleted. Where the
/// recording is left with no note -- offered for recovery -- only
/// [`remove_sidecar`] runs, so the recovered note still gets its name, folder
/// and delete choice.
pub fn remove_sidecars(recording: &Path) {
    remove_sidecar(recording);
    remove_plan(recording);
    clear_unfinished(recording);
}

/// Remove the plan alone: the recording it describes is gone, but the typed
/// notes beside it could not be saved and stay on disk.
pub fn remove_plan(recording: &Path) {
    remove_quietly(&plan_path(recording), "meeting plan");
}

/// Remove the plan, allowing deletion to stop if its meeting details remain.
pub fn try_remove_plan(recording: &Path) -> std::io::Result<()> {
    match std::fs::remove_file(plan_path(recording)) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        result => result,
    }
}

fn remove_quietly(path: &Path, what: &str) {
    if let Err(error) = std::fs::remove_file(path) {
        if error.kind() != std::io::ErrorKind::NotFound {
            tracing::warn!(%error, path = %path.display(), "could not remove the {what}");
        }
    }
}

#[tauri::command]
pub async fn meeting_notes_update<R: Runtime>(
    window: WebviewWindow<R>,
    draft: State<'_, Arc<NotesDraft>>,
    markdown: String,
) -> Result<(), String> {
    if window.label() != crate::meeting_window::LABEL {
        return Err("Only the meeting window can update meeting notes".into());
    }
    draft.update(markdown).await
}

#[tauri::command]
pub async fn meeting_notes_get<R: Runtime>(
    window: WebviewWindow<R>,
    draft: State<'_, Arc<NotesDraft>>,
) -> Result<String, String> {
    if window.label() != crate::meeting_window::LABEL {
        return Err("Only the meeting window can read meeting notes".into());
    }
    Ok(draft.text())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn beginning_a_session_clears_text_and_removes_a_stale_sidecar() {
        let dir = tempfile::tempdir().unwrap();
        let wav = dir.path().join("x.wav");
        let sidecar = sidecar_path(&wav);
        let draft = NotesDraft::default();
        draft.begin(&wav);
        draft.update("old".into()).await.unwrap();
        assert!(sidecar.exists());
        draft.begin(&wav);
        assert_eq!(draft.text(), "");
        assert!(!sidecar.exists());
    }

    #[tokio::test]
    async fn updates_require_an_open_meeting_and_are_mirrored_beside_the_wav() {
        let dir = tempfile::tempdir().unwrap();
        let wav = dir.path().join("x.wav");
        let draft = NotesDraft::default();
        assert!(draft.update("early".into()).await.is_err());
        draft.begin(&wav);
        draft.update("typed notes".into()).await.unwrap();
        assert_eq!(draft.text(), "typed notes");
        assert_eq!(
            std::fs::read_to_string(sidecar_path(&wav)).unwrap(),
            "typed notes"
        );
        assert_eq!(draft.take(), "typed notes");
        assert_eq!(draft.text(), "");
        assert!(draft.update("late".into()).await.is_err());
    }

    #[test]
    fn the_sidecar_uses_a_notes_markdown_extension_beside_the_recording() {
        assert_eq!(
            sidecar_path(Path::new("x.wav")),
            PathBuf::from("x.notes.md")
        );
    }

    /// Fallible notes removal accepts an absent file but reports a directory.
    #[test]
    fn trying_to_remove_the_sidecar_reports_whether_it_was_removed() {
        let dir = tempfile::tempdir().unwrap();
        let wav = dir.path().join("x.wav");
        let sidecar = sidecar_path(&wav);
        std::fs::write(&sidecar, b"typed").unwrap();
        assert!(try_remove_sidecar(&wav).is_ok());
        assert!(!sidecar.exists());
        assert!(try_remove_sidecar(&wav).is_ok());
        std::fs::create_dir(&sidecar).unwrap();
        assert!(try_remove_sidecar(&wav).is_err());
        assert!(sidecar.is_dir());
    }

    /// Fallible plan removal accepts an absent file but reports a directory.
    #[test]
    fn trying_to_remove_the_plan_reports_whether_it_was_removed() {
        let dir = tempfile::tempdir().unwrap();
        let wav = dir.path().join("x.wav");
        let plan = plan_path(&wav);
        std::fs::write(&plan, b"{}").unwrap();
        assert!(try_remove_plan(&wav).is_ok());
        assert!(!plan.exists());
        assert!(try_remove_plan(&wav).is_ok());
        std::fs::create_dir(&plan).unwrap();
        assert!(try_remove_plan(&wav).is_err());
        assert!(plan.is_dir());
    }

    /* ------------------------------------------------------- the plan (KNEG-365) */

    #[test]
    fn the_plan_sits_beside_the_recording_as_meeting_json() {
        assert_eq!(
            plan_path(Path::new("x.wav")),
            PathBuf::from("x.meeting.json")
        );
    }

    #[test]
    fn a_plan_written_beside_the_recording_reads_back_as_written() {
        let dir = tempfile::tempdir().unwrap();
        let wav = dir.path().join("x.wav");
        let plan = MeetingPlan {
            title: Some("Board review".into()),
            folder_id: Some(3),
            delete_recording: true,
            live_provider_id: Some("openai-realtime".into()),
            calendar: None,
        };
        write_plan(&wav, &plan).unwrap();
        assert_eq!(read_plan(&wav), Some(plan));
        // Replaced, not appended to, and no temporary file left behind.
        write_plan(&wav, &MeetingPlan::default()).unwrap();
        assert_eq!(read_plan(&wav), Some(MeetingPlan::default()));
        let names: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(names, ["x.meeting.json"]);
    }

    #[test]
    fn a_missing_or_unreadable_plan_is_no_plan() {
        let dir = tempfile::tempdir().unwrap();
        let wav = dir.path().join("x.wav");
        assert_eq!(read_plan(&wav), None);
        std::fs::write(plan_path(&wav), b"{ not json").unwrap();
        assert_eq!(read_plan(&wav), None);
        std::fs::write(plan_path(&wav), b"[1, 2]").unwrap();
        assert_eq!(read_plan(&wav), None);
    }

    #[test]
    fn a_plan_missing_fields_reads_with_their_defaults() {
        // A plan written before a field existed still reads. One from before
        // KNEG-409 has no delete choice, and reads as keeping the recording.
        let dir = tempfile::tempdir().unwrap();
        let wav = dir.path().join("x.wav");
        std::fs::write(plan_path(&wav), br#"{"title":"Standup","folderId":4}"#).unwrap();
        assert_eq!(
            read_plan(&wav),
            Some(MeetingPlan {
                title: Some("Standup".into()),
                folder_id: Some(4),
                delete_recording: false,
                live_provider_id: None,
                calendar: None,
            })
        );
        std::fs::write(plan_path(&wav), b"{}").unwrap();
        assert_eq!(read_plan(&wav), Some(MeetingPlan::default()));
    }

    #[test]
    fn removing_the_sidecars_removes_the_notes_and_the_plan() {
        let dir = tempfile::tempdir().unwrap();
        let wav = dir.path().join("x.wav");
        std::fs::write(&wav, b"audio").unwrap();
        std::fs::write(sidecar_path(&wav), b"typed").unwrap();
        write_plan(&wav, &MeetingPlan::default()).unwrap();

        remove_sidecars(&wav);
        assert!(!sidecar_path(&wav).exists());
        assert!(!plan_path(&wav).exists());
        assert!(wav.exists(), "the recording is not a sidecar");
        // Nothing to remove is not a failure.
        remove_sidecars(&wav);
    }

    #[test]
    fn removing_the_plan_leaves_the_typed_notes() {
        // Discard's notes that could not be saved stay on disk; the plan
        // beside them describes a recording that is gone.
        let dir = tempfile::tempdir().unwrap();
        let wav = dir.path().join("x.wav");
        std::fs::write(sidecar_path(&wav), b"typed").unwrap();
        write_plan(&wav, &MeetingPlan::default()).unwrap();

        remove_plan(&wav);
        assert!(sidecar_path(&wav).exists());
        assert!(!plan_path(&wav).exists());
        remove_plan(&wav);
    }
}

#[cfg(all(test, feature = "tauri-test"))]
#[allow(clippy::unwrap_used)]
mod ipc_tests {
    use super::*;
    use serde_json::json;
    use tauri::WebviewWindowBuilder;

    fn invoke(
        window: &WebviewWindow<tauri::test::MockRuntime>,
        command: &str,
        body: serde_json::Value,
    ) -> Result<serde_json::Value, serde_json::Value> {
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

    #[test]
    fn notes_commands_refuse_the_main_window_at_the_ipc_boundary() {
        let draft = Arc::new(NotesDraft::default());
        let dir = tempfile::tempdir().unwrap();
        draft.begin(&dir.path().join("x.wav"));
        let app = tauri::test::mock_builder()
            .manage(Arc::clone(&draft))
            .invoke_handler(tauri::generate_handler![
                meeting_notes_update,
                meeting_notes_get
            ])
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .unwrap();
        let main = WebviewWindowBuilder::new(&app, "main", Default::default())
            .build()
            .unwrap();
        let meeting = WebviewWindowBuilder::new(&app, "meeting", Default::default())
            .build()
            .unwrap();
        assert!(invoke(&main, "meeting_notes_update", json!({"markdown": "typed"})).is_err());
        assert!(invoke(
            &meeting,
            "meeting_notes_update",
            json!({"markdown": "typed"})
        )
        .is_ok());
        assert_eq!(draft.text(), "typed");
        assert!(invoke(&main, "meeting_notes_get", json!({})).is_err());
    }
}
