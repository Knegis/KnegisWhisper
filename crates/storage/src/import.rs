//! One-way import of an existing OpenWhispr database.
//!
//! An OpenWhispr `transcriptions.db` is a *foreign* database, not an older
//! version of this one, so it gets an importer rather than a migration. Four
//! properties follow from that, and each is tested:
//!
//! * **The source is opened read-only and never written.** Not by convention —
//!   `SQLITE_OPEN_READONLY`, so a stray `INSERT` would fail rather than
//!   corrupt the user's existing installation. The import is therefore
//!   reversible by deleting the target.
//!
//! * **The source column set is introspected, not assumed.** Upstream's schema
//!   is whatever seventy conditional `ALTER TABLE` statements happened to
//!   achieve on that machine, so `pragma table_info` is the only reliable way to
//!   know what is there. A column we want but the source lacks is skipped; a
//!   column the source has but we do not want is reported as dropped.
//!
//! * **Cloud and sync columns are dropped, not carried.** They are simply absent
//!   from the copy plan, so there is no code path that could bring one across.
//!
//! * **Plaintext OAuth tokens are relocated into the OS credential store.**
//!   Upstream keeps `google_calendar_tokens.refresh_token` as a plaintext SQLite
//!   column. Importing that faithfully would import the defect. The importer
//!   moves the token and leaves a slot name behind.
//!
//! Dynamic SQL note: table and column names are interpolated into the copy
//! statements, but every name comes from [`COPY_PLAN`] — a static table in this
//! file. The source database's own names are only ever used for set membership,
//! never substituted into SQL, so a maliciously named column in a hand-crafted
//! source file cannot reach the query text.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use kw_security::{CredentialStore, KeyRef, Secret};
use serde::Serialize;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::{Row as _, SqlitePool, TypeInfo as _, ValueRef as _};

use crate::{Db, StorageError};

/// Which tables are copied, in dependency order, and which columns of each.
///
/// Everything not listed is deliberately left behind. That includes the `spaces`,
/// `pending_vector_purges`, and `optimistic_folder_delete_rows` tables, every
/// cloud and sync column, and the two token tables (handled by
/// [`relocate_calendar_tokens`] instead).
const COPY_PLAN: &[(&str, &[&str])] = &[
    (
        "folders",
        &[
            "id",
            "name",
            "is_default",
            "sort_order",
            "created_at",
            "updated_at",
        ],
    ),
    (
        "notes",
        &[
            "id",
            "title",
            "content",
            "note_type",
            "source_file",
            "audio_duration_seconds",
            "enhanced_content",
            "enhancement_prompt",
            "enhanced_at_content_hash",
            "folder_id",
            "transcript",
            "calendar_event_id",
            "participants",
            "diarization_enabled",
            "expected_speaker_count",
            "created_at",
            "updated_at",
        ],
    ),
    (
        "transcriptions",
        &[
            "id",
            "text",
            "raw_text",
            "timestamp",
            "created_at",
            "has_audio",
            "audio_duration_ms",
            "provider",
            "model",
            "status",
            "error_message",
            "error_code",
            "route_kind",
        ],
    ),
    (
        "custom_dictionary",
        &["id", "word", "source", "created_at", "updated_at"],
    ),
    (
        "snippets",
        &["id", "trigger", "replacement", "created_at", "updated_at"],
    ),
    (
        "actions",
        &[
            "id",
            "name",
            "description",
            "prompt",
            "icon",
            "is_builtin",
            "sort_order",
            "translation_key",
            "created_at",
            "updated_at",
        ],
    ),
    (
        "agent_conversations",
        &[
            "id",
            "title",
            "archived_at",
            "note_id",
            "folder_id",
            "created_at",
            "updated_at",
        ],
    ),
    (
        "agent_messages",
        &[
            "id",
            "conversation_id",
            "role",
            "content",
            "metadata",
            "created_at",
        ],
    ),
    (
        "speaker_profiles",
        &[
            "id",
            "display_name",
            "email",
            "embedding",
            "sample_count",
            "created_at",
            "updated_at",
        ],
    ),
    (
        "speaker_mappings",
        &["note_id", "speaker_id", "profile_id", "display_name"],
    ),
    (
        "note_speaker_embeddings",
        &["note_id", "speaker_id", "embedding"],
    ),
    (
        "google_calendars",
        &[
            "id",
            "summary",
            "description",
            "background_color",
            "is_selected",
            "is_primary",
            "sync_token",
            "account_email",
            "created_at",
        ],
    ),
    (
        "microsoft_calendars",
        &[
            "id",
            "summary",
            "background_color",
            "is_selected",
            "is_primary",
            "sync_token",
            "sync_token_expires_at",
            "account_email",
            "created_at",
        ],
    ),
    (
        "apple_calendars",
        &["id", "title", "color", "source_name", "created_at"],
    ),
    (
        "calendar_events",
        &[
            "id",
            "calendar_id",
            "provider",
            "summary",
            "start_time",
            "end_time",
            "is_all_day",
            "status",
            "hangout_link",
            "conference_data",
            "organizer_email",
            "attendees_count",
            "attendees",
            "synced_at",
        ],
    ),
    (
        "contacts",
        &["email", "display_name", "created_at", "updated_at"],
    ),
];

