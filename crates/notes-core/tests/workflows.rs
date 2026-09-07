#![allow(clippy::unwrap_used)]
use kw_notes_core::notes::{self, NewNote};
use kw_storage::{jobs, Db};
#[tokio::test]
async fn successful_job_and_note_commit_together_and_terminal_state_cannot_be_overwritten() {
    let db = Db::open_in_memory().await.unwrap();
    jobs::create(db.pool(), "job").await.unwrap();
    jobs::stage(db.pool(), "job", "saving", None).await.unwrap();
    let (id, _) = notes::create_with_recording_job(
        db.pool(),
        &NewNote {
            title: "Imported".into(),
            ..Default::default()
        },
        "owned.wav",
        1.0,
        "original.wav",
        Some("job"),
    )
    .await
    .unwrap();
    let job = jobs::get(db.pool(), "job").await.unwrap().unwrap();
    assert_eq!(job.note_id, Some(id));
    assert_eq!(job.stage, "succeeded");
    jobs::stage(db.pool(), "job", "cancelled", None)
        .await
        .unwrap();
    jobs::interrupt_unfinished(db.pool()).await.unwrap();
    assert_eq!(
        jobs::get(db.pool(), "job").await.unwrap().unwrap().stage,
        "succeeded"
    );
    notes::delete(db.pool(), id).await.unwrap();
    assert_eq!(
        jobs::get(db.pool(), "job").await.unwrap().unwrap().note_id,
        None
    );
}
#[tokio::test]
async fn missing_or_cancelled_job_rolls_back_the_note_and_recording() {
    let db = Db::open_in_memory().await.unwrap();
    jobs::create(db.pool(), "job").await.unwrap();
    jobs::stage(db.pool(), "job", "cancelled", None)
        .await
        .unwrap();
    assert!(notes::create_with_recording_job(
        db.pool(),
        &NewNote::default(),
        "owned.wav",
        1.0,
        "original.wav",
        Some("job")
    )
    .await
    .is_err());
    assert!(notes::list(db.pool(), None, None, 10)
        .await
        .unwrap()
        .is_empty());
}
