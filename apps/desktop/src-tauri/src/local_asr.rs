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

/// The executable that CLI installs as.
///
/// Kept only as the anchor for
/// `the_catalogue_still_publishes_the_executable_this_module_expects`. Nothing
/// builds a path out of it any more -- see [`installed_cli`] -- because doing so
/// made the screen ask the same question twice and get two independently
/// derived answers.
///
/// `#[cfg(test)]` because that is now the whole truth about it: it is an
/// expectation, not an input. Leaving it in the shipped build would be a second
/// spelling of the executable name for somebody to reach for.
#[cfg(test)]
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
    /// Whether what is installed is the build this version pins.
    ///
    /// `install` has written a record beside the extracted files since
    /// KNEG-113 and nothing here read it, so `cli_installed` stayed true while
    /// the runtime fell behind the manifest. KNEG-128.
    ///
    /// Derived from the same call as `cli_installed`, which is the point --
    /// see [`sherpa_status`].
    ///
    /// **One archive, three consumers.** The same install directory serves
    /// dictation here, `sherpa_stream`'s meeting server and `diarization`'s
    /// speaker CLI, under one record, so this answer is about all three at once
    /// and the screen says so.
    pub cli_freshness: crate::local_model::SidecarFreshness,
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

/// Where the CLI is, if it is installed, according to the catalogue.
///
/// **Goes through `kw_sidecar::locate` against the published asset**, rather
/// than joining `cli_root()/sherpa/` to a hand-built executable name as it did
/// until KNEG-128. Both spellings resolve to the same path on all five
/// published targets -- every one of them ships `.../bin/sherpa-onnx-offline`
/// -- so this is not a fix for a live bug. It is a fix for `cli_installed` and
/// the freshness sentence beside it being two independent answers to one
/// question, which is the disagreement `local_model::local_model_status` is
/// arranged to prevent and the reason its two fields come from a single call.
///
/// Takes the catalogue rather than reaching for one, so [`sherpa_status`] can
/// pass the one [`LocalAsr`] already holds.
/// Takes the root as an argument for the reason `media.rs:1334-1340` records:
/// `cli_root()` reads a real per-machine path, so a version of this that reached
/// for it directly could not be pointed at a test directory and the search would
/// be untested. A mutation replacing the `locate` call below survived the suite
/// until this was split out.
fn located_cli_in(
    catalog: &kw_sidecar::catalog::Catalog,
    root: &std::path::Path,
) -> Option<PathBuf> {
    let (_, asset) = catalog.resolve(SIDECAR).ok()?;
    kw_sidecar::locate(
        SIDECAR,
        asset,
        &kw_sidecar::SearchPath {
            bundled: None,
            downloaded: root.to_path_buf(),
        },
        &kw_sidecar::RealFs,
    )
    .executable()
    .map(std::path::Path::to_path_buf)
}

fn located_cli(catalog: &kw_sidecar::catalog::Catalog) -> Option<PathBuf> {
    located_cli_in(catalog, &cli_root())
}

