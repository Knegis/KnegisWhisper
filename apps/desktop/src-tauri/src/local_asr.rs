//! `Settings > Transcription > Local`, the sherpa-onnx half.
//!
//! The runtime this drives was measured, not guessed: `examples/sherpa_probe.rs`
//! installed the pinned CLI and a pinned model, printed the directory layout it
//! found and the flags it derived from the real `--help`, and
//! `tests/sherpa_dictation.rs` then drove the whole path end to end — 501 MB
//! downloaded, speech in, the right words out. What was missing was any way for
//! a person to reach it: no command, no screen, so four `local-asr:*` manifest
//! entries sat `reachable: false` behind a runtime that worked. This module is
//! that way in, and it deliberately follows the sequence the passing test uses
//! rather than inventing a second one.
//!
//! # One local engine, not two
//!
//! `Transport::Local` names *the* local engine, singular. A selected sherpa
//! model therefore wins over a running `whisper-server`, and choosing a whisper
//! model clears the sherpa selection — see [`clear_selection`], called from
//! `local_server_start`. The alternative, letting both be "on" and picking by
//! some precedence the screen does not show, is the shape of state that makes a
//! person distrust the whole setting: dictation would go somewhere neither
//! control claimed.
//!
//! # Why only two of the four models are offered
//!
//! The registry's `streaming` flag separates them. The two Parakeet models are
//! offline transducers and are what the probe and the test proved. The two
//! Nemotron models are streaming, served over
//! `sherpa-onnx-online-websocket-server`'s own protocol, and nobody has written
//! a client for it — so they are listed with that reason rather than hidden,
//! the same rule `Settings > Meetings` follows for a provider with no client. A
//! model that cannot be used should say why on the screen where someone would
//! look for it.
//!
//! # What is never logged
//!
//! No audio, and no transcript. Progress here is a byte count and a model id.

use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use kw_network_policy::NetworkPolicy;
use kw_provider_registry::SherpaModel;
use kw_transcription_core::sherpa::SherpaOffline;
use kw_transcription_core::sherpa_models;
use serde::Serialize;
use tauri::{AppHandle, Emitter as _, State};
use ts_rs::TS;

use crate::local_model::{InstallProgress, PROGRESS_EVENT};

/// Which sherpa model dictation should use, remembered across restarts.
pub const SHERPA_MODEL_KEY: &str = "transcription.sherpaModel";

/// The sidecar id in `config/sidecars.json` for the offline CLI.
const SIDECAR: &str = "sherpa";

/// The executable that CLI installs as, used to find it again on a later run
/// without reinstalling.
const EXECUTABLE: &str = "sherpa-onnx-offline";

/// Why a model this build knows about is not on offer.
///
/// A reason rather than absence, for the same purpose
/// `ClientAvailability::NoClient` serves: someone who read that this
/// application supports Nemotron should find out here why they cannot pick it,
/// rather than wonder whether the list is broken.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SherpaModelStatus {
    pub id: String,
    pub name: String,
    pub size_mb: Option<f64>,
    pub language: Option<String>,
    /// Whether the extracted directory has the measured transducer layout.
    ///
    /// Checked by running the same discovery `transcribe` will use, so a
    /// half-extracted directory does not count as installed.
    pub installed: bool,
    /// `None` when the model can be installed and used.
    pub not_offered: Option<String>,
}

/// Everything the sherpa half of the screen needs, with no network access.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SherpaStatus {
    /// Whether the pinned CLI is published for this platform at all, and why
    /// not. The same three-state answer `local_model` gives for
    /// `whisper-server`, and for the same reason: "not published here" and "not
    /// installed yet" are different facts.
    pub availability: crate::local_model::Availability,
    /// Whether the offline CLI itself is installed.
    pub cli_installed: bool,
    pub models: Vec<SherpaModelStatus>,
    /// Which model dictation is set to use, if any.
    pub selected: Option<String>,
    /// Where models are kept. Shown because the native binaries cannot load
    /// from a path with a space or a non-ASCII character in it, so this is
    /// sometimes not where a person would expect.
    pub models_dir: String,
}

/// The selected model, shared with whatever builds a transcriber.
///
/// A model id rather than a built [`SherpaOffline`]: the CLI is a one-shot
/// process, so there is nothing running to hold, and resolving late means a
/// model deleted from disk between selection and use is reported honestly
/// instead of transcribing from a stale path.
pub type Selection = Arc<RwLock<Option<String>>>;

