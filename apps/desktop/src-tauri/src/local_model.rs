//! Installing and running the local speech-to-text server.
//!
//! Everything here is a thin arrangement of `kw_sidecar` and
//! `kw_transcription_core::models`, which hold the parts that are worth testing:
//! digest verification, archive extraction, loopback port allocation, and the
//! reaper that refuses to signal a process it cannot prove is ours.
//!
//! Two things this module is responsible for on its own:
//!
//! * **Reaping before allocating.** A previous run that crashed leaves a
//!   `whisper-server` holding a port. Reaping happens at startup, before
//!   anything asks for a port, so the first dictation after a crash works rather
//!   than failing on a port that is already taken.
//! * **Never claiming an install it did not verify.** The status a caller gets
//!   distinguishes "installed and its digest matched" from "a file of about the
//!   right size is present", because for a model those are different facts and
//!   only one of them is a guarantee.

use std::sync::{Arc, RwLock};

use kw_sidecar::pid::PidStore;
use kw_sidecar::process::Health as _;
use kw_sidecar::{port, process, Catalog, HttpHealth, Launch, Running, StartTimings};
use kw_storage::paths;
use kw_transcription_core::local::{self, ServerOptions};
use kw_transcription_core::models;
use serde::Serialize;
use tauri::{AppHandle, Emitter as _, State};
use ts_rs::TS;

/// The sidecar this module manages.
const SIDECAR: &str = "whisper";

/// The model the user last ran the local server with.
///
/// Persisted because the route survives a restart and the running server does
/// not. Without this the application knew dictation should go to a local model
/// and had no idea which one, so the first dictation after every launch failed —
/// six recorded failures in one bench check, each of them the user speaking a
/// sentence that went nowhere.
pub const LOCAL_MODEL_KEY: &str = "transcription.localModel";

/// The event install progress is reported on.
pub const PROGRESS_EVENT: &str = "local-model-progress";

/// Everything needed to install and run the local server.
pub struct LocalTranscription {
    catalog: Catalog,
    pids: PidStore,
    policy: kw_network_policy::NetworkPolicy,
    /// The running server, if there is one.
    running: tokio::sync::Mutex<Option<Running>>,
    /// Shared with the dictation transcriber so it sees the server come up.
    port: Arc<RwLock<Option<u16>>>,
    /// Which model the running server loaded, for the history row.
    model: Arc<RwLock<Option<String>>>,
}

impl std::fmt::Debug for LocalTranscription {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LocalTranscription")
            .field("port", &self.port.read().ok().and_then(|p| *p))
            .finish_non_exhaustive()
    }
}

impl LocalTranscription {
    /// Load the catalogue and prepare, without touching the network.
    pub fn new(policy: kw_network_policy::NetworkPolicy) -> Result<Self, String> {
        let catalog = Catalog::shipped().map_err(|err| err.to_string())?;
        Ok(Self {
            catalog,
            pids: PidStore::new(&paths::data_dir()),
            policy,
            running: tokio::sync::Mutex::new(None),
            port: Arc::new(RwLock::new(None)),
            model: Arc::new(RwLock::new(None)),
        })
    }

    /// The handles the dictation transcriber needs.
    #[must_use]
    pub fn shared(&self) -> SharedLocal {
        SharedLocal {
            port: Arc::clone(&self.port),
            model: Arc::clone(&self.model),
        }
    }

    /// Clean up a server a previous run left behind.
    ///
    /// Called at startup, before any port is allocated. The reaper reads the live
    /// process's command first and refuses to signal anything it cannot prove is
    /// ours, so a reused process id is safe.
    pub async fn reap_stale(&self) {
        let Some(spec) = self.catalog.get(SIDECAR) else {
            return;
        };

        let outcome = kw_sidecar::reap_one(
            spec,
            &self.pids,
            &kw_sidecar::SystemProcessTable,
            kw_sidecar::reap::Timings::default(),
        )
        .await;

        match outcome {
            kw_sidecar::Outcome::NoRecord => {}
            other => tracing::info!(?other, "reaped a stale local model server"),
        }
    }

