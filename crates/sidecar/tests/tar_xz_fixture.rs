//! Decodes a `.tar.xz` that `lzma-rs` never produced.
//!
//! Every other archive test in this crate builds its fixture in-process with
//! the same crate (`flate2`, `zip`) that later reads it back, which proves the
//! round trip but not independence: a decoder and an encoder can agree with
//! each other while disagreeing with the rest of the world. This fixture was
//! made with the standalone `xz` command-line tool (XZ Utils 5.8.1), not with
//! `lzma-rs`, so decoding it here is evidence that `lzma-rs::xz_decompress`
//! reads a genuinely independent `.tar.xz` stream -- the shape a real ffmpeg
//! release actually ships, not just the shape this crate's own encoder makes.
//!
//! Committed rather than generated, per the fixture convention in
//! `crates/transcription-core/tests/fixtures`: 340 bytes, so committing it is
//! simpler than shelling out to a system binary from a test.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::fs;

use kw_sidecar::{extract, ArchiveKind};

const FIXTURE: &[u8] = include_bytes!("fixtures/ffmpeg-sample.tar.xz");

#[test]
fn a_real_xz_stream_from_the_xz_cli_extracts_correctly() {
    let dir = tempfile::tempdir().unwrap();
    let archive = dir.path().join("ffmpeg.tar.xz");
    fs::write(&archive, FIXTURE).unwrap();
    let dest = dir.path().join("out");
    fs::create_dir_all(&dest).unwrap();

    let count = extract(&archive, ArchiveKind::TarXz, "ffmpeg-bin", "", &dest).unwrap();
    assert_eq!(count, 2);

    assert_eq!(
        fs::read_to_string(dest.join("ffmpeg")).unwrap(),
        "a tiny stand-in for a real ffmpeg binary, used only to prove lzma-rs can decode a \
         genuinely independent .tar.xz stream\n"
    );
    assert_eq!(
        fs::read_to_string(dest.join("libavcodec.so")).unwrap(),
        "unused sibling library\n"
    );
    assert!(
        !dest.join("ffmpeg-bin").exists(),
        "the payload directory prefix is stripped, same as the other archive kinds"
    );
}
