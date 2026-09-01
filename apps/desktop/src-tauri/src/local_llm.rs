//! The llama.cpp sidecar: text generation that never leaves the machine.
//!
//! `ChatRoute::Local` has pointed at `http://127.0.0.1:8221/v1` since phase 3
//! and nothing served it. This is the thing that serves it.
//!
//! It follows [`crate::local_model`] deliberately rather than being a second
//! lifecycle: allocate a port, start the child, poll it until it answers, and
//! publish the port only once it has. The two differ in three places and each
//! one is a decision:
//!
//! 1. **Models are gigabytes, not megabytes.** The largest whisper model is
//!    3 GB and the default is 148 MB; the recommended Qwen here is 5.9 GB. So
//!    the install path reports bytes and can be cancelled, and the screen shows
//!    a size before anyone starts.
//! 2. **It stops itself.** A whisper server holding 148 MB while idle is not
//!    worth a timer. A llama server holding 6 GB is. Five minutes, per the
//!    porting matrix.
//! 3. **The port is not fixed.** `port::allocate` picks from 8221–8240, so the
//!    hardcoded `8221` in `reasoning-core` would be wrong the moment a second
//!    process held it. [`LocalEndpoint`] carries the real one.
//!
//! A fourth difference arrived later and is not a lifecycle one: because these
//! files are the largest thing this application writes to disk and they sit in a
//! user-writable directory for months, what is known about an *installed* GGUF
//! is tracked separately in [`crate::gguf_check`]. A path that exists is not a
//! file that is still what was downloaded.

use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, RwLock};
use std::time::Duration;

use kw_network_policy::NetworkPolicy;
use kw_sidecar::process::{self, HttpHealth, Launch, Running, StartTimings};
use kw_sidecar::{catalog::Catalog, pid::PidStore, port};
use kw_storage::paths;
use serde::Serialize;
use tauri::{AppHandle, Emitter as _, State};
use tokio::sync::Mutex;
use ts_rs::TS;

use crate::gguf_check::{self, GgufIntegrity};

/// The sidecar this module runs.
const SIDECAR: &str = "llama";

/// Which model was last started, so a restart can bring it back.
pub const LOCAL_LLM_MODEL_KEY: &str = "reasoning.localModel";

/// Install progress, to the frontend.
pub const PROGRESS_EVENT: &str = "local-llm-progress";

/// How long a loaded model may sit unused before the server is stopped.
///
/// Five minutes, from `docs/PORTING_MATRIX.md`. The number is a trade between
/// holding several gigabytes of resident memory and a twenty-second reload, and
/// it is worth stating that the reload is the *cheaper* of the two on a laptop.
const IDLE_STOP: Duration = Duration::from_secs(5 * 60);

/// How often the idle watcher looks. Coarse on purpose: the deadline is five
/// minutes, so a thirty-second granularity costs nothing and wakes rarely.
const IDLE_TICK: Duration = Duration::from_secs(30);

/// Where a local completion goes, and whether anything wants one.
///
/// Cloned into `ModelCleanup`, so reading the port is also how "someone used
/// it" gets recorded — a separate `touch()` would be a call somebody forgets.
#[derive(Debug, Clone, Default)]
pub struct LocalEndpoint {
    port: Arc<RwLock<Option<u16>>>,
    /// Unix seconds of the last read. `0` means never.
    last_used: Arc<AtomicI64>,
}

impl LocalEndpoint {
    /// The port a local request should go to, recording the request.
    ///
    /// `None` when nothing is running, which callers must treat as "not
    /// available" rather than falling back to a default port. A request to a
    /// port nothing is listening on fails with a connection error that blames
    /// the network.
    #[must_use]
    pub fn port(&self) -> Option<u16> {
        let port = self.port.read().ok().and_then(|p| *p);
        if port.is_some() {
            self.last_used
                .store(chrono::Utc::now().timestamp(), Ordering::Relaxed);
        }
        port
    }

    fn set(&self, port: Option<u16>) {
        if let Ok(mut held) = self.port.write() {
            *held = port;
        }
        self.last_used
            .store(chrono::Utc::now().timestamp(), Ordering::Relaxed);
    }

    fn idle_for(&self) -> Duration {
        let last = self.last_used.load(Ordering::Relaxed);
        if last == 0 {
            return Duration::ZERO;
        }
        let elapsed = chrono::Utc::now().timestamp().saturating_sub(last);
        Duration::from_secs(elapsed.max(0).unsigned_abs())
    }
}

/// The llama.cpp server this application manages.
pub struct LocalLlm {
    catalog: Catalog,
    policy: NetworkPolicy,
    pids: PidStore,
    running: Mutex<Option<Running>>,
    endpoint: LocalEndpoint,
    model: RwLock<Option<String>>,
}

impl std::fmt::Debug for LocalLlm {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LocalLlm")
            .field("port", &self.endpoint.port.read().ok().and_then(|p| *p))
            .finish_non_exhaustive()
    }
}

impl LocalLlm {
    /// Parse the catalogue and prepare the runtime. Starts nothing.
    pub fn new(policy: NetworkPolicy) -> Result<Self, String> {
        Ok(Self {
            catalog: Catalog::shipped().map_err(|e| e.to_string())?,
            policy,
            pids: PidStore::new(&paths::data_dir()),
            running: Mutex::new(None),
            endpoint: LocalEndpoint::default(),
            model: RwLock::new(None),
        })
    }

    /// The handle the cleanup path reads.
    #[must_use]
    pub fn endpoint(&self) -> LocalEndpoint {
        self.endpoint.clone()
    }

    /// The port, without recording a use.
    ///
    /// For status screens. Going through [`LocalEndpoint::port`] would reset the
    /// idle clock every time anyone looked, so a settings window left open would
    /// hold six gigabytes indefinitely.
    #[must_use]
    pub fn running_port(&self) -> Option<u16> {
        self.endpoint.port.read().ok().and_then(|p| *p)
    }

    /// Which model the running server loaded.
    #[must_use]
    pub fn running_model(&self) -> Option<String> {
        self.model.read().ok().and_then(|m| m.clone())
    }

    /// Clean up a server a previous run left behind.
    ///
    /// The reaper reads the live process's command line first and refuses to
    /// signal anything it cannot prove is ours, so a reused process id is safe.
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
            other => tracing::info!(?other, "reaped a stale text-generation server"),
        }
    }

    /// Stop the server, if one is running.
    pub async fn stop(&self) {
        let mut held = self.running.lock().await;
        if let Some(running) = held.take() {
            running.stop(&self.pids).await.unwrap_or_else(|err| {
                tracing::warn!(error = %err, "could not stop the text-generation server cleanly");
            });
        }
        self.endpoint.set(None);
        if let Ok(mut model) = self.model.write() {
            *model = None;
        }
    }

    /// Start the server for one model, or return the port if it is already up.
    pub async fn start_model(&self, model_id: &str) -> Result<u16, String> {
        if let Some(port) = self.endpoint.port() {
            return Ok(port);
        }

        let (spec, asset) = self.catalog.resolve(SIDECAR).map_err(|e| e.to_string())?;

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
            .ok_or_else(|| installation.explain("the local text-generation server"))?
            .to_path_buf();

        let registry = kw_provider_registry::registry().map_err(|e| e.to_string())?;
        let model = find_model(registry, model_id)?;
        let path = gguf_path(&model.file_name);

        if !path.exists() {
            // Checked here rather than left to the server, whose failure for a
            // missing model is an exit during startup with the reason buried in
            // captured output.
            return Err(format!(
                "{} is not installed. Install it before starting the server.",
                model.name
            ));
        }

        // Two stat calls, and no hashing. **Hashing here was considered and
        // rejected**: reading 19.8 GB back before every start would turn a
        // twenty-second load into a ninety-second one and make starting the
        // server feel broken, which is the same trade `local_llm_status` refuses
        // for the same reason.
        //
        // Nor does a disagreement refuse the start. A moved timestamp is a
        // restore from backup as often as it is anything else, and refusing to
        // load a model that works — on evidence this deliberately calls weak —
        // is a worse failure than the one being guarded against. The screen
        // reports the state and offers a re-hash; this only makes sure the fact
        // reaches the log of a session that is about to load the file anyway.
        if let GgufIntegrity::Changed {
            recorded_bytes,
            bytes,
        } = gguf_check::integrity(&path)
        {
            tracing::warn!(
                model = %model_id,
                recorded_bytes,
                bytes,
                "this model has changed since it was checked at install; loading it anyway"
            );
        }

        let chosen = port::allocate(
            spec.port_range()
                .ok_or_else(|| format!("{} is a stdio child and serves no port", spec.name))?,
            &port::BindProbe,
        )
        .map_err(|e| e.to_string())?;

        let mut args = vec![
            "--model".to_owned(),
            path.display().to_string(),
            "--port".to_owned(),
            chosen.to_string(),
            // Loopback only, spelled out rather than relied on as a default.
            // `llama-server` binds 127.0.0.1 by default today; a future release
            // changing that would silently expose the machine's models.
            "--host".to_owned(),
            "127.0.0.1".to_owned(),
        ];

        if let Some(context) = model.context_length {
            // Capped: the registry lists context lengths up to 262 144, and
            // allocating the KV cache for that is several more gigabytes on top
            // of the weights. A user who wants the full window can raise it once
            // there is a control for it; a default that fails to allocate is
            // worse than a smaller one that works.
            args.push("--ctx-size".to_owned());
            args.push(context.min(8192).to_string());
        }

        if let Some(draft) = &model.draft {
            let draft_path = gguf_path(&draft.file_name);
            if draft_path.exists() {
                // Speculative decoding. Skipped rather than refused when the
                // drafter is absent: the model works without it, more slowly.
                args.push("--model-draft".to_owned());
                args.push(draft_path.display().to_string());
            }
        }

        let launch = Launch {
            executable,
            args,
            port: chosen,
        };

        let health = HttpHealth::new(self.policy.clone(), "/health");
        let running = process::start(spec, &launch, &self.pids, &health, StartTimings::default())
            .await
            .map_err(|e| e.to_string())?;

        // Published only once it has answered, so a request never reaches a
        // server still loading six gigabytes of weights.
        self.endpoint.set(Some(chosen));
        if let Ok(mut held) = self.model.write() {
            *held = Some(model_id.to_owned());
        }
        *self.running.lock().await = Some(running);

        Ok(chosen)
    }
}