    /// Stop the server, if one is running.
    pub async fn stop(&self) {
        let mut held = self.running.lock().await;
        if let Some(running) = held.take() {
            running.stop(&self.pids).await.unwrap_or_else(|err| {
                tracing::warn!(error = %err, "could not stop the local model server cleanly");
            });
        }
        if let Ok(mut port) = self.port.write() {
            *port = None;
        }
        if let Ok(mut model) = self.model.write() {
            *model = None;
        }
    }

    /// Drop the recorded port and model for a server that has already gone.
    ///
    /// Not [`Self::stop`]: there is nothing to signal. The child is already
    /// dead, and asking the reaper to stop it would either find nothing or --
    /// worse, on a reused process id -- find something else. This only forgets.
    async fn forget_port(&self) {
        *self.running.lock().await = None;
        if let Ok(mut port) = self.port.write() {
            *port = None;
        }
        if let Ok(mut model) = self.model.write() {
            *model = None;
        }
    }
}

/// The handles this module shares with the dictation transcriber.
///
/// Named rather than a tuple so the two `Arc<RwLock<Option<_>>>` cannot be
/// swapped at a call site, which would be silent: both are the same shape.
#[derive(Debug, Clone)]
pub struct SharedLocal {
    /// Where the server is listening, when it is.
    pub port: Arc<RwLock<Option<u16>>>,
    /// Which model it loaded.
    pub model: Arc<RwLock<Option<String>>>,
}

/// What can be said about the local model server without touching the network.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct LocalStatus {
    /// Whether this platform has a published server binary at all, and why not.
    pub availability: Availability,
    /// Whether the server binary is installed.
    pub server_installed: bool,
    /// The models on offer, and what is known about each.
    pub models: Vec<ModelStatus>,
    /// The port the server is listening on, if it is running.
    pub port: Option<u16>,
    /// Which model the running server loaded, if one is running.
    ///
    /// Separate from `port` because "a server is up" and "*this* model is the
    /// one it loaded" are different claims, and the screen shows a row per
    /// model. The runtime has always tracked this for the history row; it was
    /// simply never reported, so no row could say which of them was live.
    pub running_model: Option<String>,
    /// Where installs are kept, and whether the per-user location had to be
    /// abandoned because the native binaries cannot cope with the path.
    pub install_dir: String,
    pub install_dir_fell_back: bool,
}

/// Whether the local server can be installed here.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(
    tag = "state",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum Availability {
    Installable,
    /// No binary is published for this platform, and this is why.
    NotPublished {
        reason: String,
        tracking: String,
    },
    /// The manifest says nothing about this platform. A gap, not a fact.
    Unknown,
}

impl Availability {
    /// What the catalogue says about one sidecar on the platform we are on.
    ///
    /// Shared by both local runtimes rather than written twice. The `Unknown`
    /// arms matter: an unrecognised target and a target the manifest does not
    /// mention are both "we do not know", and answering `Installable` for
    /// either would be a claim about a binary nobody has looked for.
    #[must_use]
    pub fn of(catalog: &kw_sidecar::catalog::Catalog, sidecar: &str) -> Self {
        let Some(target) = kw_sidecar::Target::current() else {
            return Self::Unknown;
        };
        match catalog.get(sidecar).map(|s| s.availability(target)) {
            Some(kw_sidecar::TargetState::Installable(_)) => Self::Installable,
            Some(kw_sidecar::TargetState::Unsupported { reason, tracking }) => Self::NotPublished {
                reason: reason.to_owned(),
                tracking: tracking.to_owned(),
            },
            _ => Self::Unknown,
        }
    }
}

/// How firmly a model's identity can be established.
///
/// Three states rather than a boolean, and an enum rather than a string. It was
/// a `&'static str` whose three values the frontend re-declared by hand as a
/// union; nothing held the two in step, so a fourth state added here would have
/// reached a UI that did not know about it. Now it cannot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum VerifiedBy {
    /// A measured SHA-256 is pinned for this model and the file matched it.
    Digest,
    /// Only a declared size is known. The file is the right length; it has not
    /// been proven to be the file the publisher built.
    Size,
    /// Neither is known. Reported, never silent.
    Nothing,
}

impl VerifiedBy {
    /// What the registry can hold this model to, before it is downloaded.
    fn declared_for(model: &kw_provider_registry::WhisperModel) -> Self {
        match models::integrity_for(model) {
            models::Integrity::Pinned { .. } => Self::Digest,
            models::Integrity::Size { .. } => Self::Size,
            models::Integrity::Unknown => Self::Nothing,
        }
    }

