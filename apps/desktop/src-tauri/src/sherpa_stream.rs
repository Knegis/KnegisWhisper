//! The `sherpa-onnx-online-websocket-server` lifecycle: the runtime behind the
//! `local-streaming` meeting provider.
//!
//! One supervisor, not a second one. `kw_sidecar::process::start` allocates the
//! port, spawns the child, waits for it to answer and — since `watch_for_death`
//! landed for KNEG-96 — says so when it dies unasked. Everything here is the
//! arguments and the readiness probe.
//!
//! # No download
//!
//! `sherpa-onnx-online-websocket-server` is in the same archive as
//! `sherpa-onnx-offline` and is extracted beside it, so installing a Parakeet
//! model under `Settings > Transcription > Local` already puts this on disk.
//! The `sherpa-stream` entry in `config/sidecars.json` says so in its targets
//! rather than describing a second 24 MB download this code does not do; what it
//! supplies is the port range and the command fragment the reaper needs.
//!
//! # Readiness is a WebSocket handshake, not a TCP connect
//!
//! There is no health endpoint — this is a WebSocket server, not an HTTP one, so
//! [`kw_sidecar::HttpHealth`] does not apply. A bare TCP connect would answer
//! "something is listening", which a half-initialised server also answers. The
//! probe completes a real handshake and closes, which is the same thing the
//! client does a moment later.
//!
//! # What is never logged
//!
//! The server writes recognised text to **stdout**, and `process::start` sends
//! stdout to `Stdio::null()` for exactly that reason — its own comment says so.
//! `--log-file` is pointed at the temp directory rather than left at its default
//! of `./log.txt` beside the executable; the file was measured on 2026-09-01
//! carrying websocketpp transport errors and no recognition output, but "what it
//! writes today" is not a guarantee, and a log this application does not choose
//! the location of is one it cannot make promises about.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use kw_provider_registry::SherpaModel;
use kw_sidecar::{port, process, Catalog, Launch, PidStore, Running, StartTimings};
use kw_storage::paths;

/// The sidecar id in `config/sidecars.json` for the streaming server.
const SIDECAR: &str = "sherpa-stream";

/// The sidecar whose install directory actually holds the binary.
const INSTALLED_BY: &str = "sherpa";

/// The executable, without a platform suffix.
const EXECUTABLE: &str = "sherpa-onnx-online-websocket-server";

/// Which streaming model the meeting provider should use, across restarts.
pub const STREAM_MODEL_KEY: &str = "meetings.streamModel";

/// How long a loaded streaming model is kept warm after its meeting ends.
const IDLE_STOP: Duration = Duration::from_secs(5 * 60);

/// The right to use the streaming server for one meeting.
///
/// The generation is deliberately opaque to the meeting runtime. It only hands
/// the value back to [`SherpaStream::release`], where it prevents an idle timer
/// belonging to an earlier meeting from stopping the server under a later one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StreamLease {
    generation: u64,
}

#[derive(Default)]
struct StreamState {
    running: Option<Running>,
    port: Option<u16>,
    model: Option<String>,
    generation: u64,
    lease: Option<StreamLease>,
}

impl StreamState {
    fn acquire_lease(&mut self) -> StreamLease {
        self.generation = self.generation.wrapping_add(1);
        let lease = StreamLease {
            generation: self.generation,
        };
        self.lease = Some(lease);
        lease
    }

    fn release_lease(&mut self, lease: StreamLease) -> bool {
        if self.lease != Some(lease) {
            return false;
        }
        self.lease = None;
        true
    }

    fn timer_is_current(&self, generation: u64) -> bool {
        self.lease.is_none() && self.generation == generation
    }

    fn invalidate(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.lease = None;
    }

    fn forget(&mut self) {
        self.port = None;
        self.model = None;
    }
}

struct StreamInner {
    pids: PidStore,
    state: tokio::sync::Mutex<StreamState>,
}

/// The running streaming server, if there is one.
pub struct SherpaStream {
    catalog: Catalog,
    /// Process state and lease state share one lock. An idle timer must decide
    /// that its generation is still idle and take the process under the same
    /// lock; otherwise a later meeting can acquire between the check and stop.
    inner: Arc<StreamInner>,
}

impl std::fmt::Debug for SherpaStream {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SherpaStream")
            .field(
                "port",
                &self.inner.state.try_lock().ok().and_then(|s| s.port),
            )
            .finish_non_exhaustive()
    }
}

