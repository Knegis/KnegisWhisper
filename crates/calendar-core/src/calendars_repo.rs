//! Reading and writing `google_calendars` / `microsoft_calendars`
//! (`crates/storage/migrations/0001_initial.sql`), which this crate adds no
//! migration to.
//!
//! A calendar refresh must never clobber two things a user or an earlier
//! sync already decided: whether the calendar is selected for sync
//! (`is_selected`), and where the delta cursor was left (`sync_token`). Every
//! `upsert_*` function here writes only the columns the provider's own
//! calendar list describes and leaves both alone; [`set_selected`] and
//! [`set_sync_token`] are the only ways either one changes.
//!
//! Same reasoning as `crate::tokens_repo` for taking a `&SqlitePool` rather
//! than a `kw_storage::Db`, and the same note about table/column names being
//! compile-time constants rather than caller-supplied strings, which is what
//! keeps interpolating them into a query free of an injection path.

use sqlx::{Row as _, SqlitePool};

use crate::google::CalendarListEntry as GoogleCalendarListEntry;
use crate::microsoft::CalendarListEntry as MicrosoftCalendarListEntry;
use crate::provider::CalendarProviderKind;

/// One row of either calendars table, in the shape a status screen wants.
/// `description` is `None` for a Microsoft calendar -- that table has no
/// such column.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CalendarRow {
    pub id: String,
    pub summary: String,
    pub description: Option<String>,
    pub background_color: Option<String>,
    pub is_selected: bool,
    pub is_primary: bool,
    pub sync_token: Option<String>,
    pub account_email: Option<String>,
}

fn table(kind: CalendarProviderKind) -> &'static str {
    match kind {
        CalendarProviderKind::Google => "google_calendars",
        CalendarProviderKind::Microsoft => "microsoft_calendars",
    }
}

/// Insert a newly seen Google calendar, or refresh the fields Google's own
/// list describes for one already stored. `is_selected` is seeded from
/// `entry.selected` on first insert only -- see the module doc.
pub async fn upsert_google_calendar(
    pool: &SqlitePool,
    account_email: &str,
    entry: &GoogleCalendarListEntry,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO google_calendars \
            (id, summary, description, background_color, is_selected, is_primary, account_email) \
         VALUES (?, ?, ?, ?, ?, ?, ?) \
         ON CONFLICT(id) DO UPDATE SET \
            summary = excluded.summary, description = excluded.description, \
            background_color = excluded.background_color, is_primary = excluded.is_primary, \
            account_email = excluded.account_email",
    )
    .bind(&entry.id)
    .bind(&entry.summary)
    .bind(&entry.description)
    .bind(&entry.background_color)
    .bind(entry.selected)
    .bind(entry.is_primary)
    .bind(account_email)
    .execute(pool)
    .await?;
    Ok(())
}

/// Same as [`upsert_google_calendar`], for a Microsoft calendar. There is no
/// Microsoft equivalent of `selected`, so a first-seen calendar takes the
/// schema's own default (`is_selected = 1`) rather than this binding it.
pub async fn upsert_microsoft_calendar(
    pool: &SqlitePool,
    account_email: &str,
    entry: &MicrosoftCalendarListEntry,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO microsoft_calendars (id, summary, background_color, is_primary, account_email) \
         VALUES (?, ?, ?, ?, ?) \
         ON CONFLICT(id) DO UPDATE SET \
            summary = excluded.summary, background_color = excluded.background_color, \
            is_primary = excluded.is_primary, account_email = excluded.account_email",
    )
    .bind(&entry.id)
    .bind(&entry.summary)
    .bind(&entry.background_color)
    .bind(entry.is_primary)
    .bind(account_email)
    .execute(pool)
    .await?;
    Ok(())
}

