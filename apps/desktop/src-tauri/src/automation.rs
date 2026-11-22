//! Adapter from authenticated automation to the same managed desktop runtimes.
use kw_local_api::{ApiError, ErrorCode};
use serde_json::{json, Value};
use std::{path::Path, sync::Arc};
use tauri::Manager as _;

static EXIT_STARTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
pub fn on_event(app: &tauri::AppHandle, event: tauri::RunEvent) {
    if let tauri::RunEvent::ExitRequested { api, .. } = event {
        if EXIT_STARTED.swap(true, std::sync::atomic::Ordering::SeqCst) {
            return;
        }
        api.prevent_exit();
        #[cfg(target_os = "linux")]
        app.state::<Arc<crate::shortcuts::Hooks>>().stop_portal();
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            let cleanup = async {
                let media = app.state::<Arc<crate::media::MediaImportRuntime>>();
                let meetings = app.state::<Arc<crate::meeting::MeetingRuntime>>();
                let api = app.state::<Arc<crate::local_api::LocalApiRuntime>>();
                // Not `cancel`, which is the person's own Discard and deletes
                // the recording. An exit is nobody's decision to discard.
                let _ = tokio::join!(api.stop(), media.shutdown(), meetings.end_for_exit());
            };
            if tokio::time::timeout(std::time::Duration::from_secs(10), cleanup)
                .await
                .is_err()
            {
                tracing::warn!(
                    "shutdown timed out; unfinished imports will be marked interrupted at restart"
                );
            }
            app.exit(0);
        });
    }
}

pub struct DesktopRuntime {
    app: tauri::AppHandle,
    recording: tokio::sync::Mutex<()>,
}
impl DesktopRuntime {
    pub fn new(app: tauri::AppHandle) -> Self {
        Self {
            app,
            recording: tokio::sync::Mutex::new(()),
        }
    }
}
fn value<T: serde::Serialize>(input: T) -> Result<Value, ApiError> {
    serde_json::to_value(input).map_err(|_| ApiError::internal())
}
fn unavailable(_: String) -> ApiError {
    ApiError::new(
        ErrorCode::Unavailable,
        "operation unavailable; check the desktop provider and device settings",
    )
}
/// Only remove generated upload files, never recursively follow a supplied path.
pub fn clean_uploads(directory: &Path) -> std::io::Result<()> {
    if !directory.exists() {
        return Ok(());
    }
    for entry in std::fs::read_dir(directory)? {
        let entry = entry?;
        if entry.file_type()?.is_file()
            && entry
                .path()
                .file_stem()
                .and_then(|s| s.to_str())
                .is_some_and(|s| uuid::Uuid::parse_str(s).is_ok())
        {
            std::fs::remove_file(entry.path())?;
        }
    }
    Ok(())
}

/// Recover decoded scratch audio after a process crash, using durable job IDs
/// as ownership evidence. Never recurse into an unknown directory or symlink.
pub async fn clean_job_scratch(db: &kw_storage::Db, directory: &Path) -> anyhow::Result<()> {
    if !directory.exists() {
        return Ok(());
    }
    for entry in std::fs::read_dir(directory)? {
        let entry = entry?;
        let id = entry.file_name().to_string_lossy().into_owned();
        if !entry.file_type()?.is_dir()
            || uuid::Uuid::parse_str(&id).is_err()
            || kw_storage::jobs::get(db.pool(), &id).await?.is_none()
        {
            continue;
        }
        let audio = entry.path().join("converted.wav");
        if let Ok(metadata) = std::fs::symlink_metadata(&audio) {
            if metadata.file_type().is_file() {
                std::fs::remove_file(&audio)?;
            }
        }
        // Empty-directory removal deliberately preserves unexpected contents.
        let _ = std::fs::remove_dir(entry.path());
    }
    Ok(())
}

