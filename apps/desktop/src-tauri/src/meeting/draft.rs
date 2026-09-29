//! Typed meeting notes live in memory and beside the WAV for crash durability.
//!
//! An interrupted meeting's `.notes.md` sidecar can be recovered from Settings
//! > Meetings with its orphan WAV. It is session data, not a settings key.

use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
use tauri::{Runtime, State, WebviewWindow};

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

/// Remove the sidecar after the notes are safely saved or discarded.
pub fn remove_sidecar(recording: &Path) {
    let sidecar = sidecar_path(recording);
    if let Err(error) = std::fs::remove_file(&sidecar) {
        if error.kind() != std::io::ErrorKind::NotFound {
            tracing::warn!(%error, path = %sidecar.display(), "could not remove meeting notes sidecar");
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