/// Find the installed CLI, for the callers that have no catalogue in hand.
///
/// [`resolve_id`] is reached from `dictation.rs` and `media.rs` through a bare
/// [`Selection`], so threading a `Catalog` to it would mean threading one
/// through `DictationRuntime` and `MediaImportRuntime` as well -- a ripple far
/// wider than the duplication it would remove. Parsing the shipped catalogue
/// here instead is an embedded-string `serde_json::from_str` on a path that is
/// about to spawn a process and transcribe a WAV, and [`LocalAsr::new`] has
/// already proved at startup that it parses. A catalogue that somehow did not
/// would report the runtime as absent, which is the safe direction: the refusal
/// names what is missing rather than transcribing somewhere unexpected.
fn installed_cli() -> Option<PathBuf> {
    located_cli(&kw_sidecar::Catalog::shipped().ok()?)
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

    // One call for both, so they cannot disagree: `Freshness::NotInstalled` is
    // exactly the case `locate` reports as absent or incomplete, which is the
    // same question `located_cli` asks.
    let freshness = crate::local_model::sidecar_freshness(&asr.catalog, SIDECAR, &cli_root());

    Ok(SherpaStatus {
        availability: crate::local_model::Availability::of(&asr.catalog, SIDECAR),
        cli_installed: !matches!(freshness, kw_sidecar::Freshness::NotInstalled),
        cli_freshness: crate::local_model::SidecarFreshness::of(&freshness),
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

    /* ------------------------------------------------------- the runtime */

    /// The catalogue still publishes the executable this module is built around.
    ///
    /// `located_cli` asks the catalogue for the executable name now rather than
    /// composing one, so a rename upstream would no longer break dictation with
    /// a "not installed" that is not true -- it would follow the rename. That is
    /// the improvement and it is also why this assertion is worth keeping: three
    /// other things depend on *which* binary lands in that directory.
    /// `sherpa_stream` finds its websocket server beside this one by name, and
    /// `diarization` finds its speaker CLI the same way, so an upstream rename
    /// is news even when this module absorbs it silently.
    #[test]
    fn the_catalogue_still_publishes_the_executable_this_module_expects() {
        let catalog = kw_sidecar::Catalog::shipped().unwrap();
        let Ok((_, asset)) = catalog.resolve(SIDECAR) else {
            // No published asset for this target. Asserted rather than skipped
            // silently: `availability` is what reports that, and a test that
            // returns here having checked nothing is the shape this wave is
            // about.
            assert!(
                located_cli(&catalog).is_none(),
                "nothing is published for this platform, so nothing can be located"
            );
            let _ = EXECUTABLE;
            return;
        };

        let published = asset.executable_name();
        assert!(
            published.starts_with(EXECUTABLE),
            "{published} is not {EXECUTABLE}; sherpa_stream and diarization both \
             expect their binaries beside this one"
        );
    }

    /// An installed runtime is reported installed **and** of known provenance,
    /// from one call.
    ///
    /// The disagreement this exists to prevent: `cli_installed` said *yes* while
    /// nothing anywhere said *which build*, so a runtime two releases behind the
    /// manifest read exactly like the pinned one. The two answers are derived
    /// from a single `sidecar_freshness` in `sherpa_status`, and this walks the
    /// three states that call reports.
    ///
    /// **Mutation that turns it red:** have `located_cli` join a hand-built
    /// executable name again, or point the search path at `cli_root()` without
    /// the sidecar subdirectory.
    #[test]
    fn an_installed_runtime_is_reported_installed_and_its_build_named() {
        let dir = tempfile::tempdir().unwrap();
        let catalog = kw_sidecar::Catalog::shipped().unwrap();
        let Ok((spec, asset)) = catalog.resolve(SIDECAR) else {
            assert_eq!(
                crate::local_model::sidecar_freshness(&catalog, SIDECAR, dir.path()),
                kw_sidecar::Freshness::NotInstalled
            );
            return;
        };

        // 1. Nothing there. Both halves agree, from an empty directory.
        assert_eq!(located_cli_in(&catalog, dir.path()), None);
        assert_eq!(
            crate::local_model::sidecar_freshness(&catalog, SIDECAR, dir.path()),
            kw_sidecar::Freshness::NotInstalled
        );

        // 2. The executable, with nothing saying where it came from. What every
        //    install made before KNEG-113 reports.
        let install = dir.path().join(SIDECAR);
        std::fs::create_dir_all(&install).unwrap();
        let exe = install.join(asset.executable_name());
        std::fs::write(&exe, b"MZ").unwrap();
        // The positive control: the negative below is measured against an
        // artefact that exists, not against an empty directory that would
        // answer `NotInstalled` for the wrong reason.
        assert!(exe.is_file(), "the subject of the assertion must be there");

        // The path half. `resolve_id` builds a transcriber out of this, and
        // `sherpa_status` reports the freshness below -- one question, and the
        // point of the change is that both go through `locate` against the
        // catalogue's own asset rather than a name composed here.
        assert_eq!(
            located_cli_in(&catalog, dir.path()).as_deref(),
            Some(exe.as_path()),
            "the runtime must be found where the catalogue says the archive puts it"
        );

        let freshness = crate::local_model::sidecar_freshness(&catalog, SIDECAR, dir.path());
        assert_eq!(
            freshness,
            kw_sidecar::Freshness::Unknown(kw_sidecar::Unrecorded::NoRecord)
        );
        assert!(
            !matches!(freshness, kw_sidecar::Freshness::NotInstalled),
            "an executable that is plainly there must not report as absent; \
             `cli_installed` is read straight off this"
        );

        // 3. Recorded, so the pair reads current.
        kw_sidecar::record::write(&install, &kw_sidecar::InstallRecord::of(spec, asset)).unwrap();
        assert_eq!(
            crate::local_model::sidecar_freshness(&catalog, SIDECAR, dir.path()),
            kw_sidecar::Freshness::Current
        );
    }
}
