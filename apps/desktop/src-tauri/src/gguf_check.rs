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
//! # Three states, not two
//!
//! A boolean over a path cannot tell *checked and unchanged* from *nothing was
//! ever recorded*, and rendering the second as the first is the same defect this
//! module exists to remove, one layer up. So [`integrity_of`] answers four ways,
//! and **the absence of a record is its own answer** — never "verified", never
//! "corrupt". Every GGUF installed before this landed is in that state, and that
//! is not a fault in the file.
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

/// The only record layout this build understands.
///
/// A record carrying anything else is read as no record at all rather than
/// interpreted optimistically: a future version that changes what `bytes` means
/// must not have this one agreeing with it by accident.
const SCHEMA: u32 = 1;

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
/// Four answers, and the third is the reason this is not a boolean. See the
/// module documentation for what each one is worth.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
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
#[must_use]
pub fn integrity_of(record: Option<&InstallRecord>, observed: Option<Observed>) -> GgufIntegrity {
    let Some(observed) = observed else {
        return GgufIntegrity::NotInstalled;
    };
    let Some(record) = record else {
        return GgufIntegrity::Unrecorded;
    };

    if record.bytes == observed.bytes && record.mtime == observed.mtime {
        GgufIntegrity::UnchangedSinceInstall {
            verified_by: record.strength(),
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
/// Missing, unreadable, malformed and written-by-a-future-schema all answer
/// `None`, and they are folded together on purpose: each one means the same
/// thing to the decision, which is that nothing is known about this file. A
/// half-written record is not evidence of tampering, and treating it as such
/// would put a warning in front of a user whose machine simply lost power.
#[must_use]
pub fn read_record(gguf: &Path) -> Option<InstallRecord> {
    let raw = std::fs::read_to_string(record_path(gguf)).ok()?;
    let record: InstallRecord = serde_json::from_str(&raw).ok()?;
    (record.schema == SCHEMA).then_some(record)
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
    };
    let json = serde_json::to_string_pretty(&record)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    std::fs::write(record_path(gguf), json)
}

/// Remove the record beside a GGUF, if there is one.
///
/// Called when a re-hash disagrees with the pinned digest. A record that
/// survived *that* is a record which was wrong — the file's length and mtime say
/// nothing has changed while its bytes say otherwise — and leaving it in place
/// would have the screen answer "unchanged since it was installed" about a file
/// this application has just proved is not the one that was installed.
///
/// Absent afterwards is the only outcome that matters, so a missing file is not
/// an error and neither is a failure to delete one that reports back as gone.
pub fn forget_record(gguf: &Path) -> std::io::Result<()> {
    match std::fs::remove_file(record_path(gguf)) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        other => other,
    }
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

    /// The four states are four distinct things on the wire.
    ///
    /// The frontend switches on `state`; two variants that serialised the same
    /// would make one of them unreachable and the other wrong.
    #[test]
    fn the_four_states_are_four_distinct_tags() {
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
                "changed"
            ]
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

    /// The whole cycle, on a real file: recorded, changed, forgotten, deleted.
    ///
    /// The pure tests above prove the decision; this proves the two integers
    /// reaching it are the ones the filesystem actually reports. Those are
    /// different claims, and the second is the one that would break on a
    /// platform whose mtime resolution is coarser than the write it follows.
    #[test]
    fn a_real_file_walks_all_four_states() {
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

        // 5. The record removed, as a failed re-hash does.
        forget_record(&gguf).unwrap();
        assert_eq!(integrity(&gguf), GgufIntegrity::Unrecorded);
        forget_record(&gguf).unwrap(); // idempotent

        // 6. The model removed. A stale record must not resurrect it.
        write_record(&gguf, None, VerifiedBy::Size).unwrap();
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
