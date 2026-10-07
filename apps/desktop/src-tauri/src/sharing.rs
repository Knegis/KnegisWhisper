//! Native dialog paths stay here. Operations survive renderer navigation.
#[cfg(all(test, windows, feature = "tauri-test"))]
#[path = "sharing_native_test.rs"]
mod native_tests;
use crate::state::AppState;
use kw_notes_core::sharing::{self, ShareOptions, SharePreview, StagedFolder};
use serde::Serialize;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use tauri::{Runtime, State, WebviewWindow};
use tauri_plugin_dialog::DialogExt;
use ts_rs::TS;

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ShareStatus {
    pub operation_id: String,
    pub direction: String,
    pub stage: String,
    pub bytes_processed: u64,
    pub preview: Option<SharePreview>,
    pub folder_id: Option<i64>,
    pub saved_to: Option<String>,
    pub error: Option<String>,
}
struct Entry {
    status: ShareStatus,
    cancel: Arc<AtomicBool>,
    staged: Option<StagedFolder>,
}
#[derive(Default, Clone)]
pub struct SharingRuntime {
    entry: Arc<Mutex<Option<Entry>>>,
    closed: Arc<AtomicBool>,
}
fn active(stage: &str) -> bool {
    !matches!(stage, "succeeded" | "failed" | "cancelled" | "interrupted")
}
fn main_window<R: Runtime>(window: &WebviewWindow<R>) -> Result<(), String> {
    if window.label() == "main" {
        Ok(())
    } else {
        Err("Folder sharing is available only in the main window.".into())
    }
}
impl SharingRuntime {
    pub async fn recover(state: &AppState) -> Result<Self, String> {
        sharing::recover(state.db().pool(), state.recording_directory()).await?;
        let runtime = Self::default();
        let previous = sharing::last_receipt(state.db().pool()).await?;
        if let Some(receipt) =
            previous.filter(|r| r.stage == "interrupted" || r.stage == "succeeded")
        {
            *runtime
                .entry
                .lock()
                .map_err(|_| "Could not read sharing state.")? = Some(Entry {
                status: ShareStatus {
                    operation_id: receipt.id,
                    direction: receipt.direction,
                    stage: receipt.stage,
                    folder_id: receipt.result_folder_id,
                    saved_to: receipt.result_path,
                    error: receipt.error,
                    bytes_processed: 0,
                    preview: None,
                },
                cancel: Arc::new(AtomicBool::new(false)),
                staged: None,
            });
        }
        Ok(runtime)
    }
    fn begin(&self, direction: &str) -> Result<(String, Arc<AtomicBool>), String> {
        let mut held = self
            .entry
            .lock()
            .map_err(|_| "Could not read sharing state.")?;
        if self.closed.load(Ordering::Relaxed) {
            return Err("The app is closing.".into());
        }
        if held.as_ref().is_some_and(|e| active(&e.status.stage)) {
            return Err("Another sharing operation is running. Finish or cancel it first.".into());
        }
        let id = uuid::Uuid::new_v4().to_string();
        let cancel = Arc::new(AtomicBool::new(false));
        *held = Some(Entry {
            status: ShareStatus {
                operation_id: id.clone(),
                direction: direction.into(),
                stage: "preparing".into(),
                bytes_processed: 0,
                preview: None,
                folder_id: None,
                saved_to: None,
                error: None,
            },
            cancel: cancel.clone(),
            staged: None,
        });
        Ok((id, cancel))
    }
    fn change(&self, id: &str, update: impl FnOnce(&mut Entry)) {
        if let Ok(mut held) = self.entry.lock() {
            if let Some(entry) = held.as_mut().filter(|e| e.status.operation_id == id) {
                update(entry);
            }
        }
    }
    fn progress(&self, id: &str, bytes: u64) {
        self.change(id, |e| e.status.bytes_processed += bytes);
    }
    fn stage(&self, id: &str, stage: &str) {
        self.change(id, |e| e.status.stage = stage.into());
    }
    async fn failed(&self, state: &AppState, id: &str, cancel: &AtomicBool, error: String) {
        let stage = if cancel.load(Ordering::Relaxed) {
            "cancelled"
        } else {
            "failed"
        };
        if let Err(e) = sharing::cleanup(state.db().pool(), state.recording_directory(), id).await {
            tracing::error!(%e,"sharing cleanup failed; startup will retry");
        }
        let _ = sharing::journal_stage(state.db().pool(), id, stage, Some(&error)).await;
        self.change(id, |e| {
            e.status.stage = stage.into();
            e.status.error = if stage == "cancelled" {
                None
            } else {
                Some(error)
            };
            e.staged = None;
        });
    }
    pub fn close(&self) {
        self.closed.store(true, Ordering::Relaxed);
        if let Ok(held) = self.entry.lock() {
            if let Some(e) = held.as_ref().filter(|e| e.status.stage != "saving") {
                e.cancel.store(true, Ordering::Relaxed);
            }
        }
    }
    async fn export(
        &self,
        window: WebviewWindow,
        state: AppState,
        id: String,
        cancel: Arc<AtomicBool>,
        selection: (i64, Vec<i64>, ShareOptions),
    ) -> Result<(), String> {
        let snapshot = sharing::capture(
            state.db().pool(),
            state.recording_directory(),
            selection.0,
            &selection.1,
            selection.2,
        )
        .await?;
        let filename = kw_notes_core::document::filename(&snapshot.folder_name, "knegis");
        let (send, receive) = tokio::sync::oneshot::channel();
        window
            .dialog()
            .file()
            .set_parent(&window)
            .set_title("Share folder")
            .set_file_name(filename)
            .add_filter("KnegisWhisper folder", &["knegis"])
            .save_file(move |chosen| {
                let _ = send.send(chosen);
            });
        let Some(chosen) = receive.await.map_err(|e| e.to_string())? else {
            cancel.store(true, Ordering::Relaxed);
            return Err("Sharing cancelled.".into());
        };
        if cancel.load(Ordering::Relaxed) {
            return Err("Sharing cancelled.".into());
        }
        let destination = chosen.into_path().map_err(|e| e.to_string())?;
        let parent = destination.parent().ok_or("Choose a destination folder.")?;
        let temporary = tempfile::Builder::new()
            .prefix(&format!(".knegis-share-{id}-"))
            .tempfile_in(parent)
            .map_err(|e| e.to_string())?;
        sharing::journal(state.db().pool(), &id, "export", temporary.path()).await?;
        sharing::journal_stage(state.db().pool(), &id, "copying", None).await?;
        self.stage(&id, "copying");
        let runtime = self.clone();
        let job = id.clone();
        let stop = cancel.clone();
        let temporary = tauri::async_runtime::spawn_blocking(move || {
            let mut temporary = temporary;
            sharing::write_package(temporary.as_file_mut(), snapshot, &stop, |bytes| {
                runtime.progress(&job, bytes)
            })?;
            let digest = sharing::package_digest(temporary.path(), &stop)?;
            Ok::<_, String>((temporary, digest))
        })
        .await
        .map_err(|e| e.to_string())??;
        if cancel.load(Ordering::Relaxed) {
            return Err("Sharing cancelled.".into());
        }
        self.stage(&id, "saving");
        let (temporary, digest) = temporary;
        sharing::publishing(state.db().pool(), &id, &destination, &digest).await?;
        // tempfile's persist atomically replaces the selected destination, after
        // the native save dialog has obtained overwrite confirmation.
        temporary.persist(&destination).map_err(|e| e.to_string())?;
        sharing::journal_stage(state.db().pool(), &id, "succeeded", None).await?;
        self.change(&id, |e| {
            e.status.stage = "succeeded".into();
            e.status.saved_to = Some(destination.to_string_lossy().into_owned());
        });
        Ok(())
    }
    async fn pick_import(
        &self,
        window: WebviewWindow,
        state: AppState,
        id: String,
        cancel: Arc<AtomicBool>,
    ) -> Result<(), String> {
        let (send, receive) = tokio::sync::oneshot::channel();
        window
            .dialog()
            .file()
            .set_parent(&window)
            .set_title("Import shared folder")
            .add_filter("KnegisWhisper folder", &["knegis"])
            .pick_file(move |chosen| {
                let _ = send.send(chosen);
            });
        let Some(chosen) = receive.await.map_err(|e| e.to_string())? else {
            cancel.store(true, Ordering::Relaxed);
            return Err("Sharing cancelled.".into());
        };
        let source = chosen.into_path().map_err(|e| e.to_string())?;
        let directory = state.recording_directory().join(".sharing").join(&id);
        sharing::journal(state.db().pool(), &id, "import", &directory).await?;
        self.stage(&id, "checking");
        let runtime = self.clone();
        let job = id.clone();
        let stop = cancel.clone();
        let staged = tauri::async_runtime::spawn_blocking(move || {
            sharing::stage_package(&source, &directory, &stop, |bytes| {
                runtime.progress(&job, bytes)
            })
        })
        .await
        .map_err(|e| e.to_string())??;
        if cancel.load(Ordering::Relaxed) {
            return Err("Sharing cancelled.".into());
        }
        sharing::journal_stage(state.db().pool(), &id, "awaiting-confirmation", None).await?;
        self.change(&id, |e| {
            e.status.stage = "awaiting-confirmation".into();
            e.status.preview = Some(staged.preview.clone());
            e.staged = Some(staged);
        });
        Ok(())
    }
}

