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
//! On Windows there is no `chmod`. A file created under `%USERPROFILE%` inherits
//! that directory's ACL, which normally grants the user and the administrators
//! group and nobody else. That is weaker than `0600` — an administrator can read
//! it — and it is reported as what it is rather than described as owner-only.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use kw_security::LocalApiToken;
use serde::{Deserialize, Serialize};

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
    /// The bearer token. This is why the file is 0600.
    pub token: String,
}

/// What the filesystem actually granted.
///
/// Never a boolean: "restricted" and "we asked for it to be restricted" are
/// different claims, and only the first is worth telling a user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Permissions {
    /// Verified by reading the mode back: owner read/write only.
    OwnerOnly,
    /// Windows. Inherits the profile directory's ACL — normally the user and
    /// administrators.
    InheritedFromProfile,
    /// The mode was set and read back as something wider than expected.
    Wider { mode: u32 },
    /// The mode could not be set or could not be read back.
    Unknown { reason: String },
}

impl Permissions {
    /// A sentence for the settings screen.
    #[must_use]
    pub fn explain(&self) -> String {
        match self {
            Self::OwnerOnly => {
                "readable only by your user account (mode 0600, verified)".to_owned()
            }
            Self::InheritedFromProfile => "readable by your user account and by administrators, \
                 which is what your profile directory allows"
                .to_owned(),
            Self::Wider { mode } => format!(
                "readable more widely than intended (mode {mode:o}). Anything that can read it \
                 can use the local API."
            ),
            Self::Unknown { reason } => format!(
                "of unknown permissions: {reason}. Treat the local API as reachable by anything \
                 running as you."
            ),
        }
    }

    /// Whether this is as restrictive as the platform allows.
    #[must_use]
    pub const fn is_as_expected(&self) -> bool {
        matches!(self, Self::OwnerOnly | Self::InheritedFromProfile)
    }
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
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|source| DiscoveryError::Io {
            action: "create",
            path: parent.to_path_buf(),
            source,
        })?;
        restrict_directory(parent);
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
    fs::write(&temporary, &body).map_err(|source| DiscoveryError::Io {
        action: "write",
        path: temporary.clone(),
        source,
    })?;
    let permissions = restrict_file(&temporary);
    fs::rename(&temporary, path).map_err(|source| DiscoveryError::Io {
        action: "move into place",
        path: path.to_path_buf(),
        source,
    })?;

    Ok(permissions)
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

#[cfg(unix)]
fn restrict_file(path: &Path) -> Permissions {
    use std::os::unix::fs::PermissionsExt;

    if let Err(err) = fs::set_permissions(path, fs::Permissions::from_mode(0o600)) {
        return Permissions::Unknown {
            reason: err.to_string(),
        };
    }

    // Read back rather than trusting the call. Some filesystems ignore mode
    // bits entirely, and a token in a world-readable file is worth knowing about
    // before it is used.
    match fs::metadata(path) {
        Ok(metadata) => {
            let mode = metadata.permissions().mode() & 0o777;
            if mode == 0o600 {
                Permissions::OwnerOnly
            } else {
                Permissions::Wider { mode }
            }
        }
        Err(err) => Permissions::Unknown {
            reason: err.to_string(),
        },
    }
}

#[cfg(not(unix))]
fn restrict_file(path: &Path) -> Permissions {
    // No chmod. The file inherits the profile directory's ACL, which normally
    // grants this user and the administrators group. Reported as that rather
    // than as owner-only, because an administrator can read it.
    let _ = path;
    Permissions::InheritedFromProfile
}

fn restrict_directory(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o700));
    }
    #[cfg(not(unix))]
    {
        let _ = path;
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
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(FILE_NAME);
        let permissions = write(&path, 8200, &token()).unwrap();

        assert!(
            permissions.is_as_expected(),
            "on a normal filesystem this should be the platform's best: {permissions:?}"
        );

        if cfg!(unix) {
            assert_eq!(permissions, Permissions::OwnerOnly);
            assert!(permissions.explain().contains("verified"));
        } else {
            // Not described as owner-only: an administrator can read it.
            assert_eq!(permissions, Permissions::InheritedFromProfile);
            assert!(permissions.explain().contains("administrators"));
        }
    }

    #[test]
    fn the_windows_explanation_does_not_claim_owner_only() {
        // The specific thing this must never do is overstate the protection.
        let explanation = Permissions::InheritedFromProfile.explain();
        assert!(!explanation.contains("only by your user"));
        assert!(explanation.contains("administrators"));
    }

    #[test]
    fn a_wider_mode_is_reported_and_says_what_it_means() {
        let permissions = Permissions::Wider { mode: 0o644 };
        assert!(!permissions.is_as_expected());
        assert!(permissions.explain().contains("644"));
        assert!(permissions.explain().contains("can read it"));
    }

    #[test]
    fn unknown_permissions_tell_the_user_to_assume_the_worst() {
        let permissions = Permissions::Unknown {
            reason: "operation not supported".to_owned(),
        };
        assert!(!permissions.is_as_expected());
        assert!(permissions.explain().contains("reachable by anything"));
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