/// A model's place on disk. One flat directory, keyed by the publisher's
/// filename, which is already unique across the registry.
fn gguf_path(file_name: &str) -> std::path::PathBuf {
    paths::model_cache_dir().path.join(file_name)
}

fn find_model<'a>(
    registry: &'a kw_provider_registry::Registry,
    id: &str,
) -> Result<&'a kw_provider_registry::LocalLlmModel, String> {
    registry
        .local_llm
        .iter()
        .flat_map(|p| p.models.iter())
        .find(|m| m.id == id)
        .ok_or_else(|| format!("{id:?} is not a model this build knows about"))
}

/// The download URL for a GGUF on Hugging Face, at the commit the registry pins.
///
/// Built here rather than stored in the registry because it is mechanical, and
/// because a URL in data is a URL somebody can point somewhere else. The host is
/// the same one the whisper models come from.
///
/// **The revision is the part that is not mechanical.** This used to interpolate
/// `main`, which is a branch pointer: the bytes behind it change whenever the
/// publisher pushes, so a digest measured against one commit would start failing
/// installs the moment upstream published anything at all -- correctly refusing,
/// for a reason that has nothing to do with the user or with the download.
/// [`LocalLlmModel::revision`] is a commit sha measured into `HF_REVISIONS` in
/// `tools/integration-audit/generate-provider-registry.mjs`, and the digest
/// beside it describes that commit's object. KNEG-103.
///
/// The whisper models get the same treatment a different way: their URL is
/// stored whole in the registry and the generator rewrites the revision into it,
/// because upstream's baseline supplies the URL and this one is composed.
fn gguf_url(repo: &str, revision: &str, file_name: &str) -> String {
    format!("https://huggingface.co/{repo}/resolve/{revision}/{file_name}")
}

/* --------------------------------------------------------------- idle stop */

/// Stop the server once nothing has used it for [`IDLE_STOP`].
///
/// Spawned per start rather than run for the process's lifetime, so the timer
/// exists exactly while there is something to stop.
fn watch_for_idle(llm: Arc<LocalLlm>, app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(IDLE_TICK).await;

            if llm.endpoint.port.read().ok().and_then(|p| *p).is_none() {
                // Stopped by hand, or never started. Nothing to watch.
                return;
            }

            if llm.endpoint.idle_for() >= IDLE_STOP {
                tracing::info!(
                    minutes = IDLE_STOP.as_secs() / 60,
                    "stopping the local text-generation server after being idle"
                );
                llm.stop().await;
                // Told, not silent: several gigabytes of loaded model going away
                // changes what the next request costs, and a screen that still
                // says "running" would be wrong.
                let _ = app.emit(PROGRESS_EVENT, LocalLlmProgress::StoppedWhenIdle);
                return;
            }
        }
    });
}

/* ------------------------------------------------------------------- status */

/// One local model on offer.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct LocalLlmModelStatus {
    pub id: String,
    pub name: String,
    /// The family it belongs to, e.g. `qwen`.
    pub provider: String,
    /// `None` when the registry does not say. Shown before anyone commits to a
    /// download that can be six gigabytes.
    pub size_bytes: Option<f64>,
    pub quantization: Option<String>,
    pub context_length: Option<f64>,
    pub recommended: bool,
    pub supports_thinking: bool,
    /// Whether the file is on disk and, if it is, what is still known about it.
    ///
    /// This replaced an `installed: bool`, and the replacement is the whole of
    /// KNEG-107. A boolean over `path.exists()` cannot tell a file that was
    /// hashed at install and has not moved since from one nothing has ever
    /// looked at, and it rendered both as "installed" — so a GGUF corrupted
    /// months after a successful install was loaded without complaint. The
    /// states, and exactly what each is worth, are [`GgufIntegrity`].
    pub integrity: GgufIntegrity,
    /// What integrity check this model can be held to, on the same terms the
    /// whisper screen already states. Shown before the button, for the reason
    /// the size is: a guarantee nobody was told about is a guarantee nobody can
    /// weigh. All thirty-five carry a digest; seven of the thirty-nine objects
    /// carry one this project measured, and the rest are the publisher's stated
    /// `lfs.oid`, which is a different claim.
    pub verified_by: crate::local_model::VerifiedBy,
    /// This model's speculative-decoding drafter, and what is known about it.
    /// `None` for the models without one.
    pub draft: Option<DraftStatus>,
}

/// A drafter alongside the main model.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct DraftStatus {
    pub file_name: String,
    pub size_bytes: Option<f64>,
    /// On the same terms as [`LocalLlmModelStatus::integrity`], and tracked
    /// apart from it because a drafter is a separate file that can rot on its
    /// own.
    pub integrity: GgufIntegrity,
    /// On the same terms as [`LocalLlmModelStatus::verified_by`]. Reported
    /// separately because a drafter is a different object in a different
    /// repository, and one of the two can be the weaker of the pair.
    pub verified_by: crate::local_model::VerifiedBy,
}

/// What the Intelligence > Local screen needs, with no network access.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct LocalLlmStatus {
    /// Whether a server binary is published for this platform.
    pub availability: crate::local_model::Availability,
    pub server_installed: bool,
    pub models: Vec<LocalLlmModelStatus>,
    /// The port the server is on, if it is running.
    pub port: Option<f64>,
    /// Which model that server loaded.
    pub running_model: Option<String>,
    pub install_dir: String,
    /// Minutes of inactivity before the server stops itself.
    pub idle_stop_minutes: f64,
}

/// Progress of an install, and the one lifecycle event worth reporting.
///
/// Named for its sidecar rather than `InstallProgress`, because
/// `local_model` already has one of those and a TypeScript file cannot hold
/// two declarations of the same name. `ipc_types` refuses the collision.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(
    tag = "stage",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum LocalLlmProgress {
    Server {
        written: f64,
        total: Option<f64>,
    },
    Model {
        id: String,
        written: f64,
        total: Option<f64>,
    },
    /// The speculative-decoding drafter, downloaded after the model it belongs
    /// to. Named separately so a second progress bar after the first does not
    /// look like the first one restarting.
    Draft {
        id: String,
        written: f64,
        total: Option<f64>,
    },
    Extracting,
    /// Hashing a finished download.
    ///
    /// Its own state because it is not instant. The check reads the whole file
    /// back, and the largest model here is 20 GB, so a rename that used to be
    /// the last thing to happen now has a minute of work in front of it. A
    /// progress line that stops moving reads as a hang.
    Verifying {
        id: String,
    },
    Done {
        id: String,
        /// What was actually checked, or `None` when the files were already on
        /// disk and this install re-checked nothing.
        ///
        /// The weaker of the model's verdict and its drafter's, where there is a
        /// drafter; see [`crate::local_model::VerifiedBy::weaker`].
        verified_by: Option<crate::local_model::VerifiedBy>,
    },
    /// The idle timer fired. Not an install event, but it arrives on the same
    /// channel because it changes the same screen.
    StoppedWhenIdle,
}

/// One registry entry as the screen needs it, against a given model directory.
///
/// Split out of [`local_llm_status`] rather than inlined there, and the reason
/// is the one `media.rs` records: nothing outside a running application can
/// construct a `tauri::AppHandle` or a `State`, so anything reachable only
/// through the command is untestable by construction. Taking `dir` as an
/// argument moves the whole mapping -- including which of the four integrity
/// states each file lands in -- to the near side of that line, and leaves the
/// command holding one call to `paths::model_cache_dir()`.
///
/// It was not written this way. It was inlined, and a mutation that made every
/// row report `NotInstalled` was the only one of eighteen that no test caught.
fn model_status(
    provider_id: &str,
    dir: &std::path::Path,
    m: &kw_provider_registry::LocalLlmModel,
) -> LocalLlmModelStatus {
    LocalLlmModelStatus {
        id: m.id.clone(),
        name: m.name.clone(),
        provider: provider_id.to_owned(),
        size_bytes: m.size_bytes.map(|b| b as f64),
        quantization: m.quantization.clone(),
        context_length: m.context_length.map(|c| c as f64),
        recommended: m.recommended,
        supports_thinking: m.supports_thinking,
        // Two stat calls and, for an installed model, one small read of the
        // record beside it. No hashing: see `GgufIntegrity`.
        integrity: gguf_check::integrity(&dir.join(&m.file_name)),
        verified_by: verified_by(
            SizeCheck::of(m.size_bytes, m.size_measured),
            &DigestCheck::of(m.sha256.as_deref(), m.sha256_measured),
        ),
        draft: m.draft.as_ref().map(|d| DraftStatus {
            file_name: d.file_name.clone(),
            size_bytes: d.size_bytes.map(|b| b as f64),
            integrity: gguf_check::integrity(&dir.join(&d.file_name)),
            verified_by: verified_by(
                SizeCheck::of(d.size_bytes, d.size_measured),
                &DigestCheck::of(d.sha256.as_deref(), d.sha256_measured),
            ),
        }),
    }
}

#[tauri::command]
pub async fn local_llm_status(llm: State<'_, Arc<LocalLlm>>) -> Result<LocalLlmStatus, String> {
    let registry = kw_provider_registry::registry().map_err(|e| e.to_string())?;
    let availability = crate::local_model::Availability::of(&llm.catalog, SIDECAR);

    // `resolve` fails on a platform with no published binary, which is not an
    // error to report: it is the answer, and `availability` already carries it.
    let sidecars = paths::sidecar_dir();
    let server_installed = llm
        .catalog
        .resolve(SIDECAR)
        .ok()
        .and_then(|(_, asset)| {
            kw_sidecar::locate(
                SIDECAR,
                asset,
                &kw_sidecar::SearchPath {
                    bundled: None,
                    downloaded: sidecars.path.clone(),
                },
                &kw_sidecar::RealFs,
            )
            .executable()
            .map(|_| ())
        })
        .is_some();

    let dir = paths::model_cache_dir().path;
    let models = registry
        .local_llm
        .iter()
        .flat_map(|p| {
            p.models
                .iter()
                .map(|m| model_status(&p.id, &dir, m))
                .collect::<Vec<_>>()
        })
        .collect();

    Ok(LocalLlmStatus {
        availability,
        server_installed,
        models,
        // Read directly rather than through `LocalEndpoint::port`, which would
        // record a use and keep the idle timer from ever firing while a settings
        // screen is open.
        port: llm.running_port().map(f64::from),
        running_model: llm.running_model(),
        install_dir: dir.display().to_string(),
        idle_stop_minutes: (IDLE_STOP.as_secs() / 60) as f64,
    })
}