    /// What was actually checked once the file was on disk.
    const fn measured(verification: &models::Verification) -> Self {
        match verification {
            models::Verification::DigestMatched => Self::Digest,
            models::Verification::SizeOnly { .. } => Self::Size,
            models::Verification::Unchecked { .. } => Self::Nothing,
        }
    }

    /// The wire spelling, for the command that returns it as a bare string.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Digest => "digest",
            Self::Size => "size",
            Self::Nothing => "nothing",
        }
    }
}

/// One model on offer.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ModelStatus {
    pub id: String,
    pub name: String,
    pub size_mb: Option<f64>,
    pub recommended: bool,
    pub installed: bool,
    /// What integrity check this model can be held to. Shown to the user rather
    /// than implied, because the three are different guarantees.
    pub verified_by: VerifiedBy,
}

/// Everything the settings screen needs, with no network access.
#[tauri::command]
pub async fn local_model_status(
    local: State<'_, Arc<LocalTranscription>>,
) -> Result<LocalStatus, String> {
    let cache = paths::sidecar_dir();
    let models_dir = paths::model_cache_dir();

    let availability = Availability::of(&local.catalog, SIDECAR);

    let server_installed = local
        .catalog
        .resolve(SIDECAR)
        .ok()
        .is_some_and(|(_, asset)| {
            kw_sidecar::locate(
                SIDECAR,
                asset,
                &kw_sidecar::SearchPath {
                    bundled: None,
                    downloaded: cache.path.clone(),
                },
                &kw_sidecar::RealFs,
            )
            .is_installed()
        });

    let registry = kw_provider_registry::registry().map_err(|err| err.to_string())?;
    let models = registry
        .local_asr
        .whisper
        .iter()
        .map(|model| ModelStatus {
            id: model.id.clone(),
            name: model.name.clone(),
            size_mb: model.size_mb,
            recommended: model.recommended,
            installed: models::is_installed(&models_dir.path, model),
            verified_by: VerifiedBy::declared_for(model),
        })
        .collect();

    Ok(LocalStatus {
        availability,
        server_installed,
        models,
        port: local.port.read().ok().and_then(|p| *p),
        running_model: local.model.read().ok().and_then(|m| m.clone()),
        install_dir: cache.path.display().to_string(),
        install_dir_fell_back: cache.fell_back || models_dir.fell_back,
    })
}

/// Progress of an install, as reported to the frontend.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(
    tag = "stage",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum InstallProgress {
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
    Done {
        /// What the model on disk was actually checked against.
        verified_by: VerifiedBy,
    },
}

/// Install the server binary and one model.
///
/// Explicit: nothing downloads on first launch. This runs only because someone
/// asked for it.
#[tauri::command]
pub async fn local_model_install(
    app: AppHandle,
    local: State<'_, Arc<LocalTranscription>>,
    model_id: String,
) -> Result<String, String> {
    let (spec, asset) = local
        .catalog
        .resolve(SIDECAR)
        .map_err(|err| err.to_string())?;

    let registry = kw_provider_registry::registry().map_err(|err| err.to_string())?;
    let model = registry
        .local_asr
        .whisper
        .iter()
        .find(|m| m.id == model_id)
        .ok_or_else(|| format!("{model_id:?} is not a model this build knows about"))?;

    let sidecars = paths::sidecar_dir();
    let models_dir = paths::model_cache_dir();

    let emit = |progress: InstallProgress| {
        if let Err(err) = app.emit(PROGRESS_EVENT, progress) {
            tracing::debug!(error = %err, "could not report install progress");
        }
    };

    // The server first: a model with no server to load it is not usable, and the
    // server is the smaller download of the two.
    kw_sidecar::install(&local.policy, spec, asset, &sidecars.path, &mut |p| {
        emit(match p {
            kw_sidecar::Progress::Downloading { written, total } => {
                InstallProgress::Server { written, total }
            }
            kw_sidecar::Progress::Verifying => InstallProgress::Verifying,
            kw_sidecar::Progress::Extracting => InstallProgress::Extracting,
        });
    })
    .await
    .map_err(|err| err.to_string())?;

    let installed = models::install(&local.policy, model, &models_dir.path, &mut |p| {
        emit(match p {
            models::ModelProgress::Downloading { written, total } => InstallProgress::Model {
                id: model_id.clone(),
                written,
                total,
            },
            models::ModelProgress::Verifying => InstallProgress::Verifying,
        });
    })
    .await
    .map_err(|err| err.to_string())?;

    let verified_by = VerifiedBy::measured(&installed.verification);
    emit(InstallProgress::Done { verified_by });

    Ok(verified_by.as_str().to_owned())
}

