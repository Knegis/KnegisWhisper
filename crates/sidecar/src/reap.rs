//! Cleaning up sidecars a previous run left behind.
//!
//! The dangerous part of this is not killing a process — it is killing the
//! *wrong* one. Operating systems reuse process ids, so a PID recorded before a
//! crash may belong to something entirely unrelated by the next launch: an
//! editor, a build, a database. Signalling it because a file on disk says it was
//! ours would be a bug that destroys other people's work.
//!
//! So the rule is: **a recorded PID is never signalled on the strength of the
//! record alone.** The live process's command is read first and must contain one
//! of the fragments the sidecar declared. If it does not match, or cannot be
//! read at all, the record is discarded and nothing is signalled. Losing track
//! of a stale sidecar is a leak; killing an innocent process is data loss, and
//! the two are not close enough to trade.
//!
//! The escalation is honest about platforms. On Unix a graceful `SIGTERM` is
//! sent first and `SIGKILL` follows if the process ignores it — a wedged server
//! spinning at full CPU does ignore it. Windows has no graceful equivalent:
//! `TerminateProcess` is immediate and unconditional, so on Windows the first
//! attempt *is* the forceful one and [`Outcome::Terminated`] reports
//! `graceful: false`. Claiming a clean shutdown there would be a lie the caller
//! might build on.

use std::time::Duration;

use crate::manifest::SidecarSpec;
use crate::pid::{PidRecord, PidStore};

/// How long a process gets to honour a graceful signal before being killed.
pub const TERMINATE_GRACE: Duration = Duration::from_secs(5);

/// How long to wait for a forceful kill to take effect before giving up.
pub const KILL_GRACE: Duration = Duration::from_secs(1);

/// How often to check whether a signalled process has gone.
pub const EXIT_POLL_INTERVAL: Duration = Duration::from_millis(200);

/// What a live process looks like to the reaper.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Liveness {
    /// No process holds this id.
    Gone,
    /// A process holds the id, and this is its command line.
    Running { command: String },
    /// A process holds the id but its command could not be read — an access
    /// denied on another user's process, or a race with its exit.
    Unidentifiable,
}

/// The two questions the reaper needs to ask the operating system.
///
/// A trait so every decision below is testable without spawning processes,
/// which is what makes the "never signal an unverified PID" rule provable
/// rather than merely intended.
pub trait ProcessTable {
    fn liveness(&self, pid: u32) -> Liveness;

    /// Request a graceful exit. Returns whether the platform has one at all;
    /// `false` means the call was a forceful termination.
    fn terminate(&self, pid: u32) -> bool;

    /// Stop the process without giving it a chance to clean up.
    fn kill(&self, pid: u32);
}

/// Whether the reaper is allowed to act on a record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// Nothing holds the id. Clear the record.
    AlreadyExited,
    /// It is ours. Signal it.
    OursToKill { command: String },
    /// Something else holds the id now. Clear the record, signal nothing.
    PidReused { command: String },
    /// Alive, but we cannot prove it is ours. Signal nothing.
    Unidentifiable,
}

/// Decide what a record permits, given what the operating system reports.
///
/// Pure, and deliberately the only place the decision is made.
#[must_use]
pub fn verdict(liveness: &Liveness, fragments: &[String]) -> Verdict {
    match liveness {
        Liveness::Gone => Verdict::AlreadyExited,
        Liveness::Unidentifiable => Verdict::Unidentifiable,
        Liveness::Running { command } => {
            let matches = fragments
                .iter()
                .any(|fragment| !fragment.is_empty() && command.contains(fragment.as_str()));
            if matches {
                Verdict::OursToKill {
                    command: command.clone(),
                }
            } else {
                Verdict::PidReused {
                    command: command.clone(),
                }
            }
        }
    }
}

/// What happened to one record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// There was no record for this sidecar.
    NoRecord,
    /// The process had already gone; the record was cleared.
    AlreadyExited,
    /// The id belonged to something else. Nothing was signalled.
    PidReused { command: String },
    /// Alive but unverifiable. Nothing was signalled, and the record was kept
    /// so the next launch can try again.
    LeftAlone,
    /// It exited. `graceful` is false on Windows, and on Unix when it took a
    /// `SIGKILL`.
    Terminated { graceful: bool },
    /// It survived a forceful kill. The record is kept for the next launch.
    SurvivedKill,
}

