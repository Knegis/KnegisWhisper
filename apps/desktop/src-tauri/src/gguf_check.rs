//! What can honestly be said about a GGUF that is already on disk.
//!
//! [`crate::local_llm`]'s installer hashes a finished download against its
//! pinned SHA-256 before renaming it into place, and until this module existed
//! that was the last time anything looked at the bytes. `installed: true` on
//! Settings > Intelligence > Local was a claim about a **path**, not about a
//! file: a GGUF corrupted, truncated or replaced months after a successful
//! install read exactly the same as one that had just been checked. KNEG-107.
//!
//! # Why the whisper approach was not copied
//!
//! The whisper path re-hashes on every status call — `models::is_installed`
//! goes through `kw_sidecar::integrity::verify` — and gets away with it because
//! its largest model is 3.1 GB. The GGUFs reach 19.8 GB. Reading that back every
//! time a settings screen opens would stall the screen for about a minute per
//! visit, so that trade is refused here.
//!
//! # What is done instead
//!
//! A small record is written beside the GGUF when it is installed, holding the
//! digest it was checked against, its exact length, and its modification time.
//! A file whose length and mtime still agree with that record counts as
//! **unchanged since it was installed**.
//!
//! # What that catches, and what it does not
//!
//! It catches the failure KNEG-107 is about: accidental corruption, a truncated
//! copy, a half-finished restore, a partial overwrite, a disk that lost a block.
//! Every one of those moves the length, the mtime, or both.
//!
//! **It does not catch a deliberate replacement that preserves both.** Writing
//! different bytes into the same length and then setting the timestamp back is a
//! few lines in any language, and this check would call the result unchanged.
//! That is a different threat from the one this was built for, and it is said
//! here plainly rather than left for a reader to assume the file is
//! authenticated on every load. **The only thing that proves the bytes is
//! hashing them**, which is what `local_llm_recheck_model` does — on request,
//! never on a screen paint.
//!
//! That is also why the state is called [`GgufIntegrity::UnchangedSinceInstall`]
//! and not `Verified`. The weaker name is the point: it is the strongest claim
//! two integers can support.
//!
//! # Five states, not two
//!
//! A boolean over a path cannot tell *checked and unchanged* from *nothing was
//! ever recorded*, and rendering the second as the first is the same defect this
//! module exists to remove, one layer up. So [`integrity_of`] answers five ways,
//! and **the absence of a record is its own answer** — never "verified", never
//! "corrupt". Every GGUF installed before this landed is in that state, and that
//! is not a fault in the file.
//!
//! # A failed re-hash is a fact, and it is kept
//!
//! The fifth state, [`GgufIntegrity::FailedRecheck`], is KNEG-130. A re-hash that
//! disagreed with the pinned digest used to **delete** the record, which sounds
//! like the safe direction and is not. The row fell back to
//! [`GgufIntegrity::Unrecorded`] -- which this module documents at length as
//! emphatically *not* an accusation -- and the one warning `local_llm::start_model`
//! emitted fired only for [`GgufIntegrity::Changed`], so the application went
//! quiet about exactly the file it had just proved was not the one that was
//! installed. Deleting the record destroyed the finding. It is written down now,
//! and it survives a restart.
//!
//! **It goes in the one record.** Not a second file beside it: two files
//! describing one model are two things that can disagree, and the disagreement
//! would be about whether a model can be trusted. The precedent is
//! [`crate::calendar`], which already persists negative verdicts --
//! `SyncOutcome::{Failed, TokenUnavailable}` -- and folds them into live status
//! through the pure `combine_token_status`, so that a screen can never see a
//! different answer from a check it just ran than from the record read back
//! later. That is the property wanted here and it is why the verdict is a field
//! of [`InstallRecord`] rather than a file of its own.
//!
//! This is still the first negative verdict this application writes to **a file
//! beside an artefact** rather than into SQLite, which is worth saying plainly:
//! the record is ordinary JSON in a user-writable directory. It is a place to
//! keep a finding, never a place to enforce one.
//!
//! # Where the line between decision and platform runs
//!
//! [`integrity_of`] is a pure function over plain integers, so the state machine
//! is testable with no files and no `tauri::AppHandle` — the constraint
//! `media.rs` records. Everything below it that touches the filesystem is a thin
//! edge whose only job is to produce those integers.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::local_model::VerifiedBy;

/// What the record is called: the GGUF's own name with this appended.
///
/// Beside the file rather than in a sidecar directory, so a user who moves or
/// deletes a model by hand takes its record with it — a record pointing at a
/// file that is gone would be the one piece of state that could go stale
/// silently.
const RECORD_SUFFIX: &str = ".checked.json";

/// The record layout this build **writes**.
///
/// Bumped to 2 by KNEG-130, which added [`InstallRecord::verdict`].
const SCHEMA: u32 = 2;

/// The record layouts this build can **read**.
///
/// Two constants rather than one, because "what we write" and "what we
/// understand" are different facts and collapsing them is what made the
/// original single `SCHEMA` a backward-compatibility hazard the moment it
/// moved: every record written before the bump would have been discarded, and
/// every model on disk would have fallen from *unchanged since it was
/// installed* to *installed, never recorded* for no reason a user could see.
///
/// A schema **1** record predates the verdict field and is read as
/// [`Verdict::Passed`] -- see [`InstallRecord::verdict`]. Anything outside this
/// set is still read as no record at all rather than interpreted
/// optimistically: a future version that changes what `bytes` means must not
/// have this one agreeing with it by accident.
const READABLE_SCHEMAS: [u32; 2] = [1, SCHEMA];

const NANOS_PER_SECOND: u32 = 1_000_000_000;

/* ----------------------------------------------------------------- the facts */

/// A file's modification time, as two integers that survive JSON.
///
/// `SystemTime` has no stable serialisation and no ordering guarantees worth
/// storing, so it is decomposed here. `seconds` is signed because a machine with
/// a badly set clock can hold a file stamped before 1970, and the normalisation
/// keeps `nanos` positive so that two `Mtime`s compare with a derived `Eq`
/// rather than needing arithmetic at the comparison site.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Mtime {
    /// Whole seconds from the Unix epoch, negative before it.
    seconds: i64,
    /// The sub-second remainder, always in `0..1_000_000_000`.
    nanos: u32,
}

