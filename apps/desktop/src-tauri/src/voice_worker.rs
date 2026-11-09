//! Bounded pipe protocol. Native libraries are loaded only in worker_main.
use kw_notes_core::known_voices::{Sample, VoiceTurn, DIM, MODEL_ID};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

pub const LIMIT: usize = 16 * 1024 * 1024;
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub version: u32,
    pub library: PathBuf,
    pub model: PathBuf,
    pub wav: PathBuf,
    pub sha256: String,
    pub turns: Vec<VoiceTurn>,
    pub duration: f64,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Response {
    pub version: u32,
    pub model_id: String,
    pub samples: Vec<Sample>,
}
pub fn hash_file(path: &Path) -> Result<String, String> {
    let mut input = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut hash = Sha256::new();
    let mut buffer = [0u8; 65536];
    loop {
        let size = input.read(&mut buffer).map_err(|e| e.to_string())?;
        if size == 0 {
            break;
        }
        hash.update(buffer.get(..size).ok_or("Invalid read length")?);
    }
    Ok(format!("{:x}", hash.finalize()))
}
pub fn validate_request(r: &Request) -> Result<(), String> {
    if r.version != 1
        || !r.duration.is_finite()
        || r.duration <= 0.0
        || r.turns.is_empty()
        || r.sha256.len() != 64
        || !r.sha256.bytes().all(|b| b.is_ascii_hexdigit())
    {
        return Err("Invalid voice request".into());
    }
    if r.turns.iter().any(|t| {
        !t.start_seconds.is_finite()
            || !t.end_seconds.is_finite()
            || t.start_seconds < 0.0
            || t.end_seconds <= t.start_seconds
            || t.end_seconds > r.duration
    }) {
        return Err("Invalid speaker turns".into());
    }
    Ok(())
}
fn validate_response(r: Response, request: &Request) -> Result<Vec<Sample>, String> {
    if r.version != 1 || r.model_id != MODEL_ID || r.samples.len() > request.turns.len() {
        return Err("Incompatible voice worker response".into());
    }
    let mut seen = std::collections::HashSet::new();
    for s in &r.samples {
        if !seen.insert(&s.speaker_id)
            || !request
                .turns
                .iter()
                .any(|t| s.speaker_id == format!("speaker_{}", t.speaker))
            || s.embedding.len() != DIM
            || s.embedding.iter().any(|v| !v.is_finite())
            || !s.seconds.is_finite()
            || s.seconds <= 0.0
            || s.seconds > request.duration
            || kw_notes_core::known_voices::normalize(&s.embedding).is_none()
        {
            return Err("Invalid voice worker sample".into());
        }
    }
    Ok(r.samples)
}
fn worker() -> Result<(), String> {
    let mut bytes = Vec::new();
    std::io::stdin()
        .take((LIMIT + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > LIMIT {
        return Err("Voice request too large".into());
    }
    let request: Request = serde_json::from_slice(&bytes).map_err(|_| "Invalid voice protocol")?;
    validate_request(&request)?;
    if hash_file(&request.wav)? != request.sha256 {
        return Err("Recording changed before extraction".into());
    }
    let digest = kw_sidecar::Sha256Digest::parse(
        "357a834f702b80161e5b981182c038e18553c1f2ca752ed6cec2052365d4129b",
    )
    .map_err(|e| e.to_string())?;
    kw_sidecar::integrity::verify(&request.model, 29596978, &digest).map_err(|e| e.to_string())?;
    let turns = request
        .turns
        .iter()
        .map(|t| kw_meeting_core::diarization::Turn {
            start_seconds: t.start_seconds,
            end_seconds: t.end_seconds,
            speaker: t.speaker,
        })
        .collect::<Vec<_>>();
    let samples = crate::known_voices_native::extract(
        &request.library,
        &request.model,
        &request.wav,
        &turns,
    )?;
    if hash_file(&request.wav)? != request.sha256 {
        return Err("Recording changed during extraction".into());
    }
    let response = Response {
        version: 1,
        model_id: MODEL_ID.into(),
        samples,
    };
    let bytes = serde_json::to_vec(&response).map_err(|e| e.to_string())?;
    if bytes.len() > LIMIT {
        return Err("Voice response too large".into());
    }
    std::io::stdout()
        .write_all(&bytes)
        .map_err(|e| e.to_string())
}
pub fn worker_main() -> std::process::ExitCode {
    match worker() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(_) => {
            eprintln!("Voice extraction failed");
            std::process::ExitCode::FAILURE
        }
    }
}
pub async fn run(request: &Request) -> Result<Vec<Sample>, String> {
    validate_request(request)?;
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    run_program(
        &exe,
        request,
        Duration::from_secs_f64((30.0 + request.duration * 0.1).clamp(30.0, 600.0)),
    )
    .await
}
// Also used by the offline example and fault-containment integration checks.
pub async fn run_program(
    exe: &Path,
    request: &Request,
    deadline: Duration,
) -> Result<Vec<Sample>, String> {
    validate_request(request)?;
    let bytes = serde_json::to_vec(request).map_err(|e| e.to_string())?;
    if bytes.len() > LIMIT {
        return Err("Voice request too large".into());
    }
    let mut cmd = tokio::process::Command::new(exe);
    cmd.arg("--known-voices-worker")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(windows)]
    cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
    let mut child = cmd.spawn().map_err(|e| e.to_string())?;
    #[cfg(windows)]
    let _job = match job::Job::assign(&child) {
        Ok(job) => job,
        Err(e) => {
            let _ = child.kill().await;
            let _ = child.wait().await;
            return Err(e);
        }
    };
    // The worker cannot begin native work until we supply stdin, after job assignment.
    let mut stdin = child.stdin.take().ok_or("Missing worker input")?;
    let stdout = child.stdout.take().ok_or("Missing worker output")?;
    let stderr = child.stderr.take().ok_or("Missing worker diagnostics")?;
    async fn read(mut stream: impl tokio::io::AsyncRead + Unpin) -> Result<Vec<u8>, String> {
        let mut bytes = Vec::new();
        (&mut stream)
            .take((LIMIT + 1) as u64)
            .read_to_end(&mut bytes)
            .await
            .map_err(|e| e.to_string())?;
        if bytes.len() > LIMIT {
            return Err("Voice worker exceeded output limit".into());
        }
        Ok(bytes)
    }
    let result = tokio::time::timeout(deadline, async {
        let send = async {
            stdin.write_all(&bytes).await.map_err(|e| e.to_string())?;
            drop(stdin);
            Ok::<(), String>(())
        };
        let (_, out, _) = tokio::try_join!(send, read(stdout), read(stderr))?;
        let status = child.wait().await.map_err(|e| e.to_string())?;
        if !status.success() {
            return Err("Voice worker failed; the transcript was preserved".into());
        }
        validate_response(
            serde_json::from_slice(&out).map_err(|_| "Invalid voice worker output")?,
            request,
        )
    })
    .await;
    match result {
        Ok(Ok(samples)) => Ok(samples),
        other => {
            let _ = child.kill().await;
            let _ = child.wait().await;
            match other {
                Ok(Err(e)) => Err(e),
                _ => Err("Voice extraction timed out".into()),
            }
        }
    }
}
#[cfg(windows)]
mod job {
    use windows::Win32::{
        Foundation::{CloseHandle, HANDLE},
        System::JobObjects::*,
    };
    pub struct Job(HANDLE);
    impl Job {
        pub fn assign(child: &tokio::process::Child) -> Result<Self, String> {
            unsafe {
                let handle = CreateJobObjectW(None, None).map_err(|e| e.to_string())?;
                let job = Self(handle);
                let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
                info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
                SetInformationJobObject(
                    handle,
                    JobObjectExtendedLimitInformation,
                    &info as *const _ as *const _,
                    std::mem::size_of_val(&info) as u32,
                )
                .map_err(|e| e.to_string())?;
                let process = child.raw_handle().ok_or("Missing worker process handle")?;
                AssignProcessToJobObject(handle, HANDLE(process)).map_err(|e| e.to_string())?;
                Ok(job)
            }
        }
    }
    // A kernel handle has no thread affinity; the guard owns it exclusively.
    unsafe impl Send for Job {}
    impl Drop for Job {
        fn drop(&mut self) {
            unsafe {
                let _ = CloseHandle(self.0);
            }
        }
    }
}
