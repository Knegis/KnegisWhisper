use crate::state::AppState;
use kw_notes_core::known_voices as voices;
use tauri::State;

/// The setting `kw_notes_core::known_voices` reads. It is written out here as well
/// because tools/settings-audit/scan.mjs reads constants from this crate's source
/// only; `key_matches_notes_core` keeps the two equal.
pub const KNOWN_VOICES_ENABLED_KEY: &str = "meetings.knownVoices";

#[derive(Debug, serde::Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct KnownVoicesStatus {
    pub enabled: bool,
    pub ready: bool,
    pub reason: Option<String>,
    pub voices: Vec<voices::KnownVoice>,
    pub enrollment_reason: Option<String>,
}
fn blocker() -> Option<String> {
    if !cfg!(windows) {
        return Some("Known voices currently requires Windows.".into());
    }
    if voices::RELEASE_POLICY.is_none() {
        return Some(
            "Known voices is awaiting validation on labeled recordings from separate days.".into(),
        );
    }
    if !crate::diarization::voice_runtime_ready() {
        return Some(
            "Install the Known voices runtime to recognize returning participants.".into(),
        );
    }
    crate::diarization::voice_model().err()
}
#[tauri::command]
pub async fn known_voices_status(
    state: State<'_, AppState>,
    note_id: Option<i64>,
    speaker_id: Option<String>,
) -> Result<KnownVoicesStatus, String> {
    let reason = blocker();
    let reason = if reason.is_none() && !crate::diarization::is_enabled(state.db()).await {
        Some("Enable Speaker labels first.".into())
    } else {
        reason
    };
    Ok(KnownVoicesStatus {
        enrollment_reason: enrollment_reason(&state, note_id, speaker_id.as_deref()).await,
        enabled: voices::enabled(state.db().pool()).await,
        ready: reason.is_none(),
        reason,
        voices: voices::list(state.db().pool())
            .await
            .map_err(|e| e.to_string())?,
    })
}
async fn enrollment_reason(
    state: &AppState,
    note_id: Option<i64>,
    speaker: Option<&str>,
) -> Option<String> {
    let Some(note_id) = note_id else {
        return Some("Open a saved meeting to remember a voice.".into());
    };
    let source = match voices::source(state.db().pool(), note_id).await {
        Ok(source) => source,
        Err(e) => return Some(e.to_string()),
    };
    if source.pipeline != voices::MODEL_ID {
        return Some("Record a new meeting to remember this voice.".into());
    }
    if !source
        .turns
        .iter()
        .any(|t| speaker == Some(format!("speaker_{}", t.speaker).as_str()))
    {
        return Some("This speaker has no original voice turns. Record a new meeting to remember their voice.".into());
    }
    let recording = match kw_notes_core::recordings::get(state.db().pool(), note_id).await {
        Ok(Some(r)) => r,
        _ => return Some("Keep the meeting recording to remember a voice.".into()),
    };
    if source.storage_key != recording.storage_key
        || !kw_notes_core::recordings::resolve(state.recording_directory(), &recording.storage_key)
            .is_some_and(|p| p.is_file())
    {
        return Some("The original meeting recording is unavailable. Record a new meeting to remember this voice.".into());
    }
    None
}
#[tauri::command]
pub async fn known_voices_set_enabled(
    state: State<'_, AppState>,
    enabled: bool,
) -> Result<(), String> {
    if enabled {
        if let Some(reason) = blocker() {
            return Err(reason);
        }
        if !crate::diarization::is_enabled(state.db()).await {
            return Err("Enable Speaker labels first".into());
        }
    }
    kw_storage::settings::set(state.db().pool(), KNOWN_VOICES_ENABLED_KEY, &enabled)
        .await
        .map_err(|e| e.to_string())
}
#[tauri::command]
pub async fn known_voice_enroll(
    state: State<'_, AppState>,
    note_id: i64,
    speaker_id: String,
    name: String,
    replace_id: Option<i64>,
) -> Result<i64, String> {
    if let Some(reason) = blocker() {
        return Err(reason);
    }
    let policy = voices::RELEASE_POLICY.ok_or("Known voices has not passed validation")?;
    let expected_generation = voices::generation(state.db().pool())
        .await
        .map_err(|e| e.to_string())?;
    if !voices::enabled(state.db().pool()).await {
        return Err("Enable Known voices first".into());
    }
    let source = voices::source(state.db().pool(), note_id)
        .await
        .map_err(|e| e.to_string())?;
    let recording = kw_notes_core::recordings::get(state.db().pool(), note_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or("Keep the meeting recording to remember a voice")?;
    if recording.storage_key != source.storage_key || source.pipeline != voices::MODEL_ID {
        return Err("The recording or analysis version changed. Record a new meeting to remember this voice.".into());
    }
    let wav =
        kw_notes_core::recordings::resolve(state.recording_directory(), &recording.storage_key)
            .ok_or("Invalid recording attachment")?;
    let _lease = crate::voice_runtime::lease()?;
    let request = request_for(&wav, &source)?;
    // Preserve every turn so overlap exclusion is identical to recognition.
    let samples = crate::voice_worker::run(&request).await?;
    let sample = samples
        .iter()
        .find(|s| s.speaker_id == speaker_id)
        .ok_or("No usable speech for this speaker")?;
    // The worker verified the file before and after extraction. Recheck again at
    // the enrollment boundary; DB source, attachment, and generation are checked in its transaction.
    if crate::voice_worker::hash_file(&wav)? != source.sha256 {
        return Err("Recording changed during enrollment".into());
    }
    voices::enroll(
        state.db().pool(),
        note_id,
        &speaker_id,
        &name,
        replace_id,
        policy,
        sample,
        &source,
        expected_generation,
    )
    .await
    .map_err(|e| e.to_string())
}
#[tauri::command]
pub async fn known_voice_rename(
    state: State<'_, AppState>,
    id: i64,
    name: String,
) -> Result<(), String> {
    voices::rename(state.db().pool(), id, &name)
        .await
        .map_err(|e| e.to_string())
}
#[tauri::command]
pub async fn known_voice_delete(state: State<'_, AppState>, id: i64) -> Result<(), String> {
    voices::delete(state.db().pool(), id)
        .await
        .map_err(|e| e.to_string())
}
#[tauri::command]
pub async fn known_voices_repair(
    asr: State<'_, std::sync::Arc<crate::local_asr::LocalAsr>>,
) -> Result<(), String> {
    crate::voice_runtime::install(&asr).await
}
fn request_for(
    wav: &std::path::Path,
    source: &voices::Source,
) -> Result<crate::voice_worker::Request, String> {
    Ok(crate::voice_worker::Request {
        version: 1,
        library: crate::diarization::voice_library(),
        model: crate::diarization::voice_model()?,
        wav: wav.to_owned(),
        sha256: source.sha256.clone(),
        turns: source.turns.clone(),
        duration: source.duration,
    })
}
pub async fn extract(
    db: &kw_storage::Db,
    wav: &std::path::Path,
    turns: &[kw_meeting_core::diarization::Turn],
    recognize: bool,
) -> Result<voices::Analysis, String> {
    extract_with_policy(db, wav, turns, recognize, voices::RELEASE_POLICY).await
}
async fn extract_with_policy(
    db: &kw_storage::Db,
    wav: &std::path::Path,
    turns: &[kw_meeting_core::diarization::Turn],
    recognize: bool,
    policy: Option<voices::Policy>,
) -> Result<voices::Analysis, String> {
    if policy.is_none() {
        return Ok(voices::Analysis::default());
    }
    let generation = voices::generation(db.pool())
        .await
        .map_err(|e| e.to_string())?;
    if !voices::enabled(db.pool()).await {
        return Ok(voices::Analysis::default());
    }
    let path = wav.to_owned();
    let (hash, duration) = tokio::task::spawn_blocking(move || {
        let hash = crate::voice_worker::hash_file(&path)?;
        let reader = hound::WavReader::open(&path).map_err(|e| e.to_string())?;
        let duration = f64::from(reader.duration()) / f64::from(reader.spec().sample_rate);
        Ok::<_, String>((hash, duration))
    })
    .await
    .map_err(|e| e.to_string())??;
    let source = voices::Source {
        storage_key: kw_notes_core::recordings::storage_key_for(wav)
            .ok_or("Invalid recording path")?,
        sha256: hash,
        pipeline: voices::MODEL_ID.into(),
        turns: turns
            .iter()
            .map(|t| voices::VoiceTurn {
                start_seconds: t.start_seconds,
                end_seconds: t.end_seconds,
                speaker: t.speaker,
            })
            .collect(),
        duration,
    };
    let mut analysis = voices::Analysis {
        source: Some(source.clone()),
        generation,
        ..Default::default()
    };
    if !recognize {
        return Ok(analysis);
    } // Recovery never retries native recognition.
    let result = async {
        let _lease = crate::voice_runtime::lease()?;
        crate::voice_worker::run(&request_for(wav, &source)?).await
    }
    .await;
    match result {
        Ok(samples) => analysis.samples = samples,
        Err(e) => {
            analysis.warning = Some(format!(
                "Known voices could not run: {e}. Speaker labels were preserved."
            ))
        }
    }
    Ok(analysis)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    #[test]
    fn key_matches_notes_core() {
        assert_eq!(
            super::KNOWN_VOICES_ENABLED_KEY,
            super::voices::KNOWN_VOICES_ENABLED_KEY
        );
    }

    #[tokio::test]
    async fn recovery_saves_provenance_without_loading_native_libraries() {
        let db = kw_storage::Db::open_in_memory().await.unwrap();
        kw_storage::settings::set(db.pool(), super::KNOWN_VOICES_ENABLED_KEY, &true)
            .await
            .unwrap();
        let dir = tempfile::tempdir().unwrap();
        let wav = dir.path().join("recovery.wav");
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 16000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writer = hound::WavWriter::create(&wav, spec).unwrap();
        for _ in 0..80000 {
            writer.write_sample(0_i16).unwrap();
        }
        writer.finalize().unwrap();
        let turns = vec![kw_meeting_core::diarization::Turn {
            start_seconds: 0.0,
            end_seconds: 5.0,
            speaker: 0,
        }];
        let policy = Some(super::voices::Policy {
            threshold: 0.9,
            margin: 0.1,
            min_seconds: 3.0,
        });
        let recovered = super::extract_with_policy(&db, &wav, &turns, false, policy)
            .await
            .unwrap();
        assert!(recovered.samples.is_empty());
        assert!(recovered.warning.is_none());
        assert_eq!(recovered.source.unwrap().storage_key, "recovery.wav");
        // Off and release-gated paths must not even attempt to open a WAV.
        kw_storage::settings::set(db.pool(), super::KNOWN_VOICES_ENABLED_KEY, &false)
            .await
            .unwrap();
        assert!(super::extract_with_policy(
            &db,
            std::path::Path::new("missing.wav"),
            &turns,
            true,
            policy
        )
        .await
        .unwrap()
        .source
        .is_none());
        kw_storage::settings::set(db.pool(), super::KNOWN_VOICES_ENABLED_KEY, &true)
            .await
            .unwrap();
        assert!(
            super::extract(&db, std::path::Path::new("missing.wav"), &turns, true)
                .await
                .unwrap()
                .source
                .is_none()
        );
    }
}
