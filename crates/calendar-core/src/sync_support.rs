//! Shared plumbing for [`crate::google`] and [`crate::microsoft`]: the error
//! type both report, a dependency-free percent-encoder for building request
//! URLs, and the attendee shape both providers normalise into.
//!
//! A percent-encoder rather than the `percent-encoding` crate because this
//! workspace already avoids a dependency where a dozen lines of hand-written
//! code covers the actual need -- the same reasoning that kept `wasapi` and
//! `axum` out of `audio-core` and `redirect.rs`. This only ever encodes an
//! opaque id or page token as one path segment or query value, never a whole
//! URL, so the unreserved-only, always-safe-to-over-encode set is enough.

use serde::{Deserialize, Serialize};

/// Why fetching or parsing a calendar list or event page failed.
#[derive(Debug, thiserror::Error)]
pub enum SyncError {
    #[error("the network policy refused the calendar endpoint: {0}")]
    Policy(#[from] kw_network_policy::PolicyError),

    #[error("the calendar endpoint could not be reached: {0}")]
    Http(#[source] reqwest::Error),

    #[error("the calendar endpoint returned {status}")]
    HttpStatus { status: u16 },

    #[error("the calendar endpoint's response could not be parsed")]
    MalformedResponse,

    #[error("the calendar provider returned an unsafe pagination URL")]
    UnsafePaginationUrl,

    /// A page-token loop that did not terminate within
    /// [`MAX_PAGES`]. Bounded the same way
    /// `kw_network_policy::NetworkPolicy::download_to_file` bounds redirects:
    /// a page chain this long is a misconfiguration or a loop, not a calendar.
    #[error("more than {MAX_PAGES} pages were returned; refusing to keep following them")]
    TooManyPages,

    #[error("database error: {0}")]
    Storage(#[from] sqlx::Error),
}

/// Upper bound on how many pages a single list/window fetch will follow.
pub const MAX_PAGES: u8 = 25;

/// One attendee, in the shape both providers normalise into and
/// [`crate::events_repo`] stores as JSON in `calendar_events.attendees`.
///
/// One shape for both providers rather than two, so a future events view does
/// not need to know which provider an event came from to render who was
/// invited.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredAttendee {
    pub email: Option<String>,
    pub name: Option<String>,
    /// Google's `responseStatus` (`accepted`, `declined`, `tentative`,
    /// `needsAction`), or Graph's `status.response` (also including `organizer`
    /// and `notResponded`). Preserve provider response words for future readers.
    pub response_status: Option<String>,
}

/// Convert a provider timestamp to a comparable UTC instant. Offset-free
/// timestamps are safe only when the provider explicitly identifies UTC;
/// interpreting another zone without its rules would invent an instant.
#[must_use]
pub fn to_utc(date_time: &str, zone: Option<&str>) -> Option<String> {
    use chrono::{DateTime, NaiveDateTime, SecondsFormat, Utc};
    if let Ok(value) = DateTime::parse_from_rfc3339(date_time) {
        return Some(
            value
                .with_timezone(&Utc)
                .to_rfc3339_opts(SecondsFormat::Secs, true),
        );
    }
    if !zone.is_some_and(|z| z.eq_ignore_ascii_case("UTC")) {
        return None;
    }
    NaiveDateTime::parse_from_str(date_time, "%Y-%m-%dT%H:%M:%S%.f")
        .ok()
        .map(|value| value.and_utc().to_rfc3339_opts(SecondsFormat::Secs, true))
}

/// A fetched snapshot contains no writes. Invalid item times preserve the
/// corresponding cached ids, while a page error discards the whole snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct WindowSnapshot {
    pub rows: Vec<crate::events_repo::EventRow>,
    pub keep_ids: Vec<String>,
}

/// Collect a bounded page chain before handing any rows to storage. Injecting
/// transport keeps the production pagination and failure semantics testable
/// without credentials or network access.
pub(crate) async fn collect_window<F, Fut>(
    initial_url: String,
    mut fetch: F,
    mut parse: impl FnMut(&str) -> Result<(WindowSnapshot, Option<String>), SyncError>,
) -> Result<WindowSnapshot, SyncError>
where
    F: FnMut(String) -> Fut,
    Fut: std::future::Future<Output = Result<(u16, String), SyncError>>,
{
    let mut url = initial_url;
    let mut snapshot = WindowSnapshot::default();
    for _ in 0..MAX_PAGES {
        let (status, body) = fetch(url).await?;
        if status != 200 {
            return Err(SyncError::HttpStatus { status });
        }
        let (page, next) = parse(&body)?;
        snapshot.rows.extend(page.rows);
        snapshot.keep_ids.extend(page.keep_ids);
        match next {
            Some(next) => url = next,
            None => return Ok(snapshot),
        }
    }
    Err(SyncError::TooManyPages)
}

/// Percent-encode `input` for use as one path segment or query value.
///
/// Keeps only the RFC 3986 unreserved set (`ALPHA` / `DIGIT` / `-` / `.` / `_`
/// / `~`) literal; everything else, including characters that would actually
/// be safe in a given context, becomes `%XX`. Over-encoding a safe character
/// never changes what the request means, so the simpler, single rule is used
/// for both a page token in a query string and a calendar id in a path.
#[must_use]
pub fn percent_encode_path_segment(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for byte in input.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(byte as char);
            }
            _ => {
                out.push('%');
                out.push_str(&format!("{byte:02X}"));
            }
        }
    }
    out
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
    fn utc_conversion_table() {
        for (input, zone, expected) in [
            (
                "2026-11-11T09:00:00+01:00",
                None,
                Some("2026-11-11T08:00:00Z"),
            ),
            (
                "2026-11-11T09:00:00-05:00",
                None,
                Some("2026-11-11T14:00:00Z"),
            ),
            ("2026-11-11T09:00:00Z", None, Some("2026-11-11T09:00:00Z")),
            (
                "2026-11-11T09:00:00.123456789123Z",
                None,
                Some("2026-11-11T09:00:00Z"),
            ),
            (
                "2026-11-11T09:00:00.0000000",
                Some("uTc"),
                Some("2026-11-11T09:00:00Z"),
            ),
            (
                "2026-11-11T09:00:00.123456789123",
                Some("UTC"),
                Some("2026-11-11T09:00:00Z"),
            ),
            (
                "2026-11-11T09:00:00",
                Some("UTC"),
                Some("2026-11-11T09:00:00Z"),
            ),
            ("2026-11-11T09:00:00", None, None),
            ("2026-11-11T09:00:00", Some("Europe/Stockholm"), None),
            ("2026-11-11", Some("UTC"), None),
            ("garbage", Some("UTC"), None),
        ] {
            assert_eq!(to_utc(input, zone).as_deref(), expected, "{input}");
        }
    }

    #[test]
    fn unreserved_characters_pass_through_unchanged() {
        assert_eq!(
            percent_encode_path_segment("abcXYZ019-._~"),
            "abcXYZ019-._~"
        );
    }

    #[test]
    fn an_email_shaped_calendar_id_is_encoded() {
        assert_eq!(
            percent_encode_path_segment("user@example.com"),
            "user%40example.com"
        );
    }

    #[test]
    fn a_slash_is_encoded_so_it_cannot_add_a_path_segment() {
        assert_eq!(percent_encode_path_segment("a/b"), "a%2Fb");
    }

    #[test]
    fn a_microsoft_style_base64_id_with_padding_is_encoded() {
        assert_eq!(percent_encode_path_segment("AAMk=="), "AAMk%3D%3D");
    }
}
