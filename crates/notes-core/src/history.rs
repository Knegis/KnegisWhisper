//! Dictation history and the custom dictionary.
//!
//! Two small tables that the local API, the settings screen, and the dictation
//! session all read. Kept together because they share a property worth being
//! deliberate about: **both hold what the user said.**
//!
//! `transcriptions.text` is the transcript and `custom_dictionary.word` is
//! whatever vocabulary they had to teach it — names of colleagues, product
//! names, medical or legal terms. Neither is ever logged here, and
//! [`Transcription`] deliberately does not implement `Debug` in a way that would
//! print the text. Deleting means deleting: [`delete`] removes the row rather
//! than setting a flag, because a "deleted" transcript still on disk is not what
//! anyone means by deleted.

use serde::Serialize;
use sqlx::{Row, SqlitePool};

use kw_storage::StorageError;

/// One row of dictation history.
#[derive(Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Transcription {
    pub id: i64,
    /// What was inserted.
    pub text: String,
    /// What the model returned before cleanup, when cleanup changed it and the
    /// user asked to keep both.
    pub raw_text: Option<String>,
    pub created_at: Option<String>,
    /// Whether the recording was kept.
    pub has_audio: bool,
    pub audio_duration_ms: Option<i64>,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub status: String,
    pub error_message: Option<String>,
    pub error_code: Option<String>,
    pub route_kind: Option<String>,
}

impl std::fmt::Debug for Transcription {
    /// Shows the shape and never the words.
    ///
    /// A transcript reaches a log through anything that derives `Debug` and
    /// holds one of these, so this is the only place that can stop it. The
    /// length is safe and is what a diagnostic actually needs.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Transcription")
            .field("id", &self.id)
            .field("text_chars", &self.text.chars().count())
            .field("has_raw_text", &self.raw_text.is_some())
            .field("has_audio", &self.has_audio)
            .field("provider", &self.provider)
            .field("model", &self.model)
            .field("status", &self.status)
            .finish_non_exhaustive()
    }
}

/// A new history row.
#[derive(Debug, Clone, Default)]
pub struct NewTranscription {
    pub text: String,
    pub raw_text: Option<String>,
    pub has_audio: bool,
    pub audio_duration_ms: Option<i64>,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub route_kind: Option<String>,
}

/// Record a completed dictation.
pub async fn record(pool: &SqlitePool, entry: &NewTranscription) -> Result<i64, StorageError> {
    let row = sqlx::query(
        "INSERT INTO transcriptions \
         (text, raw_text, has_audio, audio_duration_ms, provider, model, status, route_kind) \
         VALUES (?, ?, ?, ?, ?, ?, 'completed', ?) RETURNING id",
    )
    .bind(&entry.text)
    .bind(&entry.raw_text)
    .bind(i64::from(entry.has_audio))
    .bind(entry.audio_duration_ms)
    .bind(&entry.provider)
    .bind(&entry.model)
    .bind(&entry.route_kind)
    .fetch_one(pool)
    .await?;

    Ok(row.try_get::<i64, _>("id")?)
}

/// One page of history, newest first.
///
/// `after` is a cursor: rows with an id below it. Ids are monotonic and
/// newest-first is id-descending, so this is a stable page even while new rows
/// arrive — which an offset would not be.
pub async fn list(
    pool: &SqlitePool,
    limit: u32,
    after: Option<i64>,
) -> Result<Vec<Transcription>, StorageError> {
    // One extra row is fetched by the caller's limit choice, not here; this
    // returns exactly what was asked for.
    let rows = match after {
        Some(after) => {
            sqlx::query("SELECT * FROM transcriptions WHERE id < ? ORDER BY id DESC LIMIT ?")
                .bind(after)
                .bind(i64::from(limit))
                .fetch_all(pool)
                .await?
        }
        None => {
            sqlx::query("SELECT * FROM transcriptions ORDER BY id DESC LIMIT ?")
                .bind(i64::from(limit))
                .fetch_all(pool)
                .await?
        }
    };

    rows.iter().map(hydrate).collect()
}

