//! The discovery file: how a client finds the local API and proves it may use
//! it.
//!
//! `~/.knegiswhisper/local-api.json` holds the port and the bearer token for the
//! running application. Anything that can read that file can use the API, so the
//! file *is* the access control and its permissions are the enforcement.
//!
//! Permissions are read back rather than assumed. Setting a mode and reporting
//! success is the same mistake as calling a capture-exclusion API and reporting
//! `active` without checking — `chmod` can be ignored on an exotic filesystem,
//! and a file on a Windows network share does not behave like one in a home
//! directory. So [`write`] returns [`Permissions`], which says what was actually
//! achieved, and the caller can tell the user the truth.
//!
//! The mechanism is `kw_security::file_acl`, shared with `kw_sidecar::pid`:
//! `0600` on Unix, and on Windows an explicit protected DACL granting this
//! process's user, `NT AUTHORITY\SYSTEM` and `BUILTIN\Administrators` and
//! nobody else. Not owner-only — an administrator can take ownership and
//! re-grant themselves regardless — so what a confirmed Windows result says is
//! that the *unexpected* trustees are gone and the inheritance that handed them
//! down is off, both read back.
//!
//! Two orderings in [`write`] are load-bearing and easy to lose:
//!
//! 1. **The directory is restricted before the temporary is created**, so the
//!    temporary inherits a protected access list and the token's bytes never
//!    touch disk under the loose one. The empty temporary is then restricted in
//!    its own right before the token is written into it, which closes the same
//!    window on Unix, where the directory's mode does not stop a `0644` file.
//! 2. **The read-back happens after the rename**, against the path the caller
//!    is told about. Verifying the temporary would describe a path that no
//!    longer exists by the time the verdict is returned — true of the final file
//!    only by an argument the code never made. Measured on Windows: a rename
//!    within one directory keeps the file object and its whole security
//!    descriptor, explicit entries and `SE_DACL_PROTECTED` included.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use kw_security::file_acl::{self, PathKind};
use kw_security::LocalApiToken;
use serde::{Deserialize, Serialize};

/// What the filesystem actually granted.
///
/// Defined in `kw-security` next to the mechanism that produces it and
/// re-exported here, so `kw_sidecar::pid` — which protects a PID record for
/// integrity rather than a token for confidentiality — reports through the same
/// vocabulary instead of a second one that could drift from this.
pub use kw_security::Permissions;

/// Directory under the user's home that holds the file.
pub const DIRECTORY_NAME: &str = ".knegiswhisper";

/// The file itself.
pub const FILE_NAME: &str = "local-api.json";

/// Bumped when the shape changes in a way an older client could misread.
pub const DISCOVERY_VERSION: u32 = 1;

/// What a client needs to reach the API.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Discovery {
    pub version: u32,
    pub port: u16,
    /// The bearer token. This is why the file's permissions are narrowed and
    /// read back at all.
    pub token: String,
}

/// Why the discovery file could not be written or read.
#[derive(Debug, thiserror::Error)]
pub enum DiscoveryError {
    #[error("Local API permissions could not be secured: {verdict:?}. Correct the permissions and retry.")]
    Permissions { verdict: Permissions },
    #[error(
        "could not work out where your home directory is, so there is nowhere to put {FILE_NAME}"
    )]
    NoHomeDirectory,

    #[error("could not {action} {path}: {source}")]
    Io {
        action: &'static str,
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    #[error("{path} is not a discovery file this version wrote: {detail}")]
    Malformed { path: PathBuf, detail: String },

    #[error(
        "{path} was written by version {found}, and this build understands {DISCOVERY_VERSION}"
    )]
    Version { path: PathBuf, found: u32 },

    #[error("the token in {path} is not usable: {source}")]
    Token {
        path: PathBuf,
        #[source]
        source: kw_security::TokenError,
    },
}

