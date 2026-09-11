//! The privacy overlay window.
//!
//! A small, always-on-top, non-activating window that shows recording state. Two
//! properties matter and both are reported rather than assumed:
//!
//! * **It must not steal focus.** Dictation is useless if pressing the hotkey
//!   moves the caret out of the document being typed into.
//! * **It should be excluded from screen capture** where the platform can do
//!   that — and where it cannot, the user is told so plainly.
//!
//! There is also a rule here that is not negotiable: **the overlay is visible
//! whenever recording is active.** The local user always knows. Nothing in this
//! file hides the window while the microphone is live. The optional floating
//! microphone also remains visible while idle.
//! [`OverlayStatus::recording_indicator_visible`] is what the settings screen
//! reads to prove it.

use kw_overlay::CaptureExclusion;
use kw_platform::window::{self, NativeWindow};
use serde::Serialize;
use tauri::{AppHandle, Manager as _, WebviewUrl, WebviewWindow, WebviewWindowBuilder};
use ts_rs::TS;

/// The overlay window's label.
pub const LABEL: &str = "overlay";

/// Native window size; the frontend lays its recording indicator out inside it.
const WIDTH: f64 = 280.0;
const HEIGHT: f64 = 96.0;

/// What the overlay is actually doing, as reported to the settings screen.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct OverlayStatus {
    /// Whether the window exists.
    pub created: bool,
    /// The real result of asking the platform to exclude it from capture.
    pub capture_exclusion: CaptureExclusion,
    /// Whether the window refuses focus, and how we know.
    pub non_activating: NonActivating,
    /// Whether dictation is recording and its indicator window is visible.
    /// An idle persistent microphone does not count as a recording indicator.
    pub recording_indicator_visible: bool,
    /// What the platform's display server is, for honest failure messages.
    pub display_server: String,
}

/// Whether the window declines activation.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(
    tag = "state",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum NonActivating {
    /// Applied and read back from the window's extended style.
    Active { verified_by: &'static str },
    /// The platform has no equivalent, or it is not implemented here.
    Unsupported { reason: String },
    /// The attempt failed.
    Failed { error: String },
}

impl NonActivating {
    #[must_use]
    pub const fn is_confirmed(&self) -> bool {
        matches!(self, Self::Active { .. })
    }
}

/// Create the overlay window and apply the platform treatments.
///
/// The window starts hidden. The presentation controller shows it during
/// recording, or continuously when the floating microphone is enabled.
pub fn create(app: &AppHandle) -> tauri::Result<WebviewWindow> {
    if let Some(existing) = app.get_webview_window(LABEL) {
        return Ok(existing);
    }

    // `#/overlay` so the same bundle serves both surfaces; the frontend routes on
    // the fragment. One bundle means one CSP and one build.
    let overlay =
        WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("index.html#overlay".into()))
            .title("KnegisWhisper overlay")
            .inner_size(WIDTH, HEIGHT)
            .resizable(false)
            .decorations(false)
            .transparent(true)
            // A native shadow outlines the whole transparent window as a box.
            .shadow(false)
            .always_on_top(true)
            .skip_taskbar(true)
            // Keep non-activation in Tauri's own window flags: resizing can
            // overwrite a style applied only through Get/SetWindowLongPtr.
            // The imperative read-back below still verifies the actual result.
            .focusable(!cfg!(target_os = "windows"))
            .focused(false)
            .visible(false)
            .build()?;

    crate::persistent_overlay::create_language_popup(app)?;
    crate::window_placement::bottom_center(&overlay, WIDTH, HEIGHT, "dictation overlay");
    Ok(overlay)
}

/// Apply capture exclusion and non-activation, and report what happened.
///
/// Called after the window exists, because both need a native handle.
#[must_use]
pub fn apply_treatments(overlay: &WebviewWindow) -> OverlayStatus {
    let handle = native_handle(overlay);

    let capture_exclusion = match handle {
        Some(h) => window::exclude_from_capture(h),
        None => CaptureExclusion::Failed {
            error: "the window has no native handle on this platform".to_owned(),
        },
    };

    let non_activating = match handle {
        Some(h) => match window::set_non_activating(h) {
            Ok(true) => NonActivating::Active {
                verified_by: "GetWindowLongPtrW(GWL_EXSTYLE) & WS_EX_NOACTIVATE",
            },
            Ok(false) => NonActivating::Failed {
                error: "the style did not take effect".to_owned(),
            },
            Err(kw_platform::PlatformError::Unsupported(reason)) => {
                NonActivating::Unsupported { reason }
            }
            Err(other) => NonActivating::Failed {
                error: other.to_string(),
            },
        },
        None => NonActivating::Unsupported {
            reason: "no native window handle is available on this platform".to_owned(),
        },
    };

    if !non_activating.is_confirmed() {
        // The overlay taking focus would move the caret out of whatever the user
        // is dictating into, so this is a functional problem, not cosmetic.
        tracing::warn!(
            state = ?non_activating,
            "the overlay is not confirmed to refuse focus; it may steal the caret"
        );
    }

    if !capture_exclusion.is_confirmed() {
        // Worth a log line at info: it is a privacy-relevant limitation, not a
        // detail.
        tracing::info!(
            state = ?capture_exclusion,
            "the overlay is not confirmed to be excluded from screen capture"
        );
    }

    OverlayStatus {
        created: true,
        capture_exclusion,
        non_activating,
        // Read, not assumed. A window that failed to show would otherwise
        // report an indicator the user cannot see.
        recording_indicator_visible: crate::persistent_overlay::recording(overlay.app_handle())
            && overlay.is_visible().unwrap_or(false),
        display_server: window::display_server_description(),
    }
}

