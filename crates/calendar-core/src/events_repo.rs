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
/// version gate. The caller uses `purge_account_on` to commit the marker with the purge.
pub async fn purge_account(
    pool: &SqlitePool,
    kind: crate::provider::CalendarProviderKind,
    account_email: &str,
) -> Result<(), sqlx::Error> {
    purge_account_on(pool, kind, account_email).await
}

/// Executor variant so the version marker commits with the purge.
pub async fn purge_account_on<'e>(
    executor: impl Executor<'e, Database = Sqlite>,
    kind: crate::provider::CalendarProviderKind,
    account_email: &str,
) -> Result<(), sqlx::Error> {
    use crate::provider::CalendarProviderKind;
    let (table, provider) = match kind {
        CalendarProviderKind::Google => ("google_calendars", "google"),
        CalendarProviderKind::Microsoft => ("microsoft_calendars", "microsoft"),
    };
    sqlx::query(&format!("DELETE FROM calendar_events WHERE provider = ? AND calendar_id IN (SELECT id FROM {table} WHERE account_email = ?)"))
        .bind(provider).bind(account_email).execute(executor).await?;
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
