//! Attach an explicitly picked WAV without exposing a filesystem path over IPC.
//! The original survives; a generated copy belongs to exactly one note. Blocking
//! copy/validation runs off the webview thread, with bounded memory use.

use std::fs::{File, OpenOptions};
use std::io::BufReader;
use std::path::{Path, PathBuf};

use kw_notes_core::{notes, recordings, NoteRecording};
use kw_storage::Db;

pub(crate) async fn ensure_available(db: &Db, id: i64) -> Result<(), String> {
    if notes::get(db.pool(), id)
        .await
        .map_err(|e| format!("Could not read the note: {e}"))?
        .is_none()
    {
        return Err("This note no longer exists.".to_owned());
    }
    if recordings::get(db.pool(), id)
        .await
        .map_err(|e| format!("Could not read the recording: {e}"))?
        .is_some()
    {
        return Err(
            "This note already has a recording. Existing recordings are not replaced.".to_owned(),
        );
    }
    Ok(())
}

pub(crate) struct PendingCopy {
    pub(crate) path: PathBuf,
    pub(crate) key: String,
    pub(crate) original_name: String,
    pub(crate) duration: f64,
    pub(crate) committed: bool,
}

impl Drop for PendingCopy {
    fn drop(&mut self) {
        if !self.committed {
            // path was resolved from a generated key, never supplied by IPC.
            if let Err(error) = std::fs::remove_file(&self.path) {
                if error.kind() != std::io::ErrorKind::NotFound {
                    tracing::warn!(storage_key = %self.key, error = %error, "could not remove an unattached recording copy");
                }
            }
        }
    }
}

fn validate_wav(path: &Path) -> Result<f64, String> {
    let file = File::open(path).map_err(|e| format!("Could not open the recording copy: {e}"))?;
    let mut reader = hound::WavReader::new(BufReader::new(file))
        .map_err(|e| format!("Choose a valid WAV recording: {e}"))?;
    let spec = reader.spec();
    if spec.sample_format != hound::SampleFormat::Int
        || spec.bits_per_sample != 16
        || !matches!(spec.channels, 1 | 2)
        || spec.sample_rate == 0
    {
        return Err(
            "Choose an uncompressed 16-bit PCM WAV recording with one or two channels.".to_owned(),
        );
    }
    if reader.len() == 0 || reader.len() % u32::from(spec.channels) != 0 {
        return Err("The WAV recording has no complete audio frames.".to_owned());
    }
    let duration = f64::from(reader.duration()) / f64::from(spec.sample_rate);
    // Parsing the header alone accepts truncated data. Read every declared
    // sample, without collecting it, before attaching the copy.
    for sample in reader.samples::<i16>() {
        sample.map_err(|e| format!("The WAV recording is damaged or truncated: {e}"))?;
    }
    Ok(duration)
}

pub(crate) fn prepare_copy(source: &Path, directory: &Path) -> Result<PendingCopy, String> {
    let mut input =
        File::open(source).map_err(|e| format!("Could not open the selected recording: {e}"))?;
    if !input
        .metadata()
        .map_err(|e| format!("Could not inspect the recording: {e}"))?
        .is_file()
    {
        return Err("Choose a WAV file, not a directory or device.".to_owned());
    }
    let original_name = source
        .file_name()
        .ok_or("The selected recording has no filename.")?
        .to_string_lossy()
        .into_owned();
    std::fs::create_dir_all(directory)
        .map_err(|e| format!("Could not create the recordings directory: {e}"))?;
    let key = format!("{}.wav", uuid::Uuid::new_v4());
    let path =
        recordings::resolve(directory, &key).ok_or("Could not generate a recording filename.")?;
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|e| format!("Could not create the recording copy: {e}"))?;
    let mut pending = PendingCopy {
        path,
        key,
        original_name,
        duration: 0.0,
        committed: false,
    };
    let copied = std::io::copy(&mut input, &mut output).and_then(|_| output.sync_all());
    // Close before validation or Drop attempts removal, including on Windows.
    drop(output);
    copied.map_err(|e| format!("Could not copy the recording; check available disk space: {e}"))?;
    pending.duration = validate_wav(&pending.path)?;
    Ok(pending)
}