/// Download the server and one model.
///
/// Explicit: nothing downloads on first launch. This runs only because someone
/// asked for it, and the sizes are on screen before they do.
#[tauri::command]
pub async fn local_llm_install(
    app: AppHandle,
    llm: State<'_, Arc<LocalLlm>>,
    model_id: String,
) -> Result<(), String> {
    let (spec, asset) = llm.catalog.resolve(SIDECAR).map_err(|e| e.to_string())?;
    let registry = kw_provider_registry::registry().map_err(|e| e.to_string())?;
    let model = find_model(registry, &model_id)?;

    let emit = |progress: LocalLlmProgress| {
        let _ = app.emit(PROGRESS_EVENT, progress);
    };

    // --- the server binary ---
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

    if installation.executable().is_none() {
        kw_sidecar::install::install(&llm.policy, spec, asset, &sidecars.path, &mut |p| match p {
            kw_sidecar::install::Progress::Downloading { written, total } => {
                emit(LocalLlmProgress::Server {
                    written: written as f64,
                    total: total.map(|t| t as f64),
                });
            }
            kw_sidecar::install::Progress::Extracting => emit(LocalLlmProgress::Extracting),
            _ => {}
        })
        .await
        .map_err(|e| e.to_string())?;
    }

    // --- the weights ---
    let dir = paths::model_cache_dir().path;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;

    let checked = download_gguf(
        &llm.policy,
        &gguf_url(&model.hf_repo, &model.revision, &model.file_name),
        &gguf_path(&model.file_name),
        SizeCheck::of(model.size_bytes, model.size_measured),
        &DigestCheck::of(model.sha256.as_deref(), model.sha256_measured),
        &mut |written, total| {
            emit(LocalLlmProgress::Model {
                id: model_id.clone(),
                written: written as f64,
                total: total.map(|t| t as f64),
            });
        },
        &mut || {
            emit(LocalLlmProgress::Verifying {
                id: model_id.clone(),
            });
        },
    )
    .await?;

    // --- the drafter, if this model has one ---
    let mut verdict = checked;
    if let Some(draft) = &model.draft {
        let draft_checked = download_gguf(
            &llm.policy,
            &gguf_url(&draft.hf_repo, &draft.revision, &draft.file_name),
            &gguf_path(&draft.file_name),
            SizeCheck::of(draft.size_bytes, draft.size_measured),
            &DigestCheck::of(draft.sha256.as_deref(), draft.sha256_measured),
            &mut |written, total| {
                emit(LocalLlmProgress::Draft {
                    id: model_id.clone(),
                    written: written as f64,
                    total: total.map(|t| t as f64),
                });
            },
            &mut || {
                emit(LocalLlmProgress::Verifying {
                    id: model_id.clone(),
                });
            },
        )
        .await?;

        // The install is as verified as its weakest half. `None` from either
        // means that half was already on disk and this install checked nothing
        // about it, so it cannot strengthen the verdict and must not weaken it
        // either -- the other half's claim is still true.
        verdict = match (verdict, draft_checked) {
            (Some(model), Some(draft)) => Some(model.weaker(draft)),
            (Some(one), None) | (None, Some(one)) => Some(one),
            (None, None) => None,
        };
    }

    emit(LocalLlmProgress::Done {
        id: model_id,
        verified_by: verdict,
    });
    Ok(())
}

/// How strictly a finished download is held to the length the registry carries.
///
/// Three states, because the registry's byte counts have three provenances and
/// they are not equally trustworthy. `config/provider-registry.json` says which
/// one each number is with a `sizeMeasured` flag; the measurements themselves
/// are `LOCAL_LLM_PINS` in
/// `tools/integration-audit/generate-provider-registry.mjs`, beside the digests
/// [`DigestCheck`] reads.
///
/// **Why exactness became safe.** Until the sizes were measured, every model was
/// checked within 5%, and it had to be: upstream's figures were *derived* from a
/// two-significant-figure "GB" label rather than observed, and eighteen of the
/// thirty-five were 6.7–7.9% high because that base-10 label had been multiplied
/// by 2^30 instead of 10^9. A tolerance is the right answer to a rounded number
/// and the wrong answer to a measured one — it was wide enough to hide the
/// defect and still not wide enough to install those eighteen, which downloaded
/// in full and were deleted on arrival. Against a figure read from the
/// publisher's own object size there is nothing left to round, so the file
/// either has that many bytes or is not that file.
///
/// **What exactness costs.** A publisher who re-uploads a model — bartowski
/// re-quantising against a newer llama.cpp, say — now fails the install loudly
/// instead of passing silently. That is the trade taken deliberately: a silent
/// pass means the user runs weights nobody in this project ever measured, and a
/// loud failure means somebody re-runs the measurement. The `#[ignore]`d
/// `the_measured_sizes_still_match_hugging_face` test below is how that gets
/// noticed before a user meets it. Two models were already 2 240 bytes adrift
/// from their declared size when this was measured, so the case is not
/// hypothetical.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SizeCheck {
    /// Measured against the publisher's own object size, so the download must
    /// match it to the byte.
    Exact(u64),
    /// Upstream's declaration, never measured. Held to
    /// [`kw_transcription_core::models::within_tolerance`]'s 5%, which is what
    /// every model used to get. Reached only by a model added to the registry
    /// without being measured.
    Approximate(u64),
    /// The registry carries no length. Nothing to check, and nothing is claimed.
    Unknown,
}

impl SizeCheck {
    /// Read the check out of a registry entry.
    ///
    /// A size flagged as measured but absent is read as [`Self::Unknown`] rather
    /// than trusted: the generator cannot produce that pair, and if it ever did,
    /// the safe reading of "measured, value missing" is that nothing is known.
    fn of(size_bytes: Option<u64>, measured: bool) -> Self {
        match (size_bytes, measured) {
            (Some(bytes), true) => Self::Exact(bytes),
            (Some(bytes), false) => Self::Approximate(bytes),
            (None, _) => Self::Unknown,
        }
    }

    /// Whether a finished download of `actual` bytes is the file that was asked
    /// for, and if not, why — phrased in the terms the check was made in.
    ///
    /// "about 5 027 783 488" and "exactly 5 027 783 488" are different claims and
    /// the message must not blur them. The first invites the reader to shrug at a
    /// small difference; under [`Self::Exact`] there is no such thing as a small
    /// difference.
    fn verify(self, actual: u64) -> Result<(), String> {
        match self {
            Self::Exact(expected) if actual != expected => Err(format!(
                "the download is {actual} bytes and this model measures exactly {expected}. \
                 Nothing was installed."
            )),
            Self::Approximate(expected)
                if !kw_transcription_core::models::within_tolerance(actual, expected) =>
            {
                Err(format!(
                    "the download is {actual} bytes and the registry expects about {expected}. \
                     Nothing was installed."
                ))
            }
            _ => Ok(()),
        }
    }
}

/// Whether a finished download hashes to the digest the registry carries, and
/// which of two standards that digest meets.
///
/// Separate from [`SizeCheck`] rather than folded into it, because the two are
/// orthogonal and each has its own reason to be absent. A cross product of five
/// states would have hidden that; two enums composed at the call site do not.
///
/// **Why a digest at all, when the size is already exact.** An exact length
/// turns "roughly this file" into "a file of exactly 6 169 341 984 bytes", which
/// is a real narrowing and was the whole of `KNEG-101`. It is still satisfied by
/// any 6 169 341 984 bytes. A digest is satisfied by one file. That is the gap
/// `KNEG-45` recorded and this closes.
///
/// **The two standards, and why they are one enum with two variants rather than
/// a `String` and a `bool`.** Nothing downstream may reach the digest without
/// having been handed which kind it is; that is the only structural defence
/// against a stated oid being reported as a measured one, and the flag exists
/// precisely because that difference is easy to lose in a sentence.
#[derive(Debug, Clone, PartialEq, Eq)]
enum DigestCheck {
    /// A digest **this project measured**: the object was downloaded once,
    /// hashed here, and the result committed to this repository. Seven of the
    /// thirty-nine objects, one per publishing organisation.
    Measured(String),
    /// The **publisher's stated** digest -- Hugging Face's `lfs.oid`, which is a
    /// true SHA-256 of the object because git-lfs is content-addressed on it.
    ///
    /// Checked exactly as strictly as [`Self::Measured`]. What differs is what
    /// may be claimed afterwards: this digest arrived over the same channel as
    /// the bytes, so it catches corruption, truncation and an accidental
    /// re-upload, and it does not catch a compromised huggingface.co.
    Stated(String),
    /// The registry carries no digest. Reached only by a model added without
    /// being pinned; every object shipped today carries one, asserted by
    /// `every_local_model_carries_a_digest_the_installer_will_check`.
    Absent,
}

impl DigestCheck {
    /// Read the check out of a registry entry.
    ///
    /// A `measured` flag with no digest behind it is read as [`Self::Absent`]
    /// rather than trusted, the same way [`SizeCheck::of`] reads a measured size
    /// that is missing: the generator cannot produce that pair, and if it ever
    /// did, the safe reading of "measured, value missing" is that nothing is
    /// known.
    fn of(sha256: Option<&str>, measured: bool) -> Self {
        match (sha256, measured) {
            (Some(digest), true) => Self::Measured(digest.to_owned()),
            (Some(digest), false) => Self::Stated(digest.to_owned()),
            (None, _) => Self::Absent,
        }
    }

    /// The digest itself, for a caller that has to record what was checked.
    ///
    /// `None` for [`Self::Absent`], which is the honest answer and not a
    /// placeholder: there was no digest, so nothing about the bytes was proven
    /// and the record must not imply otherwise.
    const fn pinned(&self) -> Option<&String> {
        match self {
            Self::Measured(digest) | Self::Stated(digest) => Some(digest),
            Self::Absent => None,
        }
    }

    /// Hash the file at `path` and compare, phrasing any refusal in the terms the
    /// check was made in.
    ///
    /// Blocking, and deliberately called through `spawn_blocking`: this reads up
    /// to 20 GB back off the disk.
    fn verify(&self, path: &std::path::Path) -> Result<(), String> {
        let expected = match self {
            Self::Measured(digest) | Self::Stated(digest) => digest,
            Self::Absent => return Ok(()),
        };

        let actual = kw_sidecar::integrity::hash_file(path).map_err(|e| e.to_string())?;
        if actual == *expected {
            return Ok(());
        }

        // Two messages, because the two point at different next steps. A measured
        // digest that disagrees means this repository's pin is stale or the bytes
        // are wrong, and somebody has to re-measure. A stated one that disagrees
        // means Hugging Face's own metadata and Hugging Face's own bytes
        // disagree, which is a transfer fault far more often than anything else.
        Err(match self {
            Self::Measured(_) => format!(
                "the download hashes to {actual}, and the digest measured for this model is                  {expected}. Nothing was installed. Re-measure the pin before assuming the                  download was at fault."
            ),
            Self::Stated(_) => format!(
                "the download hashes to {actual}, and Hugging Face states {expected} for this                  object. Nothing was installed. The transfer was most likely corrupted; try                  again."
            ),
            Self::Absent => unreachable!("returned above"),
        })
    }
}

