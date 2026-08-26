//! Downloading, verifying, and unpacking a sidecar.
//!
//! The order matters and is not negotiable: **download to a temporary file,
//! verify the digest, and only then unpack**. Unpacking first and checking
//! afterwards would mean writing attacker-chosen paths to disk before finding
//! out the archive was not the pinned one, and an extraction that has already
//! happened cannot be un-happened by a later error.
//!
//! Extraction pulls out one directory from the archive — the one holding the
//! executable — because `whisper-server` links against sibling `ggml` and
//! `whisper` libraries and does not start without them. Everything outside that
//! directory is ignored: the Windows archive also carries `main.exe`,
//! `SDL2.dll`, a dozen test binaries and a `llama.dll` we have no use for.
//!
//! Every entry name is checked before it becomes a path. An archive is
//! untrusted input even when its digest matched, because the digest only proves
//! it is the file that was pinned — a pin can be made of something malicious if
//! whoever pinned it was wrong. `..`, absolute paths, and drive letters are
//! refused rather than normalised.

use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

use kw_network_policy::{NetworkPolicy, RequestPurpose};

use crate::integrity;
use crate::manifest::{ArchiveKind, Asset, SidecarSpec};

/// Why an install did not finish.
#[derive(Debug, thiserror::Error)]
pub enum InstallError {
    #[error(transparent)]
    Network(#[from] kw_network_policy::PolicyError),

    #[error(transparent)]
    Integrity(#[from] crate::IntegrityError),

    #[error("could not {action} {path}: {source}")]
    Io {
        action: &'static str,
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    #[error("{archive} is not a readable {kind:?} archive: {detail}")]
    Archive {
        archive: PathBuf,
        kind: ArchiveKind,
        detail: String,
    },

    #[error(
        "{name:?} in the archive would write outside the install directory. Refusing to \\
         extract it."
    )]
    UnsafeEntry { name: String },

    #[error(
        "the archive does not contain {expected:?}. Either the pinned asset changed shape or \\
         the manifest names the wrong path inside it."
    )]
    ExecutableMissing { expected: String },
}

/// How far along an install is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Progress {
    /// Bytes written so far, and the total when the server declared one.
    Downloading { written: u64, total: Option<u64> },
    /// Hashing the downloaded file.
    Verifying,
    /// Unpacking it.
    Extracting,
}

/// What an install produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Installed {
    pub executable: PathBuf,
    /// How many files were unpacked. Reported because "1" would mean the
    /// sibling libraries were missed and the server will not start.
    pub files: usize,
    /// Where the bytes actually came from, after redirects.
    pub source_url: String,
}

/// Download, verify, and unpack a sidecar into `root`.
///
/// Replaces any previous install of the same sidecar. Leaves nothing behind on
/// failure except a temporary directory that the next attempt overwrites.
pub async fn install(
    policy: &NetworkPolicy,
    spec: &SidecarSpec,
    asset: &Asset,
    root: &Path,
    // See `NetworkPolicy::download_to_file`: held across awaits, so `Send`.
    progress: &mut (dyn FnMut(Progress) + Send),
) -> Result<Installed, InstallError> {
    let staging = root.join(format!("{}.incoming", spec.name));
    remove_dir(&staging)?;
    create_dir(&staging)?;

    let archive = staging.join("asset");

    let download = policy
        .download_to_file(
            RequestPurpose::UserTriggeredModelDownload,
            &asset.url,
            &archive,
            &mut |written, total| progress(Progress::Downloading { written, total }),
        )
        .await?;

    progress(Progress::Verifying);
    integrity::verify(&archive, asset.size_bytes, &asset.sha256)?;

    progress(Progress::Extracting);
    let unpacked = staging.join("files");
    create_dir(&unpacked)?;
    let files = extract(
        &archive,
        asset.archive,
        asset.payload_dir(),
        asset.executable_name(),
        &unpacked,
    )?;

    let executable = unpacked.join(asset.executable_name());
    if !executable.is_file() {
        return Err(InstallError::ExecutableMissing {
            expected: asset.executable_in_archive.clone(),
        });
    }
    make_executable(&executable)?;

    // Swap into place only now that the install is known good, so a failure
    // never leaves a half-replaced directory where a working one used to be.
    let final_dir = spec.install_dir(root);
    remove_dir(&final_dir)?;
    fs::rename(&unpacked, &final_dir).map_err(|source| InstallError::Io {
        action: "move into place",
        path: final_dir.clone(),
        source,
    })?;
    remove_dir(&staging)?;

    Ok(Installed {
        executable: final_dir.join(asset.executable_name()),
        files,
        source_url: download.final_url,
    })
}

