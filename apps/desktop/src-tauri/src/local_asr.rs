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
//! # All four models are offered now, for two different things
//!
//! The registry's `streaming` flag separates them, and since 2026-09-01 it
//! separates *what they are for* rather than whether they work:
//!
//! - **Offline** (the two Parakeet models) — dictation, through the
//!   `sherpa-onnx-offline` CLI. What the probe and `tests/sherpa_dictation.rs`
//!   proved.
//! - **Streaming** (the two Nemotron models) — meetings, through
//!   `sherpa-onnx-online-websocket-server` and the `local-streaming` meeting
//!   provider. Its protocol is undocumented and was measured on 2026-09-01;
//!   see `kw_meeting_core::nemotron`.
//!
//! Until that measurement the streaming pair were listed with a reason and no
//! button, because no client for the protocol existed. The reason is gone
//! because the thing it described is gone, which is the only honest way for it
//! to go.
//!
//! **[`use_for`] is the rule, and it is one line and pure.** A model is never
//! offered for the engine that cannot run it: handing a Parakeet model to the
//! streaming server produces a server that starts, accepts a socket and
//! transcribes nothing, and `sherpa_stream::start` refuses it for exactly that
//! reason.
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
    /// Which engine it runs on, and therefore which button it gets.
    pub used_for: UsedFor,
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
    /// Which streaming model meetings are set to use, if any.
    ///
    /// A separate setting from `selected`, not a second reading of it: the two
    /// engines run different models and choosing one for dictation must not
    /// silently change what a meeting records with.
    pub selected_stream: Option<String>,
    /// Where models are kept. Shown because the native binaries cannot open a
    /// file whose path holds a non-ASCII character, so this is sometimes not
    /// where a person would expect.
    ///
    /// A space is fine, despite what this comment said until 2026-08-31 — see
    /// `kw_storage::paths` for the measurement.
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
    /// `pub(crate)` since 2026-09-01, for `diarization`, which installs the
    /// **same** sherpa sidecar this does. A second `NetworkPolicy` and a
    /// second `Catalog` for the same archive would be two things to keep in
    /// step, and the one that drifted would install a different binary than
    /// the one the other reported.
    pub(crate) policy: NetworkPolicy,
    pub(crate) catalog: kw_sidecar::catalog::Catalog,
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

/// What a model can be used for.
///
/// Two engines, and a model runs on exactly one of them. Not a boolean, for the
/// reason every other three-state answer in this codebase is not a boolean: the
/// screen needs to name the use, not just permit it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum UsedFor {
    /// The `sherpa-onnx-offline` CLI, one WAV at a time.
    Dictation,
    /// `sherpa-onnx-online-websocket-server`, words as they are recognised.
    Meetings,
}