/// What may honestly be said about a file that passed both checks.
///
/// The digest decides it where there is one, because a digest subsumes a length:
/// a file that hashes correctly is the right length by construction. Only with
/// no digest at all does the size become the strongest thing that was checked.
const fn verified_by(size: SizeCheck, digest: &DigestCheck) -> crate::local_model::VerifiedBy {
    use crate::local_model::VerifiedBy;
    match digest {
        DigestCheck::Measured(_) => VerifiedBy::Digest,
        DigestCheck::Stated(_) => VerifiedBy::StatedDigest,
        DigestCheck::Absent => match size {
            SizeCheck::Exact(_) | SizeCheck::Approximate(_) => VerifiedBy::Size,
            SizeCheck::Unknown => VerifiedBy::Nothing,
        },
    }
}

/// Download one GGUF and verify it, returning what was actually checked.
///
/// **Size and digest.** Both, for every object shipped today. How strict the
/// length check is, and why, is [`SizeCheck`]; which of two standards the digest
/// meets, and what that costs, is [`DigestCheck`].
///
/// The order is length first, then hash. The length is free -- it is already on
/// the filesystem -- and a truncated transfer is the common failure, so checking
/// it first turns the common case into an immediate, specific message instead of
/// a digest mismatch that could mean anything.
///
/// **The hash happens at install, and is still not repeated.** Re-reading up to
/// 20 GB every time a settings screen opens is not a trade worth making, and the
/// whisper path gets away with it only because its largest model is 3 GB. That
/// reasoning has not changed.
///
/// What changed is what the screen says afterwards. It used to report
/// `installed: true`, which was a claim about a path, so a file corrupted after
/// a successful install read exactly like one that had just been checked —
/// KNEG-107. This now writes a record beside the file holding the digest, the
/// exact length and the modification time, and the screen reports whether the
/// file still agrees with it. **That is a cheap check, not a strong one**: it
/// catches corruption, truncation and a half-finished copy, and it does not
/// catch a replacement that preserves both length and timestamp. See
/// [`crate::gguf_check`], which says so at length, and `local_llm_recheck_model`
/// for the re-hash that is the only thing which actually proves the bytes.
///
/// **The download is bounded only under [`SizeCheck::Exact`].** A bound refuses
/// the transfer mid-flight, so it has to be a figure that cannot be wrong on
/// the high side, and only a measured one is. `Approximate` is upstream's
/// declaration held to 5%: bounding *that* is the bug KNEG-101 fixed, one layer
/// earlier — eighteen models were declared 6.7–7.9% larger than the file
/// Hugging Face serves, and the version of this function that compared a
/// finished download against the declared figure deleted all eighteen after
/// downloading 101 GB. A bound would have refused them faster and just as
/// wrongly. `Approximate`'s tolerance therefore stays a post-download check and
/// never becomes a bound.
///
/// So the strong case gets both protections and the weak case gets the one it
/// can support. That is the same shape as `models::install` for whisper models,
/// and for the same reason.
async fn download_gguf(
    policy: &NetworkPolicy,
    url: &str,
    destination: &std::path::Path,
    size: SizeCheck,
    digest: &DigestCheck,
    progress: &mut (dyn FnMut(u64, Option<u64>) + Send),
    verifying: &mut (dyn FnMut() + Send),
) -> Result<Option<crate::local_model::VerifiedBy>, String> {
    // Already there, so nothing was downloaded and nothing was re-checked. The
    // screen only offers Install for a model it believes is absent, so this is a
    // re-entry guard rather than the ordinary path -- and `None` says "no claim"
    // rather than repeating the claim the last install earned.
    if destination.exists() {
        return Ok(None);
    }

    // Downloaded beside the destination and renamed on success, so an
    // interrupted six-gigabyte download never looks like an installed model.
    let partial = destination.with_extension("partial");

    let purpose = kw_network_policy::RequestPurpose::UserTriggeredModelDownload;
    match size {
        SizeCheck::Exact(expected) => policy
            .download_to_file_bounded(purpose, url, &partial, expected, progress)
            .await
            .map_err(|e| e.to_string())?,
        SizeCheck::Approximate(_) | SizeCheck::Unknown => policy
            .download_to_file(purpose, url, &partial, progress)
            .await
            .map_err(|e| e.to_string())?,
    };

    let actual = std::fs::metadata(&partial)
        .map_err(|e| e.to_string())?
        .len();
    if let Err(refusal) = size.verify(actual) {
        let _ = std::fs::remove_file(&partial);
        return Err(refusal);
    }

    if !matches!(digest, DigestCheck::Absent) {
        verifying();
        let hashing = digest.clone();
        let path = partial.clone();
        // Off the async runtime: hashing 20 GB is minutes of blocking work, and
        // doing it on a worker thread would stall every other task on it.
        let outcome = tauri::async_runtime::spawn_blocking(move || hashing.verify(&path))
            .await
            .map_err(|e| e.to_string())?;
        if let Err(refusal) = outcome {
            let _ = std::fs::remove_file(&partial);
            return Err(refusal);
        }
    }

    std::fs::rename(&partial, destination).map_err(|e| e.to_string())?;

    let verdict = verified_by(size, digest);
    // After the rename, never before: the record stores the length and mtime of
    // the file at its final path, and a record written against the `.partial`
    // would describe something that no longer exists.
    //
    // A failure here is logged and not returned. The model is installed and
    // correct -- the record is the weaker, cheaper claim about it -- so failing
    // the install over a few hundred bytes that could not be written would throw
    // away a download that may have taken twenty minutes. The cost of the miss
    // is that the screen reads "installed, never recorded", which is true.
    if let Err(err) =
        gguf_check::write_record(destination, digest.pinned().map(String::as_str), verdict)
    {
        tracing::warn!(
            error = %err,
            "the model installed but its verification record could not be written;              it will report as unrecorded until it is re-checked"
        );
    }

    Ok(Some(verdict))
}

/* -------------------------------------------------------------- the re-hash */

/// What a re-hash of an installed model found.
///
/// Its own type rather than a `Result<(), String>` because **a file that does
/// not match its digest is a finding, not a failure**. Returning it as an error
/// would put it through the same red line a dropped connection uses, and the two
/// deserve entirely different sentences: one says try again, the other says the
/// bytes on this disk are not the bytes that were downloaded.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(
    tag = "outcome",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum RecheckOutcome {
    /// The file was hashed and it agreed with its pinned digest. The record
    /// beside it was rewritten, so the screen reads "unchanged since it was
    /// installed" again -- this time from a check made a moment ago.
    ///
    /// `verified_by` carries which of the two digest standards was met, on the
    /// same terms as everywhere else: hashing against the publisher's stated
    /// `lfs.oid` proves the transfer, not the publisher.
    Matched {
        verified_by: crate::local_model::VerifiedBy,
    },
    /// The file was hashed and it did not agree.
    ///
    /// The file is **not deleted**. At install a mismatched download is removed
    /// because nothing had come to depend on it yet; here it may be twenty
    /// gigabytes a user chose to keep, the mismatch may be this repository's pin
    /// having gone stale rather than the file having rotted, and deleting
    /// somebody's data on a hunch is not this command's decision to make. It
    /// names both digests and leaves the choice with them.
    Mismatched {
        file_name: String,
        expected: String,
        actual: String,
    },
    /// The registry carries no digest for this object, so there is nothing a
    /// re-hash could compare against.
    ///
    /// Unreachable for anything shipped today --
    /// `every_local_model_carries_a_digest_the_installer_will_check` asserts all
    /// thirty-nine are pinned -- and reported rather than quietly treated as a
    /// pass, because "we checked and it was fine" and "there was nothing to
    /// check" are the two claims this whole change exists to keep apart.
    NothingPinned,
    /// There is no file at the path to hash.
    NotInstalled,
}

/// What a hash that has already been computed proves. Pure.
///
/// Split from the reading of the file so the decision is testable without one:
/// the interesting part is which of three answers a digest comparison yields,
/// and that does not need twenty gigabytes to exercise.
fn recheck_verdict(digest: &DigestCheck, file_name: &str, actual: &str) -> RecheckOutcome {
    let Some(expected) = digest.pinned() else {
        return RecheckOutcome::NothingPinned;
    };

    if actual == expected {
        // `SizeCheck::Unknown` because a re-hash checks the digest and nothing
        // else -- the digest subsumes the length, so a file that hashes
        // correctly is the right length by construction and a separate size
        // check could only answer a question already answered.
        RecheckOutcome::Matched {
            verified_by: verified_by(SizeCheck::Unknown, digest),
        }
    } else {
        RecheckOutcome::Mismatched {
            file_name: file_name.to_owned(),
            expected: expected.clone(),
            actual: actual.to_owned(),
        }
    }
}

/// Bring the record beside a file into line with what a re-hash just found.
///
/// The mismatch arm is the one that matters. A record can only have survived a
/// mismatch by being *wrong*: it says the length and the timestamp are what they
/// were at install while the bytes say they are not, which is precisely the
/// replacement [`crate::gguf_check`] documents itself as unable to catch.
/// Leaving it in place would have the screen answer "unchanged since it was
/// installed" about a file this application has just proved is not the one that
/// was installed, so the record goes and the state falls back to "nothing is
/// known".
///
/// Neither arm fails the command. The answer is the hash; the record is a
/// convenience for the next screen paint, and a convenience that could not be
/// written must not turn a completed check into an error.
fn settle_record(path: &std::path::Path, digest: &DigestCheck, outcome: &RecheckOutcome) {
    match outcome {
        RecheckOutcome::Matched { verified_by } => {
            if let Err(err) =
                gguf_check::write_record(path, digest.pinned().map(String::as_str), *verified_by)
            {
                tracing::warn!(
                    error = %err,
                    "the re-check passed but its record could not be written; this model \
                     will keep reporting as it did before"
                );
            }
        }
        RecheckOutcome::Mismatched { .. } => {
            if let Err(err) = gguf_check::forget_record(path) {
                tracing::warn!(
                    error = %err,
                    "the re-check failed and the record that disagrees with it could not \
                     be removed; the screen may still call this file unchanged"
                );
            }
        }
        RecheckOutcome::NothingPinned | RecheckOutcome::NotInstalled => {}
    }
}