/// The default location of the discovery file.
pub fn default_path() -> Result<PathBuf, DiscoveryError> {
    if let Some(profile) = kw_storage::paths::debug_profile() {
        return Ok(profile.join(FILE_NAME));
    }
    let home = dirs::home_dir().ok_or(DiscoveryError::NoHomeDirectory)?;
    Ok(home.join(DIRECTORY_NAME).join(FILE_NAME))
}

/// Write the discovery file, and report what permissions it ended up with.
pub fn write(path: &Path, port: u16, token: &LocalApiToken) -> Result<Permissions, DiscoveryError> {
    write_with(path, port, token, repair_path)
}

fn write_with(
    path: &Path,
    port: u16,
    token: &LocalApiToken,
    secure: impl Fn(&Path, PathKind) -> Result<Permissions, DiscoveryError>,
) -> Result<Permissions, DiscoveryError> {
    use std::io::Write;
    let parent = path.parent().ok_or(DiscoveryError::NoHomeDirectory)?;
    let io_error = |action, source| DiscoveryError::Io {
        action,
        path: path.to_owned(),
        source,
    };
    fs::create_dir_all(parent).map_err(|e| io_error("create directory", e))?;
    secure(parent, PathKind::Directory)?;
    let temporary = path.with_extension(format!("json.{}.new", uuid::Uuid::new_v4()));
    let result = (|| {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(|e| io_error("create temporary", e))?;
        secure(&temporary, PathKind::File)?;
        let body = serde_json::to_vec(&Discovery {
            version: DISCOVERY_VERSION,
            port,
            token: token.expose().to_owned(),
        })
        .map_err(|e| io_error("encode", io::Error::other(e)))?;
        file.write_all(&body).map_err(|e| io_error("write", e))?;
        file.sync_all().map_err(|e| io_error("sync", e))?;
        drop(file);
        fs::rename(&temporary, path).map_err(|e| io_error("publish", e))?;
        let verdict = inspect(path);
        if !verdict.is_confirmed() {
            let repaired = repair(path);
            tracing::warn!(
                ?repaired,
                "discovery permissions changed during publication; discarding token"
            );
            let _ = remove(path);
            return Err(DiscoveryError::Permissions { verdict });
        }
        Ok(verdict)
    })();
    if let Err(error) = fs::remove_file(&temporary) {
        if error.kind() != io::ErrorKind::NotFound {
            tracing::warn!(%error, "could not clean discovery temporary");
        }
    }
    result
}

/// Apply the intended restriction, verify it, and allow one repair attempt.
fn repair_path(path: &Path, kind: PathKind) -> Result<Permissions, DiscoveryError> {
    repair_with(|| {
        if let Err(error) = file_acl::apply(path, kind) {
            tracing::warn!(%error, "could not apply discovery permissions");
        }
        file_acl::verify(path, kind)
    })
}

fn repair_with(mut attempt: impl FnMut() -> Permissions) -> Result<Permissions, DiscoveryError> {
    let mut verdict = attempt();
    if !verdict.is_confirmed() {
        verdict = attempt();
    }
    if verdict.is_confirmed() {
        Ok(verdict)
    } else {
        Err(DiscoveryError::Permissions { verdict })
    }
}

/// Repair an exposed discovery pair once. The old token must already be revoked.
pub fn repair(path: &Path) -> Permissions {
    if let Some(parent) = path.parent() {
        let _ = file_acl::apply(parent, PathKind::Directory);
    }
    let _ = file_acl::apply(path, PathKind::File);
    inspect(path)
}

