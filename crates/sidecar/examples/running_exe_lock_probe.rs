//! What Windows lets you do to a directory that holds a *running* executable.
//!
//! `install::install` finishes by removing the old install directory and
//! renaming the freshly extracted one into its place. On Windows that removal
//! is the step that fails while the sidecar it is replacing is running, and the
//! whole shape of "refresh a sidecar" depends on exactly which operations the
//! kernel refuses. Guessing produced one wrong design already, so this measures
//! it, and it is kept so the next person does not have to take the sentence on
//! trust.
//!
//! ```text
//! cargo run -p kw-sidecar --example running_exe_lock_probe
//! ```
//!
//! # The two ways to get this wrong, both of which happened
//!
//! **A process that has already exited proves nothing.** The first attempt at
//! this measurement (2026-08-31, another session) used `timeout.exe`, which
//! exits immediately when it has no console to wait on. Every case "passed" and
//! the result was discarded as meaningless. So this probe starts `ping -n 40`,
//! and then *confirms the pid is in the process table with its executable path
//! pointing inside the directory under test* before it measures anything. A
//! case whose process died is reported as `INVALID`, never as a result.
//!
//! **A probe with no control reports the failure mode of the probe.**
//! `kw_storage`'s `native_path_probe` says the same thing for the same reason:
//! its first version reported failure for every case including the control, and
//! the control is the only reason that was caught. Here the control is the same
//! directory holding the same executable *not* running. If `remove` fails for
//! the control, the answer for the live case is worthless.
//!
//! # What it found on 2026-09-01, Windows 11 Pro 26200
//!
//! ```text
//! control  remove a directory whose exe is not running   Ok
//! control  rename a directory whose exe is not running   Ok
//! live     remove a directory whose exe is running       Err os error 5 (Access is denied)
//! live     rename a directory whose exe is running       Ok
//! live     remove it after renaming it aside             Err os error 5 (Access is denied)
//! live     create a new directory at the vacated name    Ok
//! ```
//!
//! The lock is on the mapped image file, not on the path and not on the parent
//! directory. So renaming a running sidecar aside works and *deleting it does
//! not become possible by having moved it* — a rename-aside strategy inherits a
//! directory that stays undeletable until the process ends, which is what makes
//! it need a cleanup pass somewhere. See KNEG-113.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Child;

fn main() {
    if !cfg!(windows) {
        println!(
            "This is a Windows question. Unix unlinks a running executable's file without \
             complaint, so there is nothing here to measure."
        );
        return;
    }

    let Some(root) = scratch_root() else {
        println!("could not make a scratch directory; nothing measured");
        return;
    };
    println!("scratch: {}", root.display());
    println!();

    let mut report: Vec<(&str, &str, String)> = Vec::new();

    // ---------------------------------------------------------- the controls
    //
    // The same directory, the same executable, no process. Anything but `Ok`
    // here means the probe is measuring itself and every line below it is void.
    match lay_down(&root, "control-remove") {
        Some(dir) => report.push(("control", "remove, exe not running", outcome(remove(&dir)))),
        None => report.push((
            "control",
            "remove, exe not running",
            "INVALID setup".to_owned(),
        )),
    }
    match lay_down(&root, "control-rename") {
        Some(dir) => {
            let to = root.join("control-rename.moved");
            report.push((
                "control",
                "rename, exe not running",
                outcome(rename(&dir, &to)),
            ));
        }
        None => report.push((
            "control",
            "rename, exe not running",
            "INVALID setup".to_owned(),
        )),
    }

    // ------------------------------------------------------- the live cases
    match live_case(&root, "live-remove") {
        Ok(Live { dir, mut child }) => {
            report.push(("live", "remove, exe running", outcome(remove(&dir))));
            let _ = child.kill();
            let _ = child.wait();
        }
        Err(why) => report.push(("live", "remove, exe running", format!("INVALID: {why}"))),
    }

    match live_case(&root, "live-rename") {
        Ok(Live { dir, mut child }) => {
            let aside = root.join("live-rename.replaced");
            report.push(("live", "rename, exe running", outcome(rename(&dir, &aside))));

            // Does moving it make it deletable? This is the question a
            // rename-aside install has to answer before it promises a cleanup
            // pass will ever succeed.
            report.push((
                "live",
                "remove after renaming aside",
                outcome(remove(&aside)),
            ));

            // And is the vacated name usable immediately?
            let fresh = fs::create_dir_all(&dir).and_then(|()| fs::write(dir.join("marker"), b"1"));
            report.push(("live", "create at the vacated name", outcome(fresh)));

            let _ = child.kill();
            let _ = child.wait();
        }
        Err(why) => report.push(("live", "rename, exe running", format!("INVALID: {why}"))),
    }

    println!("{:<9} {:<44} result", "kind", "operation");
    for (kind, what, result) in &report {
        println!("{kind:<9} {what:<44} {result}");
    }
    println!();

    // Best effort: the live directories hold executables whose processes have
    // only just been killed, and Windows can take a moment to release them.
    let _ = fs::remove_dir_all(&root);
    println!("scratch removed (best effort)");
}

