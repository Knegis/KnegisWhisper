//! Google Calendar API v3 calendar lists and bounded event snapshots.
//!
//! Window requests expand recurring instances and repeat the same bounds on
//! every page. These request shapes are fixture-tested; live provider access
//! is not part of the offline test suite. Parsing never writes to storage, so
//! an incomplete page chain cannot replace a calendar's cached snapshot.

use kw_network_policy::{NetworkPolicy, RequestPurpose};
use kw_security::Secret;
use serde::Deserialize;

use crate::sync_support::{
    collect_window, percent_encode_path_segment, to_utc, StoredAttendee, SyncError, WindowSnapshot,
    MAX_PAGES,
};

const CALENDAR_LIST_URL: &str = "https://www.googleapis.com/calendar/v3/users/me/calendarList";

fn events_url(calendar_id: &str) -> String {
    format!(
        "https://www.googleapis.com/calendar/v3/calendars/{}/events",
        percent_encode_path_segment(calendar_id)
    )
}

/// One entry from `calendarList.list`, after normalising away the wire
/// shape's naming.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CalendarListEntry {
    pub id: String,
    pub summary: String,
    pub description: Option<String>,
    pub background_color: Option<String>,
    pub is_primary: bool,
    /// Whether the calendar shows in the user's own Google Calendar UI.
    /// Carried through as the starting value for our own `is_selected`
    /// column on first sight of a calendar; a later refresh never touches it
    /// again, so a user's local selection choice survives -- see
    /// `crate::calendars_repo::upsert_calendar`.
    pub selected: bool,
}

#[derive(Debug, Deserialize)]
struct RawCalendarList {
    #[serde(default)]
    items: Vec<RawCalendarListEntry>,
    #[serde(rename = "nextPageToken", default)]
    next_page_token: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawCalendarListEntry {
    id: String,
    #[serde(default)]
    summary: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(rename = "backgroundColor", default)]
    background_color: Option<String>,
    #[serde(default)]
    primary: bool,
    // Absent is documented to mean "not selected" for a non-primary calendar,
    // but the primary calendar is always effectively selected. Believed, not
    // verified -- see the module doc. Defaulting to the primary calendar's
    // own value covers the one case that matters most: a fresh account's own
    // calendar should not come in unselected.
    #[serde(default)]
    selected: Option<bool>,
}

/// Parse one page of `calendarList.list`, pure so it can be fixture-tested.
fn parse_calendar_list_page(
    body: &str,
) -> Result<(Vec<CalendarListEntry>, Option<String>), SyncError> {
    let raw: RawCalendarList =
        serde_json::from_str(body).map_err(|_json_error| SyncError::MalformedResponse)?;
    let items = raw
        .items
        .into_iter()
        .map(|e| CalendarListEntry {
            selected: e.selected.unwrap_or(e.primary),
            id: e.id,
            summary: e.summary.unwrap_or_default(),
            description: e.description,
            background_color: e.background_color,
            is_primary: e.primary,
        })
        .collect();
    Ok((items, raw.next_page_token))
}

/// The primary calendar id names the account in Google calendar-list responses.
/// This assumption is fixture-tested and has not been checked against a live account.
#[must_use]
pub fn primary_email(calendars: &[CalendarListEntry]) -> Option<&str> {
    calendars
        .iter()
        .find(|c| c.is_primary)
        .map(|c| c.id.as_str())
}

async fn get(
    policy: &NetworkPolicy,
    access_token: &Secret,
    url: &str,
) -> Result<(u16, String), SyncError> {
    let response = policy
        .get(RequestPurpose::UserConfiguredCalendarProvider, url)?
        .bearer_auth(access_token.expose())
        .send()
        .await
        .map_err(SyncError::Http)?;
    let status = response.status().as_u16();
    let body = response.text().await.map_err(SyncError::Http)?;
    Ok((status, body))
}

/// Fetch every calendar in the account's list, following pagination.
///
/// UNVERIFIED network path -- see the module doc. The parsing it calls,
/// [`parse_calendar_list_page`], is what the fixture tests exercise.
pub async fn fetch_calendar_list(
    policy: &NetworkPolicy,
    access_token: &Secret,
) -> Result<Vec<CalendarListEntry>, SyncError> {
    let mut items = Vec::new();
    let mut page_token: Option<String> = None;

    for _ in 0..MAX_PAGES {
        let url = match &page_token {
            Some(token) => format!(
                "{CALENDAR_LIST_URL}?pageToken={}",
                percent_encode_path_segment(token)
            ),
            None => CALENDAR_LIST_URL.to_owned(),
        };
        let (status, body) = get(policy, access_token, &url).await?;
        if status != 200 {
            return Err(SyncError::HttpStatus { status });
        }
        let (page, next) = parse_calendar_list_page(&body)?;
        items.extend(page);
        match next {
            Some(token) => page_token = Some(token),
            None => return Ok(items),
        }
    }
    Err(SyncError::TooManyPages)
}