/// Remove every calendar of `kind` for `account_email` whose id is not in
/// `keep_ids`, returning the ids removed so the caller can also drop their
/// events (`crate::events_repo::delete_all_for_calendar`) -- a calendar the
/// provider no longer lists should not leave orphaned events behind.
pub async fn prune_calendars(
    pool: &SqlitePool,
    kind: CalendarProviderKind,
    account_email: &str,
    keep_ids: &[String],
) -> Result<Vec<String>, sqlx::Error> {
    let table = table(kind);
    let select = format!("SELECT id FROM {table} WHERE account_email = ?");
    let rows = sqlx::query(&select)
        .bind(account_email)
        .fetch_all(pool)
        .await?;

    let mut removed = Vec::new();
    for row in rows {
        let id: String = row.try_get("id")?;
        if !keep_ids.contains(&id) {
            removed.push(id);
        }
    }

    for id in &removed {
        let delete = format!("DELETE FROM {table} WHERE id = ?");
        sqlx::query(&delete).bind(id).execute(pool).await?;
    }

    Ok(removed)
}

/// List every calendar of `kind` for `account_email`.
pub async fn list_calendars(
    pool: &SqlitePool,
    kind: CalendarProviderKind,
    account_email: &str,
) -> Result<Vec<CalendarRow>, sqlx::Error> {
    let has_description = matches!(kind, CalendarProviderKind::Google);
    let columns = if has_description {
        "id, summary, description, background_color, is_selected, is_primary, sync_token, account_email"
    } else {
        "id, summary, background_color, is_selected, is_primary, sync_token, account_email"
    };
    let query = format!(
        "SELECT {columns} FROM {} WHERE account_email = ? ORDER BY is_primary DESC, summary",
        table(kind)
    );
    let rows = sqlx::query(&query)
        .bind(account_email)
        .fetch_all(pool)
        .await?;

    rows.into_iter()
        .map(|row| {
            Ok(CalendarRow {
                id: row.try_get("id")?,
                summary: row.try_get("summary")?,
                description: if has_description {
                    row.try_get("description")?
                } else {
                    None
                },
                background_color: row.try_get("background_color")?,
                is_selected: row.try_get("is_selected")?,
                is_primary: row.try_get("is_primary")?,
                sync_token: row.try_get("sync_token")?,
                account_email: row.try_get("account_email")?,
            })
        })
        .collect()
}