/// Tables that must be empty in the target before an import may run.
///
/// Row ids are preserved so foreign keys survive the copy, which is only safe
/// into a fresh database. Refusing is better than silently remapping.
const MUST_BE_EMPTY: &[&str] = &[
    "folders",
    "notes",
    "transcriptions",
    "custom_dictionary",
    "snippets",
    "actions",
    "agent_conversations",
    "agent_messages",
    "speaker_profiles",
    "google_calendar_tokens",
    "microsoft_calendar_tokens",
];

/// What one table's copy achieved.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct TableReport {
    pub table: String,
    /// Rows written.
    pub copied: u64,
    /// Rows the source had that could not be written, almost always because
    /// they referenced a row that was itself not imported.
    pub skipped: u64,
    /// Columns this importer wanted that the source did not have.
    pub absent_columns: Vec<String>,
    /// Columns the source had that were deliberately not copied.
    pub dropped_columns: Vec<String>,
    /// Why the first skipped row was skipped. A count on its own tells the user
    /// nothing they can act on.
    pub skip_reason: Option<String>,
}

/// A token moved out of SQLite and into the OS credential store.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct RelocatedCredential {
    /// The source table it came from.
    pub source_table: String,
    /// The account it belongs to, so the user can recognise it.
    pub account: String,
    /// The credential-store slot it now lives in.
    pub slot: String,
}

/// The outcome of an import.
#[derive(Debug, Clone, Serialize)]
pub struct ImportReport {
    pub source: PathBuf,
    pub tables: Vec<TableReport>,
    pub relocated_credentials: Vec<RelocatedCredential>,
    /// Tables named in the copy plan that the source did not have at all.
    pub absent_tables: Vec<String>,
    /// Rows in `notes_fts` after the import. The triggers populate it as notes
    /// are inserted, so this is an assertion that they fired, not a rebuild.
    pub fts_rows: i64,
}

impl ImportReport {
    #[must_use]
    pub fn total_copied(&self) -> u64 {
        self.tables.iter().map(|t| t.copied).sum()
    }

    #[must_use]
    pub fn total_skipped(&self) -> u64 {
        self.tables.iter().map(|t| t.skipped).sum()
    }
}

/// A single SQLite value, carried from source to target without needing to know
/// the column's declared type.
#[derive(Debug, Clone, PartialEq)]
enum Cell {
    Null,
    Int(i64),
    Real(f64),
    Text(String),
    Blob(Vec<u8>),
}

/// Import `source` into `target`.
///
/// `credentials` receives any OAuth tokens found in the source. Pass a
/// `MemoryCredentialStore` to see what an import would move without touching the
/// real keychain.
pub async fn from_openwhispr(
    target: &Db,
    source: &Path,
    credentials: &dyn CredentialStore,
) -> Result<ImportReport, StorageError> {
    if !source.exists() {
        return Err(StorageError::Import(format!(
            "no database at {}",
            source.display()
        )));
    }

    ensure_target_is_empty(target.pool()).await?;

    let src = open_source_read_only(source).await?;
    let src_tables = table_names(&src).await?;

    // One transaction on one connection, for two reasons. An import is
    // all-or-nothing, so a failure halfway must not leave a half-populated
    // database. And in WAL mode a pool of writers racing each other over
    // thousands of single-row inserts hits SQLITE_BUSY_SNAPSHOT, which
    // busy_timeout does not help with because the snapshot has to be restarted
    // rather than waited on.
    let mut tx = target.pool().begin().await?;

    let mut tables = Vec::new();
    let mut absent_tables = Vec::new();

    for (table, wanted) in COPY_PLAN {
        if !src_tables.contains(*table) {
            absent_tables.push((*table).to_owned());
            continue;
        }
        tables.push(copy_table(&src, &mut tx, table, wanted).await?);
    }

    let pending = plan_credential_relocation(&src, &mut tx, &src_tables).await?;

    let fts_rows: i64 = sqlx::query_scalar("SELECT count(*) FROM notes_fts")
        .fetch_one(&mut *tx)
        .await?;

    tx.commit().await?;
    src.close().await;

    // Secrets are written only once the database is committed. The failure this
    // orders correctly: a committed row pointing at a slot that was never
    // written costs the user one re-authentication, whereas a secret written for
    // an import that then rolled back would sit in the keychain unreferenced.
    let mut relocated = Vec::with_capacity(pending.len());
    for item in pending {
        credentials
            .set(&item.slot, &item.secret)
            .map_err(|e| {
                StorageError::Import(format!(
                    "the database imported, but storing the {} credential failed ({e}).                      Reconnect that calendar in settings.",
                    item.record.source_table
                ))
            })?;
        relocated.push(item.record);
    }

    let report = ImportReport {
        source: source.to_path_buf(),
        tables,
        relocated_credentials: relocated,
        absent_tables,
        fts_rows,
    };

    tracing::info!(
        copied = report.total_copied(),
        skipped = report.total_skipped(),
        credentials = report.relocated_credentials.len(),
        "import complete"
    );

    Ok(report)
}