/// Start the local server with an installed model.
#[tauri::command]
pub async fn local_server_start(
    state: State<'_, crate::state::AppState>,
    local: State<'_, Arc<LocalTranscription>>,
    asr: State<'_, Arc<crate::local_asr::LocalAsr>>,
    model_id: String,
) -> Result<u16, String> {
    let port = local.start_model(&model_id).await?;

    // One local engine at a time. Choosing a whisper model releases a sherpa
    // selection, because otherwise the sherpa one would keep winning and this
    // button would appear to do nothing -- see `local_asr`'s module doc.
    crate::local_asr::clear_selection(state.db(), &asr.selection()).await;

    // Remembered only after it actually came up, so a model that fails to start
    // is not the one resumed at the next launch.
    if let Err(err) = kw_storage::settings::set(state.db().pool(), LOCAL_MODEL_KEY, &model_id).await
    {
        // Not fatal: the server is running. The next launch just will not know
        // to bring it back, which is the state we were in before this existed.
        tracing::warn!(error = %err, "could not remember the local model");
    }

    Ok(port)
}

/* ------------------------------------------------------------------- revive */

/// Something that can bring the local model server back after it has gone.
///
/// A trait rather than an `Arc<LocalTranscription>` because the two callers --
/// dictation and media import -- want one sentence's worth of capability
/// ("start the remembered model, tell me the port"), and neither should be able
/// to reach the installer, the reaper or the catalogue through it.
///
/// This exists because `d489471` taught [`LocalTranscription::start_model`] to
/// notice a server that had gone and start a new one, and taught nothing else.
/// Pressing "Use for dictation" recovered; *dictating* did not, and neither did
/// a media import -- the path where it matters most, because a recording being
/// imported is usually one that cannot be made again. Recorded as KNEG-96.
#[async_trait::async_trait]
pub trait Revive: Send + Sync + std::fmt::Debug {
    /// Start the model the user last chose, and answer with its port.
    async fn revive(&self) -> Result<u16, String>;
}

/// The real [`Revive`]: whatever the user last dictated with.
///
/// Deliberately the *persisted* choice rather than the one held in memory. By
/// the time anyone asks, [`LocalTranscription::forget_port`] has cleared the
/// in-memory model alongside the port -- they go together on purpose, since a
/// model name without a server is the claim that caused this defect. The
/// persisted choice is the only durable answer to *which* model, and it is the
/// same one [`resume_last_used`] reads, so reviving mid-dictation cannot bring
/// back a different model than a relaunch would.
struct ReviveRemembered {
    local: Arc<LocalTranscription>,
    db: kw_storage::Db,
}

impl std::fmt::Debug for ReviveRemembered {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ReviveRemembered").finish_non_exhaustive()
    }
}

#[async_trait::async_trait]
impl Revive for ReviveRemembered {
    async fn revive(&self) -> Result<u16, String> {
        match model_to_resume(&self.db).await {
            Resume::Model(model_id) => self.local.start_model(&model_id).await,
            // Neither of these is an ordinary failure: something asked to
            // revive a server for a route that should not have been reaching
            // one. Say which contradiction it was rather than reporting a
            // missing server, which is the true-but-useless answer.
            Resume::NotRouted => Err(
                "dictation is not routed to a local model, so there is nothing to start".to_owned(),
            ),
            Resume::NoneRemembered => Err(
                "no local model is remembered. Choose one under Settings > Transcription > Local."
                    .to_owned(),
            ),
        }
    }
}

/// The handle dictation and media import revive the server through.
#[must_use]
pub fn revive_handle(local: &Arc<LocalTranscription>, db: &kw_storage::Db) -> Arc<dyn Revive> {
    Arc::new(ReviveRemembered {
        local: Arc::clone(local),
        db: db.clone(),
    })
}

