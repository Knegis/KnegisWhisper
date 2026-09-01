//! The update notification window.
//!
//! 392×92, content-protected, bottom-right — the same role and the same size
//! `docs/PORTING_MATRIX.md` settled for the meeting notification, and built the
//! same way: [`crate::meeting_window`] is the pattern this file copies, not
//! reinvents.
//!
//! It says one thing: that a newer release of this project exists, once
//! [`crate::updater::update_check`] has found one. It never checks on its own —
//! nothing in this module reaches the network, and nothing calls
//! [`crate::updater::update_check`] from application startup. "Opt-in, and
//! never a check at startup" is the constraint on the check itself
//! (`updater.rs`); this window is downstream of that check and inherits the
//! property structurally, by having no timer and no caller but a person's own
//! request.
//!
//! "Content-protected" means out of screen shares and **never** hidden from the
//! person at the keyboard — the rule at the top of `Overlay.tsx`, and it holds
//! here for the same reason it holds for the meeting window: a window can be
//! excluded from what a screen share sees without ever being excluded from what
//! the user sees.
//!
//! It reuses [`crate::overlay::apply_treatments`] unchanged, so it reports the
//! same `CaptureExclusion` / `NonActivating` evidence, read back from the real
//! window rather than assumed.

use crate::overlay::OverlayStatus;
use tauri::{AppHandle, Manager as _, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

/// The window label, and the fragment the frontend routes on.
pub const LABEL: &str = "update";

/// Size from the porting matrix's window-role table -- the same as the meeting
/// notification, which is the row directly above this one.
const WIDTH: f64 = 392.0;
const HEIGHT: f64 = 92.0;

/// Create the window, hidden.
///
/// Shown only once a check this project's own release feed has actually found
/// something newer; hidden otherwise. Created at startup rather than on demand
/// for the same reason as the meeting notification: a window built at the
/// moment it is first needed would appear a beat late and, on Windows, would
/// steal focus while its webview initialised.
pub fn create(app: &AppHandle) -> tauri::Result<WebviewWindow> {
    if let Some(existing) = app.get_webview_window(LABEL) {
        return Ok(existing);
    }

    // One bundle serves every surface and the frontend routes on the fragment,
    // as `#overlay` and `#meeting` already do. One bundle means one CSP and one
    // build.
    let window = WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("index.html#update".into()))
        .title("KnegisWhisper update")
        .inner_size(WIDTH, HEIGHT)
        .resizable(false)
        .decorations(false)
        .transparent(true)
        .always_on_top(true)
        .skip_taskbar(true)
        // Declarative half of not taking focus; `apply_treatments` adds the
        // imperative half. A window that grabbed focus to announce a
        // release would take the caret out of whatever the user was typing.
        .focused(false)
        .visible(false)
        .build()?;

    position(&window);
    Ok(window)
}

/// Put the window in the bottom-right of the monitor work area it is on.
pub fn position(window: &WebviewWindow) {
    crate::window_placement::bottom_right(window, WIDTH, HEIGHT, "update notification");
}

/// Apply the platform treatments and report what happened.
///
/// [`crate::overlay::apply_treatments`] unchanged, so this window's exclusion is
/// evidenced the same way the overlay's and the meeting notification's are --
/// read back from the window, never assumed.
#[must_use]
pub fn apply_treatments(window: &WebviewWindow) -> OverlayStatus {
    crate::overlay::apply_treatments(window)
}

/// Show or hide it.
///
/// Unlike the meeting notification, failing to show this window is not a
/// safety property -- nobody's privacy depends on seeing it -- so both
/// directions are logged as warnings rather than one of them being an error.
pub fn set_visible(app: &AppHandle, visible: bool) {
    let Some(window) = app.get_webview_window(LABEL) else {
        tracing::warn!("no update notification window exists to show or hide");
        return;
    };

    if visible {
        position(&window);
    }

    let result = if visible {
        window.show()
    } else {
        window.hide()
    };
    if let Err(err) = result {
        tracing::warn!(
            error = %err,
            visible,
            "could not change the update notification window's visibility"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_size_matches_the_meeting_notifications_role() {
        // Both rows in the porting matrix's window-role table say 392x92.
        // `meeting_window::WIDTH`/`HEIGHT` are private to that module, so this
        // pins the literal the table settled on rather than reaching in.
        assert!((WIDTH - 392.0).abs() < f64::EPSILON);
        assert!((HEIGHT - 92.0).abs() < f64::EPSILON);
    }

    #[test]
    fn the_label_is_distinct_from_the_other_two_windows() {
        // All three are found by label; a collision would silently return the
        // wrong window.
        assert_ne!(LABEL, crate::overlay::LABEL);
        assert_ne!(LABEL, crate::meeting_window::LABEL);
    }
}
