//! Installing a local whisper model.
//!
//! A model is *data*, not an executable, and it is verified differently for that
//! reason. `kw_sidecar` refuses to describe a binary without a pinned SHA-256,
//! because running the wrong bytes compromises the machine. Running the wrong
//! *weights* produces bad transcripts — a real problem, but not the same
//! problem, and the honest check is proportionate to it.
//!
//! So a model carries [`Integrity::Pinned`] where a digest is known and
//! [`Integrity::Size`] otherwise, and which one applied is reported rather than
//! implied. Upstream checks size alone for all six. Pinning all of them means
//! downloading roughly seven gigabytes once, which is a job for a tool rather
//! than something to invent digests for here: `tiny` is pinned because it was
//! actually downloaded and hashed while this was written, and the rest say
//! plainly that they were not.
//!
//! Size is checked with a tolerance because the registry's `expectedSizeBytes`
//! are rounded — `ggml-tiny.bin` is declared as 78,000,000 and is really
//! 77,691,713. A tolerance rather than a rewrite of the registry, because the
//! rounded figures are also what the settings screen shows.

use std::path::{Path, PathBuf};

use kw_network_policy::{NetworkPolicy, RequestPurpose};
use kw_provider_registry::WhisperModel;
use kw_sidecar::manifest::Sha256Digest;

/// How much a model file may differ from its declared size.
///
/// Generous, because the declared figures are rounded for display. It is still
/// enough to catch the failures that matter: a truncated transfer, an error page
/// saved as a model, a rate-limit response.
pub const SIZE_TOLERANCE: f64 = 0.05;

/// Digests for the models that have actually been downloaded and hashed.
///
/// Anything absent from this table is size-checked only, and
/// [`Verification::SizeOnly`] says so to the caller instead of letting it assume
/// a digest was involved.
const PINNED: [(&str, &str, u64); 1] = [(
    "tiny",
    "be07e048e1e599ad46341c8d2a135645097a538221678b7acdd1b1919c6e1b21",
    77_691_713,
)];

/// What is known about a model's bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Integrity {
    /// A digest measured from the real file.
    Pinned {
        digest: Sha256Digest,
        size_bytes: u64,
    },
    /// Only a declared size, rounded.
    Size { expected_bytes: u64 },
    /// The registry gave neither.
    Unknown,
}

/// What integrity check a model can be held to.
#[must_use]
pub fn integrity_for(model: &WhisperModel) -> Integrity {
    if let Some((_, digest, size)) = PINNED.iter().find(|(id, _, _)| *id == model.id) {
        if let Ok(digest) = Sha256Digest::parse(digest) {
            return Integrity::Pinned {
                digest,
                size_bytes: *size,
            };
        }
    }

    match model.expected_size_bytes {
        Some(expected_bytes) => Integrity::Size { expected_bytes },
        None => Integrity::Unknown,
    }
}

/// How a model was checked once it was on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verification {
    /// The digest matched.
    DigestMatched,
    /// Only the size was checked, because no digest is known for this model.
    SizeOnly { bytes: u64 },
    /// Nothing could be checked. Reported, never silent.
    Unchecked { bytes: u64 },
}

/// Why a model install failed.
#[derive(Debug, thiserror::Error)]
pub enum ModelError {
    #[error(transparent)]
    Network(#[from] kw_network_policy::PolicyError),

    #[error(transparent)]
    Integrity(#[from] kw_sidecar::IntegrityError),

    #[error("could not {action} {path}: {source}")]
    Io {
        action: &'static str,
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error(
        "{file} is {actual} bytes but about {expected} was expected. The download was cut \
         short, or the published file changed."
    )]
    Size {
        file: String,
        actual: u64,
        expected: u64,
    },

    #[error("{id:?} is not a whisper model this build knows about")]
    UnknownModel { id: String },
}

/// A model that is installed and checked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledModel {
    pub id: String,
    pub path: PathBuf,
    pub bytes: u64,
    pub verification: Verification,
}

/// Progress of a model download.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelProgress {
    Downloading { written: u64, total: Option<u64> },
    Verifying,
}

