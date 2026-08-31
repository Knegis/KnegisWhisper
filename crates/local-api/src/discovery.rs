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
use std::fs::File;
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
    let home = dirs::home_dir().ok_or(DiscoveryError::NoHomeDirectory)?;
    Ok(home.join(DIRECTORY_NAME).join(FILE_NAME))
}

/// Write the discovery file, and report what permissions it ended up with.
pub fn write(path: &Path, port: u16, token: &LocalApiToken) -> Result<Permissions, DiscoveryError> {
    let parent = path.parent();
    if let Some(parent) = parent {
        fs::create_dir_all(parent).map_err(|source| DiscoveryError::Io {
            action: "create",
            path: parent.to_path_buf(),
            source,
        })?;
        // Before the temporary exists, not after -- see this module's doc. A
        // failure here is logged rather than returned: what the caller is told
        // is the read-back at the end, and a directory that refused will show
        // up there as the file refusing too, or as `DirectoryNotRestricted`.
        // Returning an error instead would stop the local API starting over a
        // permission problem the caller is about to be told about anyway.
        if let Err(error) = file_acl::apply(parent, PathKind::Directory) {
            tracing::warn!(
                path = %parent.display(),
                %error,
                "could not restrict the directory holding the discovery file"
            );
        }
    }

    let body = serde_json::to_string_pretty(&Discovery {
        version: DISCOVERY_VERSION,
        port,
        token: token.expose().to_owned(),
    })
    .map_err(|source| DiscoveryError::Io {
        action: "encode",
        path: path.to_path_buf(),
        source: io::Error::other(source),
    })?;

    // Written whole and replaced, rather than truncated in place: a client
    // reading during a rewrite should see the old file or the new one, not half
    // a token.
    let temporary = path.with_extension("json.new");

    // Created empty and restricted before the token goes into it. `fs::write`
    // on its own would put the bytes on disk first; the window is short, but it
    // is real, and on Unix the directory's `0700` does not close it because a
    // process already inside the directory would still get a `0644` file.
    drop(
        File::create(&temporary).map_err(|source| DiscoveryError::Io {
            action: "create",
            path: temporary.clone(),
            source,
        })?,
    );
    if let Err(error) = file_acl::apply(&temporary, PathKind::File) {
        tracing::warn!(
            path = %temporary.display(),
            %error,
            "could not restrict the discovery file"
        );
    }

    fs::write(&temporary, &body).map_err(|source| DiscoveryError::Io {
        action: "write",
        path: temporary.clone(),
        source,
    })?;
    fs::rename(&temporary, path).map_err(|source| DiscoveryError::Io {
        action: "move into place",
        path: path.to_path_buf(),
        source,
    })?;

    // After the rename, against the path the caller is told about. The
    // directory's verdict gates the file's: a perfectly restricted file in a
    // directory another user may delete children of can be replaced, and a
    // client then reads an attacker's port and token rather than ours.
    let file = file_acl::verify(path, PathKind::File);
    Ok(match parent {
        Some(parent) => file_acl::combine(&file_acl::verify(parent, PathKind::Directory), &file),
        None => file,
    })
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
