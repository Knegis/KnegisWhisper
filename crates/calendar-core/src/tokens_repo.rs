//! Reading and writing the `token_ref`, `expires_at`, and `scope` columns of
//! `google_calendar_tokens` / `microsoft_calendar_tokens`.
//!
//! The tables, and the `token_ref` column, already exist
//! (`crates/storage/migrations/0001_initial.sql`) -- this crate adds no
//! migration. This module touches nothing else in those tables, and in
//! particular never writes a column that could hold a secret: `token_ref` is
//! a slot name, not a token. `kw_storage::assert_no_credential_columns` is
//! the backstop that would catch a regression here, not the reason this is
//! safe -- the reason is that no query in this file ever binds a token or a
//! verifier as a value.
//!
//! Callers pass a `&SqlitePool` rather than a `kw_storage::Db`, so this crate
//! stays decoupled from that wrapper; whoever holds the open database (the
//! Tauri command layer, in the application) passes its pool straight through.

use sqlx::{Row as _, SqlitePool};

use crate::provider::CalendarProviderKind;

/// One row of a `*_calendar_tokens` table, minus the account email.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TokenRow {
    pub token_ref: String,
    pub expires_at: i64,
    pub scope: String,
}

/// Insert or update the row for `account_email`.
pub async fn upsert(
    pool: &SqlitePool,
    kind: CalendarProviderKind,
    account_email: &str,
    token_ref: &str,
    expires_at: i64,
    scope: &str,
) -> Result<(), sqlx::Error> {
    let query = match kind {
        CalendarProviderKind::Google => {
            "INSERT INTO google_calendar_tokens (google_email, token_ref, expires_at, scope) \
             VALUES (?, ?, ?, ?) \
             ON CONFLICT(google_email) DO UPDATE SET \
                token_ref = excluded.token_ref, expires_at = excluded.expires_at, \
                scope = excluded.scope, updated_at = CURRENT_TIMESTAMP"
        }
        CalendarProviderKind::Microsoft => {
            "INSERT INTO microsoft_calendar_tokens (microsoft_email, token_ref, expires_at, scope) \
             VALUES (?, ?, ?, ?) \
             ON CONFLICT(microsoft_email) DO UPDATE SET \
                token_ref = excluded.token_ref, expires_at = excluded.expires_at, \
                scope = excluded.scope, updated_at = CURRENT_TIMESTAMP"
        }
    };
    sqlx::query(query)
        .bind(account_email)
        .bind(token_ref)
        .bind(expires_at)
        .bind(scope)
        .execute(pool)
        .await?;
    Ok(())
}

/// Read the row for `account_email`, if any.
pub async fn read(
    pool: &SqlitePool,
    kind: CalendarProviderKind,
    account_email: &str,
) -> Result<Option<TokenRow>, sqlx::Error> {
    // The table and column names are compile-time constants from
    // `CalendarProviderKind::token_table`/`email_column`, never derived from
    // `account_email` or any other caller-supplied string -- only the value
    // is bound as a parameter, so interpolating the names here does not open
    // a SQL injection path.
    let query = format!(
        "SELECT token_ref, expires_at, scope FROM {} WHERE {} = ?",
        kind.token_table(),
        kind.email_column()
    );
    let row = sqlx::query(&query)
        .bind(account_email)
        .fetch_optional(pool)
        .await?;
    let Some(row) = row else {
        return Ok(None);
    };
    Ok(Some(TokenRow {
        token_ref: row.try_get("token_ref")?,
        expires_at: row.try_get("expires_at")?,
        scope: row.try_get("scope")?,
    }))
}

