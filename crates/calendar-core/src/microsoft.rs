//! Microsoft Graph v1.0 calendar lists and bounded calendarView snapshots.
//!
//! calendarView supports non-default calendars and expands recurring meetings.
//! UTC preferences apply to every page; opaque pagination links are validated
//! against the original calendar resource before attaching credentials. Request
//! shapes are tested offline and have not been validated against a live tenant.

use kw_network_policy::{NetworkPolicy, RequestPurpose};
use kw_security::Secret;
use serde::Deserialize;

use crate::sync_support::{
    collect_window, percent_encode_path_segment, to_utc, StoredAttendee, SyncError, WindowSnapshot,
    MAX_PAGES,
};

const CALENDARS_URL: &str = "https://graph.microsoft.com/v1.0/me/calendars";

fn calendar_view_url(calendar_id: &str) -> String {
    format!(
        "https://graph.microsoft.com/v1.0/me/calendars/{}/calendarView",
        percent_encode_path_segment(calendar_id)
    )
}

/// One entry from `/me/calendars`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CalendarListEntry {
    pub id: String,
    pub summary: String,
    pub background_color: Option<String>,
    pub is_primary: bool,
    pub owner_email: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawCalendarsResponse {
    #[serde(default)]
    value: Vec<RawCalendar>,
    #[serde(rename = "@odata.nextLink", default)]
    next_link: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawCalendar {
    id: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    color: Option<String>,
    #[serde(rename = "isDefaultCalendar", default)]
    is_default_calendar: bool,
    #[serde(default)]
    owner: Option<RawOwner>,
}

#[derive(Debug, Deserialize)]
struct RawOwner {
    #[serde(default)]
    address: Option<String>,
}

fn parse_calendars_page(body: &str) -> Result<(Vec<CalendarListEntry>, Option<String>), SyncError> {
    let raw: RawCalendarsResponse =
        serde_json::from_str(body).map_err(|_json_error| SyncError::MalformedResponse)?;
    let items = raw
        .value
        .into_iter()
        .map(|c| CalendarListEntry {
            id: c.id,
            summary: c.name.unwrap_or_default(),
            background_color: c.color.filter(|c| c != "auto"),
            is_primary: c.is_default_calendar,
            owner_email: c.owner.and_then(|o| o.address),
        })
        .collect();
    Ok((items, raw.next_link))
}

/// The default calendar owner names the account. Shared calendars may have
/// different owners, so only the default entry is considered.
#[must_use]
pub fn account_owner_email(calendars: &[CalendarListEntry]) -> Option<&str> {
    calendars
        .iter()
        .find(|c| c.is_primary)
        .and_then(|c| c.owner_email.as_deref())
}

async fn get(
    policy: &NetworkPolicy,
    access_token: &Secret,
    url: &str,
    expected_path: &str,
) -> Result<(u16, String), SyncError> {
    validate_graph_url(url, expected_path)?;
    let response = policy
        .get(RequestPurpose::UserConfiguredCalendarProvider, url)?
        .bearer_auth(access_token.expose())
        .header("Prefer", "outlook.timezone=\"UTC\", odata.maxpagesize=100")
        .send()
        .await
        .map_err(SyncError::Http)?;
    let status = response.status().as_u16();
    let body = response.text().await.map_err(SyncError::Http)?;
    Ok((status, body))
}

/// Graph pagination links are opaque, but they must not be allowed
/// to change the origin or resource before the access token is attached.
fn validate_graph_url(url: &str, expected_path: &str) -> Result<(), SyncError> {
    let parsed = url::Url::parse(url).map_err(|_| SyncError::UnsafePaginationUrl)?;
    let is_graph_url = parsed.scheme() == "https"
        && parsed.host_str() == Some("graph.microsoft.com")
        && parsed.port_or_known_default() == Some(443)
        && parsed.username().is_empty()
        && parsed.password().is_none()
        && parsed.fragment().is_none()
        && parsed.path() == expected_path;

    if is_graph_url {
        Ok(())
    } else {
        Err(SyncError::UnsafePaginationUrl)
    }
}

/// Fetch every calendar in the account, following `@odata.nextLink`.
///
/// UNVERIFIED network path -- see the module doc. [`parse_calendars_page`] is
/// what the fixture tests exercise.
pub async fn fetch_calendars(
    policy: &NetworkPolicy,
    access_token: &Secret,
) -> Result<Vec<CalendarListEntry>, SyncError> {
    let mut items = Vec::new();
    let mut url = CALENDARS_URL.to_owned();
    let expected_path = url::Url::parse(CALENDARS_URL)
        .map_err(|_| SyncError::UnsafePaginationUrl)?
        .path()
        .to_owned();

    for _ in 0..MAX_PAGES {
        let (status, body) = get(policy, access_token, &url, &expected_path).await?;
        if status != 200 {
            return Err(SyncError::HttpStatus { status });
        }
        let (page, next) = parse_calendars_page(&body)?;
        items.extend(page);
        match next {
            Some(next_url) => url = next_url,
            None => return Ok(items),
        }
    }
    Err(SyncError::TooManyPages)
}

/* ------------------------------------------------------------------ events */

#[derive(Debug, Deserialize)]
struct RawWindowPage {
    value: Vec<RawEvent>,
    #[serde(rename = "@odata.nextLink")]
    next_link: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawEvent {
    id: String,
    subject: Option<String>,
    start: Option<RawGraphDateTime>,
    end: Option<RawGraphDateTime>,
    #[serde(rename = "isAllDay", default)]
    is_all_day: bool,
    #[serde(rename = "isCancelled", default)]
    is_cancelled: bool,
    #[serde(rename = "responseStatus")]
    response_status: Option<RawResponseStatus>,
    #[serde(rename = "onlineMeeting")]
    online_meeting: Option<RawOnlineMeeting>,
    organizer: Option<RawGraphOrganizer>,
    #[serde(default)]
    attendees: Vec<RawGraphAttendee>,
}
#[derive(Debug, Deserialize)]
struct RawResponseStatus {
    response: Option<String>,
}
#[derive(Debug, Deserialize)]
struct RawGraphDateTime {
    #[serde(rename = "dateTime")]
    date_time: Option<String>,
    #[serde(rename = "timeZone")]
    time_zone: Option<String>,
}
#[derive(Debug, Deserialize)]
struct RawOnlineMeeting {
    #[serde(rename = "joinUrl")]
    join_url: Option<String>,
}
#[derive(Debug, Deserialize)]
struct RawGraphOrganizer {
    #[serde(rename = "emailAddress")]
    email_address: Option<RawGraphEmailAddress>,
}
#[derive(Debug, Deserialize)]
struct RawGraphEmailAddress {
    address: Option<String>,
    name: Option<String>,
}
#[derive(Debug, Deserialize)]
struct RawGraphAttendee {
    #[serde(rename = "emailAddress")]
    email_address: Option<RawGraphEmailAddress>,
    status: Option<RawResponseStatus>,
}

fn event_time(time: &RawGraphDateTime, all_day: bool) -> Option<String> {
    let utc = to_utc(time.date_time.as_deref()?, time.time_zone.as_deref())?;
    if all_day {
        Some(utc.get(..10)?.to_owned())
    } else {
        Some(utc)
    }
}

fn parse_window_page(
    body: &str,
    calendar_id: &str,
) -> Result<(WindowSnapshot, Option<String>), SyncError> {
    use crate::events_repo::{row_id, EventRow};
    let raw: RawWindowPage =
        serde_json::from_str(body).map_err(|_| SyncError::MalformedResponse)?;
    let mut snapshot = WindowSnapshot::default();
    for event in raw.value {
        if event.is_cancelled
            || event
                .response_status
                .as_ref()
                .and_then(|r| r.response.as_deref())
                == Some("declined")
        {
            continue;
        }
        let id = row_id("microsoft", calendar_id, &event.id);
        let times = event
            .start
            .as_ref()
            .and_then(|t| event_time(t, event.is_all_day))
            .zip(
                event
                    .end
                    .as_ref()
                    .and_then(|t| event_time(t, event.is_all_day)),
            );
        let Some((start_time, end_time)) = times else {
            snapshot.keep_ids.push(id);
            continue;
        };
        snapshot.rows.push(EventRow {
            id,
            calendar_id: calendar_id.to_owned(),
            provider: "microsoft",
            summary: event.subject,
            start_time,
            end_time,
            is_all_day: event.is_all_day,
            status: "confirmed".to_owned(),
            hangout_link: event.online_meeting.and_then(|m| m.join_url),
            conference_data: None,
            organizer_email: event
                .organizer
                .and_then(|o| o.email_address)
                .and_then(|e| e.address),
            attendees: event
                .attendees
                .into_iter()
                .map(|a| StoredAttendee {
                    email: a.email_address.as_ref().and_then(|e| e.address.clone()),
                    name: a.email_address.and_then(|e| e.name),
                    response_status: a.status.and_then(|s| s.response),
                })
                .collect(),
        });
    }
    Ok((snapshot, raw.next_link))
}

/// Fetch an expanded snapshot for this calendar, using one frozen pair of
/// bounds. A page failure returns no snapshot and therefore permits no writes.
pub async fn fetch_window(
    policy: &NetworkPolicy,
    access_token: &Secret,
    calendar_id: &str,
    start: &chrono::DateTime<chrono::Utc>,
    end: &chrono::DateTime<chrono::Utc>,
) -> Result<WindowSnapshot, SyncError> {
    fetch_window_with(calendar_id, start, end, |url| async move {
        let path = url::Url::parse(&calendar_view_url(calendar_id))
            .map_err(|_| SyncError::UnsafePaginationUrl)?
            .path()
            .to_owned();
        get(policy, access_token, &url, &path).await
    })
    .await
}

async fn fetch_window_with<F, Fut>(
    calendar_id: &str,
    start: &chrono::DateTime<chrono::Utc>,
    end: &chrono::DateTime<chrono::Utc>,
    mut fetch: F,
) -> Result<WindowSnapshot, SyncError>
where
    F: FnMut(String) -> Fut,
    Fut: std::future::Future<Output = Result<(u16, String), SyncError>>,
{
    let base = calendar_view_url(calendar_id);
    let path = url::Url::parse(&base)
        .map_err(|_| SyncError::UnsafePaginationUrl)?
        .path()
        .to_owned();
    let initial = format!(
        "{base}?startDateTime={}&endDateTime={}",
        percent_encode_path_segment(&start.to_rfc3339()),
        percent_encode_path_segment(&end.to_rfc3339())
    );
    collect_window(
        initial,
        |url| {
            let safe = validate_graph_url(&url, &path);
            // Validate before calling even an injected transport: no credentials
            // or test transport should ever see a provider-controlled unsafe URL.
            let pending = safe.map(|()| fetch(url));
            async move {
                match pending {
                    Ok(future) => future.await,
                    Err(err) => Err(err),
                }
            }
        },
        |body| parse_window_page(body, calendar_id),
    )
    .await
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

    fn event(id: &str, response: &str) -> serde_json::Value {
        serde_json::json!({"id": id, "type": "occurrence", "responseStatus": {"response": response},
            "start": {"dateTime": "2026-11-11T09:00:00.0000000", "timeZone": "UTC"},
            "end": {"dateTime": "2026-11-11T10:00:00.0000000", "timeZone": "UTC"},
            "attendees": [{"emailAddress": {"name": "Guest", "address": "guest@example.com"}, "status": {"response": "tentative"}}]})
    }

    #[test]
    fn calendar_view_occurrences_dates_declines_and_attendee_responses() {
        let mut day = event("day", "organizer");
        day["isAllDay"] = true.into();
        let mut bad = event("bad", "accepted");
        bad["start"]["timeZone"] = "Europe/Stockholm".into();
        let body = serde_json::json!({"value": [event("occurrence", "tentative"), event("unknown", "unknown"), event("declined", "declined"), day, bad], "@odata.nextLink": "https://graph.microsoft.com/v1.0/me/calendars/team/calendarView?$skiptoken=abc"}).to_string();
        let (snapshot, next) = parse_window_page(&body, "team").unwrap();
        assert!(next.unwrap().contains("calendarView"));
        assert_eq!(snapshot.rows.len(), 3);
        assert_eq!(snapshot.rows[0].calendar_id, "team");
        assert_eq!(snapshot.rows[0].start_time, "2026-11-11T09:00:00Z");
        assert_eq!(
            snapshot.rows[0].attendees[0].response_status.as_deref(),
            Some("tentative")
        );
        assert!(snapshot.rows[2].is_all_day);
        assert_eq!(snapshot.rows[2].start_time, "2026-11-11");
        assert_eq!(snapshot.keep_ids, ["microsoft:team:bad"]);
    }

    #[tokio::test]
    async fn calendar_view_follows_only_valid_links_and_discards_incomplete_chains() {
        let start = crate::events_repo::parse_row_instant("2026-11-10T00:00:00Z").unwrap();
        let end = start + chrono::Duration::days(31);
        for mode in ["success", "unsafe", "failure", "overflow"] {
            let mut calls = Vec::new();
            let next = if mode == "unsafe" {
                "https://graph.microsoft.com/v1.0/me/messages"
            } else {
                "https://graph.microsoft.com/v1.0/me/calendars/team/calendarView?$skiptoken=abc"
            };
            let result = fetch_window_with("team", &start, &end, |url| {
                calls.push(url);
                let response = if calls.len() == 2 && mode == "failure" { (503, String::new()) }
                    else if calls.len() == 2 && mode == "success" { (200, r#"{"value":[]}"#.to_owned()) }
                    else { (200, serde_json::json!({"value": [event("occurrence", "accepted")], "@odata.nextLink": next}).to_string()) };
                std::future::ready(Ok(response))
            }).await;
            match mode {
                "success" => {
                    assert_eq!(result.unwrap().rows.len(), 1);
                    assert_eq!(calls[1], next);
                }
                "unsafe" => {
                    assert!(matches!(result, Err(SyncError::UnsafePaginationUrl)));
                    assert_eq!(calls.len(), 1);
                }
                "overflow" => {
                    assert!(matches!(result, Err(SyncError::TooManyPages)));
                    assert_eq!(calls.len(), 25);
                }
                _ => assert!(matches!(result, Err(SyncError::HttpStatus { status: 503 }))),
            }
            let initial = url::Url::parse(&calls[0]).unwrap();
            let params: std::collections::HashMap<_, _> =
                initial.query_pairs().into_owned().collect();
            assert_eq!(params["startDateTime"], start.to_rfc3339());
            assert_eq!(params["endDateTime"], end.to_rfc3339());
            assert_eq!(initial.path(), "/v1.0/me/calendars/team/calendarView");
        }
    }

    /// Modelled on
    /// <https://learn.microsoft.com/en-us/graph/api/calendar-list-calendars>.
    /// UNVERIFIED against a real tenant -- see the module doc.
    const CALENDARS_FIXTURE: &str = r#"{
        "value": [
            {
                "id": "AAMkAGI1AAAAAAAA=",
                "name": "Calendar",
                "color": "auto",
                "isDefaultCalendar": true,
                "owner": {"name": "Jane Doe", "address": "jane@example.com"}
            },
            {
                "id": "AAMkAGI1BBBBBBBB=",
                "name": "Team events",
                "color": "lightBlue",
                "owner": {"name": "Jane Doe", "address": "jane@example.com"}
            }
        ]
    }"#;

    #[test]
    fn calendars_are_parsed_and_auto_color_is_dropped() {
        let (entries, next) = parse_calendars_page(CALENDARS_FIXTURE).unwrap();
        assert_eq!(next, None);
        assert_eq!(entries.len(), 2);
        assert!(entries[0].is_primary);
        assert_eq!(entries[0].background_color, None, "\"auto\" is not a color");
        assert_eq!(entries[1].background_color.as_deref(), Some("lightBlue"));
    }

    #[test]
    fn the_default_calendars_owner_names_the_account() {
        let (entries, _) = parse_calendars_page(CALENDARS_FIXTURE).unwrap();
        assert_eq!(account_owner_email(&entries), Some("jane@example.com"));
    }

    #[test]
    fn no_default_calendar_is_reported_as_none_not_a_guess() {
        let body = r#"{"value":[{"id":"x","name":"x"}]}"#;
        let (entries, _) = parse_calendars_page(body).unwrap();
        assert_eq!(account_owner_email(&entries), None);
    }

    #[test]
    fn a_next_link_is_carried_through() {
        let body = r#"{"value":[],"@odata.nextLink":"https://graph.microsoft.com/v1.0/me/calendars?$skip=20"}"#;
        let (_, next) = parse_calendars_page(body).unwrap();
        assert_eq!(
            next,
            Some("https://graph.microsoft.com/v1.0/me/calendars?$skip=20".to_owned())
        );
    }

    #[test]
    fn graph_urls_are_limited_to_the_expected_origin_and_resource() {
        let path = "/v1.0/me/calendars/x/calendarView";
        assert!(validate_graph_url(
            "https://graph.microsoft.com/v1.0/me/calendars/x/calendarView?$skiptoken=abc",
            path
        )
        .is_ok());

        for unsafe_url in [
            "https://attacker.example/collect",
            "https://graph.microsoft.com.attacker.example/v1.0/me/calendars/x/calendarView",
            "https://graph.microsoft.com:8443/v1.0/me/calendars/x/calendarView",
            "https://graph.microsoft.com/v1.0/me/messages",
            "http://graph.microsoft.com/v1.0/me/calendars/x/calendarView",
        ] {
            assert!(matches!(
                validate_graph_url(unsafe_url, path),
                Err(SyncError::UnsafePaginationUrl)
            ));
        }
    }
}