/// The OS window handle, where the platform has one we can use.
fn native_handle(overlay: &WebviewWindow) -> Option<NativeWindow> {
    #[cfg(target_os = "windows")]
    {
        overlay
            .hwnd()
            .ok()
            .map(|hwnd| NativeWindow(hwnd.0 as isize))
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = overlay;
        None
    }
}

/// The status when no overlay has been created yet.
///
/// Never claims exclusion: nothing has been applied to anything.
#[must_use]
pub fn not_created() -> OverlayStatus {
    OverlayStatus {
        created: false,
        capture_exclusion: CaptureExclusion::unknown(
            "the overlay window has not been created yet, so nothing has been applied",
        ),
        non_activating: NonActivating::Unsupported {
            reason: "the overlay window has not been created yet".to_owned(),
        },
        // No window, so nothing is shown. Claiming otherwise here was the
        // clearest form of the same mistake.
        recording_indicator_visible: false,
        display_server: window::display_server_description(),
    }
}

/// Show or hide the overlay.
///
/// The only thing that changes the overlay's visibility. Called from the
/// dictation state sink with `state.is_recording()`, which is what keeps the
/// promise that the local user always knows the microphone is open.
///
/// Failures are logged rather than returned: the caller is a synchronous state
/// callback that cannot meaningfully handle one, and a dictation must not fail
/// because a window would not show. But it is logged at `error` when *showing*
/// fails, because that is a recording the user cannot see.
pub fn set_visible(app: &AppHandle, visible: bool) {
    let Some(overlay) = app.get_webview_window(LABEL) else {
        if visible {
            tracing::error!(
                "recording started but there is no overlay window to show, so the user has no                  indication the microphone is open"
            );
        }
        return;
    };

    if visible {
        // A monitor may have moved or changed scale while the overlay was
        // hidden. Re-read the work area before every recording starts.
        crate::window_placement::bottom_center(&overlay, WIDTH, HEIGHT, "dictation overlay");
    }

    let result = if visible {
        overlay.show()
    } else {
        overlay.hide()
    };

    if let Err(err) = result {
        if visible {
            tracing::error!(error = %err, "could not show the recording overlay");
        } else {
            tracing::warn!(error = %err, "could not hide the recording overlay");
        }
    }
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

    #[test]
    fn an_uncreated_overlay_never_claims_capture_exclusion() {
        let status = not_created();
        assert!(!status.created);
        assert!(!status.capture_exclusion.is_confirmed());
        assert!(!status.non_activating.is_confirmed());
    }

    #[test]
    fn an_uncreated_overlay_reports_no_indicator() {
        // This asserted the opposite until dictation was wired: the field was a
        // hardcoded `true`, so it claimed an indicator for a window that did not
        // exist. The guarantee is not "the field says true" — it is that
        // `set_visible` is driven by `SessionState::is_recording`, which
        // `crate::dictation::EventSink` does and a test there covers. Reporting
        // an indicator nobody can see was the weaker claim pretending to be the
        // stronger one.
        assert!(!not_created().recording_indicator_visible);
    }

    #[test]
    fn non_activating_states_serialize_with_a_discriminant() {
        let json = serde_json::to_value(NonActivating::Unsupported {
            reason: "wayland".to_owned(),
        })
        .unwrap();
        assert_eq!(json["state"], "unsupported");

        let json = serde_json::to_value(NonActivating::Active {
            verified_by: "test",
        })
        .unwrap();
        assert_eq!(json["state"], "active");
    }

    #[test]
    fn only_active_counts_as_confirmed_non_activation() {
        assert!(NonActivating::Active { verified_by: "x" }.is_confirmed());
        assert!(!NonActivating::Unsupported {
            reason: String::new()
        }
        .is_confirmed());
        assert!(!NonActivating::Failed {
            error: String::new()
        }
        .is_confirmed());
    }

    #[test]
    fn the_status_serializes_with_the_keys_the_frontend_expects() {
        let json = serde_json::to_value(not_created()).unwrap();
        for key in [
            "created",
            "captureExclusion",
            "nonActivating",
            "recordingIndicatorVisible",
            "displayServer",
        ] {
            assert!(json.get(key).is_some(), "missing {key}");
        }
    }
}
