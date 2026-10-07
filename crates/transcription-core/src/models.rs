//! Installing a local whisper model.
//!
//! A model is *data*, not an executable, and it is verified differently for that
//! reason. `kw_sidecar` refuses to describe a binary without a pinned SHA-256,
//! because running the wrong bytes compromises the machine. Running the wrong
//! *weights* produces bad transcripts — a real problem, but not the same
//! problem, and the honest check is proportionate to it.
//!
//! So a model carries [`Integrity::Pinned`] where a digest is known and
//! [`Integrity::Size`] otherwise, and which one applied is reported rather than
//! implied. Upstream checks size alone for its six models. **Every shipped model is now pinned,**
//! including the five KB Whisper additions.
//! This module used to hold a `PINNED` table of its own with `tiny` alone in it
//! and a note that the rest were "a job for a tool rather than something to
//! invent digests for here"; that tool is `tools/model-pin/pin.mjs`, and on
//! 2026-08-31 it downloaded and hashed roughly seven gigabytes of model in one
//! run. `tiny` reproduced the digest this file had carried since it was written.
//!
//! **The pins do not live here.** They live in `WHISPER_PINS` in
//! `tools/integration-audit/generate-provider-registry.mjs`, beside
//! `SHERPA_PINS` and `LOCAL_LLM_PINS`, and reach this module as
//! [`WhisperModel::sha256`] and [`WhisperModel::expected_size_bytes`]. Three
//! model families keeping their pins in three different places was the actual
//! problem: `model-pin --verify` had to *parse this file's Rust* to check the
//! whisper half, and now it reads one generated JSON document for all of them.
//!
//! `expected_size_bytes` is consequently the **measured** length now, not the
//! rounded one — `ggml-tiny.bin` is 77,691,713 bytes, where the baseline
//! declared 78,000,000. Nothing was lost in overriding it: the settings screen
//! renders `sizeMb`, not this field, and [`integrity_for`] below is its only
//! reader. (An earlier version of this comment claimed the rounded figures were
//! "also what the settings screen shows". That was never true — see
//! `apps/desktop/src/views/SettingsTranscription.tsx`, which formats `sizeMb`.)
//!
//! The size-only path is kept anyway; see [`Integrity::Size`].

use std::path::{Path, PathBuf};

use kw_network_policy::{NetworkPolicy, RequestPurpose};
use kw_provider_registry::WhisperModel;
use kw_sidecar::manifest::Sha256Digest;

/// How much a model file may differ from its declared size.
///
/// Generous, because a *declared* figure is rounded for display. It is still
/// enough to catch the failures that matter: a truncated transfer, an error page
/// saved as a model, a rate-limit response.
///
/// **No shipped model reaches this any more**, because all shipped models carry a measured
/// digest and an exact length. It is not dead: [`within_tolerance`]'s other
/// caller is the `SizeCheck::Approximate` arm of `download_gguf` in
/// `apps/desktop/src-tauri/src/local_llm.rs`, kept for the same reason — a model
/// added to a registry before anybody measures it should be degraded, not
/// broken. A tolerance is the right answer to a rounded number and the wrong
/// answer to a measured one.
pub const SIZE_TOLERANCE: f64 = 0.05;

/// What is known about a model's bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Integrity {
    /// A digest measured from the real file, with the length measured beside it.
    Pinned {
        digest: Sha256Digest,
        size_bytes: u64,
    },
    /// A declared length and no digest, held to [`SIZE_TOLERANCE`].
    ///
    /// **Unreachable for every model the registry ships today** — all six are
    /// pinned. Kept deliberately, and not dead code: it is the honest answer for
    /// a model added to `config/upstream-integration-baseline.json` before
    /// anybody has spent the bandwidth to hash it, which for `large` is 2.9 GB.
    /// Deleting this would leave two options for that model, refusing to install
    /// it or installing it while implying a digest match, and both are worse.
    ///
    /// **Why this is not what sherpa does.** `sherpa_models::install` refuses an
    /// unpinned model outright with `MissingPin`. The asymmetry is deliberate and
    /// rests on what the two formats are: a whisper model is a bare `.bin` that
    /// is downloaded and never unpacked, so the worst an unverified one can do is
    /// produce bad transcripts. A sherpa model is a `.tar.bz2` that goes through
    /// `kw_sidecar::extract`, which means path traversal and decompression are in
    /// play before anything reads a weight. Refusing to unpack an unauthenticated
    /// archive and agreeing to load unauthenticated weights are proportionate to
    /// two different exposures, not one rule applied inconsistently.
    Size { expected_bytes: u64 },
    /// The registry gave neither.
    Unknown,
}

