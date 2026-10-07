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
type StartOutcome = (std::time::Instant, String, Result<u16, String>);

pub struct LocalTranscription {
    operation: tokio::sync::Mutex<()>,
    last_start: std::sync::Mutex<Option<StartOutcome>>,
    runtime: RwLock<LocalRuntime>,
    catalog: Catalog,
    pids: PidStore,
    policy: kw_network_policy::NetworkPolicy,
    /// The running server, if there is one.
    running: tokio::sync::Mutex<Option<Running>>,
    /// Shared with the dictation transcriber so it sees the server come up.
    port: Arc<RwLock<Option<u16>>>,
    /// Which model the running server loaded, for the history row.
    model: Arc<RwLock<Option<String>>>,
    /// Shared with media import and meetings. See [`SpeechTools`].
    speech: SpeechTools,
    /// Whether the user is switching the model right now. See [`RouteSwitch`].
    switching: RouteSwitch,
}

/// Whether the user is switching the local model right now (KNEG-388).
///
/// Held for the whole of `local_server_start`, which is reached only when
/// somebody chooses a Whisper model for dictation (Settings › Transcription
/// and the Dictation route menu). Dictation reads it to refuse a start in that
/// window: the old server is already gone, and the route a recording would be
/// transcribed by is whichever the switch has reached by the time it stops.
/// Launch-time resume and revives do not hold it -- nobody chose those, and
/// refusing a dictation while the remembered model loads at startup would be
/// a regression. Nor does `local_server_refresh`: it brings back the model
/// that was running, and holding it would refuse every route for the length
/// of a download.
///
/// A count rather than a flag, so a second switch queued behind the first
/// keeps it held until both are done.
#[derive(Debug, Clone, Default)]
pub struct RouteSwitch(Arc<std::sync::atomic::AtomicUsize>);

impl RouteSwitch {
    /// Mark a switch as running until the returned guard is dropped.
    pub fn begin(&self) -> RouteSwitchGuard {
        self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        RouteSwitchGuard(Arc::clone(&self.0))
    }

    /// Whether any switch is running.
    #[must_use]
    pub fn in_progress(&self) -> bool {
        self.0.load(std::sync::atomic::Ordering::SeqCst) > 0
    }
}

/// One running switch. Dropping it ends it, however the command returned.
#[must_use = "the switch ends when this guard is dropped"]
#[derive(Debug)]
pub struct RouteSwitchGuard(Arc<std::sync::atomic::AtomicUsize>);

impl Drop for RouteSwitchGuard {
    fn drop(&mut self) {
        self.0.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
    }
}

/// Which earlier start a waiting one may take as its own answer.
#[derive(Debug, Clone, Copy)]
enum Join {
    /// Only a start of the same model, whatever its result. What a user's
    /// choice needs: choosing C while B starts must never answer with B.
    SameModel,
    /// That, or a start of any model that succeeded. What a revive needs: it
    /// wants *a* server, and the one the user just switched to is the right
    /// one even though the remembered model has not caught up yet (KNEG-388).
    AnySuccess,
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
    ///
    /// `vad_model` is what [`bundled_vad_model`] found.
    pub fn new(
        policy: kw_network_policy::NetworkPolicy,
        vad_model: Option<std::path::PathBuf>,
    ) -> Result<Self, String> {
        let catalog = Catalog::shipped().map_err(|err| err.to_string())?;
        Ok(Self {
            operation: tokio::sync::Mutex::new(()),
            last_start: std::sync::Mutex::new(None),
            runtime: RwLock::new(LocalRuntime::Stopped),
            catalog,
            pids: PidStore::new(&paths::data_dir()),
            policy,
            running: tokio::sync::Mutex::new(None),
            port: Arc::new(RwLock::new(None)),
            model: Arc::new(RwLock::new(None)),
            speech: SpeechTools::new(
                vad_model,
                &paths::sidecar_dir().path,
                paths::audio_temp_dir(),
            ),
            switching: RouteSwitch::default(),
        })
    }

