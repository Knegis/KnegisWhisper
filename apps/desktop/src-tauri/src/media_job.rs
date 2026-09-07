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
}
impl ImportStage {
    fn active(self) -> bool {
        !matches!(self, Self::Succeeded | Self::Failed)
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
}
#[derive(Default)]
pub struct ImportJobs(Mutex<Option<Entry>>);
impl ImportJobs {
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
    pub fn begin(&self) -> Result<ImportLease<'_>, String> {
        let mut held = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if held.as_ref().is_some_and(|e| e.snapshot.stage.active()) {
            return Err(
                "Another media import is running. Wait for it to finish before starting another."
                    .into(),
            );
        }
        let id = uuid::Uuid::new_v4().to_string();
        *held = Some(Entry {
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
            jobs: self,
            id,
            finished: false,
        })
    }
}
pub struct ImportLease<'a> {
    jobs: &'a ImportJobs,
    id: String,
    finished: bool,
}
impl ImportLease<'_> {
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
impl Drop for ImportLease<'_> {
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
