//! Reading and writing `calendar_events`
//! (`crates/storage/migrations/0001_initial.sql`), which this crate adds no
//! migration to.
//!
//! The table's primary key is a bare `id TEXT PRIMARY KEY` with no
//! `calendar_id` in the key -- there is nothing stopping two different
//! calendars from ever producing the same raw event id, on the same provider
//! or across providers. [`row_id`] guards against that by storing
//! `provider:calendar_id:raw_id` as the key rather than the provider's own
//! id verbatim, so a collision would have to happen inside one calendar,
//! which is the guarantee each provider actually documents.

use chrono::{DateTime, NaiveDate, Utc};
use sqlx::{Executor, Row as _, Sqlite, SqlitePool};
use std::collections::HashSet;

use crate::sync_support::StoredAttendee;

/// One event as it is written to `calendar_events`. `id` is already the
/// composite key -- see [`row_id`] -- not the provider's own event id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventRow {
    pub id: String,
    pub calendar_id: String,
    pub provider: &'static str,
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

/// The composite primary key one event is stored under.
#[must_use]
pub fn row_id(provider: &str, calendar_id: &str, raw_event_id: &str) -> String {
    format!("{provider}:{calendar_id}:{raw_event_id}")
}

/// Insert a newly seen event, or overwrite every column of one already
/// stored -- unlike a calendar row, an event has no local-only field to
/// protect, so a refresh may freely replace it.
pub async fn upsert_event(pool: &SqlitePool, row: &EventRow) -> Result<(), sqlx::Error> {
    upsert_on(pool, row).await
}

async fn upsert_on<'e>(
    executor: impl Executor<'e, Database = Sqlite>,
    row: &EventRow,
) -> Result<(), sqlx::Error> {
    let attendees_json = if row.attendees.is_empty() {
        None
    } else {
        serde_json::to_string(&row.attendees).ok()
    };

    sqlx::query(
        "INSERT INTO calendar_events \
            (id, calendar_id, provider, summary, start_time, end_time, is_all_day, status, \
             hangout_link, conference_data, organizer_email, attendees_count, attendees) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) \
         ON CONFLICT(id) DO UPDATE SET \
            calendar_id = excluded.calendar_id, provider = excluded.provider, \
            summary = excluded.summary, start_time = excluded.start_time, \
            end_time = excluded.end_time, is_all_day = excluded.is_all_day, \
            status = excluded.status, hangout_link = excluded.hangout_link, \
            conference_data = excluded.conference_data, \
            organizer_email = excluded.organizer_email, \
            attendees_count = excluded.attendees_count, attendees = excluded.attendees, \
            synced_at = CURRENT_TIMESTAMP",
    )
    .bind(&row.id)
    .bind(&row.calendar_id)
    .bind(row.provider)
    .bind(&row.summary)
    .bind(&row.start_time)
    .bind(&row.end_time)
    .bind(row.is_all_day)
    .bind(&row.status)
    .bind(&row.hangout_link)
    .bind(&row.conference_data)
    .bind(&row.organizer_email)
    .bind(i64::try_from(row.attendees.len()).unwrap_or(i64::MAX))
    .bind(attendees_json)
    .execute(executor)
    .await?;
    Ok(())
}

/// Remove one event by its composite id -- see [`row_id`].
pub async fn delete_event(pool: &SqlitePool, id: &str) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM calendar_events WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Remove every event stored for one calendar, returning the rows removed.
/// Calendar-list pruning performs this inside its own transaction as well.
pub async fn delete_all_for_calendar(
    pool: &SqlitePool,
    provider: &str,
    calendar_id: &str,
) -> Result<u64, sqlx::Error> {
    let result = sqlx::query("DELETE FROM calendar_events WHERE provider = ? AND calendar_id = ?")
        .bind(provider)
        .bind(calendar_id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected())
}

/// Purge pre-snapshot data for an account, including unselected calendars and
/// events outside the window. Keep SQL here because the desktop crate only
/// depends on SQLx for tests; no new application dependency is needed for the
/// version gate. The marker is written by the caller after this commits.
pub async fn purge_account(
    pool: &SqlitePool,
    kind: crate::provider::CalendarProviderKind,
    account_email: &str,
) -> Result<(), sqlx::Error> {
    use crate::provider::CalendarProviderKind;
    let (table, provider) = match kind {
        CalendarProviderKind::Google => ("google_calendars", "google"),
        CalendarProviderKind::Microsoft => ("microsoft_calendars", "microsoft"),
    };
    let mut tx = pool.begin().await?;
    sqlx::query(&format!("DELETE FROM calendar_events WHERE provider = ? AND calendar_id IN (SELECT id FROM {table} WHERE account_email = ?)"))
        .bind(provider).bind(account_email).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(())
}