/// What integrity check a model can be held to.
///
/// Reads the pin off the registry entry rather than off a table in this file;
/// `WHISPER_PINS` in `tools/integration-audit/generate-provider-registry.mjs` is
/// where it is written down, and the generator refuses to emit half a pin.
#[must_use]
pub fn integrity_for(model: &WhisperModel) -> Integrity {
    // Both halves or neither. `kw_sidecar::integrity::verify` needs the length
    // as well as the digest, so a digest on its own cannot be checked -- and
    // reporting `Pinned` for something that cannot be verified is the one
    // outcome worth ruling out here.
    if let (Some(digest), Some(size_bytes)) = (model.sha256.as_deref(), model.expected_size_bytes) {
        if let Ok(digest) = Sha256Digest::parse(digest) {
            return Integrity::Pinned { digest, size_bytes };
        }
    }

    match model.expected_size_bytes {
        Some(expected_bytes) => Integrity::Size { expected_bytes },
        None => Integrity::Unknown,
    }
}

/// How a model was checked once it was on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verification {
    /// The digest matched.
    DigestMatched,
    /// Only the size was checked, because no digest is known for this model.
    ///
    /// Nothing produces this today; it is [`Integrity::Size`]'s outcome and is
    /// kept for the same reason. The distinction it draws — "the right length"
    /// against "the file the publisher built" — is what
    /// `local_model::VerifiedBy` puts on screen, and a UI that cannot say the
    /// weaker thing will say the stronger one instead.
    SizeOnly { bytes: u64 },
    /// Nothing could be checked. Reported, never silent.
    Unchecked { bytes: u64 },
}

/// Why a model install failed.
#[derive(Debug, thiserror::Error)]
pub enum ModelError {
    #[error(transparent)]
    Network(#[from] kw_network_policy::PolicyError),

    #[error(transparent)]
    Integrity(#[from] kw_sidecar::IntegrityError),

    #[error("could not {action} {path}: {source}")]
    Io {
        action: &'static str,
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error(
        "{file} is {actual} bytes but about {expected} was expected. The download was cut \
         short, or the published file changed."
    )]
    Size {
        file: String,
        actual: u64,
        expected: u64,
    },

    #[error("{id:?} is not a whisper model this build knows about")]
    UnknownModel { id: String },
}

/// A model that is installed and checked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledModel {
    pub id: String,
    pub path: PathBuf,
    pub bytes: u64,
    pub verification: Verification,
}

/// Progress of a model download.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelProgress {
    Downloading { written: u64, total: Option<u64> },
    Verifying,
}

/// Where a model file lives once installed.
#[must_use]
pub fn model_path(dir: &Path, model: &WhisperModel) -> PathBuf {
    dir.join(&model.file_name)
}

/// Whether a model is already installed and passes its check.
///
/// Used before offering to download: a partially written file from an
/// interrupted download exists but must not count as installed.
pub fn is_installed(dir: &Path, model: &WhisperModel) -> bool {
    let path = model_path(dir, model);
    let Ok(metadata) = std::fs::metadata(&path) else {
        return false;
    };
    check(
        &path,
        metadata.len(),
        &integrity_for(model),
        &model.file_name,
    )
    .is_ok()
}

