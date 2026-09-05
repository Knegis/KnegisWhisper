//! Installing a local sherpa model archive.
//!
//! A sherpa model is a `.tar.bz2` of a whole directory -- encoder, decoder,
//! joiner, tokens, plus a `test_wavs/` sample folder -- not the single file a
//! whisper model is, so this cannot reuse `models::install` directly. What it
//! *does* reuse is `kw_sidecar::extract`: the same tar.bz2 reader
//! `ArchiveKind::TarBz2` gave the sherpa-onnx CLI sidecar in Task 17, called
//! here with an empty payload directory so the archive's own internal layout
//! survives extraction untouched -- which is the layout `sherpa::
//! discover_transducer_files` was measured against.
//!
//! `sizeMb` is only a display estimate; installation instead checks an exact
//! compressed byte count and SHA-256 pinned in the registry.
//! The download is bounded by that compressed size before it reaches disk,
//! verified before extraction, and extraction has a separate output ceiling.
//! The final layout check remains useful for diagnosing a valid archive whose
//! contents no longer match the runtime's argument contract.

use std::path::{Path, PathBuf};

use kw_network_policy::{NetworkPolicy, RequestPurpose};
use kw_provider_registry::SherpaModel;

/// Why a sherpa model install failed.
#[derive(Debug, thiserror::Error)]
pub enum SherpaModelError {
    #[error(transparent)]
    Network(#[from] kw_network_policy::PolicyError),

    #[error(transparent)]
    Extract(#[from] kw_sidecar::InstallError),

    #[error(transparent)]
    Integrity(#[from] kw_sidecar::IntegrityError),

    #[error("{model} has no pinned {field}; refusing an unauthenticated install")]
    MissingPin { model: String, field: &'static str },

    #[error(
        "{model} downloaded and extracted, but not into the layout this build knows how to \
         drive: {source}"
    )]
    Layout {
        model: String,
        #[source]
        source: crate::sherpa::LayoutError,
    },

    #[error("could not {action} {path}: {source}")]
    Io {
        action: &'static str,
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

/// Progress of a sherpa model install.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Progress {
    Downloading { written: u64, total: Option<u64> },
    Extracting,
}

/// A model that is installed and checked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledSherpaModel {
    pub id: String,
    /// The model's own directory: `dir.join(&model.extract_dir)`.
    pub dir: PathBuf,
    /// Bytes downloaded, after exact size and digest verification.
    pub bytes: u64,
    /// Resolved by the same discovery `transcribe` will use, so a caller
    /// that just installed a model does not have to call
    /// `sherpa::discover_transducer_files` a second time.
    pub files: crate::sherpa::TransducerFiles,
}

/// Where one model's files live once installed.
///
/// `extract_dir` rather than `id`, because that is the archive's own
/// top-level directory name -- the name `kw_sidecar::extract` actually
/// writes to, measured with `examples/sherpa_probe.rs`.
#[must_use]
pub fn model_dir(dir: &Path, model: &SherpaModel) -> PathBuf {
    dir.join(&model.extract_dir)
}

/// Whether a model is already installed and has the measured layout.
///
/// Checked via `sherpa::discover_transducer_files` rather than just "the
/// directory exists": a partial extraction from an interrupted install
/// leaves a directory behind, and that must not count as installed.
#[must_use]
pub fn is_installed(dir: &Path, model: &SherpaModel) -> bool {
    crate::sherpa::discover_transducer_files(&model_dir(dir, model)).is_ok()
}

/// Download a sherpa model archive into `dir` and extract it.
///
/// `dir` is the models root, not the model's own directory -- the archive's
/// internal top-level directory (`model.extract_dir`) becomes the model's
/// directory once extraction finishes, the same as `examples/sherpa_probe.rs`
/// measured it.
pub async fn install(
    policy: &NetworkPolicy,
    model: &SherpaModel,
    dir: &Path,
    // Held across awaits, so `Send`; see `NetworkPolicy::download_to_file`.
    progress: &mut (dyn FnMut(Progress) + Send),
) -> Result<InstalledSherpaModel, SherpaModelError> {
    std::fs::create_dir_all(dir).map_err(|source| SherpaModelError::Io {
        action: "create",
        path: dir.to_path_buf(),
        source,
    })?;

    // Downloaded beside the destination, so an interrupted download never
    // looks like a partial model directory.
    let archive = dir.join(format!("{}.tar.bz2.partial", model.id));

    let expected_size = model
        .expected_size_bytes
        .ok_or_else(|| SherpaModelError::MissingPin {
            model: model.id.clone(),
            field: "compressed size",
        })?;
    let digest = model
        .sha256
        .as_deref()
        .ok_or_else(|| SherpaModelError::MissingPin {
            model: model.id.clone(),
            field: "SHA-256 digest",
        })?;
    let digest =
        kw_sidecar::Sha256Digest::parse(digest).map_err(|_| SherpaModelError::MissingPin {
            model: model.id.clone(),
            field: "valid SHA-256 digest",
        })?;
    let extracted_limit =
        model
            .max_extracted_size_bytes
            .ok_or_else(|| SherpaModelError::MissingPin {
                model: model.id.clone(),
                field: "extracted-size limit",
            })?;

    policy
        .download_to_file_bounded(
            RequestPurpose::UserTriggeredModelDownload,
            &model.download_url,
            &archive,
            expected_size,
            &mut |written, total| progress(Progress::Downloading { written, total }),
        )
        .await?;

    kw_sidecar::integrity::verify(&archive, expected_size, &digest)?;

    let bytes = std::fs::metadata(&archive)
        .map_err(|source| SherpaModelError::Io {
            action: "measure",
            path: archive.clone(),
            source,
        })?
        .len();

    progress(Progress::Extracting);

    let target_dir = model_dir(dir, model);
    // A previous incomplete extraction must not merge with this one.
    let _ = std::fs::remove_dir_all(&target_dir);

    // Empty payload directory: unlike a sidecar binary, where only the files
    // beside the executable are wanted, the model's *whole* internal
    // structure is what `sherpa::discover_transducer_files` was measured
    // against, so nothing here is stripped.
    kw_sidecar::extract_with_limit(
        &archive,
        kw_sidecar::ArchiveKind::TarBz2,
        "",
        "",
        dir,
        extracted_limit,
    )?;

    let _ = std::fs::remove_file(&archive);

    // The real check: does the extracted directory have the files the CLI
    // needs, under the names this build knows to look for? See the module
    // comment for why this replaces a size comparison rather than
    // supplementing one.
    let files = crate::sherpa::discover_transducer_files(&target_dir).map_err(|source| {
        SherpaModelError::Layout {
            model: model.id.clone(),
            source,
        }
    })?;

    Ok(InstalledSherpaModel {
        id: model.id.clone(),
        dir: target_dir,
        bytes,
        files,
    })
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

    fn model(id: &str) -> &'static SherpaModel {
        registry()
            .local_asr
            .sherpa
            .iter()
            .find(|m| m.id == id)
            .unwrap_or_else(|| panic!("{id} is in the shipped registry"))
    }

    #[test]
    fn the_model_directory_is_the_registrys_extract_dir() {
        let dir = Path::new("/data/models");
        let parakeet = model("parakeet-unified-en-0.6b");
        assert_eq!(
            model_dir(dir, parakeet),
            dir.join("sherpa-onnx-nemo-parakeet-unified-en-0.6b-int8-non-streaming")
        );
    }

    #[test]
    fn every_shipped_sherpa_model_still_declares_a_size_for_display() {
        // `sizeMb` is shown to the user before they download anything, even
        // though (see the module comment) it is not what this module checks
        // the install against. If this ever fires, the registry lost
        // `sizeMb` for one and a settings screen would have nothing to show.
        for model in &registry().local_asr.sherpa {
            assert!(model.size_mb.is_some(), "{} has no declared size", model.id);
        }
    }

    #[test]
    fn every_installable_sherpa_model_has_integrity_and_size_limits() {
        for model in registry()
            .local_asr
            .sherpa
            .iter()
            .filter(|model| !model.streaming)
        {
            assert!(
                model
                    .sha256
                    .as_deref()
                    .is_some_and(|digest| { kw_sidecar::Sha256Digest::parse(digest).is_ok() }),
                "{} has no valid digest",
                model.id
            );
            assert!(model.expected_size_bytes.is_some_and(|size| size > 0));
            assert!(model.max_extracted_size_bytes.is_some_and(|size| size > 0));
        }
    }

    #[test]
    fn a_directory_missing_the_measured_layout_is_not_installed() {
        let dir = tempfile::tempdir().unwrap();
        let parakeet = model("parakeet-unified-en-0.6b");
        assert!(!is_installed(dir.path(), parakeet));

        // A directory that exists but is incomplete -- an interrupted
        // extraction -- must not count as installed either.
        std::fs::create_dir_all(model_dir(dir.path(), parakeet)).unwrap();
        std::fs::write(
            model_dir(dir.path(), parakeet).join("tokens.txt"),
            b"partial",
        )
        .unwrap();
        assert!(!is_installed(dir.path(), parakeet));
    }

    #[test]
    fn a_complete_layout_is_installed() {
        let dir = tempfile::tempdir().unwrap();
        let parakeet = model("parakeet-unified-en-0.6b");
        let target = model_dir(dir.path(), parakeet);
        std::fs::create_dir_all(&target).unwrap();
        for name in [
            "encoder.int8.onnx",
            "decoder.int8.onnx",
            "joiner.int8.onnx",
            "tokens.txt",
        ] {
            std::fs::write(target.join(name), b"x").unwrap();
        }
        assert!(is_installed(dir.path(), parakeet));
    }
}
