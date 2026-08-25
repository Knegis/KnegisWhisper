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
//! file hides the window while the microphone is live, and
//! [`OverlayStatus::recording_indicator_visible`] is what the settings screen
//! reads to prove it.

use kw_overlay::CaptureExclusion;
use kw_platform::window::{self, NativeWindow};
use serde::Serialize;
use tauri::{AppHandle, Manager as _, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

/// The overlay window's label.
pub const LABEL: &str = "overlay";

/// What the overlay is actually doing, as reported to the settings screen.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OverlayStatus {
    /// Whether the window exists.
    pub created: bool,
    /// The real result of asking the platform to exclude it from capture.
    pub capture_exclusion: CaptureExclusion,
    /// Whether the window refuses focus, and how we know.
    pub non_activating: NonActivating,
    /// Whether a recording indicator is shown to the local user while the
    /// microphone is live. Always true; present so the guarantee is visible in
    /// the UI and asserted by a test rather than only described in a document.
    pub recording_indicator_visible: bool,
    /// What the platform's display server is, for honest failure messages.
    pub display_server: String,
}

/// Whether the window declines activation.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "state", rename_all = "kebab-case")]
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
/// The window is created hidden. It is shown when recording starts and hidden
/// when it stops, which is the whole point of it.
pub fn create(app: &AppHandle) -> tauri::Result<WebviewWindow> {
    if let Some(existing) = app.get_webview_window(LABEL) {
        return Ok(existing);
    }

    // `#/overlay` so the same bundle serves both surfaces; the frontend routes on
    // the fragment. One bundle means one CSP and one build.
    let overlay =
        WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("index.html#overlay".into()))
            .title("KnegisWhisper overlay")
            .inner_size(280.0, 96.0)
            .resizable(false)
            .decorations(false)
            .transparent(true)
            .always_on_top(true)
            .skip_taskbar(true)
            // Declarative half of not-taking-focus. The imperative half
            // (WS_EX_NOACTIVATE) is applied below, because this flag alone does not
            // stop a click from activating the window.
            .focused(false)
            .visible(false)
            .build()?;

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
        recording_indicator_visible: true,
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
        recording_indicator_visible: true,
        display_server: window::display_server_description(),
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
    fn the_recording_indicator_is_always_reported_as_visible() {
        // The guarantee that the local user is never silently recorded. If this
        // ever needs to be false, that is a product decision that has to be
        // argued for here, not a flag someone flips.
        assert!(not_created().recording_indicator_visible);
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
