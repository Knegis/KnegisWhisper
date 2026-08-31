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
use crate::manifest::{is_plain_file_name, ArchiveKind, Asset, SidecarSpec};

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

    #[error("{name:?} would write outside the install directory. Refusing to extract it.")]
    UnsafeEntry { name: String },

    /// The archive extracts to more than the ceiling it was pinned to.
    ///
    /// Typed rather than folded into [`Self::Archive`], which is what this used
    /// to be: `Archive { detail: "extracted data exceeds the N-byte limit" }`.
    /// A caller could not tell "too large" from "corrupt", and
    /// `tests/tar_bz2_fixture.rs` had to assert on the substring because there
    /// was nothing better to match on. "This asset is bigger than we agreed"
    /// and "this asset is not the format it claims" are different facts and a
    /// caller may reasonably want to treat them differently.
    #[error(
        "{archive} extracts to more than the {limit} bytes this asset is pinned to. Refusing \\
         to unpack it, and removing what was already written."
    )]
    ExtractedTooLarge {
        archive: PathBuf,
        kind: ArchiveKind,
        limit: u64,
    },

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
/// For `Zip`, `TarGz`, `TarXz` and `TarBz2`, `payload_dir` is stripped from
/// each entry name, so `Release/whisper-server.exe` becomes
/// `whisper-server.exe`; an empty `payload_dir` means the whole archive.
/// `executable_name` is unused for those four -- the destination name comes
/// from the entry itself.
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
    extract_with_limit(archive, kind, payload_dir, executable_name, dest, u64::MAX)
}

/// Unpack an archive while refusing to write more than `max_bytes` in total,
/// and removing whatever it had already written if it refuses.
///
/// **Every [`ArchiveKind`] is bounded**, including `Bare`, where the download
/// *is* the payload and its own length on disk is the extracted size. This
/// used to thread `max_bytes` to `TarBz2` alone and discard it silently for
/// the other four while the doc comment promised otherwise — see KNEG-102.
/// Nothing was exposed by it, because the only caller passing a limit
/// installs `.tar.bz2` models, but that was luck rather than design.
///
/// **What the ceiling is measured against.** Two checks, not one, and they
/// answer different failure modes:
///
/// - The size the archive *declares* for an entry, checked before that entry
///   is written, so an oversized entry never lands at all. For a tar this is
///   the whole story: `tar::Entry`'s reader is an [`io::Take`] over the header
///   size, so the bytes cannot exceed what the header said.
/// - The bytes that actually arrive. A zip entry's reader is a decompressor
///   over `take(compressed_size)` — read from `zip-2.4.2/src/read.rs`, not
///   assumed — and nothing in it bounds the *inflated* output to the
///   `uncompressed_size` the central directory declares. A deflate stream that
///   understates its own output would sail past a header-only check, which is
///   precisely the decompression bomb this ceiling exists to refuse.
///
/// At most one byte past `max_bytes` reaches disk before the refusal (the
/// budget reads one byte more than it allows, so that a full budget can be
/// told from an entry that ends exactly on it), and the rollback removes the
/// file regardless.
pub fn extract_with_limit(
    archive: &Path,
    kind: ArchiveKind,
    payload_dir: &str,
    executable_name: &str,
    dest: &Path,
    max_bytes: u64,
) -> Result<usize, InstallError> {
    let mut budget = Budget::new(archive, kind, max_bytes);
    let outcome = match kind {
        ArchiveKind::Zip => extract_zip(archive, payload_dir, dest, &mut budget),
        ArchiveKind::TarGz => extract_tar_gz(archive, payload_dir, dest, &mut budget),
        ArchiveKind::TarXz => extract_tar_xz(archive, payload_dir, dest, &mut budget),
        ArchiveKind::TarBz2 => extract_tar_bz2(archive, payload_dir, dest, &mut budget),
        ArchiveKind::Bare => extract_bare(archive, executable_name, dest, &mut budget),
    };

    match outcome {
        Ok(count) => Ok(count),
        Err(err) => {
            // Every error, not only a refused ceiling: `extract_tar` and
            // `extract_zip` used to leave every file they had already written
            // behind on a corrupt entry or an unsafe name too, and a
            // half-unpacked directory can look convincing to a later
            // `locate`.
            budget.rollback();
            Err(err)
        }
    }
}