/// A credential found in the source, ready to write once the import commits.
struct PendingCredential {
    slot: KeyRef,
    secret: Secret,
    record: RelocatedCredential,
}

async fn open_source_read_only(path: &Path) -> Result<SqlitePool, StorageError> {
    let options = SqliteConnectOptions::new()
        .filename(path)
        .read_only(true)
        .create_if_missing(false)
        .foreign_keys(false);

    SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await
        .map_err(|e| {
            StorageError::Import(format!(
                "could not open {} read-only ({e}). If OpenWhispr is running, close it first: \
                 a database left in WAL mode may need recovery before it can be read.",
                path.display()
            ))
        })
}

async fn ensure_target_is_empty(pool: &SqlitePool) -> Result<(), StorageError> {
    for table in MUST_BE_EMPTY {
        // Safe interpolation: `table` is from the MUST_BE_EMPTY constant.
        let sql = format!("SELECT count(*) FROM {table}");
        let n: i64 = sqlx::query_scalar(&sql).fetch_one(pool).await?;
        if n > 0 {
            return Err(StorageError::Import(format!(
                "the target database already has {n} row(s) in {table}. \
                 An import preserves row ids so foreign keys survive, which is only \
                 safe into a fresh database. Import before adding your own data."
            )));
        }
    }
    Ok(())
}

async fn table_names(pool: &SqlitePool) -> Result<BTreeSet<String>, StorageError> {
    let rows = sqlx::query(
        "SELECT name FROM sqlite_schema WHERE type = 'table' AND name NOT LIKE 'sqlite_%'",
    )
    .fetch_all(pool)
    .await?;
    let mut set = BTreeSet::new();
    for row in rows {
        set.insert(row.try_get::<String, _>("name")?);
    }
    Ok(set)
}

async fn column_names(pool: &SqlitePool, table: &str) -> Result<BTreeSet<String>, StorageError> {
    let rows = sqlx::query("SELECT name FROM pragma_table_info(?)")
        .bind(table)
        .fetch_all(pool)
        .await?;
    let mut set = BTreeSet::new();
    for row in rows {
        set.insert(row.try_get::<String, _>("name")?);
    }
    Ok(set)
}