/// Update the expiry and scope after a refresh.
///
/// `token_ref` is left unchanged: it is derived from the account, not from
/// the token (`crate::credentials::token_ref_for`), so a refresh never needs
/// to move a row to a different credential slot.
pub async fn update_after_refresh(
    pool: &SqlitePool,
    kind: CalendarProviderKind,
    account_email: &str,
    expires_at: i64,
    scope: &str,
) -> Result<(), sqlx::Error> {
    let query = match kind {
        CalendarProviderKind::Google => {
            "UPDATE google_calendar_tokens SET expires_at = ?, scope = ?, \
             updated_at = CURRENT_TIMESTAMP WHERE google_email = ?"
        }
        CalendarProviderKind::Microsoft => {
            "UPDATE microsoft_calendar_tokens SET expires_at = ?, scope = ?, \
             updated_at = CURRENT_TIMESTAMP WHERE microsoft_email = ?"
        }
    };
    sqlx::query(query)
        .bind(expires_at)
        .bind(scope)
        .bind(account_email)
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

    async fn db() -> Db {
        Db::open_in_memory().await.unwrap()
    }

    #[tokio::test]
    async fn a_missing_account_reads_as_none() {
        let db = db().await;
        let row = read(
            db.pool(),
            CalendarProviderKind::Google,
            "nobody@example.com",
        )
        .await
        .unwrap();
        assert!(row.is_none());
    }

    #[tokio::test]
    async fn upsert_then_read_round_trips_for_google() {
        let db = db().await;
        upsert(
            db.pool(),
            CalendarProviderKind::Google,
            "user@example.com",
            "calendar-google-abc123",
            1_000,
            "calendar.readonly",
        )
        .await
        .unwrap();

        let row = read(db.pool(), CalendarProviderKind::Google, "user@example.com")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(row.token_ref, "calendar-google-abc123");
        assert_eq!(row.expires_at, 1_000);
        assert_eq!(row.scope, "calendar.readonly");
    }

    #[tokio::test]
    async fn upsert_then_read_round_trips_for_microsoft() {
        let db = db().await;
        upsert(
            db.pool(),
            CalendarProviderKind::Microsoft,
            "user@example.com",
            "calendar-microsoft-abc123",
            2_000,
            "Calendars.Read",
        )
        .await
        .unwrap();

        let row = read(
            db.pool(),
            CalendarProviderKind::Microsoft,
            "user@example.com",
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(row.token_ref, "calendar-microsoft-abc123");
    }

    #[tokio::test]
    async fn upserting_twice_for_the_same_account_updates_rather_than_duplicates() {
        let db = db().await;
        for expires_at in [1_000, 2_000] {
            upsert(
                db.pool(),
                CalendarProviderKind::Google,
                "user@example.com",
                "calendar-google-abc123",
                expires_at,
                "calendar.readonly",
            )
            .await
            .unwrap();
        }

        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM google_calendar_tokens")
            .fetch_one(db.pool())
            .await
            .unwrap();
        assert_eq!(count, 1);

        let row = read(db.pool(), CalendarProviderKind::Google, "user@example.com")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(row.expires_at, 2_000);
    }

    #[tokio::test]
    async fn update_after_refresh_changes_expiry_and_scope_but_not_the_slot() {
        let db = db().await;
        upsert(
            db.pool(),
            CalendarProviderKind::Google,
            "user@example.com",
            "calendar-google-abc123",
            1_000,
            "calendar.readonly",
        )
        .await
        .unwrap();

        update_after_refresh(
            db.pool(),
            CalendarProviderKind::Google,
            "user@example.com",
            9_000,
            "calendar.readonly",
        )
        .await
        .unwrap();

        let row = read(db.pool(), CalendarProviderKind::Google, "user@example.com")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(row.expires_at, 9_000);
        assert_eq!(row.token_ref, "calendar-google-abc123");
    }

    #[tokio::test]
    async fn nothing_written_here_trips_the_storage_crates_credential_scanner() {
        let db = db().await;
        upsert(
            db.pool(),
            CalendarProviderKind::Google,
            "user@example.com",
            "calendar-google-abc123",
            1_000,
            "calendar.readonly",
        )
        .await
        .unwrap();
        kw_storage::assert_no_credential_columns(db.pool())
            .await
            .unwrap();
    }
}