/// Hash one file and settle its record.
async fn recheck_one(
    path: &std::path::Path,
    file_name: &str,
    digest: &DigestCheck,
) -> Result<RecheckOutcome, String> {
    if digest.pinned().is_none() {
        // Answered before the read, because reading twenty gigabytes to compare
        // it against nothing is the one outcome worth not paying for.
        return Ok(RecheckOutcome::NothingPinned);
    }

    let owned = path.to_path_buf();
    // Off the async runtime, exactly as the install path hashes: this reads the
    // whole file back, which is a minute or more for the largest model, and a
    // worker thread blocked that long stalls every other task on it.
    let actual =
        tauri::async_runtime::spawn_blocking(move || kw_sidecar::integrity::hash_file(&owned))
            .await
            .map_err(|e| e.to_string())?
            .map_err(|e| e.to_string())?;

    let outcome = recheck_verdict(digest, file_name, &actual);
    settle_record(path, digest, &outcome);
    Ok(outcome)
}

/// Hash an installed model against its pinned digest, on request.
///
/// **The only thing that proves the bytes.** Everything else reported about an
/// installed file is two integers compared against a record; this reads the
/// file. It is a command rather than part of [`local_llm_status`] for the reason
/// stated throughout: the largest model is 19.8 GB, and a settings screen that
/// took a minute to paint would be a worse product than one that says plainly
/// what it has and has not checked.
///
/// Not cancellable, and that is a known limit rather than an oversight -- the
/// hash runs on a blocking thread with no polling point to check a flag at. A
/// user who starts one on a 20 GB model waits for it or closes the application.
///
/// Emits no progress. The screen sets its own message around the call, which
/// costs a static sentence instead of an event channel; `hash_file` yields no
/// intermediate state to report in the middle anyway.
#[tauri::command]
pub async fn local_llm_recheck_model(model_id: String) -> Result<RecheckOutcome, String> {
    let registry = kw_provider_registry::registry().map_err(|e| e.to_string())?;
    let model = find_model(registry, &model_id)?;

    let path = gguf_path(&model.file_name);
    if !path.exists() {
        return Ok(RecheckOutcome::NotInstalled);
    }

    let digest = DigestCheck::of(model.sha256.as_deref(), model.sha256_measured);
    let outcome = recheck_one(&path, &model.file_name, &digest).await?;
    let RecheckOutcome::Matched {
        verified_by: model_verdict,
    } = outcome
    else {
        return Ok(outcome);
    };

    // The drafter, when there is one and it is on disk. An absent drafter is
    // ordinary -- the model runs without it, more slowly -- so it neither fails
    // the check nor weakens the verdict.
    let mut verdict = model_verdict;
    if let Some(draft) = &model.draft {
        let draft_path = gguf_path(&draft.file_name);
        if draft_path.exists() {
            let draft_digest = DigestCheck::of(draft.sha256.as_deref(), draft.sha256_measured);
            match recheck_one(&draft_path, &draft.file_name, &draft_digest).await? {
                RecheckOutcome::Matched { verified_by } => verdict = verdict.weaker(verified_by),
                // A drafter that did not match, or that nothing is pinned for,
                // is the answer for the pair: both files are loaded together, so
                // reporting the model's clean result while saying nothing about
                // the other half would be the rounding-up `weaker` exists to
                // prevent. Its `file_name` says which half.
                other => return Ok(other),
            }
        }
    }

    Ok(RecheckOutcome::Matched {
        verified_by: verdict,
    })
}

#[tauri::command]
pub async fn local_llm_start(
    app: AppHandle,
    state: State<'_, crate::state::AppState>,
    llm: State<'_, Arc<LocalLlm>>,
    model_id: String,
) -> Result<u16, String> {
    let port = llm.start_model(&model_id).await?;

    // Remembered only after it came up, so a model that fails to start is not
    // the one resumed at the next launch.
    if let Err(err) =
        kw_storage::settings::set(state.db().pool(), LOCAL_LLM_MODEL_KEY, &model_id).await
    {
        tracing::warn!(error = %err, "could not remember the local text-generation model");
    }

    watch_for_idle(Arc::clone(&llm), app);
    Ok(port)
}

