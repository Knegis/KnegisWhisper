#![allow(clippy::unwrap_used, clippy::indexing_slicing)]
use super::*;
use crate::{notes, speakers};
use kw_storage::Db;
fn vector(index: usize) -> Vec<f32> {
    let mut v = vec![0.0; DIM];
    v[index] = 1.0;
    v
}
fn policy() -> Policy {
    Policy {
        threshold: 0.9,
        margin: 0.1,
        min_seconds: 3.0,
    }
}
fn sample(speaker: &str, index: usize) -> Sample {
    Sample {
        speaker_id: speaker.into(),
        embedding: vector(index),
        seconds: 5.0,
    }
}
async fn note(db: &Db) -> i64 {
    notes::create(db.pool(), &notes::NewNote::default())
        .await
        .unwrap()
}
fn provenance(id: i64) -> Source {
    Source {
        storage_key: format!("meeting-{id}.wav"),
        sha256: "a".repeat(64),
        pipeline: MODEL_ID.into(),
        duration: 10.0,
        turns: vec![VoiceTurn {
            start_seconds: 0.0,
            end_seconds: 5.0,
            speaker: 0,
        }],
    }
}
async fn remember(
    db: &Db,
    id: i64,
    name: &str,
    replace: Option<i64>,
    seconds: f64,
) -> Result<i64, StorageError> {
    let mut sample = sample("speaker_0", 0);
    sample.seconds = seconds;
    enroll(
        db.pool(),
        id,
        "speaker_0",
        name,
        replace,
        policy(),
        &sample,
        &provenance(id),
        generation(db.pool()).await?,
    )
    .await
}
async fn save(db: &Db, id: i64, samples: &[Sample]) {
    let analysis = Analysis {
        samples: samples.to_vec(),
        source: Some(provenance(id)),
        generation: generation(db.pool()).await.unwrap(),
        warning: None,
    };
    // Tests model an attached recording; extraction itself is tested by the worker suite.
    sqlx::query("INSERT OR IGNORE INTO note_recordings(note_id,storage_key) VALUES(?,?)")
        .bind(id)
        .bind(format!("meeting-{id}.wav"))
        .execute(db.pool())
        .await
        .unwrap();
    let mut tx = db.pool().begin().await.unwrap();
    persist(&mut tx, id, &analysis, policy()).await.unwrap();
    tx.commit().await.unwrap();
}
async fn enabled_db() -> Db {
    let db = Db::open_in_memory().await.unwrap();
    kw_storage::settings::set(db.pool(), ENABLED_KEY, &true)
        .await
        .unwrap();
    db
}

#[test]
fn matcher_rejects_invalid_ambiguous_and_unknown_vectors() {
    assert!(normalize(&vec![0.0; DIM]).is_none());
    assert!(normalize(&vec![f32::NAN; DIM]).is_none());
    assert!(decode(&[0; 4]).is_none());
    assert_eq!(choose(&vector(0), &[(1, vector(0))], policy()), Some(1));
    assert_eq!(choose(&vector(1), &[(1, vector(0))], policy()), None);
    assert_eq!(
        choose(&vector(0), &[(1, vector(0)), (2, vector(0))], policy()),
        None
    );
    assert_eq!(
        choose(
            &vector(0),
            &[(1, vector(0))],
            Policy {
                threshold: f32::NAN,
                ..policy()
            }
        ),
        None
    );
    assert!(
        RELEASE_POLICY.is_none(),
        "Do not enable without a reviewed evaluation report"
    );
}
#[tokio::test]
async fn explicit_enrollment_is_idempotent_and_matches_multiple_clusters() {
    let db = enabled_db().await;
    let a = note(&db).await;
    save(&db, a, &[sample("speaker_0", 0)]).await;
    let id = remember(&db, a, "Ada", None, 5.0).await.unwrap();
    assert_eq!(remember(&db, a, "Ada", None, 5.0).await.unwrap(), id);
    assert!(remember(&db, a, "Bea", None, 5.0).await.is_err());
    let b = note(&db).await;
    save(
        &db,
        b,
        &[
            sample("speaker_2", 0),
            sample("speaker_4", 0),
            sample("speaker_3", 1),
        ],
    )
    .await;
    let mappings = speakers::mappings(db.pool(), b).await.unwrap();
    assert_eq!(mappings.len(), 2);
    assert_eq!(mappings["speaker_4"], "Ada");
    assert_eq!(list(db.pool()).await.unwrap().len(), 1);
    let count: i64 = sqlx::query_scalar("SELECT sample_count FROM speaker_profiles WHERE id=?")
        .bind(id)
        .fetch_one(db.pool())
        .await
        .unwrap();
    assert_eq!(count, 1);
    speakers::rename(db.pool(), b, "speaker_2", "Bea")
        .await
        .unwrap();
    speakers::rename(db.pool(), b, "speaker_4", "")
        .await
        .unwrap();
    save(&db, b, &[sample("speaker_2", 0), sample("speaker_4", 0)]).await;
    let mappings = speakers::mappings(db.pool(), b).await.unwrap();
    assert_eq!(mappings.len(), 1);
    assert_eq!(mappings["speaker_2"], "Bea");
    rename(db.pool(), id, "Ada new").await.unwrap();
    assert_eq!(
        speakers::mappings(db.pool(), a).await.unwrap()["speaker_0"],
        "Ada"
    );
    delete(db.pool(), id).await.unwrap();
    assert!(list(db.pool()).await.unwrap().is_empty());
    let cached: i64 = sqlx::query_scalar("SELECT count(*) FROM note_speaker_embeddings")
        .fetch_one(db.pool())
        .await
        .unwrap();
    assert_eq!(cached, 0);
    assert!(source(db.pool(), a).await.is_ok());
    assert_eq!(
        speakers::mappings(db.pool(), a).await.unwrap()["speaker_0"],
        "Ada"
    );
}
#[tokio::test]
async fn disabled_short_and_legacy_samples_are_not_used() {
    let db = Db::open_in_memory().await.unwrap();
    let a = note(&db).await;
    save(&db, a, &[sample("speaker_0", 0)]).await;
    assert!(remember(&db, a, "Ada", None, 5.0).await.is_err());
    kw_storage::settings::set(db.pool(), ENABLED_KEY, &true)
        .await
        .unwrap();
    let mut short = sample("speaker_0", 0);
    short.seconds = 0.5;
    save(&db, a, &[short]).await;
    assert!(remember(&db, a, "Ada", None, 0.5).await.is_err());
    sqlx::query("INSERT INTO speaker_profiles(display_name,embedding) VALUES('Legacy',?)")
        .bind(encode(&vector(0)).unwrap())
        .execute(db.pool())
        .await
        .unwrap();
    save(&db, a, &[sample("speaker_0", 0)]).await;
    assert!(speakers::mappings(db.pool(), a).await.unwrap().is_empty());
    assert!(!list(db.pool()).await.unwrap()[0].compatible);
    let id = remember(&db, a, "Ada", None, 5.0).await.unwrap();
    kw_storage::settings::set(db.pool(), ENABLED_KEY, &false)
        .await
        .unwrap();
    let b = note(&db).await;
    save(&db, b, &[sample("speaker_0", 0)]).await;
    assert!(speakers::mappings(db.pool(), b).await.unwrap().is_empty());
    assert_eq!(list(db.pool()).await.unwrap().len(), 2);
    assert!(remember(&db, a, "Ada", Some(id), 5.0).await.is_err());
}
#[tokio::test]
async fn enrolled_identity_survives_database_reopen() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("voices.sqlite");
    let db = Db::open(&path).await.unwrap();
    kw_storage::settings::set(db.pool(), ENABLED_KEY, &true)
        .await
        .unwrap();
    let a = note(&db).await;
    save(&db, a, &[sample("speaker_0", 0)]).await;
    remember(&db, a, "Ada", None, 5.0).await.unwrap();
    db.pool().close().await;
    drop(db);
    let db = Db::open(&path).await.unwrap();
    assert!(enabled(db.pool()).await);
    let b = note(&db).await;
    save(&db, b, &[sample("speaker_7", 0)]).await;
    assert_eq!(
        speakers::mappings(db.pool(), b).await.unwrap()["speaker_7"],
        "Ada"
    );
}

