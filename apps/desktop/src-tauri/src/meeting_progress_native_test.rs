//! Hidden native WebView: real recovery/status/settings IPC in a disposable profile.
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
use tauri::{Manager, State};
struct Proof(Arc<AtomicBool>);
#[tauri::command]
fn native_progress_report(proof: State<'_, Proof>, passed: bool, message: String) {
    assert!(passed, "{message}");
    proof.0.store(true, Ordering::SeqCst);
}
#[test]
#[ignore = "Requires a dev server and a disposable KW_AUTOMATION_PROFILE; explicitly run on Windows"]
fn native_meeting_progress_and_settings_ipc() {
    let profile = kw_storage::paths::debug_profile().expect("Use a disposable debug profile");
    assert_eq!(profile.file_name().unwrap(), "native-progress-profile");
    let workspace = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    assert!(
        profile.starts_with(workspace.join("target")),
        "Profile must be in this worktree's target directory"
    );
    let directory = crate::recordings::recording_dir();
    std::fs::create_dir_all(&directory).unwrap();
    let wav = directory.join("20261012-120000-a1b2c3d4.wav");
    std::fs::write(
        &wav,
        kw_audio_core::wav::encode_wav(&vec![0.0; 32_000], 16_000),
    )
    .unwrap();
    std::fs::write(
        crate::meeting::draft::sidecar_path(&wav),
        "Typed native progress fixture",
    )
    .unwrap();
    let state = tauri::async_runtime::block_on(AppState::in_memory()).unwrap();
    let finished = Arc::new(AtomicBool::new(false));
    let observed = finished.clone();
    let script = r#"(async()=>{
      const ipc=await import('/src/lib/ipc.ts');
      const updates=[]; const stop=await ipc.onMeetingProgress(update=>updates.push(update));
      const invoke=(cmd,args={})=>window.__TAURI_INTERNALS__.invoke(cmd,args);
      const original=await invoke('summary_settings_get');
      await invoke('summary_settings_set',{settings:{...original,enabled:false,language:'sv'}});
      if((await invoke('summary_settings_get')).language!=='sv')throw Error('Summary settings round trip');
      let rejected=false;try{await invoke('summary_settings_set',{settings:{...original,enabled:true,provider:'invalid',model:'fixture'}});}catch{rejected=true;}
      if(!rejected||(await invoke('summary_settings_get')).language!=='sv')throw Error('Invalid settings changed committed value');
      const result=await invoke('meeting_recover_recording',{storageKey:'20261012-120000-a1b2c3d4.wav'});
      if(result.outcome!=='saved')throw Error('Typed recording did not recover');
      await new Promise(resolve=>setTimeout(resolve,300));
      const stages=updates.filter(update=>update.progress).map(update=>update.progress.stage);
      for(const stage of ['preparing-recording','transcribing-recording','saving-note'])if(!stages.includes(stage))throw Error('Missing '+stage);
      if(stages.includes('finishing-live-transcript')||stages.includes('analysing-speakers'))throw Error('Reported work that did not run');
      const active=updates.filter(update=>update.progress);
      if(active.some(update=>update.progress.kind!=='recovery'))throw Error('Wrong operation kind');
      if(active.some((update,index)=>index>0&&update.progress.elapsedSeconds<active[index-1].progress.elapsedSeconds))throw Error('Clock moved backwards');
      const status=await invoke('meeting_status');
      if(status.state!=='idle'||status.finalisation.progress!==null)throw Error('Progress remained after success');
      let failed=false;try{await invoke('meeting_recover_recording',{storageKey:'invalid'});}catch{failed=true;}
      if(!failed||(await invoke('meeting_status')).finalisation.progress!==null)throw Error('Progress remained after failure');
      stop();
      await invoke('native_progress_report',{passed:true,message:'Real typed-only recovery stages, elapsed clock, success/failure cleanup and settings validation passed'});
    })().catch(error=>window.__TAURI_INTERNALS__.invoke('native_progress_report',{passed:false,message:String(error)}));"#;
    let mut context = tauri::generate_context!();
    context.config_mut().build.dev_url = Some(
        std::env::var("KW_PROGRESS_NATIVE_URL")
            .unwrap_or_else(|_| "http://127.0.0.1:5191/".into())
            .parse()
            .unwrap(),
    );
    context.config_mut().app.windows[0].visible = false;
    context.config_mut().app.windows[0].create = true;
    context.config_mut().app.windows[0].data_directory = Some(profile.join("webview"));
    let once = Arc::new(AtomicBool::new(false));
    let app = tauri::Builder::default()
        .any_thread()
        .manage(state)
        .manage(Proof(finished.clone()))
        .invoke_handler(tauri::generate_handler![
            native_progress_report,
            crate::meeting::meeting_status,
            crate::meeting::meeting_recover_recording,
            crate::summaries::summary_settings_get,
            crate::summaries::summary_settings_set
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
            let state = app.state::<AppState>();
            let local = Arc::new(
                crate::local_model::LocalTranscription::new(state.policy().clone(), None).unwrap(),
            );
            let runtime = Arc::new(crate::meeting::MeetingRuntime::new(
                handle.clone(),
                state.db().clone(),
                state.policy().clone(),
                state.credentials_arc(),
                local.shared(),
                crate::local_model::meeting_revive_handle(&local, state.db()),
                crate::local_model::MeetingServer::new(&local, state.db()),
                Arc::new(std::sync::RwLock::new(None)),
                Arc::new(crate::sherpa_stream::SherpaStream::new().unwrap()),
                Arc::new(crate::meeting::draft::NotesDraft::default()),
            ));
            app.manage(runtime);
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
        "Native meeting progress proof failed"
    );
    assert!(wav.exists(), "Recovery lost the retained recording");
}
