//! Finding an installed sidecar on disk.
//!
//! There are two places a sidecar can legitimately be: bundled with the
//! application by the packaging step, or downloaded into the application data
//! directory at the user's request. Both are searched, in that order, so a
//! packaged binary is preferred over a downloaded one — it went through the
//! same release review as the app.
//!
//! Upstream's equivalent returns `null` when nothing is found, which turns a
//! missing sidecar into "local transcription silently unavailable". This one
//! reports every root it looked in, and distinguishes *absent* from
//! *incomplete*: a directory holding a partial extraction is a different
//! problem with a different fix, and answering "not installed" to it sends the
//! user to re-download something that is already there.

use std::path::{Path, PathBuf};

use crate::manifest::{Asset, Target};

/// Where a sidecar might be, in search order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchPath {
    /// Shipped with the application. Preferred: it went through release review.
    pub bundled: Option<PathBuf>,
    /// Downloaded at the user's request.
    pub downloaded: PathBuf,
}

impl SearchPath {
    /// Roots in search order.
    fn roots(&self) -> Vec<(&'static str, &Path)> {
        let mut roots: Vec<(&'static str, &Path)> = Vec::with_capacity(2);
        if let Some(bundled) = &self.bundled {
            roots.push(("bundled with the application", bundled.as_path()));
        }
        roots.push(("downloaded", self.downloaded.as_path()));
        roots
    }
}

/// Where a sidecar was found, or why it was not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Installation {
    Installed {
        executable: PathBuf,
        /// Which root it came from, for diagnostics.
        origin: &'static str,
    },
    /// No directory for this sidecar exists in any root.
    NotInstalled {
        /// Every root that was searched, so the report is actionable.
        looked_in: Vec<PathBuf>,
    },
    /// A directory exists but the executable is not in it. A partial download,
    /// an interrupted extraction, or a manual deletion.
    Incomplete { dir: PathBuf, expected: PathBuf },
}

impl Installation {
    /// The executable, if there is one.
    #[must_use]
    pub fn executable(&self) -> Option<&Path> {
        match self {
            Self::Installed { executable, .. } => Some(executable.as_path()),
            _ => None,
        }
    }

    #[must_use]
    pub const fn is_installed(&self) -> bool {
        matches!(self, Self::Installed { .. })
    }

    /// A sentence for the user, naming the next step.
    #[must_use]
    pub fn explain(&self, sidecar: &str) -> String {
        match self {
            Self::Installed { executable, origin } => {
                format!(
                    "{sidecar} is installed at {} ({origin}).",
                    executable.display()
                )
            }
            Self::NotInstalled { looked_in } => {
                let roots = looked_in
                    .iter()
                    .map(|p| p.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("{sidecar} is not installed. Looked in: {roots}.")
            }
            Self::Incomplete { dir, expected } => format!(
                "{sidecar} is partly installed: {} exists but {} is missing. Reinstalling \
                 will replace it.",
                dir.display(),
                expected.display()
            ),
        }
    }
}

/// Anything that can answer whether a path is a readable file or a directory.
///
/// A trait purely so the search order can be tested without laying down real
/// files for every case, including the ones that are awkward to create (a
/// directory where a file is expected).
pub trait FileSystem {
    fn is_file(&self, path: &Path) -> bool;
    fn is_dir(&self, path: &Path) -> bool;
}

/// The real filesystem.
#[derive(Debug, Clone, Copy, Default)]
pub struct RealFs;

impl FileSystem for RealFs {
    fn is_file(&self, path: &Path) -> bool {
        path.is_file()
    }