impl Mtime {
    /// Decompose a `SystemTime`, saturating rather than failing.
    ///
    /// Saturation is safe in the direction that matters: a clamped value is a
    /// value that will not compare equal to the next reading of a *different*
    /// time, so a saturating conversion can report a change that did not happen
    /// but cannot hide one that did. Both bounds are ±292 billion years out.
    #[must_use]
    pub fn of(time: SystemTime) -> Self {
        match time.duration_since(SystemTime::UNIX_EPOCH) {
            Ok(after) => Self {
                seconds: i64::try_from(after.as_secs()).unwrap_or(i64::MAX),
                nanos: after.subsec_nanos(),
            },
            Err(before) => {
                let before = before.duration();
                let whole = i64::try_from(before.as_secs()).unwrap_or(i64::MAX);
                match before.subsec_nanos() {
                    0 => Self {
                        seconds: whole.saturating_neg(),
                        nanos: 0,
                    },
                    // Borrow a second so the remainder stays positive:
                    // 0.25 s before the epoch is (-1 s, +0.75 s).
                    sub => Self {
                        seconds: whole.saturating_neg().saturating_sub(1),
                        nanos: NANOS_PER_SECOND - sub,
                    },
                }
            }
        }
    }
}

/// What the filesystem says about a file right now.
///
/// The two figures [`integrity_of`] decides on, and nothing else. Deliberately
/// not a `Metadata`: the decision must not be able to reach for a third fact
/// without that fact appearing in this type first.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Observed {
    pub bytes: u64,
    pub mtime: Mtime,
}

/// What the last check of these bytes concluded.
///
/// Two states and not a `bool`, for the reason every other answer in this module
/// is not a `bool`: the variants carry different evidence. A pass carries
/// nothing beyond itself -- the record's own `sha256` and `verified_by` already
/// say what was met -- and a failure has to carry the pair of digests that
/// disagreed, because "it did not match" without saying what it did not match is
/// not something anybody can act on.
///
/// **The digests live here rather than being read back off
/// [`InstallRecord::sha256`].** They are the same string today. They are not the
/// same claim: `sha256` records what the *install* pinned, and these record what
/// the *re-check* actually compared. A future in which those diverge -- a pin
/// updated in the registry after a model was installed -- is exactly when a
/// person reading this file needs to see both.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(tag = "outcome", rename_all = "kebab-case")]
pub enum Verdict {
    /// Nothing has contradicted this file. The state of every record written by
    /// an install, and of every record written before this field existed.
    #[default]
    Passed,
    /// A re-hash read the whole file and it did not match the digest it is
    /// pinned to.
    Failed {
        /// The digest the file was compared against.
        expected: String,
        /// What it actually hashed to.
        actual: String,
    },
}

/// What was checked when a GGUF was installed, as written beside it.
///
/// `verified_by` is a plain string rather than a [`VerifiedBy`], and that is not
/// laziness. This is an on-disk format that has to survive being read by a build
/// older or newer than the one that wrote it; binding it to an IPC enum's serde
/// attributes would make a rename on the wire a silent read failure here. The
/// mapping back is [`Self::strength`], which answers [`VerifiedBy::Nothing`] for
/// a spelling it does not know — understating rather than guessing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallRecord {
    /// Always [`SCHEMA`] when written; anything else makes this no record.
    schema: u32,
    /// The digest the bytes were hashed against, or `None` when the registry
    /// carried none and only the length was ever checked.
    sha256: Option<String>,
    /// Which of [`VerifiedBy`]'s four standards that check met, in the same
    /// kebab-case spelling the IPC boundary uses.
    verified_by: String,
    /// The file's length at the moment it was checked.
    bytes: u64,
    /// Its modification time at the same moment.
    mtime: Mtime,
    /// When the check was made. Not reported to the frontend today; it is here
    /// because the file is meant to be readable by a person looking at their own
    /// model directory, and "when" is the first thing they will want.
    checked_at_unix_seconds: i64,
    /// What that check concluded.
    ///
    /// `#[serde(default)]` is the whole of the backward compatibility. A schema
    /// 1 record has no such key and deserialises to [`Verdict::Passed`], which
    /// is the correct reading of it: nothing had ever contradicted the file, and
    /// a record written by an install is a record of a check that passed. The
    /// distinction between *passed* and *no verdict was recorded* exists in the
    /// history and not in the answer -- both mean the screen may say `unchanged
    /// since it was installed` when the two integers still agree -- so it is not
    /// carried as a third variant that nothing would ever branch on.
    #[serde(default)]
    verdict: Verdict,
}

impl InstallRecord {
    /// The strongest thing the install could honestly claim about these bytes.
    #[must_use]
    pub fn strength(&self) -> VerifiedBy {
        match self.verified_by.as_str() {
            s if s == VerifiedBy::Digest.as_str() => VerifiedBy::Digest,
            s if s == VerifiedBy::StatedDigest.as_str() => VerifiedBy::StatedDigest,
            s if s == VerifiedBy::Size.as_str() => VerifiedBy::Size,
            // Includes `"nothing"` and anything this build does not recognise.
            // A spelling from a future variant must round *down*: claiming a
            // strength we cannot name is the failure mode this whole module is
            // about.
            _ => VerifiedBy::Nothing,
        }
    }
}

/* ------------------------------------------------------------- the decision */