/// Everything needed to answer "what should dictate locally", per request.
#[derive(Debug)]
pub struct LocalAsr {
    policy: NetworkPolicy,
    catalog: kw_sidecar::catalog::Catalog,
    selected: Selection,
}

impl LocalAsr {
    /// Fails only if the shipped catalogue cannot be read, which is a broken
    /// build rather than a runtime condition -- the same failure
    /// `LocalTranscription::new` reports.
    pub fn new(policy: NetworkPolicy) -> anyhow::Result<Self> {
        Ok(Self {
            policy,
            catalog: kw_sidecar::Catalog::shipped()?,
            selected: Arc::new(RwLock::new(None)),
        })
    }

    #[must_use]
    pub fn selection(&self) -> Selection {
        Arc::clone(&self.selected)
    }

    /// Remember a selection made on a previous run.
    ///
    /// Called at startup. Unlike `whisper-server` there is no process to
    /// resume — the selection *is* the whole state — so this cannot fail in a
    /// way worth reporting: a model whose files have since gone is caught by
    /// [`resolve`] at the moment it is needed, with a message naming the model.
    pub async fn restore(&self, db: &kw_storage::Db) {
        let stored = kw_storage::settings::get_or(db.pool(), SHERPA_MODEL_KEY, String::new())
            .await
            .unwrap_or_default();
        let stored = stored.trim().to_owned();
        if stored.is_empty() {
            return;
        }
        if let Ok(mut held) = self.selected.write() {
            tracing::info!(model = %stored, "sherpa model selected from a previous run");
            *held = Some(stored);
        }
    }
}

/// Clear the sherpa selection, in memory and on disk.
///
/// Called when a whisper model is chosen, so that exactly one local engine is
/// ever selected. See the module doc.
pub async fn clear_selection(db: &kw_storage::Db, selected: &Selection) {
    if let Ok(mut held) = selected.write() {
        if held.is_none() {
            return;
        }
        *held = None;
    }
    if let Err(err) = kw_storage::settings::set(db.pool(), SHERPA_MODEL_KEY, &String::new()).await {
        tracing::warn!(error = %err, "could not clear the remembered sherpa model");
    }
}

/// Where sherpa models are kept: beside the whisper ones, under the same
/// ASCII-safe cache the native binaries can actually read from.
fn models_root() -> PathBuf {
    kw_storage::paths::model_cache_dir().path.join("sherpa")
}

/// Where the CLI is installed to.
fn cli_root() -> PathBuf {
    kw_storage::paths::sidecar_dir().path
}

/// Find the installed CLI, if it is there.
fn installed_cli() -> Option<PathBuf> {
    let direct = cli_root().join(SIDECAR).join(exe_name());
    direct.is_file().then_some(direct)
}

fn exe_name() -> String {
    if cfg!(windows) {
        format!("{EXECUTABLE}.exe")
    } else {
        EXECUTABLE.to_owned()
    }
}

/// Look one model up in the registry.
fn model_named(id: &str) -> Result<SherpaModel, String> {
    let registry = kw_provider_registry::registry().map_err(|err| err.to_string())?;
    registry
        .local_asr
        .sherpa
        .iter()
        .find(|m| m.id == id)
        .cloned()
        .ok_or_else(|| format!("no local model called {id:?} is in the registry"))
}

/// Why this model cannot be picked, or `None` if it can.
///
/// Pure and taking the flag rather than the whole model, so the rule is one
/// line and testable.
#[must_use]
pub fn not_offered(streaming: bool) -> Option<String> {
    streaming.then(|| {
        "streaming model — it is served over sherpa-onnx's own WebSocket protocol, \
         and no client for that protocol has been written"
            .to_owned()
    })
}

/// Build the transcriber for the selected model, if there is one.
///
/// Every failure is a refusal naming what is missing rather than a silent
/// fallback to whisper: dictation going somewhere the screen did not say is the
/// defect this whole module exists to avoid.
pub fn resolve(selected: &Selection) -> Option<Result<SherpaOffline, String>> {
    let id = selected.read().ok()?.clone()?;
    Some(resolve_id(&id))
}

fn resolve_id(id: &str) -> Result<SherpaOffline, String> {
    let model = model_named(id)?;
    let executable = installed_cli().ok_or_else(|| {
        format!(
            "the sherpa-onnx runtime is not installed. Install {id} again under \
                 Settings > Transcription > Local."
        )
    })?;
    let dir = sherpa_models::model_dir(&models_root(), &model);
    let files = kw_transcription_core::sherpa::discover_transducer_files(&dir)
        .map_err(|err| format!("the files for {id} could not be read: {err}"))?;

    Ok(SherpaOffline::new(
        executable,
        files,
        Some(model.id),
        kw_storage::paths::audio_temp_dir(),
        None,
    ))
}