#[async_trait::async_trait]
impl kw_local_api::runtime::Runtime for DesktopRuntime {
    async fn transcribe(&self, upload: kw_local_api::runtime::Upload) -> Result<Value, ApiError> {
        self.app
            .state::<Arc<crate::media::MediaImportRuntime>>()
            .inner()
            .submit_upload(self.app.clone(), upload)
            .await
    }
    async fn cancel(&self, id: &str) -> Result<Value, ApiError> {
        self.app
            .state::<Arc<crate::media::MediaImportRuntime>>()
            .cancel_job(id)
            .await
    }
    async fn recording_start(&self, provider: &str, system_audio: bool) -> Result<Value, ApiError> {
        if EXIT_STARTED.load(std::sync::atomic::Ordering::SeqCst) {
            return Err(ApiError::new(
                ErrorCode::Unavailable,
                "the app is shutting down",
            ));
        }
        let _operation = self
            .recording
            .try_lock()
            .map_err(|_| ApiError::new(ErrorCode::Conflict, "recording operation in progress"))?;
        let runtime = self.app.state::<Arc<crate::meeting::MeetingRuntime>>();
        let status = value(runtime.status().await)?;
        if status
            .get("state")
            .and_then(Value::as_str)
            .is_some_and(|s| s != "idle")
        {
            return Err(ApiError::new(
                ErrorCode::Conflict,
                "a meeting is already active",
            ));
        }
        value(
            runtime
                // The API has no title or folder input; resolve its calendar
                // snapshot automatically, exactly as the desktop Start does.
                .start(
                    provider,
                    system_audio,
                    crate::meeting::MeetingPlan::default(),
                    crate::meeting::CalendarLinkChoice::Auto,
                )
                .await
                .map_err(unavailable)?,
        )
    }
    async fn recording_stop(&self) -> Result<Value, ApiError> {
        let _operation = self
            .recording
            .try_lock()
            .map_err(|_| ApiError::new(ErrorCode::Conflict, "recording operation in progress"))?;
        value(
            self.app
                .state::<Arc<crate::meeting::MeetingRuntime>>()
                .stop()
                .await
                .map_err(|_| {
                    ApiError::new(
                        ErrorCode::Conflict,
                        "meeting could not be stopped; inspect desktop status",
                    )
                })?,
        )
    }
    async fn status(&self) -> Result<Value, ApiError> {
        let route = crate::dictation::dictation_route(self.app.state())
            .await
            .map_err(unavailable)?;
        let media = self.app.state::<Arc<crate::media::MediaImportRuntime>>();
        let snapshot = media.snapshot();
        // Expose metadata only, not snapshot.result (which includes transcript text).
        let job = if let Some(s) = snapshot {
            kw_storage::jobs::get(
                self.app.state::<crate::state::AppState>().db().pool(),
                &s.operation_id,
            )
            .await
            .map_err(|_| ApiError::internal())?
        } else {
            None
        };
        let meeting = meeting_status_for_api(value(
            self.app
                .state::<Arc<crate::meeting::MeetingRuntime>>()
                .status()
                .await,
        )?);
        Ok(json!({
            "dictation": self.app.state::<Arc<crate::dictation::DictationRuntime>>().state(),
            "meeting": meeting,
            "route": route,
            "import_job": job,
        }))
    }
    async fn models(&self) -> Result<Value, ApiError> {
        Ok(json!({
            "providers": crate::commands::providers(self.app.state()).map_err(unavailable)?,
            "whisper": crate::local_model::local_model_status(self.app.state()).await.map_err(unavailable)?,
            "sherpa": crate::local_asr::sherpa_status(self.app.state(), self.app.state()).await.map_err(unavailable)?,
            "meeting_providers": crate::meeting::meeting_providers(self.app.state()).await.map_err(unavailable)?,
            "connectivity_verified": false,
        }))
    }
}

/// Calendar linking extends desktop IPC, not the local API's response schema.
/// Its status used to forward all meeting fields, so explicitly leave the new
/// session controls and event snapshot out rather than silently extending it.
fn meeting_status_for_api(mut status: Value) -> Value {
    if let Some(object) = status.as_object_mut() {
        for key in ["sessionId", "calendarNotice", "calendarChoices"] {
            object.remove(key);
        }
        if let Some(plan) = object.get_mut("plan").and_then(Value::as_object_mut) {
            plan.remove("calendar");
        }
    }
    status
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    #[test]
    #[allow(clippy::indexing_slicing)]
    fn api_meeting_status_preserves_its_shape_without_calendar_controls_or_snapshot() {
        let before = serde_json::json!({
            "state": "recording", "provider": "local",
            "plan": {"title": "Meeting", "folderId": null, "deleteRecording": false, "liveProviderId": "local"}
        });
        let mut desktop = before.clone();
        desktop["sessionId"] = serde_json::json!("20261111-120000-fixture");
        desktop["calendarNotice"] = serde_json::json!(null);
        desktop["calendarChoices"] = serde_json::json!([]);
        desktop["plan"]["calendar"] = serde_json::json!({"eventId": "fixture", "participants": []});
        assert_eq!(super::meeting_status_for_api(desktop), before);
        let idle = serde_json::json!({"state": "idle", "plan": null});
        assert_eq!(super::meeting_status_for_api(idle.clone()), idle);
    }

    #[tokio::test]
    async fn crash_cleanup_removes_only_audio_owned_by_a_durable_job() {
        let directory = tempfile::tempdir().unwrap();
        let db = kw_storage::Db::open_in_memory().await.unwrap();
        let known = uuid::Uuid::new_v4().to_string();
        let unknown = uuid::Uuid::new_v4().to_string();
        kw_storage::jobs::create(db.pool(), &known).await.unwrap();
        for id in [&known, &unknown] {
            std::fs::create_dir(directory.path().join(id)).unwrap();
            std::fs::write(directory.path().join(id).join("converted.wav"), b"fixture").unwrap();
        }
        super::clean_job_scratch(&db, directory.path())
            .await
            .unwrap();
        assert!(!directory.path().join(&known).exists());
        assert!(directory
            .path()
            .join(&unknown)
            .join("converted.wav")
            .exists());
    }
}