/// Unpack `archive` into `dest`, returning how many files were written.
///
/// For `Zip`, `TarGz` and `TarXz`, `payload_dir` is stripped from each entry
/// name, so `Release/whisper-server.exe` becomes `whisper-server.exe`; an
/// empty `payload_dir` means the whole archive. `executable_name` is unused
/// for those three -- the destination name comes from the entry itself.
///
/// For `Bare` there is no archive to read entries from: `archive` already
/// *is* the executable, `payload_dir` is meaningless, and the file is copied
/// to `dest.join(executable_name)` verbatim.
pub fn extract(
    archive: &Path,
    kind: ArchiveKind,
    payload_dir: &str,
    executable_name: &str,
    dest: &Path,
) -> Result<usize, InstallError> {
    match kind {
        ArchiveKind::Zip => extract_zip(archive, payload_dir, dest),
        ArchiveKind::TarGz => extract_tar_gz(archive, payload_dir, dest),
        ArchiveKind::TarXz => extract_tar_xz(archive, payload_dir, dest),
        ArchiveKind::Bare => extract_bare(archive, executable_name, dest),
    }
}

fn extract_zip(archive: &Path, payload_dir: &str, dest: &Path) -> Result<usize, InstallError> {
    let file = fs::File::open(archive).map_err(|source| InstallError::Io {
        action: "open",
        path: archive.to_path_buf(),
        source,
    })?;
    let mut zip = zip::ZipArchive::new(file).map_err(|err| InstallError::Archive {
        archive: archive.to_path_buf(),
        kind: ArchiveKind::Zip,
        detail: err.to_string(),
    })?;

    let mut written = 0_usize;
    for index in 0..zip.len() {
        let mut entry = zip.by_index(index).map_err(|err| InstallError::Archive {
            archive: archive.to_path_buf(),
            kind: ArchiveKind::Zip,
            detail: err.to_string(),
        })?;

        if entry.is_dir() {
            continue;
        }

        let Some(relative) = payload_relative(entry.name(), payload_dir)? else {
            continue;
        };

        let target = dest.join(&relative);
        write_entry(&target, &mut entry)?;
        written += 1;
    }

    Ok(written)
}

fn extract_tar_gz(archive: &Path, payload_dir: &str, dest: &Path) -> Result<usize, InstallError> {
    let file = fs::File::open(archive).map_err(|source| InstallError::Io {
        action: "open",
        path: archive.to_path_buf(),
        source,
    })?;
    extract_tar(
        flate2::read::GzDecoder::new(file),
        ArchiveKind::TarGz,
        archive,
        payload_dir,
        dest,
    )
}

/// Decompress a `.tar.xz` with `lzma-rs` (pure Rust, decode-only -- see the
/// `lzma-rs` comment in the root `Cargo.toml`), then read the result as a tar
/// the same way `extract_tar_gz` does.
///
/// `lzma-rs` decompresses into a `Write` rather than exposing a streaming
/// `Read`, so the whole tar is held in memory instead of being streamed
/// entry-by-entry the way the gzip path is. ffmpeg's tar.xz is the only
/// pinned use of this today and is small enough that a hand-rolled streaming
/// adapter is not worth building.
fn extract_tar_xz(archive: &Path, payload_dir: &str, dest: &Path) -> Result<usize, InstallError> {
    let file = fs::File::open(archive).map_err(|source| InstallError::Io {
        action: "open",
        path: archive.to_path_buf(),
        source,
    })?;
    let mut reader = io::BufReader::new(file);
    let mut decompressed = Vec::new();
    lzma_rs::xz_decompress(&mut reader, &mut decompressed).map_err(|err| {
        InstallError::Archive {
            archive: archive.to_path_buf(),
            kind: ArchiveKind::TarXz,
            detail: err.to_string(),
        }
    })?;

    extract_tar(
        io::Cursor::new(decompressed),
        ArchiveKind::TarXz,
        archive,
        payload_dir,
        dest,
    )
}

