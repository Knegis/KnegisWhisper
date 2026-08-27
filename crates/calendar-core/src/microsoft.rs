//! Microsoft Graph calendars: the calendar list and delta event sync.
//!
//! # What is verified and what is not
//!
//! Same position as `crate::google`, for the same reason: there is no
//! Microsoft OAuth client id anywhere in this checkout, so nothing here has
//! ever completed a real request. Every shape below is written against
//! Microsoft Graph's public reference
//! (<https://learn.microsoft.com/en-us/graph/api/resources/calendar>,
//! <https://learn.microsoft.com/en-us/graph/api/resources/event>) and its
//! delta-query guide
//! (<https://learn.microsoft.com/en-us/graph/delta-query-events>), pinned by
//! the fixture tests below rather than by a live call. **UNVERIFIED**,
//! specifically:
//!
//! * That `/me/calendars` needs no scope beyond `Calendars.Read` -- the only
//!   one `crate::provider::ProviderEndpoints::microsoft` requests -- and that
//!   a calendar's `owner.address` is the signed-in account's own mailbox
//!   address. [`account_owner_email`] depends on this to name the account a
//!   set of tokens belongs to; nothing here has confirmed it against a real
//!   tenant, and a shared or delegated calendar could plausibly report a
//!   different owner than expected.
//! * That the default calendar's events, fetched with no `Prefer:
//!   outlook.timezone` header, come back in UTC with `start.timeZone` /
//!   `end.timeZone` reading `"UTC"`. [`normalize_event`] stores
//!   `start.dateTime` as-is on that assumption; a tenant configured
//!   differently would be stored in whatever zone Graph actually used, not
//!   corrected to UTC.
//! * That Microsoft does not expire a delta link on a fixed schedule the way
//!   this schema's `sync_token_expires_at` column implies for its own sake
//!   (it exists for a schema shared with Google, whose `syncToken` genuinely
//!   can expire). Nothing here ever sets it -- see
//!   `crate::calendars_repo::set_sync_token`.
//!
//! # What this module does not do
//!
//! Same as `crate::google`: no database access. This only talks to Graph and
//! shapes the result for `crate::calendars_repo` / `crate::events_repo`.

use kw_network_policy::{NetworkPolicy, RequestPurpose};
use kw_security::Secret;
use serde::Deserialize;

use crate::sync_support::{percent_encode_path_segment, StoredAttendee, SyncError, MAX_PAGES};

const CALENDARS_URL: &str = "https://graph.microsoft.com/v1.0/me/calendars";

