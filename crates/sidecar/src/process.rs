//! Starting and stopping a sidecar.
//!
//! Three things here are decisions rather than plumbing.
//!
//! **A child that dies during startup is not a timeout.** Waiting the full
//! budget and then reporting "did not start in time" throws away the reason: a
//! missing model file, an unsupported CPU, a port that was taken between the
//! probe and the spawn. So the wait watches the child as well as the port, and a
//! process that exits reports its exit code and the tail of its diagnostics.
//!
//! **Standard output is dropped, standard error is kept, bounded.** whisper.cpp
//! prints recognised segments on stdout in its command-line modes; capturing
//! that would put transcript text into this application's logs, which the brief
//! forbids. Standard error carries the startup diagnostics that make a failure
//! explicable, so it is kept — capped, and surfaced only when a start actually
//! fails rather than routinely.
//!
//! **A start that fails still stops the child.** Otherwise a wedged sidecar
//! keeps its port and the next attempt fails differently, which is how one bad
//! start becomes a feature that never works again.

use std::path::PathBuf;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::io::AsyncReadExt as _;

use crate::manifest::SidecarSpec;
use crate::pid::{PidRecord, PidStore};

/// How long a sidecar gets to answer its health check before being given up on.
pub const START_TIMEOUT: Duration = Duration::from_secs(60);

/// How often to ask during startup. Frequent, because most starts are fast and
/// the user is waiting on one.
pub const START_POLL_INTERVAL: Duration = Duration::from_millis(100);

/// How much standard error to keep for diagnostics.
const STDERR_TAIL_BYTES: usize = 4 * 1024;

/// What to run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Launch {
    pub executable: PathBuf,
    pub args: Vec<String>,
    /// The loopback port the arguments tell it to bind.
    pub port: u16,
}

/// Why a sidecar did not start.
#[derive(Debug, thiserror::Error)]
pub enum ProcessError {
    #[error("could not start {executable}: {source}")]
    Spawn {
        executable: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("{name} exited during startup with {exit}. Its last diagnostics were:\n{stderr}")]
    ExitedDuringStartup {
        name: String,
        exit: String,
        stderr: String,
    },

    #[error(
        "{name} did not answer on port {port} within {seconds}s. It was stopped rather than \
         left holding the port."
    )]
    Timeout {
        name: String,
        port: u16,
        seconds: u64,
    },

    #[error(transparent)]
    Pid(#[from] crate::PidError),
}

/// Whether a sidecar is answering yet.
///
/// A trait so the startup sequence — the part with the interesting failure
/// modes — is testable without a real server or a real socket.
#[async_trait::async_trait]
pub trait Health: Send + Sync {
    async fn is_ready(&self, port: u16) -> bool;
}

/// Timings, so tests do not wait a minute.
#[derive(Debug, Clone, Copy)]
pub struct StartTimings {
    pub timeout: Duration,
    pub poll_interval: Duration,
    /// How often [`watch_for_death`] asks whether the child is still there.
    pub watch_interval: Duration,
}

impl Default for StartTimings {
    fn default() -> Self {
        Self {
            timeout: START_TIMEOUT,
            poll_interval: START_POLL_INTERVAL,
            watch_interval: WATCH_INTERVAL,
        }
    }
}

/// How often a started sidecar is asked whether it is still alive.
///
/// Five seconds. This is a post-mortem, not a supervisor: nothing acts on the
/// answer, because the transcription paths already health-check the recorded
/// port and revive the server themselves. Noticing a death within five seconds
/// is enough to put it next to the log lines that explain it.
const WATCH_INTERVAL: Duration = Duration::from_secs(5);