/// Toggle whether a calendar is synced.
pub async fn set_selected(
    pool: &SqlitePool,
    kind: CalendarProviderKind,
    calendar_id: &str,
    selected: bool,
) -> Result<(), sqlx::Error> {
    let query = format!("UPDATE {} SET is_selected = ? WHERE id = ?", table(kind));
    sqlx::query(&query)
        .bind(selected)
        .bind(calendar_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Store the delta cursor for one calendar, or clear it (`None`) after a
/// sync token expired and a full resync ran.
///
/// `sync_token_expires_at` (Microsoft's table only) is never written here --
/// see `crate::microsoft`'s module doc on why inventing a value for it would
/// be a fabricated fact rather than a documented one.
pub async fn set_sync_token(
    pool: &SqlitePool,
    kind: CalendarProviderKind,
    calendar_id: &str,
    token: Option<&str>,
) -> Result<(), sqlx::Error> {
    let query = format!("UPDATE {} SET sync_token = ? WHERE id = ?", table(kind));
    sqlx::query(&query)
        .bind(token)
        .bind(calendar_id)
        .execute(pool)
        .await?;
    Ok(())
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

    fn google_entry(id: &str, selected: bool, primary: bool) -> GoogleCalendarListEntry {
        GoogleCalendarListEntry {
            id: id.to_owned(),
            summary: format!("{id} summary"),
            description: None,
            background_color: None,
            is_primary: primary,
            selected,
        }
    }

    #[tokio::test]
    async fn a_fresh_google_calendar_seeds_is_selected_from_the_wire_value() {
        let db = Db::open_in_memory().await.unwrap();
        upsert_google_calendar(
            db.pool(),
            "user@example.com",
            &google_entry("cal-1", false, true),
        )
        .await
        .unwrap();

        let rows = list_calendars(db.pool(), CalendarProviderKind::Google, "user@example.com")
            .await
            .unwrap();
        assert_eq!(rows.len(), 1);
        assert!(!rows[0].is_selected);
        assert!(rows[0].is_primary);
    }

    #[tokio::test]
    async fn refreshing_a_calendar_does_not_touch_is_selected_or_sync_token() {
        let db = Db::open_in_memory().await.unwrap();
        upsert_google_calendar(
            db.pool(),
            "user@example.com",
            &google_entry("cal-1", true, false),
        )
        .await
        .unwrap();
        set_selected(db.pool(), CalendarProviderKind::Google, "cal-1", false)
            .await
            .unwrap();
        set_sync_token(
            db.pool(),
            CalendarProviderKind::Google,
            "cal-1",
            Some("cursor-1"),
        )
        .await
        .unwrap();

        // A refresh reports the calendar as primary now and a new summary --
        // this must land, but the local selection and cursor must survive it.
        upsert_google_calendar(
            db.pool(),
            "user@example.com",
            &google_entry("cal-1", true, true),
        )
        .await
        .unwrap();

        let rows = list_calendars(db.pool(), CalendarProviderKind::Google, "user@example.com")
            .await
            .unwrap();
        assert!(rows[0].is_primary, "the refreshed field did update");
        assert!(
            !rows[0].is_selected,
            "the user's local selection must survive a refresh"
        );
        assert_eq!(
            rows[0].sync_token.as_deref(),
            Some("cursor-1"),
            "the delta cursor must survive a refresh"
        );
    }

    #[tokio::test]
    async fn a_calendar_no_longer_listed_is_pruned() {
        let db = Db::open_in_memory().await.unwrap();
        upsert_google_calendar(
            db.pool(),
            "user@example.com",
            &google_entry("cal-1", true, true),
        )
        .await
        .unwrap();
        upsert_google_calendar(
            db.pool(),
            "user@example.com",
            &google_entry("cal-2", true, false),
        )
        .await
        .unwrap();

        let removed = prune_calendars(
            db.pool(),
            CalendarProviderKind::Google,
            "user@example.com",
            &["cal-1".to_owned()],
        )
        .await
        .unwrap();
        assert_eq!(removed, vec!["cal-2".to_owned()]);

        let rows = list_calendars(db.pool(), CalendarProviderKind::Google, "user@example.com")
            .await
            .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, "cal-1");
    }

    #[tokio::test]
    async fn microsoft_calendars_have_no_description_and_default_to_selected() {
        let db = Db::open_in_memory().await.unwrap();
        let entry = MicrosoftCalendarListEntry {
            id: "ms-1".to_owned(),
            summary: "Calendar".to_owned(),
            background_color: None,
            is_primary: true,
            owner_email: Some("jane@example.com".to_owned()),
        };
        upsert_microsoft_calendar(db.pool(), "jane@example.com", &entry)
            .await
            .unwrap();

        let rows = list_calendars(
            db.pool(),
            CalendarProviderKind::Microsoft,
            "jane@example.com",
        )
        .await
        .unwrap();
        assert_eq!(rows[0].description, None);
        assert!(rows[0].is_selected, "the schema default is selected");
    }

    #[tokio::test]
    async fn nothing_written_here_trips_the_storage_crates_credential_scanner() {
        let db = Db::open_in_memory().await.unwrap();
        upsert_google_calendar(
            db.pool(),
            "user@example.com",
            &google_entry("cal-1", true, true),
        )
        .await
        .unwrap();
        set_sync_token(
            db.pool(),
            CalendarProviderKind::Google,
            "cal-1",
            Some("cursor-1"),
        )
        .await
        .unwrap();
        kw_storage::assert_no_credential_columns(db.pool())
            .await
            .unwrap();
    }
}
