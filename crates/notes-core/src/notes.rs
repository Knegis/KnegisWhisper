//! Notes and folders.
//!
//! Full-text search goes through the FTS5 external-content table the storage
//! crate builds, and the query it receives is **escaped rather than passed
//! through**. That is the one thing here worth reading carefully: FTS5 has its
//! own query syntax, so an unescaped search for `C++ (draft)` is a syntax error
//! rather than a search, and a search for `NEAR` matches nothing. Users type
//! prose, not query expressions.

use kw_storage::StorageError;
use serde::Serialize;
use sqlx::{FromRow, SqlitePool};
use ts_rs::TS;

/// A note, as listed.
#[derive(Debug, Clone, FromRow, Serialize, PartialEq, TS)]
#[serde(rename_all = "camelCase")]
pub struct Note {
    pub id: i64,
    pub title: String,
    pub content: String,
    pub note_type: String,
    pub enhanced_content: Option<String>,
    pub folder_id: Option<i64>,
    pub transcript: Option<String>,
    pub participants: Option<String>,
    pub audio_duration_seconds: Option<f64>,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
}

/// A folder.
#[derive(Debug, Clone, FromRow, Serialize, PartialEq, Eq, TS)]
#[serde(rename_all = "camelCase")]
pub struct Folder {
    pub id: i64,
    pub name: String,
    pub is_default: bool,
    pub sort_order: i64,
}

/// A new note.
#[derive(Debug, Clone, Default)]
pub struct NewNote {
    pub title: String,
    pub content: String,
    pub note_type: Option<String>,
    pub folder_id: Option<i64>,
    pub transcript: Option<String>,
    pub participants: Option<String>,
    pub audio_duration_seconds: Option<f64>,
}

/// Create a note, returning its id.
pub async fn create(pool: &SqlitePool, note: &NewNote) -> Result<i64, StorageError> {
    let title = if note.title.trim().is_empty() {
        "Untitled Note".to_owned()
    } else {
        note.title.clone()
    };

    let id: i64 = sqlx::query_scalar(
        "INSERT INTO notes
            (title, content, note_type, folder_id, transcript, participants,
             audio_duration_seconds)
         VALUES (?, ?, ?, ?, ?, ?, ?)
         RETURNING id",
    )
    .bind(&title)
    .bind(&note.content)
    .bind(note.note_type.as_deref().unwrap_or("personal"))
    .bind(note.folder_id)
    .bind(&note.transcript)
    .bind(&note.participants)
    .bind(note.audio_duration_seconds)
    .fetch_one(pool)
    .await?;

    Ok(id)
}

const COLUMNS: &str = "id, title, content, note_type, enhanced_content, folder_id, transcript, \
                       participants, audio_duration_seconds, created_at, updated_at";

/// One note by id.
pub async fn get(pool: &SqlitePool, id: i64) -> Result<Option<Note>, StorageError> {
    let sql = format!("SELECT {COLUMNS} FROM notes WHERE id = ?");
    Ok(sqlx::query_as::<_, Note>(&sql)
        .bind(id)
        .fetch_optional(pool)
        .await?)
}

/// Notes, newest first. `limit` is clamped so a frontend cannot ask for the
/// whole table.
pub async fn list(
    pool: &SqlitePool,
    note_type: Option<&str>,
    folder_id: Option<i64>,
    limit: i64,
) -> Result<Vec<Note>, StorageError> {
    let limit = limit.clamp(1, 500);

    // Ordered by `updated_at`, because a list of notes is most useful with the
    // one you were last working on at the top. That is also why there is no
    // cursor here, unlike `history::list`: a stable cursor over a timestamp
    // needs an (updated_at, id) tuple comparison, and a cursor that is only
    // *nearly* stable silently skips or repeats rows. Callers that need paging
    // get an honest `has_more` and no cursor rather than a broken one.
    //
    // The WHERE clause is assembled from fixed fragments and every value is
    // bound, so nothing a caller sends reaches the SQL text.
    let mut sql = format!("SELECT {COLUMNS} FROM notes");
    let mut clauses: Vec<&str> = Vec::new();
    if note_type.is_some() {
        clauses.push("note_type = ?");
    }
    if folder_id.is_some() {
        clauses.push("folder_id = ?");
    }
    if !clauses.is_empty() {
        sql.push_str(" WHERE ");
        sql.push_str(&clauses.join(" AND "));
    }
    sql.push_str(" ORDER BY updated_at DESC LIMIT ?");

    let mut query = sqlx::query_as::<_, Note>(&sql);
    if let Some(note_type) = note_type {
        query = query.bind(note_type.to_owned());
    }
    if let Some(folder_id) = folder_id {
        query = query.bind(folder_id);
    }

    Ok(query.bind(limit).fetch_all(pool).await?)
}