/// The selection comes only from the Rust dialog. An owned task finishes the
/// copy/insert/cleanup even if the requesting view goes away while it is pending.
pub(crate) async fn from_selection(
    db: Db,
    id: i64,
    selected: Option<PathBuf>,
    directory: PathBuf,
) -> Result<Option<NoteRecording>, String> {
    let Some(source) = selected else {
        return Ok(None);
    };
    tokio::spawn(async move {
        ensure_available(&db, id).await?;
        let mut pending = tokio::task::spawn_blocking(move || prepare_copy(&source, &directory))
            .await.map_err(|e| format!("Recording copy failed: {e}"))??;
        // Foreign key and unique constraints enforce eligibility again after
        // the copy, including simultaneous attachment and note deletion.
        let recording = recordings::attach_and_get(db.pool(), id, &pending.key,
            Some(pending.duration), Some(&pending.original_name)).await
            .map_err(|e| format!("Could not attach the recording. The note may have been deleted or already attached: {e}"))?;
        pending.committed = true;
        Ok(Some(recording))
    }).await.map_err(|e| format!("Recording attachment failed: {e}"))?
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use super::*;

    #[cfg(feature = "tauri-test")]
    // get_ipc_response blocks its caller; keep the SQL pool driver runnable.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn desktop_delete_removes_an_attached_copy_through_real_ipc() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("original.wav");
        let root = dir.path().join("recordings");
        wav(&source, 1, 16);
        let state = crate::state::AppState::in_memory()
            .await
            .unwrap()
            .with_recording_directory(root.clone());
        let db = state.db().clone();
        let id = note(&db).await;
        let recording = from_selection(db.clone(), id, Some(source.clone()), root.clone())
            .await
            .unwrap()
            .unwrap();
        let app = tauri::test::mock_builder()
            .manage(state)
            .invoke_handler(tauri::generate_handler![crate::commands::notes_delete])
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .unwrap();
        let webview = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
            .build()
            .unwrap();
        let result = tauri::test::get_ipc_response(
            &webview,
            tauri::webview::InvokeRequest {
                cmd: "notes_delete".to_owned(),
                callback: tauri::ipc::CallbackFn(0),
                error: tauri::ipc::CallbackFn(1),
                url: "http://tauri.localhost".parse().unwrap(),
                body: tauri::ipc::InvokeBody::Json(serde_json::json!({"id": id})),
                headers: Default::default(),
                invoke_key: tauri::test::INVOKE_KEY.to_owned(),
            },
        )
        .unwrap()
        .deserialize::<bool>()
        .unwrap();
        assert!(result);
        assert!(notes::get(db.pool(), id).await.unwrap().is_none());
        assert!(!root.join(recording.storage_key).exists());
        assert!(source.exists());
    }

    /// Start Vite with `npm run dev`, then explicitly run this ignored test.
    /// Notes and read-only diagnostics are registered: no capture, sidecars,
    /// discovery file or local HTTP server is started. Close the window to finish.
    #[cfg(all(windows, feature = "tauri-test"))]
    #[test]
    #[ignore = "interactive Windows picker and playback check; requires Vite on port 5183"]
    fn native_recording_attachment_smoke() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("recordings");
        let source = dir.path().join("original möte.wav");
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../crates/transcription-core/tests/fixtures/speech-16k-mono.wav");
        std::fs::copy(fixture, &source).unwrap();
        let original = std::fs::read(&source).unwrap();
        let state = tauri::async_runtime::block_on(crate::state::AppState::in_memory())
            .unwrap()
            .with_recording_directory(root.clone());
        let db = state.db().clone();
        let id = tauri::async_runtime::block_on(note(&db));
        println!(
            "SMOKE: open Notes > Older note, cancel Attach once, then attach {}",
            source.display()
        );
        println!("SMOKE: play the transcript row, close/reopen the note, then close the window.");
        let mut context = tauri::generate_context!();
        context.config_mut().app.windows[0].title =
            "KnegisWhisper — recording smoke test".to_owned();
        let exit_code = tauri::Builder::default()
            .any_thread()
            .plugin(tauri_plugin_dialog::init())
            .manage(state)
            .register_asynchronous_uri_scheme_protocol(
                crate::recordings::SCHEME,
                crate::recordings::serve,
            )
            .invoke_handler(tauri::generate_handler![
                crate::diagnostics::diagnostics,
                crate::commands::notes_list,
                crate::commands::notes_search,
                crate::commands::notes_get,
                crate::commands::notes_update,
                crate::commands::notes_delete,
                crate::commands::folders_list,
                crate::commands::note_transcript_rows,
                crate::commands::note_recording_get,
                crate::commands::note_recording_attach_from_dialog,
            ])
            .build(context)
            .unwrap()
            .run_return(|_, _| {});
        assert_eq!(exit_code, 0);
        let attached = tauri::async_runtime::block_on(recordings::get(db.pool(), id))
            .unwrap()
            .unwrap();
        assert!(root.join(&attached.storage_key).exists());
        assert_eq!(std::fs::read(&source).unwrap(), original);
        assert_eq!(std::fs::read_dir(&root).unwrap().count(), 1);
        println!("SMOKE: attachment persisted; exactly one copy; original unchanged.");
    }

    async fn note(db: &Db) -> i64 {
        notes::create(
            db.pool(),
            &notes::NewNote {
                title: "Older note".to_owned(),
                content: "Keep this Markdown".to_owned(),
                transcript: Some(r#"[{"text":"hello","timestamp":0.0}]"#.to_owned()),
                ..Default::default()
            },
        )
        .await
        .unwrap()
    }

    fn wav(path: &Path, channels: u16, bits_per_sample: u16) {
        let mut writer = hound::WavWriter::create(
            path,
            hound::WavSpec {
                channels,
                sample_rate: 16_000,
                bits_per_sample,
                sample_format: hound::SampleFormat::Int,
            },
        )
        .unwrap();
        for _ in 0..(16_000 * u32::from(channels)) {
            writer.write_sample(100_i16).unwrap();
        }
        writer.finalize().unwrap();
    }

    #[tokio::test]
    async fn attaches_unicode_wav_preserves_original_and_plays_through_protocol() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("recordings");
        let source = dir.path().join("möte med Åsa.wav");
        wav(&source, 2, 16);
        let original = std::fs::read(&source).unwrap();
        let db = Db::open_in_memory().await.unwrap();
        let id = note(&db).await;
        let before = notes::get(db.pool(), id).await.unwrap();
        let recording = from_selection(db.clone(), id, Some(source.clone()), root.clone())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(recording.duration_seconds, Some(1.0));
        assert_eq!(
            recording.original_filename.as_deref(),
            Some("möte med Åsa.wav")
        );
        assert_eq!(
            recordings::get(db.pool(), id).await.unwrap(),
            Some(recording.clone())
        );
        assert_eq!(notes::get(db.pool(), id).await.unwrap(), before);
        let copy = recordings::resolve(&root, &recording.storage_key).unwrap();
        assert_eq!(std::fs::read(&copy).unwrap(), original);
        assert_eq!(std::fs::read(&source).unwrap(), original);
        let request = tauri::http::Request::builder()
            .header("Range", "bytes=44-63")
            .body(Vec::new())
            .unwrap();
        let response = crate::recordings::response_for(&request, &copy);
        assert_eq!(response.status(), tauri::http::StatusCode::PARTIAL_CONTENT);
        assert_eq!(response.body(), &original[44..64]);
        assert!(recordings::delete_with_recording(db.pool(), id, &root)
            .await
            .unwrap());
        assert!(!copy.exists());
        assert_eq!(std::fs::read(source).unwrap(), original);
    }

    #[tokio::test]
    async fn cancellation_missing_notes_and_missing_sources_create_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("recordings");
        let db = Db::open_in_memory().await.unwrap();
        let id = note(&db).await;
        assert!(from_selection(db.clone(), id, None, root.clone())
            .await
            .unwrap()
            .is_none());
        assert!(!root.exists());
        assert!(from_selection(
            db.clone(),
            id,
            Some(dir.path().join("missing.wav")),
            root.clone()
        )
        .await
        .is_err());
        assert!(from_selection(
            db.clone(),
            id + 1,
            Some(dir.path().join("missing.wav")),
            root.clone()
        )
        .await
        .is_err());
        assert!(!root.exists());
        assert!(recordings::get(db.pool(), id).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn malformed_truncated_and_unsupported_audio_are_cleaned_up() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("recordings");
        let source = dir.path().join("bad.wav");
        let db = Db::open_in_memory().await.unwrap();
        let id = note(&db).await;
        for case in 0..4 {
            match case {
                0 => std::fs::write(&source, b"not a wave file").unwrap(),
                1 => {
                    wav(&source, 1, 16);
                    OpenOptions::new()
                        .write(true)
                        .open(&source)
                        .unwrap()
                        .set_len(60)
                        .unwrap();
                }
                2 => wav(&source, 1, 24),
                _ => wav(&source, 3, 16),
            }
            assert!(
                from_selection(db.clone(), id, Some(source.clone()), root.clone())
                    .await
                    .is_err()
            );
            assert_eq!(std::fs::read_dir(&root).unwrap().count(), 0);
            assert!(recordings::get(db.pool(), id).await.unwrap().is_none());
            assert!(source.exists());
        }
    }

    #[tokio::test]
    async fn copy_and_database_failure_leave_no_attachment() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("recordings");
        let source = dir.path().join("source.wav");
        wav(&source, 1, 16);
        let db = Db::open_in_memory().await.unwrap();
        let id = note(&db).await;
        std::fs::write(&root, b"directory unavailable").unwrap();
        assert!(
            from_selection(db.clone(), id, Some(source.clone()), root.clone())
                .await
                .is_err()
        );
        std::fs::remove_file(&root).unwrap();
        sqlx::query("CREATE TRIGGER fail_attach BEFORE INSERT ON note_recordings BEGIN SELECT RAISE(ABORT, 'test failure'); END")
            .execute(db.pool()).await.unwrap();
        assert!(
            from_selection(db.clone(), id, Some(source.clone()), root.clone())
                .await
                .is_err()
        );
        assert_eq!(std::fs::read_dir(&root).unwrap().count(), 0);
        assert!(recordings::get(db.pool(), id).await.unwrap().is_none());
        assert!(source.exists());
    }

    #[tokio::test]
    async fn concurrent_attachments_keep_exactly_one_copy_and_never_replace_it() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("recordings");
        let source = dir.path().join("source.wav");
        wav(&source, 1, 16);
        let db = Db::open_in_memory().await.unwrap();
        let id = note(&db).await;
        let (a, b) = tokio::join!(
            from_selection(db.clone(), id, Some(source.clone()), root.clone()),
            from_selection(db.clone(), id, Some(source.clone()), root.clone()),
        );
        assert_ne!(a.is_ok(), b.is_ok());
        let attached = recordings::get(db.pool(), id).await.unwrap().unwrap();
        assert_eq!(std::fs::read_dir(&root).unwrap().count(), 1);
        assert!(root.join(&attached.storage_key).exists());
        assert!(from_selection(db.clone(), id, Some(source), root.clone())
            .await
            .is_err());
        assert_eq!(
            recordings::get(db.pool(), id).await.unwrap(),
            Some(attached)
        );
        assert_eq!(std::fs::read_dir(&root).unwrap().count(), 1);
    }
}
