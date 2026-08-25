//! Proving a downloaded file is the one that was pinned.
//!
//! Size is checked first because it is free and rules out the common case — a
//! truncated transfer, a captive-portal HTML page served with a 200, a rate-limit
//! response. Hashing several megabytes only to report "not the expected bytes"
//! is a worse error message than "expected 8 MB, got 4 KB".
//!
//! The digest is then checked over the whole file. There is no "verify if a
//! digest is present" path: [`crate::manifest::Asset`] cannot exist without one.

use std::fs::File;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::manifest::Sha256Digest;

/// Read size for hashing. Large enough that syscall overhead is irrelevant,
/// small enough to stay out of the way on a constrained machine.
const CHUNK: usize = 64 * 1024;

/// Why a file was not accepted.
#[derive(Debug, thiserror::Error)]
pub enum IntegrityError {
    #[error("could not read {path} to verify it: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    #[error(
        "{path} is {found} bytes but {expected} was pinned. A truncated download, or the \
         published asset changed."
    )]
    Size {
        path: PathBuf,
        expected: u64,
        found: u64,
    },

    #[error(
        "{path} does not match its pinned digest.\n  pinned: {expected}\n  actual: {found}\n\
         This file is not the one that was reviewed, so it will not be run."
    )]
    Digest {
        path: PathBuf,
        expected: Sha256Digest,
        found: String,
    },
}

/// The SHA-256 of a file, streamed rather than buffered whole.
pub fn hash_file(path: &Path) -> Result<String, IntegrityError> {
    let mut file = File::open(path).map_err(|source| IntegrityError::Read {
        path: path.to_path_buf(),
        source,
    })?;

    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; CHUNK];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|source| IntegrityError::Read {
                path: path.to_path_buf(),
                source,
            })?;
        if read == 0 {
            break;
        }
        match buffer.get(..read) {
            Some(chunk) => hasher.update(chunk),
            None => break,
        }
    }

    Ok(hex(&hasher.finalize()))
}

/// Check a file against a pinned size and digest.
///
/// Size first, then the digest. Both must match.
pub fn verify(
    path: &Path,
    expected_size: u64,
    expected: &Sha256Digest,
) -> Result<(), IntegrityError> {
    let found_size = std::fs::metadata(path)
        .map_err(|source| IntegrityError::Read {
            path: path.to_path_buf(),
            source,
        })?
        .len();

    if found_size != expected_size {
        return Err(IntegrityError::Size {
            path: path.to_path_buf(),
            expected: expected_size,
            found: found_size,
        });
    }

    let found = hash_file(path)?;
    if found != expected.as_str() {
        return Err(IntegrityError::Digest {
            path: path.to_path_buf(),
            expected: expected.clone(),
            found,
        });
    }

    Ok(())
}

/// The SHA-256 of some bytes already in memory.
#[must_use]
pub fn hash_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex(&hasher.finalize())
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes.iter().fold(String::with_capacity(64), |mut out, b| {
        // Writing to a String cannot fail; the result is discarded rather than
        // unwrapped so this stays panic-free under the workspace lints.
        let _ = write!(out, "{b:02x}");
        out
    })
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use std::io::Write;

    use super::*;

    /// The published SHA-256 of the empty input, from FIPS 180-4.
    const EMPTY: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
    /// And of the three bytes "abc", the standard test vector.
    const ABC: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

    fn write(bytes: &[u8]) -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("asset.bin");
        let mut file = File::create(&path).unwrap();
        file.write_all(bytes).unwrap();
        (dir, path)
    }

    #[test]
    fn the_hash_matches_the_published_test_vectors() {
        // Guards against a chunking bug producing a self-consistent but wrong
        // digest, which comparing our own output to itself would never catch.
        assert_eq!(hash_bytes(b""), EMPTY);
        assert_eq!(hash_bytes(b"abc"), ABC);
    }

    #[test]
    fn hashing_a_file_agrees_with_hashing_its_bytes() {
        let (_dir, path) = write(b"abc");
        assert_eq!(hash_file(&path).unwrap(), ABC);
    }

    #[test]
    fn a_file_larger_than_one_chunk_hashes_correctly() {
        // The loop is the only interesting part of hash_file.
        let bytes = vec![0x5a_u8; CHUNK * 2 + 7];
        let (_dir, path) = write(&bytes);
        assert_eq!(hash_file(&path).unwrap(), hash_bytes(&bytes));
    }

    #[test]
    fn a_matching_file_verifies() {
        let (_dir, path) = write(b"abc");
        verify(&path, 3, &Sha256Digest::parse(ABC).unwrap()).unwrap();
    }

    #[test]
    fn a_truncated_download_is_reported_as_a_size_problem() {
        // The common failure, and the one where the size check earns its place:
        // the message names bytes rather than hashes.
        let (_dir, path) = write(b"ab");
        let err = verify(&path, 3, &Sha256Digest::parse(ABC).unwrap()).unwrap_err();
        assert!(matches!(
            err,
            IntegrityError::Size {
                expected: 3,
                found: 2,
                ..
            }
        ));
        assert!(err.to_string().contains("truncated"));
    }

    #[test]
    fn the_right_length_and_the_wrong_bytes_is_a_digest_problem() {
        let (_dir, path) = write(b"abd");
        let err = verify(&path, 3, &Sha256Digest::parse(ABC).unwrap()).unwrap_err();
        match err {
            IntegrityError::Digest {
                expected, found, ..
            } => {
                assert_eq!(expected.as_str(), ABC);
                assert_ne!(found, ABC);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn the_digest_error_shows_both_values_and_says_it_will_not_run() {
        let (_dir, path) = write(b"abd");
        let err = verify(&path, 3, &Sha256Digest::parse(ABC).unwrap())
            .unwrap_err()
            .to_string();
        assert!(err.contains(ABC), "the pinned digest");
        assert!(err.contains("actual"), "and what was found");
        assert!(err.contains("will not be run"));
    }

    #[test]
    fn a_missing_file_is_a_read_error_naming_the_path() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("absent.bin");
        let err = verify(&path, 3, &Sha256Digest::parse(ABC).unwrap()).unwrap_err();
        assert!(matches!(err, IntegrityError::Read { .. }));
        assert!(err.to_string().contains("absent.bin"));
    }

    #[test]
    fn an_empty_file_where_bytes_were_expected_fails_on_size() {
        // A captive portal or a rate-limit page saved to disk.
        let (_dir, path) = write(b"");
        assert!(matches!(
            verify(&path, 8_361_840, &Sha256Digest::parse(EMPTY).unwrap()).unwrap_err(),
            IntegrityError::Size { found: 0, .. }
        ));
    }

    #[test]
    fn hex_output_is_lowercase_and_fixed_width() {
        // Sha256Digest::parse refuses uppercase, so a comparison against our own
        // output would fail if this ever emitted capitals.
        let digest = hash_bytes(&[0x00, 0x0f, 0xff]);
        assert_eq!(digest.len(), 64);
        assert!(digest.chars().all(|c| c.is_ascii_hexdigit()));
        assert!(!digest.chars().any(|c| c.is_ascii_uppercase()));
        assert!(Sha256Digest::parse(&digest).is_ok());
    }
}
