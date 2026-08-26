//! Checking this project's own GitHub releases for a version newer than the
//! one running.
//!
//! Three things are non-negotiable here, straight from the brief:
//!
//! * **Opt-in, and never at startup.** [`check_now`] is the only place a
//!   network request happens, and its only caller is [`update_check`], a
//!   `#[tauri::command]` a person invokes from Settings. `lib.rs::run`'s
//!   `setup` never calls it, there is no timer anywhere in this module, and
//!   there will not be a boolean setting that starts one either -- the
//!   constraint is structural (this function simply has no other caller), not
//!   a default value that could be flipped on without anyone noticing.
//! * **This project's own releases only.** Every request goes through
//!   [`kw_network_policy::NetworkPolicy`] tagged
//!   [`RequestPurpose::OwnGithubReleasesUpdater`] -- there is no other egress
//!   point in the workspace, and this is the only purpose that names the
//!   GitHub releases feed as its destination.
//! * **No silent unsigned trust, and no silent nothing.** If no updater public
//!   key has been configured, [`UpdateCheckResult::NoPublicKeyConfigured`] is
//!   returned honestly rather than either checking anyway (which would make an
//!   unverifiable update look identical to a verified one) or reporting
//!   "up to date" for a reason that has nothing to do with whether one exists.
//!
//! # What this module does not do
//!
//! It does not download an update, verify a signature, or install anything.
//! Notifying that a newer release exists is this task; installing one without
//! an explicit action is a different task, not attempted here. The update
//! notification window ([`crate::update_window`]) it drives is exactly that: a
//! notice, with a dismiss button, never a downloader.
//!
//! # What has never been exercised
//!
//! This project has never published a release --
//! `.github/workflows/release.yml` is `workflow_dispatch`-only and has never
//! been run -- so [`check_now`]'s request against a real repository has only
//! ever come back 404. The `GithubRelease` shape it parses is GitHub's
//! documented response for `GET /repos/{owner}/{repo}/releases/latest`,
//! believed from that documentation rather than confirmed against a real
//! payload. See `docs/PLATFORM_SUPPORT.md`.

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter as _, State};
use ts_rs::TS;

use kw_network_policy::{NetworkPolicy, RequestPurpose};

/// Carries the result to the update notification window, which is created
/// hidden at startup and so is already listening by the time a person clicks
/// "Check for updates" -- unlike [`update_check_last`], which only answers a
/// window that asks after the fact.
pub const RESULT_EVENT: &str = "update-check-result";

/// The two fields this module reads out of GitHub's release response.
/// Everything else GitHub sends is ignored.
#[derive(Debug, Deserialize)]
struct GithubRelease {
    tag_name: String,
    html_url: String,
}

