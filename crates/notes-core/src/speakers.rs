//! Names a person assigns to the diarised speakers of a note.
//!
//! A rename is a note edit, so it lives beside the note instead of rewriting
//! segment JSON. One mapping names every segment with that raw speaker id.
//! The existing transcript resolver already uses these mappings; the database
//! reads and writes below are exercised against an in-memory migrated database.
//! Suggestions reuse stored participants and other notes' mappings, without
//! assuming that matching names identify the same person.

use std::collections::{BTreeMap, HashSet};

use kw_storage::StorageError;
use sqlx::SqlitePool;

use crate::transcript::{self, NoteMeta};

/// Every name a person has given this note's speakers, keyed by raw label.
pub async fn mappings(
    pool: &SqlitePool,
    note_id: i64,
) -> Result<BTreeMap<String, String>, StorageError> {
    let rows: Vec<(String, String)> =
        sqlx::query_as("SELECT speaker_id, display_name FROM speaker_mappings WHERE note_id = ?")
            .bind(note_id)
            .fetch_all(pool)
            .await?;
    Ok(rows.into_iter().collect())
}

/// Name one speaker of one note. Whitespace is collapsed; an empty name removes
/// the mapping. Returns false for a missing note or a blank speaker id, and
/// writes nothing in either case.
pub async fn rename(
    pool: &SqlitePool,
    note_id: i64,
    speaker_id: &str,
    name: &str,
) -> Result<bool, StorageError> {
    if speaker_id.trim().is_empty() {
        return Ok(false);
    }
    let mut tx = pool.begin().await?;
    let exists = sqlx::query_scalar::<_, i64>("SELECT 1 FROM notes WHERE id = ?")
        .bind(note_id)
        .fetch_optional(&mut *tx)
        .await?
        .is_some();
    if !exists {
        return Ok(false);
    }
    let name = name.split_whitespace().collect::<Vec<_>>().join(" ");
    if name.is_empty() {
        sqlx::query("DELETE FROM speaker_mappings WHERE note_id = ? AND speaker_id = ?")
            .bind(note_id)
            .bind(speaker_id)
            .execute(&mut *tx)
            .await?;
    } else {
        // Bound values keep free-form names and labels out of SQL text.
        sqlx::query(
            "INSERT INTO speaker_mappings (note_id, speaker_id, display_name) VALUES (?, ?, ?) \
             ON CONFLICT(note_id, speaker_id) DO UPDATE SET display_name = excluded.display_name",
        )
        .bind(note_id)
        .bind(speaker_id)
        .bind(name)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(true)
}

/// Participants first, then names from other notes in most recently updated
/// note order. Each spelling appears once, and names already assigned to this
/// note are omitted. A missing note has no suggestions.
pub async fn suggestions(pool: &SqlitePool, note_id: i64) -> Result<Vec<String>, StorageError> {
    let Some(note) = crate::notes::get(pool, note_id).await? else {
        return Ok(Vec::new());
    };
    let used = mappings(pool, note_id).await?;
    let mut seen: HashSet<String> = used.values().map(|name| name.to_lowercase()).collect();
    let mut result = Vec::new();
    let meta = NoteMeta {
        participants: note.participants,
        ..Default::default()
    };
    let others: Vec<(String,)> = sqlx::query_as(
        "SELECT sm.display_name FROM speaker_mappings sm \
         JOIN notes n ON n.id = sm.note_id WHERE sm.note_id != ? \
         ORDER BY n.updated_at DESC, n.id DESC, sm.speaker_id",
    )
    .bind(note_id)
    .fetch_all(pool)
    .await?;
    for name in transcript::participants(&meta)
        .into_iter()
        .chain(others.into_iter().map(|row| row.0))
    {
        let name = name.trim();
        if !name.is_empty() && seen.insert(name.to_lowercase()) {
            result.push(name.to_owned());
            if result.len() == 20 {
                break;
            }
        }
    }
    Ok(result)
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
    use crate::notes::{self, NewNote};
    use kw_storage::Db;

    async fn note(pool: &SqlitePool, participants: Option<&str>) -> i64 {
        notes::create(
            pool,
            &NewNote {
                title: "Meeting".into(),
                participants: participants.map(str::to_owned),
                ..Default::default()
            },
        )
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn renaming_replaces_normalizes_and_removes_a_mapping() {
        let db = Db::open_in_memory().await.unwrap();
        let id = note(db.pool(), None).await;
        assert!(rename(db.pool(), id, "speaker_0", "  Axel \t Karlsson  ")
            .await
            .unwrap());
        assert_eq!(
            mappings(db.pool(), id).await.unwrap()["speaker_0"],
            "Axel Karlsson"
        );
        assert!(rename(db.pool(), id, "speaker_0", "Alex").await.unwrap());
        assert_eq!(mappings(db.pool(), id).await.unwrap()["speaker_0"], "Alex");
        assert!(rename(db.pool(), id, "speaker_0", " \n ").await.unwrap());
        assert!(mappings(db.pool(), id).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn missing_notes_and_blank_speaker_ids_write_nothing() {
        let db = Db::open_in_memory().await.unwrap();
        let id = note(db.pool(), None).await;
        assert!(!rename(db.pool(), id + 10, "speaker_0", "Axel")
            .await
            .unwrap());
        assert!(!rename(db.pool(), id, "  ", "Axel").await.unwrap());
        assert!(mappings(db.pool(), id).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn deleting_a_note_cascades_its_speaker_names() {
        let db = Db::open_in_memory().await.unwrap();
        let id = note(db.pool(), None).await;
        rename(db.pool(), id, "speaker_0", "Axel").await.unwrap();
        assert!(notes::delete(db.pool(), id).await.unwrap());
        assert!(mappings(db.pool(), id).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn suggestions_prioritize_participants_dedupe_exclude_and_stop_at_twenty() {
        let db = Db::open_in_memory().await.unwrap();
        let old = note(db.pool(), None).await;
        rename(db.pool(), old, "speaker_0", "Older").await.unwrap();
        let recent = note(db.pool(), None).await;
        rename(db.pool(), recent, "speaker_0", "Recent")
            .await
            .unwrap();
        rename(db.pool(), recent, "speaker_1", "AXEL")
            .await
            .unwrap();
        let people = serde_json::json!([
            {"displayName":"Axel"}, {"displayName":"Local"}, {"displayName":"local"}
        ])
        .to_string();
        let target = note(db.pool(), Some(&people)).await;
        rename(db.pool(), target, "speaker_0", "Recent")
            .await
            .unwrap();
        let names = suggestions(db.pool(), target).await.unwrap();
        assert_eq!(names, ["Axel", "Local", "Older"]);

        for i in 0..25 {
            let other = note(db.pool(), None).await;
            rename(db.pool(), other, "speaker_0", &format!("Person {i}"))
                .await
                .unwrap();
        }
        let names = suggestions(db.pool(), target).await.unwrap();
        assert_eq!(names.len(), 20);
        assert_eq!(names[0], "Axel");
        assert_eq!(names[1], "Local");
        assert!(!names.iter().any(|name| name == "Recent"));
    }
}
