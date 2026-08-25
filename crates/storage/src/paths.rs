//! Where things go on disk.
//!
//! Storage owns this because it owns the database file, and everything else that
//! needs a directory (models, sidecar binaries, temporary audio) should agree
//! with it rather than compute its own.
//!
//! One non-obvious rule is ported from upstream and is load-bearing: the model
//! cache must not sit under a path containing non-ASCII characters or spaces,
//! because the native whisper.cpp and sherpa-onnx binaries fail on such paths.
//! A Windows account named `Ann Marie` or `Håkan` is enough to trigger it, and
//! the failure surfaces as an unexplained transcription error rather than
//! anything that points at the path. See [`model_cache_dir`].

use std::path::{Path, PathBuf};

/// The application name used for every on-disk location.
pub const APP_DIR: &str = "KnegisWhisper";

/// Per-user data directory: the database, settings, logs.
///
/// Falls back to `./` only if the platform has no home directory at all, which
/// in practice means a misconfigured container.
#[must_use]
pub fn data_dir() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(APP_DIR)
}

/// The database file.
#[must_use]
pub fn database_path() -> PathBuf {
    data_dir().join("knegiswhisper.db")
}

/// Whether `path` is safe to hand to the native ASR binaries.
///
/// Rejects any non-ASCII byte and any space. Deliberately strict: the failure it
/// prevents is silent and hard to diagnose, and the cost of being wrong in this
/// direction is only a slightly less convenient cache location.
#[must_use]
pub fn is_native_binary_safe(path: &Path) -> bool {
    path.to_str()
        .is_some_and(|s| s.is_ascii() && !s.contains(' '))
}

/// Where downloaded models live.
///
/// Prefers a subdirectory of [`data_dir`]. If that path is not
/// [`is_native_binary_safe`], falls back to a machine-wide location that is:
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
/// The same rule applies for the same reason: `whisper-server` loads its sibling
/// `ggml` libraries relative to its own directory, and the native code chokes on
/// a path with a space or a non-ASCII character in it. A Windows account named
/// `Ann Marie` or `Håkan` is enough to trigger it.
#[must_use]
pub fn sidecar_dir() -> ModelCache {
    native_safe_dir("sidecars")
}

/// A subdirectory of [`data_dir`] that the native binaries can cope with, or a
/// machine-wide fallback that is more likely to be safe.
fn native_safe_dir(leaf: &str) -> ModelCache {
    let preferred = data_dir().join(leaf);
    if is_native_binary_safe(&preferred) {
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
    /// True if the per-user location was rejected as unsafe for the native
    /// binaries. Worth surfacing: it means models are shared machine-wide.
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

/// Scratch directory for in-flight audio. Subject to the same ASCII rule,
/// because the same binaries read from it.
#[must_use]
pub fn audio_temp_dir() -> PathBuf {
    let preferred = data_dir().join("audio");
    if is_native_binary_safe(&preferred) {
        preferred
    } else {
        machine_wide_cache_dir().join("audio")
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

    #[test]
    fn ordinary_ascii_paths_are_safe() {
        assert!(is_native_binary_safe(Path::new(
            "C:\\Users\\axel\\AppData\\Roaming\\KnegisWhisper"
        )));
        assert!(is_native_binary_safe(Path::new(
            "/home/axel/.local/share/KnegisWhisper"
        )));
    }

    #[test]
    fn spaces_and_non_ascii_are_not_safe() {
        // Both of these are ordinary Windows account names, and both break the
        // native ASR binaries.
        assert!(!is_native_binary_safe(Path::new(
            "C:\\Users\\Ann Marie\\AppData"
        )));
        assert!(!is_native_binary_safe(Path::new(
            "C:\\Users\\Håkan\\AppData"
        )));
        assert!(!is_native_binary_safe(Path::new("/home/José/models")));
    }

    #[test]
    fn model_cache_reports_whether_it_fell_back() {
        let cache = model_cache_dir();
        // Whatever this machine's home directory looks like, the two must agree.
        assert_eq!(
            cache.fell_back,
            !is_native_binary_safe(&data_dir().join("models"))
        );
        assert!(cache.path.is_absolute() || cache.path.starts_with("."));
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
