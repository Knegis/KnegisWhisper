//! Notices for newer versions from this project's own GitHub releases.
//!
//! The Settings button always checks on request. Automatic checking defaults
//! on, with a switch in Settings > Diagnostics. After a 15-second startup
//! delay, an hourly evaluation reads settings; requests are at least 24 hours
//! apart. The attempt time is stored before sending, including failed checks.
//! If that write fails, nothing is sent: database failure must not cause hourly
//! retries. Manual checks do not change that automatic schedule.
//!
//! **Own releases only.** Every request uses NetworkPolicy and
//! RequestPurpose::OwnGithubReleasesUpdater. Published previews count too.
//! No signing key is needed to display a notice and open a release page: this
//! code downloads and installs nothing, so a key would verify nothing here.
//! A verified signature is still required before any future code downloads or
//! installs an update. Failure to see a release is an honest state, never
//! reported as "up to date".
//!
//! # What has never been exercised
//!
//! The app has never requested the list endpoint. Its response shape is
//! believed from GitHub's documentation. On 2026-11-10 an authenticated
//! request to `/releases/latest` returned tag_name `v0.1.1` for this
//! repository. An unauthenticated request got 404 on 2026-11-13, while the
//! repository was private. On 2026-11-16, with it public, an unauthenticated
//! `curl` of the list returned 200 with `tag_name` and `draft` for v0.2.0
//! and v0.1.1.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tauri::{AppHandle, Emitter as _, Manager as _, State};
use ts_rs::TS;

use crate::state::AppState;
use kw_network_policy::{NetworkPolicy, PolicyError, RequestPurpose};

pub const RESULT_EVENT: &str = "update-check-result";
const AUTOMATIC_KEY: &str = "updates.automatic";
const LAST_AUTOMATIC_CHECK_KEY: &str = "updates.lastAutomaticCheck";
const STARTUP_DELAY: Duration = Duration::from_secs(15);
const EVALUATE_EVERY: Duration = Duration::from_secs(60 * 60);
const AUTOMATIC_INTERVAL: chrono::Duration = chrono::Duration::hours(24);

/// Only these fields affect the decision; GitHub's other fields are ignored.
#[derive(Debug, Deserialize)]
struct GithubRelease {
    tag_name: String,
    #[serde(default)]
    draft: bool,
}

