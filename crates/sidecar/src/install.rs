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
use crate::record;

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

    /// The install directory could not be replaced because something is using
    /// it. On Windows that is nearly always the sidecar itself still running.
    ///
    /// Typed rather than left as [`Self::Io`], which is what it was. A caller
    /// that wants to stop the sidecar and try again had nothing to match on,
    /// and the user got "could not remove …: Access is denied (os error 5)"
    /// with no hint that the cure is in their hands. **Measured on 2026-09-01
    /// with `cargo run -p kw-sidecar --example running_exe_lock_probe`**: on
    /// Windows, `remove_dir_all` of a directory holding a running executable
    /// fails with os error 5, reported against the `.exe`, while the same call
    /// on the same directory with the process stopped succeeds.
    #[error(
        "{path} could not be replaced because something is still using it. If {sidecar} is \\
         running, stop it and try again."
    )]
    InUse {
        sidecar: String,
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    /// Every candidate staging directory is present and cannot be cleared.
    ///
    /// This used to be an ordinary [`Self::Io`] on the single `{name}.incoming`
    /// path, and it was a **permanently stuck install**: one leftover directory
    /// nothing could delete — a file held open by a virus scanner, a
    /// hand-created directory with the wrong owner — and every future install
    /// of that sidecar failed at the first line, for ever, with no way for the
    /// user to get past it from inside the application. Now the install moves
    /// to the next staging name and only gives up when it has run out. See
    /// KNEG-113.
    #[error(
        "none of the {attempts} staging directories for {sidecar} under {root} could be \\
         cleared. The last attempt said: {last}. Remove {root} by hand and try again."
    )]
    NoStagingDirectory {
        sidecar: String,
        root: PathBuf,
        attempts: usize,
        last: String,
    },
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
/// Replaces any previous install of the same sidecar, and writes a
/// [`record::InstallRecord`] beside the files saying which pinned archive they
/// came from — that record is the only thing that ever makes an *installed*
/// sidecar's identity knowable, since [`crate::locate`] proves existence and
/// nothing more.
///
/// Leaves nothing behind on failure except a staging directory that a later
/// attempt clears or steps past. Two rules govern that directory, and both are
/// there because the single-name, hard-erroring version made one stuck install
/// permanent:
///
/// - A staging directory is used only when it was **absent, or emptied by this
///   call**. Never merely because it could be written to: whatever a previous
///   attempt left there did not finish verifying, and promoting it would put
///   unverified bytes where the sidecar goes.
/// - Failing to clear one is not fatal. [`take_staging`] moves to the next
///   name, and only [`STAGING_ATTEMPTS`] exhausted names are an error.
///
/// **Replacing the install directory is different and stays fatal**, because
/// `rename` cannot land on a directory that is still there. When it fails for
/// permissions the error is [`InstallError::InUse`], which is the caller's cue
/// to stop the sidecar and try again.
///
/// **Both bounds come from the manifest.** The download is capped at
/// [`Asset::size_bytes`], which is exact — `tools/sidecar-pin/pin.mjs` takes it
/// from `stat` after a full download, never from `content-length` — and the
/// extraction at [`Asset::max_extracted_size_bytes`].
///
/// The digest is the stronger guarantee and it comes first in importance, not
/// in time. That is the whole point of the download bound: `integrity::verify`
/// cannot run until the bytes are on disk, so a hostile or broken mirror gets
/// one window in which to fill the disk, and this closes it. The extraction
/// ceiling is defence in depth behind a digest that has already matched — a
/// pinned archive can still be a bomb if whoever pinned it was wrong.
pub async fn install(
    policy: &NetworkPolicy,
    spec: &SidecarSpec,
    asset: &Asset,
    root: &Path,
    // See `NetworkPolicy::download_to_file`: held across awaits, so `Send`.
    progress: &mut (dyn FnMut(Progress) + Send),
) -> Result<Installed, InstallError> {
    let staging = take_staging(root, &spec.name)?;

    let archive = staging.join("asset");

    let download = policy
        .download_to_file_bounded(
            RequestPurpose::UserTriggeredModelDownload,
            &asset.url,
            &archive,
            asset.size_bytes,
            &mut |written, total| progress(Progress::Downloading { written, total }),
        )
        .await?;

    progress(Progress::Verifying);
    integrity::verify(&archive, asset.size_bytes, &asset.sha256)?;

    progress(Progress::Extracting);
    let unpacked = staging.join("files");
    create_dir(&unpacked)?;
    let files = extract_with_limit(
        &archive,
        asset.archive,
        asset.payload_dir(),
        asset.executable_name(),
        &unpacked,
        asset.max_extracted_size_bytes,
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
    promote(spec, asset, &unpacked, &final_dir)?;

    // Best effort, and deliberately not `?`. The install has succeeded: the
    // user has the sidecar they asked for, and a staging directory that will
    // not go away is a few tens of megabytes rather than a failure. It used to
    // be `remove_dir(&staging)?`, which turned a file some scanner still had
    // open into a reported install failure for an install that had already
    // worked. `take_staging` routes around whatever is left here next time.
    sweep_staging(root, &spec.name);

    Ok(Installed {
        executable: final_dir.join(asset.executable_name()),
        files,
        source_url: download.final_url,
    })
}

/// How many staging directories are tried before an install gives up.
///
/// Ten rather than one, and rather than unbounded. One is the number that made
/// a single undeletable leftover a permanent failure; unbounded would fill the
/// disk with `.incoming.N` directories on a root that has become unwritable for
/// some quite different reason. Ten is enough that a real install always finds
/// one and few enough that the failure message is still worth printing.
const STAGING_ATTEMPTS: usize = 10;

/// The nth candidate staging directory for a sidecar.
///
/// The zeroth keeps the historical `{name}.incoming` name, so an install on a
/// machine with a leftover from an older build reuses it rather than stepping
/// past it and leaving it there for ever.
fn staging_path(root: &Path, name: &str, attempt: usize) -> PathBuf {
    if attempt == 0 {
        root.join(format!("{name}.incoming"))
    } else {
        root.join(format!("{name}.incoming.{attempt}"))
    }
}

/// Find a staging directory that is certainly empty, and create it.
///
/// **The one rule: never build on top of a directory whose contents are
/// unknown.** Whatever is in a leftover staging directory came from a download
/// that did not finish, and promoting it would put unverified files where the
/// sidecar goes. So a candidate is used only when it was absent or this call
/// removed it — never merely because `create_dir_all` succeeded on it.
fn take_staging(root: &Path, name: &str) -> Result<PathBuf, InstallError> {
    let mut last: Option<io::Error> = None;

    for attempt in 0..STAGING_ATTEMPTS {
        let candidate = staging_path(root, name, attempt);
        match fs::remove_dir_all(&candidate) {
            Ok(()) => {}
            Err(err) if err.kind() == io::ErrorKind::NotFound => {}
            Err(err) => {
                tracing::warn!(
                    path = %candidate.display(),
                    error = %err,
                    "a staging directory could not be cleared; trying the next one"
                );
                last = Some(err);
                continue;
            }
        }
        match fs::create_dir_all(&candidate) {
            Ok(()) => return Ok(candidate),
            Err(err) => {
                last = Some(err);
                continue;
            }
        }
    }

    Err(InstallError::NoStagingDirectory {
        sidecar: name.to_owned(),
        root: root.to_path_buf(),
        attempts: STAGING_ATTEMPTS,
        last: last.map_or_else(|| "nothing".to_owned(), |err| err.to_string()),
    })
}

/// Remove every staging directory this sidecar could be using.
///
/// Called after a successful promote. Silent by design — see the call site.
fn sweep_staging(root: &Path, name: &str) {
    for attempt in 0..STAGING_ATTEMPTS {
        let path = staging_path(root, name, attempt);
        if let Err(err) = fs::remove_dir_all(&path) {
            if err.kind() != io::ErrorKind::NotFound {
                tracing::debug!(
                    path = %path.display(),
                    error = %err,
                    "a staging directory outlived the install that made it"
                );
            }
        }
    }
}

/// Put a verified staging payload where the sidecar lives.
///
/// The record is written **into the staging directory**, not into the final
/// one, so it is carried across by the same `rename` as the files it describes.
/// There is no moment at which the install directory exists without its record,
/// or holds a record left over from a different extraction. See
/// [`crate::record`].
///
/// Separated from [`install`] so the sequence can be tested: everything above
/// it needs a network, and this is the part that decides what an installed
/// sidecar will later be able to say about itself.
fn promote(
    spec: &SidecarSpec,
    asset: &Asset,
    unpacked: &Path,
    final_dir: &Path,
) -> Result<(), InstallError> {
    record::write(unpacked, &record::InstallRecord::of(spec, asset)).map_err(|source| {
        InstallError::Io {
            action: "record the install in",
            path: unpacked.to_path_buf(),
            source,
        }
    })?;

    replace_dir(final_dir, &spec.name)?;

    fs::rename(unpacked, final_dir).map_err(|source| InstallError::Io {
        action: "move into place",
        path: final_dir.to_path_buf(),
        source,
    })
}

/// Clear the install directory so the new one can be renamed onto it.
///
/// A hard error, unlike the staging directories, and it has to be: `rename`
/// onto an existing directory fails on Windows, so there is nowhere else for
/// the install to go. What changed is the *shape* of the error — see
/// [`replace_failure`].
fn replace_dir(dir: &Path, sidecar: &str) -> Result<(), InstallError> {
    match fs::remove_dir_all(dir) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(replace_failure(dir, sidecar, source)),
    }
}