async fn copy_table(
    src: &SqlitePool,
    dst: &mut sqlx::SqliteConnection,
    table: &str,
    wanted: &[&str],
) -> Result<TableReport, StorageError> {
    let present = column_names(src, table).await?;

    let columns: Vec<&str> = wanted
        .iter()
        .copied()
        .filter(|c| present.contains(*c))
        .collect();
    let absent_columns: Vec<String> = wanted
        .iter()
        .filter(|c| !present.contains(**c))
        .map(|c| (*c).to_owned())
        .collect();
    let dropped_columns: Vec<String> = present
        .iter()
        .filter(|c| !wanted.contains(&c.as_str()))
        .cloned()
        .collect();

    if columns.is_empty() {
        return Ok(TableReport {
            table: table.to_owned(),
            copied: 0,
            skipped: 0,
            absent_columns,
            dropped_columns,
            skip_reason: None,
        });
    }

    // Upstream soft-deletes rather than deleting. A row the user deleted in
    // OpenWhispr must not reappear here.
    let filter = if present.contains("deleted_at") {
        " WHERE deleted_at IS NULL"
    } else {
        ""
    };

    let select = format!("SELECT {} FROM {table}{filter}", columns.join(", "));
    let placeholders = vec!["?"; columns.len()].join(", ");
    let insert = format!(
        "INSERT INTO {table} ({}) VALUES ({placeholders})",
        columns.join(", ")
    );

    let rows = sqlx::query(&select).fetch_all(src).await.map_err(|e| {
        StorageError::Import(format!("reading {table} from the source database: {e}"))
    })?;

    let mut copied = 0_u64;
    let mut skipped = 0_u64;
    let mut first_skip_reason: Option<String> = None;

    for row in &rows {
        let cells = read_row(row)?;

        let mut q = sqlx::query(&insert);
        for cell in &cells {
            q = match cell {
                Cell::Null => q.bind(Option::<String>::None),
                Cell::Int(v) => q.bind(*v),
                Cell::Real(v) => q.bind(*v),
                Cell::Text(v) => q.bind(v.clone()),
                Cell::Blob(v) => q.bind(v.clone()),
            };
        }

        // Per-row inserts so one unsatisfiable foreign key -- a note whose
        // folder was soft-deleted, say -- costs that row and not the import.
        match q.execute(&mut *dst).await {
            Ok(_) => copied += 1,
            Err(err) => {
                skipped += 1;
                if first_skip_reason.is_none() {
                    first_skip_reason = Some(err.to_string());
                }
            }
        }
    }

    if skipped > 0 {
        tracing::warn!(
            table,
            skipped,
            reason = first_skip_reason.as_deref().unwrap_or("unknown"),
            "some rows could not be imported"
        );
    }

    Ok(TableReport {
        table: table.to_owned(),
        copied,
        skipped,
        absent_columns,
        dropped_columns,
        skip_reason: first_skip_reason,
    })
}

/// Read a row into type-erased cells.
fn read_row(row: &sqlx::sqlite::SqliteRow) -> Result<Vec<Cell>, StorageError> {
    let mut cells = Vec::with_capacity(row.columns().len());

    for i in 0..row.columns().len() {
        let raw = row.try_get_raw(i)?;
        if raw.is_null() {
            cells.push(Cell::Null);
            continue;
        }

        // The storage class of the value, which is what matters for copying;
        // SQLite columns have no enforced type.
        let cell = match raw.type_info().name() {
            "INTEGER" | "BOOLEAN" => Cell::Int(row.try_get::<i64, _>(i)?),
            "REAL" => Cell::Real(row.try_get::<f64, _>(i)?),
            "BLOB" => Cell::Blob(row.try_get::<Vec<u8>, _>(i)?),
            // TEXT, and anything unexpected: text is the lossless fallback,
            // since SQLite will convert on the way in if the target column has
            // a numeric affinity.
            _ => Cell::Text(row.try_get::<String, _>(i)?),
        };
        cells.push(cell);
    }

    Ok(cells)
}

/// Take calendar OAuth tokens out of the source and write the *references* into
/// the target, returning the secrets for the caller to store once the import is
/// committed.
///
/// Upstream stores `access_token` and `refresh_token` as plaintext columns. Here
/// the pair is serialised as JSON into one credential slot and SQLite keeps only
/// the slot name.
async fn plan_credential_relocation(
    src: &SqlitePool,
    dst: &mut sqlx::SqliteConnection,
    src_tables: &BTreeSet<String>,
) -> Result<Vec<PendingCredential>, StorageError> {
    let mut pending = Vec::new();

    for (table, email_column, slot_prefix) in [
        ("google_calendar_tokens", "google_email", "calendar-google"),
        (
            "microsoft_calendar_tokens",
            "microsoft_email",
            "calendar-microsoft",
        ),
    ] {
        if !src_tables.contains(table) {
            continue;
        }

        let present = column_names(src, table).await?;
        if !present.contains(email_column) {
            continue;
        }

        // Only the columns that exist; upstream added some of these over time.
        let has = |c: &str| present.contains(c);
        let select = format!(
            "SELECT {email_column}{}{}{}{} FROM {table} ORDER BY rowid",
            if has("access_token") {
                ", access_token"
            } else {
                ""
            },
            if has("refresh_token") {
                ", refresh_token"
            } else {
                ""
            },
            if has("expires_at") {
                ", expires_at"
            } else {
                ""
            },
            if has("scope") { ", scope" } else { "" },
        );

        let rows = sqlx::query(&select).fetch_all(src).await.map_err(|e| {
            StorageError::Import(format!("reading {table} from the source database: {e}"))
        })?;

        for (index, row) in rows.iter().enumerate() {
            let email: String = row.try_get(email_column)?;

            // The slot name is derived from position, not from the address: an
            // email address contains characters a credential slot may not.
            let slot_name = format!("{slot_prefix}-{}", index + 1);
            let Some(slot) = KeyRef::new(&slot_name) else {
                return Err(StorageError::Import(format!(
                    "derived an invalid slot name {slot_name}"
                )));
            };

            let mut bundle = serde_json::Map::new();
            if has("access_token") {
                if let Ok(Some(v)) = row.try_get::<Option<String>, _>("access_token") {
                    bundle.insert("access_token".into(), v.into());
                }
            }
            if has("refresh_token") {
                if let Ok(Some(v)) = row.try_get::<Option<String>, _>("refresh_token") {
                    bundle.insert("refresh_token".into(), v.into());
                }
            }

            if bundle.is_empty() {
                tracing::info!(table, "a token row held no token; nothing to relocate");
                continue;
            }

            let encoded = serde_json::Value::Object(bundle).to_string();

            let expires_at: i64 = if has("expires_at") {
                row.try_get::<Option<i64>, _>("expires_at")?.unwrap_or(0)
            } else {
                0
            };
            let scope: String = if has("scope") {
                row.try_get::<Option<String>, _>("scope")?
                    .unwrap_or_default()
            } else {
                String::new()
            };

            let insert = format!(
                "INSERT INTO {table} ({email_column}, token_ref, expires_at, scope) VALUES (?, ?, ?, ?)"
            );
            sqlx::query(&insert)
                .bind(&email)
                .bind(slot.slot())
                .bind(expires_at)
                .bind(&scope)
                .execute(&mut *dst)
                .await?;

            pending.push(PendingCredential {
                slot,
                secret: Secret::new(encoded),
                record: RelocatedCredential {
                    source_table: table.to_owned(),
                    account: email,
                    slot: slot_name,
                },
            });
        }
    }

    Ok(pending)
}

