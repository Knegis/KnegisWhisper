//! Privacy overlay state and capture-exclusion diagnostics.
//!
//! The window itself is created by the Tauri shell; this crate owns the *honest
//! reporting* of what the platform actually did.
//!
//! The design constraint is one finding from the upstream audit: Pluely applies
//! Tauri's `contentProtected` flag and then reports nothing. Its README promises
//! "absolute stealth" and being "undetectable everywhere", while the mechanism is
//! a no-op on Linux and is never verified anywhere. We do the opposite — a
//! capability is reported as active only with evidence, and `unknown` is a
//! perfectly good answer.
//!
//! See `docs/PLATFORM_SUPPORT.md`.

use serde::Serialize;

/// How we know a capture-exclusion claim is true.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Evidence {
    /// The platform exposes a getter and it returned the expected value. This is
    /// the only evidence that justifies [`CaptureExclusion::Active`].
    ReadBack {
        /// The API consulted, e.g. `NSWindow.sharingType`.
        api: &'static str,
        /// The value it returned.
        value: String,
    },
}

/// The result of attempting to exclude a window from screen capture.
///
/// Four states rather than a boolean, because "we called the API and it did not
/// complain" is genuinely different from "we know this works".
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "kebab-case")]
pub enum CaptureExclusion {
    /// Applied and positively verified.
    Active { verified_by: Evidence },
    /// The platform or compositor cannot do this at all.
    Unsupported { reason: String },
    /// The attempt returned an error.
    Failed { error: String },
    /// Applied, but the result cannot be read back, so we will not claim it
    /// worked. This is the expected state on Windows and macOS.
    Unknown { reason: String },
}

impl CaptureExclusion {
    /// Whether the user should be told the overlay is hidden from capture.
    ///
    /// Only [`CaptureExclusion::Active`] qualifies. This method exists so no UI
    /// code has to decide for itself, and so `Unknown` can never be rendered as
    /// a reassuring green check.
    #[must_use]
    pub const fn is_confirmed(&self) -> bool {
        matches!(self, Self::Active { .. })
    }

    /// A short, honest phrase for the settings UI.
    #[must_use]
    pub fn summary(&self) -> &'static str {
        match self {
            Self::Active { .. } => "Hidden from screen capture",
            Self::Unsupported { .. } => "Not supported on this system",
            Self::Failed { .. } => "Could not be applied",
            Self::Unknown { .. } => "Applied, but cannot be verified",
        }
    }

    /// Construct the state for a platform that has no such facility.
    #[must_use]
    pub fn unsupported(reason: impl Into<String>) -> Self {
        Self::Unsupported {
            reason: reason.into(),
        }
    }

    /// Construct the state for "the call succeeded but nothing can confirm it".
    #[must_use]
    pub fn unknown(reason: impl Into<String>) -> Self {
        Self::Unknown {
            reason: reason.into(),
        }
    }
}

/// The display server in use, which decides what the overlay can do on Linux.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum DisplayServer {
    Windows,
    MacOs,
    X11,
    Wayland,
    Unknown,
}

/// The Linux compositor, where behaviour differs enough to matter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Compositor {
    Gnome,
    Kde,
    Hyprland,
    Sway,
    Other,
    NotApplicable,
}

/// What the overlay can actually do here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OverlayCapabilities {
    pub display_server: DisplayServer,
    pub compositor: Compositor,
    /// Whether the window avoids taking focus from the foreground application.
    pub non_activating: bool,
    /// Whether mouse events can be passed through to the window beneath.
    pub click_through: bool,
    /// Whether the overlay can float above full-screen applications.
    pub above_fullscreen: bool,
    pub capture_exclusion: CaptureExclusion,
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
    fn only_active_is_confirmed() {
        let active = CaptureExclusion::Active {
            verified_by: Evidence::ReadBack {
                api: "NSWindow.sharingType",
                value: "NSWindowSharingNone".to_owned(),
            },
        };
        assert!(active.is_confirmed());

        // The whole point: none of these may read as success.
        assert!(!CaptureExclusion::unknown("no getter available").is_confirmed());
        assert!(!CaptureExclusion::unsupported("wayland has no such api").is_confirmed());
        assert!(!CaptureExclusion::Failed {
            error: "E_INVALIDARG".to_owned()
        }
        .is_confirmed());
    }

    #[test]
    fn unknown_does_not_describe_itself_as_hidden() {
        let summary = CaptureExclusion::unknown("SetWindowDisplayAffinity has no getter").summary();
        assert!(!summary.to_lowercase().contains("hidden"));
        assert_eq!(summary, "Applied, but cannot be verified");
    }

    #[test]
    fn states_serialize_with_a_discriminant_the_ui_can_switch_on() {
        let json = serde_json::to_value(CaptureExclusion::unsupported("linux")).unwrap();
        assert_eq!(json["state"], "unsupported");
        assert_eq!(json["reason"], "linux");

        let json = serde_json::to_value(CaptureExclusion::Active {
            verified_by: Evidence::ReadBack {
                api: "test",
                value: "v".to_owned(),
            },
        })
        .unwrap();
        assert_eq!(json["state"], "active");
        assert_eq!(json["verified_by"]["kind"], "read-back");
    }

    #[test]
    fn active_cannot_be_built_without_evidence() {
        // A compile-time property, asserted here as documentation: the Active
        // variant has no Default and no evidence-free constructor, so there is
        // no way to claim success without naming how it was verified.
        let e = Evidence::ReadBack {
            api: "GetWindowDisplayAffinity",
            value: "WDA_EXCLUDEFROMCAPTURE".to_owned(),
        };
        let active = CaptureExclusion::Active { verified_by: e };
        match active {
            CaptureExclusion::Active { verified_by } => {
                let Evidence::ReadBack { api, .. } = verified_by;
                assert!(!api.is_empty());
            }
            other => panic!("expected Active, got {other:?}"),
        }
    }
}
