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

/// Shared desktop and API normalization, preserving preferred casing.
pub fn normalize(word: &str) -> String {
    word.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub fn validate_word(word: &str) -> Result<String, StorageError> {
    let word = normalize(word);
    if word.is_empty() || word.chars().count() > 100 {
        return Err(StorageError::Import(
            "Vocabulary terms must contain 1-100 characters".into(),
        ));
    }
    Ok(word)
}

/// One dictionary entry.
#[derive(Clone, FromRow, Serialize, ts_rs::TS, PartialEq, Eq)]
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
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, ts_rs::TS)]
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
    let add = add
        .iter()
        .map(|w| validate_word(w))
        .collect::<Result<Vec<_>, _>>()?;
    let mut change = DictionaryChange::default();
    let mut tx = pool.begin().await?;

    let existing: Vec<String> = sqlx::query_scalar("SELECT word FROM custom_dictionary")
        .fetch_all(&mut *tx)
        .await?;
    let mut known: std::collections::HashSet<String> =
        existing.iter().map(|w| normalize(w)).collect();
    for word in &add {
        if !known.insert(word.clone()) {
            change.already_present += 1;
            continue;
        }
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

/// Merge legacy meeting terms exactly once, in the same transaction as the marker.
pub async fn migrate_vocabulary(pool: &SqlitePool) -> Result<(), StorageError> {
    let mut tx = pool.begin().await?;
    let done: Option<String> =
        sqlx::query_scalar("SELECT value FROM settings WHERE key = 'vocabulary.migrated'")
            .fetch_optional(&mut *tx)
            .await?;
    if done.is_some() {
        tx.rollback().await?;
        return Ok(());
    }
    let raw: Option<String> =
        sqlx::query_scalar("SELECT value FROM settings WHERE key = 'meetings.keyterms'")
            .fetch_optional(&mut *tx)
            .await?;
    if raw.is_none() {
        tx.rollback().await?;
        return Ok(());
    }
    let legacy: Vec<String> = raw
        .map(|value| serde_json::from_str(&value))
        .transpose()
        .map_err(|_| StorageError::Import("Could not migrate meeting vocabulary".into()))?
        .unwrap_or_default();
    let existing: Vec<String> = sqlx::query_scalar("SELECT word FROM custom_dictionary")
        .fetch_all(&mut *tx)
        .await?;
    let mut seen: std::collections::HashSet<String> =
        existing.iter().map(|w| normalize(w)).collect();
    for word in legacy {
        let word = normalize(&word);
        if !word.is_empty() && seen.insert(word.clone()) {
            sqlx::query("INSERT OR IGNORE INTO custom_dictionary (word, source) VALUES (?, 'meeting-keyterms')")
                .bind(word).execute(&mut *tx).await?;
        }
    }
    sqlx::query("INSERT INTO settings (key, value) VALUES ('vocabulary.migrated', 'true')")
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(())
}

/// Editing preserves identity and provenance; a duplicate is a visible error.
pub async fn edit(pool: &SqlitePool, id: i64, word: &str) -> Result<(), StorageError> {
    let word = validate_word(word)?;
    let mut tx = pool.begin().await?;
    let others: Vec<String> =
        sqlx::query_scalar("SELECT word FROM custom_dictionary WHERE id != ?")
            .bind(id)
            .fetch_all(&mut *tx)
            .await?;
    if others.iter().any(|other| normalize(other) == word) {
        return Err(StorageError::Import(
            "Vocabulary entry already exists".into(),
        ));
    }
    let result = sqlx::query(
        "UPDATE custom_dictionary SET word = ?, updated_at = CURRENT_TIMESTAMP WHERE id = ?",
    )
    .bind(word)
    .bind(id)
    .execute(&mut *tx)
    .await?;
    if result.rows_affected() == 0 {
        return Err(StorageError::Import(
            "Vocabulary entry no longer exists".into(),
        ));
    }
    tx.commit().await?;
    Ok(())
}

/// Delete by identity and return the original entry for a session-scoped undo.
pub async fn delete(pool: &SqlitePool, id: i64) -> Result<DictionaryWord, StorageError> {
    let mut tx = pool.begin().await?;
    let word: DictionaryWord =
        sqlx::query_as("SELECT id, word, source, created_at FROM custom_dictionary WHERE id = ?")
            .bind(id)
            .fetch_one(&mut *tx)
            .await?;
    sqlx::query("DELETE FROM custom_dictionary WHERE id = ?")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(word)
}

/// Restore only a previously stored entry; legacy over-limit terms remain intact.
pub async fn restore(pool: &SqlitePool, word: &DictionaryWord) -> Result<(), StorageError> {
    sqlx::query("INSERT INTO custom_dictionary (id, word, source, created_at) VALUES (?, ?, ?, ?)")
        .bind(word.id)
        .bind(&word.word)
        .bind(&word.source)
        .bind(&word.created_at)
        .execute(pool)
        .await?;
    Ok(())
}

/// Compatibility for callers that replace the entire meeting list.
pub async fn replace(pool: &SqlitePool, words: &[String]) -> Result<(), StorageError> {
    let words = words
        .iter()
        .map(|w| validate_word(w))
        .collect::<Result<Vec<_>, _>>()?;
    let mut tx = pool.begin().await?;
    let rows: Vec<(i64, String)> = sqlx::query_as("SELECT id, word FROM custom_dictionary")
        .fetch_all(&mut *tx)
        .await?;
    for (id, word) in rows {
        if !words.contains(&word) {
            sqlx::query("DELETE FROM custom_dictionary WHERE id = ?")
                .bind(id)
                .execute(&mut *tx)
                .await?;
        }
    }
    for word in words {
        sqlx::query("INSERT OR IGNORE INTO custom_dictionary (word, source) VALUES (?, 'manual')")
            .bind(word)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
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
    use crate::Db;

    async fn db() -> Db {
        Db::open_in_memory().await.unwrap()
    }

    fn owned(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| (*s).to_owned()).collect()
    }

    #[tokio::test]
    async fn undo_restores_identity_and_legacy_terms_without_relaxing_new_term_validation() {
        let db = db().await;
        let word = "x".repeat(120);
        sqlx::query("INSERT INTO custom_dictionary (word, source) VALUES (?, 'legacy')")
            .bind(&word)
            .execute(db.pool())
            .await
            .unwrap();
        let before = all(db.pool()).await.unwrap();
        let removed = delete(db.pool(), before[0].id).await.unwrap();
        assert!(all(db.pool()).await.unwrap().is_empty());
        restore(db.pool(), &removed).await.unwrap();
        assert_eq!(all(db.pool()).await.unwrap(), before);
        assert!(update(db.pool(), &[word], &[]).await.is_err());
    }

    #[tokio::test]
    async fn migration_preserves_existing_ids_and_legacy_long_terms_and_is_idempotent() {
        let db = db().await;
        update(db.pool(), &owned(&["Åsa Karlsson"]), &[])
            .await
            .unwrap();
        let before = all(db.pool()).await.unwrap();
        crate::settings::set(
            db.pool(),
            "meetings.keyterms",
            &vec![
                " Åsa   Karlsson ".to_owned(),
                "åsa Karlsson".into(),
                "x".repeat(120),
            ],
        )
        .await
        .unwrap();
        sqlx::query("DELETE FROM settings WHERE key = 'vocabulary.migrated'")
            .execute(db.pool())
            .await
            .unwrap();
        migrate_vocabulary(db.pool()).await.unwrap();
        migrate_vocabulary(db.pool()).await.unwrap();
        let after = all(db.pool()).await.unwrap();
        assert_eq!(after.len(), 3);
        assert!(after.contains(&before[0]));
        edit(db.pool(), before[0].id, "Åsa   Svensson")
            .await
            .unwrap();
        assert!(all(db.pool())
            .await
            .unwrap()
            .iter()
            .any(|w| w.id == before[0].id && w.word == "Åsa Svensson"));
    }

    #[tokio::test]
    async fn invalid_updates_and_duplicate_edits_do_not_partly_apply() {
        let db = db().await;
        update(db.pool(), &owned(&["Dyve", "Knegis"]), &[])
            .await
            .unwrap();
        let before = all(db.pool()).await.unwrap();
        assert!(
            update(db.pool(), &owned(&["valid", " "]), &owned(&["Dyve"]))
                .await
                .is_err()
        );
        assert!(edit(db.pool(), before[0].id, "Knegis").await.is_err());
        assert_eq!(all(db.pool()).await.unwrap(), before);
    }

    #[tokio::test]
    async fn corrupt_legacy_data_does_not_mark_migration_complete() {
        let db = db().await;
        sqlx::query("DELETE FROM settings WHERE key = 'vocabulary.migrated'")
            .execute(db.pool())
            .await
            .unwrap();
        crate::settings::set(db.pool(), "meetings.keyterms", &123)
            .await
            .unwrap();
        assert!(migrate_vocabulary(db.pool()).await.is_err());
        assert!(
            crate::settings::get::<bool>(db.pool(), "vocabulary.migrated")
                .await
                .unwrap()
                .is_none()
        );
    }

    #[tokio::test]
    async fn vocabulary_and_migration_survive_reopening() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("vocabulary.sqlite");
        let db = crate::Db::open(&path).await.unwrap();
        update(db.pool(), &owned(&["Dyve"]), &[]).await.unwrap();
        crate::settings::set(
            db.pool(),
            "meetings.keyterms",
            &owned(&["\u{00c5}sa Karlsson"]),
        )
        .await
        .unwrap();
        db.pool().close().await;
        let reopened = crate::Db::open(&path).await.unwrap();
        let entries = all(reopened.pool()).await.unwrap();
        assert_eq!(entries.len(), 2);
        assert!(entries
            .iter()
            .any(|entry| entry.word == "\u{00c5}sa Karlsson"));
        let ids: Vec<_> = entries.iter().map(|entry| entry.id).collect();
        reopened.pool().close().await;
        let again = crate::Db::open(&path).await.unwrap();
        assert_eq!(
            all(again.pool())
                .await
                .unwrap()
                .iter()
                .map(|entry| entry.id)
                .collect::<Vec<_>>(),
            ids
        );
        again.pool().close().await;
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
