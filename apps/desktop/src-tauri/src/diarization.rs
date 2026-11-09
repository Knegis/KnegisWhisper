//! Speaker labels on a saved meeting note.
//!
//! The thin half of [`kw_meeting_core::diarization`], which holds every rule
//! and every measurement. This spawns the process and owns the settings
//! surface; nothing here decides anything a test could not reach without a
//! process, which is the split `kw_transcription_core::sherpa` follows and the
//! reason `media.rs:1334-1340` gives for following it.
//!
//! ## What it costs, measured
//!
//! `sherpa-onnx-offline-speaker-diarization` reported a real-time factor of
//! **0.081** on this machine over a 56.9-second recording — 4.6 seconds of CPU.
//! Linear in the recording, so a one-hour meeting is roughly **five minutes**
//! added to *Stop and transcribe*, on top of the whole-recording transcription
//! pass that already runs there.
//!
//! That is why this is off by default and says its cost on the screen. It is
//! not a background nicety; it is a second pass over the whole recording, and a
//! person who turns it on should know they are choosing to wait.
//!
//! ## English is the measured case
//!
//! The segmentation model is language-independent; the **embedding model is
//! not**. Measured 2026-09-01: the release's Chinese four-speaker reference
//! came back as three voices through the pinned English CAM++ model and as four
//! through a language-matched one, same audio and same segmentation. Only the
//! English model is pinned, so a meeting in another language gets worse speaker
//! turns and the output does not say so. The screen says it instead.
//!
//! ## A label is never a name
//!
//! `Speaker 1` is a placeholder and is written into the note as one —
//! `speaker_is_placeholder: true`. Diarization clusters voices inside one
//! recording; it knows nothing about who they belong to, and the same person is
//! `Speaker 1` in one meeting and `Speaker 3` in the next. Nothing here reaches
//! for `contacts`.

use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;

use serde::Serialize;
use tauri::{AppHandle, Emitter, State};
use ts_rs::TS;

use kw_meeting_core::diarization::{self, Turn};
use kw_meeting_core::diarization_models;

/// Whether a saved meeting note gets speaker labels.
pub const DIARIZE_KEY: &str = "meetings.diarize";

/// The sherpa sidecar, which ships the diarization CLI beside the offline
/// transcription one. The same archive `local_asr` installs — so a user who
/// already has Parakeet has this binary too and does not download it twice.
const SIDECAR: &str = "sherpa";
const EXECUTABLE: &str = "sherpa-onnx-offline-speaker-diarization";

