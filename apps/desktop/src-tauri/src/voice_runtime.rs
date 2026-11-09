//! Explicit Known voices installation; never replaces the sherpa CLI directory.
use std::path::{Path, PathBuf};
const VERSION: &str = "known-voices-1.13.4";
const PREFIX: &str = "sherpa-onnx-v1.13.4-win-x64-shared-MT-Release/lib";
pub const FILES: [&str; 3] = [
    "sherpa-onnx-c-api.dll",
    "onnxruntime.dll",
    "onnxruntime_providers_shared.dll",
];
pub fn directory() -> PathBuf {
    kw_storage::paths::sidecar_dir().path.join(VERSION)
}
pub fn ready() -> bool {
    let dir = directory();
    dir.join("verified-archive.txt").is_file() && FILES.iter().all(|f| dir.join(f).is_file())
}
/// OS file sharing provides cross-process exclusion and releases automatically on crash.
pub fn lease() -> Result<std::fs::File, String> {
    lease_in(&kw_storage::paths::sidecar_dir().path)
}
fn lease_in(root: &Path) -> Result<std::fs::File, String> {
    std::fs::create_dir_all(root).map_err(|e| e.to_string())?;
    let mut options = std::fs::OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.share_mode(0);
    }
    options.open(root.join("known-voices.lock")).map_err(|_| {
        "Known voices is in use. Wait for extraction or repair to finish, then try again.".into()
    })
}
fn publish(archive: &Path, asset: &kw_sidecar::Asset, root: &Path) -> Result<(), String> {
    kw_sidecar::integrity::verify(archive, asset.size_bytes, &asset.sha256)
        .map_err(|e| e.to_string())?;
    let stage = tempfile::Builder::new()
        .prefix(".known-voices-install-")
        .tempdir_in(root)
        .map_err(|e| e.to_string())?;
    let fresh = stage.path().join("fresh");
    std::fs::create_dir(&fresh).map_err(|e| e.to_string())?;
    kw_sidecar::extract_with_limit(archive, asset.archive, PREFIX, "", &fresh, 22013440)
        .map_err(|e| e.to_string())?;
    if std::fs::read_dir(&fresh)
        .map_err(|e| e.to_string())?
        .count()
        != 3
        || !FILES.iter().all(|f| fresh.join(f).is_file())
    {
        return Err("Incomplete voice runtime archive".into());
    }
    std::fs::write(fresh.join("verified-archive.txt"), asset.sha256.to_string())
        .map_err(|e| e.to_string())?;
    let target = root.join(VERSION);
    let backup = stage.path().join("previous");
    if target.exists() {
        std::fs::rename(&target, &backup)
            .map_err(|_| "Voice runtime is in use; repair was not applied")?;
    }
    if let Err(e) = std::fs::rename(&fresh, &target) {
        if backup.exists() {
            let _ = std::fs::rename(&backup, &target);
        }
        return Err(e.to_string());
    }
    Ok(())
}
pub async fn install(asr: &crate::local_asr::LocalAsr) -> Result<(), String> {
    if !cfg!(windows) {
        return Err("Known voices currently requires Windows".into());
    }
    let _lease = lease()?;
    let (_, asset) = asr.catalog.resolve("sherpa").map_err(|e| e.to_string())?;
    // This installer only understands the measured, pinned DLL payload.
    if asset.sha256.to_string()
        != "c312b30e55258d291067537ce4a6f90e155f1c16b6a10381a5729924e98b7879"
    {
        return Err("Voice runtime pin requires a new measured installer".into());
    }
    let root = kw_storage::paths::sidecar_dir().path;
    let cache = root.join("known-voices-archive.tar.bz2");
    if kw_sidecar::integrity::verify(&cache, asset.size_bytes, &asset.sha256).is_err() {
        let stage = tempfile::Builder::new()
            .prefix(".known-voices-download-")
            .tempdir_in(&root)
            .map_err(|e| e.to_string())?;
        let download = stage.path().join("archive");
        asr.policy
            .download_to_file_bounded(
                kw_network_policy::RequestPurpose::UserTriggeredModelDownload,
                &asset.url,
                &download,
                asset.size_bytes,
                &mut |_, _| {},
            )
            .await
            .map_err(|e| e.to_string())?;
        kw_sidecar::integrity::verify(&download, asset.size_bytes, &asset.sha256)
            .map_err(|e| e.to_string())?;
        if cache.exists() {
            std::fs::remove_file(&cache).map_err(|e| e.to_string())?;
        }
        std::fs::rename(&download, &cache).map_err(|e| e.to_string())?;
    }
    let asset = asset.clone();
    tokio::task::spawn_blocking(move || {
        let _lease = _lease;
        publish(&cache, &asset, &root)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    #[test]
    fn runtime_is_outside_the_transcription_install() {
        assert!(
            !super::directory().starts_with(kw_storage::paths::sidecar_dir().path.join("sherpa"))
        );
    }
    #[cfg(windows)]
    #[test]
    fn leases_exclude_repair_and_release_on_drop() {
        let temp = tempfile::tempdir().unwrap();
        let first = super::lease_in(temp.path()).unwrap();
        assert!(super::lease_in(temp.path()).is_err());
        drop(first);
        assert!(super::lease_in(temp.path()).is_ok());
    }
}

#[cfg(all(test, windows))]
#[allow(clippy::unwrap_used)]
mod archive_tests {
    #[test]
    #[ignore = "requires KW_SHERPA_ARCHIVE pointing at the pinned Windows archive"]
    fn repair_leaves_locked_sherpa_untouched_and_preserves_runtime_on_failure() {
        use std::os::windows::fs::OpenOptionsExt;
        let archive = std::path::PathBuf::from(std::env::var_os("KW_SHERPA_ARCHIVE").unwrap());
        let catalog = kw_sidecar::Catalog::shipped().unwrap();
        let (_, asset) = catalog.resolve("sherpa").unwrap();
        let root = tempfile::tempdir().unwrap();
        let sherpa = root.path().join("sherpa");
        std::fs::create_dir(&sherpa).unwrap();
        let cli = sherpa.join("running.exe");
        std::fs::write(&cli, b"existing CLI").unwrap();
        let locked = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(1)
            .open(&cli)
            .unwrap();
        assert!(std::fs::remove_file(&cli).is_err());
        super::publish(&archive, asset, root.path()).unwrap();
        assert_eq!(std::fs::read(&cli).unwrap(), b"existing CLI");
        let runtime = root.path().join(super::VERSION);
        assert_eq!(std::fs::read_dir(&runtime).unwrap().count(), 4);
        let before =
            crate::voice_worker::hash_file(&runtime.join("sherpa-onnx-c-api.dll")).unwrap();
        let dll = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(1)
            .open(runtime.join("sherpa-onnx-c-api.dll"))
            .unwrap();
        assert!(super::publish(&archive, asset, root.path()).is_err());
        assert_eq!(
            crate::voice_worker::hash_file(&runtime.join("sherpa-onnx-c-api.dll")).unwrap(),
            before
        );
        drop(dll);
        super::publish(&archive, asset, root.path()).unwrap();
        let broken = root.path().join("bad-archive");
        std::fs::write(&broken, b"bad").unwrap();
        assert!(super::publish(&broken, asset, root.path()).is_err());
        assert_eq!(
            crate::voice_worker::hash_file(&runtime.join("sherpa-onnx-c-api.dll")).unwrap(),
            before
        );
        drop(locked);
    }
}