    fn is_dir(&self, path: &Path) -> bool {
        path.is_dir()
    }
}

/// The name the executable has on disk after install.
///
/// Taken from the asset rather than recomputed, so the install step and the
/// lookup step cannot disagree about it.
#[must_use]
pub fn executable_name(asset: &Asset) -> &str {
    asset.executable_name()
}

/// Look for a sidecar.
pub fn locate<F: FileSystem>(
    sidecar: &str,
    asset: &Asset,
    search: &SearchPath,
    fs: &F,
) -> Installation {
    let exe = asset.executable_name();
    let mut incomplete: Option<Installation> = None;

    for (origin, root) in search.roots() {
        let dir = root.join(sidecar);
        let candidate = dir.join(exe);

        if fs.is_file(&candidate) {
            return Installation::Installed {
                executable: candidate,
                origin,
            };
        }

        // Remember the first directory that exists without its executable, but
        // keep searching: a complete install in a later root beats a broken one
        // in an earlier root.
        if incomplete.is_none() && fs.is_dir(&dir) {
            incomplete = Some(Installation::Incomplete {
                dir,
                expected: candidate,
            });
        }
    }

    incomplete.unwrap_or_else(|| Installation::NotInstalled {
        looked_in: search
            .roots()
            .into_iter()
            .map(|(_, root)| root.join(sidecar))
            .collect(),
    })
}

/// Whether an executable name is right for a target.
///
/// Used by the install step to refuse an asset whose executable would not be
/// runnable on the platform it is being installed for.
#[must_use]
pub fn suits_target(asset: &Asset, target: Target) -> bool {
    target.is_windows() == asset.executable_name().ends_with(".exe")
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;
    use crate::manifest::{ArchiveKind, Sha256Digest};

    const DIGEST: &str = "c2a4b60edb11f7e11a9191ffb50929535527d4d91c9903dbe3e554583bbbc63d";

    fn asset(exe: &str) -> Asset {
        Asset {
            url: "https://example.invalid/x.zip".to_owned(),
            archive: ArchiveKind::Zip,
            sha256: Sha256Digest::parse(DIGEST).unwrap(),
            size_bytes: 10,
            executable_in_archive: exe.to_owned(),
        }
    }

    #[derive(Default)]
    struct Fake {
        files: BTreeSet<PathBuf>,
        dirs: BTreeSet<PathBuf>,
    }

    impl Fake {
        fn with_file(mut self, path: &str) -> Self {
            let path = PathBuf::from(path);
            if let Some(parent) = path.parent() {
                self.dirs.insert(parent.to_path_buf());
            }
            self.files.insert(path);
            self
        }

        fn with_dir(mut self, path: &str) -> Self {
            self.dirs.insert(PathBuf::from(path));
            self
        }
    }

    impl FileSystem for Fake {
        fn is_file(&self, path: &Path) -> bool {
            self.files.contains(path)
        }

        fn is_dir(&self, path: &Path) -> bool {
            self.dirs.contains(path)
        }
    }

    fn search() -> SearchPath {
        SearchPath {
            bundled: Some(PathBuf::from("/app/resources/sidecars")),
            downloaded: PathBuf::from("/data/sidecars"),
        }
    }

    #[test]
    fn a_downloaded_sidecar_is_found() {
        let fs = Fake::default().with_file("/data/sidecars/whisper/whisper-server");
        let found = locate("whisper", &asset("dir/whisper-server"), &search(), &fs);
        assert_eq!(
            found,
            Installation::Installed {
                executable: PathBuf::from("/data/sidecars/whisper/whisper-server"),
                origin: "downloaded",
            }
        );
    }

    #[test]
    fn a_bundled_sidecar_wins_over_a_downloaded_one() {
        // The bundled copy went through the same release review as the app.
        let fs = Fake::default()
            .with_file("/app/resources/sidecars/whisper/whisper-server")
            .with_file("/data/sidecars/whisper/whisper-server");
        let found = locate("whisper", &asset("dir/whisper-server"), &search(), &fs);
        assert_eq!(
            found.executable().unwrap(),
            Path::new("/app/resources/sidecars/whisper/whisper-server")
        );
    }

    #[test]
    fn a_complete_download_beats_a_broken_bundle() {
        // A bundled directory missing its executable must not shadow a working
        // download; answering "incomplete" there would block a usable install.
        let fs = Fake::default()
            .with_dir("/app/resources/sidecars/whisper")
            .with_file("/data/sidecars/whisper/whisper-server");
        let found = locate("whisper", &asset("dir/whisper-server"), &search(), &fs);
        assert!(found.is_installed());
        assert_eq!(
            found.executable().unwrap(),
            Path::new("/data/sidecars/whisper/whisper-server")
        );
    }

    #[test]
    fn nothing_anywhere_reports_every_root_it_looked_in() {
        // Upstream returns null here, which becomes "silently unavailable".
        let fs = Fake::default();
        let found = locate("whisper", &asset("dir/whisper-server"), &search(), &fs);
        match &found {
            Installation::NotInstalled { looked_in } => {
                assert_eq!(looked_in.len(), 2);
                assert!(looked_in
                    .iter()
                    .any(|p| p.ends_with("resources/sidecars/whisper")));
                assert!(looked_in.iter().any(|p| p.ends_with("sidecars/whisper")));
            }
            other => panic!("{other:?}"),
        }
        assert!(found.explain("whisper").contains("Looked in"));
    }

    #[test]
    fn a_directory_without_its_executable_is_incomplete_not_absent() {
        // Different problem, different fix. "Not installed" would send the user
        // to download something that is already on disk.
        let fs = Fake::default().with_dir("/data/sidecars/whisper");
        let found = locate("whisper", &asset("dir/whisper-server"), &search(), &fs);
        match &found {
            Installation::Incomplete { dir, expected } => {
                assert_eq!(dir, Path::new("/data/sidecars/whisper"));
                assert!(expected.ends_with("whisper-server"));
            }
            other => panic!("{other:?}"),
        }
        assert!(found.explain("whisper").contains("partly installed"));
    }

    #[test]
    fn a_build_with_no_bundled_root_only_searches_the_download_root() {
        let search = SearchPath {
            bundled: None,
            downloaded: PathBuf::from("/data/sidecars"),
        };
        let fs = Fake::default();
        match locate("whisper", &asset("dir/whisper-server"), &search, &fs) {
            Installation::NotInstalled { looked_in } => assert_eq!(looked_in.len(), 1),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn the_executable_name_comes_from_the_archive_path() {
        // Install and lookup must not disagree about the name on disk.
        let windows = asset("Release/whisper-server.exe");
        assert_eq!(executable_name(&windows), "whisper-server.exe");

        let fs = Fake::default().with_file("/data/sidecars/whisper/whisper-server.exe");
        assert!(locate("whisper", &windows, &search(), &fs).is_installed());
    }

    #[test]
    fn an_asset_is_checked_against_the_targets_convention() {
        assert!(suits_target(
            &asset("Release/whisper-server.exe"),
            Target::WindowsX8664
        ));
        assert!(suits_target(
            &asset("dir/whisper-server"),
            Target::LinuxX8664
        ));

        // A unix binary offered for Windows, and the reverse.
        assert!(!suits_target(
            &asset("dir/whisper-server"),
            Target::WindowsX8664
        ));
        assert!(!suits_target(
            &asset("Release/whisper-server.exe"),
            Target::MacosAarch64
        ));
    }

    #[test]
    fn the_real_filesystem_agrees_with_the_fake_on_a_real_install() {
        // Guards the Fake against drifting from RealFs semantics.
        let dir = tempfile::tempdir().unwrap();
        let install = dir.path().join("whisper");
        std::fs::create_dir_all(&install).unwrap();

        let search = SearchPath {
            bundled: None,
            downloaded: dir.path().to_path_buf(),
        };
        let asset = asset("dir/whisper-server");

        // Directory but no executable.
        assert!(matches!(
            locate("whisper", &asset, &search, &RealFs),
            Installation::Incomplete { .. }
        ));

        std::fs::write(install.join("whisper-server"), b"#!/bin/sh\n").unwrap();
        assert!(locate("whisper", &asset, &search, &RealFs).is_installed());
    }
}