/// How many events are stored for one calendar -- the screen's "N events"
/// summary. Deliberately a count of what sync actually wrote, not the
/// provider's own total, so it never overstates what a partial or failed
/// sync produced.
pub async fn count_events(
    pool: &SqlitePool,
    provider: &str,
    calendar_id: &str,
) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT count(*) FROM calendar_events WHERE provider = ? AND calendar_id = ?",
    )
    .bind(provider)
    .bind(calendar_id)
    .fetch_one(pool)
    .await
}

/// Interpret stored times consistently for pruning and future readers. Dates
/// are midnight UTC; RFC 3339 offsets are honoured even for legacy rows.
#[must_use]
pub fn parse_row_instant(value: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|t| t.with_timezone(&Utc))
        .or_else(|| {
            NaiveDate::parse_from_str(value, "%Y-%m-%d")
                .ok()?
                .and_hms_opt(0, 0, 0)
                .map(|t| t.and_utc())
        })
}

/// Replace one calendar's complete window atomically. Day-prefix filtering is
/// deliberately wider than the window because old offset timestamps can name
/// a neighbouring UTC day. Exact instant overlap determines which rows go;
/// unreadable legacy times within the prefilter are discarded as stale data.
/// Rows protected by an item-level time error are never pruned.
pub async fn replace_window(
    pool: &SqlitePool,
    provider: &str,
    calendar_id: &str,
    start: &DateTime<Utc>,
    end: &DateTime<Utc>,
    rows: &[EventRow],
    keep_ids: &[String],
) -> Result<u64, sqlx::Error> {
    let mut tx = pool.begin().await?;
    for row in rows {
        upsert_on(&mut *tx, row).await?;
    }
    let protected: HashSet<&str> = rows
        .iter()
        .map(|r| r.id.as_str())
        .chain(keep_ids.iter().map(String::as_str))
        .collect();
    let candidates = sqlx::query("SELECT id, start_time, end_time FROM calendar_events WHERE provider = ? AND calendar_id = ? AND substr(start_time,1,10) <= ? AND substr(end_time,1,10) >= ?")
        .bind(provider).bind(calendar_id)
        .bind((*end + chrono::Duration::days(1)).format("%Y-%m-%d").to_string())
        .bind((*start - chrono::Duration::days(1)).format("%Y-%m-%d").to_string())
        .fetch_all(&mut *tx).await?;
    let mut removed = 0;
    for row in candidates {
        let id: String = row.try_get("id")?;
        if protected.contains(id.as_str()) {
            continue;
        }
        let row_start: String = row.try_get("start_time")?;
        let row_end: String = row.try_get("end_time")?;
        let overlaps = match (parse_row_instant(&row_start), parse_row_instant(&row_end)) {
            (Some(a), Some(b)) => a < *end && b > *start,
            _ => true,
        };
        if overlaps {
            removed += sqlx::query("DELETE FROM calendar_events WHERE id = ?")
                .bind(id)
                .execute(&mut *tx)
                .await?
                .rows_affected();
        }
    }
    tx.commit().await?;
    Ok(removed)
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
    use kw_storage::Db;

    #[test]
    fn stored_time_parser_uses_midnight_utc_for_dates_and_honours_offsets() {
        assert_eq!(
            parse_row_instant("2026-11-11"),
            parse_row_instant("2026-11-11T00:00:00Z")
        );
        assert_eq!(
            parse_row_instant("2026-11-11T01:00:00+01:00"),
            parse_row_instant("2026-11-11")
        );
        assert!(parse_row_instant("2026-11-11T09:00:00").is_none());
        assert!(parse_row_instant("garbage").is_none());
    }

    #[tokio::test]
    async fn window_pruning_uses_exact_overlap_and_protects_returned_and_unreadable_items() {
        let db = Db::open_in_memory().await.unwrap();
        let start = parse_row_instant("2026-11-11T12:00:00Z").unwrap();
        let end = parse_row_instant("2026-12-11T12:00:00Z").unwrap();
        for (id, a, b) in [
            ("long", "2026-01-01T00:00:00Z", "2026-12-01T00:00:00Z"),
            ("inside", "2026-11-12T09:00:00Z", "2026-11-12T10:00:00Z"),
            ("date", "2026-11-11", "2026-11-12"),
            ("keep", "2026-11-12T09:00:00Z", "2026-11-12T10:00:00Z"),
            ("returned", "2026-11-12T09:00:00Z", "2026-11-12T10:00:00Z"),
            ("before", "2026-10-01T00:00:00Z", "2026-11-11T12:00:00Z"),
            ("after", "2026-12-11T12:00:00Z", "2026-12-12T00:00:00Z"),
            (
                "offset-outside",
                "2026-11-11T11:00:00+01:00",
                "2026-11-11T12:00:00+01:00",
            ),
            (
                "offset-inside",
                "2026-11-11T08:00:00-05:00",
                "2026-11-11T09:00:00-05:00",
            ),
            ("legacy", "2026-11-12T09:00:00", "2026-11-12T10:00:00"),
        ] {
            let mut r = row(id, "cal-1");
            r.start_time = a.to_owned();
            r.end_time = b.to_owned();
            upsert_event(db.pool(), &r).await.unwrap();
        }
        let mut other = row("other", "cal-2");
        other.start_time = "2026-11-12".to_owned();
        other.end_time = "2026-11-13".to_owned();
        upsert_event(db.pool(), &other).await.unwrap();
        let mut provider = other.clone();
        provider.provider = "microsoft";
        provider.calendar_id = "cal-1".to_owned();
        provider.id = row_id("microsoft", "cal-1", "other");
        upsert_event(db.pool(), &provider).await.unwrap();
        let mut returned = row("returned", "cal-1");
        returned.summary = Some("updated".to_owned());
        returned.start_time = "2026-11-12T09:00:00Z".to_owned();
        returned.end_time = "2026-11-12T10:00:00Z".to_owned();
        let removed = replace_window(
            db.pool(),
            "google",
            "cal-1",
            &start,
            &end,
            &[returned],
            &[row_id("google", "cal-1", "keep")],
        )
        .await
        .unwrap();
        assert_eq!(removed, 5);
        let ids: Vec<String> = sqlx::query_scalar("SELECT id FROM calendar_events ORDER BY id")
            .fetch_all(db.pool())
            .await
            .unwrap();
        assert_eq!(
            ids,
            [
                "google:cal-1:after",
                "google:cal-1:before",
                "google:cal-1:keep",
                "google:cal-1:offset-outside",
                "google:cal-1:returned",
                "google:cal-2:other",
                "microsoft:cal-1:other"
            ]
        );
        let title: String = sqlx::query_scalar(
            "SELECT summary FROM calendar_events WHERE id = 'google:cal-1:returned'",
        )
        .fetch_one(db.pool())
        .await
        .unwrap();
        assert_eq!(title, "updated");
    }

    #[tokio::test]
    async fn a_pruning_failure_rolls_back_all_upserts() {
        let db = Db::open_in_memory().await.unwrap();
        let start = parse_row_instant("2026-01-01").unwrap();
        let end = parse_row_instant("2026-02-01").unwrap();
        upsert_event(db.pool(), &row("old", "cal-1")).await.unwrap();
        sqlx::query("CREATE TRIGGER refuse_prune BEFORE DELETE ON calendar_events BEGIN SELECT RAISE(ABORT, 'fixture'); END").execute(db.pool()).await.unwrap();
        assert!(replace_window(
            db.pool(),
            "google",
            "cal-1",
            &start,
            &end,
            &[row("new", "cal-1")],
            &[]
        )
        .await
        .is_err());
        let ids: Vec<String> = sqlx::query_scalar("SELECT id FROM calendar_events")
            .fetch_all(db.pool())
            .await
            .unwrap();
        assert_eq!(ids, ["google:cal-1:old"]);
    }

    fn row(id: &str, calendar_id: &str) -> EventRow {
        EventRow {
            id: row_id("google", calendar_id, id),
            calendar_id: calendar_id.to_owned(),
            provider: "google",
            summary: Some("Standup".to_owned()),
            start_time: "2026-01-05T09:00:00Z".to_owned(),
            end_time: "2026-01-05T09:15:00Z".to_owned(),
            is_all_day: false,
            status: "confirmed".to_owned(),
            hangout_link: None,
            conference_data: None,
            organizer_email: None,
            attendees: Vec::new(),
        }
    }

    #[test]
    fn the_composite_id_keeps_two_calendars_events_apart() {
        // The defect this guards against: the table's own primary key is a
        // bare `id`, so two calendars whose provider happened to assign the
        // same raw event id would collide without this.
        assert_ne!(
            row_id("google", "cal-a", "evt-1"),
            row_id("google", "cal-b", "evt-1")
        );
    }

    #[tokio::test]
    async fn an_event_round_trips_and_counts() {
        let db = Db::open_in_memory().await.unwrap();
        upsert_event(db.pool(), &row("evt-1", "cal-1"))
            .await
            .unwrap();
        assert_eq!(count_events(db.pool(), "google", "cal-1").await.unwrap(), 1);
    }

    #[tokio::test]
    async fn upserting_the_same_event_twice_replaces_rather_than_duplicates() {
        let db = Db::open_in_memory().await.unwrap();
        let mut r = row("evt-1", "cal-1");
        upsert_event(db.pool(), &r).await.unwrap();
        r.summary = Some("Standup (moved)".to_owned());
        upsert_event(db.pool(), &r).await.unwrap();
        assert_eq!(count_events(db.pool(), "google", "cal-1").await.unwrap(), 1);
    }

    #[tokio::test]
    async fn deleting_an_event_removes_only_that_one() {
        let db = Db::open_in_memory().await.unwrap();
        upsert_event(db.pool(), &row("evt-1", "cal-1"))
            .await
            .unwrap();
        upsert_event(db.pool(), &row("evt-2", "cal-1"))
            .await
            .unwrap();
        delete_event(db.pool(), &row_id("google", "cal-1", "evt-1"))
            .await
            .unwrap();
        assert_eq!(count_events(db.pool(), "google", "cal-1").await.unwrap(), 1);
    }

    #[tokio::test]
    async fn deleting_all_for_a_calendar_leaves_other_calendars_alone() {
        let db = Db::open_in_memory().await.unwrap();
        upsert_event(db.pool(), &row("evt-1", "cal-1"))
            .await
            .unwrap();
        upsert_event(db.pool(), &row("evt-2", "cal-2"))
            .await
            .unwrap();

        let removed = delete_all_for_calendar(db.pool(), "google", "cal-1")
            .await
            .unwrap();
        assert_eq!(removed, 1);
        assert_eq!(count_events(db.pool(), "google", "cal-1").await.unwrap(), 0);
        assert_eq!(count_events(db.pool(), "google", "cal-2").await.unwrap(), 1);
    }

    #[tokio::test]
    async fn attendees_are_stored_as_json_only_when_there_are_any() {
        let db = Db::open_in_memory().await.unwrap();
        let mut r = row("evt-1", "cal-1");
        r.attendees.push(StoredAttendee {
            email: Some("a@example.com".to_owned()),
            name: None,
            response_status: Some("accepted".to_owned()),
        });
        upsert_event(db.pool(), &r).await.unwrap();

        let stored: (i64, Option<String>) =
            sqlx::query_as("SELECT attendees_count, attendees FROM calendar_events WHERE id = ?")
                .bind(&r.id)
                .fetch_one(db.pool())
                .await
                .unwrap();
        assert_eq!(stored.0, 1);
        assert!(stored.1.unwrap().contains("a@example.com"));
    }

    #[tokio::test]
    async fn nothing_written_here_trips_the_storage_crates_credential_scanner() {
        let db = Db::open_in_memory().await.unwrap();
        upsert_event(db.pool(), &row("evt-1", "cal-1"))
            .await
            .unwrap();
        kw_storage::assert_no_credential_columns(db.pool())
            .await
            .unwrap();
    }
}

