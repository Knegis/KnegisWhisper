//! SQLite storage: connection handling, versioned migrations, and the one-way
//! importer for an existing OpenWhispr database.
//!
//! Three things distinguish this from the upstream implementation it replaces.
//!
//! **Migrations are versioned.** Upstream rebuilds its schema on every boot from
//! `CREATE TABLE IF NOT EXISTS` plus roughly seventy `ALTER TABLE ... ADD COLUMN`
//! blocks with duplicate-column handlers, and uses `pragma user_version` only as
//! a one-time data-seed sentinel. The on-disk schema version is therefore
//! unknowable from the database. Here `sqlx::migrate!` owns it.
//!
//! **Credentials are not stored.** [`assert_no_credential_columns`] fails if a
//! column whose name suggests a credential appears anywhere in the schema. It is
//! run as a test and again at startup, because the specific defect being
//! guarded against — upstream's plaintext `refresh_token` columns — is the kind
//! of thing that gets ported faithfully by accident.
//!
//! **An OpenWhispr database is foreign, not old.** It gets [`import`], a
//! read-only one-way copy, rather than an in-place migration.
//!
//! Queries use the runtime `sqlx::query` API rather than the `query!` macros.
//! The macros need a live database or a checked-in cache at compile time, which
//! would mean either a `DATABASE_URL` in CI or a generated artifact to keep
//! fresh. The schema is asserted by tests instead.

pub mod dictionary;
pub mod import;
pub mod paths;
pub mod settings;
pub mod transcriptions;

use std::path::Path;

use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};
use sqlx::{Row as _, SqlitePool};

/// Failure modes of the storage layer.
#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("database error: {0}")]
    Sqlx(#[from] sqlx::Error),

    #[error("migration failed: {0}")]
    Migrate(#[from] sqlx::migrate::MigrateError),

    #[error("could not create the data directory {path}: {source}")]
    DataDir {
        path: String,
        #[source]
        source: std::io::Error,
    },

    /// The schema contains a column that looks like it holds a credential.
    /// Deliberately fatal: see the module documentation.
    #[error(
        "schema integrity: {table}.{column} looks like a credential column. \
         Secrets belong in the OS credential store, not in SQLite. If this name \
         is a false positive, add it to CREDENTIAL_COLUMN_ALLOWLIST with a reason."
    )]
    CredentialColumn { table: String, column: String },

    #[error("import failed: {0}")]
    Import(String),
}

/// Substrings that mark a column name as a probable credential.
const CREDENTIAL_COLUMN_MARKERS: [&str; 7] = [
    "token",
    "secret",
    "password",
    "credential",
    "apikey",
    "api_key",
    "passphrase",
];

/// Column names that match a marker but are demonstrably not credentials.
///
/// Every entry needs a reason. Anything not listed here fails the check, so a
/// newly added `oauth_token` column is a test failure rather than a leak.
const CREDENTIAL_COLUMN_ALLOWLIST: [(&str, &str); 3] = [
    (
        "token_ref",
        "names a slot in the OS credential store; holds no secret",
    ),
    (
        "sync_token",
        "a calendar delta cursor issued by Google/Microsoft, not an auth token",
    ),
    ("sync_token_expires_at", "expiry of the above cursor"),
];

/// An open database.
#[derive(Debug, Clone)]
pub struct Db {
    pool: SqlitePool,
}

impl Db {
    /// Open (creating if absent) the database at `path` and bring it to the
    /// latest schema version.
    pub async fn open(path: &Path) -> Result<Self, StorageError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|source| StorageError::DataDir {
                path: parent.display().to_string(),
                source,
            })?;
        }

        let options = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            // WAL so a long read (a full-text search over every note) does not
            // block the write that records a finished dictation.
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Normal)
            .foreign_keys(true)
            .busy_timeout(std::time::Duration::from_secs(5));

        let pool = SqlitePoolOptions::new()
            .max_connections(4)
            .connect_with(options)
            .await?;

        Self::from_pool(pool).await
    }

    /// Open a private in-memory database, migrated. For tests, and for the
    /// dry-run half of an import.
    ///
    /// Pinned to a single connection: an in-memory SQLite database belongs to
    /// its connection, so a pool that opened a second one would hand out an
    /// empty, unmigrated database.
    pub async fn open_in_memory() -> Result<Self, StorageError> {
        let options = SqliteConnectOptions::new()
            .in_memory(true)
            .foreign_keys(true);

        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .min_connections(1)
            .idle_timeout(None)
            .max_lifetime(None)
            .connect_with(options)
            .await?;

        Self::from_pool(pool).await
    }

    async fn from_pool(pool: SqlitePool) -> Result<Self, StorageError> {
        sqlx::migrate!("./migrations").run(&pool).await?;
        assert_no_credential_columns(&pool).await?;
        Ok(Self { pool })
    }

    #[must_use]
    pub const fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    /// Flush and close, called on shutdown.
    ///
    /// **This does not guarantee the WAL has been checkpointed**, and the
    /// comment here used to say it did. Measured on 2026-08-31: on two runs in
    /// four hundred, under load, a committed row was still only in `-wal` after
    /// this returned. SQLite checkpoints when the last connection closes and
    /// skips it if it cannot take the lock, so this is best effort.
    ///
    /// Nothing in the application depends on it, because SQLite recovers the
    /// WAL on the next open. What did depend on it was a *test* that read the
    /// main file directly and drew a conclusion from what it did not find —
    /// see `import::tests::every_database_file` (KNEG-88).
    pub async fn close(&self) {
        self.pool.close().await;
    }
}