/// One history row.
pub async fn get(pool: &SqlitePool, id: i64) -> Result<Option<Transcription>, StorageError> {
    let row = sqlx::query("SELECT * FROM transcriptions WHERE id = ?")
        .bind(id)
        .fetch_optional(pool)
        .await?;

    row.as_ref().map(hydrate).transpose()
}

/// Delete a history row. Returns whether there was one.
pub async fn delete(pool: &SqlitePool, id: i64) -> Result<bool, StorageError> {
    let result = sqlx::query("DELETE FROM transcriptions WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}

/// Mark a row's audio as gone.
///
/// Only the flag: the file itself is the caller's to remove, because this crate
/// does not know where recordings are kept. Returning whether the row had audio
/// lets the caller tell "deleted it" from "there was nothing to delete", which
/// matters when the user asked for the recording to be destroyed.
pub async fn forget_audio(pool: &SqlitePool, id: i64) -> Result<AudioForgotten, StorageError> {
    let row = sqlx::query("SELECT has_audio FROM transcriptions WHERE id = ?")
        .bind(id)
        .fetch_optional(pool)
        .await?;

    let Some(row) = row else {
        return Ok(AudioForgotten::NoSuchRow);
    };

    if row.try_get::<i64, _>("has_audio")? == 0 {
        return Ok(AudioForgotten::NoAudioToForget);
    }

    sqlx::query("UPDATE transcriptions SET has_audio = 0, audio_duration_ms = NULL WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await?;

    Ok(AudioForgotten::Cleared)
}

/// What `forget_audio` did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioForgotten {
    Cleared,
    NoAudioToForget,
    NoSuchRow,
}

fn hydrate(row: &sqlx::sqlite::SqliteRow) -> Result<Transcription, StorageError> {
    Ok(Transcription {
        id: row.try_get("id")?,
        text: row.try_get("text")?,
        raw_text: row.try_get("raw_text")?,
        created_at: row.try_get("created_at")?,
        has_audio: row.try_get::<i64, _>("has_audio")? != 0,
        audio_duration_ms: row.try_get("audio_duration_ms")?,
        provider: row.try_get("provider")?,
        model: row.try_get("model")?,
        status: row.try_get("status")?,
        error_message: row.try_get("error_message")?,
        error_code: row.try_get("error_code")?,
        route_kind: row.try_get("route_kind")?,
    })
}

// ---------------------------------------------------------------- dictionary

/// One dictionary entry.
#[derive(Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DictionaryWord {
    pub id: i64,
    pub word: String,
    /// `manual`, or the feature that added it.
    pub source: String,
    pub created_at: Option<String>,
}

impl std::fmt::Debug for DictionaryWord {
    /// The words are the user's vocabulary — colleagues' names, product names,
    /// clinical terms. Not logged.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DictionaryWord")
            .field("id", &self.id)
            .field("word_chars", &self.word.chars().count())
            .field("source", &self.source)
            .finish_non_exhaustive()
    }
}

/// Every dictionary word, alphabetically.
pub async fn dictionary(pool: &SqlitePool) -> Result<Vec<DictionaryWord>, StorageError> {
    let rows = sqlx::query(
        "SELECT id, word, source, created_at FROM custom_dictionary \
         ORDER BY word COLLATE NOCASE ASC",
    )
    .fetch_all(pool)
    .await?;

    rows.iter()
        .map(|row| {
            Ok(DictionaryWord {
                id: row.try_get("id")?,
                word: row.try_get("word")?,
                source: row.try_get("source")?,
                created_at: row.try_get("created_at")?,
            })
        })
        .collect()
}

/// What a dictionary update changed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DictionaryChange {
    pub added: u32,
    /// Words that were already there. Reported rather than counted as added, so
    /// a caller retrying a request can tell nothing new happened.
    pub already_present: u32,
    pub removed: u32,
    /// Words asked to be removed that were not there.
    pub not_present: u32,
}