    /// The handles the dictation transcriber needs.
    #[must_use]
    pub fn shared(&self) -> SharedLocal {
        SharedLocal {
            port: Arc::clone(&self.port),
            model: Arc::clone(&self.model),
            speech: self.speech.clone(),
            switching: self.switching.clone(),
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
        let _operation = self.operation.lock().await;
        self.stop_inner().await;
    }

    async fn stop_inner(&self) {
        *self
            .last_start
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
        self.set_runtime(LocalRuntime::Stopped);
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
    /// What long recordings need to find their speech first.
    pub speech: SpeechTools,
    /// Whether the user is switching the model, for dictation to refuse a
    /// start meanwhile. See [`RouteSwitch`].
    pub switching: RouteSwitch,
}

/// The Silero VAD model `tauri.conf.json` bundles under `resources/`
/// (KNEG-404). From `ggml-org/whisper-vad` on Hugging Face; the digest is the
/// file's LFS SHA-256 there, and the bytes here were checked against it.
pub const VAD_MODEL_FILE: &str = "ggml-silero-v5.1.2.bin";
const VAD_MODEL_BYTES: u64 = 885_098;
const VAD_MODEL_SHA256: &str = "29940d98d42b91fbd05ce489f3ecf7c72f0a42f027e4875919a28fb4c04ea2cf";

/// The speech finder, from the same whisper.cpp archive as the server and
/// extracted beside it.
const VAD_TOOL: &str = "whisper-vad-speech-segments";

/// The bundled VAD model under `resource_dir`, if it is there, is the pinned
/// file, and sits on a path the finder can open.
///
/// Checked once, at startup. A wrong file fails the finder, which costs every
/// long recording its search for speech -- not its transcript, because a
/// failed search sends the recording whole, but quietly. A path outside ASCII
/// fails it the same way: measured on 2026-10-06, the finder exited 127 with no
/// output for a model under `Åsa ö`, which is the ANSI code page problem
/// `kw_storage::paths` describes for the model cache.
pub fn bundled_vad_model(resource_dir: Option<&std::path::Path>) -> Option<std::path::PathBuf> {
    let path = resource_dir?.join("resources").join(VAD_MODEL_FILE);
    let pinned = kw_sidecar::Sha256Digest::parse(VAD_MODEL_SHA256).ok()?;
    if let Err(err) = kw_sidecar::integrity::verify(&path, VAD_MODEL_BYTES, &pinned) {
        tracing::warn!(error = %err, "the bundled VAD model is not usable; long recordings will be sent whole");
        return None;
    }
    if !path.to_str().is_some_and(str::is_ascii) {
        tracing::warn!("the bundled VAD model is on a path the speech finder cannot open; long recordings will be sent whole");
        return None;
    }
    Some(path)
}

/// What a long recording needs to find its speech before whisper hears it
/// (KNEG-404): the bundled model, verified at startup, and the finder, which
/// arrives with the whisper sidecar and so may be installed or removed while
/// the application runs.
#[derive(Debug, Clone)]
pub struct SpeechTools {
    model: Option<std::path::PathBuf>,
    tool: std::path::PathBuf,
    scratch: std::path::PathBuf,
}

impl SpeechTools {
    /// For the whisper sidecar installed under `sidecars`, staging recordings
    /// in `scratch`.
    #[must_use]
    pub fn new(
        model: Option<std::path::PathBuf>,
        sidecars: &std::path::Path,
        scratch: std::path::PathBuf,
    ) -> Self {
        Self {
            model,
            tool: sidecars
                .join(SIDECAR)
                .join(format!("{VAD_TOOL}{}", std::env::consts::EXE_SUFFIX)),
            scratch,
        }
    }

    /// The finder for a recording made for `context`, when everything it
    /// needs is here.
    ///
    /// `kw_audio_core::vad`'s rule decides, with no preferences stored, so
    /// meetings and recordings find their speech first and dictation never
    /// does. `None` is not a failure: the recording is sent whole, as before.
    #[must_use]
    pub fn finder(
        &self,
        context: kw_audio_core::vad::Context,
    ) -> Option<Arc<dyn kw_transcription_core::speech::FindSpeech>> {
        use kw_audio_core::vad::{silero_enabled_for, SileroPreferences, VadConfig};
        if !silero_enabled_for(context, SileroPreferences::default()) {
            return None;
        }
        let model = self.model.clone()?;
        if !self.tool.is_file() {
            return None;
        }
        Some(Arc::new(kw_transcription_core::speech::SpeechFinder::new(
            self.tool.clone(),
            model,
            VadConfig::default(),
            self.scratch.clone(),
        )))
    }
}

/// What can be said about the local model server without touching the network.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct LocalStatus {
    pub runtime_message: String,
    /// Whether this platform has a published server binary at all, and why not.
    pub availability: Availability,
    /// Whether the server binary is installed.
    pub server_installed: bool,
    /// Whether the installed one is the build this version pins.
    ///
    /// Separate from `server_installed`, and it has to be: "a `whisper-server.exe`
    /// is there" and "it is *our* `whisper-server.exe`" are different claims,
    /// and until KNEG-113 only the first was ever made. The screen showed
    /// *Installed* for a binary two releases behind the manifest and nothing
    /// anywhere would have said so.
    pub server_freshness: SidecarFreshness,
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
    /// What the server is doing, as data. `runtime_message` is this as one
    /// sentence, for Diagnostics.
    pub runtime: LocalRuntime,
}

/// What the local model server is doing (KNEG-357).
///
/// Dictation words this plainly for colleagues who are not technical: nothing
/// while it is ready, and failures in plain view with the mechanism folded.
/// [`Self::message`] is the sentence Diagnostics shows whole, model id and
/// port included. Both come from here, so they cannot drift apart; until
/// KNEG-357 the sentence was all there was, and Dictation showed it as is:
/// "Local model base is ready on port 8178."
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(
    tag = "state",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum LocalRuntime {
    Stopped,
    /// Looking for a server already up for this model, or starting one.
    Checking {
        model: String,
    },
    /// The process is starting and loading the model.
    Loading {
        model: String,
    },
    Ready {
        model: String,
        port: u16,
    },
    Failed {
        error: String,
    },
    /// Went away on its own. The next dictation or import starts it again.
    Exited {
        report: String,
    },
}

impl LocalRuntime {
    /// The state as one sentence, for Diagnostics.
    #[must_use]
    pub fn message(&self) -> String {
        match self {
            Self::Stopped => "Local model server is stopped.".to_owned(),
            Self::Checking { model } => format!("Checking or starting local model {model}..."),
            Self::Loading { model } => format!("Loading local model {model}..."),
            Self::Ready { model, port } => format!("Local model {model} is ready on port {port}."),
            Self::Failed { error } => format!(
                "Local model could not start: {error}. Retry dictation or choose a model in \
                 Settings › Transcription."
            ),
            Self::Exited { report } => format!(
                "Local model server exited. The next dictation or import will attempt \
                 recovery. {report}"
            ),
        }
    }
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

/// Whether the installed server binary is the build this version pins.
///
/// The wire form of [`kw_sidecar::Freshness`]. A separate type for the reason
/// [`Availability`] is one: the crate's enum carries `Sha256Digest` and a
/// structured reason, neither of which should become a TypeScript shape that
/// then has to be kept in step across the boundary.
///
/// **`Unknown` is not a rounded-down `Stale`.** An install this application did
/// not record cannot be compared with anything, and the screen says exactly
/// that rather than picking whichever of the other two answers reads better.
/// The machine this was written on is in that state right now.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(
    tag = "state",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum SidecarFreshness {
    /// Nothing is installed, so there is nothing to be out of date.
    NotInstalled,
    /// What is installed came from the archive this build pins.
    Current,
    /// It came from a different archive. Both digests, so a bug report can be
    /// checked rather than believed.
    Stale { installed: String, expected: String },
    /// Something is installed and this build cannot say what it is.
    Unknown { why: String },
}

impl SidecarFreshness {
    /// The wire form of a verdict.
    #[must_use]
    pub fn of(freshness: &kw_sidecar::Freshness) -> Self {
        match freshness {
            kw_sidecar::Freshness::NotInstalled => Self::NotInstalled,
            kw_sidecar::Freshness::Current => Self::Current,
            kw_sidecar::Freshness::Stale {
                installed,
                expected,
            } => Self::Stale {
                installed: installed.to_string(),
                expected: expected.to_string(),
            },
            kw_sidecar::Freshness::Unknown(why) => Self::Unknown { why: why.explain() },
        }
    }
}

/// What this build knows about the sidecar on disk.
///
/// The filesystem half of the question, kept in one place because several
/// callers want it and each of them getting the search path slightly wrong is
/// exactly how `server_installed` and the start path would come to disagree.
///
/// `pub(crate)` since KNEG-128, for [`crate::local_llm`] and
/// [`crate::local_asr`], which install sidecars through the same
/// `kw_sidecar::install` and so have had the record on disk since KNEG-113
/// without ever reading it. A second copy of this function was the alternative
/// and would have been two things to keep in step -- and the one that drifted
/// would report a different answer about the same directory.
pub(crate) fn sidecar_freshness(
    catalog: &Catalog,
    sidecar: &str,
    root: &std::path::Path,
) -> kw_sidecar::Freshness {
    let Ok((_, asset)) = catalog.resolve(sidecar) else {
        // No published asset for this platform. Nothing is pinned, so nothing
        // can be behind it.
        return kw_sidecar::Freshness::NotInstalled;
    };
    let installation = kw_sidecar::locate(
        sidecar,
        asset,
        &kw_sidecar::SearchPath {
            bundled: None,
            downloaded: root.to_path_buf(),
        },
        &kw_sidecar::RealFs,
    );
    let record = kw_sidecar::record::read(&root.join(sidecar));
    kw_sidecar::freshness(&installation, &record, sidecar, &asset.sha256)
}

/// How firmly a model's identity can be established.
///
/// Four states rather than a boolean, and an enum rather than a string. It was
/// a `&'static str` whose three values the frontend re-declared by hand as a
/// union; nothing held the two in step, so a fourth state added here would have
/// reached a UI that did not know about it. Now it cannot -- and the fourth
/// state duly arrived, as [`Self::StatedDigest`].
///
/// Shared between the whisper models and the local-LLM GGUFs, which need
/// different subsets of it: whisper reaches [`Self::Digest`] on all six and
/// [`Self::StatedDigest`] on none, and the GGUFs are the other way round for
/// thirty-two of thirty-nine objects. A variant unreachable on one path is kept
/// rather than split into two enums, for the reason
/// `models::Verification::SizeOnly` is kept: it is still the honest answer for a
/// model somebody adds tomorrow.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum VerifiedBy {
    /// A SHA-256 **this project measured** is pinned for this model, and the file
    /// matched it. The strongest of the four: the digest was taken from bytes
    /// that were downloaded once and hashed here, and committed to this
    /// repository, so it is not delivered by whoever serves the download.
    Digest,
    /// The **publisher's stated** SHA-256 is pinned for this model, and the file
    /// matched it.
    ///
    /// Weaker than [`Self::Digest`] in one specific way that must not be blurred:
    /// the digest came from the same host as the bytes -- Hugging Face's
    /// `lfs.oid`, which is a true SHA-256 of the object because git-lfs is
    /// content-addressed on it. So this catches a corrupted transfer, a
    /// truncation that happened to keep the length, and an accidental re-upload;
    /// it does not catch a huggingface.co that has been compromised, since one
    /// host would serve both halves of the comparison.
    StatedDigest,
    /// Only a size is known. The file is the right length; it has not been proven
    /// to be the file the publisher built.
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
            Self::StatedDigest => "stated-digest",
            Self::Size => "size",
            Self::Nothing => "nothing",
        }
    }

