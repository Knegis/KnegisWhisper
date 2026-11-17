//! The `diagnostics` command.
//!
//! Reports what this machine can actually do. Every field is either a fact read
//! from the OS or an explicitly-unverified state — nothing here is aspirational,
//! because this report is what `docs/PLATFORM_SUPPORT.md` promises to be honest
//! about.

use kw_security::{CredentialError, CredentialStore, KeyRef, OsCredentialStore};
use serde::Serialize;
use tauri::{AppHandle, Manager as _};
use ts_rs::TS;

use crate::overlay::{self, OverlayStatus};

/// Whether the OS credential store is usable.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(
    tag = "state",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum CredentialStoreStatus {
    Available { backend: &'static str },
    Unavailable { reason: String },
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Diagnostics {
    pub app_version: &'static str,
    pub os: &'static str,
    pub arch: &'static str,
    pub webview_version: Option<String>,
    pub credential_store: CredentialStoreStatus,
    /// The live state of the real overlay window, not a prediction about the
    /// platform. Before the window exists this reports `created: false` and
    /// claims nothing.
    pub overlay: OverlayStatus,
    pub allowed_egress_purposes: Vec<&'static str>,
    /// Where the model cache ended up, and whether the per-user location had to
    /// be abandoned because the native ASR binaries cannot read it.
    pub model_cache: ModelCacheStatus,
    /// Where the application writes its log, and a panic before it ends
    /// (`crate::logging`). What somebody reporting a crash is asked to attach.
    pub log_file: String,
    /// Whether a modifier-only shortcut can work here.
    ///
    /// Reported rather than assumed by the UI. The hotkey editor has to say
    /// something about `Control+Super`, and what it should say depends entirely
    /// on whether this build has a keyboard hook — which is a fact about the
    /// platform that only Rust knows.
    pub modifier_only_shortcuts: ShortcutHookStatus,
    /// Whether what the speakers are playing can be recorded, and how.
    ///
    /// A real device enumeration, not a statement about the platform: it names
    /// the device that would be captured.
    pub system_audio: kw_audio_core::SystemAudioSupport,
}

/// Whether a low-level keyboard hook is available.
///
/// Mirrors `platform::keyhook::HookSupport`, converted here rather than derived
/// there: `kw-platform` has no reason to depend on `ts-rs`, and the boundary type
/// belongs on the side that crosses the boundary.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(
    tag = "state",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum ShortcutHookStatus {
    /// Modifier-only chords work, through this mechanism.
    Available { mechanism: String },
    /// They do not work here, and this is why.
    Unsupported { reason: String },
}

impl From<kw_platform::keyhook::HookSupport> for ShortcutHookStatus {
    fn from(support: kw_platform::keyhook::HookSupport) -> Self {
        match support {
            kw_platform::keyhook::HookSupport::Available { mechanism } => Self::Available {
                mechanism: mechanism.to_owned(),
            },
            kw_platform::keyhook::HookSupport::Unsupported { reason } => {
                Self::Unsupported { reason }
            }
        }
    }
}

/// Where downloaded models live.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ModelCacheStatus {
    pub path: String,
    /// True when the per-user path contained a non-ASCII character, which stops
    /// the native whisper and sherpa binaries opening the model at all -- and
    /// does so without saying why, which is what makes it worth relocating for.
    /// Worth surfacing here too: it means models are shared machine-wide.
    ///
    /// A space no longer counts. It never did; see
    /// `kw_storage::paths`'s module documentation for the measurement.
    pub fell_back: bool,
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

/// Read the live overlay state, or say there is no overlay yet.
fn overlay_status(app: &AppHandle) -> OverlayStatus {
    match app.get_webview_window(overlay::LABEL) {
        Some(window) => overlay::apply_treatments(&window),
        None => overlay::not_created(),
    }
}

#[tauri::command]
pub fn diagnostics(app: AppHandle) -> Diagnostics {
    let cache = kw_storage::paths::model_cache_dir();

    Diagnostics {
        app_version: env!("CARGO_PKG_VERSION"),
        os: std::env::consts::OS,
        arch: std::env::consts::ARCH,
        webview_version: tauri::webview_version().ok(),
        credential_store: probe_credential_store(),
        overlay: overlay_status(&app),
        allowed_egress_purposes: kw_network_policy::ALL_PURPOSES
            .iter()
            .map(|p| p.as_str())
            .collect(),
        modifier_only_shortcuts: kw_platform::keyhook::support().into(),
        // Enumerates devices, which is the point: "this platform can" and "this
        // machine has one" are different claims and only the second is useful.
        system_audio: kw_audio_core::system_audio::support(),
        model_cache: ModelCacheStatus {
            path: cache.path.display().to_string(),
            fell_back: cache.fell_back,
        },
        log_file: crate::logging::log_path().display().to_string(),
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
    fn the_credential_probe_never_writes_anything() {
        // Reading a slot that does not exist is enough to tell a working backend
        // from a missing one, and leaves nothing behind in the user's keychain.
        let slot = KeyRef::new("diagnostics-probe").unwrap();
        let before = OsCredentialStore::new().contains(&slot);
        let _ = probe_credential_store();
        let after = OsCredentialStore::new().contains(&slot);
        assert_eq!(before, after, "the probe changed the credential store");
    }

    #[test]
    fn an_absent_overlay_claims_nothing() {
        let status = overlay::not_created();
        assert!(!status.created);
        assert!(
            !status.capture_exclusion.is_confirmed(),
            "with no window, capture exclusion cannot be confirmed"
        );
    }

    #[test]
    fn the_model_cache_status_agrees_with_the_path_rule() {
        let cache = kw_storage::paths::model_cache_dir();
        let status = ModelCacheStatus {
            path: cache.path.display().to_string(),
            fell_back: cache.fell_back,
        };
        assert!(!status.path.is_empty());
        if !status.fell_back {
            assert!(
                kw_storage::paths::is_native_readable(std::path::Path::new(&status.path)),
                "a path reported as usable must actually be usable"
            );
        }
    }

    #[test]
    fn every_egress_purpose_is_reported() {
        let purposes: Vec<&str> = kw_network_policy::ALL_PURPOSES
            .iter()
            .map(|p| p.as_str())
            .collect();
        assert_eq!(purposes.len(), 7);
        assert!(purposes.iter().all(|p| !p.is_empty()));
    }

    #[test]
    fn the_credential_backend_name_matches_the_platform() {
        let backend = credential_backend();
        if cfg!(target_os = "windows") {
            assert_eq!(backend, "Windows Credential Manager");
        } else if cfg!(target_os = "macos") {
            assert_eq!(backend, "macOS Keychain");
        } else {
            assert_eq!(backend, "Secret Service");
        }
    }
}