/// Fail if any column in the schema looks like it holds a credential.
///
/// Runs over `sqlite_schema` rather than a hardcoded table list, so a table
/// added by a future migration is covered without anyone remembering to add it.
pub async fn assert_no_credential_columns(pool: &SqlitePool) -> Result<(), StorageError> {
    let tables = sqlx::query(
        "SELECT name FROM sqlite_schema WHERE type = 'table' AND name NOT LIKE 'sqlite_%'",
    )
    .fetch_all(pool)
    .await?;

    for table_row in tables {
        let table: String = table_row.try_get("name")?;

        // pragma_table_info is a table-valued function, so the table name is a
        // bindable parameter rather than string-interpolated SQL.
        let columns = sqlx::query("SELECT name FROM pragma_table_info(?)")
            .bind(&table)
            .fetch_all(pool)
            .await?;

        for column_row in columns {
            let column: String = column_row.try_get("name")?;
            let lower = column.to_lowercase();

            let allowed = CREDENTIAL_COLUMN_ALLOWLIST
                .iter()
                .any(|(name, _)| *name == lower);
            if allowed {
                continue;
            }

            if CREDENTIAL_COLUMN_MARKERS.iter().any(|m| lower.contains(m)) {
                return Err(StorageError::CredentialColumn { table, column });
            }
        }
    }

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
    use sqlx::Executor as _;

    async fn db() -> Db {
        Db::open_in_memory()
            .await
            .expect("in-memory db should migrate")
    }

    #[tokio::test]
    async fn migrations_apply_and_are_recorded() {
        let db = db().await;
        let applied: i64 = sqlx::query_scalar("SELECT count(*) FROM _sqlx_migrations")
            .fetch_one(db.pool())
            .await
            .unwrap();
        assert!(
            applied >= 1,
            "at least the initial migration should be recorded"
        );
    }

    #[tokio::test]
    async fn migrations_are_idempotent_across_reopens() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("kw.db");

        let first = Db::open(&path).await.unwrap();
        first.close().await;

        // The second open must not try to re-apply either migration.
        let second = Db::open(&path).await.unwrap();
        let applied: i64 = sqlx::query_scalar("SELECT count(*) FROM _sqlx_migrations")
            .fetch_one(second.pool())
            .await
            .unwrap();
        assert_eq!(
            applied, 2,
            "0001_initial and 0002_note_recordings, each recorded exactly once"
        );
        second.close().await;
    }

    #[tokio::test]
    async fn every_expected_table_exists_and_no_cloud_table_does() {
        let db = db().await;
        let rows = sqlx::query("SELECT name FROM sqlite_schema WHERE type = 'table'")
            .fetch_all(db.pool())
            .await
            .unwrap();
        let names: Vec<String> = rows.iter().map(|r| r.get::<String, _>("name")).collect();

        for expected in [
            "transcriptions",
            "custom_dictionary",
            "snippets",
            "folders",
            "notes",
            "actions",
            "agent_conversations",
            "agent_messages",
            "google_calendar_tokens",
            "google_calendars",
            "microsoft_calendar_tokens",
            "microsoft_calendars",
            "apple_calendars",
            "calendar_events",
            "contacts",
            "speaker_profiles",
            "speaker_mappings",
            "note_speaker_embeddings",
            "note_recordings",
            "settings",
        ] {
            assert!(
                names.iter().any(|n| n == expected),
                "missing table {expected}"
            );
        }

        // These three exist upstream only to serve the hosted sync service.
        for gone in [
            "spaces",
            "pending_vector_purges",
            "optimistic_folder_delete_rows",
        ] {
            assert!(!names.iter().any(|n| n == gone), "{gone} should not exist");
        }
    }

    #[tokio::test]
    async fn no_sync_or_cloud_columns_survive_anywhere() {
        let db = db().await;
        let tables = sqlx::query(
            "SELECT name FROM sqlite_schema WHERE type = 'table' AND name NOT LIKE 'sqlite_%'",
        )
        .fetch_all(db.pool())
        .await
        .unwrap();

        for t in tables {
            let table: String = t.get("name");
            let cols = sqlx::query("SELECT name FROM pragma_table_info(?)")
                .bind(&table)
                .fetch_all(db.pool())
                .await
                .unwrap();
            for c in cols {
                let col: String = c.get("name");
                for forbidden in [
                    "cloud_id",
                    "sync_status",
                    "deleted_at",
                    "space_id",
                    "left_team",
                    "is_shared",
                    "share_token",
                    "owner_user_id",
                    "updated_by_user_id",
                    "cloud_updated_at",
                ] {
                    assert_ne!(col, forbidden, "{table}.{col} is a sync column");
                }
                assert!(
                    !col.starts_with("client_") || col == "client_id",
                    "{table}.{col} looks like a sync client id"
                );
            }
        }
    }

    #[tokio::test]
    async fn credential_column_check_passes_on_our_own_schema() {
        let db = db().await;
        assert_no_credential_columns(db.pool()).await.unwrap();
    }

    #[tokio::test]
    async fn credential_column_check_catches_a_reintroduced_refresh_token() {
        let db = db().await;

        // Exactly the upstream defect: a plaintext OAuth token column.
        db.pool()
            .execute("ALTER TABLE google_calendar_tokens ADD COLUMN refresh_token TEXT")
            .await
            .unwrap();

        let err = assert_no_credential_columns(db.pool()).await.unwrap_err();
        match err {
            StorageError::CredentialColumn { table, column } => {
                assert_eq!(table, "google_calendar_tokens");
                assert_eq!(column, "refresh_token");
            }
            other => panic!("expected CredentialColumn, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn credential_column_check_catches_a_new_table_too() {
        let db = db().await;
        db.pool()
            .execute("CREATE TABLE plugins (id INTEGER PRIMARY KEY, api_key TEXT)")
            .await
            .unwrap();
        assert!(matches!(
            assert_no_credential_columns(db.pool()).await,
            Err(StorageError::CredentialColumn { .. })
        ));
    }

    #[tokio::test]
    async fn allowlisted_names_do_not_trip_the_check() {
        // token_ref and sync_token are both in the shipped schema and both match
        // a marker; if the allowlist stopped working, migration itself would fail.
        let db = db().await;
        let cols = sqlx::query("SELECT name FROM pragma_table_info('google_calendars')")
            .fetch_all(db.pool())
            .await
            .unwrap();
        let names: Vec<String> = cols.iter().map(|r| r.get::<String, _>("name")).collect();
        assert!(names.iter().any(|n| n == "sync_token"));
    }

    #[tokio::test]
    async fn fts_triggers_keep_the_index_in_step_with_notes() {
        let db = db().await;
        let p = db.pool();

        sqlx::query("INSERT INTO notes (title, content) VALUES (?, ?)")
            .bind("Quarterly review")
            .bind("discussed the budget for the warehouse")
            .execute(p)
            .await
            .unwrap();

        let hits: i64 =
            sqlx::query_scalar("SELECT count(*) FROM notes_fts WHERE notes_fts MATCH ?")
                .bind("warehouse")
                .fetch_one(p)
                .await
                .unwrap();
        assert_eq!(hits, 1, "insert trigger should have indexed the note");

        // Update: the old terms must stop matching, the new ones must start.
        sqlx::query("UPDATE notes SET content = ? WHERE title = ?")
            .bind("discussed the budget for the office")
            .bind("Quarterly review")
            .execute(p)
            .await
            .unwrap();

        let stale: i64 =
            sqlx::query_scalar("SELECT count(*) FROM notes_fts WHERE notes_fts MATCH ?")
                .bind("warehouse")
                .fetch_one(p)
                .await
                .unwrap();
        assert_eq!(stale, 0, "update trigger should have removed the old terms");

        let fresh: i64 =
            sqlx::query_scalar("SELECT count(*) FROM notes_fts WHERE notes_fts MATCH ?")
                .bind("office")
                .fetch_one(p)
                .await
                .unwrap();
        assert_eq!(fresh, 1);

        sqlx::query("DELETE FROM notes").execute(p).await.unwrap();
        let after_delete: i64 =
            sqlx::query_scalar("SELECT count(*) FROM notes_fts WHERE notes_fts MATCH ?")
                .bind("office")
                .fetch_one(p)
                .await
                .unwrap();
        assert_eq!(
            after_delete, 0,
            "delete trigger should have removed the row"
        );
    }

    #[tokio::test]
    async fn foreign_keys_are_enforced() {
        let db = db().await;
        let orphan = sqlx::query("INSERT INTO agent_messages (conversation_id, role, content) VALUES (9999, 'user', 'hi')")
            .execute(db.pool())
            .await;
        assert!(
            orphan.is_err(),
            "foreign_keys pragma should reject the orphan"
        );
    }

    #[tokio::test]
    async fn deleting_a_note_cascades_to_its_speaker_rows() {
        let db = db().await;
        let p = db.pool();

        let id: i64 = sqlx::query_scalar("INSERT INTO notes (title) VALUES ('m') RETURNING id")
            .fetch_one(p)
            .await
            .unwrap();
        sqlx::query("INSERT INTO speaker_mappings (note_id, speaker_id, display_name) VALUES (?, 'S1', 'Ada')")
            .bind(id)
            .execute(p)
            .await
            .unwrap();

        sqlx::query("DELETE FROM notes WHERE id = ?")
            .bind(id)
            .execute(p)
            .await
            .unwrap();

        let left: i64 = sqlx::query_scalar("SELECT count(*) FROM speaker_mappings")
            .fetch_one(p)
            .await
            .unwrap();
        assert_eq!(left, 0);
    }
}