/// A token's lifetime ends permanently when its discovery permissions widen.
#[derive(Debug)]
pub struct AccessGuard {
    path: PathBuf,
    revoked: std::sync::atomic::AtomicBool,
    verdict: std::sync::Mutex<Permissions>,
    pub(crate) changed: tokio::sync::Notify,
}
impl AccessGuard {
    pub fn new(path: PathBuf, verdict: Permissions) -> Self {
        Self {
            path,
            revoked: std::sync::atomic::AtomicBool::new(!verdict.is_confirmed()),
            verdict: std::sync::Mutex::new(verdict),
            changed: tokio::sync::Notify::new(),
        }
    }
    pub fn is_active(&self) -> bool {
        !self.revoked.load(std::sync::atomic::Ordering::Acquire)
    }
    pub fn verdict(&self) -> Permissions {
        self.verdict
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
    pub async fn check(self: &std::sync::Arc<Self>) -> bool {
        if !self.is_active() {
            return false;
        }
        let path = self.path.clone();
        let verdict = tokio::task::spawn_blocking(move || inspect(&path))
            .await
            .unwrap_or_else(|e| Permissions::Unknown {
                reason: e.to_string(),
            });
        let mut held = self
            .verdict
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if self.is_active() {
            let confirmed = verdict.is_confirmed();
            *held = verdict;
            if !confirmed {
                self.revoked
                    .store(true, std::sync::atomic::Ordering::Release);
                self.changed.notify_one();
            }
        }
        self.is_active()
    }
    pub(crate) async fn repair_and_remove(&self) {
        let path = self.path.clone();
        let _ = tokio::task::spawn_blocking(move || {
            let verdict = repair(&path);
            tracing::warn!(
                ?verdict,
                "local API token revoked; retry explicitly after permission repair"
            );
            if let Err(error) = remove(&path) {
                tracing::warn!(%error, "could not remove revoked discovery file");
            }
        })
        .await;
    }
}

/// What the discovery file and the directory holding it grant, right now.
///
/// Reads; never writes. Two `stat` calls on Unix, two `GetNamedSecurityInfoW`
/// calls on Windows, and nothing is applied -- so this is safe to call on a
/// path this process did not create and safe to call repeatedly.
///
/// **The directory's verdict gates the file's**, through
/// [`file_acl::combine`]. A perfectly restricted file in a directory another
/// user may delete children of can be replaced, and a client then reads an
/// attacker's port and token rather than ours. For a file whose contents *are*
/// the access control, being replaceable is worse than being readable.
///
/// Extracted from the tail of [`write`], which now ends by calling it, so the
/// verdict a caller is handed after a write and the verdict it gets from a
/// later re-read are produced by the same expression rather than by two that
/// have to be kept in step. That mattered as soon as there was a second
/// caller: `LocalApiRuntime` used to store this value at start and show it
/// forever, so anything that widened the access list afterwards went
/// unreported until the next launch (KNEG-110).
///
/// Deliberately **not** a re-apply. Silently repairing an access list somebody
/// changed on purpose is its own kind of surprise, and it would make this
/// function unusable for the thing it exists for -- a status screen must be
/// able to describe a bad state without altering it.
#[must_use]
pub fn inspect(path: &Path) -> Permissions {
    let file = file_acl::verify(path, PathKind::File);
    match path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        Some(parent) => file_acl::combine(&file_acl::verify(parent, PathKind::Directory), &file),
        None => file,
    }
}

/// Read the discovery file, validating the token on the way in.
pub fn read(path: &Path) -> Result<(u16, LocalApiToken), DiscoveryError> {
    let body = fs::read_to_string(path).map_err(|source| DiscoveryError::Io {
        action: "read",
        path: path.to_path_buf(),
        source,
    })?;

    let discovery: Discovery =
        serde_json::from_str(&body).map_err(|source| DiscoveryError::Malformed {
            path: path.to_path_buf(),
            detail: source.to_string(),
        })?;

    if discovery.version != DISCOVERY_VERSION {
        return Err(DiscoveryError::Version {
            path: path.to_path_buf(),
            found: discovery.version,
        });
    }

    let token = LocalApiToken::parse(&discovery.token).map_err(|source| DiscoveryError::Token {
        path: path.to_path_buf(),
        source,
    })?;

    Ok((discovery.port, token))
}

