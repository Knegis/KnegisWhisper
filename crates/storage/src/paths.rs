//! Where things go on disk.
//!
//! Storage owns this because it owns the database file, and everything else that
//! needs a directory (models, sidecar binaries, temporary audio) should agree
//! with it rather than compute its own.
//!
//! One non-obvious rule is load-bearing, and it is narrower than the version
//! this file carried until 2026-08-31: **the native ASR binaries cannot open a
//! file whose path contains a non-ASCII character.** A Windows account named
//! `Håkan` is enough to trigger it, and the failure surfaces as an unexplained
//! transcription error rather than as anything that points at the path. See
//! [`is_native_readable`] and [`model_cache_dir`].
//!
//! The rule ported from upstream said "non-ASCII **or space**", and applied it
//! to the binaries' own directory as well as to the files they read. Three
//! quarters of that was never true. Measured on 2026-08-31 with
//! `cargo run -p kw-storage --example native_path_probe`, against the real
//! `whisper-server` and `sherpa-onnx-offline` on this machine:
//!
//! ```text
//! model file under a path with a space      whisper: loads   sherpa: transcribes
//! model file under a non-ASCII path         whisper: dies    sherpa: "Errors in config!"
//! audio input under a path with a space                      sherpa: transcribes
//! audio input under a non-ASCII path                         sherpa: "Failed to read"
//! the binary's own directory, with a space  whisper: loads   sherpa: transcribes
//! the binary's own directory, non-ASCII     whisper: loads   sherpa: transcribes
//! ```
//!
//! So a space is fine everywhere, and the binaries' own location is fine
//! everywhere -- `whisper-server` loads its sibling `ggml-cpu-*.dll` out of a
//! directory called `Håkan Test` without complaint, and so do `llama-server`
//! and `ffmpeg`. Only a file the ASR binaries *open by path* is affected, which
//! is the model and the audio. Both failures exit 127.

use std::path::{Path, PathBuf};

/// The application name used for every on-disk location.
pub const APP_DIR: &str = "KnegisWhisper";

/// Per-user data directory: the database, settings, logs.
///
/// Falls back to `./` only if the platform has no home directory at all, which
/// in practice means a misconfigured container.
#[must_use]
pub fn data_dir() -> PathBuf {
    if let Some(profile) = debug_profile() {
        return profile;
    }
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(APP_DIR)
}

/// Isolated developer smoke profile. Release builds always use the OS profile.
#[must_use]
pub fn debug_profile() -> Option<PathBuf> {
    #[cfg(debug_assertions)]
    {
        std::env::var_os("KW_AUTOMATION_PROFILE")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
    }
    #[cfg(not(debug_assertions))]
    {
        None
    }
}

/// The database file.
#[must_use]
pub fn database_path() -> PathBuf {
    data_dir().join("knegiswhisper.db")
}

/// Whether the native ASR binaries can open files under `path`.
///
/// **ASCII only. A space is fine** -- see the module documentation for the
/// measurement, which is also why this is no longer called
/// `is_native_binary_safe`: it is not about where the binary lives, which turns
/// out not to matter at all, but about the files it opens.
///
/// A path that is not valid UTF-8 is rejected too. On Windows that means
/// unpaired surrogates, which cannot survive being handed to a process as an
/// argument, so the answer is the same one for a different reason.
#[must_use]
pub fn is_native_readable(path: &Path) -> bool {
    path.to_str().is_some_and(str::is_ascii)
}

/// Where downloaded models live.
///
/// The model file is one of the two things the rule actually applies to, so
/// this keeps the fallback. Prefers a subdirectory of [`data_dir`]; if that
/// path is not [`is_native_readable`], falls back to a machine-wide location:
/// `%ProgramData%\KnegisWhisper\cache` on Windows, `/Library/Caches/KnegisWhisper`
/// on macOS, `/var/tmp/KnegisWhisper` elsewhere.
///
/// Returns the fallback even if it too is unsafe — the caller is told which one
/// it got via [`ModelCache::fell_back`] so the UI can explain a failure rather
/// than pretending it did not happen.
#[must_use]
pub fn model_cache_dir() -> ModelCache {
    native_safe_dir("models")
}

/// Where downloaded sidecar binaries live.
///
/// **No fallback, because the rule does not reach here.** This used to relocate
/// on the same test as [`model_cache_dir`], on the stated grounds that
/// `whisper-server` loads its sibling `ggml` libraries relative to its own
/// directory and would choke. Measured on 2026-08-31: it does not.
/// `whisper-server`, `sherpa-onnx-offline`, `llama-server` and `ffmpeg` all
/// start, load their DLLs and do their work from a directory named
/// `Håkan Test`. Four out of four, so this is a measurement rather than an
/// inference from two.
///
/// Dropping it costs a non-ASCII account nothing and saves it something: the
/// machine-wide fallback is under `%ProgramData%`, which an unprivileged
/// account may not be able to write to at all.
///
/// Still returns a [`ModelCache`] rather than a bare path, so that
/// `install_dir_fell_back` in `local_model` keeps one shape to read. `fell_back`
/// is now always false here, and that is the fact, not an omission.
#[must_use]
pub fn sidecar_dir() -> ModelCache {
    ModelCache {
        path: data_dir().join("sidecars"),
        fell_back: false,
    }
}

