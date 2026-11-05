//! The meeting notification window.
//!
//! Compact offers are 392×102; the recording panel defaults to 440×560 and
//! preserves its own placement. Both are content-protected. The role settled in
//! `docs/PORTING_MATRIX.md`. It does two jobs in one window, which is why it is
//! one window:
//!
//! * **Offers to record** when a meeting is detected. It never records on its
//!   own; a person clicks.
//! * **Says it is recording** for as long as it is, with elapsed time and which
//!   sources are live.
//!
//! "Content-protected" means out of screen shares and **never** hidden from the
//! person at the keyboard. That is the rule at the top of `Overlay.tsx` and it is
//! the reason this window exists rather than a toast that disappears: something
//! recording a room should be visible for the whole time it is doing it.
//!
//! Startup reuses [`crate::overlay::apply_treatments`]. During a meeting,
//! `start` grants activation after showing the panel; `stop` and `cancel` revoke
//! it after hiding. Finishing stays activatable so the typed notes remain usable.
//! Reading tao 0.35.3 showed that show/hide regenerates the extended style from
//! its FOCUSABLE flag, so a raw style write cannot make activation durable.
//! Milkdown's block handle needs Tauri's native drag handler disabled (KNEG-258);
//! the capability audit's rule G cannot see Rust-built windows. If exclusion
//! fails on this platform the window still shows — a visible indicator in a screen share is a privacy
//! cost the user can be told about, while an invisible one is a recording nobody
//! knows about.

use crate::{overlay::OverlayStatus, state::AppState};
use serde::{Deserialize, Serialize};
use std::sync::Mutex;
use tauri::{
    AppHandle, LogicalSize, Manager as _, PhysicalPosition, Runtime, WebviewUrl, WebviewWindow,
    WebviewWindowBuilder,
};

/// The window label, and the fragment the frontend routes on.
pub const LABEL: &str = "meeting";

const COMPACT: (f64, f64) = (392.0, 102.0);
const PANEL: (f64, f64) = (440.0, 560.0);
const PANEL_MIN: (f64, f64) = (392.0, 280.0);
const PLACEMENT_KEY: &str = "meeting.notificationPlacement";

/// The offer strip or the note-taking recording panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    #[default]
    Compact,
    Panel,
}

impl Mode {
    pub const fn size(self) -> (f64, f64) {
        match self {
            Self::Compact => COMPACT,
            Self::Panel => PANEL,
        }
    }

