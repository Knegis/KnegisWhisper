//! Notes and folders.
//!
//! Full-text search goes through the FTS5 index the storage
//! crate builds, and the query it receives is **escaped rather than passed
//! through**. That is the one thing here worth reading carefully: FTS5 has its
//! own query syntax, so an unescaped search for `C++ (draft)` is a syntax error
//! rather than a search, and a search for `NEAR` matches nothing. Users type
//! prose, not query expressions.

use kw_storage::StorageError;
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, SqliteConnection, SqlitePool};
use std::collections::BTreeMap;
use ts_rs::TS;

use crate::transcript::{self, NoteMeta, SpeakerLabels};

/// A note, as listed.
#[derive(Debug, Clone, FromRow, Serialize, PartialEq, TS)]
#[serde(rename_all = "camelCase")]
pub struct Note {
    pub id: i64,
    pub title: String,
    pub content: String,
    pub recording_details: Option<String>,
    pub note_type: String,
    pub enhanced_content: Option<String>,
    pub folder_id: Option<i64>,
    pub transcript: Option<String>,
    pub participants: Option<String>,
    pub audio_duration_seconds: Option<f64>,
    /// Where the transcript was made: a [`TranscribedBy`] as JSON, kept raw
    /// and read leniently, as `participants` is (KNEG-368). `None` when it was
    /// never recorded -- notes from before then, typed notes, and OpenWhispr
    /// and shared-folder imports.
    pub transcribed_by: Option<String>,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
}

/// Where a note's transcript was made, as `Note::transcribed_by` stores it.
///
/// `provider` is the id the route reported (`local`, a registry id, a live
/// meeting provider's id, `self-hosted` or `custom`); `name` is what a person
/// reads. `live_name` is a meeting's live-caption provider when the saved
/// transcript was made on this computer and the captions were not.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TranscribedBy {
    pub provider: String,
    pub name: String,
    pub on_this_computer: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub live_name: Option<String>,
}

impl TranscribedBy {
    /// The JSON `Note::transcribed_by` holds.
    #[must_use]
    pub fn to_json(&self) -> Option<String> {
        serde_json::to_string(self).ok()
    }
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
    pub recording_details: Option<String>,
    pub note_type: Option<String>,
    pub folder_id: Option<i64>,
    pub transcript: Option<String>,
    pub participants: Option<String>,
    pub audio_duration_seconds: Option<f64>,
    /// [`TranscribedBy`] as JSON, for a note saved with a transcript.
    pub transcribed_by: Option<String>,
}

/// Create a note, returning its id.
pub async fn create(pool: &SqlitePool, note: &NewNote) -> Result<i64, StorageError> {
    let mut tx = pool.begin().await?;
    let id = create_on(&mut *tx, note).await?;
    crate::summary::enqueue_automatic(&mut tx, id).await?;
    tx.commit().await?;
    Ok(id)
}

async fn create_on<'e, E: sqlx::Executor<'e, Database = sqlx::Sqlite>>(
    pool: E,
    note: &NewNote,
) -> Result<i64, StorageError> {
    let title = if note.title.trim().is_empty() {
        "Untitled Note".to_owned()
    } else {
        note.title.clone()
    };

    let id: i64 = sqlx::query_scalar(
        "INSERT INTO notes
            (title, content, note_type, folder_id, transcript, participants,
             audio_duration_seconds, recording_details, transcribed_by)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
         RETURNING id",
    )
    .bind(&title)
    .bind(&note.content)
    .bind(note.note_type.as_deref().unwrap_or("personal"))
    .bind(note.folder_id)
    .bind(&note.transcript)
    .bind(&note.participants)
    .bind(note.audio_duration_seconds)
    .bind(&note.recording_details)
    .bind(&note.transcribed_by)
    .fetch_one(pool)
    .await?;

    Ok(id)
}

/// An imported note and recording become visible together.
pub async fn create_with_recording(
    pool: &SqlitePool,
    note: &NewNote,
    key: &str,
    duration: f64,
    original: &str,
) -> Result<(i64, crate::recordings::NoteRecording), StorageError> {
    create_with_recording_job(pool, note, key, duration, original, None).await
}