/// What checking for an update found.
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[serde(
    tag = "state",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum UpdateCheckResult {
    /// No updater public key is configured
    /// (`plugins.updater.pubkey` in `tauri.conf.json`), so a downloaded
    /// update's authenticity could never be checked. See `docs/SECURITY.md`
    /// for how a maintainer generates one.
    NoPublicKeyConfigured,
    /// The request did not succeed, or its response could not be read as the
    /// shape a release listing is expected to have.
    CheckFailed { error: String },
    /// This build's own version does not parse as semver, so nothing could be
    /// compared against it. Should not happen in a normal build; reported
    /// rather than assumed away if it somehow does.
    CurrentVersionUnparseable { version: String },
    /// The published tag does not parse as semver once a leading `v` is
    /// stripped.
    LatestVersionUnparseable { tag: String },
    /// Already on the newest published release.
    UpToDate { current: String, latest: String },
    /// A newer release exists. Nothing is downloaded and nothing is
    /// installed -- this only reports that one exists and where to read about
    /// it.
    UpdateAvailable {
        current: String,
        latest: String,
        url: String,
    },
}

impl UpdateCheckResult {
    /// Whether this result is the one state the notification window is for.
    #[must_use]
    pub const fn is_update_available(&self) -> bool {
        matches!(self, Self::UpdateAvailable { .. })
    }
}

/// Whether a raw key value counts as configured.
///
/// A pure decision over a plain `Option<&str>` rather than over `AppHandle` or
/// `tauri::Config`, so a test can exercise it without touching either --
/// see `docs/` on why a Tauri-dependent function must never be named as a
/// value in a test.
fn is_configured(raw: Option<&str>) -> bool {
    raw.is_some_and(|key| !key.trim().is_empty())
}

/// Read `plugins.updater.pubkey` from `tauri.conf.json`, if one has been set.
///
/// This is the conventional location the Tauri updater plugin itself reads,
/// so a keypair generated by `npx tauri signer generate` (see
/// `docs/SECURITY.md`) drops in without translation, even though this project
/// does not depend on that plugin's crate -- see the module doc on
/// `crates/network-policy` for why every request stays behind
/// [`kw_network_policy::NetworkPolicy`] instead of the plugin's own client.
fn configured_public_key(app: &AppHandle) -> Option<String> {
    let raw = app
        .config()
        .plugins
        .0
        .get("updater")?
        .get("pubkey")?
        .as_str()?;
    is_configured(Some(raw)).then(|| raw.to_owned())
}

/// Turn this project's own `repository` manifest field into the GitHub
/// releases API URL for its latest published, non-draft, non-prerelease
/// release.
///
/// A pure function of the string Cargo already carries (`repository.workspace
/// = true`), rather than a separately hardcoded owner and repo, so a fork does
/// not have to edit Rust to point this at its own releases -- only its own
/// `Cargo.toml`.
fn releases_api_url(repository: &str) -> Option<String> {
    let path = repository
        .trim_end_matches('/')
        .strip_prefix("https://github.com/")?;
    if path.is_empty() {
        return None;
    }
    Some(format!(
        "https://api.github.com/repos/{path}/releases/latest"
    ))
}

/// Compare this build's version against a published tag, and say what that
/// means.
///
/// Pure: no network, no `AppHandle`. [`check_now`] is the only caller, and the
/// only thing it adds on top is fetching the two strings this takes.
fn compare_versions(current: &str, tag: &str, html_url: &str) -> UpdateCheckResult {
    let Ok(current_version) = semver::Version::parse(current) else {
        return UpdateCheckResult::CurrentVersionUnparseable {
            version: current.to_owned(),
        };
    };

    let latest_raw = tag.trim_start_matches('v');
    let Ok(latest_version) = semver::Version::parse(latest_raw) else {
        return UpdateCheckResult::LatestVersionUnparseable {
            tag: tag.to_owned(),
        };
    };

    if latest_version > current_version {
        UpdateCheckResult::UpdateAvailable {
            current: current.to_owned(),
            latest: latest_raw.to_owned(),
            url: html_url.to_owned(),
        }
    } else {
        UpdateCheckResult::UpToDate {
            current: current.to_owned(),
            latest: latest_raw.to_owned(),
        }
    }
}

/// Ask this project's own GitHub releases whether a newer version has been
/// published. See the module doc for the three constraints this exists under.
async fn check_now(policy: &NetworkPolicy, pubkey_configured: bool) -> UpdateCheckResult {
    if !pubkey_configured {
        return UpdateCheckResult::NoPublicKeyConfigured;
    }

    let Some(api_url) = releases_api_url(env!("CARGO_PKG_REPOSITORY")) else {
        return UpdateCheckResult::CheckFailed {
            error: "CARGO_PKG_REPOSITORY is not a https://github.com/OWNER/REPO url".to_owned(),
        };
    };

    // UNVERIFIED: this request has never returned anything but 404, because
    // no release has ever been published. See the module doc.
    let body = match policy
        .get_text(RequestPurpose::OwnGithubReleasesUpdater, &api_url)
        .await
    {
        Ok(body) => body,
        Err(err) => {
            return UpdateCheckResult::CheckFailed {
                error: err.to_string(),
            }
        }
    };

    let release: GithubRelease = match serde_json::from_str(&body) {
        Ok(release) => release,
        Err(err) => {
            return UpdateCheckResult::CheckFailed {
                error: format!("could not parse the release list: {err}"),
            }
        }
    };

    compare_versions(
        env!("CARGO_PKG_VERSION"),
        &release.tag_name,
        &release.html_url,
    )
}

/// The most recent check's result, if a person has ever run one.
///
/// Read by the notification window when it (re)appears, and by the
/// diagnostics screen so it can show what the last check found without
/// triggering a new one. `None` until [`update_check`] has run at least once
/// -- there is deliberately no eager fill at startup.
#[derive(Default)]
pub struct LastCheck(std::sync::Mutex<Option<UpdateCheckResult>>);

impl LastCheck {
    fn set(&self, result: UpdateCheckResult) {
        let mut held = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *held = Some(result);
    }

    fn get(&self) -> Option<UpdateCheckResult> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
}

/// Check for an update. The one entry point a person's own click reaches.
#[tauri::command]
pub async fn update_check(
    app: AppHandle,
    state: State<'_, crate::state::AppState>,
    last: State<'_, LastCheck>,
) -> Result<UpdateCheckResult, String> {
    let pubkey_configured = configured_public_key(&app).is_some();
    let result = check_now(state.policy(), pubkey_configured).await;
    last.set(result.clone());

    // Told to the window before it is shown, so a window that was already
    // sitting hidden since startup has something to render the moment it
    // appears rather than a blank panel until the next event.
    if let Err(err) = app.emit(RESULT_EVENT, &result) {
        tracing::warn!(error = %err, "could not tell the update notification window what the check found");
    }

    // The floating window is reserved for the one state that is actually
    // actionable. Every other state is for whatever screen asked to read it,
    // the same way the meeting notification only appears for an offer or an
    // active recording rather than for every possible meeting state.
    crate::update_window::set_visible(&app, result.is_update_available());

    Ok(result)
}

/// What the last check found, without running a new one.
#[tauri::command]
pub fn update_check_last(last: State<'_, LastCheck>) -> Option<UpdateCheckResult> {
    last.get()
}

/// Hide the notification window without clearing the last result -- the same
/// shape as "Not now" on the meeting offer.
#[tauri::command]
pub fn update_dismiss(app: AppHandle) {
    crate::update_window::set_visible(&app, false);
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
    fn no_key_and_a_blank_key_are_both_unconfigured() {
        assert!(!is_configured(None));
        assert!(!is_configured(Some("")));
        assert!(!is_configured(Some("   ")));
        assert!(is_configured(Some("the-actual-key")));
    }

    #[test]
    fn the_releases_url_is_built_from_this_projects_own_manifest() {
        // The real value `env!("CARGO_PKG_REPOSITORY")` carries, pinned here so
        // a change to that field is a visible test failure rather than a
        // silently different URL.
        assert_eq!(
            releases_api_url("https://github.com/axelalbertkarlsson/KnegisWhisper"),
            Some(
                "https://api.github.com/repos/axelalbertkarlsson/KnegisWhisper/releases/latest"
                    .to_owned()
            )
        );
    }

    #[test]
    fn a_trailing_slash_does_not_break_the_url() {
        assert_eq!(
            releases_api_url("https://github.com/owner/repo/"),
            Some("https://api.github.com/repos/owner/repo/releases/latest".to_owned())
        );
    }

    #[test]
    fn a_non_github_repository_field_is_refused_rather_than_guessed() {
        // A fork that changes `repository` to something other than github.com
        // gets an honest refusal, not a URL built against the wrong host.
        assert_eq!(
            releases_api_url("https://gitlab.com/owner/repo"),
            None,
            "a non-github host must not silently produce a github.com API url"
        );
        assert_eq!(releases_api_url("https://github.com/"), None);
    }

    #[test]
    fn a_newer_tag_is_an_update() {
        let result = compare_versions("0.1.0", "v0.2.0", "https://example.invalid/v0.2.0");
        assert_eq!(
            result,
            UpdateCheckResult::UpdateAvailable {
                current: "0.1.0".to_owned(),
                latest: "0.2.0".to_owned(),
                url: "https://example.invalid/v0.2.0".to_owned(),
            }
        );
        assert!(result.is_update_available());
    }

    #[test]
    fn the_same_version_is_up_to_date() {
        let result = compare_versions("0.1.0", "v0.1.0", "https://example.invalid/v0.1.0");
        assert_eq!(
            result,
            UpdateCheckResult::UpToDate {
                current: "0.1.0".to_owned(),
                latest: "0.1.0".to_owned(),
            }
        );
        assert!(!result.is_update_available());
    }

    #[test]
    fn a_tag_older_than_this_build_is_up_to_date_not_a_downgrade_offer() {
        // A build run ahead of the published tag (a developer's own build, or
        // a rolled-back release feed) must never be told to "update" backward.
        let result = compare_versions("0.3.0", "v0.2.0", "https://example.invalid/v0.2.0");
        assert_eq!(
            result,
            UpdateCheckResult::UpToDate {
                current: "0.3.0".to_owned(),
                latest: "0.2.0".to_owned(),
            }
        );
    }

    #[test]
    fn an_unparseable_tag_is_reported_rather_than_treated_as_up_to_date() {
        let result = compare_versions("0.1.0", "not-a-version", "https://example.invalid");
        assert_eq!(
            result,
            UpdateCheckResult::LatestVersionUnparseable {
                tag: "not-a-version".to_owned(),
            }
        );
    }

    #[test]
    fn an_unparseable_current_version_is_reported_too() {
        let result = compare_versions("not-a-version", "v1.0.0", "https://example.invalid");
        assert_eq!(
            result,
            UpdateCheckResult::CurrentVersionUnparseable {
                version: "not-a-version".to_owned(),
            }
        );
    }

    #[tokio::test]
    async fn no_configured_key_refuses_before_any_request_is_made() {
        // The point being pinned: this returns without ever touching
        // `NetworkPolicy::get_text`, which is what makes "no silent unsigned
        // trust" true regardless of what the network would have said.
        let policy = NetworkPolicy::new().unwrap();
        let result = check_now(&policy, false).await;
        assert_eq!(result, UpdateCheckResult::NoPublicKeyConfigured);
    }

    #[test]
    fn every_state_serializes_with_a_discriminant_the_frontend_can_switch_on() {
        for (result, expected) in [
            (
                UpdateCheckResult::NoPublicKeyConfigured,
                "no-public-key-configured",
            ),
            (
                UpdateCheckResult::CheckFailed {
                    error: "x".to_owned(),
                },
                "check-failed",
            ),
            (
                UpdateCheckResult::UpToDate {
                    current: "0.1.0".to_owned(),
                    latest: "0.1.0".to_owned(),
                },
                "up-to-date",
            ),
            (
                UpdateCheckResult::UpdateAvailable {
                    current: "0.1.0".to_owned(),
                    latest: "0.2.0".to_owned(),
                    url: "https://example.invalid".to_owned(),
                },
                "update-available",
            ),
        ] {
            let json = serde_json::to_value(&result).unwrap();
            assert_eq!(json["state"], expected);
        }
    }

    #[test]
    fn only_update_available_is_reported_as_available() {
        let all = [
            UpdateCheckResult::NoPublicKeyConfigured,
            UpdateCheckResult::CheckFailed {
                error: String::new(),
            },
            UpdateCheckResult::CurrentVersionUnparseable {
                version: String::new(),
            },
            UpdateCheckResult::LatestVersionUnparseable { tag: String::new() },
            UpdateCheckResult::UpToDate {
                current: String::new(),
                latest: String::new(),
            },
        ];
        for state in all {
            assert!(!state.is_update_available(), "{state:?}");
        }
        assert!(UpdateCheckResult::UpdateAvailable {
            current: String::new(),
            latest: String::new(),
            url: String::new(),
        }
        .is_update_available());
    }

    #[test]
    fn last_check_starts_empty_and_holds_what_it_is_given() {
        let last = LastCheck::default();
        assert_eq!(last.get(), None);
        last.set(UpdateCheckResult::UpToDate {
            current: "0.1.0".to_owned(),
            latest: "0.1.0".to_owned(),
        });
        assert_eq!(
            last.get(),
            Some(UpdateCheckResult::UpToDate {
                current: "0.1.0".to_owned(),
                latest: "0.1.0".to_owned(),
            })
        );
    }
}