/// The fields a caller wants to change. `None` means leave it alone.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NoteUpdate {
    pub title: Option<String>,
    pub content: Option<String>,
    pub folder_id: Option<i64>,
}

impl NoteUpdate {
    /// Whether this would change anything.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.title.is_none() && self.content.is_none() && self.folder_id.is_none()
    }
}

/// Apply a partial update, returning whether the note existed.
///
/// Partial rather than whole-record, because a caller that only wants to move a
/// note to a folder should not have to send the content back — and a caller that
/// sends a stale copy of the content would silently discard whatever changed in
/// between.
///
/// `updated_at` is set here rather than by a trigger so the FTS triggers and
/// this write stay in one statement.
///
/// The SET clause is built from fixed fragments chosen by which fields are
/// present; every value is bound. Nothing the caller sends reaches the SQL text.
pub async fn update(pool: &SqlitePool, id: i64, change: &NoteUpdate) -> Result<bool, StorageError> {
    if change.is_empty() {
        // Nothing asked for, so nothing done -- but the caller still wants to
        // know whether the note is there.
        let exists = sqlx::query("SELECT 1 FROM notes WHERE id = ?")
            .bind(id)
            .fetch_optional(pool)
            .await?;
        return Ok(exists.is_some());
    }

    let mut sets: Vec<&str> = Vec::new();
    if change.title.is_some() {
        sets.push("title = ?");
    }
    if change.content.is_some() {
        sets.push("content = ?");
    }
    if change.folder_id.is_some() {
        sets.push("folder_id = ?");
    }
    sets.push("updated_at = CURRENT_TIMESTAMP");

    let sql = format!("UPDATE notes SET {} WHERE id = ?", sets.join(", "));
    let mut query = sqlx::query(&sql);
    if let Some(title) = &change.title {
        query = query.bind(title.clone());
    }
    if let Some(content) = &change.content {
        query = query.bind(content.clone());
    }
    if let Some(folder_id) = change.folder_id {
        query = query.bind(folder_id);
    }

    let result = query.bind(id).execute(pool).await?;
    Ok(result.rows_affected() > 0)
}