/// A sidecar that started and answered.
#[derive(Debug)]
pub struct Running {
    name: String,
    port: u16,
    pid: u32,
    executable: PathBuf,
    /// Shared with [`watch_for_death`], which needs to ask `try_wait` and must
    /// not take the handle away from [`Running::stop`].
    child: Arc<tokio::sync::Mutex<tokio::process::Child>>,
    stderr: Arc<Mutex<StderrTail>>,
    capture: tokio::sync::watch::Receiver<Option<String>>,
    exit_report: Arc<Mutex<Option<String>>>,
    /// Set by [`Running::stop`] before it kills, so the watcher can tell a
    /// shutdown we asked for from one we did not.
    asked_to_stop: Arc<AtomicBool>,
}

impl Running {
    #[must_use]
    pub const fn port(&self) -> u16 {
        self.port
    }

    #[must_use]
    pub const fn pid(&self) -> u32 {
        self.pid
    }

    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    #[must_use]
    pub fn executable(&self) -> &std::path::Path {
        &self.executable
    }

    /// Whether the process is still there.
    ///
    /// `try_wait` rather than a signal, so this can never have a side effect on
    /// a process id that has been reused.
    ///
    /// `&mut self` is kept although the handle is now behind a lock: every
    /// caller already had one, and taking `&self` would let two of them race
    /// for the same reap.
    pub async fn is_running(&mut self) -> bool {
        let mut child = self.child.lock().await;
        matches!(child.try_wait(), Ok(None))
    }

    /// The tail of the sidecar's diagnostics. Bounded, and never routinely
    /// logged.
    #[must_use]
    pub fn stderr_tail(&self) -> String {
        self.stderr
            .lock()
            .map(|tail| tail.text())
            .unwrap_or_default()
    }