/// Where a model file lives once installed.
#[must_use]
pub fn model_path(dir: &Path, model: &WhisperModel) -> PathBuf {
    dir.join(&model.file_name)
}

/// Whether a model is already installed and passes its check.
///
/// Used before offering to download: a partially written file from an
/// interrupted download exists but must not count as installed.
pub fn is_installed(dir: &Path, model: &WhisperModel) -> bool {
    let path = model_path(dir, model);
    let Ok(metadata) = std::fs::metadata(&path) else {
        return false;
    };
    check(
        &path,
        metadata.len(),
        &integrity_for(model),
        &model.file_name,
    )
    .is_ok()
}

/// Download a model into `dir` and verify it.
pub async fn install(
    policy: &NetworkPolicy,
    model: &WhisperModel,
    dir: &Path,
    progress: &mut dyn FnMut(ModelProgress),
) -> Result<InstalledModel, ModelError> {
    std::fs::create_dir_all(dir).map_err(|source| ModelError::Io {
        action: "create",
        path: dir.to_path_buf(),
        source,
    })?;

    // Downloaded beside the destination and renamed on success, so an
    // interrupted download never looks like an installed model.
    let final_path = model_path(dir, model);
    let partial = final_path.with_extension("partial");

    policy
        .download_to_file(
            RequestPurpose::UserTriggeredModelDownload,
            &model.download_url,
            &partial,
            &mut |written, total| progress(ModelProgress::Downloading { written, total }),
        )
        .await?;

    progress(ModelProgress::Verifying);

    let bytes = std::fs::metadata(&partial)
        .map_err(|source| ModelError::Io {
            action: "measure",
            path: partial.clone(),
            source,
        })?
        .len();

    let verification = match check(&partial, bytes, &integrity_for(model), &model.file_name) {
        Ok(verification) => verification,
        Err(err) => {
            // A file that failed its check must not be left where a later run
            // could mistake it for an install.
            let _ = std::fs::remove_file(&partial);
            return Err(err);
        }
    };

    std::fs::rename(&partial, &final_path).map_err(|source| ModelError::Io {
        action: "move into place",
        path: final_path.clone(),
        source,
    })?;

    Ok(InstalledModel {
        id: model.id.clone(),
        path: final_path,
        bytes,
        verification,
    })
}

/// Hold a file to whatever check its model allows.
fn check(
    path: &Path,
    bytes: u64,
    integrity: &Integrity,
    file_name: &str,
) -> Result<Verification, ModelError> {
    match integrity {
        Integrity::Pinned { digest, size_bytes } => {
            kw_sidecar::integrity::verify(path, *size_bytes, digest)?;
            Ok(Verification::DigestMatched)
        }
        Integrity::Size { expected_bytes } => {
            if within_tolerance(bytes, *expected_bytes) {
                Ok(Verification::SizeOnly { bytes })
            } else {
                Err(ModelError::Size {
                    file: file_name.to_owned(),
                    actual: bytes,
                    expected: *expected_bytes,
                })
            }
        }
        Integrity::Unknown => Ok(Verification::Unchecked { bytes }),
    }
}

