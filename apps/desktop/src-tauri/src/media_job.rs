//! App-owned import progress; renderer navigation cannot lose the operation.
use crate::media::MediaImportResult;
use serde::Serialize;
use std::sync::Mutex;
use std::time::Instant;
use ts_rs::TS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum ImportStage {
    Preparing,
    Converting,
    Transcribing,
    Saving,
    Succeeded,
    Failed,
    Cancelled,
    Interrupted,
}
impl ImportStage {
    fn active(self) -> bool {
        !matches!(
            self,
            Self::Succeeded | Self::Failed | Self::Cancelled | Self::Interrupted
        )
    }
}
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ImportSnapshot {
    pub operation_id: String,
    pub stage: ImportStage,
    pub elapsed_seconds: f64,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub result: Option<MediaImportResult>,
    pub error: Option<String>,
}
struct Entry {
    snapshot: ImportSnapshot,
    started: Instant,
    cancel: tokio::sync::watch::Sender<bool>,
}
#[derive(Default, Clone)]
pub struct ImportJobs(
    std::sync::Arc<Mutex<Option<Entry>>>,
    std::sync::Arc<std::sync::atomic::AtomicBool>,
);
impl ImportJobs {
    /// Called once during startup, after durable jobs have been reconciled.
    pub async fn recover(&self, db: &kw_storage::Db) -> Result<(), kw_storage::StorageError> {
        let Some(job) = kw_storage::jobs::last_desktop(db.pool()).await? else {
            return Ok(());
        };
        // A committed note is already succeeded in the same transaction. Never
        // invite a retry for that job, or resurrect an older interrupted import.
        if job.stage != "failed"
            || job.error.as_deref() != Some(kw_storage::jobs::INTERRUPTED_ERROR)
        {
            return Ok(());
        }
        let mut held = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if held.is_none() {
            let (cancel, _) = tokio::sync::watch::channel(false);
            *held = Some(Entry {
                started: Instant::now(),
                cancel,
                snapshot: ImportSnapshot {
                    operation_id: job.id,
                    stage: ImportStage::Interrupted,
                    elapsed_seconds: 0.0,
                    provider: job.provider,
                    model: job.model,
                    result: None,
                    error: Some("The app closed before your import finished. Choose the audio or video file again, then select Import and transcribe to retry.".into()),
                },
            });
        }
        Ok(())
    }
    pub fn close(&self) {
        let held = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        self.1.store(true, std::sync::atomic::Ordering::SeqCst);
        if let Some(entry) = held.as_ref() {
            if entry.snapshot.stage.active() && entry.snapshot.stage != ImportStage::Saving {
                let _ = entry.cancel.send(true);
            }
        }
    }
    pub fn snapshot(&self) -> Option<ImportSnapshot> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_ref()
            .map(|entry| {
                let mut snapshot = entry.snapshot.clone();
                if snapshot.stage.active() {
                    snapshot.elapsed_seconds = entry.started.elapsed().as_secs_f64();
                }
                snapshot
            })
    }
    pub fn begin(&self) -> Result<ImportLease, String> {
        let mut held = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if self.1.load(std::sync::atomic::Ordering::SeqCst) {
            return Err("The app is shutting down.".into());
        }
        if held.as_ref().is_some_and(|e| e.snapshot.stage.active()) {
            return Err(
                "Another media import is running. Wait for it to finish before starting another."
                    .into(),
            );
        }
        let id = uuid::Uuid::new_v4().to_string();
        let (cancel, cancel_rx) = tokio::sync::watch::channel(false);
        *held = Some(Entry {
            cancel,
            started: Instant::now(),
            snapshot: ImportSnapshot {
                operation_id: id.clone(),
                stage: ImportStage::Preparing,
                elapsed_seconds: 0.0,
                provider: None,
                model: None,
                result: None,
                error: None,
            },
        });
        tracing::info!(operation_id = %id, stage = ?ImportStage::Preparing, "media import stage");
        Ok(ImportLease {
            jobs: self.clone(),
            id,
            finished: false,
            cancel: cancel_rx,
        })
    }
    pub fn cancel(&self, id: &str) -> Result<(), String> {
        let held = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let entry = held
            .as_ref()
            .filter(|e| e.snapshot.operation_id == id)
            .ok_or("job is no longer active")?;
        if entry.snapshot.stage == ImportStage::Cancelled {
            return Ok(());
        }
        if !entry.snapshot.stage.active() || entry.snapshot.stage == ImportStage::Saving {
            return Err("job is saving or already finished".into());
        }
        let _ = entry.cancel.send(true);
        Ok(())
    }
}
pub struct ImportLease {
    jobs: ImportJobs,
    id: String,
    finished: bool,
    cancel: tokio::sync::watch::Receiver<bool>,
}
impl ImportLease {
    pub fn id(&self) -> &str {
        &self.id
    }
    pub fn is_cancelled(&self) -> bool {
        *self.cancel.borrow()
    }
    pub async fn cancelled(&self) {
        let mut cancel = self.cancel.clone();
        let _ = cancel.wait_for(|v| *v).await;
    }
    /// Cancellation and entry into the commit phase share the same lock.
    pub fn begin_saving(&self) -> Result<(), String> {
        let mut held = self
            .jobs
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let entry = held
            .as_mut()
            .filter(|e| e.snapshot.operation_id == self.id)
            .ok_or("job disappeared")?;
        if *entry.cancel.borrow() {
            return Err("import cancelled".into());
        }
        entry.snapshot.stage = ImportStage::Saving;
        Ok(())
    }
    pub fn finish_cancelled(mut self) {
        self.update(|s| {
            s.stage = ImportStage::Cancelled;
            s.error = None;
        });
        self.finished = true;
    }
    fn update(&self, f: impl FnOnce(&mut ImportSnapshot)) {
        let mut held = self
            .jobs
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(entry) = held.as_mut().filter(|e| e.snapshot.operation_id == self.id) {
            entry.snapshot.elapsed_seconds = entry.started.elapsed().as_secs_f64();
            f(&mut entry.snapshot);
        }
    }
    pub fn route(&self, provider: String, model: Option<String>) {
        self.update(|s| {
            s.provider = Some(provider);
            s.model = model;
        });
    }
    pub fn stage(&self, stage: ImportStage) {
        self.update(|s| s.stage = stage);
        tracing::info!(operation_id = %self.id, ?stage, "media import stage");
    }
    pub fn finish(mut self, result: &Result<MediaImportResult, String>) {
        self.update(|s| match result {
            Ok(r) => {
                s.stage = ImportStage::Succeeded;
                s.result = Some(r.clone());
            }
            Err(e) => {
                s.stage = ImportStage::Failed;
                s.error = Some(e.clone());
            }
        });
        if let Some(s) = self.jobs.snapshot() {
            tracing::info!(operation_id = %self.id, stage = ?s.stage, elapsed_seconds = s.elapsed_seconds, "media import finished");
        }
        self.finished = true;
    }
}
impl Drop for ImportLease {
    fn drop(&mut self) {
        if !self.finished {
            self.update(|s| {
                s.stage = ImportStage::Failed;
                s.error = Some("The import was interrupted. Please retry.".into());
            });
            tracing::warn!(operation_id = %self.id, "media import interrupted");
        }
    }
}
#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn restart_recovers_desktop_interruption_until_a_new_desktop_attempt() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("imports.db");
        let db = kw_storage::Db::open(&path).await.unwrap();
        kw_storage::jobs::create_desktop(db.pool(), "desktop")
            .await
            .unwrap();
        kw_storage::jobs::stage(db.pool(), "desktop", "transcribing", None)
            .await
            .unwrap();
        kw_storage::jobs::route(db.pool(), "desktop", "local", Some("tiny"))
            .await
            .unwrap();
        // A newer API upload must not hide the desktop's interrupted job.
        kw_storage::jobs::create(db.pool(), "api").await.unwrap();
        db.close().await;
        for _ in 0..2 {
            let db = kw_storage::Db::open(&path).await.unwrap();
            kw_storage::jobs::interrupt_unfinished(db.pool())
                .await
                .unwrap();
            let jobs = ImportJobs::default();
            jobs.recover(&db).await.unwrap();
            let snapshot = jobs.snapshot().unwrap();
            assert_eq!(snapshot.operation_id, "desktop");
            assert_eq!(snapshot.stage, ImportStage::Interrupted);
            assert!(!snapshot.stage.active());
            assert!(snapshot.result.is_none());
            assert_eq!(snapshot.model.as_deref(), Some("tiny"));
            assert!(snapshot
                .error
                .unwrap()
                .contains("Choose the audio or video file again"));
            assert!(jobs.cancel("desktop").is_err());
            // Recovered state does not block a fresh attempt or hang shutdown.
            let lease = jobs.begin().unwrap();
            assert_ne!(lease.id(), "desktop");
            lease.finish_cancelled();
            jobs.close();
            db.close().await;
        }
        let db = kw_storage::Db::open(&path).await.unwrap();
        for stage in ["succeeded", "failed", "cancelled"] {
            kw_storage::jobs::create_desktop(db.pool(), stage)
                .await
                .unwrap();
            kw_storage::jobs::stage(db.pool(), stage, stage, None)
                .await
                .unwrap();
            kw_storage::jobs::interrupt_unfinished(db.pool())
                .await
                .unwrap();
            let jobs = ImportJobs::default();
            jobs.recover(&db).await.unwrap();
            assert!(
                jobs.snapshot().is_none(),
                "must not resurrect the older interruption after {stage}"
            );
        }
        db.close().await;
    }

    #[tokio::test]
    async fn recovery_ignores_automation_and_preserves_an_existing_live_snapshot() {
        let db = kw_storage::Db::open_in_memory().await.unwrap();
        kw_storage::jobs::create(db.pool(), "api").await.unwrap();
        kw_storage::jobs::interrupt_unfinished(db.pool())
            .await
            .unwrap();
        let jobs = ImportJobs::default();
        jobs.recover(&db).await.unwrap();
        assert!(jobs.snapshot().is_none());
        kw_storage::jobs::create_desktop(db.pool(), "desktop")
            .await
            .unwrap();
        kw_storage::jobs::interrupt_unfinished(db.pool())
            .await
            .unwrap();
        let lease = jobs.begin().unwrap();
        jobs.recover(&db).await.unwrap();
        assert_eq!(jobs.snapshot().unwrap().operation_id, lease.id());
        drop(lease);
    }
    #[tokio::test]
    async fn cancellation_and_saving_are_mutually_exclusive_and_release_slot() {
        let jobs = ImportJobs::default();
        let lease = jobs.begin().unwrap();
        jobs.cancel(lease.id()).unwrap();
        lease.cancelled().await;
        assert!(lease.begin_saving().is_err());
        lease.finish_cancelled();
        let lease = jobs.begin().unwrap();
        lease.begin_saving().unwrap();
        assert!(jobs.cancel(lease.id()).is_err());
        lease.finish(&Err("fixture save failure".into()));
        assert!(jobs.begin().is_ok());
    }

    #[test]
    fn shutdown_refuses_new_jobs_but_does_not_cancel_a_commit() {
        let jobs = ImportJobs::default();
        let lease = jobs.begin().unwrap();
        lease.begin_saving().unwrap();
        jobs.close();
        assert!(!lease.is_cancelled());
        assert!(jobs.begin().is_err());
        drop(lease);
        assert!(jobs.begin().is_err());
    }
    #[test]
    fn busy_failure_retry_and_terminal_snapshot() {
        let jobs = ImportJobs::default();
        let lease = jobs.begin().unwrap();
        let first = jobs.snapshot().unwrap().operation_id;
        assert!(jobs.begin().is_err());
        for stage in [
            ImportStage::Converting,
            ImportStage::Transcribing,
            ImportStage::Saving,
        ] {
            lease.stage(stage);
            assert_eq!(jobs.snapshot().unwrap().stage, stage);
        }
        lease.finish(&Err("test database failure".into()));
        let snapshot = jobs.snapshot().unwrap();
        assert_eq!(snapshot.stage, ImportStage::Failed);
        assert_eq!(snapshot.error.as_deref(), Some("test database failure"));
        assert_eq!(
            jobs.snapshot().unwrap().elapsed_seconds,
            snapshot.elapsed_seconds
        );
        let next = jobs.begin().unwrap();
        assert_ne!(jobs.snapshot().unwrap().operation_id, first);
        drop(next);
        assert_eq!(jobs.snapshot().unwrap().stage, ImportStage::Failed);
        assert!(jobs.begin().is_ok());
    }
    #[test]
    fn concurrent_attempt_is_rejected() {
        let jobs = ImportJobs::default();
        let lease = jobs.begin().unwrap();
        std::thread::scope(|scope| {
            scope
                .spawn(|| assert!(jobs.begin().is_err()))
                .join()
                .unwrap();
        });
        drop(lease);
        assert!(jobs.begin().is_ok());
    }
}