    pub fn exit_report(&self) -> Option<String> {
        self.exit_report
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    /// Ask it to stop, and clear its PID record.
    ///
    /// Uses the child handle rather than the reaper: this is our own child, so
    /// there is no possibility of a reused id and nothing to verify. The reaper
    /// is for processes that outlived the run that started them.
    ///
    /// This stops the process we started, not a process tree. None of the
    /// sidecars in `config/sidecars.json` spawn children of their own, so today
    /// that is the same thing — but a future sidecar that does would leave
    /// orphans here, and the symptom is indirect: the orphan keeps the stderr
    /// pipe open, so the reader task never finishes. It showed up first as a
    /// test that took twenty seconds.
    pub async fn stop(self, store: &PidStore) -> Result<(), ProcessError> {
        // Before the kill, not after: the watcher wakes on its own schedule, and
        // a flag set afterwards would let it report a shutdown we asked for as
        // an unexplained death. It is read again on the far side of `try_wait`
        // for the same reason.
        self.asked_to_stop.store(true, Ordering::SeqCst);

        // `kill` is TerminateProcess on Windows and SIGKILL on Unix. These
        // servers hold no state that needs flushing -- the model is read-only and
        // an in-flight request is one we are abandoning on purpose -- so there is
        // nothing a graceful shutdown would protect.
        {
            let mut child = self.child.lock().await;
            let _ = child.kill().await;
            let _ = child.wait().await;
        }
        store.clear_if_pid(&self.name, self.pid)?;
        Ok(())
    }
}

/// Start a sidecar and wait for it to answer.
pub async fn start<H: Health>(
    spec: &SidecarSpec,
    launch: &Launch,
    store: &PidStore,
    health: &H,
    timings: StartTimings,
) -> Result<Running, ProcessError> {
    let mut command = tokio::process::Command::new(&launch.executable);
    command
        .args(&launch.args)
        // stdout can carry recognised text; we never want it in a log.
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .stdin(Stdio::null());

    // Run from the executable's own directory so the sibling ggml and whisper
    // libraries resolve. Without it, a start from an unrelated working directory
    // fails with a loader error rather than anything explicable.
    //
    // Guarded, because `Path::parent` of a bare command name such as `cmd.exe`
    // is an empty path and `current_dir("")` fails the spawn outright.
    if let Some(dir) = launch.executable.parent() {
        if dir.is_dir() {
            command.current_dir(dir);
        }
    }

    #[cfg(windows)]
    {
        // CREATE_NO_WINDOW, which tokio exposes on Command directly. A console
        // window flashing up on every dictation would be its own bug report.
        command.creation_flags(0x0800_0000);
    }

    let mut child = command.spawn().map_err(|source| ProcessError::Spawn {
        executable: launch.executable.clone(),
        source,
    })?;

    let pid = child.id().unwrap_or_default();
    let stderr = Arc::new(Mutex::new(StderrTail::new()));

    let (capture_tx, capture) = tokio::sync::watch::channel(None);
    if let Some(pipe) = child.stderr.take() {
        let sink = Arc::clone(&stderr);
        tokio::spawn(async move {
            let result = collect_stderr(pipe, sink).await;
            let _ = capture_tx.send(Some(result));
        });
    } else {
        let _ = capture_tx.send(Some("stderr pipe unavailable".to_owned()));
    }

    if let Err(error) = store.write(&PidRecord {
        name: spec.name.clone(),
        pid,
        port: launch.port,
        executable: launch.executable.display().to_string(),
    }) {
        let _ = child.kill().await;
        let _ = child.wait().await;
        return Err(ProcessError::Pid(error));
    }

    let running = Running {
        name: spec.name.clone(),
        port: launch.port,
        pid,
        executable: launch.executable.clone(),
        child: Arc::new(tokio::sync::Mutex::new(child)),
        stderr,
        capture,
        exit_report: Arc::new(Mutex::new(None)),
        asked_to_stop: Arc::new(AtomicBool::new(false)),
    };

    let deadline = tokio::time::Instant::now() + timings.timeout;
    loop {
        if health.is_ready(launch.port).await {
            tracing::info!(sidecar = %spec.name, port = launch.port, pid, "sidecar ready");
            // Spawned only on the success path, so a child that died during
            // startup is reported once, by the branch below, rather than twice.
            watch_for_death(&running, store, timings.watch_interval);
            return Ok(running);
        }

        // Checked after the health probe, so a server that became ready and then
        // exited in the same instant is reported as ready rather than as a
        // startup crash.
        let exited = {
            let mut child = running.child.lock().await;
            child.try_wait()
        };
        if let Ok(Some(status)) = exited {
            let _ = store.clear_if_pid(&spec.name, pid);
            let capture = drain_stderr(running.capture.clone()).await;
            tracing::debug!(%capture, "startup exit diagnostics drained");
            return Err(ProcessError::ExitedDuringStartup {
                name: spec.name.clone(),
                exit: describe(&status),
                stderr: running.stderr_tail(),
            });
        }

        if tokio::time::Instant::now() >= deadline {
            let name = spec.name.clone();
            let port = launch.port;
            // Stopped rather than leaked: a sidecar still holding the port is
            // how one bad start becomes a feature that never works again.
            running.stop(store).await?;
            return Err(ProcessError::Timeout {
                name,
                port,
                seconds: timings.timeout.as_secs(),
            });
        }

        tokio::time::sleep(timings.poll_interval).await;
    }
}

/// Say so when the child ends without being asked to.
///
/// **Nothing did, before 2026-08-31.** [`start`] reports a child that dies
/// *during* startup and [`Running::stop`] ends one deliberately. Between those
/// two there was no observer at all: the process could go away and the only
/// trace was a later connection refused, with nothing to say whether it had
/// crashed, been killed, or exited cleanly. It happened at least twice in one
/// afternoon on 2026-08-28 and the record could not distinguish those three.
/// KNEG-96.
///
/// **This does not restart anything, and that is deliberate.** The
/// transcription and import paths already health-check the recorded port and
/// revive the remembered model themselves. A second thing that restarts
/// servers would race them. What was missing was not recovery; it was an
/// explanation.
///
/// Polling rather than awaiting the child, because `stop` needs that handle
/// too and a watcher parked in `wait()` would hold it for the life of the
/// process.
///
/// **The stderr tail is logged, and stdout is not, and the difference is not an
/// accident.** [`start`] sends stdout to `Stdio::null()` precisely because it
/// can carry recognised text; stderr carries the loader and model diagnostics,
/// which is what a post-mortem needs. The same tail is already shown to the
/// user by [`ProcessError::ExitedDuringStartup`].
fn watch_for_death(running: &Running, store: &PidStore, interval: Duration) {
    let name = running.name.clone();
    let pid = running.pid;
    let port = running.port;
    let child = Arc::clone(&running.child);
    let stderr = Arc::clone(&running.stderr);
    let capture = running.capture.clone();
    let exit_report = Arc::clone(&running.exit_report);
    let executable = running.executable.clone();
    let asked_to_stop = Arc::clone(&running.asked_to_stop);
    let store: PidStore = (*store).clone();
    let started = std::time::Instant::now();

    tokio::spawn(async move {
        loop {
            tokio::time::sleep(interval).await;
            if asked_to_stop.load(Ordering::SeqCst) {
                return;
            }

            let exited = {
                let mut child = child.lock().await;
                child.try_wait()
            };

            let status = match exited {
                Ok(None) => continue,
                Ok(Some(status)) => status,
                Err(err) => {
                    tracing::warn!(
                        sidecar = %name, pid,
                        error = %err,
                        "could not tell whether the sidecar is still running; giving up watching it"
                    );
                    return;
                }
            };

            // Read again on this side of the reap: `stop` can have set it while
            // the lock above was held, and reporting a shutdown we asked for as
            // an unexplained death is exactly the noise that makes a real one
            // easy to skip past.
            if asked_to_stop.load(Ordering::SeqCst) {
                return;
            }

            let capture = drain_stderr(capture).await;
            if asked_to_stop.load(Ordering::SeqCst) {
                return;
            }
            let tail = stderr
                .lock()
                .map(|tail| tail.text())
                .unwrap_or_else(|_| "the diagnostics buffer was poisoned".to_owned());

            *exit_report.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(format!("{} (PID {pid}) exited with {} after {} seconds. Diagnostics capture: {capture}. {tail}", executable.display(), describe(&status), started.elapsed().as_secs()));
            tracing::error!(
                sidecar = %name,
                pid,
                port,
                exit = %describe(&status),
                ran_for_seconds = started.elapsed().as_secs(),
                diagnostics = %tail,
                capture = %capture,
                executable = %executable.display(),
                "the sidecar exited on its own; nothing asked it to stop"
            );

            // Cleared for the same reason `stop` clears it: a record naming a
            // process that no longer exists is how a reused process id comes to
            // look like a live server. The reaper verifies before acting, so
            // this is belt and braces -- and it is also the thing a test can
            // observe, since a log line is not.
            if let Err(err) = store.clear_if_pid(&name, pid) {
                tracing::warn!(
                    sidecar = %name,
                    error = %err,
                    "could not clear the PID record of a sidecar that exited"
                );
            }
            return;
        }
    });
}

fn describe(status: &std::process::ExitStatus) -> String {
    match status.code() {
        Some(code) => format!("exit code {code}"),
        None => "no exit code (killed by a signal)".to_owned(),
    }
}

/// A bounded ring of the most recent diagnostics.
#[derive(Debug, Default)]
struct StderrTail {
    bytes: std::collections::VecDeque<u8>,
}
impl StderrTail {
    fn new() -> Self {
        Self::default()
    }
    fn append(&mut self, bytes: &[u8]) {
        for byte in bytes {
            if self.bytes.len() == STDERR_TAIL_BYTES {
                self.bytes.pop_front();
            }
            self.bytes.push_back(*byte);
        }
    }
    #[cfg(test)]
    fn push(&mut self, line: &str) {
        self.append(line.as_bytes());
        self.append(b"\n");
    }
    fn text(&self) -> String {
        String::from_utf8_lossy(&self.bytes.iter().copied().collect::<Vec<_>>()).into_owned()
    }
}

async fn collect_stderr(
    mut pipe: impl tokio::io::AsyncRead + Unpin,
    sink: Arc<Mutex<StderrTail>>,
) -> String {
    let mut buffer = [0u8; 4096];
    loop {
        match pipe.read(&mut buffer).await {
            Ok(0) => return "complete".to_owned(),
            Ok(count) => {
                if let Ok(mut tail) = sink.lock() {
                    tail.append(buffer.get(..count).unwrap_or_default());
                }
            }
            Err(error) => return format!("read failed: {error}"),
        }
    }
}

async fn drain_stderr(mut capture: tokio::sync::watch::Receiver<Option<String>>) -> String {
    let drain = async {
        loop {
            if let Some(status) = capture.borrow_and_update().clone() {
                return status;
            }
            if capture.changed().await.is_err() {
                return "capture task ended without status".to_owned();
            }
        }
    };
    tokio::time::timeout(Duration::from_secs(1), drain)
        .await
        .unwrap_or_else(|_| "timed out draining stderr".to_owned())
}

/// A health check that asks the sidecar over loopback.
///
/// Any answer counts, including a 404: the question is whether something is
/// listening and speaking HTTP, not whether a particular route exists.
/// whisper-server answers `GET /` with its static index page, and a build that
/// dropped that page would otherwise look permanently unhealthy.
#[derive(Debug, Clone)]
pub struct HttpHealth {
    policy: kw_network_policy::NetworkPolicy,
    path: String,
    timeout: Duration,
}

impl HttpHealth {
    #[must_use]
    pub fn new(policy: kw_network_policy::NetworkPolicy, path: impl Into<String>) -> Self {
        Self {
            policy,
            path: path.into(),
            timeout: Duration::from_secs(2),
        }
    }
}

#[async_trait::async_trait]
impl Health for HttpHealth {
    async fn is_ready(&self, port: u16) -> bool {
        let Ok(request) = self.policy.loopback(reqwest::Method::GET, port, &self.path) else {
            return false;
        };
        request.timeout(self.timeout).send().await.is_ok()
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
    use std::collections::BTreeMap;
    use std::sync::atomic::{AtomicU32, Ordering};

    use super::*;
    use crate::manifest::{PortRange, Source, Transport};

    fn spec() -> SidecarSpec {
        SidecarSpec {
            name: "probe".to_owned(),
            description: "a test sidecar".to_owned(),
            source: Source {
                repo: "r".to_owned(),
                release: "t".to_owned(),
                pinned_because: "w".to_owned(),
            },
            transport: Transport::Loopback {
                port_range: PortRange {
                    first: 8178,
                    last: 8199,
                },
            },
            command_fragments: vec!["probe".to_owned()],
            targets: BTreeMap::new(),
        }
    }

    fn fast() -> StartTimings {
        StartTimings {
            timeout: Duration::from_millis(400),
            poll_interval: Duration::from_millis(10),
            // Short enough that a test can outlive one tick without waiting,
            // and long enough not to spin: the watcher runs for the life of
            // every started sidecar in this module's tests.
            watch_interval: Duration::from_millis(30),
        }
    }

    /// Ready once `after` probes have been answered; never ready when `after` is
    /// `None`.
    struct ReadyAfter {
        after: Option<u32>,
        probes: AtomicU32,
    }

    impl ReadyAfter {
        fn immediately() -> Self {
            Self {
                after: Some(0),
                probes: AtomicU32::new(0),
            }
        }

        fn never() -> Self {
            Self {
                after: None,
                probes: AtomicU32::new(0),
            }
        }
    }

    #[async_trait::async_trait]
    impl Health for ReadyAfter {
        async fn is_ready(&self, _port: u16) -> bool {
            let seen = self.probes.fetch_add(1, Ordering::SeqCst);
            self.after.is_some_and(|after| seen >= after)
        }
    }

    fn owned(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|part| (*part).to_owned()).collect()
    }

