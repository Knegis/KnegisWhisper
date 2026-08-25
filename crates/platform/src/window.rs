//! Window-level platform calls: capture exclusion, non-activating windows, and
//! the synthetic paste keystroke.
//!
//! ## Capture exclusion, and a correction to the plan
//!
//! The Phase 0 audit recorded that `SetWindowDisplayAffinity` has "no documented
//! getter", and concluded Windows would have to report `unknown`. That is wrong:
//! `GetWindowDisplayAffinity` exists and has since Windows 7. So Windows can
//! report [`CaptureExclusion::Active`] with real read-back evidence.
//!
//! What the read-back proves is worth being precise about, because it is easy to
//! overclaim. It proves **the flag is set on the window**. It does not prove a
//! particular screen recorder is actually excluded — that depends on the capture
//! API the recorder uses, and there is no way to interrogate that from inside
//! this process. `Evidence::ReadBack` names the API it consulted for exactly
//! this reason, so the claim stays attached to its source.
//!
//! Nothing here reports success without asking the OS what happened.

use kw_overlay::{CaptureExclusion, Evidence};

use crate::PlatformError;

/// A native window handle, as an opaque integer.
///
/// Taken as a plain `isize` so the Tauri shell can pass whatever
/// `window.hwnd()` gives it without this crate depending on Tauri.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NativeWindow(pub isize);

impl NativeWindow {
    #[must_use]
    pub const fn is_valid(self) -> bool {
        self.0 != 0
    }
}

/// Ask the platform to exclude `window` from screen capture, then ask it
/// whether that worked.
///
/// Never returns `Active` without evidence. On a platform with no such facility
/// this returns `Unsupported` rather than a hopeful `Active`.
#[must_use]
pub fn exclude_from_capture(window: NativeWindow) -> CaptureExclusion {
    if !window.is_valid() {
        return CaptureExclusion::Failed {
            error: "the window has no native handle yet".to_owned(),
        };
    }

    #[cfg(target_os = "windows")]
    {
        windows_impl::exclude_from_capture(window)
    }

    #[cfg(target_os = "macos")]
    {
        let _ = window;
        // NSWindow.sharingType = .none is the mechanism, and it is readable, so
        // this can become Active with evidence. Not written yet: it cannot be
        // compiled or run from this machine, and shipping an unverified claim
        // about a privacy feature is worse than shipping an honest gap.
        CaptureExclusion::unsupported(
            "not implemented on macOS yet; NSWindow.sharingType is the intended mechanism",
        )
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        let _ = window;
        // Not a gap in this implementation -- there is no such facility. Neither
        // X11 nor any current Wayland protocol lets a client ask to be omitted
        // from a screen capture. Pluely applies Tauri's contentProtected flag
        // here, which is a no-op, and its README claims the feature works
        // anyway. This is that claim being declined.
        CaptureExclusion::unsupported(format!(
            "no display server on this platform supports per-window capture exclusion \
             (detected {})",
            display_server_description()
        ))
    }
}

/// Make `window` refuse focus, so clicking it does not steal the caret from
/// whatever the user is typing into.
///
/// Returns what the platform actually did. Pluely's implementation of this for
/// Windows is commented out in its source; there is no reference to copy.
pub fn set_non_activating(window: NativeWindow) -> Result<bool, PlatformError> {
    if !window.is_valid() {
        return Err(PlatformError::Window(
            "the window has no native handle yet".to_owned(),
        ));
    }

    #[cfg(target_os = "windows")]
    {
        windows_impl::set_non_activating(window)
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = window;
        Err(PlatformError::Unsupported(
            "non-activating windows are implemented for Windows only so far".to_owned(),
        ))
    }
}