impl SherpaStream {
    /// Load the catalogue and prepare, without touching the network or disk.
    pub fn new() -> Result<Self, String> {
        let catalog = Catalog::shipped().map_err(|err| err.to_string())?;
        Ok(Self {
            catalog,
            inner: Arc::new(StreamInner {
                pids: PidStore::new(&paths::data_dir()),
                state: tokio::sync::Mutex::new(StreamState::default()),
            }),
        })
    }

    /// Clean up a server a previous run left behind.
    ///
    /// The reason `sherpa-stream` is a sidecar entry rather than a path: the
    /// reaper reads the live process's command and refuses to signal anything it
    /// cannot prove is ours, and `sherpa`'s own fragment (`sherpa-onnx-offline`)
    /// does not match this executable. Without an entry a leftover server would
    /// hold its port until the machine was restarted.
    pub async fn reap_stale(&self) {
        let Some(spec) = self.catalog.get(SIDECAR) else {
            return;
        };

        let outcome = kw_sidecar::reap_one(
            spec,
            &self.inner.pids,
            &kw_sidecar::SystemProcessTable,
            kw_sidecar::reap::Timings::default(),
        )
        .await;

        match outcome {
            kw_sidecar::Outcome::NoRecord => {}
            other => tracing::info!(?other, "reaped a stale streaming model server"),
        }
    }

    /// Start the server on `model_id`, and answer with the port it came up on.
    ///
    /// Already running on the same model is a no-op returning the same port: the
    /// meeting runtime asks for this every time a meeting starts, and restarting
    /// a loaded model between two meetings would cost the load time for nothing.
    pub async fn start(&self, model_id: &str) -> Result<(u16, StreamLease), String> {
        let mut held = self.inner.state.lock().await;
        if held.model.as_deref() == Some(model_id) && held.running.is_some() {
            let port = held.port.ok_or_else(|| {
                "the streaming server is running without a published port".to_owned()
            })?;
            let lease = held.acquire_lease();
            return Ok((port, lease));
        }

        let spec = self
            .catalog
            .get(SIDECAR)
            .ok_or_else(|| format!("{SIDECAR} is not in the sidecar catalogue"))?;

        let executable = installed().ok_or_else(|| {
            "the sherpa-onnx runtime is not installed. Install a local model under Settings > \
             Transcription > Local first -- the streaming server ships in the same archive."
                .to_owned()
        })?;

        let model = model_named(model_id)?;
        let files = model_files(&model)?;

        let chosen = port::allocate(
            spec.port_range()
                .ok_or_else(|| format!("{SIDECAR} declares no port range"))?,
            &port::BindProbe,
        )
        .map_err(|err| err.to_string())?;

        let launch = Launch {
            executable,
            args: build_args(&files, chosen, &log_path()),
            port: chosen,
        };

        // A different model means a different server. Stopped before the new one
        // is started rather than after: two of these hold two copies of a 630 MB
        // model, and the port allocator would happily give the second one a
        // different port and leave the first running.
        held.invalidate();
        if let Some(running) = held.running.take() {
            running.stop(&self.inner.pids).await.unwrap_or_else(|err| {
                tracing::warn!(error = %err, "could not stop the streaming server cleanly");
            });
        }
        held.forget();

        let running = process::start(
            spec,
            &launch,
            &self.inner.pids,
            &WebSocketHealth,
            StartTimings::default(),
        )
        .await
        .map_err(|err| err.to_string())?;

        // Published only once it has answered a handshake, so a meeting never
        // routes to a server still loading its model.
        held.port = Some(chosen);
        held.model = Some(model_id.to_owned());
        held.running = Some(running);
        let lease = held.acquire_lease();

        Ok((chosen, lease))
    }

    /// Release one meeting's use and stop the server after five idle minutes.
    pub async fn release(&self, lease: StreamLease) {
        let generation = {
            let mut held = self.inner.state.lock().await;
            if !held.release_lease(lease) {
                return;
            }
            lease.generation
        };

        let inner = Arc::clone(&self.inner);
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(IDLE_STOP).await;
            let mut held = inner.state.lock().await;
            if !held.timer_is_current(generation) {
                return;
            }

            // The current-generation check and taking the process are one
            // critical section. A new meeting cannot acquire in between them.
            if let Some(running) = held.running.take() {
                tracing::info!(
                    idle_minutes = IDLE_STOP.as_secs() / 60,
                    "stopping the streaming model server after being idle"
                );
                running.stop(&inner.pids).await.unwrap_or_else(|err| {
                    tracing::warn!(error = %err, "could not stop the streaming server cleanly");
                });
            }
            held.forget();
        });
    }
}

/* --------------------------------------------------------------- the pieces */

/// The four files a sherpa online transducer needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelFiles {
    pub tokens: PathBuf,
    pub encoder: PathBuf,
    pub decoder: PathBuf,
    pub joiner: PathBuf,
}

