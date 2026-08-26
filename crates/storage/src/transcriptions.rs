//! Dictation history.
//!
//! One row per dictation attempt, including the failures — a transcription that
//! errored is more useful in the history than absent from it, because it is the
//! only way the user can retry or see why nothing was inserted.
//!
//! Three rules are enforced here rather than left to callers:
//!
//! * History can be switched off. When it is, [`record`] is not called at all;
//!   nothing writes a row "just in case".
//! * `raw_text` — the transcript before cleanup — is stored only when the caller
//!   passes it. The default is not to keep it.
//! * **A transcript never reaches a log.** [`Transcription`] implements `Debug`
//!   by hand and prints lengths rather than words, because a transcript
//!   otherwise escapes through anything that derives `Debug` and holds one.
//!
//! ## One owner per table
//!
//! This module and a `notes-core::history` briefly both read and wrote this
//! table, each with its own row type, one used by the Tauri commands and the
//! other by the local API. They have been merged here: the schema and
//! `settings.rs` already live in this crate, and `notes-core` should stay about
//! notes. Two types for one table is how a column comes to mean two things.

use serde::{Deserialize, Serialize};
use sqlx::{FromRow, SqlitePool};
use ts_rs::TS;

use crate::StorageError;

/// The largest page anything may ask for.
///
/// A caller asking for everything would have this process hold the whole table
/// in memory to serialise it.
pub const MAX_LIMIT: i64 = 500;

/// How a dictation ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Status {
    Completed,
    Failed,
    Cancelled,
}

impl Status {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }
}

/// A new history entry.
#[derive(Debug, Clone, Default)]
pub struct NewTranscription {
    /// The text that was inserted, after any cleanup.
    pub text: String,
    /// The transcript before cleanup, if the user asked to keep it.
    pub raw_text: Option<String>,
    pub has_audio: bool,
    pub audio_duration_ms: Option<i64>,
    /// Provider id, e.g. `berget`. Never a credential.
    pub provider: Option<String>,
    pub model: Option<String>,
    pub error_message: Option<String>,
    pub error_code: Option<String>,
    /// The dictation intent, so a retry re-runs the same route.
    pub route_kind: Option<String>,
}

/// A stored history entry.
///
/// Serialised in camelCase because that is what `apps/desktop/src/lib/ipc.ts`
/// declares. They disagreed once — the struct had no `rename_all`, so it sent
/// `error_message` where the UI read `errorMessage`, and a failed dictation
/// rendered as "(empty)" instead of its reason.
#[derive(Clone, FromRow, Serialize, PartialEq, Eq, TS)]
#[serde(rename_all = "camelCase")]
pub struct Transcription {
    pub id: i64,
    pub text: String,
    pub raw_text: Option<String>,
    pub created_at: Option<String>,
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
    /// Shape, never words. See the module comment.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Transcription")
            .field("id", &self.id)
            .field("text_chars", &self.text.chars().count())
            .field("has_raw_text", &self.raw_text.is_some())
            .field("has_audio", &self.has_audio)
            .field("provider", &self.provider)
            .field("model", &self.model)
            .field("status", &self.status)
            .field("error_code", &self.error_code)
            .finish_non_exhaustive()
    }
}

/// Every column, in one place, so the two queries cannot select different sets.
const COLUMNS: &str = "id, text, raw_text, created_at, has_audio, audio_duration_ms, \
                       provider, model, status, error_message, error_code, route_kind";

/// Insert a history entry, returning its id.
pub async fn record(
    pool: &SqlitePool,
    entry: &NewTranscription,
    status: Status,
) -> Result<i64, StorageError> {
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO transcriptions
            (text, raw_text, has_audio, audio_duration_ms, provider, model,
             status, error_message, error_code, route_kind)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
         RETURNING id",
    )
    .bind(&entry.text)
    .bind(&entry.raw_text)
    .bind(i64::from(entry.has_audio))
    .bind(entry.audio_duration_ms)
    .bind(&entry.provider)
    .bind(&entry.model)
    .bind(status.as_str())
    .bind(&entry.error_message)
    .bind(&entry.error_code)
    .bind(&entry.route_kind)
    .fetch_one(pool)
    .await?;

    Ok(id)
}

/// The most recent entries, newest first.
///
/// `after` is a cursor: rows with an id below it. Ids are monotonic and
/// newest-first is id-descending, so a page stays stable while new dictations
/// arrive — which an offset would not. `limit` is clamped rather than trusted,
/// because it arrives from a frontend or from an HTTP query.
pub async fn recent(
    pool: &SqlitePool,
    limit: i64,
    after: Option<i64>,
) -> Result<Vec<Transcription>, StorageError> {
    let limit = limit.clamp(1, MAX_LIMIT);

    let rows = match after {
        Some(after) => {
            let sql = format!(
                "SELECT {COLUMNS} FROM transcriptions WHERE id < ? ORDER BY id DESC LIMIT ?"
            );
            sqlx::query_as::<_, Transcription>(&sql)
                .bind(after)
                .bind(limit)
                .fetch_all(pool)
                .await?
        }
        None => {
            let sql = format!("SELECT {COLUMNS} FROM transcriptions ORDER BY id DESC LIMIT ?");
            sqlx::query_as::<_, Transcription>(&sql)
                .bind(limit)
                .fetch_all(pool)
                .await?
        }
    };

    Ok(rows)
}