    /// The weaker of two verdicts.
    ///
    /// An install that downloads two files -- a GGUF and its speculative-decoding
    /// drafter -- is only as verified as its weakest half, and reporting the
    /// model's verdict while a drafter rode in on a weaker one would be the kind
    /// of rounding-up this enum exists to prevent.
    #[must_use]
    pub const fn weaker(self, other: Self) -> Self {
        // Ordinals rather than `PartialOrd`, so the ordering is stated here and
        // not implied by the declaration order of the variants -- reordering the
        // variants for readability must not silently reorder the strengths.
        const fn rank(v: VerifiedBy) -> u8 {
            match v {
                VerifiedBy::Digest => 3,
                VerifiedBy::StatedDigest => 2,
                VerifiedBy::Size => 1,
                VerifiedBy::Nothing => 0,
            }
        }
        if rank(other) < rank(self) {
            other
        } else {
            self
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

    // One call for both, so they cannot disagree: `Freshness::NotInstalled` is
    // exactly the case `locate` reports as absent or incomplete.
    let freshness = sidecar_freshness(&local.catalog, SIDECAR, &cache.path);
    let server_installed = !matches!(freshness, kw_sidecar::Freshness::NotInstalled);

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

    let mut runtime = local
        .runtime
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();
    let mut alive = false;
    if let Some(running) = local.running.lock().await.as_mut() {
        alive = running.is_running().await;
        if !alive {
            runtime = LocalRuntime::Exited {
                report: running
                    .exit_report()
                    .unwrap_or_else(|| running.stderr_tail()),
            };
        }
    }
    Ok(LocalStatus {
        runtime_message: runtime.message(),
        runtime,
        availability,
        server_installed,
        server_freshness: SidecarFreshness::of(&freshness),
        models,
        port: if alive {
            local.port.read().ok().and_then(|p| *p)
        } else {
            None
        },
        running_model: if alive {
            local.model.read().ok().and_then(|m| m.clone())
        } else {
            None
        },
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

/// Whether a model install has to fetch the server binary first.
///
/// A pure decision with one caller, extracted for the reason
/// `check_importable_file` and `import_pick_dialog` were: nothing outside a
/// running application can construct a `tauri::AppHandle`, so the command below
/// is untestable by construction and the judgement in it would be untested with
/// it.
///
/// The judgement is which states count as "already there".
/// [`kw_sidecar::Installation::Incomplete`] -- a directory that exists with no
/// executable in it, from an interrupted extraction -- must reinstall, and does,
/// because it has no executable. Only `Installed` skips.
///
/// Reinstalling when there is nothing to reinstall is not a wasted download; it
/// is a failed install. `install` finishes by replacing the sidecar directory,
/// and on Windows a directory holding a running executable cannot be removed --
/// measured on 2026-08-31 as `WinError 5`, on the `.exe` itself. See KNEG-113.
fn sidecar_needed(installation: &kw_sidecar::Installation) -> bool {
    installation.executable().is_none()
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
    //
    // **Only if it is not already there.** This used to reinstall
    // unconditionally, which made every whisper model after the first
    // uninstallable. `install` finishes by replacing the sidecar directory, and
    // `resume_last_used` has started `whisper-server.exe` out of that same
    // directory by the time any of this runs -- so `remove_dir_all` fails with
    // `WinError 5` on the running executable, the `?` here returns, and the
    // model is never downloaded at all. Reported from the bench on 2026-08-31,
    // pressing *Install* on `base` with `tiny` already resumed: no
    // `ggml-base.bin`, no `.partial`, and a `whisper.incoming` staging
    // directory left behind at the promote step.
    //
    // `local_llm_install` in `local_llm.rs` has always guarded it exactly this
    // way, for the same reason, which is why installing a GGUF worked on the
    // same run that this failed.
    //
    // **Nothing is lost by skipping.** The unconditional call looked like a way
    // to refresh a sidecar to the pinned version, and it never was: it can only
    // succeed when no server is running, which is only ever the first install,
    // when there is nothing to refresh. Every later call failed outright rather
    // than updating anything. Keeping a sidecar current after the first install
    // is KNEG-113, filed with the measurement.
    let installation = kw_sidecar::locate(
        SIDECAR,
        asset,
        &kw_sidecar::SearchPath {
            bundled: None,
            downloaded: sidecars.path.clone(),
        },
        &kw_sidecar::RealFs,
    );

    if sidecar_needed(&installation) {
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
    }

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

/* ------------------------------------------------------------------ refresh */

/// What became of the server a refresh had to stop.
///
/// Three states rather than an `Option<String>`, because "nothing was running"
/// and "something was running and could not be started again" are opposite
/// news and the second one is the whole risk of doing this at all.
///
/// `pub(crate)` since KNEG-128: the llama.cpp refresh in [`crate::local_llm`]
/// stops and restarts its own server for the same reason and needs the same
/// three answers about it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Restart {
    NothingWasRunning,
    Restarted(String),
    Failed { model: String, error: String },
}

impl Restart {
    pub(crate) const fn restarted(&self) -> Option<&String> {
        match self {
            Self::Restarted(model) => Some(model),
            _ => None,
        }
    }

    pub(crate) const fn error(&self) -> Option<&String> {
        match self {
            Self::Failed { error, .. } => Some(error),
            _ => None,
        }
    }
}

/// The sentence a failed refresh leaves the user with.
///
/// A refresh **stops the server before it replaces it**, so a refused download
/// ends with the old binary still on disk and nothing serving it unless it was
/// started again. The install error on its own would not say that, and the
/// difference between "your download failed" and "your download failed and
/// dictation is now down" is the whole of what the user needs to know.
///
/// Pure, and separated from the command for the reason `media.rs:1334-1340`
/// records: a judgement reachable only through an `AppHandle` is a judgement
/// nothing can test.
///
/// `pub(crate)` since KNEG-128; the llama.cpp refresh leaves the user in exactly
/// the same position and owes them the same sentence.
pub(crate) fn refresh_failure(install_error: &str, restart: &Restart) -> String {
    match restart {
        Restart::NothingWasRunning => install_error.to_owned(),
        Restart::Restarted(model) => format!(
            "{install_error} The server was not replaced, and the one that was running \
             ({model}) has been started again."
        ),
        Restart::Failed { model, error } => format!(
            "{install_error} The server was not replaced, and the one that was running \
             ({model}) could not be started again: {error}"
        ),
    }
}

/// What a refresh of a local server binary did.
///
/// **One type for both runtimes**, `whisper-server` and `llama-server`. Not
/// politeness: `ipc_types` asserts that no two Rust types render to the same
/// TypeScript name, so a second `RefreshOutcome` for the llama.cpp refresh
/// added in KNEG-128 would have been a failing test rather than a duplicate
/// nobody noticed. The two refreshes answer the same three questions -- what
/// was unpacked, what came back, and what did not -- so sharing the answer was
/// the right resolution rather than renaming one of them.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct RefreshOutcome {
    /// How many files the replacement unpacked. Reported for the reason
    /// [`kw_sidecar::Installed::files`] is: one would mean the sibling `ggml`
    /// libraries were missed and the server will not start.
    pub files: usize,
    /// The model the server was running before, now running again. A whisper
    /// model id or a GGUF id, depending on which screen asked.
    pub restarted: Option<String>,
    /// Why it could not be started again, when it could not. Not an error —
    /// the replacement succeeded — but not something to leave unsaid either.
    pub restart_error: Option<String>,
}

/// Replace the installed server binary with the one this build pins.
///
/// **Stop, replace, restart.** The application owns the process, so it can put
/// it down for the length of the install and bring it back afterwards; that is
/// the whole reason this is a few lines rather than a mechanism.
///
/// The alternative considered and rejected was renaming the running install
/// aside and unpacking over the vacated name. Measured on 2026-09-01 with
/// `cargo run -p kw-sidecar --example running_exe_lock_probe`, that does work
/// as far as it goes — on Windows, `rename` of a directory holding a running
/// executable succeeds where `remove_dir_all` fails with `os error 5`, and the
/// vacated name can be created immediately. It was rejected because of the
/// probe's third line: **the renamed-aside directory is still undeletable.**
/// The lock is on the mapped image file, not on the path. So rename-aside buys
/// a shorter outage and inherits a directory that cannot be removed until the
/// process ends, which needs a cleanup pass at some later moment and an
/// `install` that tolerates finding one. More machinery, for the case where the
/// user has explicitly asked to replace the thing they are using.
///
/// Nothing calls this on its own. Like every other download in this
/// application, it happens because somebody pressed a button.
#[tauri::command]
pub async fn local_server_refresh(
    app: AppHandle,
    local: State<'_, Arc<LocalTranscription>>,
) -> Result<RefreshOutcome, String> {
    // No `switching` guard: a refresh restarts the model that was running, so
    // a revive queued behind it brings back that same model, and refusing
    // dictation here would refuse cloud and Parakeet routes too for the whole
    // download, for a switch that is not happening (KNEG-388).
    let _operation = local.operation.lock().await;
    let (spec, asset) = local
        .catalog
        .resolve(SIDECAR)
        .map_err(|err| err.to_string())?;
    let sidecars = paths::sidecar_dir();

    let emit = |progress: InstallProgress| {
        if let Err(err) = app.emit(PROGRESS_EVENT, progress) {
            tracing::debug!(error = %err, "could not report refresh progress");
        }
    };

    // Read before the stop: `stop` clears it, and this is the only record of
    // what has to be put back.
    let was_running = local.model.read().ok().and_then(|held| held.clone());
    local.stop_inner().await;

    let installed = kw_sidecar::install(&local.policy, spec, asset, &sidecars.path, &mut |p| {
        emit(match p {
            kw_sidecar::Progress::Downloading { written, total } => {
                InstallProgress::Server { written, total }
            }
            kw_sidecar::Progress::Verifying => InstallProgress::Verifying,
            kw_sidecar::Progress::Extracting => InstallProgress::Extracting,
        });
    })
    .await;

    // Whichever way the install went. A refused download must not leave the
    // user with a server that was working a moment ago and is now simply gone.
    let restart = match &was_running {
        None => Restart::NothingWasRunning,
        Some(model) => match local.start_model_inner(model).await {
            Ok(port) => {
                tracing::info!(port, model = %model, "restarted the local model server after a refresh");
                Restart::Restarted(model.clone())
            }
            Err(error) => Restart::Failed {
                model: model.clone(),
                error,
            },
        },
    };

    match installed {
        Ok(installed) => Ok(RefreshOutcome {
            files: installed.files,
            restarted: restart.restarted().cloned(),
            restart_error: restart.error().cloned(),
        }),
        Err(err) => Err(refresh_failure(&err.to_string(), &restart)),
    }
}

/// Start the local server with an installed model.
#[tauri::command]
pub async fn local_server_start(
    state: State<'_, crate::state::AppState>,
    local: State<'_, Arc<LocalTranscription>>,
    asr: State<'_, Arc<crate::local_asr::LocalAsr>>,
    model_id: String,
) -> Result<u16, String> {
    // Held until the new model is remembered, so dictation refuses a start for
    // the whole switch rather than recording into a route half-way through
    // changing (KNEG-388).
    let _switching = local.switching.begin();
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
///
/// One exception, through [`LocalTranscription::revive_model`]: a revive that
/// waited behind a start which succeeded uses that server, whatever its model.
/// During a switch from A to B the persisted choice is still A until B is up,
/// and starting A then undid the switch (KNEG-388).
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
            Resume::Model(model_id) => self.local.revive_model(&model_id).await,
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

/// The handle a meeting on the `local` provider starts the server through
/// (KNEG-387).
///
/// Not [`revive_handle`]: that one starts a model only while *dictation* is
/// routed to local Whisper, and someone dictating to a cloud service can still
/// choose the local provider for meetings. Until this, Start then failed with
/// "the local model server is not running" while the page said Ready.
#[must_use]
pub fn meeting_revive_handle(
    local: &Arc<LocalTranscription>,
    db: &kw_storage::Db,
) -> Arc<dyn Revive> {
    Arc::new(ReviveForMeeting {
        local: Arc::clone(local),
        db: db.clone(),
    })
}

/// The [`Revive`] behind [`meeting_revive_handle`]. See [`meeting_model`].
struct ReviveForMeeting {
    local: Arc<LocalTranscription>,
    db: kw_storage::Db,
}

impl std::fmt::Debug for ReviveForMeeting {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ReviveForMeeting").finish_non_exhaustive()
    }
}

#[async_trait::async_trait]
impl Revive for ReviveForMeeting {
    async fn revive(&self) -> Result<u16, String> {
        let remembered = kw_storage::settings::get::<String>(self.db.pool(), LOCAL_MODEL_KEY)
            .await
            .ok()
            .flatten();
        let models_dir = paths::model_cache_dir();
        let registry = kw_provider_registry::registry().map_err(|err| err.to_string())?;
        let installed: Vec<String> = registry
            .local_asr
            .whisper
            .iter()
            .filter(|model| models::is_installed(&models_dir.path, model))
            .map(|model| model.id.clone())
            .collect();
        match meeting_model(remembered.as_deref(), &installed) {
            Some(model_id) => self.local.revive_model(&model_id).await,
            None => Err(
                "no local Whisper model is installed. Install one under Settings > \
                         Transcription > Local."
                    .to_owned(),
            ),
        }
    }
}

/// Which model a meeting starts: the remembered one while it is still
/// installed, otherwise the first installed, in the registry's order.
///
/// The same rule the Meetings page's readiness assumes (`lib/readiness.ts`
/// `localProviderProblem`): any installed model means Start can begin.
fn meeting_model(remembered: Option<&str>, installed: &[String]) -> Option<String> {
    remembered
        .map(str::trim)
        .filter(|id| installed.iter().any(|model| model == id))
        .map(str::to_owned)
        .or_else(|| installed.first().cloned())
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
    fn set_runtime(&self, runtime: LocalRuntime) {
        *self
            .runtime
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = runtime;
    }

    /// Start the server for one model, or return the port if it is already up.
    pub async fn start_model(&self, model_id: &str) -> Result<u16, String> {
        self.start(model_id, Join::SameModel).await
    }

    /// [`start_model`](Self::start_model), for a revive.
    ///
    /// The difference is what it accepts from a start that finished while it
    /// waited: any that succeeded, not only one of `model_id`. A revive reads
    /// the remembered model, and during a switch from A to B that is still A
    /// -- B is remembered only once it is up. Starting A then stopped the
    /// server the user had just switched to and left A running with B stored
    /// (KNEG-388).
    pub async fn revive_model(&self, model_id: &str) -> Result<u16, String> {
        self.start(model_id, Join::AnySuccess).await
    }

    async fn start(&self, model_id: &str, join: Join) -> Result<u16, String> {
        let requested = std::time::Instant::now();
        let _operation = self.operation.lock().await;
        let shared = self
            .last_start
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        if let Some((finished, model, result)) = shared {
            let joins = match join {
                Join::SameModel => model == model_id,
                Join::AnySuccess => model == model_id || result.is_ok(),
            };
            if finished >= requested && joins {
                return result;
            }
        }
        self.set_runtime(LocalRuntime::Checking {
            model: model_id.to_owned(),
        });
        let result = self.start_model_inner(model_id).await;
        self.set_runtime(match &result {
            Ok(port) => LocalRuntime::Ready {
                model: model_id.to_owned(),
                port: *port,
            },
            Err(error) => LocalRuntime::Failed {
                error: error.clone(),
            },
        });
        tracing::info!(
            model = model_id,
            succeeded = result.is_ok(),
            "local model startup or recovery completed"
        );
        *self
            .last_start
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some((
            std::time::Instant::now(),
            model_id.to_owned(),
            result.clone(),
        ));
        result
    }

    async fn start_model_inner(&self, model_id: &str) -> Result<u16, String> {
        let local = self;
        let same_model =
            local.model.read().ok().and_then(|m| m.clone()).as_deref() == Some(model_id);
        if !same_model {
            local.stop_inner().await;
        }
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
            local.stop_inner().await;
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
        };

        let launch = Launch {
            executable,
            args: local::build_args(&options),
            port: chosen,
        };

        local.set_runtime(LocalRuntime::Loading {
            model: model_id.to_owned(),
        });
        tracing::info!(model = model_id, executable = %launch.executable.display(), freshness = ?sidecar_freshness(&local.catalog, SIDECAR, &sidecars.path), "starting local model");
        let health = HttpHealth::new(local.policy.clone(), local::HEALTH_PATH);
        let running = process::start(spec, &launch, &local.pids, &health, StartTimings::default())
            .await
            .map_err(|err| err.to_string())?;

        tracing::info!(
            pid = running.pid(),
            model = model_id,
            port = chosen,
            "local model process is ready"
        );

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

/// One attempt before submitting audio; never retries a transcription request.
pub(crate) async fn ensure_whisper(
    policy: &kw_network_policy::NetworkPolicy,
    recorded: Option<u16>,
    revive: &dyn Revive,
) -> Result<u16, String> {
    if let Some(port) = recorded {
        if HttpHealth::new(policy.clone(), local::HEALTH_PATH)
            .is_ready(port)
            .await
        {
            return Ok(port);
        }
    }
    revive.revive().await
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

    /* ------------------------------- the server's state, as data (KNEG-357) */

    #[test]
    fn diagnostics_reads_the_sentences_it_always_read() {
        // Diagnostics shows the message whole, so it is pinned here. One
        // change: the failure names the screen.
        assert_eq!(
            LocalRuntime::Stopped.message(),
            "Local model server is stopped."
        );
        assert_eq!(
            LocalRuntime::Checking {
                model: "base".into()
            }
            .message(),
            "Checking or starting local model base..."
        );
        assert_eq!(
            LocalRuntime::Loading {
                model: "base".into()
            }
            .message(),
            "Loading local model base..."
        );
        assert_eq!(
            LocalRuntime::Ready {
                model: "base".into(),
                port: 8178
            }
            .message(),
            "Local model base is ready on port 8178."
        );
        assert_eq!(
            LocalRuntime::Failed {
                error: "the model file is missing".into()
            }
            .message(),
            "Local model could not start: the model file is missing. Retry dictation or choose a \
             model in Settings › Transcription."
        );
        assert_eq!(
            LocalRuntime::Exited {
                report: "exit code 3".into()
            }
            .message(),
            "Local model server exited. The next dictation or import will attempt recovery. exit \
             code 3"
        );
    }

    #[test]
    fn the_state_reaches_the_screen_as_data() {
        // Dictation words it plainly from this, not from the sentence above,
        // which carries the model id and port the page is not for.
        assert_eq!(
            serde_json::to_value(LocalRuntime::Ready {
                model: "base".into(),
                port: 8178
            })
            .unwrap(),
            serde_json::json!({ "state": "ready", "model": "base", "port": 8178 })
        );
        assert_eq!(
            serde_json::to_value(LocalRuntime::Failed { error: "x".into() }).unwrap(),
            serde_json::json!({ "state": "failed", "error": "x" })
        );
        assert_eq!(
            serde_json::to_value(LocalRuntime::Stopped).unwrap(),
            serde_json::json!({ "state": "stopped" })
        );
    }

    /// A runtime whose PID records live in a directory of the test process's
    /// own, never the application's.
    ///
    /// `LocalTranscription::new` points at the application's data directory,
    /// and the reaper cannot tell a live server from a stale one by design: it
    /// reads the record, confirms the process's command line, and terminates
    /// it. So a test that reaped through the production directory terminated
    /// the developer's running Whisper server, measured twice on 2026-09-25
    /// (KNEG-320). One directory per test process is enough; nothing here
    /// depends on the records being empty at the start of a test.
    fn local() -> LocalTranscription {
        static RECORDS: std::sync::OnceLock<tempfile::TempDir> = std::sync::OnceLock::new();
        let records = RECORDS.get_or_init(|| tempfile::tempdir().unwrap());
        let mut local =
            LocalTranscription::new(kw_network_policy::NetworkPolicy::new().unwrap(), None)
                .unwrap();
        local.pids = PidStore::new(records.path());
        local
    }

    async fn db() -> kw_storage::Db {
        kw_storage::Db::open_in_memory().await.unwrap()
    }

    /* ------------------------------ finding the speech first (KNEG-404) */

    /// The model as `tauri dev` and the tests see it: beside the manifest.
    fn manifest_dir() -> &'static std::path::Path {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
    }

    /// Speech tools under a temporary sidecar directory, with or without the
    /// finder installed there.
    fn tools(
        model: Option<std::path::PathBuf>,
        finder_installed: bool,
    ) -> (tempfile::TempDir, SpeechTools) {
        let dir = tempfile::tempdir().unwrap();
        let sidecars = dir.path().join("sidecars");
        if finder_installed {
            let whisper = sidecars.join(SIDECAR);
            std::fs::create_dir_all(&whisper).unwrap();
            std::fs::write(
                whisper.join(format!("{VAD_TOOL}{}", std::env::consts::EXE_SUFFIX)),
                b"",
            )
            .unwrap();
        }
        let tools = SpeechTools::new(model, &sidecars, dir.path().join("scratch"));
        (dir, tools)
    }

    #[test]
    fn the_bundled_vad_model_is_the_pinned_file() {
        // The file in `resources/` is what `tauri.conf.json` bundles, and in
        // a `tauri dev` build the resource directory is beside it.
        assert_eq!(
            bundled_vad_model(Some(manifest_dir())),
            Some(manifest_dir().join("resources").join(VAD_MODEL_FILE))
        );
    }

    #[test]
    fn a_vad_model_that_is_missing_or_not_the_pinned_one_is_not_used() {
        assert_eq!(bundled_vad_model(None), None);
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(bundled_vad_model(Some(dir.path())), None);
        std::fs::create_dir(dir.path().join("resources")).unwrap();
        std::fs::write(
            dir.path().join("resources").join(VAD_MODEL_FILE),
            b"not silero",
        )
        .unwrap();
        assert_eq!(bundled_vad_model(Some(dir.path())), None);
    }

    #[test]
    fn a_vad_model_on_a_path_the_finder_cannot_open_is_not_used() {
        // The right bytes, under a directory the finder exits 127 on.
        let dir = tempfile::tempdir().unwrap();
        let resources = dir.path().join("\u{c5}sa \u{f6}").join("resources");
        std::fs::create_dir_all(&resources).unwrap();
        std::fs::copy(
            manifest_dir().join("resources").join(VAD_MODEL_FILE),
            resources.join(VAD_MODEL_FILE),
        )
        .unwrap();
        assert_eq!(bundled_vad_model(resources.parent()), None);
    }

    #[test]
    fn long_recordings_find_their_speech_first_and_dictation_never_does() {
        use kw_audio_core::vad::Context;
        let (_dir, tools) = tools(Some("silero.bin".into()), true);
        assert!(tools.finder(Context::Meeting).is_some());
        assert!(tools.finder(Context::NoteRecording).is_some());
        assert!(
            tools.finder(Context::Dictation).is_none(),
            "a pause-heavy dictation stripped to nothing returns the vocabulary prompt"
        );
    }

    #[test]
    fn without_the_model_or_the_finder_a_recording_is_sent_whole() {
        use kw_audio_core::vad::Context;
        let (_dir, no_model) = tools(None, true);
        assert!(no_model.finder(Context::Meeting).is_none());
        let (_dir, no_finder) = tools(Some("silero.bin".into()), false);
        assert!(no_finder.finder(Context::Meeting).is_none());
    }

    #[test]
    fn the_finder_is_looked_for_beside_the_installed_server() {
        // `kw_sidecar::locate` finds the server at <sidecars>/whisper/, and the
        // finder is extracted from the same archive into the same directory.
        let tools = SpeechTools::new(None, std::path::Path::new("C:/sidecars"), "s".into());
        assert_eq!(
            tools.tool,
            std::path::Path::new("C:/sidecars")
                .join("whisper")
                .join(format!("{VAD_TOOL}{}", std::env::consts::EXE_SUFFIX))
        );
    }

    /* --------------------------------- the model a meeting starts (KNEG-387) */

    #[test]
    fn a_meeting_starts_the_remembered_model_while_it_is_still_installed() {
        let installed = ["tiny".to_owned(), "base".to_owned()];
        assert_eq!(
            meeting_model(Some("base"), &installed).as_deref(),
            Some("base")
        );
    }

    #[test]
    fn a_meeting_falls_back_to_the_first_installed_model() {
        // Dictation's route is not consulted: someone dictating to a cloud
        // service can still have chosen the local provider for meetings.
        let installed = ["tiny".to_owned(), "base".to_owned()];
        assert_eq!(
            meeting_model(Some("large"), &installed).as_deref(),
            Some("tiny")
        );
        assert_eq!(meeting_model(None, &installed).as_deref(), Some("tiny"));
        assert_eq!(
            meeting_model(Some("  "), &installed).as_deref(),
            Some("tiny")
        );
    }

    #[test]
    fn with_no_model_installed_a_meeting_has_nothing_to_start() {
        assert_eq!(meeting_model(Some("base"), &[]), None);
    }

    /* ------------------------------------------------ the sidecar guard */

    fn installed() -> kw_sidecar::Installation {
        kw_sidecar::Installation::Installed {
            executable: std::path::PathBuf::from("sidecars/whisper/whisper-server.exe"),
            origin: "downloaded",
        }
    }

    /// The regression that made every whisper model after the first
    /// uninstallable.
    ///
    /// `install` ends by replacing the sidecar directory, and by the time a
    /// user presses *Install* `resume_last_used` has started
    /// `whisper-server.exe` out of it. On Windows that directory cannot be
    /// removed while the executable inside it is running, so an unconditional
    /// reinstall returned an error before the model was ever downloaded.
    #[cfg(windows)]
    #[tokio::test]
    #[ignore = "starts installed Whisper base model with isolated PID records and fixture audio"]
    async fn native_concurrent_recovery_transcribes_after_process_loss() {
        use kw_transcription_core::local::LocalTranscriber as _;
        let directory = tempfile::tempdir().unwrap();
        let mut runtime = local();
        runtime.pids = PidStore::new(directory.path());
        let runtime = Arc::new(runtime);
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
        kw_storage::settings::set(db.pool(), LOCAL_MODEL_KEY, &"base")
            .await
            .unwrap();
        let worker = Arc::clone(&runtime);
        let outcome = tokio::spawn(async move {
            let (first, second) = tokio::join!(worker.start_model("base"), worker.start_model("base"));
            assert_eq!(first.as_ref().unwrap(), second.as_ref().unwrap());
            let original_pid = worker.pids.read(SIDECAR).unwrap().unwrap().pid;
            let old = worker.running.lock().await.take().unwrap();
            old.stop(&worker.pids).await.unwrap();
            // Keep the recorded port, reproducing a dead child between operations.
            let revive = revive_handle(&worker, &db);
            let recorded = worker.port.read().unwrap().to_owned();
            let (first, second) = tokio::join!(ensure_whisper(&worker.policy, recorded, revive.as_ref()), ensure_whisper(&worker.policy, recorded, revive.as_ref()));
            let port = first.unwrap();
            assert_eq!(port, second.unwrap());
            let replacement_pid = worker.pids.read(SIDECAR).unwrap().unwrap().pid;
            assert_ne!(original_pid, replacement_pid);
            let wav = include_bytes!("../../../../crates/transcription-core/tests/fixtures/speech-16k-mono.wav").to_vec();
            let result = kw_transcription_core::local::WhisperServer::new(worker.policy.clone(), port, Some("base".to_owned())).transcribe(wav, None, None).await.unwrap();
            assert!(matches!(result, kw_transcription_core::local::Inference::Text(ref text) | kw_transcription_core::local::Inference::Timed { ref text, .. } if !text.is_empty()));
            println!("Concurrent recovery: PID {original_pid} replaced by {replacement_pid}; both callers shared the replacement; fixture transcription succeeded.");
        }).await;
        runtime.stop().await;
        outcome.unwrap();
    }

    #[tokio::test]
    async fn overlapping_starts_share_the_completed_attempt_including_failure() {
        let local = Arc::new(local());
        let gate = local.operation.lock().await;
        let one = Arc::clone(&local);
        let two = Arc::clone(&local);
        let first = tokio::spawn(async move { one.start_model("fixture-model").await });
        let second = tokio::spawn(async move { two.start_model("fixture-model").await });
        tokio::task::yield_now().await;
        *local.last_start.lock().unwrap() = Some((
            std::time::Instant::now(),
            "fixture-model".to_owned(),
            Err("same failed startup".to_owned()),
        ));
        drop(gate);
        assert_eq!(first.await.unwrap(), Err("same failed startup".to_owned()));
        assert_eq!(second.await.unwrap(), Err("same failed startup".to_owned()));
    }

    /// KNEG-388. A dictation, import or local meeting that needs the server
    /// while the user switches from model A to B revives it. The revive reads
    /// the remembered model, still A, because B is remembered only once it is
    /// up. It used to wait for B, stop it and start A, leaving A running and B
    /// stored. Now a revive that waited behind a start that succeeded uses it.
    #[tokio::test]
    async fn a_revive_that_waited_behind_a_switch_keeps_the_model_switched_to() {
        let local = Arc::new(local());
        let gate = local.operation.lock().await;
        let reviver = Arc::clone(&local);
        let revive = tokio::spawn(async move { reviver.revive_model("model-a").await });
        tokio::task::yield_now().await;
        // The user's switch to B, finishing while the revive waits.
        *local.last_start.lock().unwrap() =
            Some((std::time::Instant::now(), "model-b".to_owned(), Ok(8178)));
        drop(gate);
        assert_eq!(revive.await.unwrap(), Ok(8178));
        assert_eq!(
            local
                .last_start
                .lock()
                .unwrap()
                .as_ref()
                .map(|(_, model, _)| model.clone()),
            Some("model-b".to_owned()),
            "the revive must not have started another model over B"
        );
    }

    /// A revive accepts another model's start only when it succeeded: B
    /// failing is no server to use, so the revive starts its own.
    #[tokio::test]
    async fn a_revive_does_not_take_another_models_failure_as_its_own() {
        let local = Arc::new(local());
        let gate = local.operation.lock().await;
        let reviver = Arc::clone(&local);
        let revive = tokio::spawn(async move { reviver.revive_model("model-a").await });
        tokio::task::yield_now().await;
        *local.last_start.lock().unwrap() = Some((
            std::time::Instant::now(),
            "model-b".to_owned(),
            Err("model B failed".to_owned()),
        ));
        drop(gate);
        assert_ne!(revive.await.unwrap(), Err("model B failed".to_owned()));
    }

    /// The user's own switch is unchanged: choosing C while B starts must
    /// never hand back B's server.
    #[tokio::test]
    async fn a_user_switch_does_not_accept_another_models_start() {
        let local = Arc::new(local());
        let gate = local.operation.lock().await;
        let switcher = Arc::clone(&local);
        let switch = tokio::spawn(async move { switcher.start_model("model-c").await });
        tokio::task::yield_now().await;
        *local.last_start.lock().unwrap() =
            Some((std::time::Instant::now(), "model-b".to_owned(), Ok(8178)));
        drop(gate);
        assert_ne!(switch.await.unwrap(), Ok(8178));
    }

    #[test]
    fn a_route_switch_is_in_progress_exactly_while_a_guard_is_held() {
        let local = local();
        let dictation_sees = local.shared().switching;
        assert!(!dictation_sees.in_progress());
        let first = local.switching.begin();
        assert!(
            dictation_sees.in_progress(),
            "the clone must see the switch"
        );
        let second = local.switching.begin();
        drop(first);
        assert!(
            dictation_sees.in_progress(),
            "a second switch is still running"
        );
        drop(second);
        assert!(!dictation_sees.in_progress());
    }

    #[tokio::test]
    async fn absent_or_dead_port_attempts_revival_once() {
        #[derive(Debug)]
        struct Reviver(std::sync::atomic::AtomicUsize);
        #[async_trait::async_trait]
        impl Revive for Reviver {
            async fn revive(&self) -> Result<u16, String> {
                self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                Err("fixture startup failed".to_owned())
            }
        }
        let revive = Reviver(std::sync::atomic::AtomicUsize::new(0));
        let policy = kw_network_policy::NetworkPolicy::new().unwrap();
        assert!(ensure_whisper(&policy, None, &revive).await.is_err());
        assert_eq!(revive.0.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    #[test]
    fn an_installed_sidecar_is_not_reinstalled() {
        assert!(!sidecar_needed(&installed()));
    }

    #[test]
    fn a_missing_sidecar_is_installed() {
        assert!(sidecar_needed(&kw_sidecar::Installation::NotInstalled {
            looked_in: vec![std::path::PathBuf::from("sidecars")],
        }));
    }

    /// A directory with no executable in it is an interrupted extraction, not
    /// an install. Nothing is running out of it, so replacing it is safe --
    /// which is why this is the one existing-directory case that still
    /// reinstalls.
    #[test]
    fn a_half_extracted_sidecar_is_installed_again() {
        assert!(sidecar_needed(&kw_sidecar::Installation::Incomplete {
            dir: std::path::PathBuf::from("sidecars/whisper"),
            expected: std::path::PathBuf::from("sidecars/whisper/whisper-server.exe"),
        }));
    }

    /* ---------------------------------------------- freshness on the wire */

    fn digest(hex: &str) -> kw_sidecar::Sha256Digest {
        kw_sidecar::Sha256Digest::parse(hex).unwrap()
    }

    const PINNED: &str = "c2a4b60edb11f7e11a9191ffb50929535527d4d91c9903dbe3e554583bbbc63d";
    const OTHER: &str = "1111111111111111111111111111111111111111111111111111111111111111";

    fn wire(freshness: &kw_sidecar::Freshness) -> serde_json::Value {
        serde_json::to_value(SidecarFreshness::of(freshness)).unwrap()
    }

    #[test]
    fn a_stale_sidecar_sends_both_digests_so_a_report_can_be_checked() {
        let json = wire(&kw_sidecar::Freshness::Stale {
            installed: digest(OTHER),
            expected: digest(PINNED),
        });
        assert_eq!(json["state"], "stale");
        assert_eq!(json["installed"], OTHER);
        assert_eq!(json["expected"], PINNED);
    }

    /// The state the machine this was written on is in, carried all the way to
    /// the wire.
    ///
    /// `unknown` must not arrive at the screen as either of the other two.
    /// Rounding it to `current` would excuse a binary the manifest has moved
    /// past; rounding it to `stale` would claim a comparison nobody made.
    #[test]
    fn an_unrecorded_sidecar_is_unknown_on_the_wire_and_says_why() {
        let json = wire(&kw_sidecar::Freshness::Unknown(
            kw_sidecar::Unrecorded::NoRecord,
        ));
        assert_eq!(json["state"], "unknown");
        assert_ne!(json["state"], "stale");
        assert_ne!(json["state"], "current");
        assert!(
            json["why"]
                .as_str()
                .unwrap_or_default()
                .contains("before this application started recording"),
            "{json}"
        );
    }

    #[test]
    fn a_current_sidecar_and_a_missing_one_are_different_wire_states() {
        assert_eq!(wire(&kw_sidecar::Freshness::Current)["state"], "current");
        assert_eq!(
            wire(&kw_sidecar::Freshness::NotInstalled)["state"],
            "not-installed"
        );
    }

    #[test]
    fn a_sidecar_nothing_is_published_for_is_reported_as_not_installed() {
        // `Catalog::resolve` fails on a name the manifest does not carry.
        // Answering anything but `NotInstalled` there would be a staleness
        // claim about a binary that does not exist for this platform.
        let dir = tempfile::tempdir().unwrap();
        let catalog = kw_sidecar::Catalog::shipped().unwrap();
        assert_eq!(
            sidecar_freshness(&catalog, "no-such-sidecar", dir.path()),
            kw_sidecar::Freshness::NotInstalled
        );
    }

    /// A catalogue with a published asset for every target the application
    /// supports.
    ///
    /// Product availability is not a testing invariant: whisper is deliberately
    /// not published on macOS today, which made the old positive controls return
    /// before testing anything on that CI job. This fixture pins synthetic
    /// assets for all five targets, so `Catalog::resolve`, `locate`, record I/O,
    /// and the freshness decision execute on every supported host.
    fn freshness_catalog() -> Catalog {
        fn asset(executable: &str) -> serde_json::Value {
            serde_json::json!({
                "availability": "published",
                "url": "https://example.invalid/fake-sidecar",
                "archive": "bare",
                "sha256": PINNED,
                "sizeBytes": 2,
                "maxExtractedSizeBytes": 2,
                "executableInArchive": executable,
            })
        }

        let document = serde_json::json!({
            "schemaVersion": kw_sidecar::catalog::SCHEMA_VERSION,
            "sidecars": [{
                "name": SIDECAR,
                "description": "Synthetic freshness fixture",
                "source": {
                    "repo": "example.invalid/fake",
                    "release": "test",
                    "pinnedBecause": "Deterministic cross-platform freshness coverage",
                },
                "transport": { "kind": "stdio" },
                "commandFragments": ["fake-sidecar"],
                "targets": {
                    "windows-x86_64": asset("fake-sidecar.exe"),
                    "linux-x86_64": asset("fake-sidecar"),
                    "linux-aarch64": asset("fake-sidecar"),
                    "macos-x86_64": asset("fake-sidecar"),
                    "macos-aarch64": asset("fake-sidecar"),
                },
            }],
        });
        Catalog::parse(&document.to_string()).unwrap()
    }

    struct FreshnessFixture {
        root: tempfile::TempDir,
        catalog: Catalog,
    }

    impl FreshnessFixture {
        fn new() -> Self {
            Self {
                root: tempfile::tempdir().unwrap(),
                catalog: freshness_catalog(),
            }
        }

        fn install_dir(&self) -> std::path::PathBuf {
            self.root.path().join(SIDECAR)
        }

        fn resolved(&self) -> (&kw_sidecar::SidecarSpec, &kw_sidecar::Asset) {
            self.catalog.resolve(SIDECAR).unwrap()
        }

        fn install_executable(&self) {
            let install = self.install_dir();
            let executable = self.resolved().1.executable_name();
            std::fs::create_dir_all(&install).unwrap();
            std::fs::write(install.join(executable), b"MZ").unwrap();
        }

        fn record(&self) -> kw_sidecar::InstallRecord {
            let (spec, asset) = self.resolved();
            kw_sidecar::InstallRecord::at(spec, asset, 1)
        }

        fn write_record(&self, record: &kw_sidecar::InstallRecord) {
            kw_sidecar::record::write(&self.install_dir(), record).unwrap();
        }

        fn freshness(&self) -> kw_sidecar::Freshness {
            sidecar_freshness(&self.catalog, SIDECAR, self.root.path())
        }
    }

    #[test]
    fn sidecar_freshness_reports_a_fake_missing_install_as_not_installed() {
        let fixture = FreshnessFixture::new();
        assert_eq!(fixture.freshness(), kw_sidecar::Freshness::NotInstalled);
    }

    #[test]
    fn sidecar_freshness_reports_a_fake_pinned_install_as_current() {
        let fixture = FreshnessFixture::new();
        fixture.install_executable();
        fixture.write_record(&fixture.record());
        assert_eq!(fixture.freshness(), kw_sidecar::Freshness::Current);
    }

    #[test]
    fn sidecar_freshness_reports_a_fake_older_install_as_stale() {
        let fixture = FreshnessFixture::new();
        fixture.install_executable();
        let mut record = fixture.record();
        record.asset_sha256 = digest(OTHER);
        fixture.write_record(&record);

        assert_eq!(
            fixture.freshness(),
            kw_sidecar::Freshness::Stale {
                installed: digest(OTHER),
                expected: digest(PINNED),
            }
        );
    }

    #[test]
    fn sidecar_freshness_reports_a_fake_install_without_a_record_as_unknown() {
        let fixture = FreshnessFixture::new();
        fixture.install_executable();
        assert_eq!(
            fixture.freshness(),
            kw_sidecar::Freshness::Unknown(kw_sidecar::Unrecorded::NoRecord)
        );
    }

    #[test]
    fn sidecar_freshness_reports_a_fake_unreadable_record_as_unknown() {
        let fixture = FreshnessFixture::new();
        fixture.install_executable();
        std::fs::write(
            fixture.install_dir().join(kw_sidecar::record::RECORD_FILE),
            b"not json",
        )
        .unwrap();

        match fixture.freshness() {
            kw_sidecar::Freshness::Unknown(kw_sidecar::Unrecorded::Unreadable { detail }) => {
                assert!(
                    !detail.is_empty(),
                    "the parse failure must remain actionable"
                );
            }
            other => panic!("expected an unreadable record, got {other:?}"),
        }
    }

    #[test]
    fn sidecar_freshness_reports_a_fake_newer_record_schema_as_unknown() {
        let fixture = FreshnessFixture::new();
        fixture.install_executable();
        let mut record = fixture.record();
        record.schema = kw_sidecar::record::SCHEMA + 1;
        fixture.write_record(&record);

        assert_eq!(
            fixture.freshness(),
            kw_sidecar::Freshness::Unknown(kw_sidecar::Unrecorded::UnsupportedSchema {
                found: kw_sidecar::record::SCHEMA + 1,
            })
        );
    }

    #[test]
    fn sidecar_freshness_reports_a_fake_wrong_sidecar_record_as_unknown() {
        let fixture = FreshnessFixture::new();
        fixture.install_executable();
        let mut record = fixture.record();
        record.sidecar = "some-other-sidecar".to_owned();
        fixture.write_record(&record);

        assert_eq!(
            fixture.freshness(),
            kw_sidecar::Freshness::Unknown(kw_sidecar::Unrecorded::DifferentSidecar {
                recorded: "some-other-sidecar".to_owned(),
            })
        );
    }

    /* --------------------------------------------------------- refreshing */

    #[test]
    fn a_refresh_that_failed_with_nothing_running_reports_only_the_failure() {
        // No server was stopped, so there is no second fact to report and
        // adding one would be noise.
        assert_eq!(
            refresh_failure("the download was refused.", &Restart::NothingWasRunning),
            "the download was refused."
        );
    }

    /// The reason a refresh reports two things rather than one.
    ///
    /// A refresh stops the server before replacing it. If the download then
    /// fails, the user is left with the old binary and — unless it was started
    /// again — nothing serving it. An install error on its own does not say
    /// which of those two states they are in.
    #[test]
    fn a_refresh_that_failed_says_the_old_server_came_back() {
        let message = refresh_failure(
            "the download was refused.",
            &Restart::Restarted("tiny".to_owned()),
        );
        assert!(message.contains("the download was refused."), "{message}");
        assert!(message.contains("started again"), "{message}");
        assert!(message.contains("tiny"), "{message}");
    }

    #[test]
    fn a_refresh_that_failed_twice_says_dictation_is_down() {
        // The worst outcome, and the one that must never be summarised as
        // "install failed": the server that was working when the button was
        // pressed is now gone.
        let message = refresh_failure(
            "the download was refused.",
            &Restart::Failed {
                model: "tiny".to_owned(),
                error: "port 8178 is taken".to_owned(),
            },
        );
        assert!(message.contains("could not be started again"), "{message}");
        assert!(message.contains("port 8178 is taken"), "{message}");
    }

    #[test]
    fn the_outcome_of_a_restart_is_reported_as_one_thing_or_the_other() {
        // `restarted` and `restart_error` are never both set, and never both
        // clear when something was running. A caller reading one without the
        // other must not be able to see a half-truth.
        let restarted = Restart::Restarted("tiny".to_owned());
        assert_eq!(restarted.restarted().map(String::as_str), Some("tiny"));
        assert_eq!(restarted.error(), None);

        let failed = Restart::Failed {
            model: "tiny".to_owned(),
            error: "no".to_owned(),
        };
        assert_eq!(failed.restarted(), None);
        assert_eq!(failed.error().map(String::as_str), Some("no"));

        assert_eq!(Restart::NothingWasRunning.restarted(), None);
        assert_eq!(Restart::NothingWasRunning.error(), None);
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
        let SharedLocal { port, model, .. } = local.shared();

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
        // The first launch on a new machine -- which `local()` guarantees by
        // giving the runtime a record directory of its own.
        local().reap_stale().await;
    }

    /// Pins the property `local()` exists for; see its doc comment for what
    /// happened without it.
    #[test]
    fn test_runtimes_never_see_the_applications_pid_records() {
        let production = PidStore::new(&paths::data_dir());
        assert_ne!(local().pids.dir(), production.dir());
        assert!(!local().pids.dir().starts_with(paths::data_dir()));
    }

    #[test]
    fn the_install_directory_no_longer_follows_the_model_path_rule() {
        // It used to, on the stated grounds that whisper-server loads its
        // sibling ggml libraries relative to its own directory and would choke
        // on a space or a non-ASCII character. Measured 2026-08-31: it does
        // not, and neither do sherpa-onnx-offline, llama-server or ffmpeg. So
        // sidecars stay where the user's data is, whatever the account is
        // called -- which also means they are not written to %ProgramData%,
        // where an unprivileged account may not be able to write at all.
        let dir = paths::sidecar_dir();
        assert!(!dir.fell_back);

        // The model directory is a different question and keeps the rule: a
        // path reported as usable has to actually be one.
        let models = paths::model_cache_dir();
        if !models.fell_back {
            assert!(kw_storage::paths::is_native_readable(&models.path));
        }
    }
}