/// Download a model into `dir` and verify it.
///
/// The download is bounded where the model carries a **measured** length, and
/// unbounded where it does not. All shipped models carry one; the original six were pinned as of
/// `a82d06b`, so in practice every install is bounded; the unbounded arm is for
/// the same model [`Integrity::Size`] exists for — one added to the baseline
/// before anybody has spent the bandwidth to measure it.
///
/// **The bound is only ever an exact figure.** Bounding a *declared* length
/// would reintroduce KNEG-101, where `download_gguf` compared a finished
/// download against a size that was 7.4% wrong for eighteen models and deleted
/// all eighteen. That bug wasted the bandwidth; a bound would have refused the
/// transfer mid-flight instead, which is faster and just as wrong. So
/// [`Integrity::Size`]'s tolerance stays a *post*-download check and never
/// becomes a bound: a rounded figure cannot bound anything, because the true
/// length may be on either side of it.
pub async fn install(
    policy: &NetworkPolicy,
    model: &WhisperModel,
    dir: &Path,
    // Held across awaits, so `Send`; see `NetworkPolicy::download_to_file`.
    progress: &mut (dyn FnMut(ModelProgress) + Send),
) -> Result<InstalledModel, ModelError> {
    std::fs::create_dir_all(dir).map_err(|source| ModelError::Io {
        action: "create",
        path: dir.to_path_buf(),
        source,
    })?;

    // Downloaded beside the destination and renamed on success, so an
    // interrupted download never looks like an installed model.
    let final_path = model_path(dir, model);
    let partial = final_path.with_extension("partial");

    let integrity = integrity_for(model);
    let mut report = |written, total| progress(ModelProgress::Downloading { written, total });
    match &integrity {
        // A measured length, so it is exact and can bound the transfer.
        Integrity::Pinned { size_bytes, .. } => {
            policy
                .download_to_file_bounded(
                    RequestPurpose::UserTriggeredModelDownload,
                    &model.download_url,
                    &partial,
                    *size_bytes,
                    &mut report,
                )
                .await?;
        }
        // `Integrity::Size`'s figure is declared, and `Unknown` has none at
        // all. Neither can bound a download; both are still checked against
        // afterwards by `check` below.
        Integrity::Size { .. } | Integrity::Unknown => {
            policy
                .download_to_file(
                    RequestPurpose::UserTriggeredModelDownload,
                    &model.download_url,
                    &partial,
                    &mut report,
                )
                .await?;
        }
    }

    progress(ModelProgress::Verifying);

    let bytes = std::fs::metadata(&partial)
        .map_err(|source| ModelError::Io {
            action: "measure",
            path: partial.clone(),
            source,
        })?
        .len();

    let verification = match check(&partial, bytes, &integrity, &model.file_name) {
        Ok(verification) => verification,
        Err(err) => {
            // A file that failed its check must not be left where a later run
            // could mistake it for an install.
            let _ = std::fs::remove_file(&partial);
            return Err(err);
        }
    };

    std::fs::rename(&partial, &final_path).map_err(|source| ModelError::Io {
        action: "move into place",
        path: final_path.clone(),
        source,
    })?;

    Ok(InstalledModel {
        id: model.id.clone(),
        path: final_path,
        bytes,
        verification,
    })
}

/// Hold a file to whatever check its model allows.
fn check(
    path: &Path,
    bytes: u64,
    integrity: &Integrity,
    file_name: &str,
) -> Result<Verification, ModelError> {
    match integrity {
        Integrity::Pinned { digest, size_bytes } => {
            kw_sidecar::integrity::verify(path, *size_bytes, digest)?;
            Ok(Verification::DigestMatched)
        }
        Integrity::Size { expected_bytes } => {
            if within_tolerance(bytes, *expected_bytes) {
                Ok(Verification::SizeOnly { bytes })
            } else {
                Err(ModelError::Size {
                    file: file_name.to_owned(),
                    actual: bytes,
                    expected: *expected_bytes,
                })
            }
        }
        Integrity::Unknown => Ok(Verification::Unchecked { bytes }),
    }
}

