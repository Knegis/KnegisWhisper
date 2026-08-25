//! The KnegisWhisper desktop shell.
//!
//! Deliberately thin. Window creation and command registration live here;
//! everything with behaviour lives in a `kw-*` crate so it can be tested without
//! a running application. Upstream's equivalent file grew to 10,678 lines
//! holding 478 IPC handlers, which is the outcome this split exists to avoid.

mod diagnostics;

use anyhow::Context as _;

/// Install log output.
///
/// Honours `KNEGISWHISPER_LOG`, defaulting to `info`. Errors are swallowed
/// because a second initialisation attempt (in tests, or a re-entrant setup) is
/// harmless and must not take the app down.
fn init_tracing() {
    use tracing_subscriber::{fmt, EnvFilter};

    let filter =
        EnvFilter::try_from_env("KNEGISWHISPER_LOG").unwrap_or_else(|_| EnvFilter::new("info"));

    let _ = fmt().with_env_filter(filter).with_target(false).try_init();
}

/// Build and run the application.
///
/// Returns rather than panicking so the binary can report a startup failure as a
/// message and an exit code instead of a Rust panic dump.
pub fn run() -> anyhow::Result<()> {
    init_tracing();

    tracing::info!(
        version = env!("CARGO_PKG_VERSION"),
        os = std::env::consts::OS,
        arch = std::env::consts::ARCH,
        "starting"
    );

    // No network call happens here, or anywhere else during startup. See
    // docs/NETWORK_BOUNDARY.md.
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![diagnostics::diagnostics])
        .run(tauri::generate_context!())
        .context("the application exited with an error")
}
