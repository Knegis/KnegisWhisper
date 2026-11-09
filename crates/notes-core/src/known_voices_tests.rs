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
async fn save(db: &Db, id: i64, samples: &[Sample]) {
    let mut tx = db.pool().begin().await.unwrap();
    persist(&mut tx, id, samples, policy()).await.unwrap();
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
    let id = enroll(db.pool(), a, "speaker_0", "Ada", None, policy())
        .await
        .unwrap();
    assert_eq!(
        enroll(db.pool(), a, "speaker_0", "Ada", None, policy())
            .await
            .unwrap(),
        id
    );
    assert!(enroll(db.pool(), a, "speaker_0", "Bea", None, policy())
        .await
        .is_err());
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
    assert!(enroll(db.pool(), a, "speaker_0", "Ada", None, policy())
        .await
        .is_err());
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
    assert!(enroll(db.pool(), a, "speaker_0", "Ada", None, policy())
        .await
        .is_err());
    kw_storage::settings::set(db.pool(), ENABLED_KEY, &true)
        .await
        .unwrap();
    let mut short = sample("speaker_0", 0);
    short.seconds = 0.5;
    save(&db, a, &[short]).await;
    assert!(enroll(db.pool(), a, "speaker_0", "Ada", None, policy())
        .await
        .is_err());
    sqlx::query("INSERT INTO speaker_profiles(display_name,embedding) VALUES('Legacy',?)")
        .bind(encode(&vector(0)).unwrap())
        .execute(db.pool())
        .await
        .unwrap();
    save(&db, a, &[sample("speaker_0", 0)]).await;
    assert!(speakers::mappings(db.pool(), a).await.unwrap().is_empty());
    assert!(!list(db.pool()).await.unwrap()[0].compatible);
    let id = enroll(db.pool(), a, "speaker_0", "Ada", None, policy())
        .await
        .unwrap();
    kw_storage::settings::set(db.pool(), ENABLED_KEY, &false)
        .await
        .unwrap();
    let b = note(&db).await;
    save(&db, b, &[sample("speaker_0", 0)]).await;
    assert!(speakers::mappings(db.pool(), b).await.unwrap().is_empty());
    assert_eq!(list(db.pool()).await.unwrap().len(), 2);
    assert!(enroll(db.pool(), a, "speaker_0", "Ada", Some(id), policy())
        .await
        .is_err());
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
    enroll(db.pool(), a, "speaker_0", "Ada", None, policy())
        .await
        .unwrap();
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
