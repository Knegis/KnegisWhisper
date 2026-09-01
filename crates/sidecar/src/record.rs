//! What was installed, written down beside the install.
//!
//! [`crate::locate`] proves a sidecar's executable **exists**. It says nothing
//! about which build it is, and until this module there was nothing that did:
//! the manifest's digest was checked against the archive at download time and
//! then forgotten, so after the first install the application could never
//! notice that what it was about to run was two releases behind the manifest it
//! ships. It did not notice on the machine this was written on, where the
//! installed `whisper` sidecar dated from 2026-08-25 and the pinned archive had
//! since gained two more executables. See KNEG-113.
//!
//! So `install` now writes an [`InstallRecord`] into the directory it promotes,
//! and [`freshness`] compares the digest in it against the digest this build
//! pins.
//!
//! # What the record proves, and what it does not
//!
//! It proves **this directory was produced by extracting the archive with that
//! SHA-256**, because the only thing that writes it is the install that just
//! verified that archive, and it is promoted into place by the same rename as
//! the files it describes — there is no window in which one is present without
//! the other.
//!
//! It does **not** prove the files on disk are still those bytes. Nothing here
//! re-hashes an installed executable, and a record is an ordinary JSON file
//! that anything with write access to the directory could rewrite. It is a
//! staleness check, not a tamper check, and reading it as the second would be
//! exactly the kind of rounding-up this project keeps out of its status types.
//!
//! # Absence is *unknown*
//!
//! An install that predates this module has no record. That is neither "stale"
//! nor "current" — nobody measured it — and [`Freshness::Unknown`] is a
//! first-class answer rather than a default to one of the other two. The
//! machine this was written on has such an install right now, which is the
//! whole reason the distinction is drawn: calling it current would excuse a
//! binary demonstrably behind the manifest, and calling it stale would claim a
//! comparison that never happened.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::manifest::{Asset, Sha256Digest, SidecarSpec};
use crate::resolve::Installation;

/// The file name a record is written under, inside the install directory.
///
/// Inside rather than beside, so it is promoted by the same `rename` that puts
/// the files in place and cannot survive a directory it does not describe. The
/// leading dot keeps it out of the way of the archive's own names; nothing in
/// [`crate::locate`] or [`crate::extract`] looks at it.
pub const RECORD_FILE: &str = ".kw-install.json";

/// Bump when the shape changes in a way an older build could misread.
///
/// A record from a schema this build does not understand is reported as
/// [`Unrecorded::UnsupportedSchema`] — unknown — rather than parsed on the hope
/// that the fields still mean what they used to.
pub const SCHEMA: u32 = 1;

/// What one install put on disk.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallRecord {
    pub schema: u32,
    /// Matches [`SidecarSpec::name`]. Checked, because a record copied from one
    /// sidecar's directory into another's would otherwise compare two unrelated
    /// digests and report the answer as though it meant something.
    pub sidecar: String,
    /// The SHA-256 of the archive this install was extracted from. The identity.
    pub asset_sha256: Sha256Digest,
    /// The executable's name on disk, for a human reading the file.
    pub executable: String,
    /// Where the archive came from, as the manifest wrote it.
    pub url: String,
    /// The upstream project and release, copied from [`crate::Source`].
    pub source_repo: String,
    pub source_release: String,
    /// Seconds since the Unix epoch when this was written.
    ///
    /// Zero if the clock was set before 1970, which is reported as a timestamp
    /// rather than as an error: nothing decides anything on this field, and a
    /// failed install would be a poor way to learn the clock is wrong.
    pub installed_at_unix: u64,
}

impl InstallRecord {
    /// The record for an install of `asset`, as of now.
    #[must_use]
    pub fn of(spec: &SidecarSpec, asset: &Asset) -> Self {
        Self::at(spec, asset, now_unix())
    }

