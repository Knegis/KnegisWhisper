//! Google Calendar API v3: the calendar list and incremental event sync.
//!
//! # What is verified and what is not
//!
//! There is no Google OAuth client id anywhere in this checkout -- the whole
//! point of `crate::flow` is that this crate never has one of its own -- so
//! nothing in this module has ever completed a real request. Every request
//! shape and every response shape below is written against Google's public
//! reference documentation
//! (<https://developers.google.com/calendar/api/v3/reference>) and its sync
//! guide (<https://developers.google.com/calendar/api/guides/sync>), and
//! pinned by the fixture tests at the bottom of this file rather than by a
//! live call. **UNVERIFIED**, specifically:
//!
//! * That `calendarList.list` really marks exactly one entry `primary: true`
//!   for every account, and that its `id` is the account's email address.
//!   [`primary_email`] depends on this to name the account a set of tokens
//!   belongs to.
//! * The exact 410 behaviour of `events.list` with an expired `syncToken` --
//!   believed to be a bare 410 with no distinguishing body, so
//!   [`fetch_events`] treats *any* 410 on this endpoint as
//!   [`SyncError::SyncTokenExpired`] rather than inspecting the body for a
//!   reason.
//! * That a cancelled event surfaces during an incremental sync with only
//!   `id` and `status: "cancelled"` populated, and is omitted entirely from a
//!   full sync (no `syncToken`). [`normalize_event`] and [`plan_event_writes`]
//!   are written to that description; nothing has exercised it against a
//!   real calendar with a real deletion.
//!
//! # What this module does not do
//!
//! It does not write to the database. [`crate::calendars_repo`] and
//! [`crate::events_repo`] do that; this module only talks to Google and
//! turns what comes back into the shapes those repos take. Keeping the wire
//! parsing and the SQL apart is what let the parsing be fixture-tested with
//! no database in sight.

use kw_network_policy::{NetworkPolicy, RequestPurpose};
use kw_security::Secret;
use serde::Deserialize;

use crate::sync_support::{percent_encode_path_segment, StoredAttendee, SyncError, MAX_PAGES};

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

/// The calendar whose id names the account, per the module doc's first
/// UNVERIFIED item.
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

