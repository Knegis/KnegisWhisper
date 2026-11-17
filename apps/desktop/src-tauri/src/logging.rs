//! Log output, to the terminal and to a file, and a panic written down before
//! the process ends.
//!
//! A release build has no console (`main.rs`), so until this existed its log
//! went nowhere, and a release panic aborts (`panic = "abort"`): an
//! application that closed on somebody else's computer left nothing to read.
//! That is how a meeting lost at the end of Stop came to be investigated from
//! the code alone.
//!
//! One file, in the application's own data directory, on this computer. It is
//! started again once it passes [`MAX_BYTES`] -- at launch, and while the
//! application runs, which for something that sits in the tray is most of the
//! time -- with the one before kept beside it, so the two together are bounded
//! at about twice that. Nothing sends it anywhere; a person reporting a crash
//! attaches it.

use std::fs::{File, OpenOptions};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// Past this, the file is set aside as `knegiswhisper.log.1` and a new one started.
const MAX_BYTES: u64 = 5 * 1024 * 1024;

/// Where the log is written.
pub fn log_path() -> PathBuf {
    kw_storage::paths::data_dir()
        .join("logs")
        .join("knegiswhisper.log")
}

/// Open `path` to append to, setting a file past `max_bytes` aside first.
fn open(path: &Path, max_bytes: u64) -> std::io::Result<File> {
    if let Some(directory) = path.parent() {
        std::fs::create_dir_all(directory)?;
    }
    if std::fs::metadata(path).is_ok_and(|metadata| metadata.len() > max_bytes) {
        set_aside(path);
    }
    OpenOptions::new().create(true).append(true).open(path)
}

/// Rename `path` to `path.1`, replacing the one before it.
fn set_aside(path: &Path) -> bool {
    let mut previous = path.as_os_str().to_os_string();
    previous.push(".1");
    std::fs::rename(path, PathBuf::from(previous)).is_ok()
}

/// The log file, kept under its bound while the application runs.
///
/// It counts what it writes and, past `max_bytes`, closes the file, sets it
/// aside and opens a new one. If that cannot be done -- the file is held open
/// by something else, the directory is gone -- it empties the file in place
/// instead, and failing that carries on appending: a log that grows, or loses
/// its oldest lines, is better than one that stops, and nothing here may fail
/// the write it was asked to make.
struct Bounded {
    path: PathBuf,
    max_bytes: u64,
    /// `None` only between closing the old file and opening the new one.
    file: Option<File>,
    written: u64,
}

impl Bounded {
    fn open(path: PathBuf, max_bytes: u64) -> std::io::Result<Self> {
        let file = open(&path, max_bytes)?;
        let written = file.metadata().map(|metadata| metadata.len()).unwrap_or(0);
        Ok(Self {
            path,
            max_bytes,
            file: Some(file),
            written,
        })
    }

    fn start_again(&mut self) {
        // Closed first: Windows will not rename a file that is open.
        self.file = None;
        let fresh = if set_aside(&self.path) {
            OpenOptions::new()
                .create(true)
                .append(true)
                .open(&self.path)
        } else {
            OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(true)
                .open(&self.path)
        };
        self.file = fresh
            .or_else(|_| {
                OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&self.path)
            })
            .ok();
        // Counted from here whichever of those happened, so a file that could
        // not be replaced is tried again after another `max_bytes`, not on
        // every line.
        self.written = 0;
    }
}

impl std::io::Write for Bounded {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if self.written > self.max_bytes {
            self.start_again();
        }
        let Some(file) = self.file.as_mut() else {
            // Nowhere to write: say it was written, so logging goes on to the
            // terminal and the next line tries the file again.
            self.written = self.max_bytes.saturating_add(1);
            return Ok(bytes.len());
        };
        let count = file.write(bytes)?;
        self.written = self.written.saturating_add(count as u64);
        Ok(count)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        match self.file.as_mut() {
            Some(file) => file.flush(),
            None => Ok(()),
        }
    }
}

