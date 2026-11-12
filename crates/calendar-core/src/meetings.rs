//! Local selection rules shared by the Meetings page and recording linking.
//! These functions only inspect the supplied snapshot. In particular, they
//! preserve every overlapping candidate so the caller can ask the person to
//! choose; ordering is for presentation and never breaks a linking tie.

use chrono::{DateTime, Duration, Utc};

use crate::events_repo::{parse_row_instant, EventRow};

/// Invitations or conferencing metadata make an event meeting-like. Presence
/// is sufficient: provider-specific conference contents need no interpretation
/// here, and an explicit choice may still link an ordinary solo calendar block.
#[must_use]
pub fn is_meeting_like(row: &EventRow) -> bool {
    !row.attendees.is_empty() || row.hangout_link.is_some() || row.conference_data.is_some()
}

/// Include ongoing meetings and meetings starting within the next seven days.
/// The horizon and event end are exclusive. Apply the cap after filtering and
/// ordering so `has_more` describes eligible rows, including when the cap is zero.
#[must_use]
pub fn upcoming(now: DateTime<Utc>, rows: &[EventRow], limit: usize) -> (Vec<EventRow>, bool) {
    let horizon = now
        .checked_add_signed(Duration::days(7))
        .unwrap_or(DateTime::<Utc>::MAX_UTC);
    let mut matches = select(rows, |row, start, end| {
        is_meeting_like(row) && end > now && start < horizon
    });
    let has_more = matches.len() > limit;
    matches.truncate(limit);
    (matches, has_more)
}

/// Candidates for a person's explicit selection at recording Start. A meeting
/// may begin up to ten minutes later, inclusively; one that has ended cannot
/// match. There is no cap or meeting-likeness requirement for explicit choices.
#[must_use]
pub fn time_matches(at: DateTime<Utc>, rows: &[EventRow]) -> Vec<EventRow> {
    let lead = at
        .checked_add_signed(Duration::minutes(10))
        .unwrap_or(DateTime::<Utc>::MAX_UTC);
    select(rows, |_, start, end| start <= lead && end > at)
}

/// Automatic-link candidates retain only meeting-like time matches. The caller
/// may link only when exactly one remains; this function deliberately returns
/// all overlaps instead of guessing which invitation the person intended.
#[must_use]
pub fn link_matches(at: DateTime<Utc>, rows: &[EventRow]) -> Vec<EventRow> {
    time_matches(at, rows)
        .into_iter()
        .filter(is_meeting_like)
        .collect()
}

