//! Faults live exclusively in this test executable, never in the shipped worker.
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)]
use knegiswhisper_lib::voice_worker::{run_program, Request, LIMIT};
use kw_notes_core::known_voices::{VoiceTurn, DIM, MODEL_ID};
use std::{
    io::{Read, Write},
    time::Duration,
};
fn fixture() {
    let mut bytes = Vec::new();
    std::io::stdin().read_to_end(&mut bytes).unwrap();
    let r: Request = serde_json::from_slice(&bytes).unwrap();
    std::fs::write(&r.model, std::process::id().to_string()).unwrap();
    let mode = r.library.to_str().unwrap();
    match mode {
        "abort" => std::process::abort(),
        "hang" => {
            std::thread::sleep(Duration::from_secs(60));
            return;
        }
        "malformed" => {
            print!("not JSON");
            return;
        }
        "oversize" => {
            std::io::stdout().write_all(&vec![b'x'; LIMIT + 1]).unwrap();
            return;
        }
        "stderr" => {
            std::io::stderr().write_all(&vec![b'x'; LIMIT + 1]).unwrap();
            return;
        }
        _ => {}
    }
    let mut vector = vec![0.0; DIM];
    vector[0] = 1.0;
    let mut response = serde_json::json!({"version":1,"model_id":MODEL_ID,"samples":[{"speaker_id":"speaker_0","embedding":vector,"seconds":5.0}]});
    match mode {
        "version" => response["version"] = 2.into(),
        "model" => response["model_id"] = "wrong".into(),
        "speaker" => response["samples"][0]["speaker_id"] = "speaker_99".into(),
        "dimension" => response["samples"][0]["embedding"] = serde_json::json!([1.0]),
        "zero" => response["samples"][0]["embedding"] = serde_json::json!(vec![0.0; DIM]),
        "duration" => response["samples"][0]["seconds"] = 100.0.into(),
        "duplicate" => {
            let sample = response["samples"][0].clone();
            response["samples"].as_array_mut().unwrap().push(sample);
        }
        _ => {}
    }
    print!("{response}");
}
#[cfg(windows)]
fn exited(pid: u32) -> bool {
    unsafe {
        use windows::Win32::{
            Foundation::{CloseHandle, WAIT_OBJECT_0},
            System::Threading::{OpenProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE},
        };
        match OpenProcess(PROCESS_SYNCHRONIZE, false, pid) {
            Ok(handle) => {
                let done = WaitForSingleObject(handle, 0) == WAIT_OBJECT_0;
                let _ = CloseHandle(handle);
                done
            }
            Err(_) => true,
        }
    }
}
#[cfg(not(windows))]
fn exited(_pid: u32) -> bool {
    true
}
#[tokio::main]
async fn main() {
    if std::env::args().nth(1).as_deref() == Some("--known-voices-worker") {
        fixture();
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let exe = std::env::current_exe().unwrap();
    let mut r = Request {
        version: 1,
        library: "ok".into(),
        model: dir.path().join("pid"),
        wav: "unused.wav".into(),
        sha256: "a".repeat(64),
        turns: vec![VoiceTurn {
            start_seconds: 0.0,
            end_seconds: 5.0,
            speaker: 0,
        }],
        duration: 5.0,
    };
    let good = run_program(&exe, &r, Duration::from_secs(10))
        .await
        .unwrap();
    assert_eq!(good.len(), 1);
    for mode in [
        "abort",
        "malformed",
        "oversize",
        "stderr",
        "version",
        "model",
        "speaker",
        "dimension",
        "zero",
        "duration",
        "duplicate",
    ] {
        r.library = mode.into();
        assert!(
            run_program(&exe, &r, Duration::from_secs(10))
                .await
                .is_err(),
            "accepted {mode}"
        );
        let pid = std::fs::read_to_string(&r.model).unwrap().parse().unwrap();
        assert!(exited(pid), "worker survived {mode}");
    }
    r.library = "hang".into();
    let started = std::time::Instant::now();
    assert!(run_program(&exe, &r, Duration::from_millis(500))
        .await
        .unwrap_err()
        .contains("timed out"));
    assert!(started.elapsed() < Duration::from_secs(5));
    assert!(exited(
        std::fs::read_to_string(&r.model).unwrap().parse().unwrap()
    ));
    std::fs::remove_file(&r.model).unwrap();
    let task = tokio::spawn({
        let r = r.clone();
        let exe = exe.clone();
        async move { run_program(&exe, &r, Duration::from_secs(30)).await }
    });
    let started = std::time::Instant::now();
    while !r.model.exists() {
        assert!(started.elapsed() < Duration::from_secs(5));
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let pid = std::fs::read_to_string(&r.model).unwrap().parse().unwrap();
    task.abort();
    let _ = task.await;
    let started = std::time::Instant::now();
    while !exited(pid) {
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "cancelled worker survived"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    r.duration = f64::NAN;
    assert!(run_program(&exe, &r, Duration::from_secs(10))
        .await
        .is_err());
    println!("voice worker: valid response, 11 fault modes, timeout/reap, cancellation/job termination, invalid request passed");
}
