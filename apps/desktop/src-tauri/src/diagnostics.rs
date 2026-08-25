//! The `diagnostics` command.
//!
//! Reports what this machine can actually do. Every field is either a fact read
//! from the OS or an explicitly-unverified state — nothing here is aspirational,
//! because this report is what `docs/PLATFORM_SUPPORT.md` promises to be honest
//! about.

use kw_overlay::CaptureExclusion;
use kw_security::{CredentialError, CredentialStore, KeyRef, OsCredentialStore};
use serde::Serialize;

/// Whether the OS credential store is usable.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "state", rename_all = "kebab-case")]
pub enum CredentialStoreStatus {
    Available { backend: &'static str },
    Unavailable { reason: String },
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Diagnostics {
    pub app_version: &'static str,
    pub os: &'static str,
    pub arch: &'static str,
    pub webview_version: Option<String>,
    pub credential_store: CredentialStoreStatus,
    pub capture_exclusion: CaptureExclusion,
    pub allowed_egress_purposes: Vec<&'static str>,
}

/// Name of the credential backend on this platform, for display only.
const fn credential_backend() -> &'static str {
    if cfg!(target_os = "windows") {
        "Windows Credential Manager"
    } else if cfg!(target_os = "macos") {
        "macOS Keychain"
    } else {
        "Secret Service"
    }
}

/// Probe the credential store without writing anything.
///
/// Reading a slot that does not exist is the cheapest way to tell a working
/// backend from a missing one: `NotFound` proves the backend answered, whereas
/// `NoBackend` means there is nothing to answer. We deliberately do not write a
/// sentinel — a diagnostics call should not leave anything in the user's
/// keychain.
fn probe_credential_store() -> CredentialStoreStatus {
    let Some(slot) = KeyRef::new("diagnostics-probe") else {
        return CredentialStoreStatus::Unavailable {
            reason: "internal error: probe slot name rejected".to_owned(),
        };
    };

    match OsCredentialStore::new().get(&slot) {
        // The backend answered; there is simply nothing stored, which is
        // exactly what we expect.
        Ok(_) | Err(CredentialError::NotFound(_)) => CredentialStoreStatus::Available {
            backend: credential_backend(),
        },
        Err(CredentialError::NoBackend { detail }) => {
            CredentialStoreStatus::Unavailable { reason: detail }
        }
        Err(other) => CredentialStoreStatus::Unavailable {
            reason: other.to_string(),
        },
    }
}

/// What capture exclusion will do on this platform.
///
/// The overlay window does not exist yet, so this reports the *platform's*
/// expected outcome rather than a live window state. It is still never
/// `Active`: on Windows and macOS the call cannot be read back, and on Linux
/// there is no such facility at all.
fn capture_exclusion_outlook() -> CaptureExclusion {
    #[cfg(target_os = "windows")]
    {
        CaptureExclusion::unknown(
            "SetWindowDisplayAffinity(WDA_EXCLUDEFROMCAPTURE) will be applied to the \
             overlay, but Windows exposes no getter to confirm it took effect. Requires \
             Windows 10 2004 or later.",
        )
    }
    #[cfg(target_os = "macos")]
    {
        CaptureExclusion::unknown(
            "NSWindowSharingNone will be applied to the overlay, but the result is not \
             read back.",
        )
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        CaptureExclusion::unsupported(
            "Neither X11 nor the Wayland compositors expose per-window capture \
             exclusion. The overlay is visible to screen-recording software.",
        )
    }
}

#[tauri::command]
pub fn diagnostics() -> Diagnostics {
    Diagnostics {
        app_version: env!("CARGO_PKG_VERSION"),
        os: std::env::consts::OS,
        arch: std::env::consts::ARCH,
        webview_version: tauri::webview_version().ok(),
        credential_store: probe_credential_store(),
        capture_exclusion: capture_exclusion_outlook(),
        allowed_egress_purposes: kw_network_policy::ALL_PURPOSES
            .iter()
            .map(|p| p.as_str())
            .collect(),
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
    fn capture_exclusion_is_never_reported_as_active() {
        // Nothing has been applied to any window yet, so claiming Active would
        // be a fabrication regardless of platform.
        assert!(!capture_exclusion_outlook().is_confirmed());
    }

    #[test]
    fn linux_reports_unsupported_rather_than_unknown() {
        let outlook = capture_exclusion_outlook();
        if cfg!(not(any(target_os = "windows", target_os = "macos"))) {
            assert!(matches!(outlook, CaptureExclusion::Unsupported { .. }));
        }
    }

    #[test]
    fn diagnostics_serializes_with_the_keys_the_frontend_expects() {
        let json = serde_json::to_value(diagnostics()).unwrap();
        for key in [
            "appVersion",
            "os",
            "arch",
            "webviewVersion",
            "credentialStore",
            "captureExclusion",
            "allowedEgressPurposes",
        ] {
            assert!(json.get(key).is_some(), "missing key {key}");
        }
        assert!(json["captureExclusion"]["state"].is_string());
        assert_eq!(
            json["allowedEgressPurposes"].as_array().map(Vec::len),
            Some(7),
            "all seven egress purposes should be reported"
        );
    }

    #[test]
    fn diagnostics_leaks_no_secret_material() {
        let rendered = serde_json::to_string(&diagnostics()).unwrap();
        for needle in ["sk-", "Bearer ", "password", "api_key"] {
            assert!(
                !rendered.contains(needle),
                "diagnostics output contains {needle:?}"
            );
        }
    }
}