    /// A process that stays up long enough to be polled, using the platform's
    /// own tools so this test needs no helper binary.
    ///
    /// Spawned directly rather than through a shell. Wrapping it in `cmd /c`
    /// made the whole test module take twenty seconds: `stop` killed the shell,
    /// the sleeper it had spawned survived as an orphan, and the stderr reader
    /// stayed blocked on a pipe that grandchild still held open. That is worth
    /// knowing about the production path too — see `Running::stop`.
    fn long_lived(port: u16) -> Launch {
        if cfg!(windows) {
            Launch {
                executable: PathBuf::from("ping.exe"),
                args: owned(&["-n", "20", "127.0.0.1"]),
                port,
            }
        } else {
            Launch {
                executable: PathBuf::from("/bin/sleep"),
                args: owned(&["20"]),
                port,
            }
        }
    }

    /// A process that exits straight away with a non-zero code.
    fn exits_with(code: i32, port: u16) -> Launch {
        let code = code.to_string();
        if cfg!(windows) {
            Launch {
                executable: PathBuf::from("cmd.exe"),
                args: owned(&["/c", "exit", &code]),
                port,
            }
        } else {
            Launch {
                executable: PathBuf::from("/bin/sh"),
                args: owned(&["-c", &format!("exit {code}")]),
                port,
            }
        }
    }