/// What one extraction may still write, and what it has already written.
///
/// Carries the rollback list as well as the ceiling because the two belong to
/// the same question: a refusal is only honest if what was written before it
/// goes away.
struct Budget<'a> {
    archive: &'a Path,
    kind: ArchiveKind,
    limit: u64,
    spent: u64,
    /// Files this extraction created, in the order it created them.
    files: Vec<PathBuf>,
    /// Directories this extraction created — never `dest` itself. See
    /// [`Budget::create_dir`].
    dirs: Vec<PathBuf>,
}

impl<'a> Budget<'a> {
    fn new(archive: &'a Path, kind: ArchiveKind, limit: u64) -> Self {
        Self {
            archive,
            kind,
            limit,
            spent: 0,
            files: Vec::new(),
            dirs: Vec::new(),
        }
    }

    /// How much more may be written.
    fn remaining(&self) -> u64 {
        self.limit.saturating_sub(self.spent)
    }

    /// Refuse now if an entry of `size` would not fit.
    ///
    /// `checked_add` rather than a plain sum: a header declaring a size near
    /// `u64::MAX` must refuse rather than wrap into something that fits.
    fn declare(&self, size: u64) -> Result<(), InstallError> {
        match self.spent.checked_add(size) {
            Some(total) if total <= self.limit => Ok(()),
            _ => Err(self.too_large()),
        }
    }

    fn too_large(&self) -> InstallError {
        InstallError::ExtractedTooLarge {
            archive: self.archive.to_path_buf(),
            kind: self.kind,
            limit: self.limit,
        }
    }

    /// `create_dir_all`, remembering which directories did not exist yet.
    ///
    /// `dest` is deliberately never recorded, and the walk stops there. It
    /// belongs to the caller, and for `sherpa_models::install` it is the
    /// models *root* — removing it on a refused extraction would take every
    /// other installed model with it.
    fn create_dir(&mut self, path: &Path, dest: &Path) -> Result<(), InstallError> {
        let mut missing = Vec::new();
        let mut cursor = Some(path);
        while let Some(dir) = cursor {
            if dir == dest || dir.exists() {
                break;
            }
            missing.push(dir.to_path_buf());
            cursor = dir.parent();
        }
        create_dir(path)?;
        self.dirs.append(&mut missing);
        Ok(())
    }

    /// Undo what this extraction wrote.
    ///
    /// Best-effort by design: a removal that fails must not replace the error
    /// that caused the rollback, which is the one the caller needs to see.
    fn rollback(&mut self) {
        for file in self.files.drain(..) {
            let _ = fs::remove_file(file);
        }
        // Deepest first, so a nested directory is gone before its parent is
        // tried, and `remove_dir` rather than `remove_dir_all`: a directory
        // that still holds something holds something this extraction did not
        // write, and that is not ours to delete.
        self.dirs
            .sort_by_key(|dir| std::cmp::Reverse(dir.components().count()));
        for dir in self.dirs.drain(..) {
            let _ = fs::remove_dir(dir);
        }
    }
}

fn extract_zip(
    archive: &Path,
    payload_dir: &str,
    dest: &Path,
    budget: &mut Budget<'_>,
) -> Result<usize, InstallError> {
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

        // The declared uncompressed size, refused before anything is written.
        // Not sufficient on its own here -- see `extract_with_limit` on what
        // the zip crate does and does not bound -- which is why `write_entry`
        // also holds the bytes that actually arrive to the budget.
        budget.declare(entry.size())?;

        let target = dest.join(&relative);
        write_entry(&target, &mut entry, dest, budget)?;
        written += 1;
    }

    Ok(written)
}

fn extract_tar_gz(
    archive: &Path,
    payload_dir: &str,
    dest: &Path,
    budget: &mut Budget<'_>,
) -> Result<usize, InstallError> {
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
        budget,
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
///
/// **The `max_bytes` ceiling does not cover this buffer.** It bounds what is
/// written to `dest`, and the `Vec` below is filled before a single entry is
/// looked at — so a `.tar.xz` remains a memory bomb whatever ceiling is
/// passed, independently of the output ceiling this function now honours.
/// Fixing that means a streaming LZMA2 reader; tracked as KNEG-106.
fn extract_tar_xz(
    archive: &Path,
    payload_dir: &str,
    dest: &Path,
    budget: &mut Budget<'_>,
) -> Result<usize, InstallError> {
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
        budget,
    )
}

