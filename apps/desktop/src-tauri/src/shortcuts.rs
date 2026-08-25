//! Global shortcut registration.
//!
//! One central handler, as Pluely does — it dispatches by action id and forwards
//! anything it does not recognise rather than guessing. That shape is worth
//! copying; what is not worth copying is registering blindly.
//!
//! Registration here reports per-accelerator outcomes. An accelerator that the
//! platform refuses (already taken by another application, most commonly) comes
//! back as [`Registration::Rejected`] with the reason, so the settings screen can
//! say "Alt+D is in use by something else" instead of leaving a hotkey that
//! silently does nothing.

use std::str::FromStr as _;

use kw_platform::hotkey::{Binding, HotkeyAction};
use serde::Serialize;
use tauri::{AppHandle, Emitter as _};
use tauri_plugin_global_shortcut::{GlobalShortcutExt as _, Shortcut, ShortcutState};

/// The event emitted when a hotkey fires.
pub const HOTKEY_EVENT: &str = "hotkey";

/// What happened when we tried to register one accelerator.
#[derive(Debug, Clone, Serialize)]
#[serde(
    tag = "state",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum Registration {
    Registered {
        accelerator: String,
    },
    /// The platform refused it. Usually another application already owns it.
    Rejected {
        accelerator: String,
        reason: String,
    },
    /// Not registrable in principle — modifier-only, or malformed.
    NotRegistrable {
        accelerator: String,
        reason: String,
    },
}

impl Registration {
    #[must_use]
    pub const fn succeeded(&self) -> bool {
        matches!(self, Self::Registered { .. })
    }
}

/// The outcome of registering every configured hotkey.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RegistrationReport {
    pub action: HotkeyAction,
    pub results: Vec<Registration>,
}

impl RegistrationReport {
    /// Whether this action ended up with a working hotkey.
    #[must_use]
    pub fn is_live(&self) -> bool {
        self.results.iter().any(Registration::succeeded)
    }
}

/// The payload delivered to the frontend when a hotkey fires.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HotkeyFired {
    pub action: HotkeyAction,
    pub accelerator: String,
}

/// Register every accelerator in `bindings`.
///
/// Unregisters everything first, so calling this after a settings change is the
/// whole update — there is no incremental diff to get wrong.
pub fn register_all(app: &AppHandle, bindings: &[Binding]) -> Vec<RegistrationReport> {
    let manager = app.global_shortcut();

    if let Err(err) = manager.unregister_all() {
        // Not fatal: on first run there is nothing registered.
        tracing::debug!(error = %err, "nothing to unregister");
    }

    let mut reports = Vec::with_capacity(bindings.len());

    for binding in bindings {
        let mut results = Vec::new();

        // Everything the classifier already rejected, reported as-is rather
        // than attempted and failed.
        for accelerator in &binding.accelerators {
            match accelerator {
                kw_platform::hotkey::Accelerator::Standard { .. } => {}
                other => {
                    let (accel, reason) = match other {
                        kw_platform::hotkey::Accelerator::ModifierOnly { accelerator, .. } => {
                            (accelerator.clone(), other.explain())
                        }
                        kw_platform::hotkey::Accelerator::Invalid { input, .. } => {
                            (input.clone(), other.explain())
                        }
                        kw_platform::hotkey::Accelerator::Standard { accelerator } => {
                            (accelerator.clone(), String::new())
                        }
                    };
                    results.push(Registration::NotRegistrable {
                        accelerator: accel,
                        reason,
                    });
                }
            }
        }

        for accelerator in binding.registrable() {
            results.push(register_one(app, binding.action, accelerator));
        }

        reports.push(RegistrationReport {
            action: binding.action,
            results,
        });
    }

    for report in &reports {
        if !report.is_live() {
            // Every reason, not just the fact. A count told me all five actions
            // had failed and nothing about why, which is useless: "the platform
            // refused it because another application owns it" and "this is two
            // modifiers and no key" need completely different fixes, and only one
            // of them is the user's to make.
            for result in &report.results {
                match result {
                    Registration::Registered { .. } => {}
                    Registration::Rejected {
                        accelerator,
                        reason,
                    } => tracing::warn!(
                        action = %report.action,
                        %accelerator,
                        %reason,
                        "the platform refused this accelerator"
                    ),
                    Registration::NotRegistrable {
                        accelerator,
                        reason,
                    } => tracing::warn!(
                        action = %report.action,
                        %accelerator,
                        %reason,
                        "this accelerator cannot be registered at all"
                    ),
                }
            }
            tracing::warn!(
                action = %report.action,
                "no working hotkey for this action"
            );
        }
    }

    reports
}