#[tauri::command]
pub async fn local_llm_stop(llm: State<'_, Arc<LocalLlm>>) -> Result<(), String> {
    llm.stop().await;
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

    #[test]
    fn the_catalogue_has_a_llama_sidecar_on_a_loopback_port_range() {
        let catalog = Catalog::shipped().expect("the shipped catalogue must parse");
        let (spec, _) = catalog.resolve(SIDECAR).expect("llama must be present");

        let range = spec.port_range().expect("a loopback sidecar has a range");
        assert_eq!(range.first, 8221);
        assert_eq!(range.last, 8240);
    }

    /// The range starts where `reasoning-core` says local requests go.
    ///
    /// `ChatRoute::Local` documented `127.0.0.1:8221` before anything served it.
    /// If the two ever disagreed, the first allocated port would work and every
    /// later one would not — a failure that only appears when a port is busy.
    #[test]
    fn the_first_port_is_the_one_reasoning_core_documents() {
        let catalog = Catalog::shipped().expect("catalogue");
        let (spec, _) = catalog.resolve(SIDECAR).expect("llama");
        assert_eq!(spec.port_range().expect("range").first, 8221);
    }

    /// A URL is built from the repo, the pinned commit and the filename.
    #[test]
    fn a_gguf_url_points_at_one_commit_in_the_publishers_repo() {
        assert_eq!(
            gguf_url(
                "bartowski/Qwen_Qwen3.5-9B-GGUF",
                "182be2fd6c7bc44887d88a91cb03ff009cc9f549",
                "Qwen_Qwen3.5-9B-Q4_K_M.gguf"
            ),
            "https://huggingface.co/bartowski/Qwen_Qwen3.5-9B-GGUF/resolve/\
             182be2fd6c7bc44887d88a91cb03ff009cc9f549/Qwen_Qwen3.5-9B-Q4_K_M.gguf"
        );
    }

    /// Every GGUF in the registry names a commit, and no two of an object's
    /// halves disagree about which one.
    ///
    /// The assertion that matters is `resolve/{40 hex}`: a model that slipped
    /// back to `main` would still install, still verify against its digest
    /// today, and start failing the first time its publisher pushed anything.
    /// That is the shape of the defect this replaced, and it would be invisible
    /// until it was somebody's broken install.
    #[test]
    fn every_gguf_is_fetched_from_a_commit_and_never_from_a_branch() {
        let registry = kw_provider_registry::registry().expect("registry");
        let mut checked = 0usize;
        for provider in &registry.local_llm {
            for model in &provider.models {
                for (label, repo, revision, file) in std::iter::once((
                    model.id.clone(),
                    &model.hf_repo,
                    &model.revision,
                    &model.file_name,
                ))
                .chain(model.draft.iter().map(|d| {
                    (
                        format!("{} (drafter)", model.id),
                        &d.hf_repo,
                        &d.revision,
                        &d.file_name,
                    )
                })) {
                    assert_eq!(
                        revision.len(),
                        40,
                        "{label}: {revision:?} is not a commit sha"
                    );
                    assert!(
                        revision
                            .chars()
                            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()),
                        "{label}: {revision:?} is not 40 lowercase hex characters"
                    );
                    let url = gguf_url(repo, revision, file);
                    assert!(
                        url.contains(&format!("/resolve/{revision}/")),
                        "{label}: {url} does not name the pinned commit"
                    );
                    checked += 1;
                }
            }
        }
        // The same count the digest and size assertions use. A model dropping
        // out of the registry must not make this pass over fewer objects.
        assert_eq!(checked, 39, "35 models plus 4 drafters");
    }

    /// Every model in the registry can be found by id.
    ///
    /// The screen offers what the registry lists, so an id it offers and this
    /// cannot resolve would be a button that fails when pressed.
    #[test]
    fn every_registry_model_resolves() {
        let registry = kw_provider_registry::registry().expect("registry");
        let ids: Vec<&str> = registry
            .local_llm
            .iter()
            .flat_map(|p| p.models.iter().map(|m| m.id.as_str()))
            .collect();

        assert!(
            ids.len() >= 20,
            "expected the registry's models, got {}",
            ids.len()
        );
        for id in ids {
            assert!(find_model(registry, id).is_ok(), "{id} did not resolve");
        }
        assert!(find_model(registry, "not-a-model").is_err());
    }

    /// An endpoint with nothing running answers `None`, not a default port.
    #[test]
    fn an_endpoint_with_no_server_offers_no_port() {
        let endpoint = LocalEndpoint::default();
        assert_eq!(endpoint.port(), None);

        endpoint.set(Some(8225));
        assert_eq!(endpoint.port(), Some(8225));

        endpoint.set(None);
        assert_eq!(endpoint.port(), None);
    }

    /// Reading the port counts as using it.
    ///
    /// This is what keeps the idle timer honest without a `touch()` call that
    /// somebody forgets: the only way to send a local request is to read the
    /// port, and reading it is the record.
    #[test]
    fn reading_the_port_resets_the_idle_clock() {
        let endpoint = LocalEndpoint::default();
        endpoint.set(Some(8221));

        // Backdate the last use by an hour.
        endpoint
            .last_used
            .store(chrono::Utc::now().timestamp() - 3_600, Ordering::Relaxed);
        assert!(endpoint.idle_for() >= Duration::from_secs(3_500));

        let _ = endpoint.port();
        assert!(endpoint.idle_for() < Duration::from_secs(5));
    }

    /// A never-used endpoint is not reported as idle for ever.
    #[test]
    fn an_untouched_endpoint_is_not_immediately_stale() {
        assert_eq!(LocalEndpoint::default().idle_for(), Duration::ZERO);
    }

    /// A measured size is held to the byte; a declared one is not.
    ///
    /// The third assertion is the defect, as arithmetic: a base-10 "5 GB" label
    /// multiplied by 2^30 rather than 10^9 declares 6.9% more bytes than the file
    /// has, which is outside the tolerance. Eighteen models were declared that
    /// way, downloaded in full, and deleted on arrival.
    #[test]
    fn a_measured_size_is_held_to_the_byte_and_a_declared_one_to_five_percent() {
        let bytes = 5_000_000_000_u64;

        let measured = SizeCheck::of(Some(bytes), true);
        assert!(measured.verify(bytes).is_ok());
        assert!(measured.verify(bytes - 1).is_err());
        assert!(measured.verify(bytes + 1).is_err());

        let declared = SizeCheck::of(Some(bytes), false);
        assert!(declared.verify(bytes - 1).is_ok());

        let double_converted = SizeCheck::of(Some(5 * 1024 * 1024 * 1024), false);
        assert!(double_converted.verify(bytes).is_err());

        // A flag with no number behind it claims nothing.
        assert_eq!(SizeCheck::of(None, true), SizeCheck::Unknown);
        assert!(SizeCheck::Unknown.verify(1).is_ok());
    }

    /// Every registry entry the installer can download resolves to a real check.
    ///
    /// Two of the three states are acceptable — a measured size held to the byte,
    /// a declared one held to 5% — and [`SizeCheck::Unknown`] is not: it means
    /// several gigabytes are written into the model directory with nothing having
    /// looked at them. It would take a registry that lost the field rather than a
    /// model somebody forgot to measure, which is why it is asserted rather than
    /// assumed.
    ///
    /// The second assertion is the weaker one on purpose. Every model and every
    /// drafter is measured today; a new one added before it is measured is
    /// *degraded*, not broken, and this is where that shows up. Resolve it by
    /// measuring the model into `LOCAL_LLM_PINS` — or, if it genuinely cannot be
    /// measured, by naming it here and saying why.
    #[test]
    fn every_local_model_carries_a_size_the_installer_will_check() {
        let registry = kw_provider_registry::registry().expect("registry");

        let mut checks: Vec<(String, SizeCheck)> = Vec::new();
        for provider in &registry.local_llm {
            for model in &provider.models {
                checks.push((
                    model.id.clone(),
                    SizeCheck::of(model.size_bytes, model.size_measured),
                ));
                if let Some(draft) = &model.draft {
                    checks.push((
                        format!("{} (drafter)", model.id),
                        SizeCheck::of(draft.size_bytes, draft.size_measured),
                    ));
                }
            }
        }

        assert!(
            checks.len() >= 35,
            "expected every registry model and drafter, got {}",
            checks.len()
        );

        let unchecked: Vec<&str> = checks
            .iter()
            .filter(|(_, c)| *c == SizeCheck::Unknown)
            .map(|(id, _)| id.as_str())
            .collect();
        assert!(
            unchecked.is_empty(),
            "these would install unverified: {unchecked:?}"
        );

        let unmeasured: Vec<&str> = checks
            .iter()
            .filter(|(_, c)| matches!(c, SizeCheck::Approximate(_)))
            .map(|(id, _)| id.as_str())
            .collect();
        assert!(
            unmeasured.is_empty(),
            "these still ride on upstream's declared size, which was wrong for \
             eighteen models: {unmeasured:?}. Measure them into LOCAL_LLM_PINS in \
             tools/integration-audit/generate-provider-registry.mjs."
        );
    }

    /// Every committed size still matches the file Hugging Face serves.
    ///
    /// `#[ignore]`d because it reaches the network, but it downloads nothing:
    /// one HEAD per object, no bodies, a few seconds in total. Run it:
    ///
    /// ```text
    /// cargo test -p knegiswhisper local_llm -- --ignored --nocapture
    /// ```
    ///
    /// This is the whole reason exact equality is safe to require. A committed
    /// measurement rots the moment a publisher re-uploads, and a rotted one is
    /// worse than a tolerance because it fails at the end of a twenty-gigabyte
    /// download in front of a user. This is the same measurement the pins were
    /// taken with, so a re-upload shows up here first.
    ///
    /// `x-linked-size` rather than `content-length`: Hugging Face serves these
    /// through LFS and answers a `resolve/{commit}` request with a 302 to a CDN,
    /// so `content-length` describes the redirect while `x-linked-size` carries
    /// the object. The fallback is used only on a 200, where there is no redirect
    /// for `content-length` to be measuring.
    ///
    /// Since KNEG-103 the URL names a commit rather than `main`, so this asks a
    /// narrower question than it used to: not "has the publisher re-uploaded"
    /// but "does the commit we pinned still hold an object of this length".
    /// A publisher pushing a new commit no longer moves these bytes at all.
    #[tokio::test]
    #[ignore = "sends one HEAD request per model to huggingface.co; run with --ignored"]
    async fn the_measured_sizes_still_match_hugging_face() {
        let registry = kw_provider_registry::registry().expect("registry");
        let policy = NetworkPolicy::new().expect("the shared client builds");

        let mut targets: Vec<(String, String, u64)> = Vec::new();
        for provider in &registry.local_llm {
            for model in &provider.models {
                if let (Some(bytes), true) = (model.size_bytes, model.size_measured) {
                    targets.push((
                        model.id.clone(),
                        gguf_url(&model.hf_repo, &model.revision, &model.file_name),
                        bytes,
                    ));
                }
                if let Some(draft) = &model.draft {
                    if let (Some(bytes), true) = (draft.size_bytes, draft.size_measured) {
                        targets.push((
                            format!("{} (drafter)", model.id),
                            gguf_url(&draft.hf_repo, &draft.revision, &draft.file_name),
                            bytes,
                        ));
                    }
                }
            }
        }
        assert!(
            !targets.is_empty(),
            "nothing is pinned, so nothing is checked"
        );

        let mut wrong: Vec<String> = Vec::new();
        for (id, url, committed) in &targets {
            let response = policy
                .request(
                    kw_network_policy::RequestPurpose::UserTriggeredModelDownload,
                    reqwest::Method::HEAD,
                    url,
                )
                .expect("the boundary allows huggingface.co")
                .send()
                .await
                .expect("huggingface.co answers a HEAD");

            let status = response.status();
            let served = response
                .headers()
                .get("x-linked-size")
                .or_else(|| {
                    status
                        .is_success()
                        .then(|| response.headers().get(reqwest::header::CONTENT_LENGTH))
                        .flatten()
                })
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.parse::<u64>().ok());

            match served {
                Some(bytes) if bytes == *committed => {
                    println!("{id}: {bytes} bytes, unchanged");
                }
                Some(bytes) => wrong.push(format!(
                    "{id}: committed {committed}, served {bytes} (status {status})"
                )),
                None => wrong.push(format!(
                    "{id}: no size header at all (status {status}) — the file may have moved"
                )),
            }
        }

        assert!(
            wrong.is_empty(),
            "{} of {} sizes have changed upstream; re-measure into LOCAL_LLM_PINS:\n{}",
            wrong.len(),
            targets.len(),
            wrong.join("\n")
        );
    }

    /// The two digest standards are checked equally and reported differently.
    ///
    /// That pairing is the whole design. A stated `lfs.oid` is a real SHA-256 of
    /// the object, so there is no reason to hold it more loosely than one this
    /// project measured — and every reason not to *describe* the two the same
    /// way, because only the measured one is independent of whoever serves the
    /// bytes.
    #[test]
    fn a_stated_digest_is_checked_as_strictly_as_a_measured_one_and_named_apart() {
        use crate::local_model::VerifiedBy;

        let digest = "a".repeat(64);
        let measured = DigestCheck::of(Some(&digest), true);
        let stated = DigestCheck::of(Some(&digest), false);

        assert_eq!(measured, DigestCheck::Measured(digest.clone()));
        assert_eq!(stated, DigestCheck::Stated(digest.clone()));

        // Same file, same comparison, different claim.
        assert_eq!(
            verified_by(SizeCheck::Exact(1), &measured),
            VerifiedBy::Digest
        );
        assert_eq!(
            verified_by(SizeCheck::Exact(1), &stated),
            VerifiedBy::StatedDigest
        );
        assert_ne!(
            crate::local_model::VerifiedBy::Digest.as_str(),
            crate::local_model::VerifiedBy::StatedDigest.as_str()
        );

        // A flag with no digest behind it claims nothing, exactly as
        // `SizeCheck::of(None, true)` claims nothing.
        assert_eq!(DigestCheck::of(None, true), DigestCheck::Absent);

        // With no digest, the size is the strongest thing that was checked.
        assert_eq!(
            verified_by(SizeCheck::Exact(1), &DigestCheck::Absent),
            VerifiedBy::Size
        );
        assert_eq!(
            verified_by(SizeCheck::Unknown, &DigestCheck::Absent),
            VerifiedBy::Nothing
        );
    }

    /// A digest that disagrees refuses the file, and says which pin to distrust.
    #[test]
    fn a_wrong_digest_refuses_the_file_in_the_terms_it_was_pinned_in() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("weights.gguf");
        std::fs::write(&path, b"not the model").expect("write");

        // sha256("not the model"), so the happy path is a real hash and not a
        // tautology about two constants.
        let real = kw_sidecar::integrity::hash_file(&path).expect("hash");
        assert!(DigestCheck::Stated(real.clone()).verify(&path).is_ok());
        assert!(DigestCheck::Measured(real).verify(&path).is_ok());
        assert!(DigestCheck::Absent.verify(&path).is_ok());

        let wrong = "b".repeat(64);
        let measured = DigestCheck::Measured(wrong.clone())
            .verify(&path)
            .expect_err("a wrong digest must refuse");
        let stated = DigestCheck::Stated(wrong)
            .verify(&path)
            .expect_err("a wrong digest must refuse");

        // Both say nothing was installed; each points at its own next step.
        assert!(measured.contains("Nothing was installed"), "{measured}");
        assert!(stated.contains("Nothing was installed"), "{stated}");
        assert!(measured.contains("Re-measure"), "{measured}");
        assert!(stated.contains("Hugging Face states"), "{stated}");
    }

    /// The weaker of two verdicts wins, so a drafter cannot be rounded up.
    #[test]
    fn an_install_is_only_as_verified_as_its_weakest_half() {
        use crate::local_model::VerifiedBy::{Digest, Nothing, Size, StatedDigest};

        assert_eq!(Digest.weaker(StatedDigest), StatedDigest);
        assert_eq!(StatedDigest.weaker(Digest), StatedDigest);
        assert_eq!(StatedDigest.weaker(Size), Size);
        assert_eq!(Size.weaker(Nothing), Nothing);
        assert_eq!(Digest.weaker(Digest), Digest);
        assert_eq!(Nothing.weaker(Digest), Nothing);
    }

    /// Every registry entry the installer can download carries a digest.
    ///
    /// The mirror of `every_local_model_carries_a_size_the_installer_will_check`,
    /// and the assertion that closes KNEG-45: [`DigestCheck::Absent`] means
    /// several gigabytes are written into the model directory with only a byte
    /// count having looked at them, which is where this path was before
    /// 2026-08-31.
    ///
    /// The second assertion is the weaker one on purpose, exactly as it is for
    /// the sizes. Thirty-two of the thirty-nine objects ride on the publisher's
    /// stated `lfs.oid` rather than a digest measured here; that is a deliberate
    /// cost decision — measuring all of them is 202 GB of downloading — so it is
    /// asserted as a *count* rather than forbidden. If that count moves, either
    /// somebody measured more of them, which is good and this number should
    /// follow, or a pin was quietly downgraded, which is not.
    #[test]
    fn every_local_model_carries_a_digest_the_installer_will_check() {
        let registry = kw_provider_registry::registry().expect("registry");

        let mut checks: Vec<(String, DigestCheck)> = Vec::new();
        for provider in &registry.local_llm {
            for model in &provider.models {
                checks.push((
                    model.id.clone(),
                    DigestCheck::of(model.sha256.as_deref(), model.sha256_measured),
                ));
                if let Some(draft) = &model.draft {
                    checks.push((
                        format!("{} (drafter)", model.id),
                        DigestCheck::of(draft.sha256.as_deref(), draft.sha256_measured),
                    ));
                }
            }
        }

        assert_eq!(
            checks.len(),
            39,
            "expected every registry model and drafter, got {}",
            checks.len()
        );

        let unpinned: Vec<&str> = checks
            .iter()
            .filter(|(_, c)| *c == DigestCheck::Absent)
            .map(|(id, _)| id.as_str())
            .collect();
        assert!(
            unpinned.is_empty(),
            "these would install with only a byte count having looked at them: \
             {unpinned:?}. Pin them into LOCAL_LLM_PINS in \
             tools/integration-audit/generate-provider-registry.mjs."
        );

        let measured = checks
            .iter()
            .filter(|(_, c)| matches!(c, DigestCheck::Measured(_)))
            .count();
        assert_eq!(
            measured, 7,
            "seven objects were downloaded and hashed on 2026-08-31, one per \
             publishing organisation. If this changed, say which way in the \
             commit: measuring more is progress, measuring fewer is a downgrade."
        );
    }

    /// Every committed digest still matches what Hugging Face states.
    ///
    /// `#[ignore]`d because it reaches the network, but it downloads no weights:
    /// one `tree` call per repository — 36 requests for 39 objects, because a
    /// tree lists every file in the repo at once — and a few seconds in total.
    /// Run it:
    ///
    /// ```text
    /// cargo test -p knegiswhisper local_llm -- --ignored --nocapture
    /// ```
    ///
    /// The companion to `the_measured_sizes_still_match_hugging_face`, and the
    /// same argument: a committed pin rots the moment a publisher re-uploads, and
    /// a rotted digest fails at the end of a twenty-gigabyte download in front of
    /// a user. This is cheaper than the size check it sits beside *and* strictly
    /// stronger, because `lfs.size` comes back in the same response.
    ///
    /// **What it does and does not prove.** It compares this repository's pin
    /// against Hugging Face's statement *at the commit the installer downloads
    /// from*. For the thirty-two stated digests that is the same standard they
    /// were pinned to, so agreement means "unchanged". For the seven measured
    /// ones it is weaker than how they were pinned — those bytes were hashed
    /// here — so agreement means Hugging Face has not re-uploaded, not that the
    /// bytes are still what we hashed. Only a download proves that, and
    /// `tools/model-pin/pin.mjs --url` is how.
    ///
    /// **The revision changed what this asks (KNEG-103).** Reading the tree at
    /// `main` used to ask "has the publisher re-uploaded", and a yes meant the
    /// bytes behind our URL had moved out from under the digest. The URL names a
    /// commit now, so a push cannot move them, and this asks something better:
    /// **do the digest and the revision committed beside each other describe the
    /// same object?** Nothing else in the workspace checks that pair. It also
    /// catches a pinned commit that has stopped resolving — a repository going
    /// private, or a force-push — which would break every install of its models
    /// and which nothing else would notice.
    #[tokio::test]
    #[ignore = "sends one tree request per repository to huggingface.co; run with --ignored"]
    async fn the_stated_digests_still_match_hugging_face() {
        use std::collections::HashMap;

        let registry = kw_provider_registry::registry().expect("registry");
        let policy = NetworkPolicy::new().expect("the shared client builds");

        /// One committed pin, as the tree read has to see it. A struct rather
        /// than the six-tuple this outgrew when the revision joined it.
        struct Stated {
            label: String,
            repo: String,
            revision: String,
            file: String,
            digest: String,
            size: Option<u64>,
        }

        let mut targets: Vec<Stated> = Vec::new();
        for provider in &registry.local_llm {
            for model in &provider.models {
                if let Some(digest) = &model.sha256 {
                    targets.push(Stated {
                        label: model.id.clone(),
                        repo: model.hf_repo.clone(),
                        revision: model.revision.clone(),
                        file: model.file_name.clone(),
                        digest: digest.clone(),
                        size: model.size_bytes,
                    });
                }
                if let Some(draft) = &model.draft {
                    if let Some(digest) = &draft.sha256 {
                        targets.push(Stated {
                            label: format!("{} (drafter)", model.id),
                            repo: draft.hf_repo.clone(),
                            revision: draft.revision.clone(),
                            file: draft.file_name.clone(),
                            digest: digest.clone(),
                            size: draft.size_bytes,
                        });
                    }
                }
            }
        }
        assert!(
            !targets.is_empty(),
            "nothing is pinned, so nothing is checked"
        );

        // One request per repository *and revision*, not per object: three of
        // the repos here hold two objects each. Keying on both rather than on
        // the repository alone is not redundant — one revision per repository
        // is how `HF_REVISIONS` is built, and a cache that assumed it would
        // answer from the wrong tree, silently, the day that stopped holding.
        let mut trees: HashMap<String, serde_json::Value> = HashMap::new();
        for target in &targets {
            let key = format!("{}@{}", target.repo, target.revision);
            if trees.contains_key(&key) {
                continue;
            }
            let (repo, revision) = (&target.repo, &target.revision);
            let url = format!(
                "https://huggingface.co/api/models/{repo}/tree/{revision}\
                 ?expand=true&recursive=true"
            );
            let body = policy
                .request(
                    kw_network_policy::RequestPurpose::UserTriggeredModelDownload,
                    reqwest::Method::GET,
                    &url,
                )
                .expect("the boundary allows huggingface.co")
                .send()
                .await
                .expect("huggingface.co answers a tree request")
                .json::<serde_json::Value>()
                .await
                .expect("the tree is JSON");
            trees.insert(key, body);
        }
        println!(
            "{} tree requests for {} objects",
            trees.len(),
            targets.len()
        );

        let mut wrong: Vec<String> = Vec::new();
        for target in &targets {
            let Stated {
                label,
                repo,
                revision,
                file,
                digest: committed,
                size: committed_size,
            } = target;
            let entry = trees
                .get(&format!("{repo}@{revision}"))
                .and_then(|t| t.as_array())
                .and_then(|entries| {
                    entries
                        .iter()
                        .find(|e| e.get("path").and_then(|p| p.as_str()) == Some(file.as_str()))
                });

            let Some(entry) = entry else {
                wrong.push(format!(
                    "{label}: {file} is not in {repo} at {revision} — the pin names a commit \
                     that does not hold it"
                ));
                continue;
            };

            // `lfs.oid` is the object's SHA-256: git-lfs is content-addressed on
            // it. A file served without an `lfs` block is not stored in LFS at
            // all, which for a multi-gigabyte GGUF would itself be the news.
            let stated = entry
                .get("lfs")
                .and_then(|l| l.get("oid"))
                .and_then(|o| o.as_str());
            match stated {
                Some(oid) if oid == committed => {}
                Some(oid) => wrong.push(format!("{label}: committed {committed}, stated {oid}")),
                None => wrong.push(format!(
                    "{label}: no lfs.oid in the tree entry — it may no longer be an LFS object"
                )),
            }

            // Free, because it arrived in the same response, and it is the same
            // figure `the_measured_sizes_still_match_hugging_face` HEADs for.
            let size = entry
                .get("lfs")
                .and_then(|l| l.get("size"))
                .and_then(|s| s.as_u64());
            if let (Some(stated), Some(committed)) = (size, *committed_size) {
                if stated != committed {
                    wrong.push(format!(
                        "{label}: committed {committed} bytes, stated {stated}"
                    ));
                }
            }
        }

        assert!(
            wrong.is_empty(),
            "{} of {} objects disagree with the commit they are pinned to. That is a \
             hand-edited registry or a repository that has been rewritten, not an ordinary \
             re-upload — a push to main cannot reach these. Re-read HF_REVISIONS and \
             re-measure LOCAL_LLM_PINS together:\n{}",
            wrong.len(),
            targets.len(),
            wrong.join("\n")
        );
    }

    /// The install progress states cross the boundary tagged and camelCased.
    #[test]
    fn progress_serializes_with_a_discriminant() {
        let json = serde_json::to_value(LocalLlmProgress::StoppedWhenIdle).unwrap();
        assert_eq!(json["stage"], "stopped-when-idle");

        let json = serde_json::to_value(LocalLlmProgress::Draft {
            id: "m".to_owned(),
            written: 1.0,
            total: None,
        })
        .unwrap();
        assert_eq!(json["stage"], "draft");
        assert_eq!(json["written"], 1.0);
    }

    /// A row reports the state of the file in the directory it was given.
    ///
    /// The wiring test, and the one that was missing: every assertion above this
    /// is about `gguf_check` in isolation, and a status call that ignored it and
    /// answered `NotInstalled` for everything would have passed all of them. It
    /// walks three of the four states in the same directory, because the defect
    /// KNEG-107 is about is not one state being wrong but two of them rendering
    /// alike.
    #[test]
    fn a_status_row_reports_the_state_of_the_file_in_the_model_directory() {
        use crate::gguf_check::{self, GgufIntegrity};
        use crate::local_model::VerifiedBy;

        let registry = kw_provider_registry::registry().expect("registry");
        // A model with a drafter, so both halves of the row are exercised. Four
        // of the thirty-five have one, and not in the first provider.
        let (provider, model) = registry
            .local_llm
            .iter()
            .flat_map(|p| p.models.iter().map(move |m| (p, m)))
            .find(|(_, m)| m.draft.is_some())
            .expect("a model with a drafter");
        let draft = model.draft.as_ref().expect("the drafter");

        let dir = tempfile::tempdir().expect("tempdir");

        // 1. Nothing on disk.
        let row = model_status(&provider.id, dir.path(), model);
        assert_eq!(row.id, model.id);
        assert_eq!(row.integrity, GgufIntegrity::NotInstalled);
        assert_eq!(
            row.draft.as_ref().expect("drafter row").integrity,
            GgufIntegrity::NotInstalled
        );

        // 2. The model present with nothing recorded beside it -- which is the
        //    state of every GGUF installed before this existed.
        let path = dir.path().join(&model.file_name);
        std::fs::write(&path, b"weights").expect("write");
        let row = model_status(&provider.id, dir.path(), model);
        assert_eq!(row.integrity, GgufIntegrity::Unrecorded);
        assert_ne!(
            row.integrity,
            GgufIntegrity::UnchangedSinceInstall {
                verified_by: row.verified_by
            },
            "an unrecorded file must not borrow the claim the registry makes \
             about what an install *would* check"
        );

        // 3. Recorded. Only the model, so the two halves of the row disagree --
        //    which is the point of tracking them apart.
        gguf_check::write_record(&path, Some(&"a".repeat(64)), VerifiedBy::StatedDigest)
            .expect("record");
        let row = model_status(&provider.id, dir.path(), model);
        assert_eq!(
            row.integrity,
            GgufIntegrity::UnchangedSinceInstall {
                verified_by: VerifiedBy::StatedDigest
            }
        );
        assert_eq!(
            row.draft.as_ref().expect("drafter row").integrity,
            GgufIntegrity::NotInstalled,
            "the drafter is a separate file and must not inherit the model's state"
        );
        assert_eq!(
            row.draft.as_ref().expect("drafter row").file_name,
            draft.file_name
        );

        // 4. Truncated behind the application's back.
        std::fs::write(&path, b"w").expect("truncate");
        assert!(matches!(
            model_status(&provider.id, dir.path(), model).integrity,
            GgufIntegrity::Changed { .. }
        ));
    }

    /* --------------------------------------------------------- the re-hash */

    /// A digest that is pinned is reachable, and one that is not is `None`.
    ///
    /// `pinned` is what decides whether a re-hash has anything to compare
    /// against, so an `Absent` that answered `Some("")` would have
    /// `recheck_one` read twenty gigabytes to compare them with nothing.
    #[test]
    fn only_a_pinned_digest_is_reachable_for_recording() {
        let digest = "e".repeat(64);
        assert_eq!(
            DigestCheck::Measured(digest.clone()).pinned(),
            Some(&digest)
        );
        assert_eq!(DigestCheck::Stated(digest.clone()).pinned(), Some(&digest));
        assert_eq!(DigestCheck::Absent.pinned(), None);
    }

    /// A re-hash that agrees reports the standard it agreed with, undiluted.
    ///
    /// The pairing is the same one the install path holds to: both digests are
    /// compared identically and named differently, because only the measured one
    /// is independent of whoever served the bytes. A re-check that reported
    /// `Digest` for a stated pin would be the rounding-up this codebase carries a
    /// per-row flag to prevent, arriving by the back door.
    #[test]
    fn a_recheck_that_agrees_reports_the_standard_it_agreed_with() {
        use crate::local_model::VerifiedBy;

        let digest = "f".repeat(64);

        assert_eq!(
            recheck_verdict(&DigestCheck::Measured(digest.clone()), "m.gguf", &digest),
            RecheckOutcome::Matched {
                verified_by: VerifiedBy::Digest
            }
        );
        assert_eq!(
            recheck_verdict(&DigestCheck::Stated(digest.clone()), "m.gguf", &digest),
            RecheckOutcome::Matched {
                verified_by: VerifiedBy::StatedDigest
            }
        );
    }

    /// A re-hash that disagrees names both digests and which file it read.
    ///
    /// Both, because the two point at different next steps and a message with
    /// only one of them cannot be acted on. The file name is there because a
    /// model and its drafter are checked in one command and "it did not match"
    /// without saying which half is not an answer.
    #[test]
    fn a_recheck_that_disagrees_names_both_digests_and_the_file() {
        let pinned = "1".repeat(64);
        let found = "2".repeat(64);

        assert_eq!(
            recheck_verdict(&DigestCheck::Measured(pinned.clone()), "draft.gguf", &found),
            RecheckOutcome::Mismatched {
                file_name: "draft.gguf".to_owned(),
                expected: pinned,
                actual: found,
            }
        );
    }

    /// Nothing pinned is said out loud, not treated as a pass.
    ///
    /// The failure this whole change is about is a state that renders as fine
    /// when it is not, and "there was nothing to check" rendering as "checked
    /// and fine" is that failure with a different subject.
    #[test]
    fn a_recheck_with_nothing_pinned_says_so_rather_than_passing() {
        let outcome = recheck_verdict(&DigestCheck::Absent, "m.gguf", &"0".repeat(64));

        assert_eq!(outcome, RecheckOutcome::NothingPinned);
        assert!(
            !matches!(outcome, RecheckOutcome::Matched { .. }),
            "an unpinned object must never come back as a pass"
        );
    }

    /// A re-hash that agrees leaves a record the next status call will believe.
    ///
    /// The end-to-end claim, on a real file: hashing proves the bytes, the
    /// record carries that forward cheaply, and the screen reads it back. Each
    /// of the three is tested on its own elsewhere; this is the seam between
    /// them.
    #[test]
    fn a_passed_recheck_leaves_a_record_the_status_call_believes() {
        use crate::gguf_check::{self, GgufIntegrity};
        use crate::local_model::VerifiedBy;

        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("weights.gguf");
        std::fs::write(&path, b"the model").expect("write");

        // A real hash of real bytes, so the happy path is not two constants
        // agreeing with each other.
        let real = kw_sidecar::integrity::hash_file(&path).expect("hash");
        let digest = DigestCheck::Stated(real.clone());

        assert_eq!(gguf_check::integrity(&path), GgufIntegrity::Unrecorded);

        let outcome = recheck_verdict(&digest, "weights.gguf", &real);
        settle_record(&path, &digest, &outcome);

        assert_eq!(
            gguf_check::integrity(&path),
            GgufIntegrity::UnchangedSinceInstall {
                verified_by: VerifiedBy::StatedDigest
            }
        );
    }

    /// A re-hash that disagrees takes the record with it.
    ///
    /// This is the one case where the cheap check and the real one contradict
    /// each other, and it is exactly the replacement `gguf_check` documents
    /// itself as unable to see: same length, same timestamp, different bytes.
    /// The record has to go, because leaving it would have the screen answer
    /// "unchanged since it was installed" about a file just proved otherwise —
    /// the strongest available claim, made about the worst available file.
    ///
    /// The file itself stays. It may be twenty gigabytes somebody chose to keep,
    /// and the pin going stale is as likely an explanation as the bytes rotting.
    #[test]
    fn a_failed_recheck_removes_the_record_that_contradicts_it_and_keeps_the_file() {
        use crate::gguf_check::{self, GgufIntegrity};
        use crate::local_model::VerifiedBy;

        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("weights.gguf");
        std::fs::write(&path, b"the model").expect("write");

        let pinned = "9".repeat(64);
        gguf_check::write_record(&path, Some(&pinned), VerifiedBy::Digest).expect("record");
        assert_eq!(
            gguf_check::integrity(&path),
            GgufIntegrity::UnchangedSinceInstall {
                verified_by: VerifiedBy::Digest
            },
            "the record claims the strongest thing there is, before the re-hash"
        );

        let digest = DigestCheck::Measured(pinned);
        let outcome = recheck_verdict(&digest, "weights.gguf", &"8".repeat(64));
        assert!(matches!(outcome, RecheckOutcome::Mismatched { .. }));
        settle_record(&path, &digest, &outcome);

        assert_eq!(
            gguf_check::integrity(&path),
            GgufIntegrity::Unrecorded,
            "a record that survived a mismatch would be the strongest claim in \
             this application, standing on bytes it has just been shown are wrong"
        );
        assert!(
            path.exists(),
            "the weights are the user's, not ours to delete"
        );
    }

    /// Nothing pinned changes nothing on disk.
    ///
    /// Neither writing nor deleting: no check was made, so no claim may be
    /// created and none that already exists may be destroyed.
    #[test]
    fn nothing_pinned_leaves_the_record_exactly_as_it_found_it() {
        use crate::gguf_check::{self, GgufIntegrity};
        use crate::local_model::VerifiedBy;

        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("weights.gguf");
        std::fs::write(&path, b"the model").expect("write");
        gguf_check::write_record(&path, Some(&"7".repeat(64)), VerifiedBy::Size).expect("record");

        settle_record(&path, &DigestCheck::Absent, &RecheckOutcome::NothingPinned);
        assert_eq!(
            gguf_check::integrity(&path),
            GgufIntegrity::UnchangedSinceInstall {
                verified_by: VerifiedBy::Size
            }
        );

        settle_record(&path, &DigestCheck::Absent, &RecheckOutcome::NotInstalled);
        assert_eq!(
            gguf_check::integrity(&path),
            GgufIntegrity::UnchangedSinceInstall {
                verified_by: VerifiedBy::Size
            }
        );
    }

    /// The four outcomes are four distinct things on the wire.
    #[test]
    fn the_recheck_outcomes_cross_the_boundary_tagged_and_distinct() {
        use crate::local_model::VerifiedBy;

        let tags: Vec<String> = [
            RecheckOutcome::Matched {
                verified_by: VerifiedBy::Digest,
            },
            RecheckOutcome::Mismatched {
                file_name: "m.gguf".to_owned(),
                expected: "a".repeat(64),
                actual: "b".repeat(64),
            },
            RecheckOutcome::NothingPinned,
            RecheckOutcome::NotInstalled,
        ]
        .iter()
        .map(|o| {
            serde_json::to_value(o).unwrap()["outcome"]
                .as_str()
                .unwrap()
                .to_owned()
        })
        .collect();

        assert_eq!(
            tags,
            vec!["matched", "mismatched", "nothing-pinned", "not-installed"]
        );

        let json = serde_json::to_value(RecheckOutcome::Mismatched {
            file_name: "m.gguf".to_owned(),
            expected: "a".repeat(64),
            actual: "b".repeat(64),
        })
        .unwrap();
        assert_eq!(json["fileName"], "m.gguf");
        assert_ne!(json["expected"], json["actual"]);
    }
}