/// Whether `actual` is close enough to `expected`.
///
/// Two callers, and neither is reached by anything the registry ships today:
/// [`check`]'s [`Integrity::Size`] arm above, and `SizeCheck::Approximate` in
/// `apps/desktop/src-tauri/src/local_llm.rs` — the last remaining consumer
/// outside this crate. Both exist for the model nobody has measured yet. See
/// [`SIZE_TOLERANCE`].
#[must_use]
pub fn within_tolerance(actual: u64, expected: u64) -> bool {
    if expected == 0 {
        return false;
    }
    let expected_f = expected as f64;
    let difference = (actual as f64 - expected_f).abs();
    difference / expected_f <= SIZE_TOLERANCE
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use super::*;

    fn registry() -> &'static kw_provider_registry::Registry {
        kw_provider_registry::registry().expect("the shipped registry loads")
    }

    fn model(id: &str) -> &'static WhisperModel {
        registry()
            .local_asr
            .whisper
            .iter()
            .find(|m| m.id == id)
            .expect("a whisper model with that id")
    }

    /// A model shaped like one added to the registry before it was measured.
    ///
    /// Built by hand rather than found in the registry, because there is no
    /// unpinned whisper model any more — which is what
    /// `every_whisper_model_is_pinned_to_a_measured_digest` asserts, and the
    /// reason the size path needs a fixture to be exercised at all.
    fn unmeasured_model(expected_size_bytes: Option<u64>) -> WhisperModel {
        WhisperModel {
            id: "not-yet-measured".to_owned(),
            name: "Not yet measured".to_owned(),
            file_name: "ggml-not-yet-measured.bin".to_owned(),
            download_url: "https://huggingface.co/example/resolve/main/x.bin".to_owned(),
            size_mb: Some(142.0),
            sha256: None,
            expected_size_bytes,
            recommended: false,
        }
    }

    #[test]
    fn the_pinned_model_is_held_to_its_measured_digest_and_length() {
        // 77,691,713 rather than the baseline's rounded 78,000,000: this also
        // asserts that WHISPER_PINS' measured size reached the registry, since
        // the generator overrides upstream's figure with it.
        match integrity_for(model("tiny")) {
            Integrity::Pinned { digest, size_bytes } => {
                assert_eq!(size_bytes, 77_691_713);
                assert_eq!(digest.as_str().len(), 64);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn every_whisper_model_is_pinned_to_a_measured_digest() {
        // Mirrors `every_installable_sherpa_model_has_integrity_and_size_limits`
        // in sherpa_models.rs. The original six were measured on 2026-08-31;
        // KB Whisper Tiny and Base were measured on 2026-09-10, and Small,
        // Medium and Large on 2026-10-07 (KNEG-407),
        // so anything weaker than Pinned here means a pin was dropped from
        // WHISPER_PINS in tools/integration-audit/generate-provider-registry.mjs
        // -- and a model that quietly fell back to a size check would install
        // while the screen still said "digest" for its neighbours.
        for model in &registry().local_asr.whisper {
            match integrity_for(model) {
                Integrity::Pinned { digest, size_bytes } => {
                    assert_eq!(digest.as_str().len(), 64, "{}", model.id);
                    assert!(size_bytes > 0, "{}", model.id);
                    // The measured length, never the rounded display figure.
                    assert_ne!(size_bytes % 1_000_000, 0, "{} looks rounded", model.id);
                }
                other => panic!("{} is {other:?} rather than pinned", model.id),
            }
        }
    }

    #[test]
    fn an_unpinned_model_says_so_rather_than_implying_a_digest() {
        // The distinction the caller needs: nothing here pretends the bytes were
        // verified against a known-good digest when they were not.
        match integrity_for(&unmeasured_model(Some(148_000_000))) {
            Integrity::Size { expected_bytes } => assert_eq!(expected_bytes, 148_000_000),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_model_with_no_size_at_all_is_unknown_rather_than_pinned() {
        assert_eq!(integrity_for(&unmeasured_model(None)), Integrity::Unknown);
    }

    #[test]
    fn a_digest_with_no_length_is_not_reported_as_pinned() {
        // The generator cannot emit this pair -- WHISPER_PINS dies on half a pin
        // -- but `verify` needs both numbers, so claiming Pinned here would
        // promise a check that cannot run.
        let mut model = unmeasured_model(None);
        model.sha256 = Some("ab".repeat(32));
        assert_eq!(integrity_for(&model), Integrity::Unknown);
    }

    #[test]
    fn the_rounded_registry_size_still_accepts_the_real_file() {
        // What the tolerance is for, in the one case that still reaches it: a
        // model carrying upstream's rounded declaration. ggml-tiny.bin was
        // declared as 78,000,000 and is really 77,691,713, which is the shape of
        // every figure in the baseline.
        assert!(within_tolerance(77_691_713, 78_000_000));
    }

    #[test]
    fn a_truncated_download_is_outside_the_tolerance() {
        assert!(!within_tolerance(4_096, 78_000_000));
        assert!(!within_tolerance(0, 78_000_000));
        // Half a file is not a rounding difference.
        assert!(!within_tolerance(39_000_000, 78_000_000));
    }

    #[test]
    fn a_zero_expected_size_never_passes() {
        // Otherwise a registry entry with a missing size would accept anything.
        assert!(!within_tolerance(0, 0));
        assert!(!within_tolerance(100, 0));
    }

    #[test]
    fn the_model_path_uses_the_registry_file_name() {
        let dir = Path::new("C:/kw/models");
        assert_eq!(
            model_path(dir, model("tiny")),
            dir.join("ggml-tiny.bin"),
            "the file name has to match what whisper-server is told to load"
        );
    }

    #[test]
    fn a_partial_file_does_not_count_as_installed() {
        // An interrupted download leaves a file; treating it as installed sends
        // whisper-server at a truncated model.
        let dir = tempfile::tempdir().unwrap();
        let model = model("tiny");

        assert!(!is_installed(dir.path(), model), "nothing there yet");

        std::fs::write(model_path(dir.path(), model), b"first few bytes").unwrap();
        assert!(
            !is_installed(dir.path(), model),
            "a short file is not an install"
        );
    }

    #[test]
    fn a_file_that_fails_its_size_check_is_reported_with_both_numbers() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ggml-not-yet-measured.bin");
        std::fs::write(&path, b"nope").unwrap();

        let unmeasured = unmeasured_model(Some(148_000_000));
        let err = check(
            &path,
            4,
            &integrity_for(&unmeasured),
            "ggml-not-yet-measured.bin",
        )
        .unwrap_err();

        let message = err.to_string();
        assert!(message.contains('4'), "{message}");
        assert!(message.contains("cut short"), "{message}");
    }

    #[test]
    fn a_pinned_model_whose_bytes_are_wrong_fails_on_the_digest_not_the_length() {
        // The upgrade this pass bought, asserted rather than assumed: a file of
        // exactly the right length that is not the right file used to pass for
        // five of the six models. It cannot now, and the error names integrity
        // rather than a cut-short download. Five bytes rather than the real
        // 77 MB, because the arm being exercised is `check`'s, not the network's.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ggml-tiny.bin");
        std::fs::write(&path, b"wrong").unwrap();

        let pinned = Integrity::Pinned {
            digest: Sha256Digest::parse(&"ab".repeat(32)).unwrap(),
            size_bytes: 5,
        };
        let err = check(&path, 5, &pinned, "ggml-tiny.bin").unwrap_err();
        assert!(
            matches!(err, ModelError::Integrity(_)),
            "{err} -- the length matched, so only the digest can have caught this"
        );
    }

    #[test]
    fn an_unknown_integrity_reports_unchecked_rather_than_passing_quietly() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("model.bin");
        std::fs::write(&path, b"bytes").unwrap();

        assert_eq!(
            check(&path, 5, &Integrity::Unknown, "model.bin").unwrap(),
            Verification::Unchecked { bytes: 5 }
        );
    }

    #[test]
    fn the_recommended_model_is_the_one_the_settings_screen_would_offer() {
        // Guards against a registry edit that leaves nothing recommended, which
        // would make the install screen have no default.
        let recommended: Vec<&str> = registry()
            .local_asr
            .whisper
            .iter()
            .filter(|m| m.recommended)
            .map(|m| m.id.as_str())
            .collect();
        assert_eq!(recommended, ["base"]);
    }
}