    pub const fn min_size(self) -> (f64, f64) {
        match self {
            Self::Compact => COMPACT,
            Self::Panel => PANEL_MIN,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SavedPlacement {
    monitor: String,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

#[derive(Debug, Default)]
struct PlacementState {
    mode: Mutex<Mode>,
    saved: Mutex<Option<SavedPlacement>>,
    programmatic: Mutex<Option<(i32, i32, u32, u32)>>,
    writes: tokio::sync::Mutex<()>,
}

pub fn initialize(app: &AppHandle) {
    app.manage(PlacementState::default());
    let db = app.state::<AppState>();
    match tauri::async_runtime::block_on(kw_storage::settings::get::<SavedPlacement>(
        db.db().pool(),
        PLACEMENT_KEY,
    )) {
        Ok(Some(saved)) => {
            if let Ok(mut held) = app.state::<PlacementState>().saved.lock() {
                *held = Some(saved);
            }
        }
        Ok(None) => (),
        Err(error) => tracing::warn!(%error, "could not load meeting notification placement"),
    }
}

/// Create the window, hidden.
///
/// Shown when there is something to say and hidden when there is not, like the
/// overlay. Built here at startup rather than on demand because a window created
/// at the moment a meeting is detected would appear a second late and, on
/// Windows, would steal focus while its webview initialised.
pub fn create(app: &AppHandle) -> tauri::Result<WebviewWindow> {
    if let Some(existing) = app.get_webview_window(LABEL) {
        return Ok(existing);
    }

    // One bundle serves every surface and the frontend routes on the fragment,
    // as `#overlay` already does. One bundle means one CSP and one build.
    let window =
        WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("index.html#meeting".into()))
            .title("KnegisWhisper meeting")
            .inner_size(COMPACT.0, COMPACT.1)
            .min_inner_size(COMPACT.0, COMPACT.1)
            .resizable(true)
            .maximizable(false)
            .decorations(false)
            .transparent(true)
            // Left on, Windows 11 draws its own slightly rounded border around the
            // whole transparent window, outside the card's far rounder corners.
            // Off, Tauri's resize band runs inside all four edges instead.
            .shadow(false)
            .always_on_top(true)
            .skip_taskbar(true)
            // tao regenerates GWL_EXSTYLE from FOCUSABLE on every show/hide;
            // a raw WS_EX_NOACTIVATE write is lost at the next flag change.
            .focusable(false)
            .focused(false)
            // Milkdown's block handle needs native drag handling off (KNEG-258).
            // Capability audit rule G cannot inspect Rust-built windows.
            .disable_drag_drop_handler()
            .visible(false)
            .build()?;

    restore(&window, Mode::Compact);
    Ok(window)
}

/// Put the window in the bottom-right of the monitor work area it is on.
pub fn position(window: &WebviewWindow) {
    position_for(window);
}

fn position_for<R: Runtime>(window: &WebviewWindow<R>) {
    let mode = window
        .app_handle()
        .state::<PlacementState>()
        .mode
        .lock()
        .map(|m| *m)
        .unwrap_or_default();
    let (width, height) = mode.size();
    crate::window_placement::bottom_right_for(window, width, height, "meeting notification");
}

fn record_programmatic_placement<R: Runtime>(window: &WebviewWindow<R>) {
    // Restoring and resetting fire move/resize events too. Read back the final
    // physical rectangle so their delayed save cannot turn an automatic position
    // into a user preference. A timer cannot tell those events from a real drag.
    let rectangle = window
        .outer_position()
        .and_then(|position| {
            window.outer_size().map(|size| {
                (position.x, position.y, size.width, size.height)
            })
        })
        .map(Some)
        .unwrap_or_else(|error| {
            tracing::warn!(%error, "could not read meeting notification placement after positioning");
            None
        });
    match window
        .app_handle()
        .state::<PlacementState>()
        .programmatic
        .lock()
    {
        Ok(mut held) => *held = rectangle,
        Err(error) => tracing::warn!(%error, "could not record meeting notification placement"),
    }
}

fn restore(window: &WebviewWindow, mode: Mode) {
    let (min_width, min_height) = mode.min_size();
    if let Err(error) = window.set_min_size(Some(LogicalSize::new(min_width, min_height))) {
        tracing::warn!(%error, "could not set meeting notification minimum size");
    }
    let app = window.app_handle();
    let saved = app
        .state::<PlacementState>()
        .saved
        .lock()
        .map(|held| held.clone())
        .unwrap_or_else(|error| {
            tracing::warn!(%error, "could not read meeting notification placement");
            None
        });
    if let Some(saved) = saved.filter(|p| {
        mode == Mode::Panel && {
            p.x.is_finite() && p.y.is_finite() && p.width.is_finite() && p.height.is_finite()
        }
    }) {
        match window.available_monitors() {
            Ok(monitors) => {
                if let Some(monitor) = monitors
                    .into_iter()
                    .find(|m| m.name() == Some(&saved.monitor))
                {
                    let scale = monitor.scale_factor();
                    if scale.is_finite() && scale > 0.0 {
                        let size = LogicalSize::new(
                            saved.width.max(PANEL_MIN.0),
                            saved.height.max(PANEL_MIN.1),
                        );
                        let origin = monitor.position();
                        let placement = window.set_size(size).and_then(|()| {
                            window.set_position(PhysicalPosition::new(
                                f64::from(origin.x) + saved.x * scale,
                                f64::from(origin.y) + saved.y * scale,
                            ))
                        });
                        match placement {
                            Ok(()) => {
                                crate::window_placement::clamp_into_work_area(
                                    window,
                                    "meeting notification",
                                );
                                record_programmatic_placement(window);
                                return;
                            }
                            Err(error) => {
                                tracing::warn!(%error, "could not restore meeting notification placement")
                            }
                        }
                    }
                }
            }
            Err(error) => {
                tracing::warn!(%error, "could not list monitors for meeting notification")
            }
        }
    }
    position(window);
    record_programmatic_placement(window);
}

#[tauri::command]
pub async fn meeting_notification_placement_save<R: Runtime>(
    app: AppHandle<R>,
    window: WebviewWindow<R>,
) -> Result<(), String> {
    if window.label() != LABEL {
        return Err("Only the meeting window can save its placement".into());
    }
    if *app
        .state::<PlacementState>()
        .mode
        .lock()
        .map_err(|error| error.to_string())?
        != Mode::Panel
    {
        return Ok(());
    }
    let state = app.state::<PlacementState>();
    let _guard = state.writes.lock().await;
    // Stop can switch to Compact while a debounced save waits on this lock.
    if *state.mode.lock().map_err(|error| error.to_string())? != Mode::Panel {
        return Ok(());
    }
    let Some(placement) = save_placement(&app, &window).await? else {
        return Ok(());
    };
    let db = app.state::<AppState>();
    kw_storage::settings::set(db.db().pool(), PLACEMENT_KEY, &placement)
        .await
        .map_err(|error| error.to_string())?;
    *state.saved.lock().map_err(|error| error.to_string())? = Some(placement);
    Ok(())
}

// Both frontend saves and Stop use the command above. The settings audit reads
// writer calls in command bodies, so measurement is shared here and the write
// stays in that command.
async fn save_placement<R: Runtime>(
    app: &AppHandle<R>,
    window: &WebviewWindow<R>,
) -> Result<Option<SavedPlacement>, String> {
    let state = app.state::<PlacementState>();
    if !window.is_visible().map_err(|error| error.to_string())? {
        return Ok(None);
    }
    let position = window.outer_position().map_err(|error| error.to_string())?;
    let outer_size = window.outer_size().map_err(|error| error.to_string())?;
    if *state
        .programmatic
        .lock()
        .map_err(|error| error.to_string())?
        == Some((position.x, position.y, outer_size.width, outer_size.height))
    {
        return Ok(None);
    }
    let monitor = window
        .current_monitor()
        .map_err(|error| error.to_string())?
        .ok_or("Meeting notification monitor unavailable")?;
    let name = monitor
        .name()
        .ok_or("Meeting notification monitor has no name")?;
    let scale = monitor.scale_factor();
    if !scale.is_finite() || scale <= 0.0 {
        return Err("Meeting notification monitor has an invalid scale factor".into());
    }
    let origin = monitor.position();
    let size = window.inner_size().map_err(|error| error.to_string())?;
    let placement = SavedPlacement {
        monitor: name.clone(),
        x: (f64::from(position.x) - f64::from(origin.x)) / scale,
        y: (f64::from(position.y) - f64::from(origin.y)) / scale,
        width: f64::from(size.width) / scale,
        height: f64::from(size.height) / scale,
    };
    Ok(Some(placement))
}

/// Persist the last panel position before Stop switches to Compact.
pub async fn remember_panel_placement(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(LABEL) {
        if let Err(error) = meeting_notification_placement_save(app.clone(), window).await {
            tracing::warn!(%error, "could not remember meeting panel placement");
        }
    }
}

#[tauri::command]
pub async fn meeting_notification_placement_reset<R: Runtime>(
    app: AppHandle<R>,
    window: WebviewWindow<R>,
) -> Result<(), String> {
    if window.label() != "main" {
        return Err("Only the main window can reset the meeting notification placement".into());
    }
    let state = app.state::<PlacementState>();
    let _guard = state.writes.lock().await;
    let db = app.state::<AppState>();
    kw_storage::settings::remove(db.db().pool(), PLACEMENT_KEY)
        .await
        .map_err(|error| error.to_string())?;
    *state.saved.lock().map_err(|error| error.to_string())? = None;
    if let Some(notification) = app.get_webview_window(LABEL) {
        if notification
            .is_visible()
            .map_err(|error| error.to_string())?
        {
            let mode = *state.mode.lock().map_err(|error| error.to_string())?;
            let (min_width, min_height) = mode.min_size();
            notification
                .set_min_size(Some(LogicalSize::new(min_width, min_height)))
                .map_err(|error| error.to_string())?;
            position_for(&notification);
            record_programmatic_placement(&notification);
        }
    }
    Ok(())
}

/// Apply the platform treatments and report what happened.
///
/// [`crate::overlay::apply_treatments`] unchanged, so this window's exclusion is
/// evidenced the same way the overlay's is — read back from the window, never
/// assumed.
#[must_use]
pub fn apply_treatments(window: &WebviewWindow) -> OverlayStatus {
    crate::overlay::apply_treatments(window)
}

/// Apply capture exclusion without changing focusability.
pub fn capture_exclusion(window: &WebviewWindow) -> kw_overlay::CaptureExclusion {
    crate::overlay::capture_exclusion(window)
}

/// Show the window in its chosen mode.
///
/// Failing to *show* it is logged as an error, because it is one: the window is
/// the visible indicator that a meeting is being recorded, and a recording with
/// no indicator is the thing the brief forbids.
pub fn show(app: &AppHandle, mode: Mode) {
    let Some(window) = app.get_webview_window(LABEL) else {
        tracing::error!("there is no meeting notification window, so a recording meeting has no visible indicator of its own");
        return;
    };
    match app.state::<PlacementState>().mode.lock() {
        Ok(mut held) => *held = mode,
        Err(error) => tracing::warn!(%error, "could not set meeting notification mode"),
    }
    restore(&window, mode);
    if let Err(error) = window.show() {
        tracing::error!(%error, "could not show the meeting notification");
    }
}

/// Hide the meeting notification.
pub fn hide(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(LABEL) {
        if let Err(error) = window.hide() {
            tracing::warn!(%error, "could not hide the meeting notification");
        }
    }
}

/// Grant or revoke activation through tao's durable FOCUSABLE flag.
pub fn set_activatable(app: &AppHandle, activatable: bool) {
    if let Some(window) = app.get_webview_window(LABEL) {
        if let Err(error) = window.set_focusable(activatable) {
            tracing::warn!(%error, activatable, "could not change meeting notification activation");
        }
    }
}

/// Correct an edge-driven visibility state that has no other corrective.
///
/// KNEG-167 measured a visible, empty notification window after a missed hide
/// edge. Detection polling is the later observation that can repair that state.
/// This is deliberately one-directional: `start` owns showing the window at
/// the end of its work, because showing while `Starting` would flash an empty
/// panel before there is state to render.
pub fn reconcile(app: &AppHandle, has_something_to_say: bool) {
    let Some(window) = app.get_webview_window(LABEL) else {
        return;
    };

    let visible = match window.is_visible() {
        Ok(visible) => visible,
        Err(err) => {
            tracing::debug!(
                error = %err,
                "could not read the meeting notification visibility while reconciling"
            );
            return;
        }
    };

    if visible && !has_something_to_say {
        tracing::warn!(
            "a stranded meeting notification window was visible with nothing to render; hiding it"
        );
        hide(app);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_window_modes_keep_their_default_and_minimum_sizes() {
        assert_eq!(Mode::Compact.size(), (392.0, 102.0));
        assert_eq!(Mode::Compact.min_size(), (392.0, 102.0));
        assert_eq!(Mode::Panel.size(), (440.0, 560.0));
        assert_eq!(Mode::Panel.min_size(), (392.0, 280.0));
    }

    #[test]
    fn the_label_and_the_overlays_are_distinct() {
        // Both windows are found by label; one label would silently return the
        // other's window and treat the overlay as the notification.
        assert_ne!(LABEL, crate::overlay::LABEL);
    }
}

#[cfg(all(test, feature = "tauri-test"))]
#[allow(clippy::unwrap_used)]
mod ipc_tests {
    use super::*;
    use serde_json::json;

    fn invoke(
        window: &WebviewWindow<tauri::test::MockRuntime>,
        command: &str,
    ) -> Result<serde_json::Value, serde_json::Value> {
        tauri::test::get_ipc_response(
            window,
            tauri::webview::InvokeRequest {
                cmd: command.into(),
                callback: tauri::ipc::CallbackFn(0),
                error: tauri::ipc::CallbackFn(1),
                url: window.url().unwrap(),
                body: tauri::ipc::InvokeBody::Json(json!({})),
                headers: Default::default(),
                invoke_key: tauri::test::INVOKE_KEY.into(),
            },
        )
        .map(|response| response.deserialize().unwrap())
    }

    #[test]
    fn placement_commands_refuse_the_other_window_at_the_ipc_boundary() {
        let app = tauri::test::mock_builder()
            .invoke_handler(tauri::generate_handler![
                meeting_notification_placement_save,
                meeting_notification_placement_reset
            ])
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .unwrap();
        let main = WebviewWindowBuilder::new(&app, "main", Default::default())
            .build()
            .unwrap();
        let meeting = WebviewWindowBuilder::new(&app, LABEL, Default::default())
            .build()
            .unwrap();
        assert!(invoke(&main, "meeting_notification_placement_save").is_err());
        assert!(invoke(&meeting, "meeting_notification_placement_reset").is_err());
    }
}