/// Save a recovered meeting and its application-owned WAV in one transaction.
/// A failed attachment must not leave a second, unattached note on retry.
pub async fn create_meeting_with_recording(
    pool: &SqlitePool,
    note: &NewNote,
    key: &str,
    duration: f64,
) -> Result<(i64, crate::recordings::NoteRecording), StorageError> {
    create_with_recording_on(pool, note, key, duration, None, None).await
}

/// A completed import and its durable job reference commit together.
pub async fn create_with_recording_job(
    pool: &SqlitePool,
    note: &NewNote,
    key: &str,
    duration: f64,
    original: &str,
    job: Option<&str>,
) -> Result<(i64, crate::recordings::NoteRecording), StorageError> {
    create_with_recording_on(pool, note, key, duration, Some(original), job).await
}

async fn create_with_recording_on(
    pool: &SqlitePool,
    note: &NewNote,
    key: &str,
    duration: f64,
    original: Option<&str>,
    job: Option<&str>,
) -> Result<(i64, crate::recordings::NoteRecording), StorageError> {
    let mut tx = pool.begin().await?;
    let id = create_on(&mut *tx, note).await?;
    crate::summary::enqueue_automatic(&mut tx, id).await?;
    let recording =
        crate::recordings::attach_on(&mut *tx, id, key, Some(duration), original).await?;
    if let Some(job) = job {
        let changed = sqlx::query("UPDATE import_jobs SET stage = 'succeeded', note_id = ?, updated_at = CURRENT_TIMESTAMP WHERE id = ? AND stage = 'saving'")
            .bind(id).bind(job).execute(&mut *tx).await?;
        if changed.rows_affected() != 1 {
            return Err(StorageError::Sqlx(sqlx::Error::RowNotFound));
        }
    }
    tx.commit().await?;
    Ok((id, recording))
}

/// Every column `Note` is read from. Crate-wide, so every reader of a whole
/// note -- `sharing::capture` too -- follows a column added here instead of
/// failing to decode it.
pub(crate) const COLUMNS: &str =
    "id, title, content, recording_details, note_type, enhanced_content, folder_id, transcript, \
                       participants, audio_duration_seconds, transcribed_by, created_at, updated_at";

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
#[derive(Debug, Clone, Serialize, PartialEq, TS)]
#[serde(rename_all = "camelCase")]
pub struct SearchHit {
    pub id: i64,
    pub title: String,
    /// A snippet with the match marked by `[` and `]`. The row's own when
    /// `transcript_row` names one, so the time and the words agree.
    pub snippet: String,
    /// Where in the transcript the words were said, when every word is in a
    /// meeting transcript and one row holds them all (KNEG-406). `None` when
    /// the notes hold every word, when the words are only in the title or are
    /// split between fields, and when no single row can be named: the note
    /// then opens as it always has, rather than at a guess. A title that also
    /// holds them doesn't stop a row being named.
    pub transcript_row: Option<SearchHitRow>,
}

/// The transcript row a search hit's words are in.
#[derive(Debug, Clone, Serialize, PartialEq, TS)]
#[serde(rename_all = "camelCase")]
pub struct SearchHitRow {
    /// Its position in the stored transcript: `TranscriptRow::index`.
    pub index: usize,
    /// Seconds from the start of the recording, as the Transcript tab shows
    /// it. `None` when the row has no time; it is still the row.
    pub start_seconds: Option<f64>,
}

/// A note-level hit before its row is looked for. `matched` is which column
/// the snippet came from: 1 the notes, 3 the transcript, -1 neither alone.
#[derive(FromRow)]
struct NoteHit {
    id: i64,
    title: String,
    snippet: String,
    matched: i64,
}

/// What a row's time is computed from: the fields `NoteMeta` needs, and the
/// stored transcript.
#[derive(FromRow)]
struct TranscriptSource {
    title: String,
    created_at: Option<String>,
    participants: Option<String>,
    transcript: Option<String>,
}

