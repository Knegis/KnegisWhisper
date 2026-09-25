//! The meeting notification window.
//!
//! 392×92 by default and at minimum, content-protected, bottom-right until moved — the role settled in
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
//! It reuses [`crate::overlay::apply_treatments`] unchanged, so it reports the
//! same `CaptureExclusion` / `NonActivating` evidence, read back from the real
//! window rather than assumed. If exclusion fails on this platform the window
//! still shows — a visible indicator that appears in a screen share is a privacy
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

/// Default and minimum size from the porting matrix's window-role table.
const WIDTH: f64 = 392.0;
const HEIGHT: f64 = 92.0;
const PLACEMENT_KEY: &str = "meeting.notificationPlacement";

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
            .inner_size(WIDTH, HEIGHT)
            .min_inner_size(WIDTH, HEIGHT)
            .resizable(true)
            .maximizable(false)
            .decorations(false)
            .transparent(true)
            .always_on_top(true)
            .skip_taskbar(true)
            // Declarative half of not taking focus; `apply_treatments` adds the
            // imperative half. A window that grabbed focus mid-meeting would take
            // the caret out of whatever the user was typing in.
            .focused(false)
            .visible(false)
            .build()?;

    restore(&window);
    Ok(window)
}

/// Put the window in the bottom-right of the monitor work area it is on.
pub fn position(window: &WebviewWindow) {
    position_for(window);
}

fn position_for<R: Runtime>(window: &WebviewWindow<R>) {
    crate::window_placement::bottom_right_for(window, WIDTH, HEIGHT, "meeting notification");
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

fn restore(window: &WebviewWindow) {
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
        p.x.is_finite() && p.y.is_finite() && p.width.is_finite() && p.height.is_finite()
    }) {
        match window.available_monitors() {
            Ok(monitors) => {
                if let Some(monitor) = monitors
                    .into_iter()
                    .find(|m| m.name() == Some(&saved.monitor))
                {
                    let scale = monitor.scale_factor();
                    if scale.is_finite() && scale > 0.0 {
                        let size =
                            LogicalSize::new(saved.width.max(WIDTH), saved.height.max(HEIGHT));
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
    let state = app.state::<PlacementState>();
    let _guard = state.writes.lock().await;
    if !window.is_visible().map_err(|error| error.to_string())? {
        return Ok(());
    }
    let position = window.outer_position().map_err(|error| error.to_string())?;
    let outer_size = window.outer_size().map_err(|error| error.to_string())?;
    if *state
        .programmatic
        .lock()
        .map_err(|error| error.to_string())?
        == Some((position.x, position.y, outer_size.width, outer_size.height))
    {
        return Ok(());
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
    let db = app.state::<AppState>();
    kw_storage::settings::set(db.db().pool(), PLACEMENT_KEY, &placement)
        .await
        .map_err(|error| error.to_string())?;
    *state.saved.lock().map_err(|error| error.to_string())? = Some(placement);
    Ok(())
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

/// Show or hide it.
///
/// Failing to *show* it is logged as an error, because it is one: the window is
/// the visible indicator that a meeting is being recorded, and a recording with
/// no indicator is the thing the brief forbids. Failing to hide it is a warning.
pub fn set_visible(app: &AppHandle, visible: bool) {
    let Some(window) = app.get_webview_window(LABEL) else {
        if visible {
            tracing::error!(
                "there is no meeting notification window, so a recording meeting has no visible \
                 indicator of its own"
            );
        }
        return;
    };

    // Repositioned on the way up: a monitor may have been unplugged, or the
    // resolution changed, since it was created.
    if visible {
        restore(&window);
    }

    let result = if visible {
        window.show()
    } else {
        window.hide()
    };
    if let Err(err) = result {
        if visible {
            tracing::error!(error = %err, "could not show the meeting notification");
        } else {
            tracing::warn!(error = %err, "could not hide the meeting notification");
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
        set_visible(app, false);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_and_minimum_size_are_the_ones_the_porting_matrix_settled() {
        // 392x92 is the default and minimum in the window-role table.
        assert!((WIDTH - 392.0).abs() < f64::EPSILON);
        assert!((HEIGHT - 92.0).abs() < f64::EPSILON);
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