    /// The record for an install of `asset` at a stated time.
    ///
    /// Split from [`Self::of`] so the timestamp is an argument in tests rather
    /// than whatever the clock says while they run.
    #[must_use]
    pub fn at(spec: &SidecarSpec, asset: &Asset, installed_at_unix: u64) -> Self {
        Self {
            schema: SCHEMA,
            sidecar: spec.name.clone(),
            asset_sha256: asset.sha256.clone(),
            executable: asset.executable_name().to_owned(),
            url: asset.url.clone(),
            source_repo: spec.source.repo.clone(),
            source_release: spec.source.release.clone(),
            installed_at_unix,
        }
    }
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs())
}

/// What was found where a record should be.
///
/// Three states rather than `Option<InstallRecord>`. "No file" and "a file this
/// build cannot read" both end in [`Freshness::Unknown`], but they are
/// different problems with different fixes — one is an install from an older
/// build, the other is corruption — and folding them together would hide the
/// second behind the first, which is much the commoner of the two.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Record {
    Present(Box<InstallRecord>),
    /// No record file at all. An install from before records existed, or one
    /// somebody assembled by hand.
    Absent,
    /// A file is there and this build could not make sense of it.
    Unreadable {
        detail: String,
    },
}

/// Read the record from an install directory. Never fails: an unreadable record
/// is an answer.
#[must_use]
pub fn read(install_dir: &Path) -> Record {
    let path = install_dir.join(RECORD_FILE);
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Record::Absent,
        Err(err) => {
            return Record::Unreadable {
                detail: err.to_string(),
            }
        }
    };

    match serde_json::from_str::<InstallRecord>(&text) {
        Ok(record) => Record::Present(Box::new(record)),
        Err(err) => Record::Unreadable {
            detail: err.to_string(),
        },
    }
}

/// Write a record into a directory, replacing any record already there.
///
/// Called on the staging directory before it is promoted, so the record lands
/// atomically with the files it describes.
pub fn write(dir: &Path, record: &InstallRecord) -> Result<PathBuf, io::Error> {
    let path = dir.join(RECORD_FILE);
    let json = serde_json::to_string_pretty(record)
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
    fs::write(&path, json.as_bytes())?;
    Ok(path)
}

/// Why the identity of an install could not be established.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unrecorded {
    /// Installed before this application wrote records.
    NoRecord,
    /// A record is present and could not be read.
    Unreadable { detail: String },
    /// A record is present and was written by a schema this build does not know.
    UnsupportedSchema { found: u32 },
    /// A record is present and describes a different sidecar.
    DifferentSidecar { recorded: String },
}

impl Unrecorded {
    /// A sentence for the user.
    #[must_use]
    pub fn explain(&self) -> String {
        match self {
            Self::NoRecord => {
                "it was installed before this application started recording what it installs"
                    .to_owned()
            }
            Self::Unreadable { detail } => {
                format!("the record of what was installed could not be read: {detail}")
            }
            Self::UnsupportedSchema { found } => format!(
                "the record of what was installed is schema {found}, and this build \
                 understands schema {SCHEMA}"
            ),
            Self::DifferentSidecar { recorded } => format!(
                "the record in that directory says it holds {recorded}, which is not what \
                 was looked for"
            ),
        }
    }
}

/// Whether what is installed is what this build pins.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Freshness {
    /// Nothing usable is installed, so there is nothing to be stale.
    NotInstalled,
    /// The recorded digest is the digest this build pins.
    Current,
    /// The recorded digest is a different one.
    Stale {
        installed: Sha256Digest,
        expected: Sha256Digest,
    },
    /// Something is installed and its identity is not established. **Not a
    /// synonym for stale.** See the module documentation.
    Unknown(Unrecorded),
}

impl Freshness {
    /// Whether the user should be told about this.
    ///
    /// True for [`Self::Stale`] and [`Self::Unknown`] and false for the other
    /// two, which is the one judgement in this type: an install nobody can
    /// identify is worth a sentence on screen, because the alternative is
    /// running it silently, which is the state KNEG-113 was filed about.
    #[must_use]
    pub const fn worth_reporting(&self) -> bool {
        matches!(self, Self::Stale { .. } | Self::Unknown(_))
    }