/// Whether `actual` is close enough to `expected`.
#[must_use]
pub fn within_tolerance(actual: u64, expected: u64) -> bool {
    if expected == 0 {
        return false;
    }
    let expected_f = expected as f64;
    let difference = (actual as f64 - expected_f).abs();
    difference / expected_f <= SIZE_TOLERANCE
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

    fn registry() -> &'static kw_provider_registry::Registry {
        kw_provider_registry::registry().expect("the shipped registry loads")
    }

    fn model(id: &str) -> &'static WhisperModel {
        registry()
            .local_asr
            .whisper
            .iter()
            .find(|m| m.id == id)
            .expect("a whisper model with that id")
    }

    #[test]
    fn the_pinned_model_is_held_to_its_digest() {
        match integrity_for(model("tiny")) {
            Integrity::Pinned { digest, size_bytes } => {
                assert_eq!(size_bytes, 77_691_713);
                assert_eq!(digest.as_str().len(), 64);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn an_unpinned_model_says_so_rather_than_implying_a_digest() {
        // The distinction the caller needs: nothing here pretends the bytes were
        // verified against a known-good digest when they were not.
        match integrity_for(model("base")) {
            Integrity::Size { expected_bytes } => assert!(expected_bytes > 0),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn every_shipped_whisper_model_can_be_checked_somehow() {
        // A model with neither a digest nor a size would install unverified, so
        // this fails if the registry ever loses expectedSizeBytes.
        for model in &registry().local_asr.whisper {
            assert_ne!(
                integrity_for(model),
                Integrity::Unknown,
                "{} has nothing to check against",
                model.id
            );
        }
    }

    #[test]
    fn the_rounded_registry_size_still_accepts_the_real_file() {
        // ggml-tiny.bin is declared as 78,000,000 and is really 77,691,713.
        // A strict equality check would reject every model in the registry.
        assert!(within_tolerance(77_691_713, 78_000_000));
    }

    #[test]
    fn a_truncated_download_is_outside_the_tolerance() {
        assert!(!within_tolerance(4_096, 78_000_000));
        assert!(!within_tolerance(0, 78_000_000));
        // Half a file is not a rounding difference.
        assert!(!within_tolerance(39_000_000, 78_000_000));
    }

    #[test]
    fn a_zero_expected_size_never_passes() {
        // Otherwise a registry entry with a missing size would accept anything.
        assert!(!within_tolerance(0, 0));
        assert!(!within_tolerance(100, 0));
    }

    #[test]
    fn the_model_path_uses_the_registry_file_name() {
        let dir = Path::new("C:/kw/models");
        assert_eq!(
            model_path(dir, model("tiny")),
            dir.join("ggml-tiny.bin"),
            "the file name has to match what whisper-server is told to load"
        );
    }

    #[test]
    fn a_partial_file_does_not_count_as_installed() {
        // An interrupted download leaves a file; treating it as installed sends
        // whisper-server at a truncated model.
        let dir = tempfile::tempdir().unwrap();
        let model = model("tiny");

        assert!(!is_installed(dir.path(), model), "nothing there yet");

        std::fs::write(model_path(dir.path(), model), b"first few bytes").unwrap();
        assert!(
            !is_installed(dir.path(), model),
            "a short file is not an install"
        );
    }

    #[test]
    fn a_file_that_fails_its_size_check_is_reported_with_both_numbers() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ggml-base.bin");
        std::fs::write(&path, b"nope").unwrap();

        let err = check(&path, 4, &integrity_for(model("base")), "ggml-base.bin").unwrap_err();

        let message = err.to_string();
        assert!(message.contains('4'), "{message}");
        assert!(message.contains("cut short"), "{message}");
    }

    #[test]
    fn an_unknown_integrity_reports_unchecked_rather_than_passing_quietly() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("model.bin");
        std::fs::write(&path, b"bytes").unwrap();

        assert_eq!(
            check(&path, 5, &Integrity::Unknown, "model.bin").unwrap(),
            Verification::Unchecked { bytes: 5 }
        );
    }

    #[test]
    fn the_recommended_model_is_the_one_the_settings_screen_would_offer() {
        // Guards against a registry edit that leaves nothing recommended, which
        // would make the install screen have no default.
        let recommended: Vec<&str> = registry()
            .local_asr
            .whisper
            .iter()
            .filter(|m| m.recommended)
            .map(|m| m.id.as_str())
            .collect();
        assert_eq!(recommended, ["base"]);
    }
}
