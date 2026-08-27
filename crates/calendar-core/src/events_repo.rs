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

use sqlx::SqlitePool;

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
    .execute(pool)
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

/// Remove every event stored for one calendar. Used both when a calendar
/// disappears from the provider's own list, and after a Google `syncToken`
/// expires and a full resync has to replace an incremental history it can no
/// longer reconcile with.
///
/// Returns the number of rows removed.
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