/// The part of `extract_tar_gz` and `extract_tar_xz` that does not care how
/// `reader` was decompressed: walk the tar, skip non-files and entries outside
/// `payload_dir`, and write the rest.
fn extract_tar<R: io::Read>(
    reader: R,
    kind: ArchiveKind,
    archive: &Path,
    payload_dir: &str,
    dest: &Path,
) -> Result<usize, InstallError> {
    let mut tar = tar::Archive::new(reader);

    let entries = tar.entries().map_err(|err| InstallError::Archive {
        archive: archive.to_path_buf(),
        kind,
        detail: err.to_string(),
    })?;

    let mut written = 0_usize;
    for entry in entries {
        let mut entry = entry.map_err(|err| InstallError::Archive {
            archive: archive.to_path_buf(),
            kind,
            detail: err.to_string(),
        })?;

        if !entry.header().entry_type().is_file() {
            continue;
        }

        let name = entry
            .path()
            .map_err(|err| InstallError::Archive {
                archive: archive.to_path_buf(),
                kind,
                detail: err.to_string(),
            })?
            .to_string_lossy()
            .into_owned();

        let Some(relative) = payload_relative(&name, payload_dir)? else {
            continue;
        };

        let target = dest.join(&relative);
        write_entry(&target, &mut entry)?;
        written += 1;
    }

    Ok(written)
}

/// `Bare`: the downloaded file *is* the executable. Copied verbatim to
/// `dest.join(executable_name)` -- no entries to walk, no payload directory to
/// strip.
///
/// `executable_name` is [`Asset::executable_name`], which is always the
/// substring after the last path separator (or the whole string, if there is
/// none), so it can never itself contain a separator. `dest.join` cannot be
/// walked outside `dest` by it.
fn extract_bare(archive: &Path, executable_name: &str, dest: &Path) -> Result<usize, InstallError> {
    let target = dest.join(executable_name);
    fs::copy(archive, &target).map_err(|source| InstallError::Io {
        action: "copy",
        path: target,
        source,
    })?;
    Ok(1)
}

/// An archive entry's path relative to the payload directory, or `None` if it
/// is outside it.
///
/// This is where archive names stop being strings and become paths, so it is
/// where they get checked.
fn payload_relative(name: &str, payload_dir: &str) -> Result<Option<PathBuf>, InstallError> {
    let normalised = name.replace('\\', "/");

    let stripped = if payload_dir.is_empty() {
        normalised.as_str()
    } else {
        let prefix = format!("{}/", payload_dir.replace('\\', "/"));
        match normalised.strip_prefix(&prefix) {
            Some(rest) => rest,
            // Outside the directory we want. Not an error -- the Windows
            // archive legitimately carries binaries we have no use for.
            None => return Ok(None),
        }
    };

    if stripped.is_empty() {
        return Ok(None);
    }

    let candidate = PathBuf::from(stripped);
    let safe = candidate
        .components()
        .all(|component| matches!(component, Component::Normal(_) | Component::CurDir));
    if !safe {
        // `..`, a leading `/`, or `C:` -- the zip-slip family. Refused rather
        // than normalised, because a normalised traversal is still someone
        // trying to write outside the directory.
        return Err(InstallError::UnsafeEntry {
            name: name.to_owned(),
        });
    }

    Ok(Some(candidate))
}

fn write_entry<R: io::Read>(target: &Path, source: &mut R) -> Result<(), InstallError> {
    if let Some(parent) = target.parent() {
        create_dir(parent)?;
    }
    let mut out = fs::File::create(target).map_err(|source| InstallError::Io {
        action: "create",
        path: target.to_path_buf(),
        source,
    })?;
    io::copy(source, &mut out).map_err(|source| InstallError::Io {
        action: "write",
        path: target.to_path_buf(),
        source,
    })?;
    Ok(())
}

fn create_dir(path: &Path) -> Result<(), InstallError> {
    fs::create_dir_all(path).map_err(|source| InstallError::Io {
        action: "create",
        path: path.to_path_buf(),
        source,
    })
}

fn remove_dir(path: &Path) -> Result<(), InstallError> {
    match fs::remove_dir_all(path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(InstallError::Io {
            action: "remove",
            path: path.to_path_buf(),
            source,
        }),
    }
}

