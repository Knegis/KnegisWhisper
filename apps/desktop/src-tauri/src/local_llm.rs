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

/// The download URL for a GGUF on Hugging Face.
///
/// Built here rather than stored in the registry because it is mechanical, and
/// because a URL in data is a URL somebody can point somewhere else. The host is
/// the same one the whisper models come from.
fn gguf_url(repo: &str, file_name: &str) -> String {
    format!("https://huggingface.co/{repo}/resolve/main/{file_name}")
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
    pub installed: bool,
    /// Whether this model has a speculative-decoding drafter, and whether that
    /// drafter is installed. Absent for the models without one.
    pub draft: Option<DraftStatus>,
}

/// A drafter alongside the main model.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct DraftStatus {
    pub file_name: String,
    pub size_bytes: Option<f64>,
    pub installed: bool,
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
    Done {
        id: String,
    },
    /// The idle timer fired. Not an install event, but it arrives on the same
    /// channel because it changes the same screen.
    StoppedWhenIdle,
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

    let models = registry
        .local_llm
        .iter()
        .flat_map(|p| {
            p.models.iter().map(move |m| LocalLlmModelStatus {
                id: m.id.clone(),
                name: m.name.clone(),
                provider: p.id.clone(),
                size_bytes: m.size_bytes.map(|b| b as f64),
                quantization: m.quantization.clone(),
                context_length: m.context_length.map(|c| c as f64),
                recommended: m.recommended,
                supports_thinking: m.supports_thinking,
                installed: gguf_path(&m.file_name).exists(),
                draft: m.draft.as_ref().map(|d| DraftStatus {
                    file_name: d.file_name.clone(),
                    size_bytes: d.size_bytes.map(|b| b as f64),
                    installed: gguf_path(&d.file_name).exists(),
                }),
            })
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
        install_dir: paths::model_cache_dir().path.display().to_string(),
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

    download_gguf(
        &llm.policy,
        &gguf_url(&model.hf_repo, &model.file_name),
        &gguf_path(&model.file_name),
        SizeCheck::of(model.size_bytes, model.size_measured),
        &mut |written, total| {
            emit(LocalLlmProgress::Model {
                id: model_id.clone(),
                written: written as f64,
                total: total.map(|t| t as f64),
            });
        },
    )
    .await?;

    // --- the drafter, if this model has one ---
    if let Some(draft) = &model.draft {
        download_gguf(
            &llm.policy,
            &gguf_url(&draft.hf_repo, &draft.file_name),
            &gguf_path(&draft.file_name),
            SizeCheck::of(draft.size_bytes, draft.size_measured),
            &mut |written, total| {
                emit(LocalLlmProgress::Draft {
                    id: model_id.clone(),
                    written: written as f64,
                    total: total.map(|t| t as f64),
                });
            },
        )
        .await?;
    }

    emit(LocalLlmProgress::Done { id: model_id });
    Ok(())
}

/// How strictly a finished download is held to the length the registry carries.
///
/// Three states, because the registry's byte counts have three provenances and
/// they are not equally trustworthy. `config/provider-registry.json` says which
/// one each number is with a `sizeMeasured` flag; the measurements themselves
/// are `LOCAL_LLM_SIZES` in
/// `tools/integration-audit/generate-provider-registry.mjs`.
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

/// Download one GGUF, verified by size where the registry carries one.
///
/// **Size, not digest.** The registry carries no SHA-256 for these files and
/// inventing one from a download would pin whatever arrived rather than what the
/// publisher built — a pin that verifies nothing while looking like it does. The
/// honest position is that a GGUF is checked by length and the screen says so,
/// exactly as `VerifiedBy::Size` already does for whisper models.
///
/// How strict that length check is, and why, is [`SizeCheck`].
async fn download_gguf(
    policy: &NetworkPolicy,
    url: &str,
    destination: &std::path::Path,
    size: SizeCheck,
    progress: &mut (dyn FnMut(u64, Option<u64>) + Send),
) -> Result<(), String> {
    if destination.exists() {
        return Ok(());
    }

    // Downloaded beside the destination and renamed on success, so an
    // interrupted six-gigabyte download never looks like an installed model.
    let partial = destination.with_extension("partial");

    policy
        .download_to_file(
            kw_network_policy::RequestPurpose::UserTriggeredModelDownload,
            url,
            &partial,
            progress,
        )
        .await
        .map_err(|e| e.to_string())?;

    let actual = std::fs::metadata(&partial)
        .map_err(|e| e.to_string())?
        .len();
    if let Err(refusal) = size.verify(actual) {
        let _ = std::fs::remove_file(&partial);
        return Err(refusal);
    }

    std::fs::rename(&partial, destination).map_err(|e| e.to_string())
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

    /// A URL is built from the repo and filename, never stored.
    #[test]
    fn a_gguf_url_points_at_the_publishers_repo() {
        assert_eq!(
            gguf_url(
                "bartowski/Qwen_Qwen3.5-9B-GGUF",
                "Qwen_Qwen3.5-9B-Q4_K_M.gguf"
            ),
            "https://huggingface.co/bartowski/Qwen_Qwen3.5-9B-GGUF/resolve/main/\
             Qwen_Qwen3.5-9B-Q4_K_M.gguf"
        );
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
    /// measuring the model into `LOCAL_LLM_SIZES` — or, if it genuinely cannot be
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
             eighteen models: {unmeasured:?}. Measure them into LOCAL_LLM_SIZES in \
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
    /// through LFS and answers a `resolve/main` request with a 302 to a CDN, so
    /// `content-length` describes the redirect while `x-linked-size` carries the
    /// object. The fallback is used only on a 200, where there is no redirect for
    /// `content-length` to be measuring.
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
                        gguf_url(&model.hf_repo, &model.file_name),
                        bytes,
                    ));
                }
                if let Some(draft) = &model.draft {
                    if let (Some(bytes), true) = (draft.size_bytes, draft.size_measured) {
                        targets.push((
                            format!("{} (drafter)", model.id),
                            gguf_url(&draft.hf_repo, &draft.file_name),
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
            "{} of {} sizes have changed upstream; re-measure into LOCAL_LLM_SIZES:\n{}",
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
}