/* ------------------------------------------------------------------ commands */

/// The sherpa models, what is installed, and which one is selected.
#[tauri::command]
pub async fn sherpa_status(asr: State<'_, Arc<LocalAsr>>) -> Result<SherpaStatus, String> {
    let registry = kw_provider_registry::registry().map_err(|err| err.to_string())?;
    let root = models_root();

    let models = registry
        .local_asr
        .sherpa
        .iter()
        .map(|m| SherpaModelStatus {
            id: m.id.clone(),
            name: m.name.clone(),
            size_mb: m.size_mb,
            language: m.language.clone(),
            installed: sherpa_models::is_installed(&root, m),
            not_offered: not_offered(m.streaming),
        })
        .collect();

    Ok(SherpaStatus {
        availability: crate::local_model::Availability::of(&asr.catalog, SIDECAR),
        cli_installed: installed_cli().is_some(),
        models,
        selected: asr.selected.read().ok().and_then(|held| held.clone()),
        models_dir: root.display().to_string(),
    })
}

/// Install one model, and the CLI if it is not already there.
///
/// Both in one command because a model without the runtime transcribes
/// nothing, and asking a person to press two buttons in the right order is a
/// way of making them responsible for an ordering the application knows.
#[tauri::command]
pub async fn sherpa_install_model(
    app: AppHandle,
    asr: State<'_, Arc<LocalAsr>>,
    model_id: String,
) -> Result<(), String> {
    let model = model_named(&model_id)?;
    if let Some(reason) = not_offered(model.streaming) {
        return Err(format!("{} cannot be installed: {reason}", model.name));
    }

    let emit = |progress: InstallProgress| {
        if let Err(err) = app.emit(PROGRESS_EVENT, progress) {
            tracing::warn!(error = %err, "could not report install progress");
        }
    };

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

    sherpa_models::install(&asr.policy, &model, &models_root(), &mut |p| {
        emit(match p {
            sherpa_models::Progress::Downloading { written, total } => InstallProgress::Model {
                id: model_id.clone(),
                written,
                total,
            },
            sherpa_models::Progress::Extracting => InstallProgress::Extracting,
        });
    })
    .await
    .map_err(|err| err.to_string())?;

    emit(InstallProgress::Done {
        verified_by: crate::local_model::VerifiedBy::Digest,
    });
    Ok(())
}

/// Send dictation to one installed sherpa model.
///
/// Resolved before it is remembered, so a selection that cannot actually run is
/// refused now with a message naming what is missing, rather than at the next
/// dictation.
#[tauri::command]
pub async fn sherpa_use_model(
    state: State<'_, crate::state::AppState>,
    asr: State<'_, Arc<LocalAsr>>,
    model_id: String,
) -> Result<(), String> {
    resolve_id(&model_id)?;

    if let Ok(mut held) = asr.selected.write() {
        *held = Some(model_id.clone());
    }
    kw_storage::settings::set(state.db().pool(), SHERPA_MODEL_KEY, &model_id)
        .await
        .map_err(|err| err.to_string())?;

    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn the_streaming_models_are_the_ones_not_offered() {
        // The rule, stated once, against the registry rather than a literal
        // list of ids -- a fifth model landing should be classified by its own
        // flag, not by whether someone remembered to edit this test.
        let registry = kw_provider_registry::registry().unwrap();

        let offered: Vec<&str> = registry
            .local_asr
            .sherpa
            .iter()
            .filter(|m| not_offered(m.streaming).is_none())
            .map(|m| m.id.as_str())
            .collect();

        assert!(
            !offered.is_empty(),
            "no local model is offerable, which would make this screen pointless"
        );
        for id in &offered {
            let model = registry
                .local_asr
                .sherpa
                .iter()
                .find(|m| &m.id == id)
                .unwrap();
            assert!(
                !model.streaming,
                "{id} is streaming and should not be offered"
            );
        }
    }

    #[test]
    fn a_streaming_model_says_why_rather_than_going_missing() {
        let reason = not_offered(true).expect("a streaming model must carry a reason");
        assert!(reason.contains("WebSocket"), "{reason}");
        assert!(reason.contains("no client"), "{reason}");
    }

    #[test]
    fn an_unknown_model_is_refused_by_name() {
        let err = model_named("not-a-model").unwrap_err();
        assert!(err.contains("not-a-model"), "{err}");
    }
}
