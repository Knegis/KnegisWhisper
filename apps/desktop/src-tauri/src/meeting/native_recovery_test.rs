//! Disposable real WebView/IPC recovery check with synthetic transcription output.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
use super::*;
use crate::AppState;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use tauri::State;
struct Proof {
    wav: std::path::PathBuf,
    finished: Arc<AtomicBool>,
}
#[tauri::command]
async fn native_recover_fixture(
    state: State<'_, AppState>,
    proof: State<'_, Proof>,
) -> Result<i64, String> {
    let key = proof.wav.file_name().unwrap().to_str().unwrap();
    let segments = [kw_notes_core::transcript::Segment {
        text: "Synthetic recovered transcript".into(),
        timestamp: Some(0.0),
        ..Default::default()
    }];
    match save_recovered(
        state.db(),
        &proof.wav,
        key,
        "Recovered personal notes",
        &segments,
        "Synthetic recovery.",
        1.0,
    )
    .await?
    {
        MeetingOutcome::Saved { note_id, .. } => Ok(note_id),
        other => Err(format!("Unexpected recovery {other:?}")),
    }
}
#[tauri::command]
fn native_recovery_report(proof: State<'_, Proof>, passed: bool, message: String) {
    println!("NATIVE RECOVERY: {message}");
    proof.finished.store(passed, Ordering::SeqCst);
}
#[test]
#[ignore = "requires interactive Windows and the worktree Vite server on 5198"]
fn recovered_provider_and_lightweight_notes_cross_native_ipc() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("recordings");
    std::fs::create_dir_all(&root).unwrap();
    let wav = root.join("2026-10-08_12-00-00.wav");
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../crates/transcription-core/tests/fixtures/speech-16k-mono.wav");
    std::fs::copy(fixture, &wav).unwrap();
    let state = tauri::async_runtime::block_on(AppState::in_memory())
        .unwrap()
        .with_recording_directory(root);
    let plan = tauri::async_runtime::block_on(plan_at_start(
        state.db(),
        MeetingPlan {
            live_provider_id: Some("forged".into()),
            ..Default::default()
        },
        "openai-realtime",
    ));
    draft::write_plan(&wav, &plan).unwrap();
    let finished = Arc::new(AtomicBool::new(false));
    let observed = finished.clone();
    let proof = Proof {
        wav: wav.clone(),
        finished: finished.clone(),
    };
    let script = r#"(async()=>{
      const invoke=(command,args)=>window.__TAURI_INTERNALS__.invoke(command,args);
      const id=await invoke('native_recover_fixture');
      const note=await invoke('notes_get',{id});
      const by=JSON.parse(note.transcribedBy);
      if(by.provider!=='local'||by.liveName!=='OpenAI Realtime')throw Error('Incorrect recovery provenance');
      const listed=await invoke('notes_list_summaries',{});
      if(listed.length!==1||listed[0].id!==id||!listed[0].hasTranscript||'transcript' in listed[0]||'content' in listed[0])throw Error('Incorrect metadata projection');
      if(!await invoke('notes_update',{id,request:{title:'Native saved metadata'}}))throw Error('Update failed');
      const metadata=await invoke('notes_get_summary',{id});
      if(metadata.title!=='Native saved metadata'||'content' in metadata)throw Error('Wrong acknowledgment');
      await invoke('native_recovery_report',{passed:true,message:'recovery provenance, metadata list and committed update passed'});
    })().catch(error=>window.__TAURI_INTERNALS__.invoke('native_recovery_report',{passed:false,message:String(error)}));"#;
    let mut context = tauri::generate_context!();
    context.config_mut().build.dev_url = Some("http://127.0.0.1:5198/".parse().unwrap());
    context.config_mut().app.windows[0].visible = false;
    context.config_mut().app.windows[0].create = true;
    context.config_mut().app.windows[0].data_directory = Some(temp.path().join("webview"));
    context.config_mut().app.windows[0].title = "Knegis recovery IPC smoke".into();
    let once = Arc::new(AtomicBool::new(false));
    let app = tauri::Builder::default()
        .any_thread()
        .manage(state)
        .manage(proof)
        .invoke_handler(tauri::generate_handler![
            native_recover_fixture,
            native_recovery_report,
            crate::commands::notes_get,
            crate::commands::notes_list_summaries,
            crate::commands::notes_get_summary,
            crate::commands::notes_update
        ])
        .on_page_load(move |window, payload| {
            if window.label() == "main"
                && matches!(payload.event(), tauri::webview::PageLoadEvent::Finished)
                && !once.swap(true, Ordering::SeqCst)
            {
                window.eval(script).unwrap();
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
        "Native recovery/IPC did not pass"
    );
    assert!(
        !draft::plan_path(&wav).exists(),
        "Saved recovery removes its sidecar"
    );
}
