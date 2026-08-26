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

use kw_sidecar::{extract, ArchiveKind};

const FIXTURE: &[u8] = include_bytes!("fixtures/sherpa-sample.tar.bz2");

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