#[tauri::command]
pub async fn folder_share_preview<R: Runtime>(
    window: WebviewWindow<R>,
    state: State<'_, AppState>,
    folder_id: i64,
) -> Result<SharePreview, String> {
    main_window(&window)?;
    sharing::preview(state.db().pool(), state.recording_directory(), folder_id).await
}
#[tauri::command]
pub fn folder_share_export(
    window: WebviewWindow,
    state: State<'_, AppState>,
    runtime: State<'_, SharingRuntime>,
    folder_id: i64,
    note_ids: Vec<i64>,
    options: ShareOptions,
) -> Result<String, String> {
    main_window(&window)?;
    let runtime = runtime.inner().clone();
    let state = state.inner().clone();
    let (id, cancel) = runtime.begin("export")?;
    let operation = id.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(e) = runtime
            .export(
                window,
                state.clone(),
                id.clone(),
                cancel.clone(),
                (folder_id, note_ids, options),
            )
            .await
        {
            runtime.failed(&state, &id, &cancel, e).await;
        }
    });
    Ok(operation)
}
#[tauri::command]
pub fn folder_share_pick_import(
    window: WebviewWindow,
    state: State<'_, AppState>,
    runtime: State<'_, SharingRuntime>,
) -> Result<String, String> {
    main_window(&window)?;
    let runtime = runtime.inner().clone();
    let state = state.inner().clone();
    let (id, cancel) = runtime.begin("import")?;
    let operation = id.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(e) = runtime
            .pick_import(window, state.clone(), id.clone(), cancel.clone())
            .await
        {
            runtime.failed(&state, &id, &cancel, e).await;
        }
    });
    Ok(operation)
}
#[tauri::command]
pub fn folder_share_status<R: Runtime>(
    window: WebviewWindow<R>,
    runtime: State<'_, SharingRuntime>,
) -> Result<Option<ShareStatus>, String> {
    main_window(&window)?;
    Ok(runtime
        .entry
        .lock()
        .map_err(|_| "Could not read sharing state.")?
        .as_ref()
        .map(|e| e.status.clone()))
}
#[tauri::command]
pub fn folder_share_import<R: Runtime>(
    window: WebviewWindow<R>,
    state: State<'_, AppState>,
    runtime: State<'_, SharingRuntime>,
    operation_id: String,
    folder_name: String,
) -> Result<(), String> {
    main_window(&window)?;
    if folder_name.trim().is_empty() || folder_name.len() > 1024 {
        return Err("Choose a folder name between 1 and 1,024 bytes.".into());
    }
    let (folder, cancel) = {
        let mut held = runtime
            .entry
            .lock()
            .map_err(|_| "Could not read sharing state.")?;
        let entry = held
            .as_mut()
            .filter(|e| {
                e.status.operation_id == operation_id
                    && e.status.stage == "awaiting-confirmation"
                    && !e.cancel.load(Ordering::Relaxed)
            })
            .ok_or("This import preview is no longer available.")?;
        let folder = entry
            .staged
            .take()
            .ok_or("This import preview is no longer available.")?;
        entry.status.stage = "saving".into();
        (folder, entry.cancel.clone())
    };
    let runtime = runtime.inner().clone();
    let state = state.inner().clone();
    tauri::async_runtime::spawn(async move {
        match sharing::restore(
            state.db().pool(),
            state.recording_directory(),
            &operation_id,
            folder,
            &folder_name,
            &cancel,
        )
        .await
        {
            Ok(folder_id) => {
                let _ = sharing::cleanup(
                    state.db().pool(),
                    state.recording_directory(),
                    &operation_id,
                )
                .await;
                runtime.change(&operation_id, |e| {
                    e.status.stage = "succeeded".into();
                    e.status.folder_id = Some(folder_id);
                });
            }
            Err(e) => runtime.failed(&state, &operation_id, &cancel, e).await,
        }
    });
    Ok(())
}
#[tauri::command]
pub async fn folder_share_cancel<R: Runtime>(
    window: WebviewWindow<R>,
    state: State<'_, AppState>,
    runtime: State<'_, SharingRuntime>,
    operation_id: String,
) -> Result<(), String> {
    main_window(&window)?;
    let preview = {
        let mut held = runtime
            .entry
            .lock()
            .map_err(|_| "Could not read sharing state.")?;
        let entry = held
            .as_mut()
            .filter(|e| e.status.operation_id == operation_id)
            .ok_or("This sharing operation no longer exists.")?;
        if entry.status.stage == "saving" {
            return Err("Sharing is finishing. Please wait.".into());
        }
        if !active(&entry.status.stage) {
            return Ok(());
        }
        entry.cancel.store(true, Ordering::Relaxed);
        if entry.status.stage == "awaiting-confirmation" {
            entry.status.stage = "cancelling".into();
            entry.staged = None;
            true
        } else {
            false
        }
    };
    if preview {
        sharing::cleanup(
            state.db().pool(),
            state.recording_directory(),
            &operation_id,
        )
        .await?;
        sharing::journal_stage(state.db().pool(), &operation_id, "cancelled", None).await?;
        runtime.stage(&operation_id, "cancelled");
    }
    Ok(())
}
#[tauri::command]
pub async fn folder_share_dismiss<R: Runtime>(
    window: WebviewWindow<R>,
    state: State<'_, AppState>,
    runtime: State<'_, SharingRuntime>,
    operation_id: String,
) -> Result<(), String> {
    main_window(&window)?;
    {
        let held = runtime
            .entry
            .lock()
            .map_err(|_| "Could not read sharing state.")?;
        if let Some(entry) = held.as_ref() {
            if entry.status.operation_id != operation_id || active(&entry.status.stage) {
                return Err("Finish or cancel this sharing operation first.".into());
            }
        }
    }
    sharing::journal_stage(state.db().pool(), &operation_id, "dismissed", None).await?;
    let mut held = runtime
        .entry
        .lock()
        .map_err(|_| "Could not read sharing state.")?;
    if held
        .as_ref()
        .is_some_and(|e| e.status.operation_id == operation_id)
    {
        *held = None;
    }
    Ok(())
}
#[tauri::command]
pub fn folder_share_show_export(
    window: WebviewWindow,
    runtime: State<'_, SharingRuntime>,
    operation_id: String,
) -> Result<(), String> {
    main_window(&window)?;
    let parent = {
        let held = runtime
            .entry
            .lock()
            .map_err(|_| "Could not read sharing state.")?;
        let entry = held
            .as_ref()
            .filter(|e| e.status.operation_id == operation_id && e.status.stage == "succeeded")
            .ok_or("This export is not available.")?;
        std::path::PathBuf::from(
            entry
                .status
                .saved_to
                .as_ref()
                .ok_or("This operation did not export a file.")?,
        )
        .parent()
        .ok_or("The destination folder is unavailable.")?
        .to_owned()
    };
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer.exe")
            .arg(parent)
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(parent)
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open")
            .arg(parent)
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod types {
    use super::*;
    #[test]
    fn sharing_ipc_types_are_current() {
        let config = ts_rs::Config::new().with_large_int("number");
        let rendered=format!("/** GENERATED by sharing::types::sharing_ipc_types_are_current. */\nexport {}\nexport {}\nexport {}\nexport {}\n",ShareOptions::decl(&config),sharing::ShareNotePreview::decl(&config),SharePreview::decl(&config),ShareStatus::decl(&config));
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../src/generated/sharing-types.ts");
        if std::env::var_os("KW_WRITE_IPC_TYPES").is_some() {
            std::fs::write(&path, &rendered).unwrap();
        }
        assert_eq!(std::fs::read_to_string(path).unwrap(), rendered);
    }
}

#[cfg(all(test, feature = "tauri-test"))]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
mod ipc_tests {
    use super::*;
    use serde_json::{json, Value};
    use tauri::Manager;
    fn invoke(
        window: &WebviewWindow<tauri::test::MockRuntime>,
        cmd: &str,
        body: Value,
    ) -> Result<Value, Value> {
        tauri::test::get_ipc_response(
            window,
            tauri::webview::InvokeRequest {
                cmd: cmd.into(),
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
    async fn sharing_ipc_enforces_window_and_operation_identity_and_imports_once() {
        let temp = tempfile::tempdir().unwrap();
        let recordings = temp.path().join("audio");
        let state = AppState::in_memory()
            .await
            .unwrap()
            .with_recording_directory(recordings.clone());
        let db = state.db().clone();
        let folder = kw_notes_core::notes::create_folder(db.pool(), "Original")
            .await
            .unwrap();
        let note = kw_notes_core::notes::create(
            db.pool(),
            &kw_notes_core::NewNote {
                title: "Shared note".into(),
                folder_id: Some(folder),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let snapshot = sharing::capture(
            db.pool(),
            &recordings,
            folder,
            &[note],
            ShareOptions::default(),
        )
        .await
        .unwrap();
        let source = temp.path().join("shared.knegis");
        sharing::write_package(
            &mut std::fs::File::create(&source).unwrap(),
            snapshot,
            &AtomicBool::new(false),
            |_| {},
        )
        .unwrap();
        let runtime = SharingRuntime::default();
        let (id, _) = runtime.begin("import").unwrap();
        let directory = recordings.join(".sharing").join(&id);
        sharing::journal(db.pool(), &id, "import", &directory)
            .await
            .unwrap();
        let staged =
            sharing::stage_package(&source, &directory, &AtomicBool::new(false), |_| {}).unwrap();
        runtime.change(&id, |e| {
            e.status.stage = "awaiting-confirmation".into();
            e.status.preview = Some(staged.preview.clone());
            e.staged = Some(staged);
        });
        let app = tauri::test::mock_builder()
            .manage(state)
            .manage(runtime.clone())
            .invoke_handler(tauri::generate_handler![
                folder_share_preview,
                folder_share_status,
                folder_share_import,
                folder_share_cancel,
                folder_share_dismiss
            ])
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .unwrap();
        let main = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
            .build()
            .unwrap();
        let overlay = tauri::WebviewWindowBuilder::new(&app, "overlay", Default::default())
            .build()
            .unwrap();
        assert!(invoke(&overlay, "folder_share_preview", json!({"folderId":folder})).is_err());
        assert!(invoke(&overlay, "folder_share_status", json!({})).is_err());
        assert!(invoke(
            &main,
            "folder_share_import",
            json!({"operationId":"foreign","folderName":"Imported"})
        )
        .is_err());
        assert!(invoke(
            &main,
            "folder_share_cancel",
            json!({"operationId":"foreign"})
        )
        .is_err());
        assert!(invoke(&main, "folder_share_dismiss", json!({"operationId":id})).is_err());
        invoke(
            &main,
            "folder_share_import",
            json!({"operationId":id,"folderName":"Imported"}),
        )
        .unwrap();
        assert!(invoke(
            &main,
            "folder_share_import",
            json!({"operationId":id,"folderName":"Again"})
        )
        .is_err());
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            let status = invoke(&main, "folder_share_status", json!({})).unwrap();
            if status["stage"] == "succeeded" {
                break;
            }
            assert!(std::time::Instant::now() < deadline, "{status}");
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        assert_eq!(
            kw_notes_core::notes::folders(db.pool())
                .await
                .unwrap()
                .len(),
            2
        );
        invoke(&main, "folder_share_dismiss", json!({"operationId":id})).unwrap();
        assert!(invoke(&main, "folder_share_status", json!({}))
            .unwrap()
            .is_null());
        assert!(SharingRuntime::recover(app.state::<AppState>().inner())
            .await
            .unwrap()
            .entry
            .lock()
            .unwrap()
            .is_none());
    }
}
