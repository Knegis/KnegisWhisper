//! Audio staged on disk for a native tool that reads a file, not stdin.
//!
//! Shared by `sherpa-onnx-offline` ([`crate::sherpa`]) and
//! `whisper-vad-speech-segments` ([`crate::speech`]): both take a path, so
//! every call writes the recording out first and must not leave it behind.

use std::path::{Path, PathBuf};

/// A unique path for one call's input WAV, inside `dir`.
///
/// Named uniquely rather than reused so two overlapping calls (a retry racing
/// a cancellation, say) cannot clobber each other's input.
pub(crate) fn unique_wav_path(dir: &Path, prefix: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    dir.join(format!("{prefix}-{}-{nanos}.wav", std::process::id()))
}

/// Removes the staged file when dropped, which covers a call cancelled while
/// it waits for the child as well as one that finishes.
pub(crate) struct ScratchWav(pub(crate) PathBuf);

impl Drop for ScratchWav {
    fn drop(&mut self) {
        let path = self.0.clone();
        match std::fs::remove_file(&path) {
            Ok(()) => return,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return,
            Err(_) => {}
        }
        // A cancelled Windows child may still be releasing its file handle.
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            runtime.spawn(async move {
                for _ in 0..5 {
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                    match tokio::fs::remove_file(&path).await {
                        Ok(()) => return,
                        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return,
                        Err(_) => {}
                    }
                }
                tracing::warn!("could not remove scratch audio after process cleanup");
            });
        } else {
            tracing::warn!("could not remove scratch audio without an active runtime");
        }
    }
}
