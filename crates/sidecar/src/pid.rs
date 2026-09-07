//! Remembering which processes we started, so they can be cleaned up after a
//! crash.
//!
//! A sidecar outlives an application that dies without running its shutdown
//! path — a panic, a `SIGKILL`, a power cut. On the next launch the only trace
//! is a PID, so it gets written to disk at spawn and removed at exit.
//!
//! A PID on its own is not enough to act on, because the operating system
//! reuses them. So each record also carries the executable path, and
//! `crate::reap` will not signal a process whose command does not match. See
//! that module for why the check is the important half.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use kw_security::file_acl::{self, PathKind};
use serde::{Deserialize, Serialize};

/// Subdirectory of the application data directory holding the records.
pub const PID_DIR_NAME: &str = "sidecar-pids";

/// What we know about a process we started.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PidRecord {
    /// The sidecar's name, matching `SidecarSpec::name`.
    pub name: String,
    pub pid: u32,
    /// The loopback port it was told to listen on.
    pub port: u16,
    /// The executable we launched. Compared against the live process's command
    /// before any signal is sent.
    pub executable: String,
}

/// Why a PID record could not be read or written.
#[derive(Debug, thiserror::Error)]
pub enum PidError {
    #[error("could not create the PID directory at {path}: {source}")]
    CreateDir {
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    #[error("could not write the PID file at {path}: {source}")]
    Write {
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    #[error("could not read the PID directory at {path}: {source}")]
    ReadDir {
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    #[error("{path} is not a PID record this version wrote: {detail}")]
    Malformed { path: PathBuf, detail: String },

    #[error("{name:?} is not usable as a file name")]
    UnsafeName { name: String },
}

/// The on-disk store of PID records.
#[derive(Debug, Clone)]
pub struct PidStore {
    dir: PathBuf,
}

static RECORD_MUTATION: std::sync::Mutex<()> = std::sync::Mutex::new(());

impl PidStore {
    /// Open the store under an application data directory. Does not touch the
    /// filesystem; the directory is created on first write.
    #[must_use]
    pub fn new(app_data_dir: &Path) -> Self {
        Self {
            dir: app_data_dir.join(PID_DIR_NAME),
        }
    }

    #[must_use]
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Path of one record. Rejects a name that could escape the directory.
    fn path_for(&self, name: &str) -> Result<PathBuf, PidError> {
        let safe = !name.is_empty()
            && name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
        if !safe {
            return Err(PidError::UnsafeName {
                name: name.to_owned(),
            });
        }
        Ok(self.dir.join(format!("{name}.pid")))
    }

    /// Record a process we just started, replacing any previous record.
    pub fn write(&self, record: &PidRecord) -> Result<(), PidError> {
        let _lock = RECORD_MUTATION
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let path = self.path_for(&record.name)?;

        fs::create_dir_all(&self.dir).map_err(|source| PidError::CreateDir {
            path: self.dir.clone(),
            source,
        })?;
        restrict(&self.dir, PathKind::Directory);

        let body = serde_json::to_string_pretty(record).map_err(|source| PidError::Write {
            path: path.clone(),
            source: io::Error::other(source),
        })?;

        fs::write(&path, body).map_err(|source| PidError::Write {
            path: path.clone(),
            source,
        })?;
        restrict(&path, PathKind::File);

        Ok(())
    }

    /// Forget a record. Absent is success — the caller wants it gone, and it is.
    pub fn clear(&self, name: &str) -> Result<(), PidError> {
        let _lock = RECORD_MUTATION
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        self.clear_unlocked(name)
    }

    /// A watcher may only remove the record of the process it observed.
    pub fn clear_if_pid(&self, name: &str, pid: u32) -> Result<(), PidError> {
        let _lock = RECORD_MUTATION
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if self.read(name)?.is_some_and(|record| record.pid == pid) {
            self.clear_unlocked(name)?;
        }
        Ok(())
    }

    fn clear_unlocked(&self, name: &str) -> Result<(), PidError> {
        let path = self.path_for(name)?;
        match fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(source) => Err(PidError::Write { path, source }),
        }
    }

    /// Read one record, or `None` if there is no record for that name.
    pub fn read(&self, name: &str) -> Result<Option<PidRecord>, PidError> {
        let path = self.path_for(name)?;
        let body = match fs::read_to_string(&path) {
            Ok(body) => body,
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(source) => return Err(PidError::Write { path, source }),
        };
        parse(&path, &body).map(Some)
    }

    /// Every readable record, plus the paths of any that could not be parsed.
    ///
    /// Unreadable records are returned rather than swallowed: a file we cannot
    /// parse may be a live sidecar we are about to lose track of, and the honest
    /// report is what lets the caller say so instead of silently leaking a
    /// process.
    pub fn read_all(&self) -> Result<Inventory, PidError> {
        let entries = match fs::read_dir(&self.dir) {
            Ok(entries) => entries,
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(Inventory::default()),
            Err(source) => {
                return Err(PidError::ReadDir {
                    path: self.dir.clone(),
                    source,
                })
            }
        };

        let mut inventory = Inventory::default();
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_none_or(|ext| ext != "pid") {
                continue;
            }
            match fs::read_to_string(&path).map_err(|e| e.to_string()) {
                Ok(body) => match parse(&path, &body) {
                    Ok(record) => inventory.records.push(record),
                    Err(err) => inventory.unreadable.push((path, err.to_string())),
                },
                Err(detail) => inventory.unreadable.push((path, detail)),
            }
        }

        inventory.records.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(inventory)
    }
}

/// What `read_all` found.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Inventory {
    pub records: Vec<PidRecord>,
    /// Files that look like records but could not be read, with the reason.
    pub unreadable: Vec<(PathBuf, String)>,
}