    #[must_use]
    pub const fn is_current(&self) -> bool {
        matches!(self, Self::Current)
    }

    /// A sentence for the user, naming what is known and what is not.
    #[must_use]
    pub fn explain(&self, sidecar: &str) -> String {
        match self {
            Self::NotInstalled => format!("{sidecar} is not installed."),
            Self::Current => {
                format!("{sidecar} is the build this version of the application pins.")
            }
            Self::Stale {
                installed,
                expected,
            } => format!(
                "{sidecar} on disk was installed from {installed}, and this version pins \
                 {expected}. Updating it will download the pinned build and replace it."
            ),
            Self::Unknown(why) => format!(
                "{sidecar} is installed and it is not known whether it is the build this \
                 version pins: {}. Updating it will download the pinned build and replace it.",
                why.explain()
            ),
        }
    }
}

/// Is what is on disk the thing this build ships with?
///
/// Pure: every fact it decides on is an argument. Reading the directory and
/// asking the manifest happen in the caller, for the reason `media.rs`
/// records — a decision that can only be reached through an `AppHandle` is a
/// decision nothing can test.
#[must_use]
pub fn freshness(
    installation: &Installation,
    record: &Record,
    sidecar: &str,
    expected: &Sha256Digest,
) -> Freshness {
    // An `Incomplete` directory can hold a record — the record is written
    // before the promote, and a later hand-deletion of the executable leaves it
    // behind. It is still not an install, and answering `Current` for a
    // directory with no executable in it would be a claim about a binary that
    // is not there.
    if installation.executable().is_none() {
        return Freshness::NotInstalled;
    }

    let found = match record {
        Record::Absent => return Freshness::Unknown(Unrecorded::NoRecord),
        Record::Unreadable { detail } => {
            return Freshness::Unknown(Unrecorded::Unreadable {
                detail: detail.clone(),
            })
        }
        Record::Present(found) => found,
    };

    if found.schema != SCHEMA {
        return Freshness::Unknown(Unrecorded::UnsupportedSchema {
            found: found.schema,
        });
    }
    if found.sidecar != sidecar {
        return Freshness::Unknown(Unrecorded::DifferentSidecar {
            recorded: found.sidecar.clone(),
        });
    }

    if found.asset_sha256 == *expected {
        Freshness::Current
    } else {
        Freshness::Stale {
            installed: found.asset_sha256.clone(),
            expected: expected.clone(),
        }
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
    use std::collections::BTreeMap;

    use super::*;
    use crate::manifest::{ArchiveKind, Availability, PortRange, Source, Transport};

    const PINNED: &str = "c2a4b60edb11f7e11a9191ffb50929535527d4d91c9903dbe3e554583bbbc63d";
    const OTHER: &str = "1111111111111111111111111111111111111111111111111111111111111111";

    fn digest(hex: &str) -> Sha256Digest {
        Sha256Digest::parse(hex).unwrap()
    }

    fn asset(hex: &str) -> Asset {
        Asset {
            url: "https://example.invalid/whisper-bin-x64.zip".to_owned(),
            archive: ArchiveKind::Zip,
            sha256: digest(hex),
            size_bytes: 10,
            max_extracted_size_bytes: 10,
            executable_in_archive: "Release/whisper-server.exe".to_owned(),
        }
    }

    fn spec(name: &str, hex: &str) -> SidecarSpec {
        let mut targets = BTreeMap::new();
        targets.insert(
            crate::manifest::Target::WindowsX8664,
            Availability::Published(asset(hex)),
        );
        SidecarSpec {
            name: name.to_owned(),
            description: "a test sidecar".to_owned(),
            source: Source {
                repo: "ggml-org/whisper.cpp".to_owned(),
                release: "b4938".to_owned(),
                pinned_because: "it is the one that was reviewed".to_owned(),
            },
            transport: Transport::Loopback {
                port_range: PortRange {
                    first: 8178,
                    last: 8188,
                },
            },
            command_fragments: vec!["whisper-server".to_owned()],
            targets,
        }
    }

    fn installed() -> Installation {
        Installation::Installed {
            executable: PathBuf::from("/data/sidecars/whisper/whisper-server.exe"),
            origin: "downloaded",
        }
    }

    fn present(record: InstallRecord) -> Record {
        Record::Present(Box::new(record))
    }

    /* ------------------------------------------------------- the decision */

    #[test]
    fn a_record_naming_the_pinned_digest_is_current() {
        let record = InstallRecord::at(&spec("whisper", PINNED), &asset(PINNED), 1_756_000_000);
        assert_eq!(
            freshness(&installed(), &present(record), "whisper", &digest(PINNED)),
            Freshness::Current
        );
    }

    #[test]
    fn a_record_naming_another_digest_is_stale_and_names_both() {
        // Both digests, because "out of date" without saying what is on disk
        // gives the person reading a bug report nothing to check against.
        let record = InstallRecord::at(&spec("whisper", OTHER), &asset(OTHER), 1_756_000_000);
        let verdict = freshness(&installed(), &present(record), "whisper", &digest(PINNED));
        assert_eq!(
            verdict,
            Freshness::Stale {
                installed: digest(OTHER),
                expected: digest(PINNED),
            }
        );
        assert!(verdict.worth_reporting());
        assert!(verdict.explain("whisper").contains(OTHER));
        assert!(verdict.explain("whisper").contains(PINNED));
    }

    /// The state the machine this was written on is in right now.
    ///
    /// Its `whisper` sidecar was installed on 2026-08-25 by a build that wrote
    /// no records. Calling it current would excuse a binary the pinned archive
    /// has since moved past; calling it stale would claim a comparison nobody
    /// made. Neither is true, so neither is what this answers.
    #[test]
    fn an_install_with_no_record_is_unknown_and_not_stale_and_not_current() {
        let verdict = freshness(&installed(), &Record::Absent, "whisper", &digest(PINNED));
        assert_eq!(verdict, Freshness::Unknown(Unrecorded::NoRecord));
        assert!(!verdict.is_current());
        assert!(!matches!(verdict, Freshness::Stale { .. }));
        assert!(verdict.worth_reporting());
        assert!(verdict.explain("whisper").contains("not known whether"));
    }

    #[test]
    fn an_unreadable_record_is_unknown_and_says_why() {
        let verdict = freshness(
            &installed(),
            &Record::Unreadable {
                detail: "expected value at line 1 column 1".to_owned(),
            },
            "whisper",
            &digest(PINNED),
        );
        match &verdict {
            Freshness::Unknown(Unrecorded::Unreadable { detail }) => {
                assert!(detail.contains("line 1"));
            }
            other => panic!("{other:?}"),
        }
        // Distinguishable from a missing one: different problem, different fix.
        assert_ne!(verdict, Freshness::Unknown(Unrecorded::NoRecord));
    }

    #[test]
    fn a_record_from_a_schema_this_build_does_not_know_is_unknown() {
        // Not parsed on the hope the fields still mean what they used to. A
        // schema number that is ignored is a schema number that does nothing.
        let mut record = InstallRecord::at(&spec("whisper", PINNED), &asset(PINNED), 0);
        record.schema = SCHEMA + 1;
        assert_eq!(
            freshness(&installed(), &present(record), "whisper", &digest(PINNED)),
            Freshness::Unknown(Unrecorded::UnsupportedSchema { found: SCHEMA + 1 })
        );
    }

    #[test]
    fn a_record_describing_a_different_sidecar_is_unknown_rather_than_stale() {
        // Two unrelated digests compare unequal, so without this check a record
        // copied between directories would report `Stale` -- a definite claim
        // built on a comparison of two things that were never comparable.
        let record = InstallRecord::at(&spec("llama", OTHER), &asset(OTHER), 0);
        assert_eq!(
            freshness(&installed(), &present(record), "whisper", &digest(PINNED)),
            Freshness::Unknown(Unrecorded::DifferentSidecar {
                recorded: "llama".to_owned(),
            })
        );
    }

    #[test]
    fn nothing_installed_is_not_stale_however_good_the_record_is() {
        // A record can outlive the executable it describes: it is written
        // before the promote, and someone can delete the `.exe` afterwards.
        let record = InstallRecord::at(&spec("whisper", PINNED), &asset(PINNED), 0);
        for installation in [
            Installation::NotInstalled {
                looked_in: vec![PathBuf::from("/data/sidecars/whisper")],
            },
            Installation::Incomplete {
                dir: PathBuf::from("/data/sidecars/whisper"),
                expected: PathBuf::from("/data/sidecars/whisper/whisper-server.exe"),
            },
        ] {
            assert_eq!(
                freshness(
                    &installation,
                    &present(record.clone()),
                    "whisper",
                    &digest(PINNED)
                ),
                Freshness::NotInstalled
            );
        }
    }

    #[test]
    fn only_stale_and_unknown_are_worth_saying_out_loud() {
        assert!(!Freshness::Current.worth_reporting());
        assert!(!Freshness::NotInstalled.worth_reporting());
        assert!(Freshness::Unknown(Unrecorded::NoRecord).worth_reporting());
        assert!(Freshness::Stale {
            installed: digest(OTHER),
            expected: digest(PINNED),
        }
        .worth_reporting());
    }

    /* ---------------------------------------------------------- the file */

    #[test]
    fn a_record_written_is_a_record_read() {
        let dir = tempfile::tempdir().unwrap();
        let record = InstallRecord::at(&spec("whisper", PINNED), &asset(PINNED), 1_756_000_000);
        let path = write(dir.path(), &record).unwrap();
        assert_eq!(path.file_name().unwrap(), RECORD_FILE);
        assert_eq!(read(dir.path()), present(record));
    }

    #[test]
    fn a_directory_with_no_record_reads_as_absent_rather_than_as_an_error() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(read(dir.path()), Record::Absent);
    }

    #[test]
    fn a_record_that_is_not_json_reads_as_unreadable() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join(RECORD_FILE), b"not json at all").unwrap();
        assert!(matches!(read(dir.path()), Record::Unreadable { .. }));
    }

    #[test]
    fn a_record_with_a_malformed_digest_reads_as_unreadable_not_as_a_digest() {
        // `Sha256Digest` refuses anything that is not 64 lowercase hex
        // characters, and that refusal has to reach here as "cannot read this"
        // rather than as a digest that compares unequal and reports `Stale`.
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join(RECORD_FILE),
            br#"{"schema":1,"sidecar":"whisper","assetSha256":"ABC","executable":"x.exe",
                "url":"https://example.invalid/x.zip","sourceRepo":"a/b","sourceRelease":"c",
                "installedAtUnix":0}"#,
        )
        .unwrap();
        assert!(matches!(read(dir.path()), Record::Unreadable { .. }));
    }

    #[test]
    fn the_record_is_camel_case_on_disk() {
        // Read by a person debugging an install, and by the same reader across
        // versions. The spelling is part of the format.
        let record = InstallRecord::at(&spec("whisper", PINNED), &asset(PINNED), 1_756_000_000);
        let json = serde_json::to_string(&record).unwrap();
        assert!(json.contains("\"assetSha256\""), "{json}");
        assert!(json.contains("\"installedAtUnix\""), "{json}");
        assert!(json.contains("\"sourceRelease\":\"b4938\""), "{json}");
    }

    #[test]
    fn the_record_carries_the_pinned_digest_and_not_a_recomputed_one() {
        // `of` must take the digest from the manifest asset. Recomputing it
        // from the download would make the record agree with whatever arrived,
        // which is the one thing it must not do.
        let record = InstallRecord::of(&spec("whisper", PINNED), &asset(PINNED));
        assert_eq!(record.asset_sha256, digest(PINNED));
        assert_eq!(record.executable, "whisper-server.exe");
        assert_eq!(record.source_release, "b4938");
    }
}