/* ------------------------------------------------------------------ events */

#[derive(Debug, Deserialize)]
struct RawEventsPage {
    #[serde(default)]
    items: Vec<RawEvent>,
    #[serde(rename = "nextPageToken", default)]
    next_page_token: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawEvent {
    id: String,
    #[serde(default = "confirmed")]
    status: String,
    summary: Option<String>,
    start: Option<RawEventTime>,
    end: Option<RawEventTime>,
    #[serde(rename = "hangoutLink")]
    hangout_link: Option<String>,
    #[serde(rename = "conferenceData")]
    conference_data: Option<serde_json::Value>,
    organizer: Option<RawOrganizer>,
    #[serde(default)]
    attendees: Vec<RawAttendee>,
}

fn confirmed() -> String {
    "confirmed".to_owned()
}

#[derive(Debug, Deserialize)]
struct RawEventTime {
    date: Option<String>,
    #[serde(rename = "dateTime")]
    date_time: Option<String>,
    #[serde(rename = "timeZone")]
    time_zone: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawOrganizer {
    email: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawAttendee {
    email: Option<String>,
    #[serde(rename = "responseStatus")]
    response_status: Option<String>,
    #[serde(rename = "displayName")]
    display_name: Option<String>,
    #[serde(rename = "self", default)]
    is_self: bool,
}

fn event_time(time: &RawEventTime) -> Option<(String, bool)> {
    if let Some(date) = &time.date {
        chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d").ok()?;
        Some((date.clone(), true))
    } else {
        Some((
            to_utc(time.date_time.as_deref()?, time.time_zone.as_deref())?,
            false,
        ))
    }
}

fn parse_window_page(
    body: &str,
    calendar_id: &str,
) -> Result<(WindowSnapshot, Option<String>), SyncError> {
    use crate::events_repo::{row_id, EventRow};
    let raw: RawEventsPage =
        serde_json::from_str(body).map_err(|_| SyncError::MalformedResponse)?;
    let mut snapshot = WindowSnapshot::default();
    for event in raw.items {
        if event.status == "cancelled"
            || event
                .attendees
                .iter()
                .any(|a| a.is_self && a.response_status.as_deref() == Some("declined"))
        {
            continue;
        }
        let id = row_id("google", calendar_id, &event.id);
        let times = event
            .start
            .as_ref()
            .and_then(event_time)
            .zip(event.end.as_ref().and_then(event_time));
        let Some(((start_time, is_all_day), (end_time, _))) = times else {
            snapshot.keep_ids.push(id);
            continue;
        };
        snapshot.rows.push(EventRow {
            id,
            calendar_id: calendar_id.to_owned(),
            provider: "google",
            summary: event.summary,
            start_time,
            end_time,
            is_all_day,
            status: event.status,
            hangout_link: event.hangout_link,
            conference_data: event
                .conference_data
                .and_then(|v| serde_json::to_string(&v).ok()),
            organizer_email: event.organizer.and_then(|o| o.email),
            attendees: event
                .attendees
                .into_iter()
                .map(|a| StoredAttendee {
                    email: a.email,
                    name: a.display_name,
                    response_status: a.response_status,
                })
                .collect(),
        });
    }
    Ok((snapshot, raw.next_page_token))
}

fn window_url(
    calendar_id: &str,
    start: &chrono::DateTime<chrono::Utc>,
    end: &chrono::DateTime<chrono::Utc>,
) -> String {
    format!(
        "{}?singleEvents=true&timeMin={}&timeMax={}&showDeleted=false&maxResults=2500",
        events_url(calendar_id),
        percent_encode_path_segment(&start.to_rfc3339()),
        percent_encode_path_segment(&end.to_rfc3339())
    )
}

/// Fetch every expanded instance in the frozen window. Only a complete page
/// chain is returned; callers must not commit an error as an empty snapshot.
pub async fn fetch_window(
    policy: &NetworkPolicy,
    access_token: &Secret,
    calendar_id: &str,
    start: &chrono::DateTime<chrono::Utc>,
    end: &chrono::DateTime<chrono::Utc>,
) -> Result<WindowSnapshot, SyncError> {
    fetch_window_with(calendar_id, start, end, |url| async move {
        get(policy, access_token, &url).await
    })
    .await
}

async fn fetch_window_with<F, Fut>(
    calendar_id: &str,
    start: &chrono::DateTime<chrono::Utc>,
    end: &chrono::DateTime<chrono::Utc>,
    fetch: F,
) -> Result<WindowSnapshot, SyncError>
where
    F: FnMut(String) -> Fut,
    Fut: std::future::Future<Output = Result<(u16, String), SyncError>>,
{
    let base = window_url(calendar_id, start, end);
    collect_window(base.clone(), fetch, |body| {
        let (page, next) = parse_window_page(body, calendar_id)?;
        Ok((
            page,
            next.map(|token| format!("{base}&pageToken={}", percent_encode_path_segment(&token))),
        ))
    })
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

    fn timed_event(id: &str, response: &str, offset: &str) -> serde_json::Value {
        serde_json::json!({"id": id, "start": {"dateTime": format!("2026-11-11T09:00:00{offset}")},
            "end": {"dateTime": format!("2026-11-11T10:00:00{offset}")},
            "attendees": [{"self": true, "displayName": "Connected User", "responseStatus": response}]})
    }

    #[test]
    fn window_fixture_normalizes_offsets_dates_names_and_self_declines() {
        let body = serde_json::json!({"items": [
            timed_event("plus", "tentative", "+01:00"), timed_event("minus", "unknown", "-05:00"),
            timed_event("utc", "accepted", "Z"), timed_event("declined", "declined", "Z"),
            {"id": "day", "start": {"date": "2026-11-11"}, "end": {"date": "2026-11-12"}},
            {"id": "bad", "start": {"dateTime": "garbage"}},
            {"id": "cancelled", "status": "cancelled"}
        ], "nextPageToken": "page two"})
        .to_string();
        let (snapshot, next) = parse_window_page(&body, "team").unwrap();
        assert_eq!(next.as_deref(), Some("page two"));
        assert_eq!(snapshot.rows.len(), 4);
        assert_eq!(snapshot.rows[0].start_time, "2026-11-11T08:00:00Z");
        assert_eq!(snapshot.rows[1].start_time, "2026-11-11T14:00:00Z");
        assert_eq!(snapshot.rows[2].start_time, "2026-11-11T09:00:00Z");
        assert!(snapshot.rows[3].is_all_day);
        assert_eq!(snapshot.rows[3].end_time, "2026-11-12");
        assert_eq!(
            snapshot.rows[0].attendees[0].name.as_deref(),
            Some("Connected User")
        );
        assert_eq!(snapshot.keep_ids, ["google:team:bad"]);
        let body = serde_json::json!({"items": [{"id": "other", "start": {"date": "2026-11-11"}, "end": {"date": "2026-11-12"}, "attendees": [{"self": false, "responseStatus": "declined"}]}]}).to_string();
        assert_eq!(parse_window_page(&body, "team").unwrap().0.rows.len(), 1);
    }

    #[tokio::test]
    async fn every_window_page_repeats_the_frozen_query() {
        let start = crate::events_repo::parse_row_instant("2026-11-10T12:00:00Z").unwrap();
        let end = start + chrono::Duration::days(31);
        let mut urls = Vec::new();
        let snapshot = fetch_window_with("team@example.com", &start, &end, |url| {
            urls.push(url);
            let body = if urls.len() == 1 {
                r#"{"items":[],"nextPageToken":"page two"}"#
            } else {
                r#"{"items":[]}"#
            };
            std::future::ready(Ok((200, body.to_owned())))
        })
        .await
        .unwrap();
        assert!(snapshot.rows.is_empty());
        assert_eq!(urls.len(), 2);
        assert_eq!(urls[1], format!("{}&pageToken=page%20two", urls[0]));
        let url = url::Url::parse(&urls[0]).unwrap();
        let params: std::collections::HashMap<_, _> = url.query_pairs().into_owned().collect();
        assert_eq!(params["timeMin"], start.to_rfc3339());
        assert_eq!(params["timeMax"], end.to_rfc3339());
        assert_eq!(params["maxResults"], "2500");
        assert_eq!(params["singleEvents"], "true");
        assert_eq!(params["showDeleted"], "false");
    }

    #[tokio::test]
    async fn incomplete_pages_preserve_cache_while_another_calendar_commits() {
        use crate::events_repo::{count_events, replace_window};
        let db = kw_storage::Db::open_in_memory().await.unwrap();
        let start = crate::events_repo::parse_row_instant("2026-11-10T00:00:00Z").unwrap();
        let end = start + chrono::Duration::days(31);
        let body = serde_json::json!({"items": [timed_event("old", "accepted", "Z")]}).to_string();
        let cached = parse_window_page(&body, "bad-calendar").unwrap().0;
        replace_window(
            db.pool(),
            "google",
            "bad-calendar",
            &start,
            &end,
            &cached.rows,
            &[],
        )
        .await
        .unwrap();
        for failure in ["http", "parse", "overflow"] {
            let mut calls = 0;
            let result = fetch_window_with("bad-calendar", &start, &end, |_| {
                calls += 1;
                let response = if calls == 2 && failure == "http" {
                    (503, String::new())
                } else if calls == 2 && failure == "parse" {
                    (200, "invalid json".to_owned())
                } else {
                    (200, r#"{"items":[],"nextPageToken":"more"}"#.to_owned())
                };
                std::future::ready(Ok(response))
            })
            .await;
            assert!(result.is_err());
            if failure == "overflow" {
                assert!(matches!(result, Err(SyncError::TooManyPages)));
                assert_eq!(calls, 25);
            }
            if let Ok(snapshot) = result {
                replace_window(
                    db.pool(),
                    "google",
                    "bad-calendar",
                    &start,
                    &end,
                    &snapshot.rows,
                    &snapshot.keep_ids,
                )
                .await
                .unwrap();
            }
            let stored: String = sqlx::query_scalar(
                "SELECT start_time FROM calendar_events WHERE id = 'google:bad-calendar:old'",
            )
            .fetch_one(db.pool())
            .await
            .unwrap();
            assert_eq!(stored, cached.rows[0].start_time);
        }
        let good = fetch_window_with("good-calendar", &start, &end, |_| {
            std::future::ready(Ok((200, body.clone())))
        })
        .await
        .unwrap();
        replace_window(
            db.pool(),
            "google",
            "good-calendar",
            &start,
            &end,
            &good.rows,
            &good.keep_ids,
        )
        .await
        .unwrap();
        assert_eq!(
            count_events(db.pool(), "google", "good-calendar")
                .await
                .unwrap(),
            1
        );
        assert_eq!(
            count_events(db.pool(), "google", "bad-calendar")
                .await
                .unwrap(),
            1
        );
    }

    /// Modelled on the shape documented at
    /// <https://developers.google.com/calendar/api/v3/reference/calendarList/list>.
    /// UNVERIFIED against a real response -- see the module doc.
    const CALENDAR_LIST_FIXTURE: &str = r##"{
        "kind": "calendar#calendarList",
        "items": [
            {
                "kind": "calendar#calendarListEntry",
                "id": "user@example.com",
                "summary": "user@example.com",
                "primary": true,
                "selected": true
            },
            {
                "kind": "calendar#calendarListEntry",
                "id": "abc123@group.calendar.google.com",
                "summary": "Team events",
                "description": "Shared team calendar",
                "backgroundColor": "#ff0000",
                "selected": false
            }
        ]
    }"##;

    #[test]
    fn a_calendar_list_page_is_parsed_into_normalised_entries() {
        let (entries, next) = parse_calendar_list_page(CALENDAR_LIST_FIXTURE).unwrap();
        assert_eq!(next, None);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].id, "user@example.com");
        assert!(entries[0].is_primary);
        assert!(entries[0].selected);
        assert_eq!(entries[1].summary, "Team events");
        assert_eq!(
            entries[1].description.as_deref(),
            Some("Shared team calendar")
        );
        assert!(!entries[1].selected);
    }

    #[test]
    fn the_primary_calendars_id_is_the_account_email() {
        let (entries, _) = parse_calendar_list_page(CALENDAR_LIST_FIXTURE).unwrap();
        assert_eq!(primary_email(&entries), Some("user@example.com"));
    }

    #[test]
    fn no_primary_entry_is_reported_as_none_not_a_guess() {
        let body = r#"{"items":[{"id":"x@group.calendar.google.com","summary":"x"}]}"#;
        let (entries, _) = parse_calendar_list_page(body).unwrap();
        assert_eq!(primary_email(&entries), None);
    }

    #[test]
    fn a_next_page_token_is_carried_through() {
        let body = r#"{"items":[],"nextPageToken":"page-2"}"#;
        let (_, next) = parse_calendar_list_page(body).unwrap();
        assert_eq!(next, Some("page-2".to_owned()));
    }

    #[test]
    fn unparseable_json_is_a_malformed_response_not_a_panic() {
        assert!(matches!(
            parse_calendar_list_page("not json"),
            Err(SyncError::MalformedResponse)
        ));
    }
}