/// Failure to read releases is distinct from finding no newer version.
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[serde(
    tag = "state",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum UpdateCheckResult {
    ReleasesNotPublic,
    NoPublishedRelease,
    CheckFailed {
        error: String,
    },
    CurrentVersionUnparseable {
        version: String,
    },
    UpToDate {
        current: String,
        latest: String,
    },
    UpdateAvailable {
        current: String,
        latest: String,
        url: String,
    },
}

impl UpdateCheckResult {
    #[must_use]
    pub const fn is_update_available(&self) -> bool {
        matches!(self, Self::UpdateAvailable { .. })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct UpdatePreferences {
    pub automatic: bool,
    pub last_automatic_check_at: Option<String>,
}

/// Derive both destinations from Cargo's repository field, so forks do not
/// need a second owner/repository configuration in Rust.
fn repository_path(repository: &str) -> Option<&str> {
    let path = repository
        .trim_end_matches('/')
        .strip_prefix("https://github.com/")?;
    (!path.is_empty()).then_some(path)
}

fn releases_api_url(repository: &str) -> Option<String> {
    Some(format!(
        "https://api.github.com/repos/{}/releases?per_page=30",
        repository_path(repository)?
    ))
}

/// The raw tag has already parsed as semver (after one optional `v`), so it
/// contains only version characters and cannot add browser URL syntax.
fn release_page_url(repository: &str, tag: &str) -> Option<String> {
    semver::Version::parse(tag.strip_prefix('v').unwrap_or(tag)).ok()?;
    Some(format!(
        "https://github.com/{}/releases/tag/{tag}",
        repository_path(repository)?
    ))
}

/// GitHub ordering and prerelease flags do not decide version precedence.
/// Drafts and tags that do not parse are excluded before choosing the newest.
fn choose_release(
    current: &str,
    repository: &str,
    releases: &[GithubRelease],
) -> UpdateCheckResult {
    let Ok(current_version) = semver::Version::parse(current) else {
        return UpdateCheckResult::CurrentVersionUnparseable {
            version: current.to_owned(),
        };
    };
    let newest = releases
        .iter()
        .filter(|r| !r.draft)
        .filter_map(|release| {
            let raw = release
                .tag_name
                .strip_prefix('v')
                .unwrap_or(&release.tag_name);
            semver::Version::parse(raw)
                .ok()
                .map(|version| (version, raw, &release.tag_name))
        })
        .max_by(|a, b| a.0.cmp(&b.0));
    let Some((version, latest, tag)) = newest else {
        return UpdateCheckResult::NoPublishedRelease;
    };
    if version > current_version {
        let Some(url) = release_page_url(repository, tag) else {
            return UpdateCheckResult::CheckFailed {
                error: "CARGO_PKG_REPOSITORY is not a https://github.com/OWNER/REPO url".to_owned(),
            };
        };
        UpdateCheckResult::UpdateAvailable {
            current: current.to_owned(),
            latest: latest.to_owned(),
            url,
        }
    } else {
        UpdateCheckResult::UpToDate {
            current: current.to_owned(),
            latest: latest.to_owned(),
        }
    }
}

fn fetch_error(err: PolicyError) -> UpdateCheckResult {
    match err {
        PolicyError::HttpStatus { status: 404, .. } => UpdateCheckResult::ReleasesNotPublic,
        _ => UpdateCheckResult::CheckFailed {
            error: err.to_string(),
        },
    }
}

fn automatic_due(enabled: bool, last_attempt: Option<DateTime<Utc>>, now: DateTime<Utc>) -> bool {
    enabled && last_attempt.is_none_or(|last| last > now || now - last >= AUTOMATIC_INTERVAL)
}

fn automatic_shows(result: &UpdateCheckResult, dismissed: Option<&str>) -> bool {
    matches!(result, UpdateCheckResult::UpdateAvailable { latest, .. } if Some(latest.as_str()) != dismissed)
}

async fn check_now(policy: &NetworkPolicy) -> UpdateCheckResult {
    let Some(api_url) = releases_api_url(env!("CARGO_PKG_REPOSITORY")) else {
        return UpdateCheckResult::CheckFailed {
            error: "CARGO_PKG_REPOSITORY is not a https://github.com/OWNER/REPO url".to_owned(),
        };
    };
    // UNVERIFIED: the app has not exercised this list endpoint; its shape is
    // believed from GitHub's documentation. See the dated evidence above.
    let body = match policy
        .get_text(RequestPurpose::OwnGithubReleasesUpdater, &api_url)
        .await
    {
        Ok(body) => body,
        Err(err) => return fetch_error(err),
    };
    let releases: Vec<GithubRelease> = match serde_json::from_str(&body) {
        Ok(releases) => releases,
        Err(err) => {
            return UpdateCheckResult::CheckFailed {
                error: format!("could not read the release list: {err}"),
            }
        }
    };
    choose_release(
        env!("CARGO_PKG_VERSION"),
        env!("CARGO_PKG_REPOSITORY"),
        &releases,
    )
}

#[derive(Default)]
struct CheckState {
    result: Option<UpdateCheckResult>,
    dismissed: Option<String>,
}

/// Result and dismissal share a lock so automatic presentation uses the same
/// session state as dismissal. Neither is persisted across restarts.
#[derive(Default)]
pub struct LastCheck(std::sync::Mutex<CheckState>);

impl LastCheck {
    fn record(&self, result: UpdateCheckResult, automatic: bool) -> bool {
        let mut held = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let show = if automatic {
            automatic_shows(&result, held.dismissed.as_deref())
        } else {
            result.is_update_available()
        };
        held.result = Some(result);
        show
    }

    fn get(&self) -> Option<UpdateCheckResult> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .result
            .clone()
    }

    fn dismiss(&self) {
        let mut held = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(UpdateCheckResult::UpdateAvailable { latest, .. }) = &held.result {
            held.dismissed = Some(latest.clone());
        }
    }
}

/// Both entry points publish the same event and last result. Only automatic
/// presentation respects the version dismissed earlier in this session.
fn publish(app: &AppHandle, last: &LastCheck, result: &UpdateCheckResult, automatic: bool) {
    let show = last.record(result.clone(), automatic);
    if let Err(err) = app.emit(RESULT_EVENT, result) {
        tracing::warn!(error = %err, "could not tell the update notification window what the check found");
    }
    crate::update_window::set_visible(app, show);
}

#[tauri::command]
pub async fn update_check(
    app: AppHandle,
    state: State<'_, AppState>,
    last: State<'_, LastCheck>,
) -> Result<UpdateCheckResult, String> {
    let result = check_now(state.policy()).await;
    publish(&app, &last, &result, false);
    Ok(result)
}

#[tauri::command]
pub fn update_check_last(last: State<'_, LastCheck>) -> Option<UpdateCheckResult> {
    last.get()
}

#[tauri::command]
pub fn update_dismiss(app: AppHandle, last: State<'_, LastCheck>) {
    last.dismiss();
    crate::update_window::set_visible(&app, false);
}

/// The frontend supplies no URL: only the Rust-built page in the last result
/// may be opened. A failed browser spawn leaves the notice actionable.
#[tauri::command]
pub fn update_open_release(app: AppHandle, last: State<'_, LastCheck>) -> Result<(), String> {
    let Some(UpdateCheckResult::UpdateAvailable { url, .. }) = last.get() else {
        return Err("No newer release is known, so there is no page to open.".to_owned());
    };
    crate::calendar::open_in_browser(&url).map_err(|_| "Could not open the browser.".to_owned())?;
    last.dismiss();
    crate::update_window::set_visible(&app, false);
    Ok(())
}

async fn read_preferences(state: &AppState) -> Result<UpdatePreferences, String> {
    let automatic = kw_storage::settings::get_or(state.db().pool(), AUTOMATIC_KEY, true)
        .await
        .map_err(|err| format!("could not read the update setting: {err}"))?;
    let last_automatic_check_at = kw_storage::settings::get_or::<Option<String>>(
        state.db().pool(),
        LAST_AUTOMATIC_CHECK_KEY,
        None,
    )
    .await
    .map_err(|err| format!("could not read the last automatic check: {err}"))?;
    Ok(UpdatePreferences {
        automatic,
        last_automatic_check_at,
    })
}

#[tauri::command]
pub async fn update_preferences(state: State<'_, AppState>) -> Result<UpdatePreferences, String> {
    read_preferences(&state).await
}

#[tauri::command]
pub async fn update_set_automatic(state: State<'_, AppState>, enabled: bool) -> Result<(), String> {
    kw_storage::settings::set(state.db().pool(), AUTOMATIC_KEY, &enabled)
        .await
        .map_err(|err| format!("could not save the update setting: {err}"))
}

/// Evaluating is local. Persist before sending even when the request will
/// fail, so a private repository or broken connection cannot cause retries.
pub fn start_automatic(app: AppHandle, state: AppState) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(STARTUP_DELAY).await;
        loop {
            match read_preferences(&state).await {
                Ok(preferences) => {
                    let now = Utc::now();
                    let last_attempt = preferences
                        .last_automatic_check_at
                        .as_deref()
                        .and_then(|raw| DateTime::parse_from_rfc3339(raw).ok())
                        .map(|time| time.with_timezone(&Utc));
                    if automatic_due(preferences.automatic, last_attempt, now) {
                        match kw_storage::settings::set(
                            state.db().pool(),
                            LAST_AUTOMATIC_CHECK_KEY,
                            &now.to_rfc3339(),
                        )
                        .await
                        {
                            Ok(()) => {
                                let result = check_now(state.policy()).await;
                                publish(&app, &app.state::<LastCheck>(), &result, true);
                            }
                            Err(err) => {
                                tracing::warn!(error = %err, "could not record automatic update attempt; no request sent")
                            }
                        }
                    }
                }
                Err(err) => {
                    tracing::warn!(error = %err, "could not evaluate automatic update check; no request sent")
                }
            }
            tokio::time::sleep(EVALUATE_EVERY).await;
        }
    });
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

    fn releases(tags: &[&str]) -> Vec<GithubRelease> {
        tags.iter()
            .map(|tag| GithubRelease {
                tag_name: (*tag).to_owned(),
                draft: false,
            })
            .collect()
    }
    fn choose(current: &str, tags: &[&str]) -> UpdateCheckResult {
        choose_release(current, env!("CARGO_PKG_REPOSITORY"), &releases(tags))
    }
    fn available() -> UpdateCheckResult {
        choose("0.1.0", &["v0.2.0"])
    }

    #[test]
    fn urls_use_the_real_manifest_and_handle_edges() {
        assert_eq!(
            releases_api_url(env!("CARGO_PKG_REPOSITORY")).as_deref(),
            Some("https://api.github.com/repos/Knegis/KnegisWhisper/releases?per_page=30")
        );
        assert_eq!(
            releases_api_url("https://github.com/owner/repo/").as_deref(),
            Some("https://api.github.com/repos/owner/repo/releases?per_page=30")
        );
        assert_eq!(releases_api_url("https://gitlab.com/owner/repo"), None);
        assert_eq!(releases_api_url("https://github.com/"), None);
        assert_eq!(
            release_page_url(env!("CARGO_PKG_REPOSITORY"), "v0.2.0").as_deref(),
            Some("https://github.com/Knegis/KnegisWhisper/releases/tag/v0.2.0")
        );
        assert_eq!(
            release_page_url("https://github.com/owner/repo/", "0.2.0").as_deref(),
            Some("https://github.com/owner/repo/releases/tag/0.2.0")
        );
        assert_eq!(
            release_page_url("https://gitlab.com/owner/repo", "v0.2.0"),
            None
        );
        assert_eq!(
            release_page_url(env!("CARGO_PKG_REPOSITORY"), "vv0.2.0"),
            None
        );
    }

    #[test]
    fn newest_wins_in_any_order_and_invalid_tags_are_skipped() {
        for tags in [
            ["v0.1.0", "v0.2.0", "v0.1.1"],
            ["v0.2.0", "v0.1.1", "v0.1.0"],
            ["v0.1.1", "v0.1.0", "v0.2.0"],
        ] {
            assert_eq!(choose("0.1.0", &tags), available());
        }
        assert_eq!(choose("0.1.0", &["bad", "vv9.0.0", "v0.2.0"]), available());
        assert!(
            matches!(choose("0.1.0", &["0.2.0"]), UpdateCheckResult::UpdateAvailable { url, .. } if url.ends_with("/0.2.0"))
        );
        assert_eq!(choose("0.1.0", &[]), UpdateCheckResult::NoPublishedRelease);
        assert_eq!(
            choose("0.1.0", &["bad", "vv1.0.0"]),
            UpdateCheckResult::NoPublishedRelease
        );
        let mut list = releases(&["v9.0.0", "v0.2.0"]);
        list[0].draft = true;
        assert_eq!(
            choose_release("0.1.0", env!("CARGO_PKG_REPOSITORY"), &list),
            available()
        );
    }

    #[test]
    fn documented_list_shape_counts_github_prereleases() {
        // Shaped from GitHub's list-releases documentation, not a captured response.
        let list: Vec<GithubRelease> = serde_json::from_str(r#"[{"tag_name":"v0.2.0","draft":false,"html_url":"https://example.invalid/ignored","prerelease":true,"name":"Preview","assets":[],"published_at":"2026-11-13T07:30:00Z"}]"#).unwrap();
        assert_eq!(
            choose_release("0.1.0", env!("CARGO_PKG_REPOSITORY"), &list),
            available()
        );
        let default: GithubRelease = serde_json::from_str(r#"{"tag_name":"v0.2.0"}"#).unwrap();
        assert!(!default.draft);
    }

    #[test]
    fn comparison_never_offers_a_downgrade_and_preserves_the_raw_tag() {
        for current in ["0.2.0", "0.3.0"] {
            assert_eq!(
                choose(current, &["v0.2.0"]),
                UpdateCheckResult::UpToDate {
                    current: current.to_owned(),
                    latest: "0.2.0".to_owned()
                }
            );
        }
        assert_eq!(
            available(),
            UpdateCheckResult::UpdateAvailable {
                current: "0.1.0".to_owned(),
                latest: "0.2.0".to_owned(),
                url: "https://github.com/Knegis/KnegisWhisper/releases/tag/v0.2.0".to_owned()
            }
        );
        assert_eq!(
            choose("bad", &["v0.2.0"]),
            UpdateCheckResult::CurrentVersionUnparseable {
                version: "bad".to_owned()
            }
        );
    }

    #[test]
    fn fetch_errors_distinguish_private_releases() {
        assert_eq!(
            fetch_error(PolicyError::HttpStatus {
                url: "url".into(),
                status: 404
            }),
            UpdateCheckResult::ReleasesNotPublic
        );
        let err = PolicyError::HttpStatus {
            url: "url".into(),
            status: 403,
        };
        let expected = err.to_string();
        assert_eq!(
            fetch_error(err),
            UpdateCheckResult::CheckFailed { error: expected }
        );
    }

    #[test]
    fn due_logic_records_daily_attempts_and_handles_clock_reversal() {
        let now = Utc::now();
        for (last, due) in [
            (None, true),
            (Some(now - chrono::Duration::hours(23)), false),
            (Some(now - chrono::Duration::hours(24)), true),
            (Some(now + chrono::Duration::hours(1)), true),
        ] {
            assert_eq!(automatic_due(true, last, now), due);
            assert!(!automatic_due(false, last, now));
        }
    }

    #[test]
    fn every_state_serializes_and_only_available_shows_automatically() {
        for (result, tag) in [
            (UpdateCheckResult::ReleasesNotPublic, "releases-not-public"),
            (
                UpdateCheckResult::NoPublishedRelease,
                "no-published-release",
            ),
            (
                UpdateCheckResult::CheckFailed { error: "x".into() },
                "check-failed",
            ),
            (
                UpdateCheckResult::CurrentVersionUnparseable {
                    version: "bad".into(),
                },
                "current-version-unparseable",
            ),
            (choose("0.2.0", &["v0.2.0"]), "up-to-date"),
            (available(), "update-available"),
        ] {
            assert_eq!(serde_json::to_value(&result).unwrap()["state"], tag);
            assert_eq!(automatic_shows(&result, None), tag == "update-available");
            assert!(!automatic_shows(&result, Some("0.2.0")));
        }
        assert!(automatic_shows(
            &choose("0.1.0", &["v0.3.0"]),
            Some("0.2.0")
        ));
        assert_eq!(
            serde_json::to_value(UpdatePreferences {
                automatic: true,
                last_automatic_check_at: Some("time".into())
            })
            .unwrap(),
            serde_json::json!({"automatic":true,"lastAutomaticCheckAt":"time"})
        );
    }

    #[test]
    fn last_check_holds_result_and_session_dismissal_together() {
        let last = LastCheck::default();
        assert_eq!(last.get(), None);
        assert_eq!(last.0.lock().unwrap().dismissed, None);
        last.dismiss();
        assert!(last.record(available(), true));
        last.dismiss();
        assert_eq!(last.get(), Some(available()));
        assert_eq!(last.0.lock().unwrap().dismissed.as_deref(), Some("0.2.0"));
        assert!(!last.record(available(), true));
        assert!(last.record(available(), false));
        assert!(last.record(choose("0.1.0", &["v0.3.0"]), true));
    }
}