/// Remove the discovery file. Absent is success.
///
/// Called on shutdown, so a client does not spend its next run trying a port
/// nothing is listening on with a token nothing accepts.
pub fn remove(path: &Path) -> Result<(), DiscoveryError> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(DiscoveryError::Io {
            action: "remove",
            path: path.to_path_buf(),
            source,
        }),
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

    fn token() -> LocalApiToken {
        LocalApiToken::generate().unwrap()
    }

    #[test]
    fn a_discovery_file_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(FILE_NAME);
        let written = token();

        write(&path, 8200, &written).unwrap();
        let (port, read_back) = read(&path).unwrap();

        assert_eq!(port, 8200);
        assert!(read_back.matches_header(&written.header_value()));
    }

    #[test]
    fn the_parent_directory_is_created() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(DIRECTORY_NAME).join(FILE_NAME);
        write(&path, 8200, &token()).unwrap();
        assert!(path.is_file());
    }

    #[test]
    fn permissions_are_reported_as_what_the_platform_actually_gave() {
        // Re-scoped when the Windows branch stopped being a no-op. It used to
        // assert the non-Unix answer was `InheritedFromProfile` -- the honest
        // report of a gap. There is now a mechanism on Windows, and this
        // asserts what it achieved, still per-platform and still read back.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(FILE_NAME);
        let permissions = write(&path, 8200, &token()).unwrap();

        assert!(
            permissions.is_confirmed(),
            "the platform's restriction was applied and read back: {permissions:?}"
        );

        if cfg!(unix) {
            assert_eq!(permissions, Permissions::OwnerOnly);
            assert!(permissions.explain().contains("verified"));
        } else if cfg!(windows) {
            assert_eq!(permissions, Permissions::OwnerAndAdministrators);
            // Still not described as owner-only: an administrator can read it.
            assert!(permissions.explain().contains("administrators"));
            assert!(!permissions.explain().contains("only by your user"));
        }
    }

    #[test]
    fn the_windows_explanation_does_not_claim_owner_only() {
        // The specific thing this must never do is overstate the protection.
        // Re-pointed at the state Windows now reaches: `InheritedFromProfile`
        // is no longer what a Windows machine gets, and its sentence is now
        // platform-neutral because the only target left that can produce it is
        // one that is neither Unix nor Windows.
        let explanation = Permissions::OwnerAndAdministrators.explain();
        assert!(!explanation.contains("only by your user"));
        assert!(explanation.contains("administrators"));

        let inherited = Permissions::InheritedFromProfile.explain();
        assert!(!inherited.contains("only by your user"));
        assert!(!Permissions::InheritedFromProfile.is_confirmed());
    }

    #[test]
    fn a_wider_mode_is_reported_and_says_what_it_means() {
        let permissions = Permissions::Wider { mode: 0o644 };
        assert!(!permissions.is_confirmed());
        assert!(permissions.explain().contains("644"));
        assert!(permissions.explain().contains("more widely"));
    }

    #[test]
    fn unknown_permissions_tell_the_user_to_assume_the_worst() {
        let permissions = Permissions::Unknown {
            reason: "operation not supported".to_owned(),
        };
        assert!(!permissions.is_confirmed());
        assert!(permissions.explain().contains("operation not supported"));
        assert!(permissions
            .explain()
            .contains("Assume anything running as you"));
    }

    #[test]
    fn the_directory_is_restricted_before_the_temporary_is_created() {
        // The ordering property. Written as an assertion about the directory
        // rather than about the write, because the window it closes -- the
        // token's bytes on disk under a loose access list -- cannot be observed
        // after the fact.
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join(DIRECTORY_NAME);
        let path = home.join(FILE_NAME);
        write(&path, 8200, &token()).unwrap();

        let directory = kw_security::file_acl::verify(&home, PathKind::Directory);
        assert!(
            directory.is_confirmed(),
            "the directory holding the token must be restricted too: {directory:?}"
        );
    }

    #[test]
    fn the_read_back_describes_the_file_the_caller_is_told_about() {
        // Not the temporary, which no longer exists by the time `write`
        // returns. Asserted by pointing `verify` at the final path
        // independently and requiring the same answer.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(FILE_NAME);
        let reported = write(&path, 8200, &token()).unwrap();

        let file = kw_security::file_acl::verify(&path, PathKind::File);
        let directory = kw_security::file_acl::verify(dir.path(), PathKind::Directory);
        assert_eq!(reported, kw_security::file_acl::combine(&directory, &file));

        // And that `write` produces it by calling `inspect`, rather than by a
        // second copy of the same expression that could drift from it.
        assert_eq!(reported, inspect(&path));
    }

    #[test]
    fn inspect_reads_and_never_writes() {
        // It is called from a status screen, on a path this process may not
        // have created. A function that repaired what it found would make a
        // bad state impossible to describe without changing it.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(FILE_NAME);

        let verdict = inspect(&path);

        assert!(!path.exists(), "inspect created a file");
        assert_eq!(
            std::fs::read_dir(dir.path()).unwrap().count(),
            0,
            "inspect wrote something into the directory"
        );
        assert!(
            !verdict.is_confirmed(),
            "a file that is not there is not a verified restriction: {}",
            verdict.explain()
        );
    }

    #[test]
    fn inspect_sees_a_change_write_could_not_have_known_about() {
        // The whole of KNEG-110: the verdict `write` returns describes the
        // moment it ran, and anything with write access to the directory can
        // change the answer afterwards.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(FILE_NAME);
        let at_write = write(&path, 8200, &token()).unwrap();

        std::fs::remove_file(&path).unwrap();

        assert_ne!(
            at_write,
            inspect(&path),
            "inspect returned the same verdict for a file that is no longer there"
        );
    }

    #[test]
    fn a_stale_file_is_removed_and_removing_a_missing_one_succeeds() {
        // A leftover file sends the next client at a port nothing is listening
        // on, with a token nothing accepts.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(FILE_NAME);
        write(&path, 8200, &token()).unwrap();

        remove(&path).unwrap();
        assert!(!path.exists());
        remove(&path).unwrap();
    }

    #[test]
    fn a_file_from_a_future_version_is_refused_rather_than_guessed_at() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(FILE_NAME);
        fs::write(&path, r#"{"version":99,"port":8200,"token":"aa"}"#).unwrap();

        assert!(matches!(
            read(&path).unwrap_err(),
            DiscoveryError::Version { found: 99, .. }
        ));
    }

    #[test]
    fn a_hand_edited_token_is_refused() {
        // A short token in a file is worse than no file: it looks usable and is
        // easier to guess.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(FILE_NAME);
        fs::write(&path, r#"{"version":1,"port":8200,"token":"abc123"}"#).unwrap();

        assert!(matches!(
            read(&path).unwrap_err(),
            DiscoveryError::Token { .. }
        ));
    }

    #[test]
    fn an_unknown_field_is_refused() {
        // So a future field cannot be silently ignored by an older build.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(FILE_NAME);
        fs::write(
            &path,
            r#"{"version":1,"port":8200,"token":"aa","bindAddress":"0.0.0.0"}"#,
        )
        .unwrap();

        assert!(matches!(
            read(&path).unwrap_err(),
            DiscoveryError::Malformed { .. }
        ));
    }

    #[test]
    fn a_missing_file_is_an_io_error_naming_the_path() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(FILE_NAME);
        let err = read(&path).unwrap_err();
        assert!(err.to_string().contains(FILE_NAME));
    }

    #[test]
    fn no_temporary_file_is_left_behind() {
        // The write goes through a temporary and a rename so a client never sees
        // half a token; the temporary must not survive.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(FILE_NAME);
        write(&path, 8200, &token()).unwrap();

        // The control. "Nothing else is in the directory" is also true of a
        // directory with nothing in it, so the discovery file being there is
        // what makes the rest of this an observation rather than a tautology.
        assert!(path.is_file(), "the discovery file itself is missing");

        let leftovers: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|name| name != FILE_NAME)
            .collect();
        assert!(leftovers.is_empty(), "left behind {leftovers:?}");
    }

    #[test]
    fn rewriting_replaces_the_previous_token() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(FILE_NAME);
        let first = token();
        let second = token();

        write(&path, 8200, &first).unwrap();
        write(&path, 8201, &second).unwrap();

        let (port, read_back) = read(&path).unwrap();
        assert_eq!(port, 8201);
        assert!(read_back.matches_header(&second.header_value()));
        assert!(
            !read_back.matches_header(&first.header_value()),
            "the old token must not still work"
        );
    }

    #[test]
    fn the_default_path_is_under_the_home_directory() {
        // Not asserting an absolute path: this runs on three platforms.
        let path = default_path().expect("a home directory");
        assert!(path.ends_with(Path::new(DIRECTORY_NAME).join(FILE_NAME)));
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod enforcement_tests {
    use super::*;
    #[test]
    fn unsecurable_empty_file_never_receives_token_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("local-api.json");
        let result = write_with(
            &path,
            8200,
            &LocalApiToken::generate().unwrap(),
            |path, kind| {
                if matches!(kind, PathKind::File) {
                    assert_eq!(fs::metadata(path).unwrap().len(), 0);
                    Err(DiscoveryError::Permissions {
                        verdict: Permissions::Unknown {
                            reason: "fixture read-back unavailable".to_owned(),
                        },
                    })
                } else {
                    repair_path(path, kind)
                }
            },
        );
        assert!(result.is_err());
        assert!(!path.exists());
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
    }
    #[test]
    fn repair_is_bounded_and_unknown_is_never_accepted() {
        let mut attempts = 0;
        assert!(repair_with(|| {
            attempts += 1;
            Permissions::ExtraTrustees { count: 1 }
        })
        .is_err());
        assert_eq!(attempts, 2);
        let mut attempts = 0;
        assert!(repair_with(|| {
            attempts += 1;
            if attempts == 1 {
                Permissions::DirectoryNotRestricted
            } else {
                Permissions::OwnerOnly
            }
        })
        .is_ok());
        assert_eq!(attempts, 2);
        assert!(repair_with(|| Permissions::InheritedFromProfile).is_err());
    }

    #[tokio::test]
    async fn missing_discovery_revokes_permanently_even_after_repair() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("local-api.json");
        let token = LocalApiToken::generate().unwrap();
        let guard = std::sync::Arc::new(AccessGuard::new(
            path.clone(),
            write(&path, 8200, &token).unwrap(),
        ));
        assert!(guard.check().await);
        remove(&path).unwrap();
        assert!(!guard.check().await);
        write(&path, 8200, &token).unwrap();
        assert!(!guard.check().await);
    }
    #[cfg(windows)]
    #[test]
    fn explicit_extra_trustee_is_removed_by_existing_acl_application() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("local-api.json");
        let token = LocalApiToken::generate().unwrap();
        write(&path, 8200, &token).unwrap();
        let status = std::process::Command::new("icacls")
            .arg(dir.path())
            .args(["/grant", "*S-1-1-0:(OI)(CI)(RX)"])
            .output()
            .unwrap();
        assert!(status.status.success());
        assert!(!inspect(&path).is_confirmed());
        assert!(repair(&path).is_confirmed());
    }
    #[test]
    fn failed_publication_removes_its_exclusively_created_temporary() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("local-api.json");
        fs::create_dir(&path).unwrap();
        assert!(write(&path, 8200, &LocalApiToken::generate().unwrap()).is_err());
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }
}