/// Each note's rows in `note_segments_fts` are the rowids
/// `id * SEGMENT_SPAN ..= id * SEGMENT_SPAN + SEGMENT_SPAN - 1`: the row's
/// position added to the note's base. The migration that builds it,
/// `0008_transcript_rows.sql`, writes 2^20 as 1048576.
const SEGMENT_SPAN: i64 = 1 << 20;

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

    // One read transaction, so the note index and the row index are read as
    // of the same moment.
    let mut tx = pool.begin().await?;
    let notes = sqlx::query_as::<_, NoteHit>(
        "SELECT notes.id AS id,
                notes.title AS title,
                snippet(notes_fts, CASE WHEN notes.id IN (SELECT rowid FROM notes_fts WHERE content MATCH ?1) THEN 1 WHEN notes.id IN (SELECT rowid FROM notes_fts WHERE transcript_text MATCH ?1) THEN 3 ELSE -1 END, '[', ']', '…', 24) AS snippet,
                CASE WHEN notes.id IN (SELECT rowid FROM notes_fts WHERE content MATCH ?1) THEN 1 WHEN notes.id IN (SELECT rowid FROM notes_fts WHERE transcript_text MATCH ?1) THEN 3 ELSE -1 END AS matched
         FROM notes_fts
         JOIN notes ON notes.id = notes_fts.rowid
         WHERE notes_fts MATCH ?1
         ORDER BY rank
         LIMIT ?2",
    )
    .bind(&escaped)
    .bind(limit)
    .fetch_all(&mut *tx)
    .await?;

    let mut hits = Vec::with_capacity(notes.len());
    for note in notes {
        // Only a hit whose words are all in the transcript: a hit in what the
        // person typed opens their notes, not a row.
        let row = if note.matched == 3 {
            transcript_row(&mut tx, &escaped, note.id).await?
        } else {
            None
        };
        let (transcript_row, snippet) = match row {
            Some((row, snippet)) => (Some(row), snippet),
            None => (None, note.snippet),
        };
        hits.push(SearchHit {
            id: note.id,
            title: note.title,
            snippet,
            transcript_row,
        });
    }
    tx.commit().await?;

    Ok(hits)
}

