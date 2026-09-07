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

mod automation;
mod binary_locator;
mod calendar;
mod commands;
mod diagnostics;
mod diarization;
mod dictation;
mod gguf_check;
mod integrations;
// Test-only: it renders `apps/desktop/src/generated/ipc-types.ts` and asserts
// the committed copy still matches the Rust types. Nothing at runtime calls it.
#[cfg(test)]
mod ipc_types;
mod local_api;
mod local_asr;
mod local_llm;
mod local_model;
mod media;
mod media_job;
mod meeting;
mod meeting_window;
mod overlay;
mod reasoning;
mod recording_attach;
mod recordings;
mod sherpa_stream;
mod shortcuts;
mod state;
mod tray;
mod update_window;
mod updater;
mod window_placement;

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
        .register_asynchronous_uri_scheme_protocol(recordings::SCHEME, recordings::serve)
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
            // Cloned before it is moved into the dictation runtime below.
            let meeting_credentials = std::sync::Arc::clone(&credentials);
            let calendar_credentials = std::sync::Arc::clone(&credentials);
            let policy = state.policy().clone();
            // Cloned before `policy` is moved into the dictation runtime below.
            let meeting_policy = policy.clone();
            let calendar_policy = policy.clone();
            app.manage(state);

            // Local transcription. The catalogue is parsed here so a malformed
            // config/sidecars.json is a startup message rather than a surprise
            // on the first dictation.
            let local = std::sync::Arc::new(
                local_model::LocalTranscription::new(policy.clone())
                    .map_err(|e| anyhow::anyhow!(e))
                    .context("could not load config/sidecars.json")?,
            );

            // The streaming model server behind the `local-streaming` meeting
            // provider. Not started here -- a 630 MB model held from launch for
            // a user who never records a meeting is a cost nobody asked for --
            // but reaped here, beside the others, before any port is allocated.
            let stream = std::sync::Arc::new(
                sherpa_stream::SherpaStream::new()
                    .map_err(|e| anyhow::anyhow!(e))
                    .context("could not load config/sidecars.json")?,
            );

            // Before anything asks for a port: a previous run that crashed may
            // have left a server holding one.
            tauri::async_runtime::block_on(local.reap_stale());
            tauri::async_runtime::block_on(stream.reap_stale());

            let shared = local.shared();

            // How dictation and media import start the server again when it has
            // gone out from under them. Built here because it needs both the
            // runtime and the database, and neither path holds the other.
            let revive = local_model::revive_handle(&local, &db);

            // The other local engine. No process and no port -- a selection and
            // a one-shot CLI -- so there is nothing to reap and nothing to
            // start, only a remembered choice to read back. See `local_asr`'s
            // module doc on why only one of the two is ever selected.
            let asr = std::sync::Arc::new(
                local_asr::LocalAsr::new(policy.clone())
                    .context("could not load config/sidecars.json for local ASR")?,
            );
            tauri::async_runtime::block_on(asr.restore(&db));
            let sherpa_selection = asr.selection();
            app.manage(std::sync::Arc::clone(&asr));

            // Media import: audio, video and url import all end by going
            // through the existing transcription route, so this needs exactly
            // the handles that route does. Cloned before `credentials` and
            // `policy` are moved into the dictation runtime below.
            let media_import = std::sync::Arc::new(
                media::MediaImportRuntime::new(
                    policy.clone(),
                    std::sync::Arc::clone(&credentials),
                    std::sync::Arc::clone(&shared.port),
                    std::sync::Arc::clone(&shared.model),
                    asr.selection(),
                    std::sync::Arc::clone(&revive),
                    db.clone(),
                )
                .map_err(|e| anyhow::anyhow!(e))
                .context("could not load config/sidecars.json for media import")?,
            );
            app.manage(media_import);

            // The route survives a restart; the server process does not. Bring
            // back whichever model the user was last dictating with, in the
            // background so the window does not wait for a model to load.
            local_model::resume_last_used(&local, &db);

            app.manage(std::sync::Arc::clone(&local));

            // Local text generation. Same lifecycle, one more sidecar. Nothing
            // starts here: the model is gigabytes and starting it uninvited
            // would be several of them resident before anyone asked.
            let llm = std::sync::Arc::new(
                local_llm::LocalLlm::new(policy.clone())
                    .map_err(|e| anyhow::anyhow!(e))
                    .context("could not prepare the local text-generation server")?,
            );
            tauri::async_runtime::block_on(llm.reap_stale());
            let llm_endpoint = llm.endpoint();
            app.manage(std::sync::Arc::clone(&llm));

            let session = std::sync::Arc::new(dictation::DictationRuntime::build(
                handle.clone(),
                db.clone(),
                credentials,
                policy,
                dictation::LocalTargets {
                    transcription_port: shared.port,
                    transcription_model: shared.model,
                    transcription_sherpa: sherpa_selection,
                    transcription_revive: revive,
                    generation: llm_endpoint,
                },
                // Off until `reasoning::apply_at_startup` fills it, and off
                // for good if the user has never switched cleanup on. The raw
                // transcript is a correct answer; a half-configured cleanup
                // that silently does nothing is not.
                std::sync::Arc::new(std::sync::RwLock::new(None)),
            ));

            // The route survives a restart. Bring it back, in the background:
            // the window should not wait on a settings read.
            reasoning::apply_at_startup(&session, &db);

            app.manage(session);

            // Meetings. Nothing starts here and nothing is scheduled: a meeting
            // begins only when somebody presses the button, which is what "the
            // user must know it is recording" means in practice.
            // The notification window, hidden. Created at startup rather than on
            // demand: one created at the moment a meeting is detected would
            // appear a second late and, on Windows, would steal focus while its
            // webview initialised.
            match meeting_window::create(&handle) {
                Ok(window) => {
                    let status = meeting_window::apply_treatments(&window);
                    tracing::info!(
                        capture_exclusion = ?status.capture_exclusion,
                        non_activating = ?status.non_activating,
                        "meeting notification window ready"
                    );
                }
                Err(err) => tracing::error!(
                    error = %err,
                    "no meeting notification window, so a recording meeting will have no                      indicator of its own"
                ),
            }

            // The update notification, hidden. Created here for the same
            // reason as the meeting window: one built at the moment a check
            // found something would appear a beat late and could steal focus
            // while its webview initialised. Nothing here reaches the
            // network -- see updater.rs's module doc for why creating this
            // window is not a startup check.
            match update_window::create(&handle) {
                Ok(window) => {
                    let status = update_window::apply_treatments(&window);
                    tracing::info!(
                        capture_exclusion = ?status.capture_exclusion,
                        non_activating = ?status.non_activating,
                        "update notification window ready"
                    );
                }
                Err(err) => tracing::error!(
                    error = %err,
                    "could not create the update notification window"
                ),
            }
            app.manage(updater::LastCheck::default());

            // Empty at startup, and only `pick_import_file` ever adds to it:
            // the three commands that take a file path refuse anything this
            // set does not hold. See `commands::DialogGrants`.
            app.manage(commands::DialogGrants::default());

            let meetings = std::sync::Arc::new(meeting::MeetingRuntime::new(
                handle.clone(),
                db.clone(),
                meeting_policy,
                meeting_credentials,
                local.shared(),
                // So the whole-recording pass uses the engine dictation uses.
                asr.selection(),
                std::sync::Arc::clone(&stream),
            ));
            // Polls the process table while idle and offers to record a meeting
            // in progress. There is no path from it to `start` without a click.
            meetings.watch();
            app.manage(meetings);
            app.manage(std::sync::Arc::clone(&stream));

            // Calendars. Nothing here reaches the network either: connecting an
            // account and syncing one are both explicit clicks on
            // `Integrations > Calendar` -- see calendar.rs's module doc.
            let calendars = std::sync::Arc::new(calendar::CalendarRuntime::new(
                db.clone(),
                calendar_credentials,
                calendar_policy,
            ));
            app.manage(calendars);

            // Off unless the user has switched it on. See local_api.rs.
            tauri::async_runtime::block_on(kw_storage::jobs::interrupt_unfinished(db.pool()))?;
            tauri::async_runtime::block_on(automation::clean_job_scratch(&db,
                &kw_storage::paths::audio_temp_dir().join("import")))?;
            let uploads = kw_storage::paths::database_path().with_file_name("api-uploads");
            automation::clean_uploads(&uploads)?;
            let api = std::sync::Arc::new(local_api::LocalApiRuntime::new(db)
                .with_runtime(std::sync::Arc::new(automation::DesktopRuntime::new(handle.clone())), uploads));
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

            // Held in state so a settings change replaces the hook rather than
            // trying to install a second one, which Windows refuses.
            let hooks = std::sync::Arc::new(shortcuts::Hooks::default());
            let registrations = shortcuts::register_all(&handle, &bindings, &hooks);
            app.manage(std::sync::Arc::clone(&hooks));
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
            commands::hotkey_set_mode,
            commands::history,
            commands::history_delete,
            commands::history_clear,
            commands::history_enabled,
            commands::history_set_enabled,
            commands::history_keep_raw_text,
            commands::history_set_keep_raw_text,
            commands::providers,
            commands::credential_slots,
            commands::provider_credential_set,
            commands::provider_credential_present,
            commands::probe_providers,
            reasoning::reasoning_status,
            reasoning::reasoning_set_cleanup,
            reasoning::enterprise_config_set,
            reasoning::enterprise_config_clear,
            commands::import_openwhispr,
            commands::pick_import_file,
            commands::notes_list,
            commands::notes_search,
            commands::notes_get,
            commands::notes_create,
            commands::notes_update,
            commands::notes_delete,
            commands::note_recording_get,
            commands::note_recording_attach_from_dialog,
            commands::note_transcript_rows,
            commands::folders_list,
            commands::folder_create,
            commands::folder_delete,
            commands::note_export,
            commands::note_export_to_file,
            dictation::dictation_state,
            dictation::dictation_route,
            dictation::dictation_use_local,
            dictation::dictation_set_cloud_provider,
            dictation::dictation_toggle,
            dictation::dictation_cancel,
            dictation::dictation_set_device,
            dictation::input_devices,
            dictation::insertion_supported,
            local_model::local_model_status,
            local_model::local_model_install,
            local_model::local_server_start,
            local_model::local_server_stop,
            local_model::local_server_refresh,
            local_asr::sherpa_status,
            local_asr::sherpa_install_model,
            local_asr::sherpa_use_model,
            local_asr::sherpa_use_stream_model,
            diarization::diarization_status,
            diarization::diarization_install_model,
            diarization::diarization_set_enabled,
            local_api::local_api_status,
            local_api::local_api_start,
            local_api::local_api_stop,
            integrations::cli_status,
            integrations::mcp_status,
            local_llm::local_llm_status,
            local_llm::local_llm_install,
            local_llm::local_llm_start,
            local_llm::local_llm_stop,
            local_llm::local_llm_recheck_model,
            local_llm::local_llm_refresh_server,
            media::media_import_status,
            media::media_import_snapshot,
            media::media_import_install_sidecar,
            media::media_import_audio,
            media::media_import_video,
            meeting::meeting_providers,
            meeting::meeting_status,
            meeting::meeting_start,
            meeting::meeting_stop,
            meeting::meeting_cancel,
            meeting::meeting_dismiss,
            meeting::meeting_set_prompt,
            meeting::meeting_detect_lists,
            meeting::meeting_set_detect_lists,
            meeting::meeting_language,
            meeting::meeting_set_language,
            meeting::meeting_keyterms,
            meeting::meeting_set_keyterms,
            meeting::meeting_review_config,
            updater::update_check,
            updater::update_check_last,
            updater::update_dismiss,
            calendar::calendar_status,
            calendar::calendar_set_client_id,
            calendar::calendar_connect,
            calendar::calendar_disconnect,
            calendar::calendar_sync,
            calendar::calendar_set_selected,
        ])
        .build(tauri::generate_context!())
        .context("the application could not be built")?
        .run(automation::on_event);
    Ok(())
}
