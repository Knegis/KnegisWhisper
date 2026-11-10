//! Install speaker models with verified downloads.
//!
//! Segmentation uses a commit-pinned file manifest and the shared staging/locking
//! transaction. The embedding model retains its size/digest-pinned GitHub download:
//! a byte-equivalent publisher mirror has not been verified (KNEG-532).
//! Existing installed paths and the English embedding limitation remain unchanged.

use std::path::{Path, PathBuf};

use kw_network_policy::{NetworkPolicy, RequestPurpose};
use kw_provider_registry::{DiarizationModel, DiarizationRole};

/// Why installing a diarization model failed.
#[derive(Debug, thiserror::Error)]
pub enum InstallError {
    #[error(transparent)]
    Files(#[from] kw_transcription_core::sherpa_models::SherpaModelError),
    #[error(transparent)]
    Network(#[from] kw_network_policy::PolicyError),

    #[error(transparent)]
    Extract(#[from] kw_sidecar::InstallError),

    #[error(transparent)]
    Integrity(#[from] kw_sidecar::IntegrityError),

    #[error("{model} has no pinned {field}; refusing an unauthenticated install")]
    MissingPin { model: String, field: &'static str },

    #[error(
        "{model} declares neither an archive directory nor a file name, so there is no way to \
         know what installing it should produce"
    )]
    Shapeless { model: String },

    #[error("{model} downloaded and extracted, but not into the expected layout: {source}")]
    Layout {
        model: String,
        #[source]
        source: crate::diarization::LayoutError,
    },

    #[error("could not {action} {path}: {source}")]
    Io {
        action: &'static str,
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

/// Progress of one model's install.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Progress {
    Downloading { written: u64, total: Option<u64> },
    Verifying,
    Extracting,
}

/// Where one model ends up under the diarization models root.
///
/// One function for both shapes, because every caller that wants a path wants
/// it without first asking which kind of model this is.
#[must_use]
pub fn model_path(root: &Path, model: &DiarizationModel) -> Option<PathBuf> {
    model
        .extract_dir
        .as_ref()
        .or(model.file_name.as_ref())
        .map(|name| root.join(name))
}

/// The two paths [`crate::diarization::Models::locate`] wants, if both models
/// are installed.
///
/// Returns `None` rather than an error when something is missing: "not
/// installed yet" is the ordinary state of a machine that has never used this
/// feature, and an error would make the status call look like a fault.
#[must_use]
pub fn installed(root: &Path, models: &[DiarizationModel]) -> Option<crate::diarization::Models> {
    let seg = of_role(models, DiarizationRole::Segmentation)?;
    let emb = of_role(models, DiarizationRole::Embedding)?;
    crate::diarization::Models::locate(&model_path(root, seg)?, &model_path(root, emb)?).ok()
}

/// Whether one model on its own is installed.
///
/// Per model rather than per pair, because a screen offering two downloads has
/// to be able to say which of them is already there.
#[must_use]
pub fn is_installed(root: &Path, model: &DiarizationModel) -> bool {
    match model_path(root, model) {
        // The archive's own `model.onnx` rather than the directory: a partial
        // extraction leaves a directory behind, and that must not read as
        // installed. The same rule `sherpa_models::is_installed` follows.
        Some(path) if model.extract_dir.is_some() => path
            .join(crate::diarization::SEGMENTATION_MODEL_FILE)
            .is_file(),
        Some(path) => path.is_file(),
        None => false,
    }
}

fn of_role(models: &[DiarizationModel], role: DiarizationRole) -> Option<&DiarizationModel> {
    models.iter().find(|m| m.role == Some(role))
}

/// Download one model, verify it, and put it where [`installed`] will find it.
///
/// # Errors
///
/// [`InstallError`], which distinguishes a refused install (no pin) from a
/// failed one.
pub async fn install(
    policy: &NetworkPolicy,
    model: &DiarizationModel,
    root: &Path,
    // Held across awaits, so `Send`; see `NetworkPolicy::download_to_file`.
    progress: &mut (dyn FnMut(Progress) + Send),
) -> Result<PathBuf, InstallError> {
    std::fs::create_dir_all(root).map_err(|source| InstallError::Io {
        action: "create",
        path: root.to_path_buf(),
        source,
    })?;

    let destination = model_path(root, model).ok_or_else(|| InstallError::Shapeless {
        model: model.id.clone(),
    })?;

    if !model.files.is_empty() {
        let folder = model
            .extract_dir
            .as_deref()
            .ok_or_else(|| InstallError::Shapeless {
                model: model.id.clone(),
            })?;
        kw_transcription_core::sherpa_models::install_verified_files(
            policy,
            &model.id,
            &model.files,
            root,
            folder,
            &[crate::diarization::SEGMENTATION_MODEL_FILE],
            &mut |p| match p {
                kw_transcription_core::sherpa_models::Progress::Downloading { written, total } => {
                    progress(Progress::Downloading { written, total })
                }
                kw_transcription_core::sherpa_models::Progress::Extracting => {
                    progress(Progress::Extracting)
                }
            },
        )
        .await?;
        return Ok(destination);
    }

    let expected_size = model
        .expected_size_bytes
        .ok_or_else(|| InstallError::MissingPin {
            model: model.id.clone(),
            field: "compressed size",
        })?;
    let digest = model
        .sha256
        .as_deref()
        .ok_or_else(|| InstallError::MissingPin {
            model: model.id.clone(),
            field: "SHA-256 digest",
        })?;
    let digest = kw_sidecar::Sha256Digest::parse(digest).map_err(|_| InstallError::MissingPin {
        model: model.id.clone(),
        field: "valid SHA-256 digest",
    })?;

    let partial = root.join(format!("{}.partial", model.id));

    policy
        .download_to_file_bounded(
            RequestPurpose::UserTriggeredModelDownload,
            &model.download_url,
            &partial,
            expected_size,
            &mut |written, total| progress(Progress::Downloading { written, total }),
        )
        .await?;

    progress(Progress::Verifying);
    if let Err(err) = kw_sidecar::integrity::verify(&partial, expected_size, &digest) {
        // Removed rather than left for the next attempt to find: a file that
        // failed its digest is not a head start, and leaving it invites a
        // resume that trusts the part already on disk.
        let _ = std::fs::remove_file(&partial);
        return Err(err.into());
    }

    if model.extract_dir.is_some() {
        let limit = model
            .max_extracted_size_bytes
            .ok_or_else(|| InstallError::MissingPin {
                model: model.id.clone(),
                field: "extracted-size limit",
            })?;
        progress(Progress::Extracting);

        // A previous incomplete extraction must not merge with this one.
        let _ = std::fs::remove_dir_all(&destination);

        // Empty payload directory, as `sherpa_models::install` does: the
        // archive's whole internal structure is what discovery was measured
        // against, so nothing is stripped.
        kw_sidecar::extract_with_limit(
            &partial,
            kw_sidecar::ArchiveKind::TarBz2,
            "",
            "",
            root,
            limit,
        )?;
        let _ = std::fs::remove_file(&partial);

        let model_file = destination.join(crate::diarization::SEGMENTATION_MODEL_FILE);
        if !model_file.is_file() {
            return Err(InstallError::Layout {
                model: model.id.clone(),
                source: crate::diarization::LayoutError::MissingSegmentation {
                    dir: destination.clone(),
                    file: crate::diarization::SEGMENTATION_MODEL_FILE,
                },
            });
        }
    } else {
        std::fs::rename(&partial, &destination).map_err(|source| InstallError::Io {
            action: "move into place",
            path: destination.clone(),
            source,
        })?;
    }

    Ok(destination)
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

    fn shipped() -> &'static [DiarizationModel] {
        &kw_provider_registry::registry()
            .expect("the shipped registry loads")
            .local_asr
            .diarization
    }

    #[tokio::test]
    #[ignore = "Downloads the pinned 6 MB segmentation model"]
    async fn pinned_segmentation_installs_from_immutable_files() {
        let dir = tempfile::tempdir().unwrap();
        let model = model(DiarizationRole::Segmentation);
        assert!(!model.files.is_empty());
        let policy = NetworkPolicy::new().unwrap();
        let path = install(&policy, model, dir.path(), &mut |p| {
            assert!(!matches!(p, Progress::Extracting));
        })
        .await
        .unwrap();
        assert!(path
            .join(crate::diarization::SEGMENTATION_MODEL_FILE)
            .is_file());
        assert!(is_installed(dir.path(), model));
    }

    fn model(role: DiarizationRole) -> &'static DiarizationModel {
        of_role(shipped(), role).expect("both roles are in the shipped registry")
    }

    #[test]
    fn the_two_shapes_produce_two_different_paths() {
        let root = Path::new("/models/diar");
        let seg = model_path(root, model(DiarizationRole::Segmentation)).unwrap();
        let emb = model_path(root, model(DiarizationRole::Embedding)).unwrap();

        // A directory to extract into, against a file to land as.
        assert!(seg.ends_with("sherpa-onnx-pyannote-segmentation-3-0"));
        assert!(emb.extension().is_some_and(|e| e == "onnx"));
        assert_ne!(seg, emb);
    }

    #[test]
    fn a_model_declaring_neither_shape_has_no_path() {
        // The generator refuses to emit one, so this can only arrive from a
        // hand-edited registry -- and `install` answers it with `Shapeless`
        // rather than picking a path.
        let mut broken = model(DiarizationRole::Embedding).clone();
        broken.file_name = None;
        broken.extract_dir = None;
        assert_eq!(model_path(Path::new("/m"), &broken), None);
    }

    #[test]
    fn an_extracted_directory_with_no_model_file_is_not_installed() {
        // The failure this rule exists for: an interrupted extraction leaves
        // the directory behind. Without the file check, the next launch reads
        // that as an installed model and the CLI fails much later with a
        // sentence about configuration.
        let root = std::env::temp_dir().join("kw-diar-install-test");
        let _ = std::fs::remove_dir_all(&root);
        let seg = model(DiarizationRole::Segmentation);

        std::fs::create_dir_all(model_path(&root, seg).unwrap()).unwrap();
        assert!(
            !is_installed(&root, seg),
            "an empty directory is not a model"
        );

        std::fs::write(
            model_path(&root, seg)
                .unwrap()
                .join(crate::diarization::SEGMENTATION_MODEL_FILE),
            b"not really a model",
        )
        .unwrap();
        assert!(is_installed(&root, seg));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn both_must_be_there_before_a_run_can_be_configured() {
        // `installed` is what decides whether the feature can be offered at
        // all. One model of two is not half a diarization -- it is none.
        let root = std::env::temp_dir().join("kw-diar-pair-test");
        let _ = std::fs::remove_dir_all(&root);
        let seg = model(DiarizationRole::Segmentation);
        let emb = model(DiarizationRole::Embedding);

        assert!(installed(&root, shipped()).is_none(), "nothing installed");

        std::fs::create_dir_all(model_path(&root, seg).unwrap()).unwrap();
        std::fs::write(
            model_path(&root, seg)
                .unwrap()
                .join(crate::diarization::SEGMENTATION_MODEL_FILE),
            b"x",
        )
        .unwrap();
        assert!(
            installed(&root, shipped()).is_none(),
            "the segmentation model alone cannot diarize"
        );

        std::fs::write(model_path(&root, emb).unwrap(), b"x").unwrap();
        let pair = installed(&root, shipped()).expect("both installed");
        assert!(pair
            .segmentation
            .ends_with(crate::diarization::SEGMENTATION_MODEL_FILE));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn an_unpinned_model_is_refused_before_any_request() {
        // The same refusal `sherpa_models::install` makes. Asserted against a
        // model with the digest stripped rather than against the shipped pair,
        // which are both pinned -- so this stays a test of the refusal and not
        // of today's registry contents.
        let mut unpinned = model(DiarizationRole::Embedding).clone();
        unpinned.sha256 = None;
        let policy = kw_network_policy::NetworkPolicy::new().expect("a policy");
        let root = std::env::temp_dir().join("kw-diar-unpinned-test");

        let err = install(&policy, &unpinned, &root, &mut |_| {})
            .await
            .expect_err("an unpinned model must not be downloaded");
        assert!(
            matches!(err, InstallError::MissingPin { field, .. } if field.contains("digest")),
            "{err}"
        );
    }
}
