//! Install sherpa speech models from immutable, per-file manifests.
//!
//! Existing archive layouts and model IDs remain compatible. New installs stage
//! every file, verify exact sizes and SHA-256, then publish under an OS install lock.
//! Cancellation drops staging data. Complete existing layouts are reused unchanged.
//! Archive metadata and the legacy installer remain for models without file manifests.

use std::path::{Path, PathBuf};

use kw_network_policy::{NetworkPolicy, RequestPurpose};
use kw_provider_registry::SherpaModel;

/// Why a sherpa model install failed.
#[derive(Debug, thiserror::Error)]
pub enum SherpaModelError {
    #[error("invalid model file manifest: {0}")]
    Manifest(String),
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

    if !model.files.is_empty() {
        return install_files(policy, model, dir, progress).await;
    }

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

fn io_error(action: &'static str, path: &Path, source: std::io::Error) -> SherpaModelError {
    SherpaModelError::Io {
        action,
        path: path.to_owned(),
        source,
    }
}

fn validate_files(
    files: &[kw_provider_registry::ModelFile],
    required: &[&str],
) -> Result<u64, SherpaModelError> {
    let mut seen = std::collections::BTreeSet::new();
    let mut total = 0_u64;
    for file in files {
        // Only the measured runtime's four flat filenames are accepted.
        if !required.contains(&file.path.as_str())
            || !seen.insert(&file.path)
            || file.expected_size_bytes == 0
            || kw_sidecar::Sha256Digest::parse(&file.sha256).is_err()
        {
            return Err(SherpaModelError::Manifest(file.path.clone()));
        }
        let url =
            url::Url::parse(&file.url).map_err(|_| SherpaModelError::Manifest(file.url.clone()))?;
        let segments: Vec<_> = url.path().split('/').collect();
        if url.scheme() != "https"
            || url.host_str() != Some("huggingface.co")
            || segments.len() != 6
            || segments.get(3) != Some(&"resolve")
            || !segments.get(4).is_some_and(|s| {
                s.len() == 40
                    && s.bytes()
                        .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
            })
            || segments.get(5) != Some(&file.path.as_str())
            || url.query().is_some()
            || url.fragment().is_some()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.port().is_some()
        {
            return Err(SherpaModelError::Manifest(file.url.clone()));
        }
        total = total
            .checked_add(file.expected_size_bytes)
            .ok_or_else(|| SherpaModelError::Manifest("size overflow".into()))?;
    }
    if seen.len() != required.len() {
        return Err(SherpaModelError::Manifest(
            "required runtime files missing".into(),
        ));
    }
    Ok(total)
}

async fn install_files(
    policy: &NetworkPolicy,
    model: &SherpaModel,
    dir: &Path,
    progress: &mut (dyn FnMut(Progress) + Send),
) -> Result<InstalledSherpaModel, SherpaModelError> {
    let bytes = install_verified_files(
        policy,
        &model.id,
        &model.files,
        dir,
        &model.extract_dir,
        &[
            "encoder.int8.onnx",
            "decoder.int8.onnx",
            "joiner.int8.onnx",
            "tokens.txt",
        ],
        progress,
    )
    .await?;
    let target = model_dir(dir, model);
    let files = crate::sherpa::discover_transducer_files(&target).map_err(|source| {
        SherpaModelError::Layout {
            model: model.id.clone(),
            source,
        }
    })?;
    Ok(InstalledSherpaModel {
        id: model.id.clone(),
        dir: target,
        files,
        bytes,
    })
}

/// Install a flat, immutable model payload without modifying an existing complete layout.
/// The same transaction is used for speech and segmentation model files.
pub async fn install_verified_files(
    policy: &NetworkPolicy,
    id: &str,
    files: &[kw_provider_registry::ModelFile],
    dir: &Path,
    folder: &str,
    required: &[&str],
    progress: &mut (dyn FnMut(Progress) + Send),
) -> Result<u64, SherpaModelError> {
    install_files_with(
        &PolicyDownload(policy),
        id,
        files,
        dir,
        folder,
        required,
        progress,
    )
    .await
}

#[async_trait::async_trait]
trait FileDownload: Send + Sync {
    async fn fetch(
        &self,
        file: &kw_provider_registry::ModelFile,
        path: &Path,
        progress: &mut (dyn FnMut(u64) + Send),
    ) -> Result<(), SherpaModelError>;
}

struct PolicyDownload<'a>(&'a NetworkPolicy);
#[async_trait::async_trait]
impl FileDownload for PolicyDownload<'_> {
    async fn fetch(
        &self,
        file: &kw_provider_registry::ModelFile,
        path: &Path,
        progress: &mut (dyn FnMut(u64) + Send),
    ) -> Result<(), SherpaModelError> {
        self.0
            .download_to_file_bounded(
                RequestPurpose::UserTriggeredModelDownload,
                &file.url,
                path,
                file.expected_size_bytes,
                &mut |written, _| progress(written),
            )
            .await?;
        Ok(())
    }
}