/// Set the executable bit on Unix. A no-op on Windows, where it is the
/// extension that decides.
///
/// Zip archives do carry Unix permission bits, but the ones published here do
/// not set them reliably, and a downloaded server that cannot be executed is a
/// confusing failure. Set explicitly rather than inherited.
fn make_executable(path: &Path) -> Result<(), InstallError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).map_err(|source| {
            InstallError::Io {
                action: "make executable",
                path: path.to_path_buf(),
                source,
            }
        })
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        Ok(())
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
    use std::io::Write as _;

    use super::*;

    /// Build a zip in memory from (name, contents) pairs.
    fn zip_with(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut buffer = io::Cursor::new(Vec::new());
        {
            let mut writer = zip::ZipWriter::new(&mut buffer);
            let options: zip::write::FileOptions<'_, ()> = zip::write::FileOptions::default();
            for (name, contents) in entries {
                writer.start_file(*name, options).unwrap();
                writer.write_all(contents).unwrap();
            }
            writer.finish().unwrap();
        }
        buffer.into_inner()
    }

    fn tar_gz_with(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        {
            let mut builder = tar::Builder::new(&mut gz);
            for (name, contents) in entries {
                let mut header = tar::Header::new_gnu();
                header.set_size(contents.len() as u64);
                header.set_mode(0o644);
                header.set_cksum();
                builder.append_data(&mut header, name, *contents).unwrap();
            }
            builder.finish().unwrap();
        }
        gz.finish().unwrap()
    }

    /// Build a `.tar.xz` in memory from (name, contents) pairs, using
    /// `lzma-rs` for both the tar step (via the same `tar` crate the other
    /// helpers use) and the xz step. This proves the round trip through this
    /// crate's own code; `tests/tar_xz_fixture.rs` separately proves
    /// `lzma_rs::xz_decompress` against a stream this crate never produced.
    fn tar_xz_with(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut tar_bytes = Vec::new();
        {
            let mut builder = tar::Builder::new(&mut tar_bytes);
            for (name, contents) in entries {
                let mut header = tar::Header::new_gnu();
                header.set_size(contents.len() as u64);
                header.set_mode(0o644);
                header.set_cksum();
                builder.append_data(&mut header, name, *contents).unwrap();
            }
            builder.finish().unwrap();
        }

        let mut compressed = Vec::new();
        lzma_rs::xz_compress(&mut io::Cursor::new(tar_bytes), &mut compressed).unwrap();
        compressed
    }

    fn write_archive(dir: &Path, bytes: &[u8]) -> PathBuf {
        let path = dir.join("asset");
        fs::write(&path, bytes).unwrap();
        path
    }

    // ------------------------------------------------------------------ zip

    #[test]
    fn a_payload_directory_is_extracted_flattened() {
        // The real Windows layout: everything under Release/, and the server
        // needs its sibling libraries or it will not start.
        let dir = tempfile::tempdir().unwrap();
        let archive = write_archive(
            dir.path(),
            &zip_with(&[
                ("Release/whisper-server.exe", b"server"),
                ("Release/whisper.dll", b"lib"),
                ("Release/ggml-base.dll", b"lib"),
            ]),
        );
        let dest = dir.path().join("out");
        fs::create_dir_all(&dest).unwrap();

        // "" for executable_name: unused for Zip -- see the doc comment on `extract`.
        let count = extract(&archive, ArchiveKind::Zip, "Release", "", &dest).unwrap();
        assert_eq!(count, 3);
        assert_eq!(
            fs::read(dest.join("whisper-server.exe")).unwrap(),
            b"server"
        );
        assert!(dest.join("whisper.dll").is_file());
        assert!(dest.join("ggml-base.dll").is_file());
        assert!(!dest.join("Release").exists(), "the prefix is stripped");
    }

    #[test]
    fn entries_outside_the_payload_directory_are_skipped() {
        // The published archive carries main.exe, SDL2.dll, llama.dll and a
        // dozen test binaries we have no use for.
        let dir = tempfile::tempdir().unwrap();
        let archive = write_archive(
            dir.path(),
            &zip_with(&[
                ("Release/whisper-server.exe", b"server"),
                ("Debug/whisper-server.exe", b"wrong build"),
                ("README.md", b"docs"),
            ]),
        );
        let dest = dir.path().join("out");
        fs::create_dir_all(&dest).unwrap();

        assert_eq!(
            extract(&archive, ArchiveKind::Zip, "Release", "", &dest).unwrap(),
            1
        );
        assert_eq!(
            fs::read(dest.join("whisper-server.exe")).unwrap(),
            b"server"
        );
        assert!(!dest.join("README.md").exists());
    }

    #[test]
    fn an_empty_payload_directory_takes_the_whole_archive() {
        let dir = tempfile::tempdir().unwrap();
        let archive = write_archive(
            dir.path(),
            &zip_with(&[("whisper-server", b"a"), ("libwhisper.so", b"b")]),
        );
        let dest = dir.path().join("out");
        fs::create_dir_all(&dest).unwrap();
        assert_eq!(
            extract(&archive, ArchiveKind::Zip, "", "", &dest).unwrap(),
            2
        );
    }

    #[test]
    fn nested_structure_inside_the_payload_is_preserved() {
        let dir = tempfile::tempdir().unwrap();
        let archive = write_archive(
            dir.path(),
            &zip_with(&[("Release/models/vad.onnx", b"model")]),
        );
        let dest = dir.path().join("out");
        fs::create_dir_all(&dest).unwrap();

        assert_eq!(
            extract(&archive, ArchiveKind::Zip, "Release", "", &dest).unwrap(),
            1
        );
        assert!(dest.join("models").join("vad.onnx").is_file());
    }

    // --------------------------------------------------------------- tar.gz

    #[test]
    fn a_tar_gz_payload_directory_is_extracted_flattened() {
        // The real Linux layout.
        let dir = tempfile::tempdir().unwrap();
        let archive = write_archive(
            dir.path(),
            &tar_gz_with(&[
                ("whisper-bin-ubuntu-x64/whisper-server", b"server"),
                ("whisper-bin-ubuntu-x64/libwhisper.so", b"lib"),
                ("whisper-bin-ubuntu-x64/main", b"unused but inside"),
            ]),
        );
        let dest = dir.path().join("out");
        fs::create_dir_all(&dest).unwrap();

        let count = extract(
            &archive,
            ArchiveKind::TarGz,
            "whisper-bin-ubuntu-x64",
            "",
            &dest,
        )
        .unwrap();
        assert_eq!(count, 3);
        assert_eq!(fs::read(dest.join("whisper-server")).unwrap(), b"server");
        assert!(dest.join("libwhisper.so").is_file());
    }

    #[test]
    fn a_zip_read_as_a_tar_is_a_clear_archive_error() {
        let dir = tempfile::tempdir().unwrap();
        let archive = write_archive(dir.path(), &zip_with(&[("a/b", b"c")]));
        let dest = dir.path().join("out");
        fs::create_dir_all(&dest).unwrap();

        let err = extract(&archive, ArchiveKind::TarGz, "a", "", &dest).unwrap_err();
        assert!(matches!(err, InstallError::Archive { .. }), "{err}");
    }

    // --------------------------------------------------------------- tar.xz

    #[test]
    fn a_tar_xz_payload_directory_is_extracted_flattened() {
        // The real Linux ffmpeg layout: a payload directory holding the
        // executable and its sibling shared libraries.
        let dir = tempfile::tempdir().unwrap();
        let archive = write_archive(
            dir.path(),
            &tar_xz_with(&[
                ("ffmpeg-bin/ffmpeg", b"server"),
                ("ffmpeg-bin/libavcodec.so", b"lib"),
                ("ffmpeg-bin/ffprobe", b"unused but inside"),
            ]),
        );
        let dest = dir.path().join("out");
        fs::create_dir_all(&dest).unwrap();

        let count = extract(&archive, ArchiveKind::TarXz, "ffmpeg-bin", "", &dest).unwrap();
        assert_eq!(count, 3);
        assert_eq!(fs::read(dest.join("ffmpeg")).unwrap(), b"server");
        assert!(dest.join("libavcodec.so").is_file());
        assert!(!dest.join("ffmpeg-bin").exists(), "the prefix is stripped");
    }

    #[test]
    fn a_non_xz_file_produces_a_clear_archive_error() {
        let dir = tempfile::tempdir().unwrap();
        let archive = write_archive(dir.path(), b"not an xz stream at all");
        let dest = dir.path().join("out");
        fs::create_dir_all(&dest).unwrap();

        let err = extract(&archive, ArchiveKind::TarXz, "", "", &dest).unwrap_err();
        assert!(matches!(err, InstallError::Archive { .. }), "{err}");
    }

    // ----------------------------------------------------------------- bare

    #[test]
    fn a_bare_asset_is_copied_verbatim_to_the_executable_name() {
        // yt-dlp's shape: no archive at all, so the digest already covers the
        // exact bytes that will run -- see the comment on `ArchiveKind::Bare`.
        let dir = tempfile::tempdir().unwrap();
        let archive = write_archive(dir.path(), b"#!/bin/sh\necho yt-dlp\n");
        let dest = dir.path().join("out");
        fs::create_dir_all(&dest).unwrap();

        let count = extract(&archive, ArchiveKind::Bare, "", "yt-dlp", &dest).unwrap();
        assert_eq!(count, 1);
        assert_eq!(
            fs::read(dest.join("yt-dlp")).unwrap(),
            b"#!/bin/sh\necho yt-dlp\n"
        );
    }

    #[test]
    #[cfg(unix)]
    fn a_bare_install_ends_up_executable_on_unix() {
        // The end-to-end path through `install()`, not just `extract()`: this
        // is what actually flips the executable bit, and it only exists on
        // Unix. Unverified on this development machine, which is Windows --
        // see the report for that caveat.
        use std::os::unix::fs::PermissionsExt as _;

        let dir = tempfile::tempdir().unwrap();
        let archive = write_archive(dir.path(), b"#!/bin/sh\necho yt-dlp\n");
        let dest = dir.path().join("out");
        fs::create_dir_all(&dest).unwrap();

        extract(&archive, ArchiveKind::Bare, "", "yt-dlp", &dest).unwrap();
        let extracted = dest.join("yt-dlp");
        assert!(extracted.is_file());

        // `extract` alone does not set the bit -- `install()` does, via
        // `make_executable`, after the digest has already been checked. This
        // asserts the bit-setting helper directly against the extracted file.
        make_executable(&extracted).unwrap();
        let mode = fs::metadata(&extracted).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o755);
    }

    // ------------------------------------------------------------- zip slip

    #[test]
    fn a_traversing_entry_is_refused_rather_than_normalised() {
        // The zip-slip family. A digest match proves the archive is the pinned
        // one, not that the pinned one is harmless.
        for name in [
            "Release/../../../../etc/cron.d/backdoor",
            "Release/../../autostart.desktop",
        ] {
            let dir = tempfile::tempdir().unwrap();
            let archive = write_archive(dir.path(), &zip_with(&[(name, b"payload")]));
            let dest = dir.path().join("out");
            fs::create_dir_all(&dest).unwrap();

            let err = extract(&archive, ArchiveKind::Zip, "Release", "", &dest).unwrap_err();
            assert!(
                matches!(err, InstallError::UnsafeEntry { .. }),
                "{name} produced {err}"
            );
        }
    }

    #[test]
    fn an_absolute_entry_is_refused() {
        // Asserted against payload_relative rather than a real archive, because
        // the `tar` crate refuses to *write* an absolute path -- there is no way
        // to build the malicious fixture with its safe API. The zip traversal
        // test above does go through a real archive, so the end-to-end path is
        // covered; this covers the shape tar cannot express.
        for name in ["/etc/passwd", "/usr/local/bin/kw"] {
            assert!(
                matches!(
                    payload_relative(name, ""),
                    Err(InstallError::UnsafeEntry { .. })
                ),
                "{name} must be refused"
            );
        }
    }

    #[test]
    fn a_backslash_separated_entry_is_checked_too() {
        // A zip written on Windows can use backslashes, which would otherwise
        // slip past a check that only understands forward slashes.
        let refused = payload_relative("Release\\..\\..\\evil", "Release");
        assert!(matches!(refused, Err(InstallError::UnsafeEntry { .. })));
    }

    #[test]
    fn a_windows_drive_letter_is_refused() {
        let refused = payload_relative("C:/Windows/System32/evil.dll", "");
        assert!(
            refused.is_err(),
            "a drive-qualified path is not relative to anything"
        );
    }

    #[test]
    fn the_payload_directory_entry_itself_is_not_a_file() {
        assert_eq!(payload_relative("Release/", "Release").unwrap(), None);
        assert_eq!(payload_relative("Release", "Release").unwrap(), None);
    }

    #[test]
    fn a_directory_that_merely_starts_with_the_payload_name_is_not_inside_it() {
        // "Release2/x" must not be treated as inside "Release".
        assert_eq!(payload_relative("Release2/x", "Release").unwrap(), None);
    }
}