impl Outcome {
    /// Whether the caller may now consider the sidecar's port free.
    #[must_use]
    pub const fn port_released(&self) -> bool {
        matches!(
            self,
            Self::NoRecord | Self::AlreadyExited | Self::PidReused { .. } | Self::Terminated { .. }
        )
    }
}

/// Timings, so tests do not have to wait five real seconds.
#[derive(Debug, Clone, Copy)]
pub struct Timings {
    pub terminate_grace: Duration,
    pub kill_grace: Duration,
    pub poll_interval: Duration,
}

impl Default for Timings {
    fn default() -> Self {
        Self {
            terminate_grace: TERMINATE_GRACE,
            kill_grace: KILL_GRACE,
            poll_interval: EXIT_POLL_INTERVAL,
        }
    }
}

/// Reap one recorded sidecar.
///
/// Clears the record whenever the id is no longer ours to worry about, and keeps
/// it when the process is still alive and might still need dealing with.
pub async fn reap_one<T: ProcessTable>(
    spec: &SidecarSpec,
    store: &PidStore,
    table: &T,
    timings: Timings,
) -> Outcome {
    let Ok(Some(record)) = store.read(&spec.name) else {
        return Outcome::NoRecord;
    };

    let outcome = act(&record, spec, table, timings).await;

    match outcome {
        Outcome::LeftAlone | Outcome::SurvivedKill => {}
        _ => {
            // Best effort: a record we cannot delete is re-examined next launch,
            // where the same verdict applies. Nothing is lost by ignoring it.
            let _ = store.clear(&spec.name);
        }
    }

    outcome
}

async fn act<T: ProcessTable>(
    record: &PidRecord,
    spec: &SidecarSpec,
    table: &T,
    timings: Timings,
) -> Outcome {
    match verdict(&table.liveness(record.pid), &spec.command_fragments) {
        Verdict::AlreadyExited => Outcome::AlreadyExited,
        Verdict::PidReused { command } => {
            tracing::info!(
                sidecar = %spec.name,
                pid = record.pid,
                "recorded pid now belongs to another process; not signalling it"
            );
            Outcome::PidReused { command }
        }
        Verdict::Unidentifiable => {
            tracing::warn!(
                sidecar = %spec.name,
                pid = record.pid,
                "recorded pid is alive but its command cannot be read; leaving it alone"
            );
            Outcome::LeftAlone
        }
        Verdict::OursToKill { .. } => {
            let graceful = table.terminate(record.pid);
            if wait_for_exit(
                table,
                record.pid,
                timings.terminate_grace,
                timings.poll_interval,
            )
            .await
            {
                return Outcome::Terminated { graceful };
            }

            tracing::warn!(
                sidecar = %spec.name,
                pid = record.pid,
                "stale sidecar did not exit; killing it"
            );
            table.kill(record.pid);
            if wait_for_exit(table, record.pid, timings.kill_grace, timings.poll_interval).await {
                Outcome::Terminated { graceful: false }
            } else {
                Outcome::SurvivedKill
            }
        }
    }
}

async fn wait_for_exit<T: ProcessTable>(
    table: &T,
    pid: u32,
    budget: Duration,
    poll: Duration,
) -> bool {
    let deadline = tokio::time::Instant::now() + budget;
    loop {
        if table.liveness(pid) == Liveness::Gone {
            return true;
        }
        if tokio::time::Instant::now() >= deadline {
            return false;
        }
        tokio::time::sleep(poll).await;
    }
}

/// The real process table.
#[derive(Debug, Default)]
pub struct SystemProcessTable;

impl ProcessTable for SystemProcessTable {
    fn liveness(&self, pid: u32) -> Liveness {
        use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};

        let pid = Pid::from_u32(pid);
        let mut system = System::new();
        system.refresh_processes_specifics(
            ProcessesToUpdate::Some(&[pid]),
            true,
            ProcessRefreshKind::new().with_cmd(sysinfo::UpdateKind::Always),
        );

        let Some(process) = system.process(pid) else {
            return Liveness::Gone;
        };