#[tokio::test]
async fn stale_jobs_cannot_restore_forgotten_or_disabled_voices() {
    let db = enabled_db().await;
    let a = note(&db).await;
    save(&db, a, &[sample("speaker_0", 0)]).await;
    let id = remember(&db, a, "Ada", None, 5.0).await.unwrap();
    let before = generation(db.pool()).await.unwrap();
    delete(db.pool(), id).await.unwrap();
    assert!(enroll(
        db.pool(),
        a,
        "speaker_0",
        "Ada",
        None,
        policy(),
        &sample("speaker_0", 0),
        &provenance(a),
        before
    )
    .await
    .is_err());
    let b = note(&db).await;
    let stale = Analysis {
        samples: vec![sample("speaker_0", 0)],
        source: Some(provenance(b)),
        generation: before,
        warning: None,
    };
    let mut tx = db.pool().begin().await.unwrap();
    persist(&mut tx, b, &stale, policy()).await.unwrap();
    tx.commit().await.unwrap();
    assert!(source(db.pool(), b).await.is_err());
    let before = generation(db.pool()).await.unwrap();
    kw_storage::settings::set(db.pool(), ENABLED_KEY, &false)
        .await
        .unwrap();
    kw_storage::settings::set(db.pool(), ENABLED_KEY, &true)
        .await
        .unwrap();
    assert!(enroll(
        db.pool(),
        a,
        "speaker_0",
        "Ada",
        None,
        policy(),
        &sample("speaker_0", 0),
        &provenance(a),
        before
    )
    .await
    .is_err());
    assert!(list(db.pool()).await.unwrap().is_empty());
}
#[tokio::test]
async fn enrollment_rechecks_recording_metadata_and_repeated_manual_corrections() {
    let db = enabled_db().await;
    let a = note(&db).await;
    save(&db, a, &[sample("speaker_0", 0)]).await;
    speakers::rename(db.pool(), a, "speaker_0", "First edit")
        .await
        .unwrap();
    let before = generation(db.pool()).await.unwrap();
    speakers::rename(db.pool(), a, "speaker_0", "Second edit")
        .await
        .unwrap();
    assert!(enroll(
        db.pool(),
        a,
        "speaker_0",
        "Ada",
        None,
        policy(),
        &sample("speaker_0", 0),
        &provenance(a),
        before
    )
    .await
    .is_err());
    assert_eq!(
        speakers::mappings(db.pool(), a).await.unwrap()["speaker_0"],
        "Second edit"
    );
    let mut changed = provenance(a);
    changed.turns[0].speaker = 1;
    assert!(enroll(
        db.pool(),
        a,
        "speaker_0",
        "Ada",
        None,
        policy(),
        &sample("speaker_0", 0),
        &changed,
        generation(db.pool()).await.unwrap()
    )
    .await
    .is_err());
    sqlx::query("DELETE FROM note_recordings WHERE note_id=?")
        .bind(a)
        .execute(db.pool())
        .await
        .unwrap();
    assert!(remember(&db, a, "Ada", None, 5.0).await.is_err());
    assert!(list(db.pool()).await.unwrap().is_empty());
}
