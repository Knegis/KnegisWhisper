use crate::state::AppState;
use kw_notes_core::known_voices as voices;
use tauri::{AppHandle, State};

#[derive(Debug, serde::Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct KnownVoicesStatus {
    pub enabled: bool,
    pub ready: bool,
    pub reason: Option<String>,
    pub voices: Vec<voices::KnownVoice>,
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
        return Some("Repair the speaker runtime to install Known voices support.".into());
    }
    crate::diarization::voice_model().err()
}
#[tauri::command]
pub async fn known_voices_status(state: State<'_, AppState>) -> Result<KnownVoicesStatus, String> {
    let reason = blocker();
    let reason = if reason.is_none() && !crate::diarization::is_enabled(state.db()).await {
        Some("Enable Speaker labels first.".into())
    } else {
        reason
    };
    Ok(KnownVoicesStatus {
        enabled: voices::enabled(state.db().pool()).await,
        ready: reason.is_none(),
        reason,
        voices: voices::list(state.db().pool())
            .await
            .map_err(|e| e.to_string())?,
    })
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
    kw_storage::settings::set(state.db().pool(), voices::ENABLED_KEY, &enabled)
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
    voices::enroll(
        state.db().pool(),
        note_id,
        &speaker_id,
        &name,
        replace_id,
        policy,
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
    app: AppHandle,
    asr: State<'_, std::sync::Arc<crate::local_asr::LocalAsr>>,
) -> Result<(), String> {
    // The usual verified installer also upgrades older bin-only installations.
    crate::diarization::diarization_install_model(app, asr, "3dspeaker-campplus-en".into()).await
}
pub async fn extract(
    db: &kw_storage::Db,
    wav: &std::path::Path,
    turns: &[kw_meeting_core::diarization::Turn],
) -> Result<Vec<voices::Sample>, String> {
    if voices::RELEASE_POLICY.is_none() || !voices::enabled(db.pool()).await {
        return Ok(Vec::new());
    }
    let library = crate::diarization::voice_library();
    let model = crate::diarization::voice_model()?;
    let model_digest = kw_sidecar::Sha256Digest::parse(
        "357a834f702b80161e5b981182c038e18553c1f2ca752ed6cec2052365d4129b",
    )
    .map_err(|e| e.to_string())?;
    let wav = wav.to_owned();
    let turns = turns.to_vec();
    tokio::task::spawn_blocking(move || {
        kw_sidecar::integrity::verify(&model, 29596978, &model_digest)
            .map_err(|e| e.to_string())?;
        crate::known_voices_native::extract(&library, &model, &wav, &turns)
    })
    .await
    .map_err(|e| e.to_string())?
}