/// Add and remove dictionary words in one transaction.
///
/// One transaction because a partly applied update leaves the dictionary in a
/// state the caller did not ask for and cannot easily discover.
pub async fn update_dictionary(
    pool: &SqlitePool,
    add: &[String],
    remove: &[String],
) -> Result<DictionaryChange, StorageError> {
    let mut change = DictionaryChange::default();
    let mut tx = pool.begin().await?;

    for word in add {
        // `INSERT OR IGNORE` against the UNIQUE index, so a duplicate is
        // reported rather than being an error the caller has to interpret.
        let result = sqlx::query(
            "INSERT OR IGNORE INTO custom_dictionary (word, source) VALUES (?, 'manual')",
        )
        .bind(word)
        .execute(&mut *tx)
        .await?;

        if result.rows_affected() > 0 {
            change.added += 1;
        } else {
            change.already_present += 1;
        }
    }

    for word in remove {
        let result = sqlx::query("DELETE FROM custom_dictionary WHERE word = ?")
            .bind(word)
            .execute(&mut *tx)
            .await?;

        if result.rows_affected() > 0 {
            change.removed += 1;
        } else {
            change.not_present += 1;
        }
    }

    tx.commit().await?;
    Ok(change)
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
        Db::open_in_memory().await.expect("an in-memory database")
    }

    fn entry(text: &str) -> NewTranscription {
        NewTranscription {
            text: text.to_owned(),
            provider: Some("local".to_owned()),
            model: Some("tiny".to_owned()),
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn a_dictation_is_recorded_and_read_back() {
        let db = db().await;
        let id = record(db.pool(), &entry("hello there")).await.unwrap();

        let found = get(db.pool(), id).await.unwrap().expect("the row");
        assert_eq!(found.text, "hello there");
        assert_eq!(found.provider.as_deref(), Some("local"));
        assert_eq!(found.status, "completed");
        assert!(!found.has_audio);
    }

    #[tokio::test]
    async fn a_transcript_never_appears_in_debug_output() {
        // A transcript reaches a log through anything holding one of these.
        let db = db().await;
        let id = record(db.pool(), &entry("my private medical history"))
            .await
            .unwrap();
        let found = get(db.pool(), id).await.unwrap().unwrap();

        let rendered = format!("{found:?}");
        assert!(!rendered.contains("medical"), "{rendered}");
        assert!(rendered.contains("text_chars"));
    }

    #[tokio::test]
    async fn a_dictionary_word_never_appears_in_debug_output() {
        let word = DictionaryWord {
            id: 1,
            word: "Karolinska".to_owned(),
            source: "manual".to_owned(),
            created_at: None,
        };
        assert!(!format!("{word:?}").contains("Karolinska"));
    }

    #[tokio::test]
    async fn history_is_newest_first() {
        let db = db().await;
        for text in ["first", "second", "third"] {
            record(db.pool(), &entry(text)).await.unwrap();
        }

        let page = list(db.pool(), 10, None).await.unwrap();
        let texts: Vec<&str> = page.iter().map(|t| t.text.as_str()).collect();
        assert_eq!(texts, ["third", "second", "first"]);
    }

    #[tokio::test]
    async fn the_cursor_pages_stably_while_rows_are_added() {
        // The reason for a cursor rather than an offset: an offset would skip or
        // repeat rows as new dictations arrive between pages.
        let db = db().await;
        let mut ids = Vec::new();
        for text in ["a", "b", "c", "d"] {
            ids.push(record(db.pool(), &entry(text)).await.unwrap());
        }

        let first = list(db.pool(), 2, None).await.unwrap();
        assert_eq!(first.len(), 2);
        let cursor = first.last().map(|row| row.id).unwrap();

        // A new dictation arrives before the second page is asked for.
        record(db.pool(), &entry("e")).await.unwrap();

        let second = list(db.pool(), 2, Some(cursor)).await.unwrap();
        let texts: Vec<&str> = second.iter().map(|t| t.text.as_str()).collect();
        assert_eq!(texts, ["b", "a"], "no row skipped and none repeated");
    }

    #[tokio::test]
    async fn deleting_removes_the_row_rather_than_flagging_it() {
        // A "deleted" transcript still on disk is not what anyone means.
        let db = db().await;
        let id = record(db.pool(), &entry("forget me")).await.unwrap();

        assert!(delete(db.pool(), id).await.unwrap());
        assert!(get(db.pool(), id).await.unwrap().is_none());

        let remaining: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM transcriptions")
            .fetch_one(db.pool())
            .await
            .unwrap();
        assert_eq!(remaining, 0);
    }

    #[tokio::test]
    async fn deleting_something_absent_says_so_rather_than_failing() {
        let db = db().await;
        assert!(!delete(db.pool(), 999).await.unwrap());
    }

    #[tokio::test]
    async fn forgetting_audio_distinguishes_the_three_cases() {
        // "Deleted it", "there was nothing to delete", and "no such row" are
        // different answers, and the user asking for a recording to be destroyed
        // deserves the right one.
        let db = db().await;

        let with_audio = record(
            db.pool(),
            &NewTranscription {
                text: "spoken".to_owned(),
                has_audio: true,
                audio_duration_ms: Some(4200),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        assert_eq!(
            forget_audio(db.pool(), with_audio).await.unwrap(),
            AudioForgotten::Cleared
        );
        assert_eq!(
            forget_audio(db.pool(), with_audio).await.unwrap(),
            AudioForgotten::NoAudioToForget
        );
        assert_eq!(
            forget_audio(db.pool(), 999).await.unwrap(),
            AudioForgotten::NoSuchRow
        );

        let row = get(db.pool(), with_audio).await.unwrap().unwrap();
        assert!(!row.has_audio);
        assert_eq!(row.audio_duration_ms, None);
    }

    #[tokio::test]
    async fn dictionary_words_are_added_and_listed_alphabetically() {
        let db = db().await;
        let change =
            update_dictionary(db.pool(), &["whisper".to_owned(), "Knegis".to_owned()], &[])
                .await
                .unwrap();

        assert_eq!(change.added, 2);
        assert_eq!(change.already_present, 0);

        let entries = dictionary(db.pool()).await.unwrap();
        let words: Vec<&str> = entries.iter().map(|w| w.word.as_str()).collect();
        // NOCASE, so K sorts before w rather than after it.
        assert_eq!(words, ["Knegis", "whisper"]);
    }

    #[tokio::test]
    async fn adding_a_word_twice_is_reported_rather_than_being_an_error() {
        // A retried request must not fail, and must not claim it added something.
        let db = db().await;
        update_dictionary(db.pool(), &["Knegis".to_owned()], &[])
            .await
            .unwrap();

        let change = update_dictionary(db.pool(), &["Knegis".to_owned()], &[])
            .await
            .unwrap();
        assert_eq!(change.added, 0);
        assert_eq!(change.already_present, 1);
    }

    #[tokio::test]
    async fn removing_a_word_that_is_not_there_is_reported() {
        let db = db().await;
        let change = update_dictionary(db.pool(), &[], &["absent".to_owned()])
            .await
            .unwrap();
        assert_eq!(change.removed, 0);
        assert_eq!(change.not_present, 1);
    }

    #[tokio::test]
    async fn add_and_remove_happen_in_one_transaction() {
        let db = db().await;
        update_dictionary(db.pool(), &["old".to_owned()], &[])
            .await
            .unwrap();

        let change = update_dictionary(db.pool(), &["new".to_owned()], &["old".to_owned()])
            .await
            .unwrap();

        assert_eq!((change.added, change.removed), (1, 1));
        let words: Vec<String> = dictionary(db.pool())
            .await
            .unwrap()
            .into_iter()
            .map(|w| w.word)
            .collect();
        assert_eq!(words, ["new"]);
    }

    #[tokio::test]
    async fn an_empty_update_changes_nothing_and_reports_nothing() {
        let db = db().await;
        let change = update_dictionary(db.pool(), &[], &[]).await.unwrap();
        assert_eq!(change, DictionaryChange::default());
    }

    #[tokio::test]
    async fn a_transcript_is_serialised_with_camel_case_for_the_api() {
        let db = db().await;
        let id = record(
            db.pool(),
            &NewTranscription {
                text: "hello".to_owned(),
                has_audio: true,
                audio_duration_ms: Some(1000),
                route_kind: Some("dictation".to_owned()),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let row = get(db.pool(), id).await.unwrap().unwrap();
        let json = serde_json::to_value(&row).unwrap();
        assert_eq!(json["hasAudio"], true);
        assert_eq!(json["audioDurationMs"], 1000);
        assert_eq!(json["routeKind"], "dictation");
    }
}