/// A [`Revive`] that never does, for tests about something else.
///
/// Named for what it does rather than `Stub`, so a test that revives by
/// accident reads as wrong at the call site.
#[cfg(test)]
#[derive(Debug)]
pub struct RefusesToRevive;

#[cfg(test)]
#[async_trait::async_trait]
impl Revive for RefusesToRevive {
    async fn revive(&self) -> Result<u16, String> {
        Err("this test does not start a local model server".to_owned())
    }
}

/// Bring back the local server the user was last dictating with.
///
/// Called once at startup. The route survives a restart and the process does
/// not, so without this the application knows dictation should go to a local
/// model and has nothing to send it to — which is exactly how a bench check
/// produced six failed dictations, two of them after a relaunch.
///
/// Spawned rather than awaited: loading a model takes a second or two and the
/// window should not wait for it. Failure is logged and left alone; dictation
/// then reports the honest reason.
pub fn resume_last_used(local: &Arc<LocalTranscription>, db: &kw_storage::Db) {
    let local = Arc::clone(local);
    let db = db.clone();

    tauri::async_runtime::spawn(async move {
        match model_to_resume(&db).await {
            Resume::NotRouted => {}
            Resume::NoneRemembered => tracing::info!(
                "dictation is routed to a local model but none is remembered; choose one \
                 under Local transcription"
            ),
            Resume::Model(model_id) => match local.start_model(&model_id).await {
                Ok(port) => {
                    tracing::info!(port, model = %model_id, "resumed the local model server");
                }
                Err(err) => {
                    tracing::warn!(model = %model_id, error = %err, "could not resume the local model server");
                }
            },
        }
    });
}

/// What startup should do about the local server.
///
/// Three outcomes rather than an `Option`, because "the user does not dictate
/// locally" and "the user does, and we have forgotten which model" call for
/// different log lines — and the second one is a state worth saying out loud,
/// since it is the one that used to end in a failed dictation.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Resume {
    Model(String),
    NotRouted,
    NoneRemembered,
}

async fn model_to_resume(db: &kw_storage::Db) -> Resume {
    let route: kw_transcription_core::RouteSettings =
        kw_storage::settings::get(db.pool(), crate::dictation::ROUTE_SETTINGS_KEY)
            .await
            .ok()
            .flatten()
            .unwrap_or_default();

    if !route.use_local_whisper {
        return Resume::NotRouted;
    }

    match kw_storage::settings::get::<String>(db.pool(), LOCAL_MODEL_KEY).await {
        Ok(Some(model_id)) if !model_id.trim().is_empty() => Resume::Model(model_id),
        _ => Resume::NoneRemembered,
    }
}

impl LocalTranscription {
    /// Start the server for one model, or return the port if it is already up.
    pub async fn start_model(&self, model_id: &str) -> Result<u16, String> {
        let local = self;
        // Already running: answer with the port rather than starting a second
        // one -- but only if something is still there.
        //
        // This used to return the recorded port unconditionally, which made a
        // dead server permanent. The child can go without the application
        // noticing (a crash, a machine that slept, someone ending the task) and
        // nothing clears `port`, so every dictation failed against a port with
        // nothing behind it and pressing "Use for dictation" handed back the
        // same dead port instead of starting a server. The only cure was
        // restarting the whole application. Found on 2026-08-28: the app
        // holding 8178, no listener, no process.
        if let Some(port) = local.port.read().ok().and_then(|p| *p) {
            if HttpHealth::new(local.policy.clone(), local::HEALTH_PATH)
                .is_ready(port)
                .await
            {
                return Ok(port);
            }
            tracing::warn!(
                port,
                "the recorded local model server is not answering; starting a new one"
            );
            local.forget_port().await;
        }

        let (spec, asset) = local
            .catalog
            .resolve(SIDECAR)
            .map_err(|err| err.to_string())?;

        let sidecars = paths::sidecar_dir();
        let installation = kw_sidecar::locate(
            SIDECAR,
            asset,
            &kw_sidecar::SearchPath {
                bundled: None,
                downloaded: sidecars.path.clone(),
            },
            &kw_sidecar::RealFs,
        );
        let executable = installation
            .executable()
            .ok_or_else(|| installation.explain("the local model server"))?
            .to_path_buf();

        let registry = kw_provider_registry::registry().map_err(|err| err.to_string())?;
        let model = registry
            .local_asr
            .whisper
            .iter()
            .find(|m| m.id == model_id)
            .ok_or_else(|| format!("{model_id:?} is not a model this build knows about"))?;

        let model_path = models::model_path(&paths::model_cache_dir().path, model);
        if !local::model_is_present(&model_path.display().to_string()) {
            // Checked here rather than left to the server, whose failure for a
            // missing model is an exit during startup with the reason buried in
            // captured output.
            return Err(format!(
                "{} is not installed. Install it before starting the server.",
                model.name
            ));
        }

        let chosen = port::allocate(
            spec.port_range().ok_or_else(|| {
                format!("{} is a stdio child and does not serve a port", spec.name)
            })?,
            &port::BindProbe,
        )
        .map_err(|err| err.to_string())?;

        let options = ServerOptions {
            model_path: model_path.display().to_string(),
            port: chosen,
            language: None,
            threads: None,
            gpu_device: None,
            vad: None,
        };

        let launch = Launch {
            executable,
            args: local::build_args(&options),
            port: chosen,
        };

        let health = HttpHealth::new(local.policy.clone(), local::HEALTH_PATH);
        let running = process::start(spec, &launch, &local.pids, &health, StartTimings::default())
            .await
            .map_err(|err| err.to_string())?;

        // Published only once it has answered, so a dictation never routes to a
        // server that is still loading its model.
        if let Ok(mut port) = local.port.write() {
            *port = Some(chosen);
        }
        if let Ok(mut held) = local.model.write() {
            *held = Some(model_id.to_owned());
        }
        *local.running.lock().await = Some(running);

        Ok(chosen)
    }
}