fn register_one(app: &AppHandle, action: HotkeyAction, accelerator: &str) -> Registration {
    let shortcut = match Shortcut::from_str(accelerator) {
        Ok(s) => s,
        Err(err) => {
            return Registration::NotRegistrable {
                accelerator: accelerator.to_owned(),
                reason: format!("the shortcut plugin could not parse it: {err}"),
            }
        }
    };

    let app_for_handler = app.clone();
    let accelerator_owned = accelerator.to_owned();

    let result = app
        .global_shortcut()
        .on_shortcut(shortcut, move |_app, _shortcut, event| {
            // Press only. Without this filter every hotkey fires twice, which for
            // dictation means starting and immediately restarting the recording.
            if event.state() != ShortcutState::Pressed {
                return;
            }

            let payload = HotkeyFired {
                action,
                accelerator: accelerator_owned.clone(),
            };

            tracing::debug!(action = %action, "hotkey fired");

            if let Err(err) = app_for_handler.emit(HOTKEY_EVENT, payload) {
                tracing::warn!(error = %err, "could not deliver the hotkey event");
            }

            // Dictation is handled here rather than in the frontend. The
            // dashboard can be hidden, closed to the tray, or still starting up,
            // and a hotkey that only works while a window is listening is not a
            // global hotkey. Everything else is still forwarded for the UI to
            // act on.
            if action == HotkeyAction::Dictation {
                crate::dictation::on_hotkey(&app_for_handler);
            }
        });

    match result {
        Ok(()) => Registration::Registered {
            accelerator: accelerator.to_owned(),
        },
        Err(err) => Registration::Rejected {
            accelerator: accelerator.to_owned(),
            // Almost always "already registered by another application".
            reason: err.to_string(),
        },
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
    fn only_registered_counts_as_success() {
        assert!(Registration::Registered {
            accelerator: "Alt+D".to_owned()
        }
        .succeeded());
        assert!(!Registration::Rejected {
            accelerator: "Alt+D".to_owned(),
            reason: "taken".to_owned()
        }
        .succeeded());
        assert!(!Registration::NotRegistrable {
            accelerator: "Control+Super".to_owned(),
            reason: "modifier only".to_owned()
        }
        .succeeded());
    }

    #[test]
    fn an_action_with_only_rejections_is_not_live() {
        let report = RegistrationReport {
            action: HotkeyAction::Dictation,
            results: vec![
                Registration::NotRegistrable {
                    accelerator: "Control+Super".to_owned(),
                    reason: "modifier only".to_owned(),
                },
                Registration::Rejected {
                    accelerator: "Alt+D".to_owned(),
                    reason: "already registered".to_owned(),
                },
            ],
        };
        assert!(!report.is_live());
    }

    #[test]
    fn one_success_among_failures_makes_the_action_live() {
        let report = RegistrationReport {
            action: HotkeyAction::Dictation,
            results: vec![
                Registration::NotRegistrable {
                    accelerator: "Control+Super".to_owned(),
                    reason: "modifier only".to_owned(),
                },
                Registration::Registered {
                    accelerator: "Alt+D".to_owned(),
                },
            ],
        };
        assert!(report.is_live());
    }

    #[test]
    fn the_shortcut_plugin_agrees_with_our_classifier_on_ordinary_accelerators() {
        // Guards against the classifier accepting a syntax the plugin rejects,
        // which would show as "registered" in settings and do nothing.
        for accelerator in [
            "Control+Shift+D",
            "Alt+D",
            "CommandOrControl+Space",
            "Control+Shift+M",
            "Control+Shift+T",
        ] {
            assert!(
                kw_platform::hotkey::classify(accelerator).is_registrable(),
                "our classifier rejected {accelerator}"
            );
            assert!(
                Shortcut::from_str(accelerator).is_ok(),
                "the plugin rejected {accelerator}, which our classifier accepted"
            );
        }
    }

    #[test]
    fn the_plugin_also_refuses_modifier_only_accelerators() {
        // The premise behind reporting Control+Super as unregistrable. If the
        // plugin ever gains support for it, this test fails and tells us to
        // revisit the classifier rather than leaving a stale limitation.
        let modifier_only = Shortcut::from_str("Control+Super");
        assert!(
            modifier_only.is_err(),
            "the plugin now accepts modifier-only accelerators; revisit hotkey::classify"
        );
    }

    #[test]
    fn events_serialize_with_the_keys_the_frontend_expects() {
        let json = serde_json::to_value(HotkeyFired {
            action: HotkeyAction::Dictation,
            accelerator: "Alt+D".to_owned(),
        })
        .unwrap();
        assert_eq!(json["action"], "dictation");
        assert_eq!(json["accelerator"], "Alt+D");
    }
}