/// Decompress a `.tar.bz2` with `bzip2-rs` (pure Rust, decode-only -- same
/// reasoning as `lzma-rs` for `TarXz`, see the `bzip2-rs` comment in the root
/// `Cargo.toml`), then read the result as a tar the same way `extract_tar_gz`
/// does.
///
/// Unlike `lzma-rs::xz_decompress`, `bzip2_rs::DecoderReader` implements
/// `Read` directly over the underlying compressed reader, so this streams
/// through `tar::Archive` entry-by-entry rather than buffering the whole
/// decompressed tar in memory first. sherpa-onnx's binary bundles are the
/// pinned use of this and are large enough (tens of megabytes) that the
/// difference matters.
fn extract_tar_bz2(
    archive: &Path,
    payload_dir: &str,
    dest: &Path,
    budget: &mut Budget<'_>,
) -> Result<usize, InstallError> {
    let file = fs::File::open(archive).map_err(|source| InstallError::Io {
        action: "open",
        path: archive.to_path_buf(),
        source,
    })?;
    extract_tar(
        bzip2_rs::DecoderReader::new(file),
        ArchiveKind::TarBz2,
        archive,
        payload_dir,
        dest,
        budget,
    )
}

/// The part of `extract_tar_gz`, `extract_tar_xz` and `extract_tar_bz2` that
/// does not care how `reader` was decompressed: walk the tar, skip non-files
/// and entries outside `payload_dir`, and write the rest within `budget`.
///
/// There used to be two of these — this one and an `extract_tar_limited` that
/// differed only in taking a ceiling — and only `.tar.bz2` reached the limited
/// one. One walker, always bounded, is the fix.
fn extract_tar<R: io::Read>(
    reader: R,
    kind: ArchiveKind,
    archive: &Path,
    payload_dir: &str,
    dest: &Path,
    budget: &mut Budget<'_>,
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

        // Checked before the entry is written, so an oversized one never
        // lands. Exact for a tar: `tar::Entry`'s reader is an `io::Take` over
        // this same header size, so the bytes cannot disagree with it.
        budget.declare(entry.size())?;

        let target = dest.join(&relative);
        write_entry(&target, &mut entry, dest, budget)?;
        written += 1;
    }

    Ok(written)
}