/// A directory under test and the process running out of it.
struct Live {
    dir: PathBuf,
    child: Child,
}

fn scratch_root() -> Option<PathBuf> {
    let root = std::env::temp_dir().join(format!("kw-running-exe-probe-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).ok()?;
    Some(root)
}

/// The executable this probe copies and runs.
///
/// `ping` rather than `timeout`: `timeout.exe` exits at once without a console,
/// which is how the first attempt at this measurement produced six meaningless
/// passes. `ping -n 40 127.0.0.1` runs for about thirty-nine seconds whether or
/// not anyone is watching.
fn source_exe() -> PathBuf {
    let system_root = std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".to_owned());
    Path::new(&system_root).join("System32").join("PING.EXE")
}

/// Make a directory holding a copy of the executable. No process.
fn lay_down(root: &Path, name: &str) -> Option<PathBuf> {
    let dir = root.join(name);
    fs::create_dir_all(&dir).ok()?;
    fs::copy(source_exe(), dir.join("probe.exe")).ok()?;
    Some(dir)
}

/// Make a directory, start the copy inside it, and prove it is really running.
fn live_case(root: &Path, name: &str) -> Result<Live, String> {
    let dir = lay_down(root, name).ok_or_else(|| "could not lay down the directory".to_owned())?;
    let exe = dir.join("probe.exe");

    let child = std::process::Command::new(&exe)
        .args(["-n", "40", "127.0.0.1"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|err| format!("could not start {}: {err}", exe.display()))?;

    // Give it a moment to be scheduled, then insist on seeing it. A `Child`
    // handle stays valid after the process exits, so `spawn` succeeding is not
    // evidence of anything.
    std::thread::sleep(std::time::Duration::from_millis(750));
    confirm_in_process_table(child.id(), &exe)?;

    Ok(Live { dir, child })
}

/// Insist the pid is in the process table and its image is the file under test.
///
/// This is the check the discarded 2026-08-31 attempt did not have.
fn confirm_in_process_table(pid: u32, exe: &Path) -> Result<(), String> {
    use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};

    let pid = Pid::from_u32(pid);
    let mut system = System::new();
    system.refresh_processes_specifics(
        ProcessesToUpdate::Some(&[pid]),
        true,
        ProcessRefreshKind::new().with_exe(sysinfo::UpdateKind::Always),
    );

    let Some(process) = system.process(pid) else {
        return Err(format!("pid {pid} is not in the process table"));
    };
    match process.exe() {
        Some(found) if found == exe => Ok(()),
        Some(found) => Err(format!(
            "pid {pid} is running {} rather than {}",
            found.display(),
            exe.display()
        )),
        None => Err(format!("pid {pid} would not say what it is running")),
    }
}

/// Exactly what `install::install` does to the old directory.
fn remove(dir: &Path) -> std::io::Result<()> {
    fs::remove_dir_all(dir)
}

fn rename(from: &Path, to: &Path) -> std::io::Result<()> {
    fs::rename(from, to)
}

fn outcome(result: std::io::Result<()>) -> String {
    match result {
        Ok(()) => "Ok".to_owned(),
        Err(err) => match err.raw_os_error() {
            Some(code) => format!("Err os error {code} ({err})"),
            None => format!("Err ({err})"),
        },
    }
}
