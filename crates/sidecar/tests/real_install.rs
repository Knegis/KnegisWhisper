//! Installs the real pinned whisper sidecar over the real network.
//!
//! `#[ignore]`d, so `cargo test` does not reach the network and CI stays
//! hermetic. Run it deliberately:
//!
//! ```text
//! cargo test -p kw-sidecar --test real_install -- --ignored --nocapture
//! ```
//!
//! It exists because everything else in this crate is tested against fixtures,
//! and fixtures cannot tell you whether the pinned digest is the digest of the
//! file that host actually serves, whether the redirect chain is followable, or
//! whether the archive still has the shape the manifest claims. Those are the
//! three things that break when an upstream release is re-tagged, and they only
//! break in production.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use kw_network_policy::NetworkPolicy;
use kw_sidecar::{install, Catalog, Progress};

#[tokio::test]
#[ignore = "downloads ~8 MB from github.com; run with --ignored"]
async fn the_pinned_whisper_asset_downloads_verifies_and_unpacks() {
    let catalog = Catalog::shipped().unwrap();
    let (spec, asset) = match catalog.resolve("whisper") {
        Ok(pair) => pair,
        Err(err) => {
            // On macOS this is the honest refusal, not a failure.
            println!("skipped: {err}");
            return;
        }
    };

    let root = tempfile::tempdir().unwrap();
    let policy = NetworkPolicy::new().unwrap();

    let mut saw_download = false;
    let mut saw_verify = false;
    let mut saw_extract = false;
    let mut last_written = 0_u64;

    let installed = install(
        &policy,
        spec,
        asset,
        root.path(),
        &mut |progress| match progress {
            Progress::Downloading { written, .. } => {
                saw_download = true;
                last_written = written;
            }
            Progress::Verifying => saw_verify = true,
            Progress::Extracting => saw_extract = true,
        },
    )
    .await
    .expect("the pinned asset should install");

    assert!(
        saw_download && saw_verify && saw_extract,
        "every stage reports"
    );
    assert_eq!(
        last_written, asset.size_bytes,
        "progress should end at the pinned size"
    );

    // The redirect chain is real: a release download answers with a 302 to a
    // storage host, and the report must name where the bytes came from rather
    // than the URL that was asked for.
    assert_ne!(
        installed.source_url, asset.url,
        "github redirects release downloads to storage"
    );

    assert!(installed.executable.is_file());
    assert!(
        installed.files > 5,
        "whisper-server needs its sibling ggml libraries; got {} file(s)",
        installed.files
    );

    // The install can say what it is. Everything else about the record is
    // tested against fixtures; what only a real install can show is that the
    // digest written down is the one the manifest pins and not one recomputed
    // from whatever arrived.
    let install_dir = spec.install_dir(root.path());
    let record = kw_sidecar::record::read(&install_dir);
    match &record {
        kw_sidecar::Record::Present(found) => {
            assert_eq!(found.asset_sha256, asset.sha256);
            assert_eq!(found.sidecar, "whisper");
            assert_eq!(found.source_release, spec.source.release);
        }
        other => panic!("no record beside a fresh install: {other:?}"),
    }

    let found = kw_sidecar::locate(
        "whisper",
        asset,
        &kw_sidecar::SearchPath {
            bundled: None,
            downloaded: root.path().to_path_buf(),
        },
        &kw_sidecar::RealFs,
    );
    assert_eq!(
        kw_sidecar::freshness(&found, &record, "whisper", &asset.sha256),
        kw_sidecar::Freshness::Current,
        "a sidecar installed a moment ago is the one this build pins"
    );

    // A leftover staging directory must not make the next install impossible.
    // This is the state a real machine was left in on 2026-08-31, and under the
    // old single-name, hard-erroring code it was permanent for that sidecar.
    let leftover = root.path().join("whisper.incoming");
    std::fs::create_dir_all(leftover.join("files")).unwrap();
    std::fs::write(leftover.join("files").join("half.dll"), b"junk").unwrap();

    // Installing again over a working install must leave a working install.
    let again = install(&policy, spec, asset, root.path(), &mut |_| {})
        .await
        .expect("reinstall should succeed");
    assert!(again.executable.is_file());
    assert_eq!(again.files, installed.files);
    assert!(
        !leftover.exists(),
        "the sweep should have taken the leftover staging directory with it"
    );
    assert_eq!(
        kw_sidecar::freshness(
            &found,
            &kw_sidecar::record::read(&install_dir),
            "whisper",
            &asset.sha256,
        ),
        kw_sidecar::Freshness::Current
    );
}