async fn install_files_with(
    download: &dyn FileDownload,
    id: &str,
    files: &[kw_provider_registry::ModelFile],
    dir: &Path,
    folder: &str,
    required: &[&str],
    progress: &mut (dyn FnMut(Progress) + Send),
) -> Result<u64, SherpaModelError> {
    let total = validate_files(files, required)?;
    for name in [id, folder] {
        if name.is_empty()
            || !name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
            || name == "."
            || name == ".."
        {
            return Err(SherpaModelError::Manifest(name.to_owned()));
        }
    }
    std::fs::create_dir_all(dir).map_err(|e| io_error("create model root", dir, e))?;
    // OS lock releases on cancellation or process death. Never unlink the lock file:
    // another installer may already have opened that same inode.
    let target = dir.join(folder);
    let lock_path = dir.join(format!("{id}.install.lock"));
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&lock_path)
        .map_err(|e| io_error("open install lock", &lock_path, e))?;
    lock.try_lock().map_err(|e| {
        io_error(
            "lock model (another install may be running)",
            &lock_path,
            e.into(),
        )
    })?;
    if required.iter().all(|name| target.join(name).is_file()) {
        return Ok(0);
    }
    let stage = tempfile::Builder::new()
        .prefix(".model-install-")
        .tempdir_in(dir)
        .map_err(|e| io_error("create staging directory", dir, e))?;
    let payload = stage.path().join("payload");
    std::fs::create_dir(&payload).map_err(|e| io_error("create payload", &payload, e))?;
    let mut completed = 0;
    for file in files {
        let path = payload.join(&file.path);
        download
            .fetch(file, &path, &mut |written| {
                progress(Progress::Downloading {
                    written: completed + written,
                    total: Some(total),
                })
            })
            .await?;
        let digest = kw_sidecar::Sha256Digest::parse(&file.sha256)
            .map_err(|_| SherpaModelError::Manifest(file.path.clone()))?;
        kw_sidecar::integrity::verify(&path, file.expected_size_bytes, &digest)?;
        completed += file.expected_size_bytes;
    }

    // No await between moving the old incomplete layout and publishing the new one.
    // The backup lives outside the temporary directory so failed rollback cannot delete it.
    let backup = dir.join(format!("{id}.incomplete-backup"));
    if backup.exists() {
        return Err(SherpaModelError::Manifest(format!(
            "recover retained backup at {}",
            backup.display()
        )));
    }
    let had_old = target.exists();
    if had_old {
        std::fs::rename(&target, &backup)
            .map_err(|e| io_error("retain previous layout", &target, e))?;
    }
    if let Err(e) = std::fs::rename(&payload, &target) {
        if had_old {
            let _ = std::fs::rename(&backup, &target);
        }
        return Err(io_error("publish verified model", &target, e));
    }
    if had_old {
        let _ = std::fs::remove_dir_all(&backup);
    }
    Ok(completed)
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

    fn pinned_fixture() -> SherpaModel {
        let mut m = model("parakeet-unified-en-0.6b").clone();
        m.files = [
            "encoder.int8.onnx",
            "decoder.int8.onnx",
            "joiner.int8.onnx",
            "tokens.txt",
        ]
        .into_iter()
        .map(|name| kw_provider_registry::ModelFile {
            path: name.to_owned(),
            url: format!(
                "https://huggingface.co/owner/model/resolve/{}/{name}",
                "a".repeat(40)
            ),
            sha256: "ab".repeat(32),
            expected_size_bytes: 5,
        })
        .collect();
        m
    }

    #[test]
    fn manifests_reject_unsafe_paths_missing_duplicates_and_moving_urls() {
        let good = pinned_fixture();
        let names = [
            "encoder.int8.onnx",
            "decoder.int8.onnx",
            "joiner.int8.onnx",
            "tokens.txt",
        ];
        assert_eq!(validate_files(&good.files, &names).unwrap(), 20);
        for path in [
            "../tokens.txt",
            "C:/tokens.txt",
            "tokens.txt:stream",
            "tokens.txt/extra",
        ] {
            let mut m = good.clone();
            m.files[0].path = path.into();
            assert!(validate_files(&m.files, &names).is_err());
        }
        let mut m = good.clone();
        m.files.pop();
        assert!(validate_files(&m.files, &names).is_err());
        let mut m = good.clone();
        m.files[0] = m.files[1].clone();
        assert!(validate_files(&m.files, &names).is_err());
        for url in [
            "http://huggingface.co/o/m/resolve/main/tokens.txt",
            "https://example.com/tokens.txt",
            "https://huggingface.co/o/m/resolve/main/tokens.txt",
        ] {
            let mut m = good.clone();
            m.files[3].url = url.into();
            assert!(validate_files(&m.files, &names).is_err());
        }
    }

    #[tokio::test]
    async fn existing_models_are_preserved_and_concurrent_installs_are_refused() {
        let root = tempfile::tempdir().unwrap();
        let m = pinned_fixture();
        let target = model_dir(root.path(), &m);
        std::fs::create_dir(&target).unwrap();
        for f in &m.files {
            std::fs::write(target.join(&f.path), b"existing").unwrap();
        }
        let policy = NetworkPolicy::new().unwrap();
        let installed = install(&policy, &m, root.path(), &mut |_| {
            panic!("must not download")
        })
        .await
        .unwrap();
        assert_eq!(installed.bytes, 0);
        for f in &m.files {
            assert_eq!(std::fs::read(target.join(&f.path)).unwrap(), b"existing");
        }
        let lock_path = root.path().join(format!("{}.install.lock", m.id));
        let lock = std::fs::OpenOptions::new()
            .write(true)
            .open(lock_path)
            .unwrap();
        lock.try_lock().unwrap();
        assert!(install(&policy, &m, root.path(), &mut |_| {})
            .await
            .is_err());
        drop(lock);
        assert!(install(&policy, &m, root.path(), &mut |_| {}).await.is_ok());
    }

    struct FakeDownload(&'static [u8], bool);
    #[async_trait::async_trait]
    impl FileDownload for FakeDownload {
        async fn fetch(
            &self,
            _file: &kw_provider_registry::ModelFile,
            path: &Path,
            progress: &mut (dyn FnMut(u64) + Send),
        ) -> Result<(), SherpaModelError> {
            std::fs::write(path, self.0).unwrap();
            if self.1 {
                std::future::pending::<()>().await;
            }
            progress(self.0.len() as u64);
            Ok(())
        }
    }

    fn tiny_file() -> kw_provider_registry::ModelFile {
        kw_provider_registry::ModelFile {
            path: "model.onnx".into(),
            url: format!(
                "https://huggingface.co/owner/model/resolve/{}/model.onnx",
                "a".repeat(40)
            ),
            // SHA-256 of "hello".
            sha256: "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824".into(),
            expected_size_bytes: 5,
        }
    }

    #[tokio::test]
    async fn failed_and_cancelled_downloads_keep_previous_data_and_release_lock() {
        let root = tempfile::tempdir().unwrap();
        let old = root.path().join("model");
        std::fs::create_dir(&old).unwrap();
        std::fs::write(old.join("old.txt"), b"retained").unwrap();
        let files = [tiny_file()];
        for bytes in [
            b"hell".as_slice(),
            b"hello!".as_slice(),
            b"wrong".as_slice(),
        ] {
            assert!(install_files_with(
                &FakeDownload(bytes, false),
                "test",
                &files,
                root.path(),
                "model",
                &["model.onnx"],
                &mut |_| {}
            )
            .await
            .is_err());
            assert_eq!(std::fs::read(old.join("old.txt")).unwrap(), b"retained");
            assert!(!old.join("model.onnx").exists());
        }
        let mut progress = |_| {};
        let job = install_files_with(
            &FakeDownload(b"hello", true),
            "test",
            &files,
            root.path(),
            "model",
            &["model.onnx"],
            &mut progress,
        );
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(20), job)
                .await
                .is_err()
        );
        assert_eq!(
            std::fs::read_dir(root.path()).unwrap().count(),
            2,
            "old directory and lock only"
        );
        assert_eq!(
            install_files_with(
                &FakeDownload(b"hello", false),
                "test",
                &files,
                root.path(),
                "model",
                &["model.onnx"],
                &mut |_| {}
            )
            .await
            .unwrap(),
            5
        );
        assert_eq!(std::fs::read(old.join("model.onnx")).unwrap(), b"hello");
        assert!(!old.join("old.txt").exists());
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn locked_previous_directory_is_not_deleted() {
        use std::os::windows::fs::OpenOptionsExt;
        let root = tempfile::tempdir().unwrap();
        let old = root.path().join("model");
        std::fs::create_dir(&old).unwrap();
        std::fs::write(old.join("old.txt"), b"retained").unwrap();
        let _handle = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(old.join("old.txt"))
            .unwrap();
        assert!(install_files_with(
            &FakeDownload(b"hello", false),
            "test",
            &[tiny_file()],
            root.path(),
            "model",
            &["model.onnx"],
            &mut |_| {}
        )
        .await
        .is_err());
        assert!(old.join("old.txt").exists());
        assert!(!old.join("model.onnx").exists());
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
