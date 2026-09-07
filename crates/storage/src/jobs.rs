//! Durable import metadata. Audio and transcripts stay in their existing stores.
use crate::StorageError;
use serde::Serialize;
use sqlx::{FromRow, SqlitePool};
#[derive(Debug, Clone, Serialize, FromRow)]
pub struct Job {
    pub id: String,
    pub stage: String,
    pub created_at: String,
    pub updated_at: String,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub note_id: Option<i64>,
    pub error: Option<String>,
}
pub async fn create(pool: &SqlitePool, id: &str) -> Result<(), StorageError> {
    sqlx::query("INSERT INTO import_jobs (id, stage) VALUES (?, 'preparing')")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}
pub async fn get(pool: &SqlitePool, id: &str) -> Result<Option<Job>, StorageError> {
    Ok(sqlx::query_as("SELECT * FROM import_jobs WHERE id = ?")
        .bind(id)
        .fetch_optional(pool)
        .await?)
}
pub async fn stage(
    pool: &SqlitePool,
    id: &str,
    stage: &str,
    error: Option<&str>,
) -> Result<(), StorageError> {
    sqlx::query("UPDATE import_jobs SET stage = ?, error = ?, updated_at = CURRENT_TIMESTAMP WHERE id = ? AND stage NOT IN ('succeeded','failed','cancelled')")
        .bind(stage).bind(error).bind(id).execute(pool).await?;
    Ok(())
}
pub async fn route(
    pool: &SqlitePool,
    id: &str,
    provider: &str,
    model: Option<&str>,
) -> Result<(), StorageError> {
    sqlx::query("UPDATE import_jobs SET provider = ?, model = ?, updated_at = CURRENT_TIMESTAMP WHERE id = ?")
        .bind(provider).bind(model).bind(id).execute(pool).await?;
    Ok(())
}
pub async fn interrupt_unfinished(pool: &SqlitePool) -> Result<(), StorageError> {
    sqlx::query("UPDATE import_jobs SET stage = 'failed', error = 'The app stopped before this job completed. Submit it again to retry.', updated_at = CURRENT_TIMESTAMP WHERE stage NOT IN ('succeeded','failed','cancelled')")
        .execute(pool).await?;
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn restart_preserves_terminal_jobs_and_interrupts_unfinished_jobs() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("jobs.db");
        let db = crate::Db::open(&path).await.unwrap();
        for (id, state) in [
            ("complete", "succeeded"),
            ("cancelled", "cancelled"),
            ("active", "transcribing"),
        ] {
            create(db.pool(), id).await.unwrap();
            stage(db.pool(), id, state, None).await.unwrap();
        }
        db.close().await;
        let db = crate::Db::open(&path).await.unwrap();
        interrupt_unfinished(db.pool()).await.unwrap();
        assert_eq!(
            get(db.pool(), "complete").await.unwrap().unwrap().stage,
            "succeeded"
        );
        assert_eq!(
            get(db.pool(), "cancelled").await.unwrap().unwrap().stage,
            "cancelled"
        );
        let interrupted = get(db.pool(), "active").await.unwrap().unwrap();
        assert_eq!(interrupted.stage, "failed");
        assert!(interrupted.error.unwrap().contains("Submit it again"));
        db.close().await;
    }

    #[tokio::test]
    async fn automation_migration_indexes_existing_final_text_only() {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::raw_sql(include_str!("../migrations/0001_initial.sql"))
            .execute(&pool)
            .await
            .unwrap();
        sqlx::raw_sql(include_str!("../migrations/0002_note_recordings.sql"))
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO transcriptions (text, raw_text) VALUES ('Swedish möte', 'privateoriginal')")
            .execute(&pool).await.unwrap();
        sqlx::raw_sql(include_str!("../migrations/0003_automation.sql"))
            .execute(&pool)
            .await
            .unwrap();
        for (query, expected) in [("möte", 1_i64), ("privateoriginal", 0_i64)] {
            let count: i64 = sqlx::query_scalar(
                "SELECT count(*) FROM transcriptions_fts WHERE transcriptions_fts MATCH ?",
            )
            .bind(query)
            .fetch_one(&pool)
            .await
            .unwrap();
            assert_eq!(count, expected);
        }
    }
}