/// The row of note `id`'s transcript that holds every word of `escaped`, and
/// that row's snippet: the best match among its rows, the earlier on a tie.
/// `None` when no one row holds them all, and when the transcript can't be
/// read as rows -- the Transcript tab couldn't show the row either.
async fn transcript_row(
    connection: &mut SqliteConnection,
    escaped: &str,
    id: i64,
) -> Result<Option<(SearchHitRow, String)>, StorageError> {
    let Some(first) = id.checked_mul(SEGMENT_SPAN) else {
        return Ok(None);
    };
    let found: Option<(i64, String)> = sqlx::query_as(
        "SELECT rowid - ?2, snippet(note_segments_fts, 0, '[', ']', '…', 24)
         FROM note_segments_fts
         WHERE note_segments_fts MATCH ?1 AND rowid BETWEEN ?2 AND ?3
         ORDER BY rank, rowid
         LIMIT 1",
    )
    .bind(escaped)
    .bind(first)
    .bind(first + (SEGMENT_SPAN - 1))
    .fetch_optional(&mut *connection)
    .await?;
    let Some((position, snippet)) = found else {
        return Ok(None);
    };
    let Ok(index) = usize::try_from(position) else {
        return Ok(None);
    };

    // The time as the Transcript tab computes it, from the same parse and
    // the same rows, so the hit and the tab can't disagree about it.
    let note: Option<TranscriptSource> = sqlx::query_as(
        "SELECT title, created_at, participants, transcript FROM notes WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(&mut *connection)
    .await?;
    let Some(note) = note else {
        return Ok(None);
    };
    let Ok(segments) = transcript::parse_stored(note.transcript.as_deref()) else {
        return Ok(None);
    };
    let meta = NoteMeta {
        title: note.title,
        created_at: note.created_at,
        participants: note.participants,
    };
    let rows = transcript::rows(
        &segments,
        &meta,
        &BTreeMap::new(),
        &SpeakerLabels::default(),
    );
    let Some(row) = rows.get(index) else {
        return Ok(None);
    };

    Ok(Some((
        SearchHitRow {
            index,
            start_seconds: row.start_seconds,
        },
        snippet,
    )))
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

/// `folder_id` while that folder still exists, and `None` once it is gone.
///
/// For a folder chosen before the note it is for exists -- a meeting's,
/// chosen at Start and used at Stop (KNEG-365). `notes.folder_id` references
/// `folders` and the foreign key is enforced, so a note saved with a deleted
/// folder's id is refused outright rather than filed nowhere.
pub async fn existing_folder(
    pool: &SqlitePool,
    folder_id: Option<i64>,
) -> Result<Option<i64>, StorageError> {
    let Some(id) = folder_id else {
        return Ok(None);
    };
    Ok(sqlx::query_scalar("SELECT id FROM folders WHERE id = ?")
        .bind(id)
        .fetch_optional(pool)
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

/// Move a note into a folder, or out of every folder with `None`.
///
/// Separate from [`update`] because that call's `folder_id: None` means
/// "leave it alone" -- the right reading for a partial update and the wrong
/// one for "no folder", so a caller could never unfile a note through it.
/// `updated_at` is deliberately not touched: filing a note is not working on
/// it, and the sidebar orders by that column.
pub async fn set_folder(
    pool: &SqlitePool,
    id: i64,
    folder_id: Option<i64>,
) -> Result<bool, StorageError> {
    let result = sqlx::query("UPDATE notes SET folder_id = ? WHERE id = ?")
        .bind(folder_id)
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

    /* ------------------------------------------- where it was transcribed */

    #[tokio::test]
    async fn where_a_note_was_transcribed_is_saved_and_read_back() {
        // KNEG-368. Stored as written: the reader parses it, leniently.
        let db = db().await;
        let by = TranscribedBy {
            provider: "openai".to_owned(),
            name: "OpenAI".to_owned(),
            on_this_computer: false,
            live_name: None,
        };
        let id = create(
            db.pool(),
            &NewNote {
                title: "Interview".to_owned(),
                transcribed_by: by.to_json(),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let note = get(db.pool(), id).await.unwrap().unwrap();
        assert_eq!(
            note.transcribed_by.as_deref(),
            Some(r#"{"provider":"openai","name":"OpenAI","onThisComputer":false}"#)
        );
        assert_eq!(
            list(db.pool(), None, None, 10).await.unwrap()[0].transcribed_by,
            note.transcribed_by,
            "listed as it is got"
        );
    }

    #[tokio::test]
    async fn a_note_says_nothing_about_where_it_was_transcribed_unless_told() {
        assert_eq!(NewNote::default().transcribed_by, None);
        let db = db().await;
        let id = add(db.pool(), "Typed", "no transcript at all").await;
        assert_eq!(
            get(db.pool(), id).await.unwrap().unwrap().transcribed_by,
            None
        );
    }

    #[test]
    fn live_captions_are_named_only_when_there_were_some_to_name() {
        let local = TranscribedBy {
            provider: "local".to_owned(),
            name: "this computer".to_owned(),
            on_this_computer: true,
            live_name: None,
        };
        assert_eq!(
            local.to_json().as_deref(),
            Some(r#"{"provider":"local","name":"this computer","onThisComputer":true}"#)
        );
        let captioned = TranscribedBy {
            live_name: Some("Deepgram Realtime".to_owned()),
            ..local
        };
        let json = captioned.to_json().unwrap();
        assert_eq!(
            json,
            r#"{"provider":"local","name":"this computer","onThisComputer":true,"liveName":"Deepgram Realtime"}"#
        );
        assert_eq!(
            serde_json::from_str::<TranscribedBy>(&json).unwrap(),
            captioned
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
    async fn a_note_can_be_filed_and_unfiled() {
        let db = kw_storage::Db::open_in_memory().await.unwrap();
        let pool = db.pool();
        let folder = create_folder(pool, "Work").await.unwrap();
        let id = create(
            pool,
            &NewNote {
                title: "n".into(),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        assert!(set_folder(pool, id, Some(folder)).await.unwrap());
        assert_eq!(
            get(pool, id).await.unwrap().unwrap().folder_id,
            Some(folder)
        );

        // `update` cannot say "no folder" -- `None` there means leave it
        // alone -- which is the reason `set_folder` exists.
        assert!(set_folder(pool, id, None).await.unwrap());
        assert_eq!(get(pool, id).await.unwrap().unwrap().folder_id, None);

        assert!(!set_folder(pool, id + 100, Some(folder)).await.unwrap());
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

    #[tokio::test]
    async fn a_folder_chosen_earlier_is_kept_only_while_it_exists() {
        // KNEG-365: a meeting's folder is chosen at Start and used at Stop,
        // and `notes.folder_id` refuses an id whose folder was deleted between.
        let db = db().await;
        let p = db.pool();
        let kept = create_folder(p, "Customer calls").await.unwrap();
        let gone = create_folder(p, "Temporary").await.unwrap();
        assert!(delete_folder(p, gone).await.unwrap());

        assert_eq!(existing_folder(p, Some(kept)).await.unwrap(), Some(kept));
        assert_eq!(existing_folder(p, Some(gone)).await.unwrap(), None);
        assert_eq!(existing_folder(p, None).await.unwrap(), None);
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
    async fn swedish_letters_are_not_searched_as_their_plain_vowels() {
        // KNEG-405. å, ä and ö are letters of their own in Swedish, and folding
        // them collides on its commonest words: för/for, här/har, så/sa.
        let db = db().await;
        let p = db.pool();
        let har = add(p, "A", "Vi har ett möte").await;
        let har_upper = add(p, "B", "Vi ses HÄR").await;
        let for_sv = add(p, "C", "Detta är för dig").await;
        let for_en = add(p, "D", "This is for you").await;
        let sa = add(p, "E", "Hon sa nej").await;
        let sa_sv = add(p, "F", "Det var så").await;
        let in_transcript = create(
            p,
            &NewNote {
                title: "G".into(),
                transcript: Some(r#"[{"text":"Vi ses här i transkriptet","timestamp":0}]"#.into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        for (query, mut expected) in [
            ("har", vec![har]),
            ("här", vec![har_upper, in_transcript]),
            ("for", vec![for_en]),
            ("för", vec![for_sv]),
            ("sa", vec![sa]),
            ("så", vec![sa_sv]),
            ("möte", vec![har]),
            ("mote", vec![]),
        ] {
            let mut found: Vec<i64> = search(p, query, 10)
                .await
                .unwrap()
                .into_iter()
                .map(|hit| hit.id)
                .collect();
            found.sort_unstable();
            expected.sort_unstable();
            assert_eq!(found, expected, "{query:?}");
        }
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

    #[tokio::test]
    async fn empty_personal_notes_preserve_transcript_search_and_recording_details() {
        let db = db().await;
        let p = db.pool();
        let id = create(
            p,
            &NewNote {
                title: "Meeting".into(),
                transcript: Some(r#"[{"text":"Svenska lanseringen","timestamp":0}]"#.into()),
                recording_details: Some("Microphone was quiet".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let note = get(p, id).await.unwrap().unwrap();
        assert!(note.content.is_empty());
        assert_eq!(
            note.recording_details.as_deref(),
            Some("Microphone was quiet")
        );
        let hits = search(p, "lanseringen", 10).await.unwrap();
        assert_eq!(hits[0].id, id);
        assert!(hits[0].snippet.contains("[lanseringen]"));
        update(
            p,
            id,
            &NoteUpdate {
                content: Some("My lanseringen decision".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert!(search(p, "lanseringen", 10).await.unwrap()[0]
            .snippet
            .starts_with("My "));
        sqlx::query("UPDATE notes SET transcript=? WHERE id=?")
            .bind("Legacy elephant")
            .bind(id)
            .execute(p)
            .await
            .unwrap();
        assert!(search(p, "elephant", 10).await.unwrap()[0]
            .snippet
            .contains("[elephant]"));
        delete(p, id).await.unwrap();
        assert!(search(p, "elephant", 10).await.unwrap().is_empty());
    }

    /* ------------------------------------------ the row a transcript hit names */

    async fn meeting(pool: &SqlitePool, title: &str, transcript: &str) -> i64 {
        create(
            pool,
            &NewNote {
                title: title.into(),
                note_type: Some("meeting".into()),
                transcript: Some(transcript.into()),
                ..Default::default()
            },
        )
        .await
        .unwrap()
    }

    /// KNEG-406. A hit in a meeting transcript names the row its words are in
    /// and when they were said, in seconds from the start as the Transcript
    /// tab shows them -- epoch milliseconds normalised, not raw -- and its
    /// snippet is that row's, so the time and the words agree.
    #[tokio::test]
    async fn a_transcript_hit_names_its_row_and_when_it_was_said() {
        let db = db().await;
        let p = db.pool();
        let id = meeting(
            p,
            "Weekly",
            r#"[{"text":"Opening words","timestamp":1759000000000},
                {"text":"Then the budget was agreed","timestamp":1759000760000},
                {"text":"Closing words","timestamp":1759000900000}]"#,
        )
        .await;

        let hits = search(p, "budget", 10).await.unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].id, id);
        assert_eq!(
            hits[0].transcript_row,
            Some(SearchHitRow {
                index: 1,
                start_seconds: Some(760.0)
            })
        );
        assert_eq!(hits[0].snippet, "Then the [budget] was agreed");
    }

    /// The note matches because every word is in its transcript, but no one
    /// row holds them all. Naming a row that holds one of them would be a
    /// guess, so the hit opens the note as before.
    #[tokio::test]
    async fn words_said_in_different_rows_give_no_row() {
        let db = db().await;
        let p = db.pool();
        meeting(
            p,
            "Split",
            r#"[{"text":"the budget first","timestamp":0},{"text":"then the deadline","timestamp":5}]"#,
        )
        .await;

        let hits = search(p, "budget deadline", 10).await.unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].transcript_row, None);
    }

    /// A hit in what the person typed opens their notes, not a row.
    #[tokio::test]
    async fn a_body_match_opens_without_a_row() {
        let db = db().await;
        let p = db.pool();
        let id = meeting(p, "Typed", r#"[{"text":"the budget again","timestamp":3}]"#).await;
        update(
            p,
            id,
            &NoteUpdate {
                content: Some("My budget note".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let hits = search(p, "budget", 10).await.unwrap();
        assert_eq!(hits[0].transcript_row, None);
        assert!(hits[0].snippet.starts_with("My "), "{}", hits[0].snippet);
    }

    /// The row index has the transcript index's tokenizer (KNEG-405): "har"
    /// is not "här", so a search for one names the row with it, not the first
    /// row with either.
    #[tokio::test]
    async fn a_row_is_found_by_its_swedish_letters() {
        let db = db().await;
        let p = db.pool();
        meeting(
            p,
            "Svenska",
            r#"[{"text":"Vi har en plan","timestamp":0},{"text":"Vi ses här","timestamp":4}]"#,
        )
        .await;

        for (query, index) in [("här", 1), ("har", 0)] {
            let hits = search(p, query, 10).await.unwrap();
            assert_eq!(
                hits[0].transcript_row.as_ref().map(|row| row.index),
                Some(index),
                "{query:?}"
            );
        }
    }

    /// A transcript that isn't a list of segments has no rows, so a hit in it
    /// names none: plain text from an old import, and an array the Transcript
    /// tab can't read either.
    #[tokio::test]
    async fn a_legacy_or_unparseable_transcript_gives_no_row() {
        let db = db().await;
        let p = db.pool();
        meeting(p, "Legacy", "a plain elephant transcript").await;
        meeting(
            p,
            "Mixed",
            r#"[null,{"text":"a mixed giraffe","timestamp":1}]"#,
        )
        .await;

        for query in ["elephant", "giraffe"] {
            let hits = search(p, query, 10).await.unwrap();
            assert_eq!(hits.len(), 1, "{query:?}");
            assert_eq!(hits[0].transcript_row, None, "{query:?}");
        }
    }

    /// KNEG-408. Both search indexes read a segment's `text` alone, so a word
    /// held only in its timed `words` -- which a note never shows on its own
    /// -- finds nothing, and the words beside it change nothing about a hit.
    #[tokio::test]
    async fn timed_words_are_not_searched() {
        let db = db().await;
        let p = db.pool();
        let id = meeting(
            p,
            "Worded",
            r#"[{"text":"Hej då.","timestamp":1.0,"endTimestamp":2.0,
                 "words":[{"text":"Hej","start":1.0,"end":1.3},{"text":"giraffe","start":1.4,"end":1.8}]}]"#,
        )
        .await;

        assert!(search(p, "giraffe", 10).await.unwrap().is_empty());
        let hits = search(p, "Hej", 10).await.unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].id, id);
        assert_eq!(hits[0].snippet, "[Hej] då.");
        assert_eq!(
            hits[0].transcript_row,
            Some(SearchHitRow {
                index: 0,
                start_seconds: Some(1.0)
            })
        );
    }

    /// A row with no timestamp is still the row the words are in. It is named
    /// with no time rather than dropped or put at 0:00.
    #[tokio::test]
    async fn a_row_without_a_time_is_still_named() {
        let db = db().await;
        let p = db.pool();
        meeting(
            p,
            "Untimed",
            r#"[{"text":"first"},{"text":"the zebra crossing"}]"#,
        )
        .await;

        let hits = search(p, "zebra", 10).await.unwrap();
        assert_eq!(
            hits[0].transcript_row,
            Some(SearchHitRow {
                index: 1,
                start_seconds: None
            })
        );
    }
}