/// What the Intelligence > Local screen may say about one GGUF.
///
/// Five answers, and the third is the reason this is not a boolean. See the
/// module documentation for what each one is worth.
///
/// **Not `Copy` since KNEG-130.** [`Self::FailedRecheck`] carries two 64-character
/// digests, and a state that can be duplicated by accident is one a caller can
/// report twice about a file it read once.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(
    tag = "state",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum GgufIntegrity {
    /// Nothing is at the path. The model has not been installed, or was deleted.
    NotInstalled,
    /// A record sits beside the file and both its length and its modification
    /// time still agree with what was recorded when it was checked.
    ///
    /// **This is not a re-hash.** Bytes rewritten in place, keeping the same
    /// length and the same timestamp, would still read this way. `verified_by`
    /// says how strong the *original* check was, not how strong this one is.
    UnchangedSinceInstall { verified_by: VerifiedBy },
    /// The file is there and nothing beside it says what was checked.
    ///
    /// Every GGUF installed before this record existed is in this state, so it
    /// is emphatically not an accusation. It is the honest answer — nothing is
    /// known — and it must never be rendered as either of its neighbours.
    Unrecorded,
    /// A record sits beside the file and disagrees with it.
    ///
    /// `recorded_bytes == bytes` means only the timestamp moved, which a restore
    /// from backup or a copy that preserved length would do; the bytes may still
    /// be the right ones. Only a re-hash can tell those apart, which is why this
    /// state points at one rather than declaring the file bad.
    Changed { recorded_bytes: u64, bytes: u64 },
    /// A re-hash read this file and it did not match the digest it is pinned to,
    /// and **nothing has written to it since**.
    ///
    /// The strongest negative claim this application makes about a file. Unlike
    /// [`Self::Changed`], which is two integers disagreeing and may be nothing
    /// worse than a restore from backup, this is the whole file having been read
    /// and compared. It is reported rather than inferred: the check that produced
    /// it may have been made weeks ago, and the point of writing it down was that
    /// it should still be here after a restart.
    ///
    /// The qualifier is load-bearing and is why this is not simply "the last
    /// re-check failed". A verdict describes the bytes it was measured against;
    /// once the length or the timestamp moves, those bytes are gone and the
    /// verdict with them, so the state falls back to [`Self::Changed`]. See
    /// [`integrity_of`].
    FailedRecheck { expected: String, actual: String },
}

/// Decide what may be said, from the record and the file as it is now.
///
/// Pure: two `Option`s of plain integers in, one state out. No filesystem, no
/// `AppHandle`, and therefore testable — which for a three-state answer whose
/// whole purpose is to stop one state being rendered as another is the part that
/// had to be reachable by a test.
///
/// The order of the two `None` checks is load-bearing. A deleted file with a
/// leftover record is [`GgufIntegrity::NotInstalled`], not
/// [`GgufIntegrity::Changed`]: there is nothing to have changed, and reporting a
/// change would send someone looking for corruption in a file they removed.
///
/// # Where the verdict is read, and why it is there and not earlier
///
/// **Inside the branch where the two integers still agree, never before it.**
/// This is the one placement decision in KNEG-130 and both halves of it matter:
///
/// - Reading it *only* there is what stops a recorded failure being rendered as
///   [`GgufIntegrity::UnchangedSinceInstall`]. A failed re-check writes the
///   file's **current** length and mtime, so the cheap comparison agrees --
///   which would otherwise have this function make its strongest positive claim
///   about the one file the application has proved is wrong.
/// - Reading it *before* the comparison would be the obvious way to write this
///   and is the wrong one. The verdict describes the bytes that were hashed. Once
///   the length or the timestamp moves, those bytes are gone -- somebody has
///   reinstalled the model, or restored it -- and an accusation carried forward
///   over a file that has since been repaired is a screen nobody can clear.
///   So a moved file falls to [`GgufIntegrity::Changed`] whatever the verdict
///   says, and the next re-hash settles it.
///
/// The branch is chosen by the **recorded** verdict. Nothing here hashes
/// anything; this function stays pure over integers and a small enum.
#[must_use]
pub fn integrity_of(record: Option<&InstallRecord>, observed: Option<Observed>) -> GgufIntegrity {
    let Some(observed) = observed else {
        return GgufIntegrity::NotInstalled;
    };
    let Some(record) = record else {
        return GgufIntegrity::Unrecorded;
    };

    if record.bytes == observed.bytes && record.mtime == observed.mtime {
        match &record.verdict {
            Verdict::Passed => GgufIntegrity::UnchangedSinceInstall {
                verified_by: record.strength(),
            },
            Verdict::Failed { expected, actual } => GgufIntegrity::FailedRecheck {
                expected: expected.clone(),
                actual: actual.clone(),
            },
        }
    } else {
        GgufIntegrity::Changed {
            recorded_bytes: record.bytes,
            bytes: observed.bytes,
        }
    }
}

/* ------------------------------------------------------------ the thin edge */

/// Where the record for one GGUF lives.
#[must_use]
pub fn record_path(gguf: &Path) -> PathBuf {
    match gguf.file_name() {
        Some(name) => {
            let mut name = name.to_os_string();
            name.push(RECORD_SUFFIX);
            gguf.with_file_name(name)
        }
        // A path with no final component (`/`, `..`) is not a model file. The
        // fallback must not be the input itself: `write_record` truncates what
        // it opens, and returning `gguf` here would have it overwrite a model
        // with its own record.
        None => gguf.join(RECORD_SUFFIX),
    }
}

/// The file's length and modification time, or `None` if it is not there.
///
/// A file that exists but whose metadata cannot be read answers `None` — the
/// same as absent. That is deliberate: "present but unreadable" cannot be told
/// apart from "gone" by anything the screen can offer, and the two are one
/// sentence to a user.
#[must_use]
pub fn observe(gguf: &Path) -> Option<Observed> {
    let metadata = std::fs::metadata(gguf).ok()?;
    Some(Observed {
        bytes: metadata.len(),
        mtime: Mtime::of(metadata.modified().ok()?),
    })
}

/// Read the record beside a GGUF, or `None` when there is nothing to read.
///
/// Missing, unreadable, malformed and written-by-a-schema-we-do-not-know all
/// answer `None`, and they are folded together on purpose: each one means the
/// same thing to the decision, which is that nothing is known about this file. A
/// half-written record is not evidence of tampering, and treating it as such
/// would put a warning in front of a user whose machine simply lost power.
///
/// **Both schemas are accepted**, against [`READABLE_SCHEMAS`] rather than
/// against [`SCHEMA`]. A record written before the verdict field existed reads
/// as [`Verdict::Passed`] and keeps saying exactly what it said before the bump;
/// see [`InstallRecord::verdict`]. Comparing against `SCHEMA` alone would have
/// made the version bump silently discard every record already on disk.
#[must_use]
pub fn read_record(gguf: &Path) -> Option<InstallRecord> {
    let raw = std::fs::read_to_string(record_path(gguf)).ok()?;
    let record: InstallRecord = serde_json::from_str(&raw).ok()?;
    READABLE_SCHEMAS.contains(&record.schema).then_some(record)
}

