//! The meeting notification window.
//!
//! 392×92, content-protected, bottom-right — the role settled in
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

use crate::overlay::OverlayStatus;
use tauri::{AppHandle, Manager as _, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

/// The window label, and the fragment the frontend routes on.
pub const LABEL: &str = "meeting";

/// Size from the porting matrix's window-role table.
const WIDTH: f64 = 392.0;
const HEIGHT: f64 = 92.0;

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
            .resizable(false)
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

    position(&window);
    Ok(window)
}

/// Put the window in the bottom-right of the monitor work area it is on.
pub fn position(window: &WebviewWindow) {
    crate::window_placement::bottom_right(window, WIDTH, HEIGHT, "meeting notification");
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

    // Re-positioned on the way up: a monitor may have been unplugged, or the
    // resolution changed, since it was created.
    if visible {
        position(&window);
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
    fn the_size_is_the_one_the_porting_matrix_settled() {
        // 392x92 is in the window-role table. A window that drifted from it would
        // be a silent decision to lay the role out differently.
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