        // The full command line, not just the image name: a fragment like
        // "whisper-server" should match however the process was invoked.
        let command = process
            .cmd()
            .iter()
            .map(|part| part.to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join(" ");

        if command.is_empty() {
            // Some platforms withhold the command line for processes owned by
            // another user. Unidentifiable, not ours.
            return match process.exe() {
                Some(exe) => Liveness::Running {
                    command: exe.to_string_lossy().into_owned(),
                },
                None => Liveness::Unidentifiable,
            };
        }

        Liveness::Running { command }
    }

    fn terminate(&self, pid: u32) -> bool {
        use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};

        let pid = Pid::from_u32(pid);
        let mut system = System::new();
        system.refresh_processes_specifics(
            ProcessesToUpdate::Some(&[pid]),
            true,
            ProcessRefreshKind::new(),
        );

        let Some(process) = system.process(pid) else {
            return cfg!(unix);
        };

        #[cfg(unix)]
        {
            let _ = process.kill_with(sysinfo::Signal::Term);
            true
        }
        #[cfg(not(unix))]
        {
            // TerminateProcess. Immediate and unconditional -- reported as
            // ungraceful rather than described as a clean shutdown.
            let _ = process.kill();
            false
        }
    }

    fn kill(&self, pid: u32) {
        use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};

        let pid = Pid::from_u32(pid);
        let mut system = System::new();
        system.refresh_processes_specifics(
            ProcessesToUpdate::Some(&[pid]),
            true,
            ProcessRefreshKind::new(),
        );
        if let Some(process) = system.process(pid) {
            let _ = process.kill();
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
    use std::cell::{Cell, RefCell};
    use std::collections::BTreeMap;

    use super::*;
    use crate::manifest::{PortRange, Source, Transport};

    fn spec() -> SidecarSpec {
        SidecarSpec {
            name: "whisper".to_owned(),
            description: "Local speech-to-text".to_owned(),
            source: Source {
                repo: "ggml-org/whisper.cpp".to_owned(),
                release: "b4938".to_owned(),
                pinned_because: "output stability".to_owned(),
            },
            transport: Transport::Loopback {
                port_range: PortRange {
                    first: 8178,
                    last: 8199,
                },
            },
            command_fragments: vec!["whisper-server".to_owned()],
            targets: BTreeMap::new(),
        }
    }

    fn record(pid: u32) -> PidRecord {
        PidRecord {
            name: "whisper".to_owned(),
            pid,
            port: 8178,
            executable: "/kw/whisper/whisper-server".to_owned(),
        }
    }

    /// A process table that behaves like a process rather than replaying a
    /// script.
    ///
    /// The first version of this was a list of states consumed one per query,
    /// and it made the escalation test pass without ever escalating: the
    /// scripted process happened to disappear during the grace period, so
    /// `terminate` looked like it had worked. Modelling *why* a process exits —
    /// it honours the signal, or it does not — is what makes that test able to
    /// fail.
    struct Fake {
        alive: Cell<bool>,
        command: Option<String>,
        honours_terminate: bool,
        survives_kill: bool,
        graceful_platform: bool,
        terminated: RefCell<Vec<u32>>,
        killed: RefCell<Vec<u32>>,
    }

    impl Fake {
        /// A live process with this command line.
        fn running(command: &str) -> Self {
            Self {
                alive: Cell::new(true),
                command: Some(command.to_owned()),
                honours_terminate: true,
                survives_kill: false,
                graceful_platform: true,
                terminated: RefCell::new(Vec::new()),
                killed: RefCell::new(Vec::new()),
            }
        }

        fn gone() -> Self {
            let fake = Self::running("");
            fake.alive.set(false);
            fake
        }

        /// Alive, but the operating system will not say what it is.
        fn unidentifiable() -> Self {
            Self {
                command: None,
                ..Self::running("")
            }
        }

        /// Ignores a graceful signal entirely, as a wedged server does.
        fn wedged(mut self) -> Self {
            self.honours_terminate = false;
            self
        }

        /// Survives even a forceful kill.
        fn unkillable(mut self) -> Self {
            self.honours_terminate = false;
            self.survives_kill = true;
            self
        }

        /// A platform with no graceful signal, i.e. Windows.
        fn without_graceful_signal(mut self) -> Self {
            self.graceful_platform = false;
            self
        }
    }

    impl ProcessTable for Fake {
        fn liveness(&self, _pid: u32) -> Liveness {
            if !self.alive.get() {
                return Liveness::Gone;
            }
            match &self.command {
                Some(command) => Liveness::Running {
                    command: command.clone(),
                },
                None => Liveness::Unidentifiable,
            }
        }

        fn terminate(&self, pid: u32) -> bool {
            self.terminated.borrow_mut().push(pid);
            if self.honours_terminate {
                self.alive.set(false);
            }
            self.graceful_platform
        }

        fn kill(&self, pid: u32) {
            self.killed.borrow_mut().push(pid);
            if !self.survives_kill {
                self.alive.set(false);
            }
        }
    }

    fn running(command: &str) -> Liveness {
        Liveness::Running {
            command: command.to_owned(),
        }
    }

    fn fast() -> Timings {
        Timings {
            terminate_grace: Duration::from_millis(30),
            kill_grace: Duration::from_millis(30),
            poll_interval: Duration::from_millis(5),
        }
    }

    // ---------------------------------------------------------------- verdict

    #[test]
    fn a_matching_command_is_ours() {
        let liveness = running("/kw/whisper/whisper-server --port 8178");
        assert!(matches!(
            verdict(&liveness, &["whisper-server".to_owned()]),
            Verdict::OursToKill { .. }
        ));
    }

    #[test]
    fn a_reused_pid_is_never_ours() {
        // The case this whole module exists for. A PID recorded before a crash
        // now belongs to an editor; killing it would destroy someone's work.
        let liveness = running("C:/Program Files/Editor/editor.exe main.rs");
        assert!(matches!(
            verdict(&liveness, &["whisper-server".to_owned()]),
            Verdict::PidReused { .. }
        ));
    }

    #[test]
    fn an_unreadable_command_is_not_ours_either() {
        // Absence of evidence is not evidence it is safe to kill.
        assert_eq!(
            verdict(&Liveness::Unidentifiable, &["whisper-server".to_owned()]),
            Verdict::Unidentifiable
        );
    }

    #[test]
    fn an_empty_fragment_matches_nothing() {
        // `"".contains("")` is true, so an empty fragment in a config would
        // otherwise match every process on the machine.
        let liveness = running("C:/Program Files/Editor/editor.exe");
        assert!(matches!(
            verdict(&liveness, &[String::new()]),
            Verdict::PidReused { .. }
        ));
    }

    #[test]
    fn no_fragments_at_all_matches_nothing() {
        let liveness = running("/kw/whisper/whisper-server");
        assert!(matches!(verdict(&liveness, &[]), Verdict::PidReused { .. }));
    }

    #[test]
    fn any_one_of_several_fragments_is_enough() {
        // Parakeet ships two binaries, offline and online.
        let fragments = vec![
            "sherpa-onnx-ws-".to_owned(),
            "sherpa-onnx-online-ws-".to_owned(),
        ];
        let liveness = running("/kw/sherpa/sherpa-onnx-online-ws-server --port 6006");
        assert!(matches!(
            verdict(&liveness, &fragments),
            Verdict::OursToKill { .. }
        ));
    }

    // ---------------------------------------------------------------- reaping

    #[tokio::test]
    async fn no_record_is_not_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let store = PidStore::new(dir.path());
        let table = Fake::gone();
        assert_eq!(
            reap_one(&spec(), &store, &table, fast()).await,
            Outcome::NoRecord
        );
    }

    #[tokio::test]
    async fn a_process_that_already_exited_just_clears_the_record() {
        let dir = tempfile::tempdir().unwrap();
        let store = PidStore::new(dir.path());
        store.write(&record(4242)).unwrap();

        let table = Fake::gone();
        assert_eq!(
            reap_one(&spec(), &store, &table, fast()).await,
            Outcome::AlreadyExited
        );
        assert!(table.terminated.borrow().is_empty());
        assert_eq!(store.read("whisper").unwrap(), None, "record cleared");
    }

    #[tokio::test]
    async fn a_reused_pid_is_cleared_without_being_signalled() {
        let dir = tempfile::tempdir().unwrap();
        let store = PidStore::new(dir.path());
        store.write(&record(4242)).unwrap();

        let table = Fake::running("/usr/bin/psql -d production");
        let outcome = reap_one(&spec(), &store, &table, fast()).await;

        assert!(matches!(outcome, Outcome::PidReused { .. }));
        assert!(
            table.terminated.borrow().is_empty() && table.killed.borrow().is_empty(),
            "nothing may be signalled on a pid we cannot prove is ours"
        );
        assert_eq!(store.read("whisper").unwrap(), None);
    }

    #[tokio::test]
    async fn an_unidentifiable_process_is_left_alone_and_the_record_is_kept() {
        let dir = tempfile::tempdir().unwrap();
        let store = PidStore::new(dir.path());
        store.write(&record(4242)).unwrap();

        let table = Fake::unidentifiable();
        assert_eq!(
            reap_one(&spec(), &store, &table, fast()).await,
            Outcome::LeftAlone
        );
        assert!(table.killed.borrow().is_empty());
        assert!(
            store.read("whisper").unwrap().is_some(),
            "keeping the record is what gives the next launch another chance"
        );
    }

    #[tokio::test]
    async fn a_stale_sidecar_that_honours_the_signal_exits_gracefully() {
        let dir = tempfile::tempdir().unwrap();
        let store = PidStore::new(dir.path());
        store.write(&record(4242)).unwrap();

        let table = Fake::running("/kw/whisper/whisper-server --port 8178");
        assert_eq!(
            reap_one(&spec(), &store, &table, fast()).await,
            Outcome::Terminated { graceful: true }
        );
        assert_eq!(*table.terminated.borrow(), vec![4242]);
        assert!(table.killed.borrow().is_empty(), "no escalation was needed");
    }

    #[tokio::test]
    async fn a_wedged_sidecar_is_escalated_to_a_kill() {
        // Observed upstream with qdrant spinning at full CPU: SIGTERM is
        // ignored entirely, so reaping has to verify death and escalate.
        let dir = tempfile::tempdir().unwrap();
        let store = PidStore::new(dir.path());
        store.write(&record(4242)).unwrap();

        let table = Fake::running("/kw/whisper/whisper-server --port 8178").wedged();

        assert_eq!(
            reap_one(&spec(), &store, &table, fast()).await,
            Outcome::Terminated { graceful: false }
        );
        assert_eq!(*table.terminated.borrow(), vec![4242]);
        assert_eq!(*table.killed.borrow(), vec![4242]);
    }

    #[tokio::test]
    async fn a_process_that_survives_a_kill_keeps_its_record() {
        let dir = tempfile::tempdir().unwrap();
        let store = PidStore::new(dir.path());
        store.write(&record(4242)).unwrap();

        let table = Fake::running("/kw/whisper/whisper-server").unkillable();
        let outcome = reap_one(&spec(), &store, &table, fast()).await;

        assert_eq!(outcome, Outcome::SurvivedKill);
        assert!(!outcome.port_released(), "its port is still held");
        assert!(
            store.read("whisper").unwrap().is_some(),
            "dropping the record would lose the only handle on it"
        );
    }

    #[tokio::test]
    async fn a_platform_without_a_graceful_signal_says_so() {
        // Windows: TerminateProcess is immediate. Reporting graceful: true here
        // would be a clean shutdown the caller could wrongly rely on.
        let dir = tempfile::tempdir().unwrap();
        let store = PidStore::new(dir.path());
        store.write(&record(4242)).unwrap();

        let table = Fake::running("/kw/whisper/whisper-server").without_graceful_signal();
        assert_eq!(
            reap_one(&spec(), &store, &table, fast()).await,
            Outcome::Terminated { graceful: false }
        );
    }

    #[test]
    fn only_a_settled_outcome_releases_the_port() {
        assert!(Outcome::AlreadyExited.port_released());
        assert!(Outcome::NoRecord.port_released());
        assert!(Outcome::Terminated { graceful: true }.port_released());
        assert!(Outcome::PidReused {
            command: "x".to_owned()
        }
        .port_released());

        assert!(!Outcome::LeftAlone.port_released());
        assert!(!Outcome::SurvivedKill.port_released());
    }
}