/// One event fetched from a Google calendar, or the fact that a previously
/// seen one was cancelled and should be removed locally.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyncedEvent {
    Upsert(NormalizedEvent),
    /// `status: "cancelled"` on an incremental sync -- Google's signal that
    /// the client should delete this id. UNVERIFIED; see the module doc.
    Removed {
        id: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NormalizedEvent {
    pub id: String,
    pub summary: Option<String>,
    pub start_time: String,
    pub end_time: String,
    pub is_all_day: bool,
    pub status: String,
    pub hangout_link: Option<String>,
    pub conference_data: Option<String>,
    pub organizer_email: Option<String>,
    pub attendees: Vec<StoredAttendee>,
}

#[derive(Debug, Deserialize)]
struct RawEventsPage {
    #[serde(default)]
    items: Vec<RawEvent>,
    #[serde(rename = "nextPageToken", default)]
    next_page_token: Option<String>,
    #[serde(rename = "nextSyncToken", default)]
    next_sync_token: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawEvent {
    id: String,
    #[serde(default = "confirmed")]
    status: String,
    #[serde(default)]
    summary: Option<String>,
    #[serde(default)]
    start: Option<RawEventTime>,
    #[serde(default)]
    end: Option<RawEventTime>,
    #[serde(rename = "hangoutLink", default)]
    hangout_link: Option<String>,
    #[serde(rename = "conferenceData", default)]
    conference_data: Option<serde_json::Value>,
    #[serde(default)]
    organizer: Option<RawOrganizer>,
    #[serde(default)]
    attendees: Vec<RawAttendee>,
}

fn confirmed() -> String {
    "confirmed".to_owned()
}

#[derive(Debug, Deserialize)]
struct RawEventTime {
    #[serde(default)]
    date: Option<String>,
    #[serde(rename = "dateTime", default)]
    date_time: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawOrganizer {
    #[serde(default)]
    email: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawAttendee {
    #[serde(default)]
    email: Option<String>,
    #[serde(rename = "responseStatus", default)]
    response_status: Option<String>,
}

/// Turn one raw event into what gets written locally, or the fact that it
/// should be removed. `None` for an event this cannot make sense of --
/// missing both `date` and `dateTime` on an event that is not a removal --
/// which is skipped rather than written with an invented time.
fn normalize_event(raw: RawEvent) -> Option<SyncedEvent> {
    if raw.status == "cancelled" {
        return Some(SyncedEvent::Removed { id: raw.id });
    }

    let start = raw.start?;
    let end = raw.end?;
    let (start_time, start_all_day) = event_time(&start)?;
    let (end_time, _) = event_time(&end)?;

    Some(SyncedEvent::Upsert(NormalizedEvent {
        id: raw.id,
        summary: raw.summary,
        start_time,
        end_time,
        is_all_day: start_all_day,
        status: raw.status,
        hangout_link: raw.hangout_link,
        conference_data: raw
            .conference_data
            .and_then(|v| serde_json::to_string(&v).ok()),
        organizer_email: raw.organizer.and_then(|o| o.email),
        attendees: raw
            .attendees
            .into_iter()
            .map(|a| StoredAttendee {
                email: a.email,
                name: None,
                response_status: a.response_status,
            })
            .collect(),
    }))
}

/// `(value, is_all_day)`, or `None` if neither `date` nor `dateTime` is set.
fn event_time(time: &RawEventTime) -> Option<(String, bool)> {
    if let Some(date) = &time.date {
        Some((date.clone(), true))
    } else {
        time.date_time.clone().map(|dt| (dt, false))
    }
}

/// One parsed page of `events.list`.
struct EventsPage {
    items: Vec<SyncedEvent>,
    next_page_token: Option<String>,
    next_sync_token: Option<String>,
}

/// Parse one page of `events.list`, pure so it can be fixture-tested.
fn parse_events_page(body: &str) -> Result<EventsPage, SyncError> {
    let raw: RawEventsPage =
        serde_json::from_str(body).map_err(|_json_error| SyncError::MalformedResponse)?;
    let items = raw.items.into_iter().filter_map(normalize_event).collect();
    Ok(EventsPage {
        items,
        next_page_token: raw.next_page_token,
        next_sync_token: raw.next_sync_token,
    })
}

/// What one calendar's fetch produced: every item seen, and the cursor to
/// store for next time -- `None` when the provider never sent one, which
/// [`crate::calendars_repo`] leaves the existing value alone rather than
/// erasing it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct EventsFetch {
    pub events: Vec<SyncedEvent>,
    pub next_sync_token: Option<String>,
}

/// Fetch events for `calendar_id`, following `sync_token` when there is one.
///
/// A 410 is reported as [`SyncError::SyncTokenExpired`] without inspecting the
/// body -- see the module doc. The caller's answer to that is to retry with
/// `sync_token: None`, which asks for a full list instead of a delta.
///
/// UNVERIFIED network path; [`parse_events_page`] is what is fixture-tested.
pub async fn fetch_events(
    policy: &NetworkPolicy,
    access_token: &Secret,
    calendar_id: &str,
    sync_token: Option<&str>,
) -> Result<EventsFetch, SyncError> {
    let base = events_url(calendar_id);
    let mut events = Vec::new();
    let mut next_sync_token = None;
    let mut page_token: Option<String> = None;

    for _ in 0..MAX_PAGES {
        let mut params = vec!["singleEvents=true".to_owned()];
        if let Some(token) = &page_token {
            params.push(format!("pageToken={}", percent_encode_path_segment(token)));
        } else if let Some(token) = sync_token {
            params.push(format!("syncToken={}", percent_encode_path_segment(token)));
        }
        let url = format!("{base}?{}", params.join("&"));

        let (status, body) = get(policy, access_token, &url).await?;
        if status == 410 {
            return Err(SyncError::SyncTokenExpired);
        }
        if status != 200 {
            return Err(SyncError::HttpStatus { status });
        }

        let page = parse_events_page(&body)?;
        events.extend(page.items);
        if page.next_sync_token.is_some() {
            next_sync_token = page.next_sync_token;
        }

        match page.next_page_token {
            Some(token) => page_token = Some(token),
            None => {
                return Ok(EventsFetch {
                    events,
                    next_sync_token,
                })
            }
        }
    }
    Err(SyncError::TooManyPages)
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

    /// Modelled on
    /// <https://developers.google.com/calendar/api/v3/reference/events/list>.
    /// UNVERIFIED against a real response -- see the module doc.
    const EVENTS_FIXTURE: &str = r#"{
        "items": [
            {
                "id": "evt1",
                "status": "confirmed",
                "summary": "Standup",
                "start": {"dateTime": "2026-01-05T09:00:00Z"},
                "end": {"dateTime": "2026-01-05T09:15:00Z"},
                "hangoutLink": "https://meet.google.com/abc-defg-hij",
                "organizer": {"email": "lead@example.com"},
                "attendees": [
                    {"email": "a@example.com", "responseStatus": "accepted"},
                    {"email": "b@example.com", "responseStatus": "needsAction"}
                ]
            },
            {
                "id": "evt2",
                "status": "confirmed",
                "summary": "All-day offsite",
                "start": {"date": "2026-02-01"},
                "end": {"date": "2026-02-02"}
            },
            {
                "id": "evt3",
                "status": "cancelled"
            }
        ],
        "nextSyncToken": "sync-cursor-1"
    }"#;

    #[test]
    fn a_timed_event_is_normalised_with_its_attendees() {
        let page = parse_events_page(EVENTS_FIXTURE).unwrap();
        assert_eq!(page.next_sync_token, Some("sync-cursor-1".to_owned()));
        let SyncedEvent::Upsert(evt) = &page.items[0] else {
            panic!("expected an upsert");
        };
        assert_eq!(evt.id, "evt1");
        assert!(!evt.is_all_day);
        assert_eq!(evt.start_time, "2026-01-05T09:00:00Z");
        assert_eq!(
            evt.hangout_link.as_deref(),
            Some("https://meet.google.com/abc-defg-hij")
        );
        assert_eq!(evt.organizer_email.as_deref(), Some("lead@example.com"));
        assert_eq!(evt.attendees.len(), 2);
    }

    #[test]
    fn an_all_day_event_is_marked_as_such() {
        let page = parse_events_page(EVENTS_FIXTURE).unwrap();
        let SyncedEvent::Upsert(evt) = &page.items[1] else {
            panic!("expected an upsert");
        };
        assert!(evt.is_all_day);
        assert_eq!(evt.start_time, "2026-02-01");
    }

    #[test]
    fn a_cancelled_event_is_a_removal_not_an_upsert() {
        let page = parse_events_page(EVENTS_FIXTURE).unwrap();
        assert_eq!(
            page.items[2],
            SyncedEvent::Removed {
                id: "evt3".to_owned()
            }
        );
    }

    #[test]
    fn an_event_with_neither_date_nor_datetime_is_skipped_not_invented() {
        let body = r#"{"items":[{"id":"broken","status":"confirmed"}]}"#;
        let page = parse_events_page(body).unwrap();
        assert!(page.items.is_empty());
    }

    #[test]
    fn conference_data_round_trips_as_a_json_string() {
        let body = r#"{"items":[{
            "id":"evt-conf",
            "status":"confirmed",
            "start":{"dateTime":"2026-01-05T09:00:00Z"},
            "end":{"dateTime":"2026-01-05T09:15:00Z"},
            "conferenceData":{"conferenceId":"abc123"}
        }]}"#;
        let page = parse_events_page(body).unwrap();
        let SyncedEvent::Upsert(evt) = &page.items[0] else {
            panic!("expected an upsert");
        };
        let stored = evt.conference_data.as_deref().unwrap();
        assert!(stored.contains("abc123"));
    }
}