fn select(
    rows: &[EventRow],
    keep: impl Fn(&EventRow, DateTime<Utc>, DateTime<Utc>) -> bool,
) -> Vec<EventRow> {
    let mut matches: Vec<_> = rows
        .iter()
        .filter_map(|row| {
            if row.is_all_day || row.status == "cancelled" {
                return None;
            }
            let start = parse_row_instant(&row.start_time)?;
            let end = parse_row_instant(&row.end_time)?;
            keep(row, start, end).then_some((start, row))
        })
        .collect();
    matches
        .sort_by(|(a_start, a), (b_start, b)| a_start.cmp(b_start).then_with(|| a.id.cmp(&b.id)));
    matches.into_iter().map(|(_, row)| row.clone()).collect()
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
    use crate::sync_support::StoredAttendee;

    fn instant(value: &str) -> DateTime<Utc> {
        parse_row_instant(value).unwrap()
    }

    fn row(id: &str, start: &str, end: &str) -> EventRow {
        EventRow {
            id: id.to_owned(),
            calendar_id: "cal".to_owned(),
            provider: "google",
            summary: None,
            start_time: start.to_owned(),
            end_time: end.to_owned(),
            is_all_day: false,
            status: "confirmed".to_owned(),
            hangout_link: Some("https://meet.example/test".to_owned()),
            conference_data: None,
            organizer_email: None,
            attendees: Vec::new(),
        }
    }

    #[test]
    fn meeting_likeness_uses_each_presence_signal() {
        for (attendee, hangout, conference, expected) in [
            (false, false, false, false),
            (true, false, false, true),
            (false, true, false, true),
            (false, false, true, true),
        ] {
            let mut r = row("test", "", "");
            r.hangout_link = hangout.then(String::new);
            r.conference_data = conference.then(String::new);
            if attendee {
                r.attendees.push(StoredAttendee {
                    email: None,
                    name: None,
                    response_status: None,
                });
            }
            assert_eq!(is_meeting_like(&r), expected);
        }
    }

    #[test]
    fn selection_boundaries_and_exclusions_table() {
        let now = instant("2026-11-11T12:00:00Z");
        for (id, start, end, all_day, cancelled, solo, upcoming_expected, time_expected) in [
            (
                "at-now",
                "2026-11-11T12:00:00Z",
                "2026-11-11T13:00:00Z",
                false,
                false,
                false,
                true,
                true,
            ),
            (
                "end-now",
                "2026-11-11T11:00:00Z",
                "2026-11-11T12:00:00Z",
                false,
                false,
                false,
                false,
                false,
            ),
            (
                "ongoing",
                "2026-11-09T12:00:00Z",
                "2026-11-12T12:00:00Z",
                false,
                false,
                false,
                true,
                true,
            ),
            (
                "horizon",
                "2026-11-18T12:00:00Z",
                "2026-11-18T13:00:00Z",
                false,
                false,
                false,
                false,
                false,
            ),
            (
                "before-horizon",
                "2026-11-18T11:59:59Z",
                "2026-11-18T13:00:00Z",
                false,
                false,
                false,
                true,
                false,
            ),
            (
                "lead",
                "2026-11-11T12:10:00Z",
                "2026-11-11T13:00:00Z",
                false,
                false,
                false,
                true,
                true,
            ),
            (
                "after-lead",
                "2026-11-11T12:10:01Z",
                "2026-11-11T13:00:00Z",
                false,
                false,
                false,
                true,
                false,
            ),
            (
                "all-day",
                "2026-11-11",
                "2026-11-12",
                true,
                false,
                false,
                false,
                false,
            ),
            (
                "cancelled",
                "2026-11-11T12:00:00Z",
                "2026-11-11T13:00:00Z",
                false,
                true,
                false,
                false,
                false,
            ),
            (
                "solo",
                "2026-11-11T12:00:00Z",
                "2026-11-11T13:00:00Z",
                false,
                false,
                true,
                false,
                true,
            ),
            (
                "bad-start",
                "2026-11-11T12:00:00",
                "2026-11-11T13:00:00Z",
                false,
                false,
                false,
                false,
                false,
            ),
            (
                "bad-end",
                "2026-11-11T12:00:00Z",
                "garbage",
                false,
                false,
                false,
                false,
                false,
            ),
            (
                "offset",
                "2026-11-11T13:10:00+01:00",
                "2026-11-11T14:00:00+01:00",
                false,
                false,
                false,
                true,
                true,
            ),
        ] {
            let mut r = row(id, start, end);
            r.is_all_day = all_day;
            if cancelled {
                r.status = "cancelled".to_owned();
            }
            if solo {
                r.hangout_link = None;
            }
            let rows = [r];
            assert_eq!(
                upcoming(now, &rows, 5).0.len(),
                usize::from(upcoming_expected),
                "upcoming {id}"
            );
            assert_eq!(
                time_matches(now, &rows).len(),
                usize::from(time_expected),
                "time {id}"
            );
            assert_eq!(
                link_matches(now, &rows).len(),
                usize::from(time_expected && !solo),
                "link {id}"
            );
        }
    }

    #[test]
    fn sorting_caps_and_overlaps_preserve_every_candidate() {
        let now = instant("2026-11-11T12:00:00Z");
        let rows = [
            row("b", "2026-11-11T13:00:00+01:00", "2026-11-11T14:00:00Z"),
            row("a", "2026-11-11T12:00:00Z", "2026-11-11T14:00:00Z"),
            row("earlier", "2026-11-11T11:00:00Z", "2026-11-11T14:00:00Z"),
        ];
        for (limit, expected, has_more) in [
            (0, vec![], true),
            (1, vec!["earlier"], true),
            (2, vec!["earlier", "a"], true),
            (3, vec!["earlier", "a", "b"], false),
            (5, vec!["earlier", "a", "b"], false),
        ] {
            let (actual, more) = upcoming(now, &rows, limit);
            assert_eq!(
                actual.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(),
                expected
            );
            assert_eq!(more, has_more);
        }
        for actual in [time_matches(now, &rows), link_matches(now, &rows)] {
            assert_eq!(
                actual.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(),
                ["earlier", "a", "b"]
            );
        }
        assert_eq!(upcoming(now, &[], 0), (vec![], false));
    }
}