/// Delete a note. Its FTS row and speaker rows go with it.
pub async fn delete(pool: &SqlitePool, id: i64) -> Result<bool, StorageError> {
    let result = sqlx::query("DELETE FROM notes WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}

/// Escape a user's search text into an FTS5 query.
///
/// FTS5 has its own syntax: bare `-`, `*`, `:`, `^`, `(`, `)` and `"` are
/// operators, and `AND`, `OR`, `NOT` and `NEAR` are keywords. A user typing
/// `C++ (draft)` gets a syntax error, and one typing `NEAR` gets nothing.
///
/// Every token is therefore quoted, which makes it a literal phrase term, and
/// embedded quotes are doubled. The tokens are combined with `AND` so more words
/// narrows the search, which is what people expect from a search box.
#[must_use]
pub fn escape_query(input: &str) -> Option<String> {
    let tokens: Vec<String> = input
        .split_whitespace()
        // Keep only characters FTS5's default tokenizer would index anyway.
        .map(|token| token.replace('"', "\"\""))
        .filter(|token| token.chars().any(char::is_alphanumeric))
        .map(|token| format!("\"{token}\""))
        .collect();

    if tokens.is_empty() {
        return None;
    }

    Some(tokens.join(" AND "))
}

/// A search hit.
#[derive(Debug, Clone, FromRow, Serialize, PartialEq, TS)]
#[serde(rename_all = "camelCase")]
pub struct SearchHit {
    pub id: i64,
    pub title: String,
    /// A snippet with the match marked by `[` and `]`.
    pub snippet: String,
}

/// Search notes.
///
/// Returns an empty result for a query with nothing searchable in it, rather
/// than every note — an empty search box should not look like a full-table
/// scan.
pub async fn search(
    pool: &SqlitePool,
    query: &str,
    limit: i64,
) -> Result<Vec<SearchHit>, StorageError> {
    let Some(escaped) = escape_query(query) else {
        return Ok(Vec::new());
    };

    let limit = limit.clamp(1, 200);

    let rows = sqlx::query_as::<_, SearchHit>(
        "SELECT notes.id AS id,
                notes.title AS title,
                snippet(notes_fts, 1, '[', ']', '…', 24) AS snippet
         FROM notes_fts
         JOIN notes ON notes.id = notes_fts.rowid
         WHERE notes_fts MATCH ?
         ORDER BY rank
         LIMIT ?",
    )
    .bind(&escaped)
    .bind(limit)
    .fetch_all(pool)
    .await?;

    Ok(rows)
}

/* ------------------------------------------------------------------ folders */

/// Create a folder.
pub async fn create_folder(pool: &SqlitePool, name: &str) -> Result<i64, StorageError> {
    let id: i64 = sqlx::query_scalar("INSERT INTO folders (name) VALUES (?) RETURNING id")
        .bind(name.trim())
        .fetch_one(pool)
        .await?;
    Ok(id)
}

/// Folders in display order.
pub async fn folders(pool: &SqlitePool) -> Result<Vec<Folder>, StorageError> {
    Ok(sqlx::query_as::<_, Folder>(
        "SELECT id, name, is_default, sort_order FROM folders ORDER BY sort_order, name",
    )
    .fetch_all(pool)
    .await?)
}

/// Delete a folder. Its notes survive, with no folder.
pub async fn delete_folder(pool: &SqlitePool, id: i64) -> Result<bool, StorageError> {
    let result = sqlx::query("DELETE FROM folders WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
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

    async fn add(pool: &SqlitePool, title: &str, content: &str) -> i64 {
        create(
            pool,
            &NewNote {
                title: title.to_owned(),
                content: content.to_owned(),
                ..Default::default()
            },
        )
        .await
        .unwrap()
    }

    /* --------------------------------------------------------------- crud */

    #[tokio::test]
    async fn creates_reads_updates_and_deletes() {
        let db = db().await;
        let p = db.pool();

        let id = add(p, "Kickoff", "we agreed on the warehouse plan").await;
        let note = get(p, id).await.unwrap().unwrap();
        assert_eq!(note.title, "Kickoff");
        assert_eq!(note.note_type, "personal");

        assert!(update(
            p,
            id,
            &NoteUpdate {
                title: Some("Kickoff (revised)".to_owned()),
                content: Some("we agreed on the office plan".to_owned()),
                ..Default::default()
            },
        )
        .await
        .unwrap());
        assert_eq!(
            get(p, id).await.unwrap().unwrap().title,
            "Kickoff (revised)"
        );

        assert!(delete(p, id).await.unwrap());
        assert_eq!(get(p, id).await.unwrap(), None);
        assert!(
            !delete(p, id).await.unwrap(),
            "deleting twice is not a second removal"
        );
    }

    #[tokio::test]
    async fn an_untitled_note_gets_a_placeholder_rather_than_an_empty_title() {
        let db = db().await;
        let id = create(
            db.pool(),
            &NewNote {
                title: "   ".to_owned(),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(
            get(db.pool(), id).await.unwrap().unwrap().title,
            "Untitled Note"
        );
    }

    #[tokio::test]
    async fn listing_is_clamped_and_can_be_filtered_by_folder() {
        let db = db().await;
        let p = db.pool();

        let folder = create_folder(p, "Meetings").await.unwrap();
        for i in 0..3 {
            create(
                p,
                &NewNote {
                    title: format!("in folder {i}"),
                    folder_id: Some(folder),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        }
        add(p, "loose note", "").await;

        assert_eq!(list(p, None, None, 100).await.unwrap().len(), 4);
        assert_eq!(list(p, None, Some(folder), 100).await.unwrap().len(), 3);
        assert_eq!(list(p, None, None, i64::MAX).await.unwrap().len(), 4);
        assert_eq!(
            list(p, None, None, 0).await.unwrap().len(),
            1,
            "a zero limit is clamped to one"
        );
    }

    #[tokio::test]
    async fn deleting_a_folder_keeps_its_notes() {
        let db = db().await;
        let p = db.pool();

        let folder = create_folder(p, "Temporary").await.unwrap();
        let note = create(
            p,
            &NewNote {
                title: "survivor".to_owned(),
                folder_id: Some(folder),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        assert!(delete_folder(p, folder).await.unwrap());

        let survivor = get(p, note)
            .await
            .unwrap()
            .expect("the note should survive");
        assert_eq!(survivor.folder_id, None, "it should just lose its folder");
    }

    /* --------------------------------------------------------- query escaping */

    #[test]
    fn ordinary_words_become_an_and_query() {
        assert_eq!(
            escape_query("warehouse plan").as_deref(),
            Some("\"warehouse\" AND \"plan\"")
        );
    }

    #[test]
    fn fts_operators_are_escaped_rather_than_interpreted() {
        // Each of these is a syntax error or a silent no-match if passed
        // through, and users type prose rather than query expressions.
        for input in [
            "C++ (draft)",
            "-minus",
            "wild*",
            "colon:here",
            "^caret",
            "a AND b",
            "NEAR",
        ] {
            let escaped = escape_query(input).unwrap_or_default();
            // Every token is quoted, so nothing is an operator any more.
            assert!(
                escaped.is_empty() || escaped.starts_with('"'),
                "{input:?} produced {escaped:?}"
            );
        }
    }

    #[test]
    fn an_embedded_quote_is_doubled() {
        let escaped = escape_query("say \"hello\"").unwrap();
        assert!(escaped.contains("\"\""), "{escaped}");
    }

    #[test]
    fn a_query_with_nothing_searchable_returns_none() {
        for input in ["", "   ", "--", "*", "()", "!!!"] {
            assert_eq!(escape_query(input), None, "{input:?}");
        }
    }

    /* ------------------------------------------------------------- searching */

    #[tokio::test]
    async fn finds_notes_by_content_and_ranks_them() {
        let db = db().await;
        let p = db.pool();

        add(
            p,
            "Warehouse move",
            "we will relocate the warehouse in March",
        )
        .await;
        add(p, "Office plan", "the office layout needs work").await;
        add(p, "Unrelated", "lunch options").await;

        let hits = search(p, "warehouse", 10).await.unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].title, "Warehouse move");
        assert!(
            hits[0].snippet.contains('['),
            "the match should be marked: {:?}",
            hits[0].snippet
        );
    }

    #[tokio::test]
    async fn more_words_narrows_the_search() {
        let db = db().await;
        let p = db.pool();

        add(p, "A", "warehouse in March").await;
        add(p, "B", "warehouse in April").await;

        assert_eq!(search(p, "warehouse", 10).await.unwrap().len(), 2);
        assert_eq!(search(p, "warehouse March", 10).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn a_query_that_would_be_a_syntax_error_searches_instead_of_failing() {
        let db = db().await;
        let p = db.pool();
        add(p, "Notes on C++", "the C++ (draft) standard").await;

        // Passed through unescaped, FTS5 rejects this outright.
        let hits = search(p, "C++ (draft)", 10).await.unwrap();
        assert_eq!(hits.len(), 1, "the search should work, not error");
    }

    #[tokio::test]
    async fn a_query_containing_an_fts_keyword_is_treated_as_text() {
        let db = db().await;
        let p = db.pool();
        add(
            p,
            "Proximity",
            "the NEAR keyword is part of the query language",
        )
        .await;

        let hits = search(p, "NEAR keyword", 10).await.unwrap();
        assert_eq!(hits.len(), 1);
    }

    #[tokio::test]
    async fn an_empty_search_returns_nothing_rather_than_everything() {
        let db = db().await;
        let p = db.pool();
        add(p, "A", "content").await;
        add(p, "B", "content").await;

        for query in ["", "   ", "***"] {
            assert!(
                search(p, query, 10).await.unwrap().is_empty(),
                "{query:?} should find nothing"
            );
        }
    }

    #[tokio::test]
    async fn an_edited_note_is_searchable_by_its_new_text_and_not_its_old() {
        let db = db().await;
        let p = db.pool();

        let id = add(p, "Plan", "the warehouse option").await;
        assert_eq!(search(p, "warehouse", 10).await.unwrap().len(), 1);

        update(
            p,
            id,
            &NoteUpdate {
                title: Some("Plan".to_owned()),
                content: Some("the office option".to_owned()),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        assert!(
            search(p, "warehouse", 10).await.unwrap().is_empty(),
            "the FTS index went stale"
        );
        assert_eq!(search(p, "office", 10).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn a_deleted_note_leaves_the_index() {
        let db = db().await;
        let p = db.pool();
        let id = add(p, "Gone", "ephemeral content").await;
        delete(p, id).await.unwrap();
        assert!(search(p, "ephemeral", 10).await.unwrap().is_empty());
    }
}