/// Which error a failure to clear the install directory deserves.
///
/// A refusal that looks like a permission problem becomes
/// [`InstallError::InUse`], with the one sentence the user can act on, because
/// on Windows this is nearly always the sidecar's own executable still running
/// out of the directory — measured on 2026-09-01, `os error 5` against the
/// `.exe`, by `examples/running_exe_lock_probe.rs`. Anything else stays a plain
/// [`InstallError::Io`]: a full disk and a running server are not the same
/// advice.
///
/// Its own function so the mapping can be tested without arranging for the
/// operating system to deny anything.
fn replace_failure(dir: &Path, sidecar: &str, source: io::Error) -> InstallError {
    if source.kind() == io::ErrorKind::PermissionDenied {
        return InstallError::InUse {
            sidecar: sidecar.to_owned(),
            path: dir.to_path_buf(),
            source,
        };
    }
    InstallError::Io {
        action: "remove",
        path: dir.to_path_buf(),
        source,
    }
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

/// A streaming [`io::Read`] over `lzma-rs`, which only decompresses into a
/// [`io::Write`].
///
/// **Why this exists.** `lzma_rs::xz_decompress` takes a `Write` and offers no
/// streaming reader, so the obvious code fills a `Vec` with the entire
/// decompressed tar before looking at a single entry. That is a memory bomb no
/// output ceiling can bound, because `max_bytes` counts what reaches `dest` and
/// the `Vec` is full before the first byte gets there. KNEG-106.
///
/// **How it is bounded, and it is the only mechanism here.** The decompressor
/// runs on its own thread and hands blocks to a [`mpsc::sync_channel`] of
/// [`CAPACITY`]. A full channel blocks the producer. So the bytes in flight
/// cannot exceed [`IN_FLIGHT_LIMIT`] however large the archive decompresses to
/// — the backpressure *is* the bound, and there is no second check to keep in
/// step with it. Which means the test for it has to be hard to fool, and the
/// first one was not: it watched a producer race a live consumer, and passed
/// with the channel widened to sixty-four gigabytes. It was measuring the
/// scheduler. What replaced it lets nothing read at all.
///
/// **What it does not bound.** `lzma-rs` allocates an LZMA2 dictionary whose
/// size the xz header declares, inside the decompressor and outside this
/// channel. A few megabytes for every real archive, and not attacker-chosen
/// here in any case: `extract` runs only after `integrity::verify` matched a
/// mandatory SHA-256, so the bytes are the pinned bytes. Said out loud because
/// "streaming" would otherwise read as "constant memory", and it is constant
/// plus a header-declared dictionary.
///
/// **A failure arriving after the consumer stops is not reported.** Dropping the
/// reader closes the channel, which ends the producer, and any error it was
/// about to report goes with it. [`extract_tar_gz`] and [`extract_tar_bz2`] have
/// exactly that property already — their decoders are driven by the tar reader
/// and stop when it does — so this matches the module rather than adding a new
/// hole. What a consumer that reads to the end always sees is the error, because
/// it is delivered in-band as the last item rather than through a side channel.
mod xz_stream {
    use std::fs;
    use std::io::{self, Write as _};
    use std::path::Path;
    use std::sync::mpsc;

    /// Bytes per block handed to the channel.
    ///
    /// 64 KiB: large enough that a multi-hundred-megabyte archive is thousands
    /// of handoffs rather than millions, small enough that the bound below is a
    /// megabyte rather than something worth thinking about.
    const BLOCK: usize = 64 * 1024;

    /// The most decompressed data that can be in flight at once.
    ///
    /// One megabyte, and this is the number that was chosen: the queue depth
    /// below is derived from it rather than the other way round, so the bound
    /// is stated and the plumbing follows. A test asserts the mechanism really
    /// holds to it, which is why it is a constant and not a sentence.
    pub(super) const IN_FLIGHT_LIMIT: usize = 1024 * 1024;

    /// Blocks the channel holds before the producer blocks.
    ///
    /// Two fewer than the bound allows: one block can sit in the producer's
    /// partial buffer and one in the consumer's cursor while the rest are
    /// queued, so the queue is the bound minus what is in hand at either end.
    const CAPACITY: usize = IN_FLIGHT_LIMIT / BLOCK - 2;

    /// Accumulates into blocks and hands them over, blocking when the channel
    /// is full.
    struct BlockWriter {
        tx: mpsc::SyncSender<io::Result<Vec<u8>>>,
        block: Vec<u8>,
    }

    impl BlockWriter {
        fn hand_over(&mut self) -> io::Result<()> {
            if self.block.is_empty() {
                return Ok(());
            }
            let block = std::mem::take(&mut self.block);
            self.tx.send(Ok(block)).map_err(|_| {
                // The consumer has gone: the tar reader found everything it
                // needed, or the budget was exceeded and extraction was
                // abandoned. Ending the decompressor is the right answer, and
                // an error out of `write` is how it is asked to end.
                io::Error::new(io::ErrorKind::BrokenPipe, "the reader stopped")
            })
        }
    }

    impl io::Write for BlockWriter {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            // Chunked rather than extended wholesale: `write` may be handed a
            // slice larger than BLOCK, and appending it whole would put the
            // producer's own buffer outside the bound this module exists to
            // hold.
            for chunk in buf.chunks(BLOCK) {
                self.block.extend_from_slice(chunk);
                if self.block.len() >= BLOCK {
                    self.hand_over()?;
                }
            }
            Ok(buf.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            self.hand_over()
        }
    }

    /// The consumer end: a plain `Read` over the blocks as they arrive.
    pub(super) struct XzReader {
        rx: mpsc::Receiver<io::Result<Vec<u8>>>,
        current: io::Cursor<Vec<u8>>,
        done: bool,
    }

    impl io::Read for XzReader {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            // A zero-length request is answered without touching the channel.
            // Otherwise the loop below would take a block, fail to put any of
            // it into an empty buffer, and go round again for ever.
            if buf.is_empty() {
                return Ok(0);
            }

            loop {
                let taken = self.current.read(buf)?;
                if taken > 0 {
                    return Ok(taken);
                }
                if self.done {
                    return Ok(0);
                }
                match self.rx.recv() {
                    Ok(Ok(block)) => self.current = io::Cursor::new(block),
                    Ok(Err(err)) => {
                        self.done = true;
                        return Err(err);
                    }
                    // Both ends of the producer dropped: the stream ended.
                    Err(_) => {
                        self.done = true;
                        return Ok(0);
                    }
                }
            }
        }
    }

    /// Start decompressing `archive` on a thread and return the reading end.
    ///
    /// The join handle is dropped deliberately. The thread ends when the
    /// channel closes, which happens whether the consumer reads to the end or
    /// abandons it, so there is nothing to wait for and waiting would mean
    /// holding a handle through a call that can abandon the reader.
    pub(super) fn open(archive: &Path) -> io::Result<XzReader> {
        let file = fs::File::open(archive)?;
        let (tx, rx) = mpsc::sync_channel::<io::Result<Vec<u8>>>(CAPACITY);

        std::thread::Builder::new()
            .name("kw-xz-decompress".to_owned())
            .spawn(move || {
                let mut reader = io::BufReader::new(file);
                let mut writer = BlockWriter {
                    tx: tx.clone(),
                    block: Vec::with_capacity(BLOCK),
                };

                let outcome = lzma_rs::xz_decompress(&mut reader, &mut writer)
                    .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err.to_string()))
                    .and_then(|()| writer.flush());

                if let Err(err) = outcome {
                    // In-band, as the last item, so a consumer reading to the
                    // end is told why the stream stopped instead of seeing a
                    // short one. Ignored if the consumer has already gone,
                    // which is the case that produced the error to begin with.
                    let _ = tx.send(Err(err));
                }
            })?;

        Ok(XzReader {
            rx,
            current: io::Cursor::new(Vec::new()),
            done: false,
        })
    }

    #[cfg(test)]
    #[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    mod tests {
        use super::*;
        use std::io::Read as _;
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;

        /// Nothing reading means the producer stops at the bound.
        ///
        /// This is the whole mechanism, so it is the test that has to be
        /// hard to fool. The first version counted how far the producer got
        /// ahead of a consumer running concurrently and asserted the gap stayed
        /// small — which passed with the channel widened to sixty-four
        /// **gigabytes** of queue, because a fast consumer kept the observed gap
        /// small anyway. It was measuring the scheduler.
        ///
        /// Nothing reads here at all. The timeout is in the safe direction:
        /// expiring means the producer was still blocked, which is the pass,
        /// and only the producer *finishing* fails it. A slow machine cannot
        /// cause a failure; an unbounded channel finishes in milliseconds.
        #[test]
        fn nothing_reading_means_the_producer_stops_at_the_bound() {
            let (tx, rx) = mpsc::sync_channel::<io::Result<Vec<u8>>>(CAPACITY);
            let (done_tx, done_rx) = mpsc::channel::<usize>();

            std::thread::spawn(move || {
                let mut writer = BlockWriter {
                    tx,
                    block: Vec::with_capacity(BLOCK),
                };
                let payload = vec![0_u8; BLOCK];
                let mut wrote = 0_usize;
                while wrote < IN_FLIGHT_LIMIT * 2 {
                    if writer.write_all(&payload).is_err() {
                        break;
                    }
                    wrote += BLOCK;
                }
                let _ = done_tx.send(wrote);
            });

            match done_rx.recv_timeout(std::time::Duration::from_secs(1)) {
                // Still blocked with a full channel, which is the bound doing
                // its job.
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Ok(wrote) => panic!(
                    "the producer wrote {wrote} bytes with nothing reading, so the channel is \
                     not applying backpressure and the buffer is unbounded again"
                ),
                Err(other) => panic!("the producer ended unexpectedly: {other}"),
            }

            // Held until here on purpose: dropping it earlier would release the
            // producer and the test would be measuring a closed channel.
            drop(rx);
        }

        /// One oversized `write` is still broken into blocks.
        ///
        /// `lzma-rs` decides how much it hands over at a time, and appending it
        /// whole would move the buffering problem out of the `Vec` and into
        /// `BlockWriter` — where it would be just as unbounded and much harder
        /// to see. The bound above cannot catch this on its own: a producer fed
        /// exactly one block at a time behaves identically either way, which is
        /// why this is a second test and not a second assertion.
        #[test]
        fn one_oversized_write_is_still_broken_into_blocks() {
            let (tx, rx) = mpsc::sync_channel::<io::Result<Vec<u8>>>(CAPACITY);

            std::thread::spawn(move || {
                let mut writer = BlockWriter {
                    tx,
                    block: Vec::with_capacity(BLOCK),
                };
                let _ = writer.write_all(&vec![3_u8; IN_FLIGHT_LIMIT * 4]);
                let _ = writer.flush();
            });

            let mut biggest = 0_usize;
            let mut total = 0_usize;
            while let Ok(Ok(block)) = rx.recv() {
                biggest = biggest.max(block.len());
                total += block.len();
            }

            assert_eq!(total, IN_FLIGHT_LIMIT * 4, "every byte came back out");
            assert!(
                biggest <= BLOCK,
                "a block of {biggest} bytes went through, over the {BLOCK} cap"
            );
        }

        /// Every byte written comes back out, in order, through the pipe.
        #[test]
        fn the_pipe_delivers_the_stream_unchanged() {
            let (tx, rx) = mpsc::sync_channel::<io::Result<Vec<u8>>>(CAPACITY);
            let written: Vec<u8> = (0..200_000_u32).map(|i| (i % 251) as u8).collect();
            let expected = written.clone();

            std::thread::spawn(move || {
                let mut writer = BlockWriter {
                    tx,
                    block: Vec::with_capacity(BLOCK),
                };
                let _ = writer.write_all(&written);
                let _ = writer.flush();
            });

            let mut reader = XzReader {
                rx,
                current: io::Cursor::new(Vec::new()),
                done: false,
            };
            let mut read_back = Vec::new();
            reader
                .read_to_end(&mut read_back)
                .expect("the pipe delivered the stream");

            assert_eq!(read_back, expected);
        }

        /// A consumer that stops early ends the producer rather than leaving a
        /// thread blocked on a channel nobody will drain. The budget being
        /// exceeded mid-archive is exactly this case.
        #[test]
        fn abandoning_the_reader_ends_the_decompressor() {
            let (tx, rx) = mpsc::sync_channel::<io::Result<Vec<u8>>>(CAPACITY);
            let wrote = Arc::new(AtomicUsize::new(0));

            let producer = {
                let wrote = Arc::clone(&wrote);
                std::thread::spawn(move || {
                    let mut writer = BlockWriter {
                        tx,
                        block: Vec::with_capacity(BLOCK),
                    };
                    let payload = vec![0_u8; BLOCK];
                    // Would never finish against a channel nobody drains, so
                    // the test hanging is itself the failure this asserts
                    // against.
                    for _ in 0..100_000 {
                        if writer.write_all(&payload).is_err() {
                            break;
                        }
                        wrote.fetch_add(BLOCK, Ordering::SeqCst);
                    }
                })
            };

            {
                let mut reader = XzReader {
                    rx,
                    current: io::Cursor::new(Vec::new()),
                    done: false,
                };
                let mut buffer = vec![0_u8; 1024];
                let _ = reader.read(&mut buffer);
            }

            producer.join().expect("the producer thread ended");
            assert!(
                wrote.load(Ordering::SeqCst) < 100_000 * BLOCK,
                "the producer ran to completion, so it was never blocked"
            );
        }
    }
}