/// One entry.
pub async fn get(pool: &SqlitePool, id: i64) -> Result<Option<Transcription>, StorageError> {
    let sql = format!("SELECT {COLUMNS} FROM transcriptions WHERE id = ?");
    Ok(sqlx::query_as::<_, Transcription>(&sql)
        .bind(id)
        .fetch_optional(pool)
        .await?)
}

/// Delete one entry. Returns whether a row was removed.
///
/// Deleting means deleting: the row goes, rather than gaining a flag. A
/// "deleted" transcript still on disk is not what anyone means by deleted.
pub async fn delete(pool: &SqlitePool, id: i64) -> Result<bool, StorageError> {
    let result = sqlx::query("DELETE FROM transcriptions WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}

/// Delete every entry. Returns how many were removed.
pub async fn clear(pool: &SqlitePool) -> Result<u64, StorageError> {
    let result = sqlx::query("DELETE FROM transcriptions")
        .execute(pool)
        .await?;
    Ok(result.rows_affected())
}

/// What [`forget_audio`] did.
///
/// Three outcomes rather than a `()`, because "deleted it", "there was nothing
/// to delete", and "no such entry" are different answers, and someone asking for
/// a recording to be destroyed deserves the right one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioForgotten {
    Cleared,
    NoAudioToForget,
    NoSuchRow,
}

/// Mark the audio for an entry as no longer retained.
///
/// Only the flag: removing the file is the caller's job, because this crate does
/// not know where recordings are kept. Called *after* the file is gone, so
/// `has_audio` never claims a file that is not there.
pub async fn forget_audio(pool: &SqlitePool, id: i64) -> Result<AudioForgotten, StorageError> {
    let existing: Option<i64> =
        sqlx::query_scalar("SELECT has_audio FROM transcriptions WHERE id = ?")
            .bind(id)
            .fetch_optional(pool)
            .await?;

    let Some(has_audio) = existing else {
        return Ok(AudioForgotten::NoSuchRow);
    };
    if has_audio == 0 {
        return Ok(AudioForgotten::NoAudioToForget);
    }

    sqlx::query("UPDATE transcriptions SET has_audio = 0, audio_duration_ms = NULL WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await?;

    Ok(AudioForgotten::Cleared)
}

/// How many entries exist.
pub async fn count(pool: &SqlitePool) -> Result<i64, StorageError> {
    Ok(sqlx::query_scalar("SELECT count(*) FROM transcriptions")
        .fetch_one(pool)
        .await?)
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
    use crate::Db;

    async fn db() -> Db {
        Db::open_in_memory().await.unwrap()
    }

    fn entry(text: &str) -> NewTranscription {
        NewTranscription {
            text: text.to_owned(),
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn records_and_reads_back_newest_first() {
        let db = db().await;
        let p = db.pool();

        record(p, &entry("first"), Status::Completed).await.unwrap();
        record(p, &entry("second"), Status::Completed)
            .await
            .unwrap();
        record(p, &entry("third"), Status::Completed).await.unwrap();

        let rows = recent(p, 10, None).await.unwrap();
        assert_eq!(
            rows.iter().map(|r| r.text.as_str()).collect::<Vec<_>>(),
            vec!["third", "second", "first"]
        );
        assert_eq!(count(p).await.unwrap(), 3);
    }

    #[tokio::test]
    async fn a_failure_is_recorded_with_its_reason() {
        let db = db().await;
        let id = record(
            db.pool(),
            &NewTranscription {
                text: String::new(),
                error_message: Some("provider returned 429".to_owned()),
                error_code: Some("rate_limited".to_owned()),
                provider: Some("openai".to_owned()),
                ..Default::default()
            },
            Status::Failed,
        )
        .await
        .unwrap();

        let rows = recent(db.pool(), 1, None).await.unwrap();
        assert_eq!(rows[0].id, id);
        assert_eq!(rows[0].status, "failed");
        assert_eq!(rows[0].error_code.as_deref(), Some("rate_limited"));
    }

    #[tokio::test]
    async fn raw_text_is_absent_unless_asked_for() {
        let db = db().await;
        record(db.pool(), &entry("cleaned up"), Status::Completed)
            .await
            .unwrap();
        assert_eq!(recent(db.pool(), 1, None).await.unwrap()[0].raw_text, None);

        record(
            db.pool(),
            &NewTranscription {
                text: "cleaned up".to_owned(),
                raw_text: Some("uh cleaned up".to_owned()),
                ..Default::default()
            },
            Status::Completed,
        )
        .await
        .unwrap();
        assert_eq!(
            recent(db.pool(), 1, None).await.unwrap()[0]
                .raw_text
                .as_deref(),
            Some("uh cleaned up")
        );
    }

    #[tokio::test]
    async fn limit_is_clamped_rather_than_trusted() {
        let db = db().await;
        for i in 0..5 {
            record(db.pool(), &entry(&format!("n{i}")), Status::Completed)
                .await
                .unwrap();
        }
        // A frontend asking for everything, or for nothing, gets something sane.
        assert_eq!(recent(db.pool(), i64::MAX, None).await.unwrap().len(), 5);
        assert_eq!(recent(db.pool(), 0, None).await.unwrap().len(), 1);
        assert_eq!(recent(db.pool(), -10, None).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn the_cursor_pages_stably_while_rows_are_added() {
        // The reason for a cursor rather than an offset: an offset skips or
        // repeats rows as new dictations arrive between pages.
        let db = db().await;
        for text in ["a", "b", "c", "d"] {
            record(db.pool(), &entry(text), Status::Completed)
                .await
                .unwrap();
        }

        let first = recent(db.pool(), 2, None).await.unwrap();
        assert_eq!(first.len(), 2);
        let cursor = first.last().map(|row| row.id).unwrap();

        record(db.pool(), &entry("e"), Status::Completed)
            .await
            .unwrap();

        let second = recent(db.pool(), 2, Some(cursor)).await.unwrap();
        assert_eq!(
            second.iter().map(|r| r.text.as_str()).collect::<Vec<_>>(),
            vec!["b", "a"],
            "no row skipped and none repeated"
        );
    }

    #[tokio::test]
    async fn one_entry_can_be_read_by_id() {
        let db = db().await;
        let id = record(db.pool(), &entry("hello"), Status::Completed)
            .await
            .unwrap();
        assert_eq!(get(db.pool(), id).await.unwrap().unwrap().text, "hello");
        assert!(get(db.pool(), 999).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn delete_and_clear() {
        let db = db().await;
        let p = db.pool();
        let id = record(p, &entry("x"), Status::Completed).await.unwrap();
        record(p, &entry("y"), Status::Completed).await.unwrap();

        assert!(delete(p, id).await.unwrap());
        assert!(
            !delete(p, id).await.unwrap(),
            "deleting twice is not a second removal"
        );
        assert_eq!(count(p).await.unwrap(), 1);

        assert_eq!(clear(p).await.unwrap(), 1);
        assert_eq!(count(p).await.unwrap(), 0);
    }

    #[tokio::test]
    async fn forgetting_audio_distinguishes_the_three_cases() {
        let db = db().await;
        let id = record(
            db.pool(),
            &NewTranscription {
                text: "x".to_owned(),
                has_audio: true,
                audio_duration_ms: Some(4200),
                ..Default::default()
            },
            Status::Completed,
        )
        .await
        .unwrap();
        assert!(recent(db.pool(), 1, None).await.unwrap()[0].has_audio);

        assert_eq!(
            forget_audio(db.pool(), id).await.unwrap(),
            AudioForgotten::Cleared
        );
        assert_eq!(
            forget_audio(db.pool(), id).await.unwrap(),
            AudioForgotten::NoAudioToForget
        );
        assert_eq!(
            forget_audio(db.pool(), 999).await.unwrap(),
            AudioForgotten::NoSuchRow
        );

        let row = &recent(db.pool(), 1, None).await.unwrap()[0];
        assert!(!row.has_audio);
        assert_eq!(row.audio_duration_ms, None);
    }

    #[tokio::test]
    async fn a_transcript_never_appears_in_debug_output() {
        // A transcript reaches a log through anything that derives Debug and
        // holds one of these, so this is the only place that can stop it.
        let db = db().await;
        record(
            db.pool(),
            &entry("my private medical history"),
            Status::Completed,
        )
        .await
        .unwrap();

        let rendered = format!("{:?}", recent(db.pool(), 1, None).await.unwrap()[0]);
        assert!(!rendered.contains("medical"), "{rendered}");
        assert!(rendered.contains("text_chars"));
    }

    #[tokio::test]
    async fn the_json_keys_are_the_ones_the_frontend_declares() {
        // They disagreed once: no rename_all here meant `error_message` on the
        // wire against `errorMessage` in ipc.ts, so a failed dictation rendered
        // as "(empty)" rather than its reason.
        let db = db().await;
        record(
            db.pool(),
            &NewTranscription {
                text: String::new(),
                has_audio: true,
                audio_duration_ms: Some(1000),
                error_message: Some("provider returned 429".to_owned()),
                error_code: Some("rate_limited".to_owned()),
                route_kind: Some("dictation".to_owned()),
                raw_text: Some("uh".to_owned()),
                ..Default::default()
            },
            Status::Failed,
        )
        .await
        .unwrap();

        let row = &recent(db.pool(), 1, None).await.unwrap()[0];
        let json = serde_json::to_value(row).unwrap();

        for key in [
            "rawText",
            "createdAt",
            "hasAudio",
            "audioDurationMs",
            "errorMessage",
            "errorCode",
            "routeKind",
        ] {
            assert!(json.get(key).is_some(), "{key} missing from {json}");
        }
        assert_eq!(json["errorMessage"], "provider returned 429");
    }
}