/// `Bare`: the downloaded file *is* the executable. Copied verbatim to
/// `dest.join(executable_name)` -- no entries to walk, no payload directory to
/// strip.
///
/// `executable_name` is [`Asset::executable_name`], which is always the
/// substring after the last path separator -- *or the whole string, if there
/// is none*. That second case is why this cannot just trust it: an
/// `executableInArchive` of exactly `".."`, with no separator at all, comes
/// back from `executable_name` unchanged, and `dest.join("..")` is the
/// staging directory's parent, not a path inside it. `is_plain_file_name`
/// refuses that shape (and `.`, and an empty name) before it ever reaches
/// `dest.join`.
///
/// `SidecarSpec::problems` runs the same check at manifest-read time, so this
/// is the defence for a value that got past that check some other way (a
/// spec built by hand in a test, for instance), not the only one.
fn extract_bare(
    archive: &Path,
    executable_name: &str,
    dest: &Path,
    budget: &mut Budget<'_>,
) -> Result<usize, InstallError> {
    if !is_plain_file_name(executable_name) {
        return Err(InstallError::UnsafeEntry {
            name: executable_name.to_owned(),
        });
    }

    // The download *is* the payload, so the extracted size is the file's own
    // length -- measured off the file rather than declared by a header, which
    // makes this the one kind where the ceiling cannot be lied to. A `Bare`
    // asset over its ceiling means the pin and the manifest disagree, not
    // that anything decompressed unexpectedly.
    let size = fs::metadata(archive)
        .map_err(|source| InstallError::Io {
            action: "measure",
            path: archive.to_path_buf(),
            source,
        })?
        .len();
    budget.declare(size)?;

    let target = dest.join(executable_name);
    // Recorded before the copy, not after: a `fs::copy` that fails part way
    // through still leaves a partial file to clean up.
    budget.files.push(target.clone());
    fs::copy(archive, &target).map_err(|source| InstallError::Io {
        action: "copy",
        path: target,
        source,
    })?;
    budget.spent = budget.spent.saturating_add(size);
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

/// Write one archive entry, within `budget` and recorded for its rollback.
///
/// The copy is bounded on the bytes that actually arrive rather than on the
/// size the entry declared, because for a zip the two can differ — see
/// [`extract_with_limit`]. One byte more than the budget allows is read, so
/// that an entry ending exactly on the ceiling can be told from one that runs
/// past it; that byte is written and then removed with the file.
fn write_entry<R: io::Read>(
    target: &Path,
    source: &mut R,
    dest: &Path,
    budget: &mut Budget<'_>,
) -> Result<(), InstallError> {
    if let Some(parent) = target.parent() {
        budget.create_dir(parent, dest)?;
    }
    let mut out = fs::File::create(target).map_err(|source| InstallError::Io {
        action: "create",
        path: target.to_path_buf(),
        source,
    })?;
    budget.files.push(target.to_path_buf());

    let allowance = budget.remaining();
    let mut limited = io::Read::take(&mut *source, allowance.saturating_add(1));
    let written = io::copy(&mut limited, &mut out).map_err(|source| InstallError::Io {
        action: "write",
        path: target.to_path_buf(),
        source,
    })?;
    if written > allowance {
        return Err(budget.too_large());
    }
    budget.spent = budget.spent.saturating_add(written);
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

    /// Rewrite a **single-entry** zip's declared uncompressed size, in both
    /// the local file header and the central directory, so a test can present
    /// an archive whose header understates what its deflate stream actually
    /// produces. The `zip` crate's writer cannot be asked to lie.
    ///
    /// Offsets are walked rather than scanned for: the end-of-central-directory
    /// record gives the central directory's offset, and a one-entry zip puts
    /// its local header at 0. Scanning for `PK\x01\x02` would risk matching
    /// those bytes inside the compressed data.
    fn understate_zip_size(mut bytes: Vec<u8>, declared: u32) -> Vec<u8> {
        let eocd = (0..=bytes.len() - 22)
            .rev()
            .find(|&i| bytes[i..i + 4] == [0x50, 0x4b, 0x05, 0x06])
            .expect("a zip ends with an end-of-central-directory record");
        assert_eq!(
            u16::from_le_bytes([bytes[eocd + 10], bytes[eocd + 11]]),
            1,
            "this helper only handles a one-entry zip"
        );
        let directory = u32::from_le_bytes([
            bytes[eocd + 16],
            bytes[eocd + 17],
            bytes[eocd + 18],
            bytes[eocd + 19],
        ]) as usize;

        // Local file header: uncompressed size at +22. Central directory
        // record: uncompressed size at +24.
        bytes[22..26].copy_from_slice(&declared.to_le_bytes());
        bytes[directory + 24..directory + 28].copy_from_slice(&declared.to_le_bytes());
        bytes
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

    // -------------------------------------------------------------- tar.bz2
    //
    // `bzip2-rs` is decode-only, so unlike the zip/tar.gz/tar.xz sections
    // above there is no in-memory encoder to build a round-trip fixture with.
    // The extraction-and-flattening behaviour for a real `.tar.bz2` stream is
    // proved instead by `tests/tar_bz2_fixture.rs`, against a fixture built
    // with the standalone `bzip2` command-line tool. This is the one case
    // that does not need real bytes: any non-bzip2 input, however produced,
    // must fail the same way.

    #[test]
    fn a_non_bz2_file_produces_a_clear_archive_error() {
        let dir = tempfile::tempdir().unwrap();
        let archive = write_archive(dir.path(), b"not a bzip2 stream at all");
        let dest = dir.path().join("out");
        fs::create_dir_all(&dest).unwrap();

        let err = extract(&archive, ArchiveKind::TarBz2, "", "", &dest).unwrap_err();
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
    fn a_bare_executable_name_of_dotdot_is_refused() {
        // `Asset::executable_name` returns the whole string when it has no
        // separator, so an `executableInArchive` of exactly ".." would
        // otherwise make `extract_bare` copy the download to the staging
        // directory's *parent* -- caught here rather than actually doing it.
        // `SidecarSpec::problems` refuses the same shape at manifest-read
        // time; this is `extract_bare`'s own defence underneath that.
        let dir = tempfile::tempdir().unwrap();
        let archive = write_archive(dir.path(), b"payload");
        let dest = dir.path().join("out");
        fs::create_dir_all(&dest).unwrap();

        for unsafe_name in ["..", ".", ""] {
            let err = extract(&archive, ArchiveKind::Bare, "", unsafe_name, &dest).unwrap_err();
            assert!(
                matches!(err, InstallError::UnsafeEntry { .. }),
                "{unsafe_name:?} produced {err}"
            );
        }

        // Nothing was written inside `dest` either -- not just that the call
        // returned an error. (The shared system temp directory above `dir` is
        // not asserted on: other tests create and remove entries there
        // concurrently, which would make that check flaky rather than
        // meaningful.)
        assert_eq!(fs::read_dir(&dest).unwrap().count(), 0);
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

    // ------------------------------------------------------- the output ceiling
    //
    // One test per `ArchiveKind` that `max_bytes` is honoured. `TarBz2` is the
    // exception and lives in `tests/tar_bz2_fixture.rs`, because `bzip2-rs` is
    // decode-only and there is no in-process way to build a `.tar.bz2` bomb.
    //
    // Every one of these overflows on a *later* entry rather than the first.
    // The only ceiling test that existed before used `max_bytes = 1`, which
    // trips on the first entry and therefore says nothing about whether a
    // refusal removes what it had already written.

    /// A payload of `size` zero bytes: compresses to almost nothing, so a
    /// multi-megabyte declared size costs a few hundred bytes of fixture.
    fn zeros(size: usize) -> Vec<u8> {
        vec![0u8; size]
    }

    #[test]
    fn a_zip_over_its_ceiling_is_refused_and_leaves_nothing_behind() {
        let dir = tempfile::tempdir().unwrap();
        let bomb = zeros(4 * 1024 * 1024);
        let archive = write_archive(
            dir.path(),
            &zip_with(&[("Release/first", b"eight!!!"), ("Release/second", &bomb)]),
        );
        let dest = dir.path().join("out");
        fs::create_dir_all(&dest).unwrap();

        let err =
            extract_with_limit(&archive, ArchiveKind::Zip, "Release", "", &dest, 1024).unwrap_err();
        assert!(
            matches!(err, InstallError::ExtractedTooLarge { limit: 1024, .. }),
            "{err}"
        );
        // The point of a *later* entry: "first" fitted and was written, and the
        // refusal on "second" has to take it back with it.
        assert!(
            !dest.join("first").exists(),
            "the entry that fitted must be removed with the one that did not"
        );
        assert!(!dest.join("second").exists());
        assert_eq!(fs::read_dir(&dest).unwrap().count(), 0);
    }

    #[test]
    fn a_zip_whose_header_understates_its_output_is_refused_on_the_real_bytes() {
        // The measured reason the declared-size check is not the whole ceiling:
        // a zip entry's reader is a decompressor over `take(compressed_size)`
        // (zip-2.4.2/src/read.rs), and nothing in it holds the *inflated*
        // output to the `uncompressed_size` the central directory declares. So
        // an archive claiming one byte and producing a megabyte would sail past
        // a header-only check -- which is the decompression bomb this ceiling
        // exists for.
        let dir = tempfile::tempdir().unwrap();
        let bomb = zeros(1024 * 1024);
        let honest = zip_with(&[("Release/lying", &bomb)]);
        let archive = write_archive(dir.path(), &understate_zip_size(honest, 1));
        let dest = dir.path().join("out");
        fs::create_dir_all(&dest).unwrap();

        let err =
            extract_with_limit(&archive, ArchiveKind::Zip, "Release", "", &dest, 4096).unwrap_err();
        assert!(
            matches!(err, InstallError::ExtractedTooLarge { limit: 4096, .. }),
            "a header-only ceiling would have let this through: {err}"
        );
        assert!(!dest.join("lying").exists());
    }

    #[test]
    fn a_tar_gz_over_its_ceiling_is_refused_and_leaves_nothing_behind() {
        let dir = tempfile::tempdir().unwrap();
        let bomb = zeros(4 * 1024 * 1024);
        let archive = write_archive(
            dir.path(),
            &tar_gz_with(&[("payload/first", b"eight!!!"), ("payload/second", &bomb)]),
        );
        let dest = dir.path().join("out");
        fs::create_dir_all(&dest).unwrap();

        let err = extract_with_limit(&archive, ArchiveKind::TarGz, "payload", "", &dest, 1024)
            .unwrap_err();
        assert!(
            matches!(err, InstallError::ExtractedTooLarge { limit: 1024, .. }),
            "{err}"
        );
        assert!(!dest.join("first").exists());
        assert!(!dest.join("second").exists());
    }

    #[test]
    fn a_tar_xz_over_its_ceiling_is_refused_and_leaves_nothing_behind() {
        let dir = tempfile::tempdir().unwrap();
        let bomb = zeros(1024 * 1024);
        let archive = write_archive(
            dir.path(),
            &tar_xz_with(&[("payload/first", b"eight!!!"), ("payload/second", &bomb)]),
        );
        let dest = dir.path().join("out");
        fs::create_dir_all(&dest).unwrap();

        let err = extract_with_limit(&archive, ArchiveKind::TarXz, "payload", "", &dest, 1024)
            .unwrap_err();
        assert!(
            matches!(err, InstallError::ExtractedTooLarge { limit: 1024, .. }),
            "{err}"
        );
        assert!(!dest.join("first").exists());
        assert!(!dest.join("second").exists());
    }

    #[test]
    fn a_bare_asset_over_its_ceiling_is_refused_before_it_is_copied() {
        // `Bare` has no header to check: the extracted size is the download's
        // own length on disk, so this is the one kind whose ceiling cannot be
        // lied to.
        let dir = tempfile::tempdir().unwrap();
        let archive = write_archive(dir.path(), &zeros(4096));
        let dest = dir.path().join("out");
        fs::create_dir_all(&dest).unwrap();

        let err =
            extract_with_limit(&archive, ArchiveKind::Bare, "", "yt-dlp", &dest, 1024).unwrap_err();
        assert!(
            matches!(err, InstallError::ExtractedTooLarge { limit: 1024, .. }),
            "{err}"
        );
        assert_eq!(fs::read_dir(&dest).unwrap().count(), 0);
    }

    #[test]
    fn an_extraction_that_fits_its_ceiling_exactly_still_succeeds() {
        // A bound that refuses a legitimate install is worse than no bound, so
        // the boundary is asserted on both sides rather than only the refusing
        // one. 8 + 4 = 12 bytes of payload, and 12 must be enough.
        let dir = tempfile::tempdir().unwrap();
        let entries: &[(&str, &[u8])] =
            &[("Release/first", b"eight!!!"), ("Release/second", b"four")];

        for (kind, bytes) in [
            (ArchiveKind::Zip, zip_with(entries)),
            (ArchiveKind::TarGz, tar_gz_with(entries)),
            (ArchiveKind::TarXz, tar_xz_with(entries)),
        ] {
            let archive = write_archive(dir.path(), &bytes);
            let dest = dir.path().join(format!("out-{kind:?}"));
            fs::create_dir_all(&dest).unwrap();

            let count = extract_with_limit(&archive, kind, "Release", "", &dest, 12).unwrap();
            assert_eq!(count, 2, "{kind:?}");
            assert_eq!(
                fs::read(dest.join("first")).unwrap(),
                b"eight!!!",
                "{kind:?}"
            );
            assert_eq!(fs::read(dest.join("second")).unwrap(), b"four", "{kind:?}");

            // One byte short is a refusal, on the second entry.
            let tight = dir.path().join(format!("tight-{kind:?}"));
            fs::create_dir_all(&tight).unwrap();
            let err = extract_with_limit(&archive, kind, "Release", "", &tight, 11).unwrap_err();
            assert!(
                matches!(err, InstallError::ExtractedTooLarge { .. }),
                "{kind:?}: {err}"
            );
            assert_eq!(fs::read_dir(&tight).unwrap().count(), 0, "{kind:?}");
        }
    }

    #[test]
    fn a_ceiling_counts_only_what_is_extracted_not_the_whole_archive() {
        // Entries outside `payload_dir` are skipped, so they must not consume
        // the budget either -- the Windows whisper archive carries a dozen
        // test binaries and a `llama.dll` that are never written.
        let dir = tempfile::tempdir().unwrap();
        let archive = write_archive(
            dir.path(),
            &zip_with(&[
                ("Release/wanted", b"four"),
                ("Debug/unwanted", &zeros(4 * 1024 * 1024)),
            ]),
        );
        let dest = dir.path().join("out");
        fs::create_dir_all(&dest).unwrap();

        assert_eq!(
            extract_with_limit(&archive, ArchiveKind::Zip, "Release", "", &dest, 4).unwrap(),
            1
        );
        assert_eq!(fs::read(dest.join("wanted")).unwrap(), b"four");
    }

    // ------------------------------------------------ rollback on other errors

    #[test]
    fn an_unsafe_entry_takes_the_files_written_before_it_with_it() {
        // Not a ceiling refusal: `extract_zip` and `extract_tar` used to leave
        // everything they had already written behind on *any* error, and a
        // half-unpacked directory can look convincing to a later `locate`.
        // Extracted with no ceiling at all, so this asserts the rollback rather
        // than the budget.
        let dir = tempfile::tempdir().unwrap();
        let archive = write_archive(
            dir.path(),
            &zip_with(&[
                ("Release/whisper-server.exe", b"server"),
                ("Release/../../../evil", b"payload"),
            ]),
        );
        let dest = dir.path().join("out");
        fs::create_dir_all(&dest).unwrap();

        let err = extract(&archive, ArchiveKind::Zip, "Release", "", &dest).unwrap_err();
        assert!(matches!(err, InstallError::UnsafeEntry { .. }), "{err}");
        assert!(
            !dest.join("whisper-server.exe").exists(),
            "the entry written before the refusal must not survive it"
        );
        assert_eq!(fs::read_dir(&dest).unwrap().count(), 0);
    }

    #[test]
    fn a_rollback_removes_the_directories_it_created_and_nothing_else() {
        // `dest` itself and anything already in it are the caller's: for
        // `sherpa_models::install`, `dest` is the models *root*, so a refused
        // extraction that removed it would take every other installed model.
        let dir = tempfile::tempdir().unwrap();
        let bomb = zeros(4 * 1024 * 1024);
        let archive = write_archive(
            dir.path(),
            &zip_with(&[
                ("Release/models/nested/first", b"four"),
                ("Release/second", &bomb),
            ]),
        );
        let dest = dir.path().join("out");
        fs::create_dir_all(&dest).unwrap();
        // A neighbour that predates the extraction.
        fs::create_dir_all(dest.join("another-model")).unwrap();
        fs::write(dest.join("another-model").join("weights"), b"keep me").unwrap();

        let err =
            extract_with_limit(&archive, ArchiveKind::Zip, "Release", "", &dest, 1024).unwrap_err();
        assert!(
            matches!(err, InstallError::ExtractedTooLarge { .. }),
            "{err}"
        );

        assert!(dest.is_dir(), "dest belongs to the caller");
        assert_eq!(
            fs::read(dest.join("another-model").join("weights")).unwrap(),
            b"keep me",
            "a neighbour the extraction never touched must survive"
        );
        assert!(
            !dest.join("models").exists(),
            "the directories the extraction created are gone: {:?}",
            fs::read_dir(&dest)
                .unwrap()
                .filter_map(Result::ok)
                .map(|e| e.file_name())
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn an_unlimited_extraction_is_still_unlimited() {
        // `extract` is `extract_with_limit` at `u64::MAX`, and the budget's
        // arithmetic must not turn that into a refusal -- `remaining()`
        // saturates and `declare` uses `checked_add`.
        let dir = tempfile::tempdir().unwrap();
        let archive = write_archive(
            dir.path(),
            &zip_with(&[("Release/a", &zeros(1024 * 1024)), ("Release/b", b"x")]),
        );
        let dest = dir.path().join("out");
        fs::create_dir_all(&dest).unwrap();

        assert_eq!(
            extract(&archive, ArchiveKind::Zip, "Release", "", &dest).unwrap(),
            2
        );
    }
}