/// A subdirectory of [`data_dir`] the native binaries can read from, or a
/// machine-wide fallback that is more likely to be one.
fn native_safe_dir(leaf: &str) -> ModelCache {
    let preferred = data_dir().join(leaf);
    if is_native_readable(&preferred) {
        return ModelCache {
            path: preferred,
            fell_back: false,
        };
    }

    ModelCache {
        path: machine_wide_cache_dir().join(leaf),
        fell_back: true,
    }
}

/// The outcome of [`model_cache_dir`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelCache {
    pub path: PathBuf,
    /// True if the per-user location was rejected as unreadable by the native
    /// binaries. Worth surfacing: it means models are shared machine-wide.
    ///
    /// Always false from [`sidecar_dir`], which no longer relocates.
    pub fell_back: bool,
}

fn machine_wide_cache_dir() -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        let program_data = std::env::var_os("ProgramData")
            .unwrap_or_else(|| std::ffi::OsString::from("C:\\ProgramData"));
        PathBuf::from(program_data).join(APP_DIR).join("cache")
    }

    #[cfg(target_os = "macos")]
    {
        PathBuf::from("/Library/Caches").join(APP_DIR)
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        PathBuf::from("/var/tmp").join(APP_DIR)
    }
}

/// Scratch directory for in-flight audio.
///
/// Creates or verifies a private directory, separated by user and profile.
/// An ASCII user data directory is preferred; otherwise a private child of
/// ProgramData or /var/tmp is used. The legacy shared audio directory is never
/// reused. Refuses insecure preexisting paths before returning a writable path.
///
/// The other of the two things the rule applies to, and measured rather than
/// assumed: `sherpa-onnx-offline` takes the wav as an argument and answers
/// `Failed to read '...'` when its path holds a non-ASCII character, while the
/// same file under a path with a space in it transcribes.
pub fn audio_temp_dir() -> std::io::Result<PathBuf> {
    // Keeping the guard for the process lifetime also pins Windows ancestors
    // while ffmpeg, sherpa and cancellation cleanup use ordinary paths.
    static ROOT: std::sync::OnceLock<
        Result<kw_security::private_directory::PrivateDirectory, String>,
    > = std::sync::OnceLock::new();
    let result = ROOT.get_or_init(|| {
        let profile = data_dir();
        let parent = if is_native_readable(&profile) {
            profile.clone()
        } else {
            #[cfg(windows)]
            {
                PathBuf::from(
                    std::env::var_os("ProgramData").unwrap_or_else(|| "C:\\ProgramData".into()),
                )
            }
            #[cfg(unix)]
            {
                PathBuf::from("/var/tmp")
            }
            #[cfg(not(any(windows, unix)))]
            {
                return Err("private audio scratch is unsupported on this platform".into());
            }
        };
        if !is_native_readable(&parent) {
            return Err("the private audio scratch location must have an ASCII path".into());
        }
        let name = if parent == profile {
            // The containing profile already separates users and profiles.
            // Keep the native tool's file arguments short for ordinary paths.
            "audio-private-v1".to_owned()
        } else {
            kw_security::private_directory::scratch_name(&profile)?
        };
        kw_security::private_directory::PrivateDirectory::create(&parent, &name)
    });
    result
        .as_ref()
        .map(|root| root.path().to_owned())
        .map_err(|e| std::io::Error::other(e.clone()))
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

    #[test]
    fn ordinary_ascii_paths_are_readable() {
        assert!(is_native_readable(Path::new(
            "C:\\Users\\axel\\AppData\\Roaming\\KnegisWhisper"
        )));
        assert!(is_native_readable(Path::new(
            "/home/axel/.local/share/KnegisWhisper"
        )));
    }

    /// The half of the old rule that was never true.
    ///
    /// `Ann Marie` is an ordinary Windows account name, and until 2026-08-31
    /// this file sent her models to a machine-wide directory to avoid a failure
    /// that does not happen. Measured: a model under a path with a space in it
    /// loads in `whisper-server` and transcribes in `sherpa-onnx-offline`.
    #[test]
    fn a_space_is_readable() {
        assert!(is_native_readable(Path::new(
            "C:\\Users\\Ann Marie\\AppData"
        )));
        assert!(is_native_readable(Path::new("/home/ann marie/models")));
    }

    /// The half that is true, and is the whole of the rule.
    #[test]
    fn non_ascii_is_not_readable() {
        assert!(!is_native_readable(Path::new("C:\\Users\\Håkan\\AppData")));
        assert!(!is_native_readable(Path::new("/home/José/models")));
        // A space next to it changes nothing either way.
        assert!(!is_native_readable(Path::new("/home/José Maria/models")));
    }

    #[test]
    fn model_cache_reports_whether_it_fell_back() {
        let cache = model_cache_dir();
        // Whatever this machine's home directory looks like, the two must agree.
        assert_eq!(
            cache.fell_back,
            !is_native_readable(&data_dir().join("models"))
        );
        assert!(cache.path.is_absolute() || cache.path.starts_with("."));
    }

    /// Sidecars stay under the per-user directory whatever it is called.
    ///
    /// The measurement this rests on is in the module documentation: all four
    /// sidecar binaries run from a directory named `Håkan Test`.
    #[test]
    fn sidecars_never_relocate() {
        let dir = sidecar_dir();
        assert!(!dir.fell_back);
        assert_eq!(dir.path, data_dir().join("sidecars"));
    }

    #[test]
    fn database_lives_under_the_data_dir() {
        assert!(database_path().starts_with(data_dir()));
        assert_eq!(
            database_path().file_name().and_then(|s| s.to_str()),
            Some("knegiswhisper.db")
        );
    }
}