/// The command line, pure so it can be read in a test rather than in a spawn.
///
/// Every flag was taken from the real `--help` on 2026-09-01, not from the
/// project's documentation site.
#[must_use]
pub fn build_args(files: &ModelFiles, port: u16, log: &std::path::Path) -> Vec<String> {
    vec![
        format!("--port={port}"),
        format!("--tokens={}", files.tokens.display()),
        format!("--encoder={}", files.encoder.display()),
        format!("--decoder={}", files.decoder.display()),
        format!("--joiner={}", files.joiner.display()),
        // Two sockets per meeting, one per capture, and the default is 3. Five
        // leaves headroom for both to decode at once without letting the pool
        // grow unbounded on a machine with many cores.
        "--num-work-threads=5".to_owned(),
        // Away from its default of `./log.txt`, which is beside the executable
        // in the sidecar directory. See the module documentation.
        format!("--log-file={}", log.display()),
    ]
}

/// Where the server's own log goes.
fn log_path() -> PathBuf {
    paths::audio_temp_dir().join("sherpa-stream.log")
}

/// Find the streaming server, which lives in the `sherpa` sidecar's directory.
fn installed() -> Option<PathBuf> {
    let name = if cfg!(windows) {
        format!("{EXECUTABLE}.exe")
    } else {
        EXECUTABLE.to_owned()
    };
    let path = paths::sidecar_dir().path.join(INSTALLED_BY).join(name);
    path.is_file().then_some(path)
}

/// Look one streaming model up in the registry.
fn model_named(id: &str) -> Result<SherpaModel, String> {
    let registry = kw_provider_registry::registry().map_err(|err| err.to_string())?;
    let model = registry
        .local_asr
        .sherpa
        .iter()
        .find(|m| m.id == id)
        .cloned()
        .ok_or_else(|| format!("no local model called {id:?} is in the registry"))?;

    if !model.streaming {
        // Refused rather than started. An offline transducer handed to the
        // streaming server produces a server that starts, accepts a socket and
        // transcribes nothing -- the failure this whole module is arranged to
        // avoid.
        return Err(format!(
            "{} is an offline model. The streaming server needs a streaming one.",
            model.name
        ));
    }
    Ok(model)
}

/// The four files, or a refusal naming what is missing.
fn model_files(model: &SherpaModel) -> Result<ModelFiles, String> {
    let dir = kw_transcription_core::sherpa_models::model_dir(&models_root(), model);
    let found = kw_transcription_core::sherpa::discover_transducer_files(&dir)
        .map_err(|err| format!("the files for {} could not be read: {err}", model.id))?;

    Ok(ModelFiles {
        tokens: found.tokens,
        encoder: found.encoder,
        decoder: found.decoder,
        joiner: found.joiner,
    })
}

/// Where sherpa models are kept. The same root `local_asr` installs into.
fn models_root() -> PathBuf {
    paths::model_cache_dir().path.join("sherpa")
}

/* ------------------------------------------------------------------- health */

/// Ready when a WebSocket handshake completes.
///
/// Not a TCP connect: something listening and something able to speak the
/// protocol are different claims, and the second is the one a meeting depends
/// on. Not an HTTP GET either — there is no health endpoint, and a bare GET
/// against a WebSocket server is a 400 that would read as unhealthy forever.
struct WebSocketHealth;

#[async_trait::async_trait]
impl kw_sidecar::Health for WebSocketHealth {
    async fn is_ready(&self, port: u16) -> bool {
        let address = format!("ws://{}:{port}/", std::net::Ipv4Addr::LOCALHOST);
        match tokio_tungstenite::connect_async(address).await {
            Ok((socket, _)) => {
                // Closed rather than dropped, so the readiness probe does not
                // leave the server holding a decoder for a session that never
                // sent anything.
                let mut socket = socket;
                let _ = socket.close(None).await;
                true
            }
            Err(_) => false,
        }
    }
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

    fn files() -> ModelFiles {
        ModelFiles {
            tokens: PathBuf::from("/m/tokens.txt"),
            encoder: PathBuf::from("/m/encoder.onnx"),
            decoder: PathBuf::from("/m/decoder.onnx"),
            joiner: PathBuf::from("/m/joiner.onnx"),
        }
    }