    /// A process that answers, then exits on its own shortly afterwards.
    ///
    /// The shape KNEG-96 is about: the health check passes, `start` returns
    /// `Ok`, and some time later the child is gone with nobody having asked.
    fn dies_after_a_moment(port: u16) -> Launch {
        if cfg!(windows) {
            // Two pings, about a second apart, then exit 0. `timeout` needs a
            // console and refuses one when stdin is null.
            Launch {
                executable: PathBuf::from("ping.exe"),
                args: owned(&["-n", "2", "127.0.0.1"]),
                port,
            }
        } else {
            Launch {
                executable: PathBuf::from("/bin/sleep"),
                args: owned(&["1"]),
                port,
            }
        }
    }

    /// Wait for the PID record to go, or give up.
    ///
    /// Polled rather than slept for a fixed time: the child's own lifetime is
    /// approximate, and a fixed sleep long enough to be reliable would be long
    /// enough to be worth avoiding.
    async fn record_cleared_within(store: &PidStore, name: &str, patience: Duration) -> bool {
        let deadline = tokio::time::Instant::now() + patience;
        while tokio::time::Instant::now() < deadline {
            if store.read(name).ok().flatten().is_none() {
                return true;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        false
    }

    /// A sidecar that goes away on its own is noticed, and its PID record does
    /// not outlive it.
    ///
    /// The log line is the point of KNEG-96 and a log line is not assertable,
    /// so the record is what this checks: same code path, same trigger, and a
    /// stale record naming a dead process is independently the thing that makes
    /// a reused process id look like a live server.
    #[tokio::test]
    async fn a_sidecar_that_dies_unasked_is_noticed() {
        let dir = tempfile::tempdir().unwrap();
        let store = PidStore::new(dir.path());

        let running = start(
            &spec(),
            &dies_after_a_moment(8190),
            &store,
            &ReadyAfter::immediately(),
            fast(),
        )
        .await
        .unwrap();

        assert!(
            store.read("probe").unwrap().is_some(),
            "the record should exist while the process does"
        );

        assert!(
            record_cleared_within(&store, "probe", Duration::from_secs(20)).await,
            "nothing noticed the sidecar had gone"
        );

        // Dropped afterwards rather than stopped: stopping it would be the very
        // case this test is distinguishing itself from.
        drop(running);
    }

    /// A shutdown we asked for is not reported as a death.
    ///
    /// Without the flag, `stop` would kill the child and the watcher would wake
    /// up seconds later and log an unexplained exit for it. Every deliberate
    /// model switch would produce one, which is exactly the noise that makes a
    /// real occurrence easy to scroll past.
    #[tokio::test]
    async fn stopping_a_sidecar_is_not_reported_as_a_death() {
        let dir = tempfile::tempdir().unwrap();
        let store = PidStore::new(dir.path());

        let running = start(
            &spec(),
            &long_lived(8191),
            &store,
            &ReadyAfter::immediately(),
            fast(),
        )
        .await
        .unwrap();

        let asked_to_stop = Arc::clone(&running.asked_to_stop);
        running.stop(&store).await.unwrap();
        assert!(
            asked_to_stop.load(Ordering::SeqCst),
            "stop must mark the shutdown before it kills, or the watcher reports it"
        );

        // Long enough for several watcher ticks at the interval `fast` sets.
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert!(
            store.read("probe").unwrap().is_none(),
            "stop clears the record itself"
        );
    }

    #[tokio::test]
    async fn a_sidecar_that_answers_is_returned_running() {
        let dir = tempfile::tempdir().unwrap();
        let store = PidStore::new(dir.path());

        let mut running = start(
            &spec(),
            &long_lived(8178),
            &store,
            &ReadyAfter::immediately(),
            fast(),
        )
        .await
        .unwrap();

        assert_eq!(running.port(), 8178);
        assert!(running.pid() > 0);
        assert!(running.is_running().await);

        running.stop(&store).await.unwrap();
    }

    #[tokio::test]
    async fn the_pid_record_is_written_on_start_and_cleared_on_stop() {
        // The record is what lets the next launch clean up after a crash, so it
        // has to exist while the process does, and not afterwards.
        let dir = tempfile::tempdir().unwrap();
        let store = PidStore::new(dir.path());

        let running = start(
            &spec(),
            &long_lived(8179),
            &store,
            &ReadyAfter::immediately(),
            fast(),
        )
        .await
        .unwrap();

        let record = store
            .read("probe")
            .unwrap()
            .expect("a record while running");
        assert_eq!(record.pid, running.pid());
        assert_eq!(record.port, 8179);

        running.stop(&store).await.unwrap();
        assert_eq!(store.read("probe").unwrap(), None);
    }

    #[tokio::test]
    async fn a_child_that_exits_during_startup_reports_its_exit_code_not_a_timeout() {
        // The behaviour this module exists for. "Did not start in time" throws
        // away the reason — a missing model, an unsupported CPU, a taken port.
        let dir = tempfile::tempdir().unwrap();
        let store = PidStore::new(dir.path());

        let err = start(
            &spec(),
            &exits_with(3, 8180),
            &store,
            &ReadyAfter::never(),
            fast(),
        )
        .await
        .unwrap_err();

        match &err {
            ProcessError::ExitedDuringStartup { exit, name, .. } => {
                assert_eq!(name, "probe");
                assert!(exit.contains('3'), "the exit code should be named: {exit}");
            }
            other => panic!("expected an exit, got {other}"),
        }

        assert_eq!(
            store.read("probe").unwrap(),
            None,
            "a process that never started must not leave a record"
        );
    }

    #[tokio::test]
    async fn a_sidecar_that_never_answers_times_out_and_is_stopped() {
        let dir = tempfile::tempdir().unwrap();
        let store = PidStore::new(dir.path());

        let err = start(
            &spec(),
            &long_lived(8181),
            &store,
            &ReadyAfter::never(),
            fast(),
        )
        .await
        .unwrap_err();

        match &err {
            ProcessError::Timeout { port, .. } => assert_eq!(*port, 8181),
            other => panic!("expected a timeout, got {other}"),
        }
        assert!(err.to_string().contains("rather than"));

        assert_eq!(
            store.read("probe").unwrap(),
            None,
            "the timeout path stops the child, so the record goes with it"
        );
    }

    #[tokio::test]
    async fn a_sidecar_that_takes_a_few_polls_still_starts() {
        let dir = tempfile::tempdir().unwrap();
        let store = PidStore::new(dir.path());

        let health = ReadyAfter {
            after: Some(3),
            probes: AtomicU32::new(0),
        };
        let running = start(&spec(), &long_lived(8182), &store, &health, fast())
            .await
            .unwrap();
        assert!(health.probes.load(Ordering::SeqCst) >= 4);
        running.stop(&store).await.unwrap();
    }

    #[tokio::test]
    async fn a_missing_executable_is_a_spawn_error_naming_it() {
        let dir = tempfile::tempdir().unwrap();
        let store = PidStore::new(dir.path());

        let launch = Launch {
            executable: dir.path().join("not-installed"),
            args: Vec::new(),
            port: 8183,
        };
        let err = start(&spec(), &launch, &store, &ReadyAfter::immediately(), fast())
            .await
            .unwrap_err();

        assert!(matches!(err, ProcessError::Spawn { .. }));
        assert!(err.to_string().contains("not-installed"));
        assert_eq!(store.read("probe").unwrap(), None);
    }

    #[test]
    fn the_diagnostics_tail_is_bounded() {
        // A server failing in a loop must not grow this without limit.
        let mut tail = StderrTail::new();
        for i in 0..10_000 {
            tail.push(&format!("line {i} with some padding to take up room"));
        }
        assert!(
            tail.text().len() <= STDERR_TAIL_BYTES,
            "kept {} bytes",
            tail.text().len()
        );
        assert!(
            tail.text().contains("line 9999"),
            "the most recent lines are the useful ones"
        );
    }

    #[test]
    fn the_diagnostics_tail_keeps_short_output_whole() {
        let mut tail = StderrTail::new();
        tail.push("whisper_init_from_file_with_params_no_state: loading model");
        tail.push("error: failed to open model file");
        assert!(tail.text().contains("failed to open model file"));
        assert_eq!(tail.text().lines().count(), 2);
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod capture_regressions {
    use super::*;
    #[tokio::test]
    async fn a_reader_failure_is_reported_as_incomplete_capture() {
        struct Broken;
        impl tokio::io::AsyncRead for Broken {
            fn poll_read(
                self: std::pin::Pin<&mut Self>,
                _: &mut std::task::Context<'_>,
                _: &mut tokio::io::ReadBuf<'_>,
            ) -> std::task::Poll<std::io::Result<()>> {
                std::task::Poll::Ready(Err(std::io::Error::other("fixture read failure")))
            }
        }
        let result = collect_stderr(Broken, Arc::new(Mutex::new(StderrTail::new()))).await;
        assert!(result.contains("read failed: fixture read failure"));
    }

    #[tokio::test]
    async fn invalid_utf8_and_unterminated_tail_are_preserved() {
        let sink = Arc::new(Mutex::new(StderrTail::new()));
        let mut bytes = vec![b'x'; STDERR_TAIL_BYTES * 3];
        bytes.extend_from_slice(b"\xff final exit detail");
        assert_eq!(
            collect_stderr(bytes.as_slice(), Arc::clone(&sink)).await,
            "complete"
        );
        let tail = sink.lock().unwrap();
        assert_eq!(tail.bytes.len(), STDERR_TAIL_BYTES);
        assert!(tail.text().ends_with("final exit detail"));
        assert!(tail.text().contains(char::REPLACEMENT_CHARACTER));
    }
    #[tokio::test]
    async fn completed_capture_does_not_wait_and_open_capture_is_bounded() {
        let (tx, rx) = tokio::sync::watch::channel(Some("complete".to_owned()));
        assert_eq!(drain_stderr(rx).await, "complete");
        drop(tx);
        let (_tx, rx) = tokio::sync::watch::channel(None);
        assert_eq!(drain_stderr(rx).await, "timed out draining stderr");
    }
    #[test]
    fn old_watcher_cannot_clear_replacement_record() {
        let dir = tempfile::tempdir().unwrap();
        let store = PidStore::new(dir.path());
        let record = PidRecord {
            name: "whisper".to_owned(),
            pid: 2,
            port: 8178,
            executable: "fixture".to_owned(),
        };
        store.write(&record).unwrap();
        store.clear_if_pid("whisper", 1).unwrap();
        assert_eq!(store.read("whisper").unwrap(), Some(record));
        store.clear_if_pid("whisper", 2).unwrap();
        assert!(store.read("whisper").unwrap().is_none());
    }
}