/// A description of the display server, used in honest `Unsupported` reasons.
#[must_use]
pub fn display_server_description() -> String {
    if cfg!(target_os = "windows") {
        return "the Windows desktop window manager".to_owned();
    }
    if cfg!(target_os = "macos") {
        return "Quartz".to_owned();
    }

    let wayland = std::env::var_os("WAYLAND_DISPLAY").is_some();
    let x11 = std::env::var_os("DISPLAY").is_some();
    let desktop = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_else(|_| "unknown".to_owned());

    match (wayland, x11) {
        (true, _) => format!("Wayland under {desktop}"),
        (false, true) => format!("X11 under {desktop}"),
        (false, false) => "no display server".to_owned(),
    }
}

/// The Windows paste keystroke.
#[cfg(target_os = "windows")]
#[derive(Debug, Default, Clone, Copy)]
pub struct WindowsPaste;

#[cfg(target_os = "windows")]
impl crate::insertion::PasteKeystroke for WindowsPaste {
    fn send(&self) -> Result<(), PlatformError> {
        windows_impl::send_ctrl_v()
    }

    fn describe(&self) -> &'static str {
        "SendInput Ctrl+V"
    }
}

#[cfg(target_os = "windows")]
mod windows_impl {
    use super::{CaptureExclusion, Evidence, NativeWindow, PlatformError};

    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP,
        VK_CONTROL, VK_V,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        GetWindowDisplayAffinity, GetWindowLongPtrW, SetWindowDisplayAffinity, SetWindowLongPtrW,
        GWL_EXSTYLE, WDA_EXCLUDEFROMCAPTURE, WS_EX_NOACTIVATE,
    };

    fn hwnd(window: NativeWindow) -> HWND {
        HWND(window.0 as *mut core::ffi::c_void)
    }

    pub(super) fn exclude_from_capture(window: NativeWindow) -> CaptureExclusion {
        let handle = hwnd(window);

        // WDA_EXCLUDEFROMCAPTURE needs Windows 10 2004 (build 19041). On older
        // builds the call fails, which lands in the Failed arm below with the
        // OS's own message rather than a guess.
        if let Err(err) = unsafe { SetWindowDisplayAffinity(handle, WDA_EXCLUDEFROMCAPTURE) } {
            return CaptureExclusion::Failed {
                error: format!(
                    "SetWindowDisplayAffinity(WDA_EXCLUDEFROMCAPTURE) failed: {err}. \
                     This needs Windows 10 version 2004 or later."
                ),
            };
        }

        // Ask what actually took effect. This is the whole point: the affinity
        // is readable, so there is no need to assume.
        let mut affinity: u32 = 0;
        if let Err(err) = unsafe { GetWindowDisplayAffinity(handle, &mut affinity) } {
            return CaptureExclusion::unknown(format!(
                "the flag was set, but GetWindowDisplayAffinity failed ({err}), \
                 so it cannot be confirmed"
            ));
        }

        if affinity == WDA_EXCLUDEFROMCAPTURE.0 {
            CaptureExclusion::Active {
                verified_by: Evidence::ReadBack {
                    api: "GetWindowDisplayAffinity",
                    value: "WDA_EXCLUDEFROMCAPTURE".to_owned(),
                },
            }
        } else {
            // The call reported success and the read-back disagrees. Say so
            // rather than trusting the first answer.
            CaptureExclusion::Failed {
                error: format!(
                    "SetWindowDisplayAffinity reported success but the window's affinity \
                     reads back as {affinity:#x}, not WDA_EXCLUDEFROMCAPTURE"
                ),
            }
        }
    }

    pub(super) fn set_non_activating(window: NativeWindow) -> Result<bool, PlatformError> {
        let handle = hwnd(window);

        let current = unsafe { GetWindowLongPtrW(handle, GWL_EXSTYLE) };
        if current == 0 {
            // GetWindowLongPtrW returns 0 both for "no extended styles" and for
            // an error, and the distinction needs GetLastError. A window with no
            // extended style bits at all is not something Tauri produces, so
            // treat it as the error it almost certainly is.
            return Err(PlatformError::Window(
                "GetWindowLongPtrW(GWL_EXSTYLE) returned 0; the handle is probably invalid"
                    .to_owned(),
            ));
        }

        let wanted = current | (WS_EX_NOACTIVATE.0 as isize);
        unsafe { SetWindowLongPtrW(handle, GWL_EXSTYLE, wanted) };

        // Read back rather than trusting the return value, which is the previous
        // style and so cannot tell us whether the new one stuck.
        let now = unsafe { GetWindowLongPtrW(handle, GWL_EXSTYLE) };
        let applied = now & (WS_EX_NOACTIVATE.0 as isize) != 0;

        if !applied {
            return Err(PlatformError::Window(
                "WS_EX_NOACTIVATE did not stick; the window still accepts activation".to_owned(),
            ));
        }

        Ok(true)
    }

    pub(super) fn send_ctrl_v() -> Result<(), PlatformError> {
        // Ctrl down, V down, V up, Ctrl up -- as one SendInput call, so nothing
        // can interleave between the modifier and the key.
        let key = |vk: windows::Win32::UI::Input::KeyboardAndMouse::VIRTUAL_KEY,
                   flags: KEYBD_EVENT_FLAGS| INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: vk,
                    wScan: 0,
                    dwFlags: flags,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        };

        let inputs = [
            key(VK_CONTROL, KEYBD_EVENT_FLAGS(0)),
            key(VK_V, KEYBD_EVENT_FLAGS(0)),
            key(VK_V, KEYEVENTF_KEYUP),
            key(VK_CONTROL, KEYEVENTF_KEYUP),
        ];

        let size = i32::try_from(core::mem::size_of::<INPUT>()).map_err(|_| {
            PlatformError::TextInsertion("INPUT struct size does not fit in an i32".to_owned())
        })?;

        let sent = unsafe { SendInput(&inputs, size) };

        // SendInput returns how many events it managed to insert. A short count
        // means something blocked the input -- most often UIPI, when the
        // foreground window belongs to a more privileged process.
        if sent as usize != inputs.len() {
            return Err(PlatformError::TextInsertion(format!(
                "SendInput delivered {sent} of {} events. The foreground window may belong to \
                 a process running at a higher integrity level, which Windows will not let a \
                 normal application send input to.",
                inputs.len()
            )));
        }

        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn an_invalid_handle_fails_rather_than_reporting_exclusion() {
        let outcome = exclude_from_capture(NativeWindow(0));
        assert!(!outcome.is_confirmed());
        assert!(matches!(outcome, CaptureExclusion::Failed { .. }));
    }

    #[test]
    fn an_invalid_handle_cannot_be_made_non_activating() {
        assert!(set_non_activating(NativeWindow(0)).is_err());
    }

    #[test]
    fn a_bogus_handle_never_yields_active() {
        // The property that matters: no path through this function reports
        // success for a window that does not exist. On Windows the OS call
        // fails; elsewhere the platform is unsupported.
        let outcome = exclude_from_capture(NativeWindow(0xDEAD_BEEF));
        assert!(
            !outcome.is_confirmed(),
            "a nonexistent window must never read as capture-excluded, got {outcome:?}"
        );
    }

    #[test]
    fn the_display_server_description_is_never_empty() {
        let described = display_server_description();
        assert!(!described.is_empty());
        if cfg!(target_os = "windows") {
            assert!(described.contains("Windows"));
        }
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    #[test]
    fn linux_reports_unsupported_with_the_detected_display_server() {
        match exclude_from_capture(NativeWindow(1)) {
            CaptureExclusion::Unsupported { reason } => {
                assert!(reason.contains("capture exclusion"));
            }
            other => panic!("expected Unsupported on Linux, got {other:?}"),
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_is_honest_about_not_being_implemented() {
        match exclude_from_capture(NativeWindow(1)) {
            CaptureExclusion::Unsupported { reason } => {
                assert!(reason.contains("not implemented"));
            }
            other => panic!("expected Unsupported on macOS, got {other:?}"),
        }
    }
}