/// Install log output and the panic record.
///
/// Honours `KNEGISWHISPER_LOG`, defaulting to `info`. Errors are swallowed
/// because a second initialisation attempt (in tests, or a re-entrant setup) is
/// harmless and must not take the app down, and because an application that
/// cannot write its log should still start.
pub fn init() {
    use tracing_subscriber::prelude::*;
    use tracing_subscriber::{fmt, EnvFilter};

    let filter =
        EnvFilter::try_from_env("KNEGISWHISPER_LOG").unwrap_or_else(|_| EnvFilter::new("info"));
    let path = log_path();
    let file = Bounded::open(path.clone(), MAX_BYTES).ok().map(|file| {
        fmt::layer()
            .with_ansi(false)
            .with_target(false)
            .with_writer(Mutex::new(file))
    });
    let written = file.is_some();
    let _ = tracing_subscriber::registry()
        .with(filter)
        .with(fmt::layer().with_target(false))
        .with(file)
        .try_init();
    if written {
        record_panics(path);
        tracing::info!(version = env!("CARGO_PKG_VERSION"), "KnegisWhisper started");
    }
}

/// Write a panic to the log before the default hook runs and, in a release
/// build, the process aborts. Appended directly, not through `tracing`: the
/// panic may be in the middle of a log call.
fn record_panics(path: PathBuf) {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        write_panic(&path, &info.to_string());
        default(info);
    }));
}

fn write_panic(path: &Path, panic: &str) {
    let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) else {
        return;
    };
    let backtrace = std::backtrace::Backtrace::force_capture();
    let _ = writeln!(
        file,
        "{} PANIC {panic}\n{backtrace}",
        chrono::Utc::now().to_rfc3339()
    );
    let _ = file.flush();
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn the_log_is_appended_to_until_it_is_too_long_and_then_started_again() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("logs").join("knegiswhisper.log");
        let previous = dir.path().join("logs").join("knegiswhisper.log.1");

        open(&path, 10).unwrap().write_all(b"first").unwrap();
        open(&path, 10).unwrap().write_all(b" run").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "first run");
        assert!(!previous.exists());

        open(&path, 10).unwrap().write_all(b", more").unwrap();
        // Fifteen bytes is past ten: the next launch starts a new file.
        open(&path, 10).unwrap().write_all(b"second").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "second");
        assert_eq!(
            std::fs::read_to_string(&previous).unwrap(),
            "first run, more"
        );
    }

    #[test]
    fn the_log_is_kept_under_its_bound_while_the_application_runs() {
        // The defect: the size was checked only at launch, so an application
        // left running wrote past it without end.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("knegiswhisper.log");
        let previous = dir.path().join("knegiswhisper.log.1");
        let mut log = Bounded::open(path.clone(), 10).unwrap();

        for _ in 0..50 {
            log.write_all(b"a line\n").unwrap();
        }
        log.flush().unwrap();

        // Never more than the bound plus the line that crossed it, in either file.
        assert!(std::fs::metadata(&path).unwrap().len() <= 10 + 7);
        assert!(std::fs::metadata(&previous).unwrap().len() <= 10 + 7);
        assert!(std::fs::read_to_string(&path)
            .unwrap()
            .ends_with("a line\n"));
    }

    #[test]
    fn a_log_that_cannot_be_set_aside_is_emptied_and_writing_goes_on() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("knegiswhisper.log");
        // A directory where the set-aside file would go: the rename fails.
        std::fs::create_dir(dir.path().join("knegiswhisper.log.1")).unwrap();
        std::fs::write(dir.path().join("knegiswhisper.log.1").join("held"), b"x").unwrap();
        let mut log = Bounded::open(path.clone(), 10).unwrap();

        for _ in 0..50 {
            log.write_all(b"a line\n").unwrap();
        }
        log.flush().unwrap();

        assert!(std::fs::metadata(&path).unwrap().len() <= 10 + 7);
    }

    #[test]
    fn a_panic_is_written_with_when_and_where() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("knegiswhisper.log");
        write_panic(&path, "panicked at src/meeting.rs:1:1: index out of bounds");
        let written = std::fs::read_to_string(&path).unwrap();
        assert!(written.contains(" PANIC panicked at src/meeting.rs:1:1: index out of bounds"));
    }

    #[test]
    fn the_log_is_in_the_applications_own_data_directory() {
        assert!(log_path().starts_with(kw_storage::paths::data_dir()));
        assert!(log_path().ends_with("logs/knegiswhisper.log"));
    }
}