/// Write the record for a GGUF that has just been checked.
///
/// **Call this only after the file is in its final place.** The length and the
/// modification time are read here, from the file itself, so a record written
/// before a rename would describe a path that no longer exists.
///
/// Not written atomically. A torn write is read back as no record at all, which
/// is the safe direction — an interrupted write cannot invent a claim, only lose
/// one.
pub fn write_record(
    gguf: &Path,
    sha256: Option<&str>,
    verified_by: VerifiedBy,
) -> std::io::Result<()> {
    let metadata = std::fs::metadata(gguf)?;
    let record = InstallRecord {
        schema: SCHEMA,
        sha256: sha256.map(str::to_owned),
        verified_by: verified_by.as_str().to_owned(),
        bytes: metadata.len(),
        mtime: Mtime::of(metadata.modified()?),
        checked_at_unix_seconds: chrono::Utc::now().timestamp(),
        verdict: Verdict::Passed,
    };
    write(gguf, &record)
}

/// Record that a re-hash of this file **disagreed** with the digest it is
/// pinned to.
///
/// Called where `forget_record` used to be. Its removal is KNEG-130.
///
/// **The reasoning that produced the deletion is preserved and better served by
/// writing.** The point was never "forget it": it was *never let the screen say
/// unchanged about this file*. A record that survived a mismatch is a record
/// which was wrong -- the length and the mtime say nothing has changed while the
/// bytes say otherwise -- so it could not be left as it was. But deleting it
/// dropped the row to [`GgufIntegrity::Unrecorded`], which this module insists is
/// *not an accusation* and which every GGUF installed before records existed also
/// reports, and it silenced `local_llm::start_model`'s warning, which fires for
/// [`GgufIntegrity::Changed`]. The finding was destroyed by the act of responding
/// to it. Overwriting the record with the verdict satisfies the original
/// requirement and keeps the finding.
///
/// **`verified_by` is written as [`VerifiedBy::Nothing`], deliberately.** Nothing
/// about these bytes was verified; the last thing that looked at them refused
/// them. Carrying the install's old strength forward would put the record's
/// strongest word beside its worst verdict.
///
/// The length and mtime stored are the file's **as it is now**, which is what
/// makes [`GgufIntegrity::FailedRecheck`] mean "and nothing has written to it
/// since" -- see [`integrity_of`].
pub fn write_failed_record(gguf: &Path, expected: &str, actual: &str) -> std::io::Result<()> {
    let metadata = std::fs::metadata(gguf)?;
    let record = InstallRecord {
        schema: SCHEMA,
        sha256: Some(expected.to_owned()),
        verified_by: VerifiedBy::Nothing.as_str().to_owned(),
        bytes: metadata.len(),
        mtime: Mtime::of(metadata.modified()?),
        checked_at_unix_seconds: chrono::Utc::now().timestamp(),
        verdict: Verdict::Failed {
            expected: expected.to_owned(),
            actual: actual.to_owned(),
        },
    };
    write(gguf, &record)
}

/// Serialise one record into place. Shared by both writers so the two cannot
/// disagree about where a record goes or how it is spelled.
fn write(gguf: &Path, record: &InstallRecord) -> std::io::Result<()> {
    let json = serde_json::to_string_pretty(record)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    std::fs::write(record_path(gguf), json)
}