/// Read a local snapshot for selected calendars of one provider account.
/// Day prefixes only narrow the SQL scan: widening by a day on either side
/// keeps legacy offset timestamps eligible, and the shared parser then applies
/// exact half-open overlap. Unreadable candidate intervals are counted once
/// per row so callers can report incomplete local data without guessing times.
/// This reader intentionally leaves cancellation and meeting eligibility to
/// the selection rules; all-day events retain their exclusive stored end date.
/// The caller is responsible for checking the snapshot version before reading.
pub async fn list_overlapping(
    pool: &SqlitePool,
    kind: crate::provider::CalendarProviderKind,
    account_email: &str,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
) -> Result<(Vec<EventRow>, u32), sqlx::Error> {
    use crate::provider::CalendarProviderKind;
    let (table, provider) = match kind {
        CalendarProviderKind::Google => ("google_calendars", "google"),
        CalendarProviderKind::Microsoft => ("microsoft_calendars", "microsoft"),
    };
    let query = format!(
        "SELECT e.* FROM calendar_events e JOIN {table} c ON c.id = e.calendar_id \
         WHERE e.provider = ? AND c.account_email = ? AND c.is_selected = 1 \
         AND substr(e.start_time,1,10) <= ? AND substr(e.end_time,1,10) >= ?"
    );
    let candidates = sqlx::query(&query)
        .bind(provider)
        .bind(account_email)
        .bind(
            to.checked_add_signed(chrono::Duration::days(1))
                .unwrap_or(DateTime::<Utc>::MAX_UTC)
                .format("%Y-%m-%d")
                .to_string(),
        )
        .bind(
            from.checked_sub_signed(chrono::Duration::days(1))
                .unwrap_or(DateTime::<Utc>::MIN_UTC)
                .format("%Y-%m-%d")
                .to_string(),
        )
        .fetch_all(pool)
        .await?;
    let mut events = Vec::new();
    let mut unreadable = 0_u32;
    for row in candidates {
        let start_time: String = row.try_get("start_time")?;
        let end_time: String = row.try_get("end_time")?;
        let (Some(start), Some(end)) =
            (parse_row_instant(&start_time), parse_row_instant(&end_time))
        else {
            unreadable = unreadable.saturating_add(1);
            continue;
        };
        if start >= to || end <= from {
            continue;
        }
        let attendees_json: Option<String> = row.try_get("attendees")?;
        let attendees = attendees_json
            .map(|json| serde_json::from_str(&json))
            .transpose()
            .map_err(|source| sqlx::Error::ColumnDecode {
                index: "attendees".to_owned(),
                source: Box::new(source),
            })?
            .unwrap_or_default();
        events.push(EventRow {
            id: row.try_get("id")?,
            calendar_id: row.try_get("calendar_id")?,
            provider,
            summary: row.try_get("summary")?,
            start_time,
            end_time,
            is_all_day: row.try_get("is_all_day")?,
            status: row.try_get("status")?,
            hangout_link: row.try_get("hangout_link")?,
            conference_data: row.try_get("conference_data")?,
            organizer_email: row.try_get("organizer_email")?,
            attendees,
        });
    }
    Ok((events, unreadable))
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod overlap_tests {
    use super::*;
    use crate::provider::CalendarProviderKind;
    use kw_storage::Db;

    fn event(provider: &'static str, calendar: &str, id: &str, start: &str, end: &str) -> EventRow {
        EventRow {
            id: row_id(provider, calendar, id),
            calendar_id: calendar.to_owned(),
            provider,
            summary: Some("Meeting".to_owned()),
            start_time: start.to_owned(),
            end_time: end.to_owned(),
            is_all_day: false,
            status: "confirmed".to_owned(),
            hangout_link: Some("https://meet.example/test".to_owned()),
            conference_data: Some("{}".to_owned()),
            organizer_email: Some("owner@example.com".to_owned()),
            attendees: vec![StoredAttendee {
                email: Some("guest@example.com".to_owned()),
                name: Some("Guest".to_owned()),
                response_status: Some("accepted".to_owned()),
            }],
        }
    }

    async fn calendar(pool: &SqlitePool, table: &str, id: &str, account: &str, selected: bool) {
        sqlx::query(&format!("INSERT INTO {table} (id, summary, account_email, is_selected) VALUES (?, 'Calendar', ?, ?)"))
            .bind(id).bind(account).bind(selected).execute(pool).await.unwrap();
    }

    #[tokio::test]
    async fn only_selected_calendars_of_the_requested_provider_account_are_read() {
        let db = Db::open_in_memory().await.unwrap();
        let from = parse_row_instant("2026-11-11T12:00:00Z").unwrap();
        let to = parse_row_instant("2026-11-11T13:00:00Z").unwrap();
        for table in ["google_calendars", "microsoft_calendars"] {
            for (id, account, selected) in [
                ("selected", "user@example.com", true),
                ("deselected", "user@example.com", false),
                ("other", "other@example.com", true),
            ] {
                calendar(db.pool(), table, id, account, selected).await;
            }
        }
        for provider in ["google", "microsoft"] {
            for id in ["selected", "deselected", "other", "orphan"] {
                let row = event(
                    provider,
                    id,
                    "event",
                    "2026-11-11T12:00:00Z",
                    "2026-11-11T13:00:00Z",
                );
                upsert_event(db.pool(), &row).await.unwrap();
                let mut bad = row.clone();
                bad.id = row_id(provider, id, "bad");
                bad.start_time = "2026-11-11-invalid".to_owned();
                upsert_event(db.pool(), &bad).await.unwrap();
            }
        }
        for (kind, provider) in [
            (CalendarProviderKind::Google, "google"),
            (CalendarProviderKind::Microsoft, "microsoft"),
        ] {
            let (rows, unreadable) =
                list_overlapping(db.pool(), kind, "user@example.com", from, to)
                    .await
                    .unwrap();
            assert_eq!(unreadable, 1, "only the selected calendar's bad row counts");
            assert_eq!(
                rows,
                [event(
                    provider,
                    "selected",
                    "event",
                    "2026-11-11T12:00:00Z",
                    "2026-11-11T13:00:00Z"
                )]
            );
            assert_eq!(
                list_overlapping(db.pool(), kind, "missing@example.com", from, to)
                    .await
                    .unwrap(),
                (vec![], 0)
            );
        }
    }

    #[tokio::test]
    async fn exact_overlap_table_includes_long_events_and_neighbouring_offset_days() {
        let db = Db::open_in_memory().await.unwrap();
        let from = parse_row_instant("2026-11-11T00:00:00Z").unwrap();
        let to = parse_row_instant("2026-11-12T00:00:00Z").unwrap();
        for (kind, provider, table) in [
            (CalendarProviderKind::Google, "google", "google_calendars"),
            (
                CalendarProviderKind::Microsoft,
                "microsoft",
                "microsoft_calendars",
            ),
        ] {
            calendar(db.pool(), table, "cal", "user@example.com", true).await;
            let mut expected = Vec::new();
            for (id, start, end, included) in [
                (
                    "ends-at-from",
                    "2026-11-10T23:00:00Z",
                    "2026-11-11T00:00:00Z",
                    false,
                ),
                (
                    "starts-at-to",
                    "2026-11-12T00:00:00Z",
                    "2026-11-12T01:00:00Z",
                    false,
                ),
                (
                    "starts-at-from",
                    "2026-11-11T00:00:00Z",
                    "2026-11-11T01:00:00Z",
                    true,
                ),
                (
                    "ends-at-to",
                    "2026-11-11T23:00:00Z",
                    "2026-11-12T00:00:00Z",
                    true,
                ),
                ("long", "2026-01-01T00:00:00Z", "2026-12-01T00:00:00Z", true),
                (
                    "offset-previous-day",
                    "2026-11-10T23:00:00-02:00",
                    "2026-11-10T23:30:00-02:00",
                    true,
                ),
                (
                    "offset-next-day",
                    "2026-11-12T01:00:00+02:00",
                    "2026-11-12T01:30:00+02:00",
                    true,
                ),
                (
                    "offset-outside",
                    "2026-11-11T00:00:00+02:00",
                    "2026-11-11T01:00:00+02:00",
                    false,
                ),
                ("all-day", "2026-11-11", "2026-11-12", true),
                ("all-day-ended", "2026-11-10", "2026-11-11", false),
                (
                    "bad-start",
                    "2026-11-11T01:00:00",
                    "2026-11-11T02:00:00Z",
                    false,
                ),
                (
                    "bad-end",
                    "2026-11-11T01:00:00Z",
                    "2026-11-11-invalid",
                    false,
                ),
                (
                    "bad-both",
                    "2026-11-11-invalid",
                    "2026-11-11-invalid",
                    false,
                ),
            ] {
                let mut row = event(provider, "cal", id, start, end);
                if id.starts_with("all-day") {
                    row.is_all_day = true;
                    row.attendees.clear();
                }
                if id == "ends-at-to" {
                    row.status = "cancelled".to_owned();
                }
                upsert_event(db.pool(), &row).await.unwrap();
                if included {
                    expected.push(row);
                }
            }
            let (mut actual, unreadable) =
                list_overlapping(db.pool(), kind, "user@example.com", from, to)
                    .await
                    .unwrap();
            actual.sort_by(|a, b| a.id.cmp(&b.id));
            expected.sort_by(|a, b| a.id.cmp(&b.id));
            assert_eq!(actual, expected);
            assert_eq!(unreadable, 3);
        }
    }
}
