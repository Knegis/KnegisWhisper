//! The custom dictionary that biases recognition.
//!
//! Words the user had to teach the model: colleagues' names, product names,
//! clinical or legal terms. That makes this list a description of who someone
//! works with and what they work on, so it gets the same treatment as a
//! transcript — [`DictionaryWord`] implements `Debug` by hand and prints lengths
//! rather than words.
//!
//! Lives beside `transcriptions` because it is dictation, not notes, and because
//! the table's schema is in this crate.

use serde::Serialize;
use sqlx::{FromRow, SqlitePool};

use crate::StorageError;

/// One dictionary entry.
#[derive(Clone, FromRow, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DictionaryWord {
    pub id: i64,
    pub word: String,
    /// `manual`, or the feature that added it.
    pub source: String,
    pub created_at: Option<String>,
}

impl std::fmt::Debug for DictionaryWord {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DictionaryWord")
            .field("id", &self.id)
            .field("word_chars", &self.word.chars().count())
            .field("source", &self.source)
            .finish_non_exhaustive()
    }
}

/// Every word, alphabetically and case-insensitively.
pub async fn all(pool: &SqlitePool) -> Result<Vec<DictionaryWord>, StorageError> {
    Ok(sqlx::query_as::<_, DictionaryWord>(
        "SELECT id, word, source, created_at FROM custom_dictionary \
         ORDER BY word COLLATE NOCASE ASC",
    )
    .fetch_all(pool)
    .await?)
}

/// Every word as plain strings, for building a recognition prompt.
pub async fn words(pool: &SqlitePool) -> Result<Vec<String>, StorageError> {
    Ok(
        sqlx::query_scalar("SELECT word FROM custom_dictionary ORDER BY word COLLATE NOCASE ASC")
            .fetch_all(pool)
            .await?,
    )
}

/// What an update changed.
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

/// Add and remove words in one transaction.
///
/// One transaction because a partly applied update leaves the dictionary in a
/// state the caller did not ask for and cannot easily discover.
pub async fn update(
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
    use crate::Db;

    async fn db() -> Db {
        Db::open_in_memory().await.unwrap()
    }

    fn owned(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| (*s).to_owned()).collect()
    }

    #[tokio::test]
    async fn words_are_added_and_listed_case_insensitively() {
        let db = db().await;
        let change = update(db.pool(), &owned(&["whisper", "Knegis"]), &[])
            .await
            .unwrap();
        assert_eq!(change.added, 2);

        // NOCASE, so K sorts before w rather than after it.
        assert_eq!(words(db.pool()).await.unwrap(), ["Knegis", "whisper"]);
    }

    #[tokio::test]
    async fn adding_twice_is_reported_rather_than_being_an_error() {
        // A retried request must not fail, and must not claim it added anything.
        let db = db().await;
        update(db.pool(), &owned(&["Knegis"]), &[]).await.unwrap();

        let change = update(db.pool(), &owned(&["Knegis"]), &[]).await.unwrap();
        assert_eq!(change.added, 0);
        assert_eq!(change.already_present, 1);
    }

    #[tokio::test]
    async fn removing_something_absent_is_reported() {
        let db = db().await;
        let change = update(db.pool(), &[], &owned(&["absent"])).await.unwrap();
        assert_eq!(change.removed, 0);
        assert_eq!(change.not_present, 1);
    }

    #[tokio::test]
    async fn add_and_remove_happen_together() {
        let db = db().await;
        update(db.pool(), &owned(&["old"]), &[]).await.unwrap();

        let change = update(db.pool(), &owned(&["new"]), &owned(&["old"]))
            .await
            .unwrap();
        assert_eq!((change.added, change.removed), (1, 1));
        assert_eq!(words(db.pool()).await.unwrap(), ["new"]);
    }

    #[tokio::test]
    async fn an_empty_update_changes_nothing() {
        let db = db().await;
        assert_eq!(
            update(db.pool(), &[], &[]).await.unwrap(),
            DictionaryChange::default()
        );
    }

    #[tokio::test]
    async fn a_word_never_appears_in_debug_output() {
        // The list describes who someone works with and what they work on.
        let db = db().await;
        update(db.pool(), &owned(&["Karolinska"]), &[])
            .await
            .unwrap();

        let rendered = format!("{:?}", all(db.pool()).await.unwrap()[0]);
        assert!(!rendered.contains("Karolinska"), "{rendered}");
        assert!(rendered.contains("word_chars"));
    }

    #[tokio::test]
    async fn an_entry_serialises_in_camel_case() {
        let db = db().await;
        update(db.pool(), &owned(&["Knegis"]), &[]).await.unwrap();

        let json = serde_json::to_value(&all(db.pool()).await.unwrap()[0]).unwrap();
        assert_eq!(json["word"], "Knegis");
        assert!(json.get("createdAt").is_some());
    }
}
