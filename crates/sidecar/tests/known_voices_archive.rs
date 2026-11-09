#![allow(clippy::unwrap_used)]
#[test]
#[ignore = "requires KW_SHERPA_ARCHIVE pointing at the pinned Windows archive"]
fn pinned_archive_installs_the_voice_runtime_within_its_measured_budget() {
    use kw_sidecar::{extract_with_limit, integrity, ArchiveKind, Sha256Digest};
    let path = std::path::PathBuf::from(std::env::var_os("KW_SHERPA_ARCHIVE").unwrap());
    integrity::verify(
        &path,
        24378225,
        &Sha256Digest::parse("c312b30e55258d291067537ce4a6f90e155f1c16b6a10381a5729924e98b7879")
            .unwrap(),
    )
    .unwrap();
    let dir = tempfile::tempdir().unwrap();
    extract_with_limit(
        &path,
        ArchiveKind::TarBz2,
        "sherpa-onnx-v1.13.4-win-x64-shared-MT-Release/lib",
        "",
        dir.path(),
        22013440,
    )
    .unwrap();
    for file in [
        "sherpa-onnx-c-api.dll",
        "onnxruntime.dll",
        "onnxruntime_providers_shared.dll",
    ] {
        assert!(dir.path().join(file).is_file());
    }
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 3);
    let cli = tempfile::tempdir().unwrap();
    extract_with_limit(
        &path,
        ArchiveKind::TarBz2,
        "sherpa-onnx-v1.13.4-win-x64-shared-MT-Release/bin",
        "",
        cli.path(),
        55574528,
    )
    .unwrap();
    assert!(cli
        .path()
        .join("sherpa-onnx-offline-speaker-diarization.exe")
        .is_file());
    assert!(!cli.path().join("voice-runtime").exists());
}