/// The whole answer for one GGUF: read the record, stat the file, decide.
///
/// Two syscalls at worst and one when the model is not installed, which is what
/// makes this affordable on a screen that asks about thirty-nine objects.
#[must_use]
pub fn integrity(gguf: &Path) -> GgufIntegrity {
    let Some(observed) = observe(gguf) else {
        // Nothing there. Reading a record for a file that does not exist would
        // be a second syscall to reach an answer already known.
        return GgufIntegrity::NotInstalled;
    };
    integrity_of(read_record(gguf).as_ref(), Some(observed))
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use std::time::Duration;

    use super::*;

    /// A record asserting a length, a time and a strength, with no file behind
    /// it. The pure tests need facts, not filesystems.
    fn record(bytes: u64, seconds: i64, nanos: u32, verified_by: VerifiedBy) -> InstallRecord {
        InstallRecord {
            schema: SCHEMA,
            sha256: Some("a".repeat(64)),
            verified_by: verified_by.as_str().to_owned(),
            bytes,
            mtime: Mtime { seconds, nanos },
            checked_at_unix_seconds: 1_700_000_000,
            verdict: Verdict::Passed,
        }
    }

    /// The same record, carrying a re-hash that disagreed.
    fn failed(bytes: u64, seconds: i64, nanos: u32) -> InstallRecord {
        InstallRecord {
            verdict: Verdict::Failed {
                expected: "1".repeat(64),
                actual: "2".repeat(64),
            },
            ..record(bytes, seconds, nanos, VerifiedBy::Nothing)
        }
    }

    fn observed(bytes: u64, seconds: i64, nanos: u32) -> Observed {
        Observed {
            bytes,
            mtime: Mtime { seconds, nanos },
        }
    }

    /* ------------------------------------------------------ the pure states */

    /// No file means no file, whatever a leftover record says.
    #[test]
    fn an_absent_file_is_not_installed_even_with_a_record_beside_it() {
        assert_eq!(integrity_of(None, None), GgufIntegrity::NotInstalled);

        let left_behind = record(10, 5, 0, VerifiedBy::Digest);
        assert_eq!(
            integrity_of(Some(&left_behind), None),
            GgufIntegrity::NotInstalled,
            "a record for a deleted file must not report a change; there is \
             nothing left to have changed"
        );
    }

    /// The state this whole issue turns on: no record is its own answer.
    ///
    /// Asserted three ways rather than once. `assert_eq!` alone would pass if
    /// the variant were renamed to something that reads as a verdict, so the two
    /// negatives spell out what it must never be — a file nobody has a record
    /// for is neither proven good nor proven bad, and every GGUF installed
    /// before this landed is in exactly that position.
    #[test]
    fn a_file_with_no_record_is_neither_unchanged_nor_changed() {
        let state = integrity_of(None, Some(observed(1_280_000_000, 1_756_000_000, 0)));

        assert_eq!(state, GgufIntegrity::Unrecorded);
        assert!(
            !matches!(state, GgufIntegrity::UnchangedSinceInstall { .. }),
            "an absent record must never render as verified"
        );
        assert!(
            !matches!(state, GgufIntegrity::Changed { .. }),
            "an absent record must never render as corrupt"
        );
    }

    /// A record that still agrees reports the strength the install earned, and
    /// does not round it up.
    #[test]
    fn a_record_that_agrees_reports_the_strength_the_install_earned() {
        for strength in [
            VerifiedBy::Digest,
            VerifiedBy::StatedDigest,
            VerifiedBy::Size,
            VerifiedBy::Nothing,
        ] {
            let held = record(4_680_000_000, 1_756_000_000, 250, strength);
            assert_eq!(
                integrity_of(
                    Some(&held),
                    Some(observed(4_680_000_000, 1_756_000_000, 250))
                ),
                GgufIntegrity::UnchangedSinceInstall {
                    verified_by: strength
                },
                "{strength:?} was checked at install and must be what is reported"
            );
        }
    }

    /// A length that moved is a change, and both lengths are reported.
    #[test]
    fn a_length_that_moved_is_a_change_that_names_both_lengths() {
        let held = record(4_680_000_000, 1_756_000_000, 0, VerifiedBy::Digest);

        assert_eq!(
            integrity_of(Some(&held), Some(observed(2_000_000, 1_756_000_000, 0))),
            GgufIntegrity::Changed {
                recorded_bytes: 4_680_000_000,
                bytes: 2_000_000,
            },
            "a truncated model is the case this exists for"
        );
    }

    /// A timestamp that moved on its own is still a change, and says so by
    /// reporting the same length twice.
    ///
    /// The pair `recorded_bytes == bytes` is how the screen tells "something
    /// wrote to this" from "this is the wrong size", and it is load-bearing: the
    /// two deserve different sentences, because a restore from backup moves a
    /// timestamp without touching a byte.
    #[test]
    fn a_timestamp_that_moved_alone_is_a_change_with_the_length_unchanged() {
        let held = record(4_680_000_000, 1_756_000_000, 0, VerifiedBy::Digest);

        let state = integrity_of(Some(&held), Some(observed(4_680_000_000, 1_756_000_100, 0)));
        assert_eq!(
            state,
            GgufIntegrity::Changed {
                recorded_bytes: 4_680_000_000,
                bytes: 4_680_000_000,
            }
        );
        match state {
            GgufIntegrity::Changed {
                recorded_bytes,
                bytes,
            } => assert_eq!(recorded_bytes, bytes, "only the timestamp moved"),
            other => panic!("{other:?}"),
        }
    }

    /// The sub-second half of the timestamp is compared too.
    ///
    /// NTFS keeps 100 ns and ext4 keeps nanoseconds, so a comparison that comes
    /// down to whole seconds would call two writes in the same second identical
    /// — which is the window a fast overwrite lands in.
    #[test]
    fn a_nanosecond_of_difference_in_the_timestamp_is_a_change() {
        let held = record(10, 1_756_000_000, 0, VerifiedBy::Digest);
        assert_eq!(
            integrity_of(Some(&held), Some(observed(10, 1_756_000_000, 1))),
            GgufIntegrity::Changed {
                recorded_bytes: 10,
                bytes: 10
            }
        );
    }

    /// The five states are five distinct things on the wire.
    ///
    /// The frontend switches on `state`; two variants that serialised the same
    /// would make one of them unreachable and the other wrong.
    #[test]
    fn the_five_states_are_five_distinct_tags() {
        let tags: Vec<String> = [
            GgufIntegrity::NotInstalled,
            GgufIntegrity::UnchangedSinceInstall {
                verified_by: VerifiedBy::Digest,
            },
            GgufIntegrity::Unrecorded,
            GgufIntegrity::Changed {
                recorded_bytes: 1,
                bytes: 2,
            },
            GgufIntegrity::FailedRecheck {
                expected: "1".repeat(64),
                actual: "2".repeat(64),
            },
        ]
        .iter()
        .map(|s| {
            serde_json::to_value(s).unwrap()["state"]
                .as_str()
                .unwrap()
                .to_owned()
        })
        .collect();

        assert_eq!(
            tags,
            vec![
                "not-installed",
                "unchanged-since-install",
                "unrecorded",
                "changed",
                "failed-recheck"
            ]
        );

        // `failed-recheck` must not be mistakable for `changed`: the first is
        // the whole file having been read, the second is two integers
        // disagreeing, and the screen says very different things about them.
        assert_eq!(
            tags.iter().collect::<std::collections::HashSet<_>>().len(),
            5,
            "two states share a tag, so one of them is unreachable"
        );

        // The name is the claim. `unchanged-since-install` is weaker than
        // `verified` on purpose: two integers cannot support the stronger word,
        // and this asserts nobody has quietly promoted it.
        assert!(
            !tags.iter().any(|t| t.contains("verified")),
            "no state may spell itself 'verified'; only a re-hash earns that"
        );

        let json = serde_json::to_value(GgufIntegrity::Changed {
            recorded_bytes: 4_680_000_000_u64,
            bytes: 2_u64,
        })
        .unwrap();
        assert_eq!(json["recordedBytes"], 4_680_000_000_u64);
        assert_eq!(json["bytes"], 2_u64);

        // Both digests cross, because a message carrying only one of them
        // cannot be acted on -- the same reason `RecheckOutcome::Mismatched`
        // carries the pair.
        let json = serde_json::to_value(GgufIntegrity::FailedRecheck {
            expected: "1".repeat(64),
            actual: "2".repeat(64),
        })
        .unwrap();
        assert_eq!(json["expected"], "1".repeat(64));
        assert_eq!(json["actual"], "2".repeat(64));
        assert_ne!(json["expected"], json["actual"]);
    }

    /* ---------------------------------------------------- the failed verdict */

    /// A recorded failure is reported even though the two integers agree.
    ///
    /// **Half one of the placement decision in [`integrity_of`], and the reason
    /// KNEG-130 exists.** A failed re-check writes the file's *current* length
    /// and mtime, so the cheap comparison agrees -- and a function that read only
    /// those two integers would answer `UnchangedSinceInstall`, which is this
    /// application's strongest positive claim, about the one file it has proved
    /// is wrong.
    ///
    /// The control is the same record with the verdict flipped to `Passed`: it
    /// answers `UnchangedSinceInstall` from identical integers, so this is not
    /// passing because everything answers `FailedRecheck`.
    ///
    /// **Mutation that turns it red:** delete the `match` on `record.verdict` in
    /// [`integrity_of`] and return `UnchangedSinceInstall` unconditionally.
    #[test]
    fn a_recorded_failure_is_reported_even_though_the_file_has_not_moved() {
        let seen = observed(4_680_000_000, 1_756_000_000, 250);

        let held = failed(4_680_000_000, 1_756_000_000, 250);
        let state = integrity_of(Some(&held), Some(seen));

        assert_eq!(
            state,
            GgufIntegrity::FailedRecheck {
                expected: "1".repeat(64),
                actual: "2".repeat(64),
            }
        );
        assert!(
            !matches!(state, GgufIntegrity::UnchangedSinceInstall { .. }),
            "a file a re-hash has refused must never make the strongest claim \
             this application has"
        );

        // The control. Identical bytes, identical timestamp, opposite verdict.
        let passed = record(4_680_000_000, 1_756_000_000, 250, VerifiedBy::Digest);
        assert_eq!(
            integrity_of(Some(&passed), Some(seen)),
            GgufIntegrity::UnchangedSinceInstall {
                verified_by: VerifiedBy::Digest
            },
            "the two integers still decide the passing case; only the verdict \
             separates these two records"
        );
    }

    /// A file written to since the failed check reports the change, not the
    /// stale verdict.
    ///
    /// **Half two of the same decision, and the half that says why the verdict is
    /// read *inside* the branch where the integers agree rather than before it.**
    /// Reading it first is the obvious way to write this and the wrong one: a
    /// verdict describes the bytes it was measured against, and once the length
    /// or the timestamp moves those bytes are gone -- somebody has reinstalled
    /// the model. An accusation carried forward over a file that has since been
    /// repaired is a screen nobody can clear.
    ///
    /// **Mutation that turns it red:** move the `match` on `record.verdict` above
    /// the `record.bytes == observed.bytes` comparison.
    #[test]
    fn a_file_written_to_since_a_failed_check_reports_the_change_not_the_stale_verdict() {
        let held = failed(4_680_000_000, 1_756_000_000, 250);

        // Same length, later timestamp: the model has been put back.
        let state = integrity_of(Some(&held), Some(observed(4_680_000_000, 1_756_000_900, 0)));
        assert_eq!(
            state,
            GgufIntegrity::Changed {
                recorded_bytes: 4_680_000_000,
                bytes: 4_680_000_000,
            },
            "the verdict described bytes that are no longer there"
        );
        assert!(
            !matches!(state, GgufIntegrity::FailedRecheck { .. }),
            "a repaired file must not keep reporting the verdict it earned \
             before it was repaired"
        );

        // The control: unmoved, the verdict still stands. Without it this would
        // pass against an `integrity_of` that never returns `FailedRecheck`.
        assert_eq!(
            integrity_of(
                Some(&held),
                Some(observed(4_680_000_000, 1_756_000_000, 250))
            ),
            GgufIntegrity::FailedRecheck {
                expected: "1".repeat(64),
                actual: "2".repeat(64),
            }
        );
    }

    /// A record whose verdict failed claims no strength.
    ///
    /// The record's `verified_by` is what
    /// [`GgufIntegrity::UnchangedSinceInstall`] reports, so a failed record that
    /// kept the install's old `digest` would be this file's strongest word
    /// sitting beside its worst verdict, one refactor away from being read out.
    #[test]
    fn a_failed_record_claims_no_strength() {
        let dir = tempfile::tempdir().unwrap();
        let gguf = dir.path().join("weights.gguf");
        std::fs::write(&gguf, b"the model").unwrap();

        // The control first: an ordinary install records what it earned.
        write_record(&gguf, Some(&"a".repeat(64)), VerifiedBy::Digest).unwrap();
        assert_eq!(read_record(&gguf).unwrap().strength(), VerifiedBy::Digest);

        write_failed_record(&gguf, &"1".repeat(64), &"2".repeat(64)).unwrap();
        assert_eq!(read_record(&gguf).unwrap().strength(), VerifiedBy::Nothing);
    }

    /// A failed verdict survives the round trip to disk and back.
    ///
    /// The pure tests above prove the decision; this proves the verdict is
    /// actually written, actually parsed back, and reaches the same answer
    /// through the real filesystem edge. Surviving a restart is the whole point
    /// of writing it down.
    #[test]
    fn a_failed_verdict_survives_the_round_trip_to_disk() {
        let dir = tempfile::tempdir().unwrap();
        let gguf = dir.path().join("weights.gguf");
        std::fs::write(&gguf, b"the model").unwrap();

        // The control: the passing writer, on the same file, first.
        write_record(&gguf, Some(&"a".repeat(64)), VerifiedBy::StatedDigest).unwrap();
        assert_eq!(
            integrity(&gguf),
            GgufIntegrity::UnchangedSinceInstall {
                verified_by: VerifiedBy::StatedDigest
            }
        );

        write_failed_record(&gguf, &"1".repeat(64), &"2".repeat(64)).unwrap();
        assert_eq!(
            integrity(&gguf),
            GgufIntegrity::FailedRecheck {
                expected: "1".repeat(64),
                actual: "2".repeat(64),
            }
        );

        // Reading it twice must not change the answer: this runs on every paint.
        assert_eq!(integrity(&gguf), integrity(&gguf));

        // And the file is still the user's.
        assert!(gguf.exists(), "the weights are not ours to delete");
    }

    /* ------------------------------------------------------------ the record */

    /// An unrecognised strength claims nothing rather than guessing.
    #[test]
    fn an_unrecognised_strength_rounds_down_to_nothing() {
        let mut held = record(1, 1, 0, VerifiedBy::Digest);
        held.verified_by = "signed-by-the-publisher".to_owned();
        assert_eq!(held.strength(), VerifiedBy::Nothing);

        held.verified_by = String::new();
        assert_eq!(held.strength(), VerifiedBy::Nothing);

        // Case matters: the wire spelling is lowercase kebab-case, and a record
        // that says `Digest` was not written by this application.
        held.verified_by = "Digest".to_owned();
        assert_eq!(held.strength(), VerifiedBy::Nothing);
    }

    /// A record round-trips through the format it is stored in.
    #[test]
    fn a_record_survives_the_json_it_is_stored_as() {
        let held = record(
            4_680_000_000,
            1_756_000_000,
            123_456_789,
            VerifiedBy::Digest,
        );
        let json = serde_json::to_string(&held).unwrap();

        // camelCase, because a person reading their own model directory should
        // see the same field names the rest of this application uses.
        assert!(json.contains("\"verifiedBy\""), "{json}");
        assert!(json.contains("\"checkedAtUnixSeconds\""), "{json}");

        assert_eq!(serde_json::from_str::<InstallRecord>(&json).unwrap(), held);
    }

    /// A record written before the verdict field existed still reads as passed.
    ///
    /// **The backward-compatibility claim, proved against a fixture rather than
    /// against a fresh install.** `SCHEMA` moved 1 -> 2 for KNEG-130, and the
    /// old `record.schema == SCHEMA` test would have discarded every record
    /// already on disk -- dropping every installed model from *unchanged since
    /// it was installed* to *installed, never recorded* for no reason a user
    /// could see, which is the exact overclaim-in-reverse this module exists to
    /// avoid.
    ///
    /// The JSON is written out by hand, with no `verdict` key, because a record
    /// built through `InstallRecord { .. }` would acquire the field from the
    /// struct and prove nothing about what is on somebody's disk today.
    ///
    /// **Mutation that turns it red:** `READABLE_SCHEMAS` back to `[SCHEMA]`.
    #[test]
    fn a_schema_one_record_still_reads_as_passed_with_no_verdict_recorded() {
        let dir = tempfile::tempdir().unwrap();
        let gguf = dir.path().join("weights.gguf");
        std::fs::write(&gguf, vec![0x47_u8; 4096]).unwrap();

        // The record has to describe this file exactly, or the answer would be
        // `Changed` for a reason that has nothing to do with the schema.
        let meta = std::fs::metadata(&gguf).unwrap();
        let mtime = Mtime::of(meta.modified().unwrap());
        let schema_one = format!(
            r#"{{
  "schema": 1,
  "sha256": "{digest}",
  "verifiedBy": "stated-digest",
  "bytes": {bytes},
  "mtime": {{ "seconds": {seconds}, "nanos": {nanos} }},
  "checkedAtUnixSeconds": 1756000000
}}"#,
            digest = "a".repeat(64),
            bytes = meta.len(),
            seconds = mtime.seconds,
            nanos = mtime.nanos,
        );
        assert!(
            !schema_one.contains("verdict"),
            "the fixture must be what a pre-KNEG-130 build wrote"
        );
        std::fs::write(record_path(&gguf), &schema_one).unwrap();

        let read = read_record(&gguf).expect("a schema 1 record is still readable");
        assert_eq!(read.verdict, Verdict::Passed);
        assert_eq!(
            integrity(&gguf),
            GgufIntegrity::UnchangedSinceInstall {
                verified_by: VerifiedBy::StatedDigest
            },
            "a record written before the bump must keep saying what it said"
        );

        // The control, in the same test: a schema this build does not know is
        // still no record at all, so the acceptance above is a decision and not
        // serde accepting anything put in front of it.
        let schema_three = schema_one.replace("\"schema\": 1", "\"schema\": 3");
        assert_ne!(schema_three, schema_one, "the fixture edit did nothing");
        std::fs::write(record_path(&gguf), schema_three).unwrap();
        assert!(read_record(&gguf).is_none());
        assert_eq!(integrity(&gguf), GgufIntegrity::Unrecorded);
    }

    /// A record from a schema this build does not know is no record at all.
    #[test]
    fn a_record_from_another_schema_is_read_as_no_record() {
        let dir = tempfile::tempdir().unwrap();
        let gguf = dir.path().join("weights.gguf");
        std::fs::write(&gguf, b"weights").unwrap();

        let mut held = record(7, 0, 0, VerifiedBy::Digest);
        held.schema = SCHEMA + 1;
        std::fs::write(record_path(&gguf), serde_json::to_string(&held).unwrap()).unwrap();

        assert!(read_record(&gguf).is_none());
        assert_eq!(
            integrity(&gguf),
            GgufIntegrity::Unrecorded,
            "a record we cannot read is nothing known, not a change"
        );
    }

    /// Malformed JSON beside a model is nothing known, not an accusation.
    #[test]
    fn an_unparseable_record_is_read_as_no_record() {
        let dir = tempfile::tempdir().unwrap();
        let gguf = dir.path().join("weights.gguf");
        std::fs::write(&gguf, b"weights").unwrap();
        std::fs::write(record_path(&gguf), b"{ half a fi").unwrap();

        assert!(read_record(&gguf).is_none());
        assert_eq!(integrity(&gguf), GgufIntegrity::Unrecorded);
    }

    /// The record sits beside the model and never on top of it.
    #[test]
    fn the_record_never_lands_on_the_model_itself() {
        let gguf = Path::new("/models/qwen2.5-1.5b-instruct-q5_k_m.gguf");
        let record = record_path(gguf);

        assert_ne!(record, gguf, "the record would overwrite the model");
        assert_eq!(record.parent(), gguf.parent(), "beside it, not elsewhere");
        assert!(
            record
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.ends_with(RECORD_SUFFIX)),
            "{record:?}"
        );

        // Two models in one directory get two records. A shared name would have
        // the second install silently overwrite the first model's claim.
        assert_ne!(
            record_path(Path::new("/models/a.gguf")),
            record_path(Path::new("/models/b.gguf"))
        );

        // A path with no final component cannot be written over either.
        let rootish = Path::new("/");
        assert_ne!(record_path(rootish), rootish);
    }

    /* -------------------------------------------------- against real files */

    /// The whole cycle, on a real file: recorded, changed, refused, deleted.
    ///
    /// The pure tests above prove the decision; this proves the two integers
    /// reaching it are the ones the filesystem actually reports. Those are
    /// different claims, and the second is the one that would break on a
    /// platform whose mtime resolution is coarser than the write it follows.
    #[test]
    fn a_real_file_walks_all_five_states() {
        let dir = tempfile::tempdir().unwrap();
        let gguf = dir.path().join("weights.gguf");

        // 1. Nothing there.
        assert_eq!(integrity(&gguf), GgufIntegrity::NotInstalled);

        // 2. Installed, nothing recorded — the state every GGUF on disk today
        //    is in.
        std::fs::write(&gguf, vec![0x47_u8; 4096]).unwrap();
        assert_eq!(integrity(&gguf), GgufIntegrity::Unrecorded);

        // 3. Recorded.
        write_record(&gguf, Some(&"c".repeat(64)), VerifiedBy::StatedDigest).unwrap();
        assert_eq!(
            integrity(&gguf),
            GgufIntegrity::UnchangedSinceInstall {
                verified_by: VerifiedBy::StatedDigest
            }
        );

        // Reading it twice must not change the answer: this is a status call,
        // and it runs every time the screen opens.
        assert_eq!(integrity(&gguf), integrity(&gguf));

        // 4. Truncated behind the application's back.
        std::fs::write(&gguf, vec![0x47_u8; 2048]).unwrap();
        match integrity(&gguf) {
            GgufIntegrity::Changed {
                recorded_bytes,
                bytes,
            } => {
                assert_eq!(recorded_bytes, 4096);
                assert_eq!(bytes, 2048);
            }
            other => panic!("a truncated file read as {other:?}"),
        }

        // 5. A re-hash refuses it. The verdict replaces the record rather than
        //    deleting it -- deleting dropped the row to `Unrecorded`, which is
        //    this module's *nothing is known* answer, and silenced the one
        //    warning the start path emits. KNEG-130.
        write_failed_record(&gguf, &"1".repeat(64), &"2".repeat(64)).unwrap();
        assert_eq!(
            integrity(&gguf),
            GgufIntegrity::FailedRecheck {
                expected: "1".repeat(64),
                actual: "2".repeat(64),
            }
        );
        assert_ne!(
            integrity(&gguf),
            GgufIntegrity::Unrecorded,
            "a refused file must not read like one nobody has looked at"
        );

        // 6. The model removed. A stale record must not resurrect it, and a
        //    stale *failed* record must not accuse a file that is not there.
        std::fs::remove_file(&gguf).unwrap();
        assert_eq!(integrity(&gguf), GgufIntegrity::NotInstalled);
    }

    /// A rewrite of the same length is caught by the timestamp, on a real disk.
    ///
    /// This is the half of the guarantee that is not the length, and the half
    /// most likely to be lost to a filesystem with a coarse clock. The sleep is
    /// deliberate and short: NTFS and HFS+ both keep sub-millisecond resolution,
    /// but a filesystem stamping whole seconds would need it, and a test that
    /// passes only because the two writes landed in different seconds by luck is
    /// worse than one that waits.
    #[test]
    fn a_same_length_rewrite_is_caught_by_the_timestamp() {
        let dir = tempfile::tempdir().unwrap();
        let gguf = dir.path().join("weights.gguf");

        std::fs::write(&gguf, b"aaaaaaaa").unwrap();
        write_record(&gguf, Some(&"d".repeat(64)), VerifiedBy::Digest).unwrap();

        std::thread::sleep(Duration::from_millis(1100));
        std::fs::write(&gguf, b"bbbbbbbb").unwrap();

        assert_eq!(
            integrity(&gguf),
            GgufIntegrity::Changed {
                recorded_bytes: 8,
                bytes: 8
            },
            "different bytes, same length — the timestamp is the only witness"
        );
    }

    /* ------------------------------------------------------------- the clock */

    /// The epoch, and a time on each side of it, decompose and compare.
    #[test]
    fn a_time_before_the_epoch_normalises_with_a_positive_remainder() {
        assert_eq!(
            Mtime::of(SystemTime::UNIX_EPOCH),
            Mtime {
                seconds: 0,
                nanos: 0
            }
        );

        assert_eq!(
            Mtime::of(SystemTime::UNIX_EPOCH + Duration::new(5, 250_000_000)),
            Mtime {
                seconds: 5,
                nanos: 250_000_000
            }
        );

        // A quarter second before the epoch is minus one second plus three
        // quarters, so the remainder never goes negative and derived equality
        // stays usable.
        assert_eq!(
            Mtime::of(SystemTime::UNIX_EPOCH - Duration::new(0, 250_000_000)),
            Mtime {
                seconds: -1,
                nanos: 750_000_000
            }
        );

        // Whole seconds before the epoch borrow nothing.
        assert_eq!(
            Mtime::of(SystemTime::UNIX_EPOCH - Duration::new(5, 0)),
            Mtime {
                seconds: -5,
                nanos: 0
            }
        );
    }

    /// Two different instants never decompose to the same `Mtime`.
    ///
    /// The normalisation borrows a second, and a borrow that landed on the wrong
    /// side would map two distinct times onto one value — which is a change this
    /// module would then not see.
    ///
    /// **A microsecond, not a nanosecond, and that is a measurement rather than
    /// a guess.** Written with `Duration::new(0, 1)` this test failed on Windows
    /// with "`Mtime { seconds: 0, nanos: 0 }` was produced twice": `SystemTime`
    /// there is a `FILETIME`, which counts in 100 ns ticks, so one nanosecond
    /// either side of the epoch *is* the epoch. That is a limit of the input and
    /// not of this function, and it is the same 100 ns NTFS stores an mtime at —
    /// so nothing finer than that could reach here from a real file anyway.
    #[test]
    fn distinct_instants_stay_distinct_after_decomposition() {
        let instants = [
            SystemTime::UNIX_EPOCH - Duration::new(1, 0),
            SystemTime::UNIX_EPOCH - Duration::new(0, 1_000),
            SystemTime::UNIX_EPOCH,
            SystemTime::UNIX_EPOCH + Duration::new(0, 1_000),
            SystemTime::UNIX_EPOCH + Duration::new(1, 0),
        ];
        let mut seen: Vec<Mtime> = Vec::new();
        for instant in instants {
            let decomposed = Mtime::of(instant);
            assert!(
                !seen.contains(&decomposed),
                "{decomposed:?} was produced twice"
            );
            assert!(
                decomposed.nanos < NANOS_PER_SECOND,
                "{decomposed:?} is not normalised"
            );
            seen.push(decomposed);
        }
    }
}