/// Which source columns were dropped, across every table, as one map. Handy for
/// the import summary the UI shows.
#[must_use]
pub fn dropped_by_table(report: &ImportReport) -> BTreeMap<String, Vec<String>> {
    report
        .tables
        .iter()
        .filter(|t| !t.dropped_columns.is_empty())
        .map(|t| (t.table.clone(), t.dropped_columns.clone()))
        .collect()
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
    use kw_security::MemoryCredentialStore;
    use sqlx::Executor as _;

    /// Build a source database shaped like a real OpenWhispr install: the full
    /// cloud-column set present, some rows soft-deleted, plaintext OAuth tokens.
    async fn openwhispr_fixture(path: &Path) -> SqlitePool {
        let options = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await
            .unwrap();

        pool.execute(
            r"
            CREATE TABLE folders (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL,
                is_default INTEGER NOT NULL DEFAULT 0,
                sort_order INTEGER NOT NULL DEFAULT 0,
                client_folder_id TEXT,
                cloud_id TEXT,
                sync_status TEXT DEFAULT 'pending',
                deleted_at TEXT,
                space_id INTEGER,
                left_team INTEGER NOT NULL DEFAULT 0,
                created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
                updated_at DATETIME
            );
            CREATE TABLE notes (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                title TEXT NOT NULL DEFAULT 'Untitled Note',
                content TEXT NOT NULL DEFAULT '',
                note_type TEXT NOT NULL DEFAULT 'personal',
                enhanced_content TEXT,
                folder_id INTEGER,
                transcript TEXT,
                audio_duration_seconds REAL,
                cloud_id TEXT,
                client_note_id TEXT,
                sync_status TEXT DEFAULT 'pending',
                deleted_at TEXT,
                is_shared INTEGER NOT NULL DEFAULT 0,
                share_token TEXT,
                space_id INTEGER,
                owner_user_id TEXT,
                cloud_updated_at TEXT,
                created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
                updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
            );
            CREATE TABLE transcriptions (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                text TEXT NOT NULL,
                raw_text TEXT,
                has_audio INTEGER NOT NULL DEFAULT 0,
                audio_duration_ms INTEGER,
                provider TEXT,
                model TEXT,
                status TEXT NOT NULL DEFAULT 'completed',
                client_transcription_id TEXT,
                cloud_id TEXT,
                sync_status TEXT DEFAULT 'pending',
                deleted_at TEXT,
                created_at DATETIME DEFAULT CURRENT_TIMESTAMP
            );
            CREATE TABLE speaker_profiles (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                display_name TEXT NOT NULL,
                email TEXT,
                embedding BLOB NOT NULL,
                sample_count INTEGER DEFAULT 1,
                created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
                updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
            );
            CREATE TABLE speaker_mappings (
                note_id INTEGER NOT NULL,
                speaker_id TEXT NOT NULL,
                profile_id INTEGER,
                display_name TEXT NOT NULL,
                PRIMARY KEY (note_id, speaker_id)
            );
            CREATE TABLE google_calendar_tokens (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                google_email TEXT NOT NULL UNIQUE,
                access_token TEXT NOT NULL,
                refresh_token TEXT NOT NULL,
                expires_at INTEGER NOT NULL,
                scope TEXT NOT NULL,
                created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
                updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
            );
            CREATE TABLE spaces (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL,
                cloud_team_id TEXT
            );
            ",
        )
        .await
        .unwrap();

        pool.execute(
            r"
            INSERT INTO folders (id, name, cloud_id, sync_status) VALUES (1, 'Videos', 'cl_1', 'synced');
            INSERT INTO folders (id, name, deleted_at) VALUES (2, 'Removed', '2026-01-01');
            INSERT INTO notes (id, title, content, folder_id, cloud_id, share_token)
                VALUES (1, 'Kickoff', 'we agreed on the warehouse plan', 1, 'cl_n1', 'tok_public');
            INSERT INTO notes (id, title, content, folder_id, deleted_at)
                VALUES (2, 'Trashed', 'should not be imported', 1, '2026-02-02');
            INSERT INTO notes (id, title, content, folder_id)
                VALUES (3, 'Orphan', 'folder was deleted', 2);
            INSERT INTO transcriptions (id, text, provider, model) VALUES (1, 'hello there', 'openai', 'whisper-1');
            INSERT INTO transcriptions (id, text, deleted_at) VALUES (2, 'deleted dictation', '2026-03-03');
            INSERT INTO speaker_profiles (id, display_name, embedding) VALUES (1, 'Ada', X'00FF10');
            INSERT INTO speaker_mappings (note_id, speaker_id, profile_id, display_name)
                VALUES (1, 'S1', 1, 'Ada');
            INSERT INTO speaker_mappings (note_id, speaker_id, display_name)
                VALUES (2, 'S1', 'Belongs to a trashed note');
            INSERT INTO google_calendar_tokens (google_email, access_token, refresh_token, expires_at, scope)
                VALUES ('user@example.com', 'ya29.access', '1//refresh-secret', 1893456000, 'calendar.readonly');
            INSERT INTO spaces (name, cloud_team_id) VALUES ('Team', 'team_1');
            ",
        )
        .await
        .unwrap();

        pool
    }

    async fn run_import() -> (ImportReport, Db, MemoryCredentialStore, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let src_path = dir.path().join("transcriptions.db");
        openwhispr_fixture(&src_path).await.close().await;

        let target = Db::open(&dir.path().join("kw.db")).await.unwrap();
        let creds = MemoryCredentialStore::new();
        let report = from_openwhispr(&target, &src_path, &creds).await.unwrap();
        (report, target, creds, dir)
    }

    #[tokio::test]
    async fn copies_the_rows_that_were_not_soft_deleted() {
        let (report, target, _, _dir) = run_import().await;

        let folders: i64 = sqlx::query_scalar("SELECT count(*) FROM folders")
            .fetch_one(target.pool())
            .await
            .unwrap();
        assert_eq!(folders, 1, "the soft-deleted folder must not be imported");

        let titles: Vec<String> = sqlx::query_scalar("SELECT title FROM notes ORDER BY id")
            .fetch_all(target.pool())
            .await
            .unwrap();
        assert_eq!(
            titles,
            vec!["Kickoff".to_owned()],
            "only the live note survives"
        );

        let texts: Vec<String> = sqlx::query_scalar("SELECT text FROM transcriptions")
            .fetch_all(target.pool())
            .await
            .unwrap();
        assert_eq!(texts, vec!["hello there".to_owned()]);

        // Exact counts, so a regression that quietly stops copying a table is a
        // failure rather than a still-passing inequality.
        let copied: Vec<(&str, u64)> = report
            .tables
            .iter()
            .filter(|t| t.copied > 0)
            .map(|t| (t.table.as_str(), t.copied))
            .collect();
        assert_eq!(
            copied,
            vec![
                ("folders", 1),
                ("notes", 1),
                ("transcriptions", 1),
                ("speaker_profiles", 1),
                ("speaker_mappings", 1),
            ]
        );
    }

    #[tokio::test]
    async fn a_row_whose_parent_was_dropped_is_skipped_and_counted() {
        let (report, _target, _, _dir) = run_import().await;

        // note 3 pointed at folder 2, which was soft-deleted; speaker_mappings
        // row for note 2 pointed at a soft-deleted note.
        let skipped: u64 = report.total_skipped();
        assert!(
            skipped >= 2,
            "expected the orphans to be skipped, got {skipped}"
        );

        let notes = report.tables.iter().find(|t| t.table == "notes").unwrap();
        assert_eq!(notes.copied, 1);
        assert_eq!(notes.skipped, 1, "the note whose folder was deleted");
    }

    #[tokio::test]
    async fn cloud_and_sync_columns_are_reported_as_dropped_and_do_not_arrive() {
        let (report, target, _, _dir) = run_import().await;

        let dropped = dropped_by_table(&report);
        let note_drops = dropped.get("notes").unwrap();
        for expected in [
            "cloud_id",
            "client_note_id",
            "sync_status",
            "share_token",
            "space_id",
        ] {
            assert!(
                note_drops.contains(&expected.to_owned()),
                "{expected} should be reported dropped"
            );
        }

        // And the target has no such column to receive them.
        crate::assert_no_credential_columns(target.pool())
            .await
            .unwrap();
        let cols: Vec<String> = sqlx::query_scalar("SELECT name FROM pragma_table_info('notes')")
            .fetch_all(target.pool())
            .await
            .unwrap();
        assert!(!cols.iter().any(|c| c == "share_token"));
    }

    #[tokio::test]
    async fn the_spaces_table_is_left_behind_entirely() {
        let (report, target, _, _dir) = run_import().await;
        assert!(!report.tables.iter().any(|t| t.table == "spaces"));

        let exists: i64 =
            sqlx::query_scalar("SELECT count(*) FROM sqlite_schema WHERE name = 'spaces'")
                .fetch_one(target.pool())
                .await
                .unwrap();
        assert_eq!(exists, 0);
    }

    #[tokio::test]
    async fn oauth_tokens_move_to_the_credential_store_and_sqlite_keeps_only_a_reference() {
        let (report, target, creds, _dir) = run_import().await;

        assert_eq!(report.relocated_credentials.len(), 1);
        let moved = &report.relocated_credentials[0];
        assert_eq!(moved.account, "user@example.com");
        assert_eq!(moved.slot, "calendar-google-1");

        // The secret is in the credential store...
        let slot = KeyRef::new(&moved.slot).unwrap();
        let stored = creds.get(&slot).unwrap();
        assert!(stored.expose().contains("1//refresh-secret"));

        // ...and the database has the slot name, not the token.
        let token_ref: String = sqlx::query_scalar("SELECT token_ref FROM google_calendar_tokens")
            .fetch_one(target.pool())
            .await
            .unwrap();
        assert_eq!(token_ref, "calendar-google-1");
    }

    #[tokio::test]
    async fn no_plaintext_token_appears_anywhere_in_the_target_file() {
        let dir = tempfile::tempdir().unwrap();
        let src_path = dir.path().join("transcriptions.db");
        openwhispr_fixture(&src_path).await.close().await;

        let target_path = dir.path().join("kw.db");
        let target = Db::open(&target_path).await.unwrap();
        let creds = MemoryCredentialStore::new();
        from_openwhispr(&target, &src_path, &creds).await.unwrap();
        target.close().await;

        // The strongest form of the assertion: grep the bytes on disk.
        let bytes = std::fs::read(&target_path).unwrap();
        let haystack = String::from_utf8_lossy(&bytes);
        assert!(
            !haystack.contains("1//refresh-secret"),
            "refresh token reached the database file"
        );
        assert!(
            !haystack.contains("ya29.access"),
            "access token reached the database file"
        );
        assert!(
            haystack.contains("calendar-google-1"),
            "the slot reference should be there"
        );
    }

    #[tokio::test]
    async fn the_source_database_is_not_modified() {
        let dir = tempfile::tempdir().unwrap();
        let src_path = dir.path().join("transcriptions.db");
        openwhispr_fixture(&src_path).await.close().await;

        let before = std::fs::read(&src_path).unwrap();

        let target = Db::open(&dir.path().join("kw.db")).await.unwrap();
        from_openwhispr(&target, &src_path, &MemoryCredentialStore::new())
            .await
            .unwrap();

        let after = std::fs::read(&src_path).unwrap();
        assert_eq!(
            before, after,
            "the import must not write to the source file"
        );
    }

    #[tokio::test]
    async fn fts_is_populated_by_the_triggers_during_import() {
        let (report, target, _, _dir) = run_import().await;
        assert_eq!(report.fts_rows, 1);

        let hits: i64 =
            sqlx::query_scalar("SELECT count(*) FROM notes_fts WHERE notes_fts MATCH ?")
                .bind("warehouse")
                .fetch_one(target.pool())
                .await
                .unwrap();
        assert_eq!(
            hits, 1,
            "an imported note should be searchable without a manual rebuild"
        );
    }

    #[tokio::test]
    async fn blobs_survive_the_copy() {
        let (_report, target, _, _dir) = run_import().await;
        let embedding: Vec<u8> = sqlx::query_scalar("SELECT embedding FROM speaker_profiles")
            .fetch_one(target.pool())
            .await
            .unwrap();
        assert_eq!(embedding, vec![0x00, 0xFF, 0x10]);
    }

    #[tokio::test]
    async fn importing_into_a_database_that_already_has_data_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let src_path = dir.path().join("transcriptions.db");
        openwhispr_fixture(&src_path).await.close().await;

        let target = Db::open(&dir.path().join("kw.db")).await.unwrap();
        sqlx::query("INSERT INTO folders (name) VALUES ('Mine')")
            .execute(target.pool())
            .await
            .unwrap();

        let err = from_openwhispr(&target, &src_path, &MemoryCredentialStore::new())
            .await
            .unwrap_err();
        assert!(err.to_string().contains("already has"), "got {err}");
    }

    #[tokio::test]
    async fn a_source_missing_optional_tables_and_columns_still_imports() {
        let dir = tempfile::tempdir().unwrap();
        let src_path = dir.path().join("old.db");

        // An install from before half the ALTERs ran: notes has no
        // enhanced_content, and there are no calendar or speaker tables at all.
        let options = SqliteConnectOptions::new()
            .filename(&src_path)
            .create_if_missing(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await
            .unwrap();
        pool.execute(
            r"
            CREATE TABLE notes (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                title TEXT NOT NULL,
                content TEXT NOT NULL DEFAULT ''
            );
            INSERT INTO notes (title, content) VALUES ('Early note', 'from an old install');
            ",
        )
        .await
        .unwrap();
        pool.close().await;

        let target = Db::open(&dir.path().join("kw.db")).await.unwrap();
        let report = from_openwhispr(&target, &src_path, &MemoryCredentialStore::new())
            .await
            .unwrap();

        assert_eq!(report.total_copied(), 1);
        assert!(report.absent_tables.contains(&"folders".to_owned()));
        assert!(report
            .absent_tables
            .contains(&"google_calendars".to_owned()));

        let notes = report.tables.iter().find(|t| t.table == "notes").unwrap();
        assert!(notes
            .absent_columns
            .contains(&"enhanced_content".to_owned()));
        assert_eq!(notes.copied, 1);
    }

    #[tokio::test]
    async fn a_skipped_row_reports_why_it_was_skipped() {
        let (report, _target, _, _dir) = run_import().await;

        let notes = report.tables.iter().find(|t| t.table == "notes").unwrap();
        assert_eq!(notes.skipped, 1);
        let reason = notes.skip_reason.as_deref().unwrap_or_default();
        assert!(
            reason.to_lowercase().contains("foreign key"),
            "a skip count with no reason is not actionable; got {reason:?}"
        );

        // A table that lost nothing must not invent a reason.
        let transcriptions = report
            .tables
            .iter()
            .find(|t| t.table == "transcriptions")
            .unwrap();
        assert_eq!(transcriptions.skipped, 0);
        assert_eq!(transcriptions.skip_reason, None);
    }

    #[tokio::test]
    async fn nothing_is_written_when_the_target_is_rejected() {
        // The emptiness check runs before the transaction opens, so a refused
        // import must not have touched the credential store either.
        let dir = tempfile::tempdir().unwrap();
        let src_path = dir.path().join("transcriptions.db");
        openwhispr_fixture(&src_path).await.close().await;

        let target = Db::open(&dir.path().join("kw.db")).await.unwrap();
        sqlx::query("INSERT INTO notes (title) VALUES ('mine')")
            .execute(target.pool())
            .await
            .unwrap();

        let creds = MemoryCredentialStore::new();
        assert!(from_openwhispr(&target, &src_path, &creds).await.is_err());

        assert!(
            !creds.contains(&KeyRef::new("calendar-google-1").unwrap()),
            "a refused import must not have stored a credential"
        );
        let folders: i64 = sqlx::query_scalar("SELECT count(*) FROM folders")
            .fetch_one(target.pool())
            .await
            .unwrap();
        assert_eq!(folders, 0, "no rows should have been copied");
    }

    #[tokio::test]
    async fn a_missing_source_file_is_a_clear_error() {
        let dir = tempfile::tempdir().unwrap();
        let target = Db::open(&dir.path().join("kw.db")).await.unwrap();
        let err = from_openwhispr(
            &target,
            &dir.path().join("nope.db"),
            &MemoryCredentialStore::new(),
        )
        .await
        .unwrap_err();
        assert!(err.to_string().contains("no database at"));
    }
}
