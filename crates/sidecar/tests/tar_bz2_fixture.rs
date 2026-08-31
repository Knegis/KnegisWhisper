//! Decodes a `.tar.bz2` that `bzip2-rs` could not possibly have produced.
//!
//! `bzip2-rs` is decode-only -- it has no encoder at all, unlike `lzma-rs`,
//! which the `tar.xz` fixture test has to check by hand against the
//! encoder's known header shape (see `tar_xz_fixture.rs`). There is no
//! equivalent risk here: this crate cannot generate a `.tar.bz2` stream by
//! any path, so any real bzip2 file decoding correctly is evidence the
//! decoder reads the actual format, not evidence it agrees with itself.
//!
//! This fixture was built with Python's `tarfile` module and then compressed
//! with the standalone `bzip2` command-line tool (bzip2 1.0.8, found on this
//! development machine at `C:\Program Files\Git\mingw64\bin\bzip2.exe`) --
//! never with `bzip2-rs`, which could not have done the compression step even
//! if asked.
//!
//! Committed rather than generated, per the fixture convention already used
//! for `ffmpeg-sample.tar.xz` in this same directory: 339 bytes, so
//! committing it is simpler than shelling out to a system binary from a test.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::fs;

use kw_sidecar::{extract, extract_with_limit, ArchiveKind, InstallError};

const FIXTURE: &[u8] = include_bytes!("fixtures/sherpa-sample.tar.bz2");

/// The fixture's three entries and their sizes, read out of the decompressed
/// tar headers rather than counted by eye:
///
/// ```text
/// sherpa-bin/sherpa-onnx-offline  134
/// sherpa-bin/onnxruntime.so        23
/// unrelated-file                   64
/// ```
///
/// So `sherpa-bin` extracts to 157 bytes, and 221 bytes of the archive is file
/// data. The ceiling tests below are written against those figures.
const PAYLOAD_TOTAL: u64 = 157;
const FIRST_ENTRY: u64 = 134;

fn fixture(dir: &std::path::Path) -> std::path::PathBuf {
    let archive = dir.join("sherpa.tar.bz2");
    fs::write(&archive, FIXTURE).unwrap();
    archive
}

#[test]
fn a_real_bzip2_stream_from_the_bzip2_cli_extracts_correctly() {
    let dir = tempfile::tempdir().unwrap();
    let archive = dir.path().join("sherpa.tar.bz2");
    fs::write(&archive, FIXTURE).unwrap();
    let dest = dir.path().join("out");
    fs::create_dir_all(&dest).unwrap();

    let count = extract(&archive, ArchiveKind::TarBz2, "sherpa-bin", "", &dest).unwrap();
    assert_eq!(count, 2);

    assert_eq!(
        fs::read_to_string(dest.join("sherpa-onnx-offline")).unwrap(),
        "a tiny stand-in for a real sherpa-onnx-offline binary, used only to prove bzip2-rs can \
         decode a genuinely independent .tar.bz2 stream\n"
    );
    assert_eq!(
        fs::read_to_string(dest.join("onnxruntime.so")).unwrap(),
        "unused sibling library\n"
    );
    assert!(
        !dest.join("sherpa-bin").exists(),
        "the payload directory prefix is stripped, same as the other archive kinds"
    );
    assert!(
        !dest.join("unrelated-file").exists(),
        "an entry outside the payload directory must be skipped, not extracted"
    );
}

#[test]
fn extraction_stops_before_the_first_entry_would_exceed_the_output_limit() {
    let dir = tempfile::tempdir().unwrap();
    let archive = fixture(dir.path());
    let dest = dir.path().join("out");
    fs::create_dir_all(&dest).unwrap();

    let err =
        extract_with_limit(&archive, ArchiveKind::TarBz2, "sherpa-bin", "", &dest, 1).unwrap_err();
    // A typed variant, not a substring of `InstallError::Archive`'s `detail`.
    // This assertion used to read `contains("extracted data exceeds")` because
    // there was nothing better to match on, and a caller could not tell "too
    // large" from "corrupt archive".
    assert!(
        matches!(err, InstallError::ExtractedTooLarge { limit: 1, .. }),
        "{err}"
    );
    assert!(!dest.join("sherpa-onnx-offline").exists());
}

#[test]
fn a_later_entry_over_the_limit_takes_the_earlier_one_with_it() {
    // The `.tar.bz2` half of the per-kind ceiling coverage. `max_bytes = 1`
    // above trips on the *first* entry and so proves nothing about partial
    // output; this one lets 134 bytes land and then refuses at 157.
    let dir = tempfile::tempdir().unwrap();
    let archive = fixture(dir.path());
    let dest = dir.path().join("out");
    fs::create_dir_all(&dest).unwrap();

    let err = extract_with_limit(
        &archive,
        ArchiveKind::TarBz2,
        "sherpa-bin",
        "",
        &dest,
        PAYLOAD_TOTAL - 1,
    )
    .unwrap_err();
    assert!(
        matches!(err, InstallError::ExtractedTooLarge { .. }),
        "{err}"
    );
    assert!(
        !dest.join("sherpa-onnx-offline").exists(),
        "the {FIRST_ENTRY}-byte entry that fitted must be removed with the one that did not"
    );
    assert_eq!(fs::read_dir(&dest).unwrap().count(), 0);
}

#[test]
fn a_ceiling_that_exactly_fits_the_payload_is_not_a_refusal() {
    // A bound that refuses a legitimate install is worse than no bound. 157 is
    // the payload directory's exact total, and it must be enough -- note also
    // that the 64-byte `unrelated-file` outside `sherpa-bin` does not consume
    // the budget, because it is never written.
    let dir = tempfile::tempdir().unwrap();
    let archive = fixture(dir.path());
    let dest = dir.path().join("out");
    fs::create_dir_all(&dest).unwrap();

    let count = extract_with_limit(
        &archive,
        ArchiveKind::TarBz2,
        "sherpa-bin",
        "",
        &dest,
        PAYLOAD_TOTAL,
    )
    .unwrap();
    assert_eq!(count, 2);
    assert!(dest.join("sherpa-onnx-offline").is_file());
    assert!(dest.join("onnxruntime.so").is_file());
}