fn parse(path: &Path, body: &str) -> Result<PidRecord, PidError> {
    let record: PidRecord = serde_json::from_str(body).map_err(|source| PidError::Malformed {
        path: path.to_path_buf(),
        detail: source.to_string(),
    })?;
    if record.pid == 0 {
        return Err(PidError::Malformed {
            path: path.to_path_buf(),
            detail: "pid 0 is not a process".to_owned(),
        });
    }
    Ok(record)
}

/// Narrow who may write here, and log it if that did not happen.
///
/// A PID is not a secret, so this is not protecting a credential — it is
/// protecting **integrity**. A record another user could rewrite is a request to
/// signal a PID of their choosing, and `crate::reap` acts on what it reads.
///
/// This used to say *"on Windows the file inherits the user profile's ACL, which
/// already excludes other users"*, and that is false on a real machine:
/// `icacls` on `%USERPROFILE%\.knegiswhisper` found two trustees with Modify
/// that were neither the user nor administrators, every entry inherited. The
/// mechanism now comes from `kw_security::file_acl`, which applies an explicit
/// protected list on Windows as well as `0600`/`0700` on Unix — and reads it
/// back, so a claim like the old one cannot be made again without evidence.
///
/// The verdict is logged rather than returned. Unlike the discovery file, whose
/// permissions *are* its access control and which therefore reports them to the
/// user, a PID record's worst case is a stale or forged entry that
/// `crate::reap`'s command-line check already refuses to act on. A sidecar that
/// would not start because its PID directory has an odd access list would be a
/// worse outcome than the exposure.
fn restrict(path: &Path, kind: PathKind) {
    if let Err(error) = file_acl::apply(path, kind) {
        tracing::warn!(path = %path.display(), %error, "could not restrict the PID record");
        return;
    }
    let verdict = file_acl::verify(path, kind);
    if !verdict.is_confirmed() {
        tracing::warn!(
            path = %path.display(),
            permissions = %verdict.explain(),
            "a PID record another local user could rewrite is a record that can name any process"
        );
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

    fn record(name: &str, pid: u32) -> PidRecord {
        PidRecord {
            name: name.to_owned(),
            pid,
            port: 8178,
            executable: "C:/kw/whisper/whisper-server.exe".to_owned(),
        }
    }

    #[test]
    fn a_record_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let store = PidStore::new(dir.path());
        let written = record("whisper", 4242);

        store.write(&written).unwrap();
        assert_eq!(store.read("whisper").unwrap(), Some(written));
    }

    #[test]
    fn the_directory_is_created_on_first_write() {
        let dir = tempfile::tempdir().unwrap();
        let store = PidStore::new(dir.path());
        assert!(!store.dir().exists());
        store.write(&record("whisper", 1)).unwrap();
        assert!(store.dir().is_dir());
    }

    #[test]
    fn reading_a_missing_record_is_not_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let store = PidStore::new(dir.path());
        assert_eq!(store.read("whisper").unwrap(), None);
    }

    #[test]
    fn read_all_on_a_store_that_was_never_written_is_empty() {
        // The first launch on a new machine. Must not look like a failure.
        let dir = tempfile::tempdir().unwrap();
        let inventory = PidStore::new(dir.path()).read_all().unwrap();
        assert_eq!(inventory, Inventory::default());
    }

    #[test]
    fn clearing_an_absent_record_succeeds() {
        // The caller wants it gone, and it is.
        let dir = tempfile::tempdir().unwrap();
        PidStore::new(dir.path()).clear("whisper").unwrap();
    }

    #[test]
    fn a_write_replaces_the_previous_record() {
        let dir = tempfile::tempdir().unwrap();
        let store = PidStore::new(dir.path());
        store.write(&record("whisper", 1)).unwrap();
        store.write(&record("whisper", 2)).unwrap();
        assert_eq!(store.read("whisper").unwrap().unwrap().pid, 2);
    }

    #[test]
    fn read_all_returns_every_record_sorted() {
        let dir = tempfile::tempdir().unwrap();
        let store = PidStore::new(dir.path());
        store.write(&record("whisper", 1)).unwrap();
        store.write(&record("llama", 2)).unwrap();
        store.write(&record("qdrant", 3)).unwrap();

        let inventory = store.read_all().unwrap();
        let names: Vec<&str> = inventory.records.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, ["llama", "qdrant", "whisper"]);
        assert!(inventory.unreadable.is_empty());
    }

    #[test]
    fn an_unparseable_record_is_reported_rather_than_skipped() {
        // A file we cannot read might be a live sidecar. Silently ignoring it
        // leaks the process; reporting it lets the caller say so.
        let dir = tempfile::tempdir().unwrap();
        let store = PidStore::new(dir.path());
        store.write(&record("whisper", 1)).unwrap();
        fs::write(store.dir().join("llama.pid"), "not json at all").unwrap();

        let inventory = store.read_all().unwrap();
        assert_eq!(inventory.records.len(), 1);
        assert_eq!(inventory.unreadable.len(), 1);
        assert!(inventory.unreadable[0].0.ends_with("llama.pid"));
    }

    #[test]
    fn files_that_are_not_pid_records_are_left_alone() {
        let dir = tempfile::tempdir().unwrap();
        let store = PidStore::new(dir.path());
        store.write(&record("whisper", 1)).unwrap();
        fs::write(store.dir().join("README.txt"), "notes").unwrap();

        let inventory = store.read_all().unwrap();
        assert_eq!(inventory.records.len(), 1);
        assert!(inventory.unreadable.is_empty());
    }

    #[test]
    fn pid_zero_is_refused_on_read() {
        let dir = tempfile::tempdir().unwrap();
        let store = PidStore::new(dir.path());
        fs::create_dir_all(store.dir()).unwrap();
        fs::write(
            store.dir().join("whisper.pid"),
            r#"{"name":"whisper","pid":0,"port":8178,"executable":"x"}"#,
        )
        .unwrap();

        let err = store.read("whisper").unwrap_err();
        assert!(err.to_string().contains("pid 0"));
    }

    #[test]
    fn a_name_that_could_escape_the_directory_is_refused() {
        // A record path is derived from a name that reaches this from a config
        // file, so traversal is refused rather than normalised.
        let dir = tempfile::tempdir().unwrap();
        let store = PidStore::new(dir.path());
        for name in ["../../etc/passwd", "a/b", "a\\b", "", "a b", "a.pid"] {
            assert!(
                store.read(name).is_err(),
                "{name:?} must not resolve to a path"
            );
        }
    }

    #[test]
    fn a_record_carries_the_executable_so_a_reused_pid_can_be_told_apart() {
        // The property crate::reap depends on.
        let dir = tempfile::tempdir().unwrap();
        let store = PidStore::new(dir.path());
        store.write(&record("whisper", 4242)).unwrap();
        let read = store.read("whisper").unwrap().unwrap();
        assert!(read.executable.contains("whisper-server"));
    }

    #[cfg(unix)]
    #[test]
    fn a_record_is_owner_only_on_unix() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let store = PidStore::new(dir.path());
        store.write(&record("whisper", 1)).unwrap();

        let mode = fs::metadata(store.dir().join("whisper.pid"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(
            mode & 0o777,
            0o600,
            "a record another user can rewrite is a request to signal a PID of their choosing"
        );
    }
}
