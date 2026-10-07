//! Disposable native IPC + real file-dialog smoke test. The browser probe covers UI.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
use super::*;
use std::os::windows::process::CommandExt;

#[tauri::command]
fn native_sharing_report(message: String) {
    println!("NATIVE SHARING: {message}");
}

#[test]
#[ignore = "requires interactive Windows desktop and the worktree Vite server on 5197"]
fn native_folder_sharing_round_trip() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("recordings");
    std::fs::create_dir_all(&root).unwrap();
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../crates/transcription-core/tests/fixtures/speech-16k-mono.wav");
    std::fs::copy(&fixture, root.join("original.wav")).unwrap();
    let state = tauri::async_runtime::block_on(AppState::in_memory())
        .unwrap()
        .with_recording_directory(root.clone());
    let db = state.db().clone();
    let folder = tauri::async_runtime::block_on(kw_notes_core::notes::create_folder(
        db.pool(),
        "Native Åäö",
    ))
    .unwrap();
    let note = tauri::async_runtime::block_on(kw_notes_core::notes::create(
        db.pool(),
        &kw_notes_core::NewNote {
            title: "Native shared meeting".into(),
            content: "Personal notes retained".into(),
            folder_id: Some(folder),
            transcript: Some(r#"[{"text":"Native transcript","timestamp":0.0}]"#.into()),
            ..Default::default()
        },
    ))
    .unwrap();
    tauri::async_runtime::block_on(kw_notes_core::recordings::attach(
        db.pool(),
        note,
        "original.wav",
        Some(1.0),
        None,
    ))
    .unwrap();
    let runtime = SharingRuntime::default();
    let watcher = runtime.clone();
    let script = format!(
        r#"(async () => {{
      const invoke=(command,args)=>window.__TAURI_INTERNALS__.invoke(command,args);
      await invoke('native_sharing_report',{{message:'page loaded'}});
      const wait=async stage=>{{const end=Date.now()+60000;let previous;while(Date.now()<end){{const s=await invoke('folder_share_status');if(s?.stage!==previous){{previous=s?.stage;await invoke('native_sharing_report',{{message:'stage '+JSON.stringify(s)}});}}if(s?.stage==='failed')throw Error(s.error);if(s?.stage===stage)return s;await new Promise(r=>setTimeout(r,100));}}throw Error('Sharing timeout');}};
      await invoke('folder_share_preview',{{folderId:{folder}}});
      await invoke('folder_share_export',{{folderId:{folder},noteIds:[{note}],options:{{personalNotes:true,summaries:true,transcripts:true,recordings:true}}}});
      let s=await wait('succeeded');await invoke('folder_share_dismiss',{{operationId:s.operationId}});
      await invoke('folder_share_pick_import');s=await wait('awaiting-confirmation');
      if(s.preview.notes.length!==1)throw Error('Wrong import preview');
      await invoke('folder_share_import',{{operationId:s.operationId,folderName:'Native imported'}});s=await wait('succeeded');
      const notes=await invoke('notes_list',{{folderId:s.folderId,limit:10}});if(notes.length!==1||notes[0].content!=='Personal notes retained')throw Error('Wrong imported note');
      const audio=document.createElement('audio');audio.muted=true;audio.src='http://recording.localhost/'+notes[0].id;document.body.append(audio);await audio.play();audio.pause();
      await invoke('folder_share_dismiss',{{operationId:s.operationId}});
    }})().catch(error=>window.__TAURI_INTERNALS__.invoke('native_sharing_report',{{message:String(error)}}));"#
    );
    let mut context = tauri::generate_context!();
    context.config_mut().build.dev_url = Some("http://127.0.0.1:5197/".parse().unwrap());
    context.config_mut().app.windows[0].visible = false;
    context.config_mut().app.windows[0].create = true;
    context.config_mut().app.windows[0].data_directory = Some(temp.path().join("webview"));
    context.config_mut().app.windows[0].title = "KnegisWhisper sharing smoke".into();
    let launched = Arc::new(AtomicBool::new(false));
    let once = launched.clone();
    let package = temp.path().join("native.knegis");
    let helper = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../tools/visual-check/sharing-dialogs.ps1");
    let mut dialogs = std::process::Command::new("powershell.exe")
        .creation_flags(0x08000000)
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
        .arg(helper)
        .arg("-AppProcessId")
        .arg(std::process::id().to_string())
        .arg("-PackagePath")
        .arg(&package)
        .spawn()
        .unwrap();
    let db_check = db.clone();
    let finished = Arc::new(AtomicBool::new(false));
    let result = finished.clone();
    let app = tauri::Builder::default()
        .any_thread()
        .plugin(tauri_plugin_dialog::init())
        .manage(state)
        .manage(runtime)
        .register_asynchronous_uri_scheme_protocol(
            crate::recordings::SCHEME,
            crate::recordings::serve,
        )
        .invoke_handler(tauri::generate_handler![
            native_sharing_report,
            folder_share_preview,
            folder_share_export,
            folder_share_pick_import,
            folder_share_status,
            folder_share_import,
            folder_share_cancel,
            folder_share_dismiss,
            crate::commands::notes_list
        ])
        .on_page_load(move |window, payload| {
            println!("NATIVE PAGE: {} {:?}", payload.url(), payload.event());
            if window.label() == "main"
                && matches!(payload.event(), tauri::webview::PageLoadEvent::Finished)
                && !once.swap(true, Ordering::Relaxed)
            {
                window.eval(&script).unwrap();
            }
        })
        .setup(move |app| {
            let handle = app.handle().clone();
            let observer = watcher.clone();
            let db = db_check.clone();
            let result = result.clone();
            std::thread::spawn(move || {
                let end = std::time::Instant::now() + std::time::Duration::from_secs(75);
                while std::time::Instant::now() < end {
                    let ready = observer.entry.lock().unwrap().is_none();
                    let imported =
                        tauri::async_runtime::block_on(kw_notes_core::notes::folders(db.pool()))
                            .unwrap()
                            .iter()
                            .any(|f| f.name == "Native imported");
                    if ready && imported {
                        result.store(true, Ordering::Relaxed);
                        break;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(100));
                }
                handle.exit(0);
            });
            Ok(())
        })
        .build(context)
        .unwrap();
    app.run_return(|_, _| {});
    let helper_result = dialogs.wait().unwrap();
    assert!(helper_result.success());
    assert!(
        finished.load(Ordering::Relaxed),
        "Native picker/import/playback did not finish."
    );
    assert!(launched.load(Ordering::Relaxed));
    assert!(package.is_file());
    let original = std::fs::read(fixture).unwrap();
    let notes =
        tauri::async_runtime::block_on(kw_notes_core::notes::list(db.pool(), None, None, 10))
            .unwrap();
    let imported = notes.iter().find(|n| n.id != note).unwrap();
    let audio =
        tauri::async_runtime::block_on(kw_notes_core::recordings::get(db.pool(), imported.id))
            .unwrap()
            .unwrap();
    let path = root.join(audio.storage_key);
    assert_eq!(std::fs::read(&path).unwrap(), original);
    let request = tauri::http::Request::builder()
        .header("Range", "bytes=0-43")
        .body(Vec::new())
        .unwrap();
    let response = crate::recordings::response_for(&request, &path);
    assert_eq!(response.status(), tauri::http::StatusCode::PARTIAL_CONTENT);
    assert_eq!(response.body(), &original[..44]);
    println!("Native save/open dialogs, export/import IPC, retained audio, range serving and WebView playback passed on disposable data.");
}