/// Decompress a `.tar.xz` with `lzma-rs` (pure Rust, decode-only -- see the
/// `lzma-rs` comment in the root `Cargo.toml`), streaming it into the tar
/// reader the same way `extract_tar_gz` does.
///
/// `lzma-rs` decompresses into a `Write` and offers no streaming `Read`, so
/// this went through [`xz_stream`] rather than through a `Vec` holding the
/// whole decompressed tar. That `Vec` was a memory bomb the `max_bytes` ceiling
/// could not touch -- the ceiling counts bytes reaching `dest`, and the buffer
/// was full before the first one got there. Fixed 2026-08-31 (KNEG-106); see
/// [`xz_stream`] for what is bounded and what is not.
fn extract_tar_xz(
    archive: &Path,
    payload_dir: &str,
    dest: &Path,
    budget: &mut Budget<'_>,
) -> Result<usize, InstallError> {
    let reader = xz_stream::open(archive).map_err(|source| InstallError::Io {
        action: "open",
        path: archive.to_path_buf(),
        source,
    })?;

    // A decompression failure now reaches this as a read error part-way through
    // the tar, so it comes back as `Archive` from `extract_tar` rather than
    // being raised here before the tar is touched. Same variant, same archive,
    // and the `lzma-rs` message is still in the detail -- it travels in-band as
    // the last item on the channel.
    extract_tar(
        reader,
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
    // The separately requested Known voices payload has a strict DLL allowlist.
    if payload_dir == "sherpa-onnx-v1.13.4-win-x64-shared-MT-Release/lib" {
        let normalized = name.replace('\\', "/");
        let prefix = format!("{payload_dir}/");
        let file = normalized.strip_prefix(&prefix);
        return Ok(file
            .filter(|file| {
                matches!(
                    *file,
                    "sherpa-onnx-c-api.dll"
                        | "onnxruntime.dll"
                        | "onnxruntime_providers_shared.dll"
                )
            })
            .map(PathBuf::from));
    }
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
    // Archive names have the same safety rules on every host. Unix treats
    // `C:/file` and `C:file` as normal components, not Windows prefixes.
    let drive_qualified =
        matches!(stripped.as_bytes(), [letter, b':', ..] if letter.is_ascii_alphabetic());
    let safe = !drive_qualified
        && candidate
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
        for name in [
            "C:/Windows/System32/evil.dll",
            "c:\\evil.dll",
            "Z:evil.dll",
            "d:",
        ] {
            for payload in ["", "Release"] {
                let entry = if payload.is_empty() {
                    name.to_owned()
                } else {
                    format!("{payload}/{name}")
                };
                assert!(
                    matches!(
                        payload_relative(&entry, payload),
                        Err(InstallError::UnsafeEntry { .. })
                    ),
                    "{entry} must be refused on every host"
                );
            }
        }
    }

    #[test]
    fn ordinary_drive_letter_names_remain_relative() {
        for name in ["C", "c.dll", "C/weights.bin"] {
            assert_eq!(
                payload_relative(name, "").unwrap(),
                Some(PathBuf::from(name))
            );
        }
    }

    #[test]
    fn a_drive_qualified_zip_entry_rolls_back_prior_files() {
        let dir = tempfile::tempdir().unwrap();
        let archive = write_archive(
            dir.path(),
            &zip_with(&[
                ("Release/first", b"written first"),
                ("Release/C:/evil.dll", b"refused"),
            ]),
        );
        let dest = dir.path().join("out");
        fs::create_dir_all(&dest).unwrap();
        fs::write(dest.join("existing"), b"keep").unwrap();
        assert!(matches!(
            extract(&archive, ArchiveKind::Zip, "Release", "", &dest),
            Err(InstallError::UnsafeEntry { .. })
        ));
        assert!(!dest.join("first").exists());
        assert_eq!(fs::read(dest.join("existing")).unwrap(), b"keep");
        assert_eq!(fs::read_dir(&dest).unwrap().count(), 1);
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

    /* ------------------------------------------- staging, promote, record */

    const PINNED: &str = "c2a4b60edb11f7e11a9191ffb50929535527d4d91c9903dbe3e554583bbbc63d";

    fn test_asset() -> Asset {
        Asset {
            url: "https://example.invalid/whisper-bin-x64.zip".to_owned(),
            archive: ArchiveKind::Zip,
            sha256: crate::Sha256Digest::parse(PINNED).unwrap(),
            size_bytes: 10,
            max_extracted_size_bytes: 10,
            executable_in_archive: "Release/whisper-server.exe".to_owned(),
        }
    }

    fn test_spec() -> SidecarSpec {
        let mut targets = std::collections::BTreeMap::new();
        targets.insert(
            crate::Target::WindowsX8664,
            crate::Availability::Published(test_asset()),
        );
        SidecarSpec {
            name: "whisper".to_owned(),
            description: "a test sidecar".to_owned(),
            source: crate::Source {
                repo: "ggml-org/whisper.cpp".to_owned(),
                release: "b4938".to_owned(),
                pinned_because: "reviewed".to_owned(),
            },
            transport: crate::manifest::Transport::Loopback {
                port_range: crate::PortRange {
                    first: 8178,
                    last: 8188,
                },
            },
            command_fragments: vec!["whisper-server".to_owned()],
            targets,
        }
    }

    /// A staging directory that looks like a finished extraction.
    fn staged(root: &Path) -> PathBuf {
        let unpacked = root.join("whisper.incoming").join("files");
        fs::create_dir_all(&unpacked).unwrap();
        fs::write(unpacked.join("whisper-server.exe"), b"MZ").unwrap();
        fs::write(unpacked.join("ggml.dll"), b"MZ").unwrap();
        unpacked
    }

    #[test]
    fn a_promoted_install_can_say_which_archive_it_came_from() {
        // The whole of KNEG-113 in one assertion. Before this, an installed
        // sidecar could only be proved to *exist*; nothing on disk said which
        // build it was, so nothing could ever notice it had fallen behind.
        let dir = tempfile::tempdir().unwrap();
        let unpacked = staged(dir.path());
        let final_dir = dir.path().join("whisper");

        promote(&test_spec(), &test_asset(), &unpacked, &final_dir).unwrap();

        let installation = crate::locate(
            "whisper",
            &test_asset(),
            &crate::SearchPath {
                bundled: None,
                downloaded: dir.path().to_path_buf(),
            },
            &crate::RealFs,
        );
        assert!(installation.is_installed());
        assert_eq!(
            record::freshness(
                &installation,
                &record::read(&final_dir),
                "whisper",
                &test_asset().sha256,
            ),
            record::Freshness::Current
        );
    }

    #[test]
    fn the_record_is_written_into_staging_before_anything_is_replaced() {
        // Ordering, and it is the whole reason a record can be trusted. Written
        // into staging, it is carried across by the same rename as the files it
        // describes. Written to the install directory *after* the rename, a
        // crash between the two steps would leave an install whose record
        // described the one it replaced -- worse than no record, because it
        // reads as a measurement.
        //
        // Observed by making the promote fail at the replace step: a plain file
        // where the install directory should be, which `remove_dir_all` will
        // not clear. The record has to be in staging by the time that happens.
        let dir = tempfile::tempdir().unwrap();
        let unpacked = staged(dir.path());
        let blocked = dir.path().join("whisper");
        fs::write(&blocked, b"not a directory").unwrap();

        assert!(promote(&test_spec(), &test_asset(), &unpacked, &blocked).is_err());
        assert!(
            unpacked.join(record::RECORD_FILE).is_file(),
            "the record has to be written before the replace is attempted"
        );
    }

    #[test]
    fn a_promote_leaves_no_copy_of_the_payload_in_staging() {
        let dir = tempfile::tempdir().unwrap();
        let unpacked = staged(dir.path());
        let final_dir = dir.path().join("whisper");

        promote(&test_spec(), &test_asset(), &unpacked, &final_dir).unwrap();

        assert!(!unpacked.exists(), "staging kept a copy of the payload");
        assert!(final_dir.join(record::RECORD_FILE).is_file());
    }

    #[test]
    fn promoting_over_an_older_install_replaces_it_record_and_all() {
        let dir = tempfile::tempdir().unwrap();
        let final_dir = dir.path().join("whisper");
        fs::create_dir_all(&final_dir).unwrap();
        fs::write(final_dir.join("whisper-server.exe"), b"old").unwrap();
        fs::write(final_dir.join("left-over.dll"), b"old").unwrap();

        let unpacked = staged(dir.path());
        promote(&test_spec(), &test_asset(), &unpacked, &final_dir).unwrap();

        assert_eq!(
            fs::read(final_dir.join("whisper-server.exe")).unwrap(),
            b"MZ"
        );
        assert!(
            !final_dir.join("left-over.dll").exists(),
            "the old directory was merged into rather than replaced"
        );
    }

    #[test]
    fn a_free_staging_name_is_the_plain_one() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            take_staging(dir.path(), "whisper").unwrap(),
            dir.path().join("whisper.incoming")
        );
    }

    #[test]
    fn a_leftover_staging_directory_is_emptied_and_reused() {
        // The state the machine this was written on is in: a complete extracted
        // copy sitting in `whisper.incoming` from an install that failed at the
        // promote step. Reusing the name matters -- stepping past it would
        // leave it there for ever -- and so does emptying it, because its
        // contents came from a run that never finished.
        let dir = tempfile::tempdir().unwrap();
        let leftover = dir.path().join("whisper.incoming");
        fs::create_dir_all(leftover.join("files")).unwrap();
        fs::write(leftover.join("files").join("half.dll"), b"junk").unwrap();

        let taken = take_staging(dir.path(), "whisper").unwrap();
        assert_eq!(taken, leftover);
        assert_eq!(fs::read_dir(&taken).unwrap().count(), 0);
    }

    #[test]
    fn a_staging_name_that_cannot_be_cleared_is_stepped_past_rather_than_fatal() {
        // This is the permanently-stuck install. One `{name}.incoming` that
        // will not go away used to fail every future install of that sidecar at
        // the first line, with nothing the user could do from inside the
        // application. A plain file with the directory's name is the portable
        // way to make `remove_dir_all` and `create_dir_all` both refuse.
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("whisper.incoming"), b"not a directory").unwrap();

        let taken = take_staging(dir.path(), "whisper").unwrap();
        assert_eq!(taken, dir.path().join("whisper.incoming.1"));
        assert!(taken.is_dir());
    }

    #[test]
    fn an_install_gives_up_only_when_every_staging_name_is_blocked() {
        let dir = tempfile::tempdir().unwrap();
        for attempt in 0..STAGING_ATTEMPTS {
            fs::write(staging_path(dir.path(), "whisper", attempt), b"blocked").unwrap();
        }

        match take_staging(dir.path(), "whisper") {
            Err(InstallError::NoStagingDirectory {
                sidecar,
                attempts,
                last,
                ..
            }) => {
                assert_eq!(sidecar, "whisper");
                assert_eq!(attempts, STAGING_ATTEMPTS);
                // The operating system's own words. Swallowing them and
                // reporting the placeholder would leave whoever is stuck with
                // nothing to look up.
                assert_ne!(last, "nothing", "the last failure was not reported");
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn the_sweep_clears_every_staging_name_and_leaves_the_install_alone() {
        let dir = tempfile::tempdir().unwrap();
        let install = dir.path().join("whisper");
        fs::create_dir_all(&install).unwrap();
        fs::write(install.join("whisper-server.exe"), b"MZ").unwrap();
        for attempt in 0..STAGING_ATTEMPTS {
            fs::create_dir_all(staging_path(dir.path(), "whisper", attempt)).unwrap();
        }

        sweep_staging(dir.path(), "whisper");

        for attempt in 0..STAGING_ATTEMPTS {
            assert!(!staging_path(dir.path(), "whisper", attempt).exists());
        }
        assert!(install.join("whisper-server.exe").is_file());
    }

    #[test]
    fn a_directory_held_open_is_reported_as_in_use_and_names_the_sidecar() {
        // The mapping only, not the operating system: arranging a real denial
        // needs a running executable, which
        // `examples/running_exe_lock_probe.rs` does and a unit test should not.
        // What is asserted here is that a denial becomes the error a caller can
        // match on and a user can act on, rather than a bare os error 5.
        let err = replace_failure(
            Path::new("/data/sidecars/whisper"),
            "whisper",
            io::Error::from(io::ErrorKind::PermissionDenied),
        );
        match &err {
            InstallError::InUse { sidecar, path, .. } => {
                assert_eq!(sidecar, "whisper");
                assert!(path.ends_with("whisper"));
            }
            other => panic!("{other:?}"),
        }
        assert!(err.to_string().contains("stop it and try again"), "{err}");
    }

    #[test]
    fn a_full_disk_is_not_reported_as_a_running_sidecar() {
        // The advice differs, so the error has to. Every non-permission failure
        // stays a plain `Io`.
        let err = replace_failure(
            Path::new("/data/sidecars/whisper"),
            "whisper",
            io::Error::from(io::ErrorKind::OutOfMemory),
        );
        assert!(matches!(err, InstallError::Io { .. }), "{err:?}");
    }

    #[test]
    fn clearing_an_install_directory_that_is_not_there_is_not_a_failure() {
        // The first install on a new machine.
        let dir = tempfile::tempdir().unwrap();
        replace_dir(&dir.path().join("whisper"), "whisper").unwrap();
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod voice_payload_tests {
    #[test]
    fn voice_dll_allowlist_is_separate_from_the_existing_cli_payload() {
        let lib = "sherpa-onnx-v1.13.4-win-x64-shared-MT-Release/lib";
        let bin = "sherpa-onnx-v1.13.4-win-x64-shared-MT-Release/bin";
        for name in [
            "sherpa-onnx-c-api.dll",
            "onnxruntime.dll",
            "onnxruntime_providers_shared.dll",
        ] {
            let path = format!("{lib}/{name}");
            assert_eq!(
                super::payload_relative(&path, lib).unwrap(),
                Some(std::path::PathBuf::from(name))
            );
            assert!(super::payload_relative(&path, bin).unwrap().is_none());
        }
        for path in [
            format!("{lib}/unknown.dll"),
            format!("{lib}/../bad.dll"),
            format!("{lib}/sub/onnxruntime.dll"),
            "other/lib/onnxruntime.dll".into(),
        ] {
            assert!(super::payload_relative(&path, lib).unwrap().is_none());
        }
    }
}
