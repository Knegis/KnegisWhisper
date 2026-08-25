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
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::io::{AsyncBufReadExt as _, BufReader};

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
}

impl Default for StartTimings {
    fn default() -> Self {
        Self {
            timeout: START_TIMEOUT,
            poll_interval: START_POLL_INTERVAL,
        }
    }
}

/// A sidecar that started and answered.
#[derive(Debug)]
pub struct Running {
    name: String,
    port: u16,
    pid: u32,
    executable: PathBuf,
    child: tokio::process::Child,
    stderr: Arc<Mutex<StderrTail>>,
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
    pub fn is_running(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
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
    pub async fn stop(mut self, store: &PidStore) -> Result<(), ProcessError> {
        // `kill` is TerminateProcess on Windows and SIGKILL on Unix. These
        // servers hold no state that needs flushing — the model is read-only and
        // an in-flight request is one we are abandoning on purpose — so there is
        // nothing a graceful shutdown would protect.
        let _ = self.child.kill().await;
        let _ = self.child.wait().await;
        store.clear(&self.name)?;
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

    if let Some(pipe) = child.stderr.take() {
        let sink = Arc::clone(&stderr);
        tokio::spawn(async move {
            let mut lines = BufReader::new(pipe).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                if let Ok(mut tail) = sink.lock() {
                    tail.push(&line);
                }
            }
        });
    }

    store.write(&PidRecord {
        name: spec.name.clone(),
        pid,
        port: launch.port,
        executable: launch.executable.display().to_string(),
    })?;

    let mut running = Running {
        name: spec.name.clone(),
        port: launch.port,
        pid,
        executable: launch.executable.clone(),
        child,
        stderr,
    };

    let deadline = tokio::time::Instant::now() + timings.timeout;
    loop {
        if health.is_ready(launch.port).await {
            tracing::info!(sidecar = %spec.name, port = launch.port, pid, "sidecar ready");
            return Ok(running);
        }

        // Checked after the health probe, so a server that became ready and then
        // exited in the same instant is reported as ready rather than as a
        // startup crash.
        if let Ok(Some(status)) = running.child.try_wait() {
            let _ = store.clear(&spec.name);
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

fn describe(status: &std::process::ExitStatus) -> String {
    match status.code() {
        Some(code) => format!("exit code {code}"),
        None => "no exit code (killed by a signal)".to_owned(),
    }
}

/// A bounded ring of the most recent diagnostics.
#[derive(Debug, Default)]
struct StderrTail {
    lines: std::collections::VecDeque<String>,
    bytes: usize,
}

impl StderrTail {
    fn new() -> Self {
        Self::default()
    }

    fn push(&mut self, line: &str) {
        self.bytes += line.len() + 1;
        self.lines.push_back(line.to_owned());
        while self.bytes > STDERR_TAIL_BYTES {
            match self.lines.pop_front() {
                Some(dropped) => self.bytes -= dropped.len() + 1,
                None => break,
            }
        }
    }

    fn text(&self) -> String {
        self.lines
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>()
            .join("\n")
    }
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
        assert!(running.is_running());

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