    #[test]
    fn the_arguments_are_the_ones_the_real_help_declares() {
        // Read off `sherpa-onnx-online-websocket-server --help` on 2026-09-01,
        // not off the documentation site. A flag this server does not know is
        // not ignored -- sherpa's option parser exits.
        let args = build_args(&files(), 8_250, std::path::Path::new("/tmp/s.log"));
        assert!(args.contains(&"--port=8250".to_owned()), "{args:?}");
        assert!(args.iter().any(|a| a.starts_with("--tokens=")));
        assert!(args.iter().any(|a| a.starts_with("--encoder=")));
        assert!(args.iter().any(|a| a.starts_with("--decoder=")));
        assert!(args.iter().any(|a| a.starts_with("--joiner=")));
        for arg in &args {
            assert!(arg.starts_with("--"), "{arg} is not a flag");
        }
    }

    #[test]
    fn the_log_is_pointed_somewhere_this_application_chose() {
        // Left at its default the server writes `./log.txt` beside its own
        // executable, in the sidecar directory. Measured carrying only
        // transport errors, but a log whose location we do not choose is one we
        // cannot make promises about.
        let args = build_args(&files(), 8_250, std::path::Path::new("/tmp/s.log"));
        let log = args
            .iter()
            .find(|a| a.starts_with("--log-file="))
            .expect("the log file is always specified");
        assert_eq!(log, "--log-file=/tmp/s.log");
    }

    #[test]
    fn the_catalogue_has_the_streaming_sidecar_on_its_own_port_range() {
        let catalog = Catalog::shipped().expect("the shipped catalogue parses");
        let spec = catalog.get(SIDECAR).expect("sherpa-stream is declared");

        let range = spec
            .port_range()
            .expect("a server sidecar declares a port range");
        assert_eq!(range.first, 8_250);

        // Not overlapping the two servers that already exist, because two
        // sidecars sharing a range is a port collision that only appears when
        // both are running.
        for other in ["whisper", "llama"] {
            let theirs = catalog
                .get(other)
                .and_then(kw_sidecar::manifest::SidecarSpec::port_range)
                .expect("a range");
            assert!(
                range.first > theirs.last || range.last < theirs.first,
                "{SIDECAR} overlaps {other}"
            );
        }
    }

    #[test]
    fn the_reaper_can_prove_the_streaming_server_is_ours() {
        // The whole reason this is a sidecar entry rather than a path. The
        // `sherpa` sidecar's own fragment does not match this executable, so
        // without an entry a leftover server could never be signalled.
        let catalog = Catalog::shipped().expect("the shipped catalogue parses");
        let spec = catalog.get(SIDECAR).expect("sherpa-stream is declared");

        let command = format!("C:\\kw\\sidecars\\sherpa\\{EXECUTABLE}.exe --port=8250");
        let liveness = kw_sidecar::reap::Liveness::Running {
            command: command.clone(),
        };
        assert!(matches!(
            kw_sidecar::reap::verdict(&liveness, &spec.command_fragments),
            kw_sidecar::reap::Verdict::OursToKill { .. }
        ));

        // And the offline CLI's own spec must not claim it, or stopping one
        // would stop the other.
        let offline = catalog.get(INSTALLED_BY).expect("sherpa is declared");
        assert!(
            !offline
                .command_fragments
                .iter()
                .any(|f| command.contains(f.as_str())),
            "the offline sidecar's fragments match the streaming server"
        );
    }

    #[test]
    fn an_offline_model_is_refused_rather_than_started() {
        // A Parakeet model handed to the streaming server produces a server
        // that starts, accepts a socket and transcribes nothing.
        let err = model_named("parakeet-unified-en-0.6b").expect_err("offline is refused");
        assert!(err.contains("offline"), "{err}");

        // And the streaming one is accepted, so the test is not passing
        // because every id is refused.
        let model = model_named("nemotron-speech-streaming-en-0.6b")
            .expect("a streaming model is accepted");
        assert!(model.streaming);
    }

    #[test]
    fn an_unknown_model_names_itself_in_the_refusal() {
        let err = model_named("not-a-model").expect_err("unknown is refused");
        assert!(err.contains("not-a-model"), "{err}");
    }

    #[test]
    fn a_released_lease_arms_only_its_own_generation() {
        let mut state = StreamState::default();
        let first = state.acquire_lease();
        assert!(state.release_lease(first));
        assert!(state.timer_is_current(first.generation));

        let second = state.acquire_lease();
        assert!(!state.timer_is_current(first.generation));
        assert!(!state.release_lease(first), "a stale release is ignored");
        assert_eq!(state.lease, Some(second));
    }

    #[test]
    fn an_explicit_stop_invalidates_an_armed_idle_timer() {
        let mut state = StreamState::default();
        let lease = state.acquire_lease();
        assert!(state.release_lease(lease));
        state.invalidate();
        assert!(!state.timer_is_current(lease.generation));
    }
}
