//! Native WebView IPC over a reopened disposable database. No microphone or ASR.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
use crate::AppState;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use tauri::State;
struct Proof(Arc<AtomicBool>);
#[tauri::command]
fn native_recording_report(proof: State<'_, Proof>, passed: bool, message: String) {
    println!("NATIVE RECORDING STATE: {message}");
    proof.0.store(passed, Ordering::SeqCst);
}
#[test]
#[ignore = "requires Windows WebView2 and the worktree Vite server on 5199"]
fn recording_deletion_retry_crosses_native_ipc_after_reopening_the_database() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("recordings");
    std::fs::create_dir_all(&root).unwrap();
    let db_path = temp.path().join("notes.db");
    let (db, id, kept) = tauri::async_runtime::block_on(async {
        let db = kw_storage::Db::open(&db_path).await.unwrap();
        let id = kw_notes_core::notes::create(db.pool(), &kw_notes_core::NewNote::default())
            .await
            .unwrap();
        kw_notes_core::recordings::attach(db.pool(), id, "retry.wav", Some(20.0), None)
            .await
            .unwrap();
        std::fs::write(root.join("retry.wav"), b"synthetic audio").unwrap();
        kw_notes_core::recording_state::establish(db.pool(), id, "retry.wav", "delete-pending")
            .await
            .unwrap();
        sqlx::query("CREATE TABLE deferred_failure(value INTEGER REFERENCES notes(id) DEFERRABLE INITIALLY DEFERRED)").execute(db.pool()).await.unwrap();
        sqlx::query("CREATE TRIGGER fail_commit AFTER DELETE ON note_recordings BEGIN INSERT INTO deferred_failure VALUES(-999); END").execute(db.pool()).await.unwrap();
        assert_eq!(
            kw_notes_core::recording_state::retry(db.pool(), id, &root)
                .await
                .unwrap()
                .status,
            "deleted-update-failed"
        );
        sqlx::query("DROP TRIGGER fail_commit")
            .execute(db.pool())
            .await
            .unwrap();
        let kept = kw_notes_core::notes::create(db.pool(), &kw_notes_core::NewNote::default())
            .await
            .unwrap();
        kw_notes_core::recordings::attach(db.pool(), kept, "kept.wav", Some(10.0), None)
            .await
            .unwrap();
        std::fs::write(root.join("kept.wav"), b"retained audio").unwrap();
        kw_notes_core::recording_state::establish(
            db.pool(),
            kept,
            "kept.wav",
            "kept-untranscribed",
        )
        .await
        .unwrap();
        db.pool().close().await;
        (kw_storage::Db::open(&db_path).await.unwrap(), id, kept)
    });
    let state = tauri::async_runtime::block_on(AppState::in_memory())
        .unwrap()
        .with_test_db(db)
        .with_recording_directory(root.clone());
    let finished = Arc::new(AtomicBool::new(false));
    let observed = finished.clone();
    let script = format!("const recordingId={id};const keptId={kept};")
        + r#"(async()=>{
        const invoke=(cmd,args={})=>window.__TAURI_INTERNALS__.invoke(cmd,args);
        const state=await invoke('note_recording_state',{id:recordingId});
        if(state.status!=='deleted-update-failed'||!state.canRetry)throw Error('Lost durable cleanup state');
        await invoke('notes_trash',{id:recordingId});
        let refused=false;try{await invoke('note_recording_retry_deletion',{id:recordingId});}catch{refused=true;}
        if(!refused)throw Error('Retry was allowed in Trash');
        await invoke('notes_restore',{id:recordingId});
        const deleted=await invoke('note_recording_retry_deletion',{id:recordingId});
        if(deleted.status!=='deleted'||deleted.canRetry)throw Error('Cleanup failed');
        if(await invoke('note_recording_get',{id:recordingId})!==null)throw Error('Attachment retained');
        if((await invoke('note_recording_retry_deletion',{id:recordingId})).status!=='deleted')throw Error('Retry was not idempotent');
        const kept=await invoke('note_recording_state',{id:keptId});
        if(kept.status!=='kept-untranscribed'||kept.canRetry)throw Error('Incorrect kept reason');
        let keptRefused=false;try{await invoke('note_recording_retry_deletion',{id:keptId});}catch{keptRefused=true;}
        if(!keptRefused)throw Error('Deleted kept recording');
        if(!Array.isArray(await invoke('people_page',{limit:51,offset:0})))throw Error('People page contract');
        if(!Array.isArray((await invoke('speech_installations')).jobs))throw Error('Install snapshot contract');
        await invoke('native_recording_report',{passed:true,message:'persisted cleanup, explicit retry, Trash refusal, kept reason, pagination and installation snapshot passed'});
    })().catch(error=>window.__TAURI_INTERNALS__.invoke('native_recording_report',{passed:false,message:String(error)}));"#;
    let mut context = tauri::generate_context!();
    context.config_mut().build.dev_url = Some("http://127.0.0.1:5199/".parse().unwrap());
    context.config_mut().app.windows[0].visible = false;
    context.config_mut().app.windows[0].create = true;
    context.config_mut().app.windows[0].data_directory = Some(temp.path().join("webview"));
    let once = Arc::new(AtomicBool::new(false));
    let app = tauri::Builder::default()
        .any_thread()
        .manage(state)
        .manage(Proof(finished.clone()))
        .invoke_handler(tauri::generate_handler![
            native_recording_report,
            crate::commands::note_recording_state,
            crate::commands::note_recording_retry_deletion,
            crate::commands::note_recording_get,
            crate::commands::notes_trash,
            crate::commands::notes_restore,
            crate::commands::people_page,
            crate::speech_install::speech_installations
        ])
        .on_page_load(move |window, payload| {
            if window.label() == "main"
                && matches!(payload.event(), tauri::webview::PageLoadEvent::Finished)
                && !once.swap(true, Ordering::SeqCst)
            {
                window.eval(&script).unwrap();
            }
        })
        .setup(move |app| {
            let handle = app.handle().clone();
            std::thread::spawn(move || {
                let end = std::time::Instant::now() + std::time::Duration::from_secs(25);
                while std::time::Instant::now() < end && !observed.load(Ordering::SeqCst) {
                    std::thread::sleep(std::time::Duration::from_millis(100));
                }
                handle.exit(0);
            });
            Ok(())
        })
        .build(context)
        .unwrap();
    app.run_return(|_, _| {});
    assert!(
        finished.load(Ordering::SeqCst),
        "Native recording retry proof failed"
    );
    assert!(!root.join("retry.wav").exists());
    assert!(root.join("kept.wav").is_file());
}
