//! Finding a companion command-line binary, honestly.
//!
//! `knegiswhisper` (the CLI) and `knegiswhisper-mcp` (the stdio MCP server) are
//! separate binaries built from the same Cargo workspace as this application.
//! Neither is bundled into the desktop application's installer, and neither
//! is installed anywhere by this application, so nothing here may claim one is
//! "installed" — only report where, if anywhere, this actually looked and
//! found a file with the expected name.
//!
//! Two places are checked, in order: next to the currently running
//! executable — the directory `cargo build --workspace` (or an equivalent
//! packaging step) puts every workspace binary in, this one included — and
//! then every directory named in `PATH`. Both are real filesystem checks;
//! neither is inferred from the other, and finding nothing in either is
//! reported as [`BinaryLocation::NotFound`] rather than defaulted to a
//! reassuring guess.

use std::path::Path;

use serde::Serialize;
use ts_rs::TS;

/// Where a companion binary was found, if this checked and found it anywhere.
///
/// Three states rather than a boolean. "Next to this executable" and
/// "somewhere on PATH" are different facts — the first means the same build
/// that produced this application also produced the tool; the second means
/// something else put a same-named file somewhere the shell would run it —
/// and collapsing either into "not found" would be exactly the
/// boolean-collapse this project's `Permissions` pattern exists to avoid.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(
    tag = "state",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum BinaryLocation {
    /// Found next to the running desktop binary — the directory
    /// `cargo build --workspace` (or an equivalent packaging step) places
    /// every workspace binary in.
    Sibling { path: String },
    /// Found on `PATH`, at this specific entry.
    OnPath { path: String },
    /// Checked both and found neither. Not "not installed" — only "not found
    /// where this looked".
    NotFound,
}

impl BinaryLocation {
    /// The path this found, if any.
    #[must_use]
    pub fn path(&self) -> Option<&str> {
        match self {
            Self::Sibling { path } | Self::OnPath { path } => Some(path),
            Self::NotFound => None,
        }
    }
}

/// Check the sibling directory, then every `PATH` entry, for a file named
/// `name`.
///
/// Pure and injectable — `sibling_dir` and `path_var` are parameters rather
/// than read from the environment here — so a test can assert the search
/// order and the fallback without touching the real filesystem or the real
/// `PATH`, and so this never has to be called through a value that would drag
/// in a Tauri type.
#[must_use]
pub fn locate(name: &str, sibling_dir: Option<&Path>, path_var: Option<&str>) -> BinaryLocation {
    if let Some(dir) = sibling_dir {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return BinaryLocation::Sibling {
                path: candidate.display().to_string(),
            };
        }
    }

    for dir in path_var.into_iter().flat_map(std::env::split_paths) {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return BinaryLocation::OnPath {
                path: candidate.display().to_string(),
            };
        }
    }

    BinaryLocation::NotFound
}

/// [`locate`] against the real running process: the directory holding this
/// executable, and the real `PATH`.
#[must_use]
pub fn locate_default(name: &str) -> BinaryLocation {
    let sibling_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf));
    let path_var = std::env::var("PATH").ok();
    locate(name, sibling_dir.as_deref(), path_var.as_deref())
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn touch(dir: &Path, name: &str) -> PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, b"").unwrap();
        path
    }

    #[test]
    fn the_sibling_directory_wins_over_path() {
        let sibling = tempfile::tempdir().unwrap();
        let on_path = tempfile::tempdir().unwrap();
        touch(sibling.path(), "tool");
        touch(on_path.path(), "tool");

        let path_var = on_path.path().display().to_string();
        let found = locate("tool", Some(sibling.path()), Some(&path_var));

        assert_eq!(
            found,
            BinaryLocation::Sibling {
                path: sibling.path().join("tool").display().to_string()
            }
        );
    }

    #[test]
    fn path_is_checked_when_the_sibling_directory_does_not_have_it() {
        let sibling = tempfile::tempdir().unwrap();
        let elsewhere = tempfile::tempdir().unwrap();
        let on_path = tempfile::tempdir().unwrap();
        touch(on_path.path(), "tool");

        // Two PATH entries, so the search has to keep going past the first.
        let path_var = std::env::join_paths([elsewhere.path(), on_path.path()])
            .unwrap()
            .into_string()
            .unwrap();
        let found = locate("tool", Some(sibling.path()), Some(&path_var));

        assert_eq!(
            found,
            BinaryLocation::OnPath {
                path: on_path.path().join("tool").display().to_string()
            }
        );
    }

    #[test]
    fn neither_location_reports_not_found_rather_than_a_guess() {
        let sibling = tempfile::tempdir().unwrap();
        let on_path = tempfile::tempdir().unwrap();

        let path_var = on_path.path().display().to_string();
        let found = locate("tool", Some(sibling.path()), Some(&path_var));

        assert_eq!(found, BinaryLocation::NotFound);
        assert_eq!(found.path(), None);
    }

    #[test]
    fn no_sibling_directory_and_no_path_is_also_not_found_not_a_panic() {
        assert_eq!(locate("tool", None, None), BinaryLocation::NotFound);
    }

    #[test]
    fn path_returns_the_found_location() {
        let sibling = tempfile::tempdir().unwrap();
        touch(sibling.path(), "tool");
        let found = locate("tool", Some(sibling.path()), None);
        assert_eq!(
            found.path(),
            Some(sibling.path().join("tool").display().to_string().as_str())
        );
    }
}
