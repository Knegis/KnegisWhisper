//! Native summary IPC with an isolated database and a loopback synthetic provider.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
use std::io::{BufRead, Read, Write};
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    Arc,
};
use tauri::State;

struct SummaryProof(Arc<AtomicBool>);
#[tauri::command]
fn native_summary_report(proof: State<'_, SummaryProof>, passed: bool, message: String) {
    println!("NATIVE SUMMARY: {message}");
    proof.0.store(passed, Ordering::SeqCst);
}

#[test]
#[ignore = "requires Windows WebView2 and the isolated Vite server on 5195"]
fn summary_setup_generation_failure_and_cancel_cross_native_ipc() {
    let temp = tempfile::tempdir().unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    listener.set_nonblocking(true).unwrap();
    let stopping = Arc::new(AtomicBool::new(false));
    let stopped = stopping.clone();
    let requests = Arc::new(AtomicUsize::new(0));
    let calls = requests.clone();
    let server = std::thread::spawn(move || {
        while !stopped.load(Ordering::SeqCst) {
            let Ok((mut stream, _)) = listener.accept() else {
                std::thread::sleep(std::time::Duration::from_millis(10));
                continue;
            };
            stream.set_nonblocking(false).unwrap();
            stream
                .set_read_timeout(Some(std::time::Duration::from_secs(5)))
                .unwrap();
            let mut reader = std::io::BufReader::new(&mut stream);
            let mut length = 0;
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                if line == "\r\n" || line.is_empty() {
                    break;
                }
                if let Some((name, value)) = line.split_once(':') {
                    if name.eq_ignore_ascii_case("content-length") {
                        length = value.trim().parse::<usize>().unwrap();
                    }
                }
            }
            let mut body = vec![0; length];
            reader.read_exact(&mut body).unwrap();
            let body: serde_json::Value = serde_json::from_slice(&body).unwrap();
            calls.fetch_add(1, Ordering::SeqCst);
            let model = body["model"].as_str().unwrap();
            if model == "delayed" {
                std::thread::sleep(std::time::Duration::from_secs(2));
            }
            let input = body["messages"].as_array().unwrap().last().unwrap()["content"]
                .as_str()
                .unwrap();
            let source = (0..36)
                .map(|i| format!("s{i}"))
                .find(|id| {
                    input.contains(&format!("[{id}]")) || input.contains(&format!("\"{id}\""))
                })
                .unwrap();
            let summary = serde_json::json!({"language":"en","overview":[{"text":"Synthetic discussion.","sources":[source]}],"decisions":[],"actions":[],"questions":[]});
            let content = if model == "bad" {
                "not JSON".to_owned()
            } else {
                summary.to_string()
            };
            let response = serde_json::json!({"choices":[{"finish_reason":"stop","message":{"content":content}}]}).to_string();
            let headers = format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", response.len());
            let _ = stream.write_all(headers.as_bytes());
            let _ = stream.write_all(response.as_bytes());
        }
    });
    let state = tauri::async_runtime::block_on(crate::AppState::in_memory())
        .unwrap()
        .with_recording_directory(temp.path().join("recordings"));
    let id = tauri::async_runtime::block_on(async {
        let transcript = (0..36).map(|i| serde_json::json!({"text":"A synthetic meeting discussion. ".repeat(12),"timestamp":i*60,"endTimestamp":(i+1)*60})).collect::<Vec<_>>();
        kw_notes_core::notes::create(
            state.db().pool(),
            &kw_notes_core::NewNote {
                title: "Native synthetic summary".into(),
                transcript: Some(serde_json::to_string(&transcript).unwrap()),
                ..Default::default()
            },
        )
        .await
        .unwrap()
    });
    let llm = Arc::new(crate::local_llm::LocalLlm::new(state.policy().clone()).unwrap());
    let worker_state = state.clone();
    let worker_llm = llm.clone();
    let finished = Arc::new(AtomicBool::new(false));
    let observed = finished.clone();
    let script = format!("const summaryId={id};const summaryEndpoint='http://{address}/v1';")
        + r#"(async()=>{
      const invoke=(cmd,args={})=>window.__TAURI_INTERNALS__.invoke(cmd,args);
      const wait=async(wanted)=>{const end=Date.now()+15000;while(Date.now()<end){const state=await invoke('note_summary_get',{id:summaryId});if(wanted.includes(state.status))return state;await new Promise(done=>setTimeout(done,50));}throw Error('Summary timeout');};
      if((await invoke('summary_readiness_get')).state!=='needs-setup')throw Error('Default setup was ready');
      let refused=false;try{await invoke('note_summary_generate',{id:summaryId,revision:0,replaceEdited:false,retry:false});}catch{refused=true;}
      if(!refused||(await invoke('note_summary_get',{id:summaryId})).status!==null)throw Error('Unconfigured generation was queued');
      const config={enabled:false,provider:'self-hosted',model:'fixture',selfHostedUrl:summaryEndpoint,enterprise:false,language:'en',contextBudget:8192};
      await invoke('summary_settings_set',{settings:config});
      if((await invoke('summary_readiness_get')).state!=='ready')throw Error('Configured self-hosted setup was not ready');
      await invoke('note_summary_generate',{id:summaryId,revision:0,replaceEdited:false,retry:false});
      const completed=await wait(['succeeded','failed']);if(completed.status!=='succeeded')throw Error(completed.error);
      if(completed.total<3||completed.completed!==completed.total||!completed.content.includes('#summary-source-'))throw Error('Long-summary progress or sources missing');
      const edited=await invoke('note_summary_update',{id:summaryId,revision:completed.revision,content:'Edited summary protected'});
      await invoke('summary_settings_set',{settings:{...config,model:'bad'}});
      await invoke('note_summary_generate',{id:summaryId,revision:edited.revision,replaceEdited:true,retry:false});
      const failed=await wait(['failed']);if(failed.content!==edited.content)throw Error('Failed regeneration lost edits');
      await invoke('summary_settings_set',{settings:{...config,model:'delayed'}});
      await invoke('note_summary_generate',{id:summaryId,revision:failed.revision,replaceEdited:true,retry:false});
      await wait(['processing']);await invoke('note_summary_cancel',{id:summaryId});
      if((await invoke('note_summary_get',{id:summaryId})).status!=='cancelled')throw Error('Cancellation failed');
      if((await invoke('summary_settings_get')).enabled)throw Error('Manual generation enabled automatic summaries');
      await invoke('native_summary_report',{passed:true,message:'readiness, rejected enqueue, multi-part generation, sources, protected edits and cancellation passed'});
    })().catch(error=>window.__TAURI_INTERNALS__.invoke('native_summary_report',{passed:false,message:String(error)}));"#;
    let mut context = tauri::generate_context!();
    context.config_mut().build.dev_url = Some("http://127.0.0.1:5195/".parse().unwrap());
    context.config_mut().app.windows[0].visible = false;
    context.config_mut().app.windows[0].create = true;
    context.config_mut().app.windows[0].data_directory = Some(temp.path().join("webview"));
    let once = Arc::new(AtomicBool::new(false));
    let app = tauri::Builder::default()
        .any_thread()
        .manage(state)
        .manage(llm)
        .manage(SummaryProof(finished.clone()))
        .invoke_handler(tauri::generate_handler![
            native_summary_report,
            crate::summaries::summary_readiness_get,
            crate::summaries::summary_settings_get,
            crate::summaries::summary_settings_set,
            crate::summaries::note_summary_get,
            crate::summaries::note_summary_generate,
            crate::summaries::note_summary_update,
            crate::summaries::note_summary_cancel
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
            crate::summaries::start(app.handle().clone(), worker_state, worker_llm);
            let handle = app.handle().clone();
            std::thread::spawn(move || {
                let end = std::time::Instant::now() + std::time::Duration::from_secs(35);
                while std::time::Instant::now() < end && !observed.load(Ordering::SeqCst) {
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
                handle.exit(0);
            });
            Ok(())
        })
        .build(context)
        .unwrap();
    app.run_return(|_, _| {});
    stopping.store(true, Ordering::SeqCst);
    server.join().unwrap();
    assert!(
        finished.load(Ordering::SeqCst),
        "Native summary proof failed"
    );
    assert!(
        requests.load(Ordering::SeqCst) > 1,
        "No hierarchical requests were observed"
    );
}