/// The install-progress event, shared with the other model screens so the
/// frontend has one listener rather than three.
const PROGRESS_EVENT: &str = "diarization://install";

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct DiarizationModelStatus {
    pub id: String,
    pub name: String,
    pub role: Option<kw_provider_registry::DiarizationRole>,
    pub size_mb: Option<f64>,
    pub installed: bool,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct DiarizationStatus {
    /// Whether the sherpa sidecar can be installed on this platform at all.
    pub availability: crate::local_model::Availability,
    pub cli_installed: bool,
    pub models: Vec<DiarizationModelStatus>,
    /// The setting, which is a different fact from whether it can run.
    pub enabled: bool,
    /// Both models present **and** the CLI installed. `enabled` without this is
    /// a setting that will be skipped, and the screen has to be able to say
    /// which of the two is missing rather than showing one green tick.
    pub ready: bool,
    pub models_dir: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
enum InstallProgress {
    Server {
        written: u64,
        total: Option<u64>,
    },
    Model {
        id: String,
        written: u64,
        total: Option<u64>,
    },
    Verifying,
    Extracting,
    Done,
}

fn models_root() -> PathBuf {
    kw_storage::paths::model_cache_dir()
        .path
        .join("diarization")
}

fn cli_root() -> PathBuf {
    kw_storage::paths::sidecar_dir().path
}

fn exe_name() -> String {
    if cfg!(windows) {
        format!("{EXECUTABLE}.exe")
    } else {
        EXECUTABLE.to_owned()
    }
}

fn installed_cli() -> Option<PathBuf> {
    let direct = cli_root().join(SIDECAR).join(exe_name());
    direct.is_file().then_some(direct)
}

fn shipped() -> Result<&'static [kw_provider_registry::DiarizationModel], String> {
    kw_provider_registry::registry()
        .map(|r| r.local_asr.diarization.as_slice())
        .map_err(|err| err.to_string())
}

/// Run the diarizer over a finished recording.
///
/// `Ok(None)` when the feature is not ready — no CLI, or a model missing. That
/// is not an error: it is the ordinary state of a machine that has never turned
/// this on, and the caller wants to save the note either way.
///
/// # Errors
///
/// A string, when the process was run and did not produce turns. Distinguished
/// from `Ok(None)` because a failure the user could act on must not read as a
/// feature that is simply off.
pub async fn turns(wav: &std::path::Path) -> Result<Option<Vec<Turn>>, String> {
    let Some(executable) = installed_cli() else {
        return Ok(None);
    };
    let Some(models) = diarization_models::installed(&models_root(), shipped()?) else {
        return Ok(None);
    };

    let args = diarization::build_args(&models, wav, &diarization::Tuning::default());
    let output = tokio::process::Command::new(&executable)
        .args(&args)
        .stdin(Stdio::null())
        .output()
        .await
        .map_err(|err| format!("the speaker diarization step could not start: {err}"))?;

    diarization::interpret(
        &String::from_utf8_lossy(&output.stdout),
        &String::from_utf8_lossy(&output.stderr),
        output.status.success(),
    )
    .map(Some)
    .map_err(|err| err.to_string())
}

/// Whether the setting is on. Read at save time, not held in memory, for the
/// reason `selected_stream` gives: one copy is one thing to keep in step.
pub async fn is_enabled(db: &kw_storage::Db) -> bool {
    kw_storage::settings::get_or(db.pool(), DIARIZE_KEY, false)
        .await
        .unwrap_or(false)
}

/* ------------------------------------------------------------------ commands */

/// The two models, what is installed, and whether the feature can run.
#[tauri::command]
pub async fn diarization_status(
    state: State<'_, crate::state::AppState>,
    asr: State<'_, Arc<crate::local_asr::LocalAsr>>,
) -> Result<DiarizationStatus, String> {
    let root = models_root();
    let models: Vec<DiarizationModelStatus> = shipped()?
        .iter()
        .map(|m| DiarizationModelStatus {
            id: m.id.clone(),
            name: m.name.clone(),
            role: m.role,
            size_mb: m.size_mb,
            installed: diarization_models::is_installed(&root, m),
        })
        .collect();

    let cli_installed = installed_cli().is_some();
    Ok(DiarizationStatus {
        availability: crate::local_model::Availability::of(&asr.catalog, SIDECAR),
        cli_installed,
        // Both models and the binary. Asked of the same function the save path
        // asks, so the screen cannot say ready while `turns` returns `None`.
        ready: cli_installed && diarization_models::installed(&root, shipped()?).is_some(),
        models,
        enabled: is_enabled(state.db()).await,
        models_dir: root.display().to_string(),
    })
}

/// Install one of the two models, and the CLI if it is not already there.
#[tauri::command]
pub async fn diarization_install_model(
    app: AppHandle,
    asr: State<'_, Arc<crate::local_asr::LocalAsr>>,
    model_id: String,
) -> Result<(), String> {
    let model = shipped()?
        .iter()
        .find(|m| m.id == model_id)
        .cloned()
        .ok_or_else(|| format!("{model_id} is not a diarization model in the registry"))?;

    let emit = |progress: InstallProgress| {
        if let Err(err) = app.emit(PROGRESS_EVENT, progress) {
            tracing::warn!(error = %err, "could not report install progress");
        }
    };

    // The same archive `local_asr` installs. Guarded rather than unconditional
    // for the reason KNEG-113 records: an unconditional call can only ever
    // succeed on the first install.
    if installed_cli().is_none() {
        let (spec, asset) = asr
            .catalog
            .resolve(SIDECAR)
            .map_err(|err| err.to_string())?;
        kw_sidecar::install(&asr.policy, spec, asset, &cli_root(), &mut |p| {
            emit(match p {
                kw_sidecar::install::Progress::Downloading { written, total } => {
                    InstallProgress::Server { written, total }
                }
                kw_sidecar::install::Progress::Verifying => InstallProgress::Verifying,
                kw_sidecar::install::Progress::Extracting => InstallProgress::Extracting,
            });
        })
        .await
        .map_err(|err| err.to_string())?;
    }

    diarization_models::install(&asr.policy, &model, &models_root(), &mut |p| {
        emit(match p {
            diarization_models::Progress::Downloading { written, total } => {
                InstallProgress::Model {
                    id: model_id.clone(),
                    written,
                    total,
                }
            }
            diarization_models::Progress::Verifying => InstallProgress::Verifying,
            diarization_models::Progress::Extracting => InstallProgress::Extracting,
        });
    })
    .await
    .map_err(|err| err.to_string())?;

    emit(InstallProgress::Done);
    Ok(())
}

/// Turn speaker labels on or off for future meetings.
///
/// Refuses to turn on what cannot run, naming which half is missing. The
/// alternative — accepting the setting and skipping it silently at save time —
/// is how a user comes to believe a feature is on for three meetings before
/// noticing no note has speaker labels.
#[tauri::command]
pub async fn diarization_set_enabled(
    state: State<'_, crate::state::AppState>,
    enabled: bool,
) -> Result<(), String> {
    if enabled {
        if installed_cli().is_none() {
            return Err(
                "The sherpa runtime is not installed. Install a diarization model first — it \
                 brings the runtime with it."
                    .to_owned(),
            );
        }
        if diarization_models::installed(&models_root(), shipped()?).is_none() {
            return Err(
                "Speaker labels need both models: the segmentation model and the speaker \
                 embedding model. Install whichever is missing."
                    .to_owned(),
            );
        }
    }
    kw_storage::settings::set(state.db().pool(), DIARIZE_KEY, &enabled)
        .await
        .map_err(|err| err.to_string())
}

pub(crate) fn voice_model() -> Result<PathBuf, String> {
    diarization_models::installed(&models_root(), shipped()?)
        .map(|m| m.embedding)
        .ok_or_else(|| "Install the speaker label models first".into())
}
pub(crate) fn voice_library() -> PathBuf {
    crate::voice_runtime::directory().join("sherpa-onnx-c-api.dll")
}
pub(crate) fn voice_runtime_ready() -> bool {
    crate::voice_runtime::ready()
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use super::*;

    #[test]
    fn the_executable_is_the_diarization_one_not_the_transcription_one() {
        // Both live in the same archive under the same sidecar name, and their
        // names differ by one word. Handing a WAV to `sherpa-onnx-offline`
        // would transcribe it and return no turns at all, which `interpret`
        // would read as a recording nobody spoke in.
        assert!(exe_name().starts_with("sherpa-onnx-offline-speaker-diarization"));
        assert_ne!(exe_name(), "sherpa-onnx-offline");
    }

    #[test]
    fn the_models_live_apart_from_the_asr_models() {
        // Same cache root, different subdirectory. `sherpa_models::install`
        // extracts with an empty payload prefix, so two model families sharing
        // one directory would be two archives writing into the same place.
        let root = models_root();
        assert!(root.ends_with("diarization"), "{root:?}");
    }
}
