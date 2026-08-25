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

    // Installing again over a working install must leave a working install.
    let again = install(&policy, spec, asset, root.path(), &mut |_| {})
        .await
        .expect("reinstall should succeed");
    assert!(again.executable.is_file());
    assert_eq!(again.files, installed.files);
}
