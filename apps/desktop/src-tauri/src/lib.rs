//! The KnegisWhisper desktop shell.
//!
//! Deliberately thin. Window creation, plugin setup, and command registration
//! live here; everything with behaviour lives in a `kw-*` crate so it can be
//! tested without a running application. Upstream's equivalent file grew to
//! 10,678 lines holding 478 IPC handlers, which is the outcome this split exists
//! to avoid.
//!
//! Nothing in `setup` touches the network. The database is opened, the tray and
//! overlay are created, and hotkeys are registered — all local. Provider
//! connection tests are explicitly invoked by the user, never run at startup.
//! See `docs/NETWORK_BOUNDARY.md`.

mod commands;
mod diagnostics;
mod dictation;
mod local_api;
mod local_model;
mod overlay;
mod shortcuts;
mod state;
mod tray;

use anyhow::Context as _;
use tauri::Manager as _;

use state::AppState;

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

/// Say so in the terminal when nothing is serving the frontend.
///
/// A development build loads the window from the Vite dev server rather than
/// from bundled files, so `cargo run` on its own produces an application whose
/// window says only "can't reach this page" — the webview's own error, which
/// names neither the cause nor the fix. The Rust side starts perfectly and logs
/// as if all were well, which is the worst combination: two honest components
/// and no diagnosis. This is that diagnosis.
///
/// Loopback only, and only in a debug build. The URL comes from
/// `tauri.conf.json` via the runtime config rather than being repeated here, so
/// changing the dev port cannot make this message wrong.
#[cfg(debug_assertions)]
fn warn_if_nothing_serves_the_frontend(app: &tauri::App) {
    use std::net::{TcpStream, ToSocketAddrs as _};
    use std::time::Duration;

    // Absent in a bundled build, where the frontend is on disk.
    let Some(url) = app.config().build.dev_url.as_ref() else {
        return;
    };

    let host = url.host_str().unwrap_or("127.0.0.1");
    let port = url.port_or_known_default().unwrap_or(80);

    let reachable = format!("{host}:{port}")
        .to_socket_addrs()
        .into_iter()
        .flatten()
        .any(|addr| TcpStream::connect_timeout(&addr, Duration::from_millis(300)).is_ok());

    if !reachable {
        tracing::warn!(
            dev_url = %url,
            "nothing is serving the frontend, so the window will show \"can't reach this page\". \
             `cargo run` starts only the Rust half of the application. Run `npm run tauri:dev` \
             from apps/desktop instead, or leave `npm run dev` running there in another terminal."
        );
    }
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

    tauri::Builder::default()
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        // Registered so `commands::note_export_to_file` can open a save dialog
        // from Rust. The renderer has no dialog permission and never sees a
        // path — see the command's doc comment.
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            // No arguments: launching at login should behave exactly like
            // launching by hand.
            None,
        ))
        .setup(|app| {
            // First, because it explains a symptom the user is already looking
            // at by the time the rest of this finishes.
            #[cfg(debug_assertions)]
            warn_if_nothing_serves_the_frontend(app);

            let handle = app.handle().clone();

            // Opening the database is the one thing that must succeed before the
            // application is usable, so it is awaited here and a failure aborts
            // startup with a readable message rather than leaving every command
            // erroring later.
            let state = tauri::async_runtime::block_on(AppState::open())
                .context("could not open the database")?;

            let bindings = tauri::async_runtime::block_on(commands::load_bindings(&state))
                .map_err(|e| anyhow::anyhow!(e))
                .context("could not read the configured hotkeys")?;

            let db = state.db().clone();
            let credentials = state.credentials_arc();
            let policy = state.policy().clone();
            app.manage(state);

            // Local transcription. The catalogue is parsed here so a malformed
            // config/sidecars.json is a startup message rather than a surprise
            // on the first dictation.
            let local = std::sync::Arc::new(
                local_model::LocalTranscription::new(policy.clone())
                    .map_err(|e| anyhow::anyhow!(e))
                    .context("could not load config/sidecars.json")?,
            );

            // Before anything asks for a port: a previous run that crashed may
            // have left a server holding one.
            tauri::async_runtime::block_on(local.reap_stale());

            let shared = local.shared();

            // The route survives a restart; the server process does not. Bring
            // back whichever model the user was last dictating with, in the
            // background so the window does not wait for a model to load.
            local_model::resume_last_used(&local, &db);

            app.manage(std::sync::Arc::clone(&local));

            let session = std::sync::Arc::new(dictation::DictationRuntime::build(
                handle.clone(),
                db.clone(),
                credentials,
                policy,
                shared.port,
                shared.model,
                // Cleanup is off until the settings screen configures a route.
                // The raw transcript is a correct answer; a half-configured
                // cleanup that silently does nothing is not.
                std::sync::Arc::new(std::sync::RwLock::new(None)),
            ));
            app.manage(session);

            // Off unless the user has switched it on. See local_api.rs.
            let api = std::sync::Arc::new(local_api::LocalApiRuntime::new(db));
            tauri::async_runtime::block_on(api.start_if_enabled());
            app.manage(api);

            tray::create(&handle).context("could not create the tray icon")?;

            // The overlay is created up front so its capture-exclusion state is
            // a fact about a real window by the time the user opens settings,
            // rather than a prediction. It stays hidden until recording starts.
            match overlay::create(&handle) {
                Ok(window) => {
                    let status = overlay::apply_treatments(&window);
                    tracing::info!(
                        capture_exclusion = ?status.capture_exclusion,
                        non_activating = ?status.non_activating,
                        "overlay ready"
                    );
                }
                Err(err) => {
                    // Not fatal. The dashboard still works; the settings screen
                    // will report the overlay as absent.
                    tracing::error!(error = %err, "could not create the overlay window");
                }
            }

            let registrations = shortcuts::register_all(&handle, &bindings);
            let live = registrations.iter().filter(|r| r.is_live()).count();
            tracing::info!(live, total = registrations.len(), "hotkeys registered");

            Ok(())
        })
        .on_window_event(|window, event| {
            // Closing the dashboard hides it. Dictation is a background service;
            // quitting is done from the tray, explicitly.
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if window.label() == "main" {
                    api.prevent_close();
                    if let Err(err) = window.hide() {
                        tracing::warn!(error = %err, "could not hide the dashboard");
                    }
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            diagnostics::diagnostics,
            commands::settings_get,
            commands::settings_set,
            commands::settings_keys,
            commands::hotkeys,
            commands::hotkey_set,
            commands::history,
            commands::history_delete,
            commands::history_clear,
            commands::providers,
            commands::provider_credential_set,
            commands::provider_credential_present,
            commands::probe_providers,
            commands::import_openwhispr,
            commands::notes_list,
            commands::notes_search,
            commands::notes_get,
            commands::notes_create,
            commands::notes_update,
            commands::notes_delete,
            commands::folders_list,
            commands::folder_create,
            commands::folder_delete,
            commands::note_export,
            commands::note_export_to_file,
            dictation::dictation_state,
            dictation::dictation_route,
            dictation::dictation_use_local,
            dictation::dictation_toggle,
            dictation::dictation_cancel,
            dictation::dictation_set_device,
            dictation::input_devices,
            dictation::insertion_supported,
            local_model::local_model_status,
            local_model::local_model_install,
            local_model::local_server_start,
            local_model::local_server_stop,
            local_api::local_api_status,
            local_api::local_api_start,
            local_api::local_api_stop,
        ])
        .run(tauri::generate_context!())
        .context("the application exited with an error")
}
