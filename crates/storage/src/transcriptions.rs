//! Dictation history.
//!
//! One row per dictation attempt, including the failures — a transcription that
//! errored is more useful in the history than absent from it, because it is the
//! only way the user can retry or see why nothing was inserted.
//!
//! Two privacy rules are enforced here rather than left to callers:
//!
//! * History can be switched off. When it is, [`record`] is not called at all;
//!   nothing writes a row "just in case".
//! * `raw_text` — the transcript before cleanup — is stored only when the caller
//!   passes it. The default is not to keep it.

use serde::{Deserialize, Serialize};
use sqlx::{FromRow, SqlitePool};

use crate::StorageError;

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
#[derive(Debug, Clone, FromRow, Serialize)]
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

/// The most recent entries, newest first. `limit` is clamped to 500 so a
/// frontend cannot ask for the entire table.
pub async fn recent(pool: &SqlitePool, limit: i64) -> Result<Vec<Transcription>, StorageError> {
    let limit = limit.clamp(1, 500);
    let rows = sqlx::query_as::<_, Transcription>(
        "SELECT id, text, raw_text, created_at, has_audio, audio_duration_ms,
                provider, model, status, error_message, error_code, route_kind
         FROM transcriptions
         ORDER BY id DESC
         LIMIT ?",
    )
    .bind(limit)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// Delete one entry. Returns whether a row was removed.
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

/// Mark the audio for an entry as no longer retained.
///
/// Called after the temporary audio file is deleted, so `has_audio` never claims
/// a file that is gone.
pub async fn forget_audio(pool: &SqlitePool, id: i64) -> Result<(), StorageError> {
    sqlx::query("UPDATE transcriptions SET has_audio = 0 WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
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

        let rows = recent(p, 10).await.unwrap();
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

        let rows = recent(db.pool(), 1).await.unwrap();
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
        assert_eq!(recent(db.pool(), 1).await.unwrap()[0].raw_text, None);

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
            recent(db.pool(), 1).await.unwrap()[0].raw_text.as_deref(),
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
        assert_eq!(recent(db.pool(), i64::MAX).await.unwrap().len(), 5);
        assert_eq!(recent(db.pool(), 0).await.unwrap().len(), 1);
        assert_eq!(recent(db.pool(), -10).await.unwrap().len(), 1);
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
    async fn forget_audio_clears_the_flag_so_it_cannot_claim_a_deleted_file() {
        let db = db().await;
        let id = record(
            db.pool(),
            &NewTranscription {
                text: "x".to_owned(),
                has_audio: true,
                ..Default::default()
            },
            Status::Completed,
        )
        .await
        .unwrap();
        assert!(recent(db.pool(), 1).await.unwrap()[0].has_audio);

        forget_audio(db.pool(), id).await.unwrap();
        assert!(!recent(db.pool(), 1).await.unwrap()[0].has_audio);
    }
}
