//! Active speech installations survive renderer navigation; no persistent job data.
use crate::local_model::InstallProgress;
use serde::Serialize;
use std::collections::BTreeMap;
use std::sync::{Mutex, OnceLock};
use tauri::{AppHandle, Emitter};
use ts_rs::TS;

const EVENT: &str = "speech-installations";
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SpeechInstall {
    pub id: String,
    pub progress: Option<InstallProgress>,
}
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SpeechInstallSnapshot {
    pub revision: u32,
    pub jobs: Vec<SpeechInstall>,
}
#[derive(Default)]
struct Active {
    revision: u32,
    jobs: BTreeMap<String, SpeechInstall>,
}
impl Active {
    fn snapshot(&self) -> SpeechInstallSnapshot {
        SpeechInstallSnapshot {
            revision: self.revision,
            jobs: self.jobs.values().cloned().collect(),
        }
    }
    fn changed(&mut self) -> SpeechInstallSnapshot {
        self.revision = self.revision.wrapping_add(1);
        self.snapshot()
    }
}
fn active() -> &'static Mutex<Active> {
    static ACTIVE: OnceLock<Mutex<Active>> = OnceLock::new();
    ACTIVE.get_or_init(Mutex::default)
}
pub struct Installing {
    app: AppHandle,
    id: String,
}
impl Installing {
    pub fn begin(app: &AppHandle, id: &str) -> Result<Self, String> {
        let mut jobs = active()
            .lock()
            .map_err(|_| "Install progress is unavailable.")?;
        if jobs.jobs.contains_key(id) {
            return Err("This speech model is already being installed.".into());
        }
        jobs.jobs.insert(
            id.to_owned(),
            SpeechInstall {
                id: id.to_owned(),
                progress: None,
            },
        );
        let snapshot = jobs.changed();
        drop(jobs);
        let _ = app.emit(EVENT, snapshot);
        Ok(Self {
            app: app.clone(),
            id: id.to_owned(),
        })
    }
    pub fn update(&self, progress: &InstallProgress) {
        if let Ok(mut jobs) = active().lock() {
            if let Some(job) = jobs.jobs.get_mut(&self.id) {
                job.progress = Some(progress.clone());
            }
            let snapshot = jobs.changed();
            drop(jobs);
            let _ = self.app.emit(EVENT, snapshot);
        }
    }
}
impl Drop for Installing {
    fn drop(&mut self) {
        if let Ok(mut jobs) = active().lock() {
            jobs.jobs.remove(&self.id);
            let snapshot = jobs.changed();
            drop(jobs);
            let _ = self.app.emit(EVENT, snapshot);
        }
    }
}
#[tauri::command]
pub fn speech_installations() -> Result<SpeechInstallSnapshot, String> {
    Ok(active()
        .lock()
        .map_err(|_| "Install progress is unavailable.")?
        .snapshot())
}