fn events_delta_url(calendar_id: &str) -> String {
    format!(
        "https://graph.microsoft.com/v1.0/me/calendars/{}/events/delta",
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

/// The default calendar's owner address, per the module doc's first
/// UNVERIFIED item.
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
        .send()
        .await
        .map_err(SyncError::Http)?;
    let status = response.status().as_u16();
    let body = response.text().await.map_err(SyncError::Http)?;
    Ok((status, body))
}

/// Graph pagination and delta links are opaque, but they must not be allowed
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyncedEvent {
    Upsert(NormalizedEvent),
    /// `@removed` on a delta page -- Graph's signal that the item was
    /// deleted (or, per the delta-query guide, moved out of scope) and
    /// should be removed locally. UNVERIFIED; see the module doc.
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
    pub organizer_email: Option<String>,
    pub attendees: Vec<StoredAttendee>,
}

#[derive(Debug, Deserialize)]
struct RawDeltaResponse {
    #[serde(default)]
    value: Vec<RawDeltaEvent>,
    #[serde(rename = "@odata.nextLink", default)]
    next_link: Option<String>,
    #[serde(rename = "@odata.deltaLink", default)]
    delta_link: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawDeltaEvent {
    #[serde(default)]
    id: Option<String>,
    #[serde(rename = "@removed", default)]
    removed: Option<serde_json::Value>,
    #[serde(default)]
    subject: Option<String>,
    #[serde(default)]
    start: Option<RawGraphDateTime>,
    #[serde(default)]
    end: Option<RawGraphDateTime>,
    #[serde(rename = "isAllDay", default)]
    is_all_day: bool,
    #[serde(rename = "isCancelled", default)]
    is_cancelled: bool,
    #[serde(rename = "onlineMeeting", default)]
    online_meeting: Option<RawOnlineMeeting>,
    #[serde(default)]
    organizer: Option<RawGraphOrganizer>,
    #[serde(default)]
    attendees: Vec<RawGraphAttendee>,
}

#[derive(Debug, Deserialize)]
struct RawGraphDateTime {
    #[serde(rename = "dateTime", default)]
    date_time: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawOnlineMeeting {
    #[serde(rename = "joinUrl", default)]
    join_url: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawGraphOrganizer {
    #[serde(rename = "emailAddress", default)]
    email_address: Option<RawGraphEmailAddress>,
}

#[derive(Debug, Deserialize)]
struct RawGraphEmailAddress {
    #[serde(default)]
    address: Option<String>,
    #[serde(default)]
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawGraphAttendee {
    #[serde(rename = "emailAddress", default)]
    email_address: Option<RawGraphEmailAddress>,
}

fn normalize_event(raw: RawDeltaEvent) -> Option<SyncedEvent> {
    if raw.removed.is_some() {
        return Some(SyncedEvent::Removed { id: raw.id? });
    }

    let id = raw.id?;
    let start = raw.start?.date_time?;
    let end = raw.end?.date_time?;

    Some(SyncedEvent::Upsert(NormalizedEvent {
        id,
        summary: raw.subject,
        start_time: start,
        end_time: end,
        is_all_day: raw.is_all_day,
        status: if raw.is_cancelled {
            "cancelled".to_owned()
        } else {
            "confirmed".to_owned()
        },
        hangout_link: raw.online_meeting.and_then(|m| m.join_url),
        organizer_email: raw
            .organizer
            .and_then(|o| o.email_address)
            .and_then(|e| e.address),
        attendees: raw
            .attendees
            .into_iter()
            .filter_map(|a| a.email_address)
            .map(|e| StoredAttendee {
                email: e.address,
                name: e.name,
                response_status: None,
            })
            .collect(),
    }))
}

/// One parsed page of a delta query.
struct DeltaPage {
    items: Vec<SyncedEvent>,
    next_link: Option<String>,
    delta_link: Option<String>,
}

fn parse_delta_page(body: &str) -> Result<DeltaPage, SyncError> {
    let raw: RawDeltaResponse =
        serde_json::from_str(body).map_err(|_json_error| SyncError::MalformedResponse)?;
    let items = raw.value.into_iter().filter_map(normalize_event).collect();
    Ok(DeltaPage {
        items,
        next_link: raw.next_link,
        delta_link: raw.delta_link,
    })
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct EventsFetch {
    pub events: Vec<SyncedEvent>,
    /// The full `@odata.deltaLink` URL to replay verbatim next time, or
    /// `None` if the provider never sent one before this stopped paging --
    /// see [`crate::calendars_repo::set_sync_token`] on what that means for
    /// the stored cursor.
    pub delta_link: Option<String>,
}

/// Fetch events for `calendar_id`, resuming from a stored `@odata.deltaLink`
/// when there is one.
///
/// `delta_cursor` is expected to be the *whole* URL a previous call returned
/// as `delta_link`, not a bare token -- Graph's delta links are opaque and
/// meant to be replayed as-is, unlike Google's `syncToken`. When it is
/// `None`, this asks for a fresh delta from the start.
///
/// A 410 is reported as [`SyncError::SyncTokenExpired`], the same as
/// `crate::google::fetch_events` -- Microsoft's own delta-query guide
/// documents a 410 with an `Location` header carrying a fresh, tokenless
/// delta link on an invalidated one, which this treats the same as Google's
/// bare 410: clear the cursor and ask the caller to retry with `None`. The
/// `Location` header itself is not read, so a retry always starts a full
/// delta rather than resuming from wherever Microsoft's header pointed --
/// UNVERIFIED, believed rather than confirmed against a real tenant.
///
/// UNVERIFIED network path otherwise too; [`parse_delta_page`] is what is
/// fixture-tested.
pub async fn fetch_events_delta(
    policy: &NetworkPolicy,
    access_token: &Secret,
    calendar_id: &str,
    delta_cursor: Option<&str>,
) -> Result<EventsFetch, SyncError> {
    let initial_url = events_delta_url(calendar_id);
    let expected_path = url::Url::parse(&initial_url)
        .map_err(|_| SyncError::UnsafePaginationUrl)?
        .path()
        .to_owned();
    let mut url = delta_cursor.map(str::to_owned).unwrap_or(initial_url);
    let mut events = Vec::new();
    let mut delta_link = None;

    for _ in 0..MAX_PAGES {
        let (status, body) = get(policy, access_token, &url, &expected_path).await?;
        if status == 410 {
            return Err(SyncError::SyncTokenExpired);
        }
        if status != 200 {
            return Err(SyncError::HttpStatus { status });
        }
        let page = parse_delta_page(&body)?;
        events.extend(page.items);
        if page.delta_link.is_some() {
            delta_link = page.delta_link;
        }
        match page.next_link {
            Some(next_url) => url = next_url,
            None => return Ok(EventsFetch { events, delta_link }),
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
        let path = "/v1.0/me/calendars/x/events/delta";
        assert!(validate_graph_url(
            "https://graph.microsoft.com/v1.0/me/calendars/x/events/delta?$skiptoken=abc",
            path
        )
        .is_ok());

        for unsafe_url in [
            "https://attacker.example/collect",
            "https://graph.microsoft.com.attacker.example/v1.0/me/calendars/x/events/delta",
            "https://graph.microsoft.com:8443/v1.0/me/calendars/x/events/delta",
            "https://graph.microsoft.com/v1.0/me/messages",
            "http://graph.microsoft.com/v1.0/me/calendars/x/events/delta",
        ] {
            assert!(matches!(
                validate_graph_url(unsafe_url, path),
                Err(SyncError::UnsafePaginationUrl)
            ));
        }
    }

    /// Modelled on
    /// <https://learn.microsoft.com/en-us/graph/api/event-delta>. UNVERIFIED
    /// against a real response -- see the module doc.
    const DELTA_FIXTURE: &str = r#"{
        "value": [
            {
                "id": "evt1",
                "subject": "Standup",
                "start": {"dateTime": "2026-01-05T09:00:00.0000000", "timeZone": "UTC"},
                "end": {"dateTime": "2026-01-05T09:15:00.0000000", "timeZone": "UTC"},
                "isAllDay": false,
                "isCancelled": false,
                "onlineMeeting": {"joinUrl": "https://teams.microsoft.com/l/meetup-join/abc"},
                "organizer": {"emailAddress": {"name": "Lead", "address": "lead@example.com"}},
                "attendees": [
                    {"emailAddress": {"name": "A", "address": "a@example.com"}, "status": {"response": "accepted"}}
                ]
            },
            {
                "id": "evt2",
                "@removed": {"reason": "deleted"}
            }
        ],
        "@odata.deltaLink": "https://graph.microsoft.com/v1.0/me/calendars/x/events/delta?$deltatoken=abc"
    }"#;

    #[test]
    fn a_timed_event_is_normalised_with_its_attendees() {
        let page = parse_delta_page(DELTA_FIXTURE).unwrap();
        assert_eq!(
            page.delta_link,
            Some(
                "https://graph.microsoft.com/v1.0/me/calendars/x/events/delta?$deltatoken=abc"
                    .to_owned()
            )
        );
        let SyncedEvent::Upsert(evt) = &page.items[0] else {
            panic!("expected an upsert");
        };
        assert_eq!(evt.id, "evt1");
        assert!(!evt.is_all_day);
        assert_eq!(
            evt.hangout_link.as_deref(),
            Some("https://teams.microsoft.com/l/meetup-join/abc")
        );
        assert_eq!(evt.organizer_email.as_deref(), Some("lead@example.com"));
        assert_eq!(evt.attendees.len(), 1);
    }

    #[test]
    fn a_removed_item_is_a_removal_not_an_upsert() {
        let page = parse_delta_page(DELTA_FIXTURE).unwrap();
        assert_eq!(
            page.items[1],
            SyncedEvent::Removed {
                id: "evt2".to_owned()
            }
        );
    }

    #[test]
    fn a_cancelled_but_not_removed_event_keeps_its_row_with_a_cancelled_status() {
        let body = r#"{"value":[{
            "id": "evt3",
            "subject": "Postponed",
            "start": {"dateTime": "2026-01-05T09:00:00.0000000"},
            "end": {"dateTime": "2026-01-05T09:15:00.0000000"},
            "isCancelled": true
        }]}"#;
        let page = parse_delta_page(body).unwrap();
        let SyncedEvent::Upsert(evt) = &page.items[0] else {
            panic!("expected an upsert");
        };
        assert_eq!(evt.status, "cancelled");
    }

    #[test]
    fn an_event_with_no_start_or_end_is_skipped_not_invented() {
        let body = r#"{"value":[{"id":"broken","subject":"no times"}]}"#;
        let page = parse_delta_page(body).unwrap();
        assert!(page.items.is_empty());
    }

    #[test]
    fn unparseable_json_is_a_malformed_response_not_a_panic() {
        assert!(matches!(
            parse_delta_page("not json"),
            Err(SyncError::MalformedResponse)
        ));
    }
}