/// Stop the local server.
#[tauri::command]
pub async fn local_server_stop(local: State<'_, Arc<LocalTranscription>>) -> Result<(), String> {
    local.stop().await;
    Ok(())
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

    fn local() -> LocalTranscription {
        LocalTranscription::new(kw_network_policy::NetworkPolicy::new().unwrap()).unwrap()
    }

    async fn db() -> kw_storage::Db {
        kw_storage::Db::open_in_memory().await.unwrap()
    }

    /* -------------------------------------------------------- reviving */

    #[tokio::test]
    async fn reviving_a_route_that_is_not_local_says_so() {
        // Reached only through a contradiction: a whisper port was recorded, so
        // a server ran, but the route no longer sends anything to it. Reporting
        // a missing server here would be true and useless -- the cure is not to
        // start one.
        let db = db().await;
        let revive = revive_handle(&Arc::new(local()), &db);

        let err = revive.revive().await.unwrap_err();
        assert!(err.contains("not routed"), "{err}");
    }

    #[tokio::test]
    async fn reviving_with_no_remembered_model_names_the_screen() {
        // The route is local and nothing says which model. There is no safe
        // guess: starting the smallest, or the first in the catalogue, would
        // transcribe with a model the user did not choose and report it as
        // though they had.
        let db = db().await;
        kw_storage::settings::set(
            db.pool(),
            crate::dictation::ROUTE_SETTINGS_KEY,
            &kw_transcription_core::RouteSettings {
                use_local_whisper: true,
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let revive = revive_handle(&Arc::new(local()), &db);

        let err = revive.revive().await.unwrap_err();
        assert!(err.contains("Settings > Transcription > Local"), "{err}");
    }

    #[tokio::test]
    async fn nothing_is_resumed_when_dictation_does_not_use_a_local_model() {
        // The default. Starting a model server for someone who dictates to a
        // cloud provider would spend a second of startup and 75 MB of memory on
        // a feature they are not using.
        assert_eq!(model_to_resume(&db().await).await, Resume::NotRouted);
    }

    #[tokio::test]
    async fn a_local_route_with_no_remembered_model_is_its_own_outcome() {
        // The state the bench check hit: route says local, nothing says which
        // model, and the first dictation after launch failed with a message
        // telling the user to install a model they already had.
        let db = db().await;
        let route = kw_transcription_core::RouteSettings {
            use_local_whisper: true,
            ..Default::default()
        };
        kw_storage::settings::set(db.pool(), crate::dictation::ROUTE_SETTINGS_KEY, &route)
            .await
            .unwrap();

        assert_eq!(model_to_resume(&db).await, Resume::NoneRemembered);
    }

    #[tokio::test]
    async fn the_remembered_model_is_resumed() {
        let db = db().await;
        let route = kw_transcription_core::RouteSettings {
            use_local_whisper: true,
            ..Default::default()
        };
        kw_storage::settings::set(db.pool(), crate::dictation::ROUTE_SETTINGS_KEY, &route)
            .await
            .unwrap();
        kw_storage::settings::set(db.pool(), LOCAL_MODEL_KEY, &"tiny")
            .await
            .unwrap();

        assert_eq!(model_to_resume(&db).await, Resume::Model("tiny".to_owned()));
    }

    #[tokio::test]
    async fn a_blank_remembered_model_is_not_a_model() {
        // An empty string round-trips through settings as a value, and asking a
        // server to load "" fails in a way that reads like a bug in the sidecar.
        let db = db().await;
        let route = kw_transcription_core::RouteSettings {
            use_local_whisper: true,
            ..Default::default()
        };
        kw_storage::settings::set(db.pool(), crate::dictation::ROUTE_SETTINGS_KEY, &route)
            .await
            .unwrap();
        kw_storage::settings::set(db.pool(), LOCAL_MODEL_KEY, &"   ")
            .await
            .unwrap();

        assert_eq!(model_to_resume(&db).await, Resume::NoneRemembered);
    }

    #[test]
    fn the_catalogue_loads_at_construction_rather_than_at_first_use() {
        // A malformed config/sidecars.json should fail where it can be reported,
        // not on the first dictation.
        assert!(local().catalog.get("whisper").is_some());
    }

    #[test]
    fn the_shared_handles_are_the_same_ones_the_transcriber_sees() {
        // If these were separate Arcs, a dictation started after the server came
        // up would still route as though nothing were running.
        let local = local();
        let SharedLocal { port, model } = local.shared();

        if let Ok(mut held) = port.write() {
            *held = Some(8178);
        }
        if let Ok(mut held) = model.write() {
            *held = Some("tiny".to_owned());
        }

        assert_eq!(local.port.read().unwrap().as_ref(), Some(&8178));
        assert_eq!(
            local.model.read().unwrap().as_deref(),
            Some("tiny"),
            "the transcriber and the lifecycle must share one handle"
        );
    }

    #[test]
    fn a_model_reports_which_check_it_can_be_held_to() {
        // "installed" alone would let the UI imply a digest match for a model
        // that was only size-checked. Every shipped model is pinned now, so the
        // screen says `digest` for all six -- and the mapping for the weaker
        // answer is still asserted below, against a model built here, because a
        // branch nothing exercises is a branch that rots.
        let registry = kw_provider_registry::registry().unwrap();
        for model in &registry.local_asr.whisper {
            assert_eq!(
                VerifiedBy::declared_for(model),
                VerifiedBy::Digest,
                "{} is not pinned; see WHISPER_PINS in the registry generator",
                model.id
            );
        }

        let unmeasured = kw_provider_registry::WhisperModel {
            id: "not-yet-measured".to_owned(),
            name: "Not yet measured".to_owned(),
            file_name: "ggml-not-yet-measured.bin".to_owned(),
            download_url: "https://huggingface.co/example/resolve/main/x.bin".to_owned(),
            size_mb: Some(142.0),
            sha256: None,
            expected_size_bytes: Some(148_000_000),
            recommended: false,
        };
        assert_eq!(VerifiedBy::declared_for(&unmeasured), VerifiedBy::Size);

        let mut nothing_known = unmeasured;
        nothing_known.expected_size_bytes = None;
        assert_eq!(
            VerifiedBy::declared_for(&nothing_known),
            VerifiedBy::Nothing
        );
    }

    #[tokio::test]
    async fn stopping_when_nothing_is_running_is_harmless() {
        let local = local();
        local.stop().await;
        assert!(local.port.read().unwrap().is_none());
    }

    #[tokio::test]
    async fn reaping_with_no_record_does_nothing_and_does_not_fail() {
        // The first launch on a new machine.
        local().reap_stale().await;
    }

    #[test]
    fn the_install_directory_follows_the_native_binary_rule() {
        // whisper-server loads its sibling ggml libraries relative to its own
        // directory and chokes on a path with a space or non-ASCII character.
        let dir = paths::sidecar_dir();
        if !dir.fell_back {
            assert!(kw_storage::paths::is_native_binary_safe(&dir.path));
        }
    }
}