/// Which engine this model runs on.
///
/// Pure and taking the flag rather than the whole model, so the rule is one
/// line and testable. It replaced `not_offered`, which returned a sentence
/// explaining that streaming models could not be used at all; that sentence
/// stopped being true when `kw_meeting_core::nemotron` landed.
#[must_use]
pub const fn use_for(streaming: bool) -> UsedFor {
    if streaming {
        UsedFor::Meetings
    } else {
        UsedFor::Dictation
    }
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
pub async fn sherpa_status(
    state: State<'_, crate::state::AppState>,
    asr: State<'_, Arc<LocalAsr>>,
) -> Result<SherpaStatus, String> {
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
            used_for: use_for(m.streaming),
        })
        .collect();

    Ok(SherpaStatus {
        availability: crate::local_model::Availability::of(&asr.catalog, SIDECAR),
        cli_installed: installed_cli().is_some(),
        models,
        selected: asr.selected.read().ok().and_then(|held| held.clone()),
        // Read from the database rather than held in memory. Unlike the
        // dictation selection there is nothing to resolve per request: the
        // meeting runtime reads the same key when a meeting starts, so a second
        // copy here would be a second thing to keep in step.
        selected_stream: kw_storage::settings::get_or(
            state.db().pool(),
            crate::sherpa_stream::STREAM_MODEL_KEY,
            String::new(),
        )
        .await
        .ok()
        .filter(|id| !id.is_empty()),
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
    // Every model in the registry is installable now. The refusal that used to
    // be here covered the streaming pair, on the grounds that no client existed
    // for their protocol -- which stopped being true when
    // `kw_meeting_core::nemotron` landed on 2026-09-01. Removed rather than
    // reworded: a check that can no longer fire is a check that stops being
    // read.
    let model = model_named(&model_id)?;

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

/// Choose the streaming model meetings record with.
///
/// The other half of [`sherpa_use_model`], and deliberately a separate command
/// writing a separate key. The two engines run different models: choosing one
/// for dictation must not silently change what a meeting records with, which is
/// what a single shared selection would do the first time somebody picked a
/// Parakeet model.
///
/// The server is **not** started here. Nothing about choosing a model says a
/// meeting is about to happen, and a 630 MB model loaded by a settings click
/// would be a cost the click did not advertise. `MeetingRuntime::provider`
/// starts it when a meeting actually begins.
#[tauri::command]
pub async fn sherpa_use_stream_model(
    state: State<'_, crate::state::AppState>,
    model_id: String,
) -> Result<(), String> {
    let model = model_named(&model_id)?;
    if use_for(model.streaming) != UsedFor::Meetings {
        // The same refusal `sherpa_stream::start` makes, made earlier and in
        // the settings screen's words. An offline model handed to the streaming
        // server produces a server that starts, accepts a socket and
        // transcribes nothing.
        return Err(format!(
            "{} is an offline model, so it is for dictation rather than meetings.",
            model.name
        ));
    }
    if !sherpa_models::is_installed(&models_root(), &model) {
        return Err(format!(
            "{} is not installed. Install it before choosing it for meetings.",
            model.name
        ));
    }

    kw_storage::settings::set(
        state.db().pool(),
        crate::sherpa_stream::STREAM_MODEL_KEY,
        &model_id,
    )
    .await
    .map_err(|err| err.to_string())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn every_model_is_offered_for_exactly_one_engine() {
        // The rule, stated once, against the registry rather than a literal
        // list of ids -- a fifth model landing should be classified by its own
        // flag, not by whether someone remembered to edit this test.
        //
        // Until 2026-09-01 this test asserted the opposite half: that the
        // streaming models were *not* offered, because no client for their
        // protocol existed. It does not read `>= 1 offered` now for the same
        // reason it did not then -- both engines having a model is the property
        // worth holding, and a count would pass with all four on one side.
        let registry = kw_provider_registry::registry().unwrap();

        let mut dictation = 0_usize;
        let mut meetings = 0_usize;
        for model in &registry.local_asr.sherpa {
            match use_for(model.streaming) {
                UsedFor::Dictation => {
                    assert!(!model.streaming, "{} is streaming", model.id);
                    dictation += 1;
                }
                UsedFor::Meetings => {
                    assert!(model.streaming, "{} is offline", model.id);
                    meetings += 1;
                }
            }
        }
        assert!(
            dictation > 0,
            "nothing can dictate, so the screen is pointless"
        );
        assert!(
            meetings > 0,
            "nothing can stream, so `local-streaming` has no model"
        );
    }

    #[test]
    fn the_two_engines_are_distinguishable() {
        // The wording lives in `labels.ts`, where an exhaustive switch with no
        // `default` makes a third variant a build failure. What this side owes
        // the screen is that the two are not the same value.
        assert_ne!(use_for(false), use_for(true));
        assert_eq!(use_for(true), UsedFor::Meetings);
    }

    #[test]
    fn an_unknown_model_is_refused_by_name() {
        let err = model_named("not-a-model").unwrap_err();
        assert!(err.contains("not-a-model"), "{err}");
    }
}
