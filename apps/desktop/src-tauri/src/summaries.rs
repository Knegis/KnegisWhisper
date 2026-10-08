//! Desktop-owned summary worker. UI lifetimes never own processing jobs.
use crate::{local_llm::LocalLlm, state::AppState};
use kw_notes_core::summary::{self, Job, SummaryReference, SummarySettings, SummaryState};
use kw_reasoning_core::summary::{Summary, PROMPT};
use kw_reasoning_core::{Chat, ChatRoute, ChatSettings, RequestOptions};
use serde_json::json;
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
    time::Duration,
};
use tauri::{AppHandle, Emitter, State};

pub const SUMMARY_SETTINGS_KEY: &str = "reasoning.summaries";
pub const SUMMARY_EVENT: &str = "note-summary-changed";

/// Keep the stored segment indexes as source ids even when blank rows are
/// omitted, so a summary link still points to the original passage.
fn prompt_rows(
    segments: &[kw_notes_core::transcript::Segment],
    mappings: &BTreeMap<String, String>,
) -> Vec<(String, String)> {
    segments
        .iter()
        .enumerate()
        .filter(|(_, segment)| !segment.text.trim().is_empty())
        .map(|(i, segment)| {
            let identified = segment
                .speaker_name
                .as_deref()
                .is_some_and(|name| !segment.speaker_is_placeholder && !name.trim().is_empty())
                || segment
                    .speaker
                    .as_deref()
                    .is_some_and(|id| !id.trim().is_empty())
                || matches!(segment.source.as_deref(), Some("mic" | "system"));
            let speaker = if identified {
                kw_notes_core::transcript::speaker_name(
                    segment,
                    mappings,
                    &kw_notes_core::transcript::SpeakerLabels::default(),
                )
            } else {
                "Unidentified".to_owned()
            };
            (format!("s{i}"), format!("{speaker}: {}", segment.text))
        })
        .collect()
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod speaker_tests {
    use super::*;
    use kw_notes_core::transcript::Segment;

    #[test]
    fn prompt_rows_use_note_names_and_number_unmapped_diarised_speakers() {
        let segments = vec![
            Segment {
                text: "named".into(),
                speaker: Some("speaker_1".into()),
                speaker_name: Some("Speaker 2".into()),
                speaker_is_placeholder: true,
                ..Default::default()
            },
            Segment {
                text: "unnamed".into(),
                speaker: Some("speaker_0".into()),
                speaker_name: Some("Speaker 1".into()),
                speaker_is_placeholder: true,
                ..Default::default()
            },
        ];
        let mappings = BTreeMap::from([("speaker_1".into(), "Axel".into())]);
        assert_eq!(
            prompt_rows(&segments, &mappings),
            [
                ("s0".into(), "Axel: named".into()),
                ("s1".into(), "Speaker 1: unnamed".into()),
            ]
        );
    }
}

/// The participants the prompt names as spelling help: their display names,
/// comma-separated, and never an email address (KNEG-362's ruling; this was
/// the first 256 characters of the stored JSON, addresses and all). Bounded at
/// 256 characters, as before, so supplementary context never truncates
/// transcript evidence.
fn participant_hint(participants: Option<&str>) -> String {
    kw_notes_core::people::display_names(participants)
        .join(", ")
        .chars()
        .take(256)
        .collect()
}

#[cfg(test)]
mod hint_tests {
    use super::*;

    /// KNEG-362's ruling: the summary model is told who was there by name,
    /// and is never sent an address.
    #[test]
    fn the_participant_hint_names_people_and_sends_no_address() {
        let hint = participant_hint(Some(
            r#"[{"displayName":"Anna Lindqvist","email":"anna@example.com"},{"email":"jonas@nordbank.example"},{"displayName":"Ravi Menon","email":"ravi@example.com"}]"#,
        ));
        assert_eq!(hint, "Anna Lindqvist, Ravi Menon");
        assert!(!hint.contains('@'), "{hint}");
        assert_eq!(participant_hint(None), "");
        assert_eq!(participant_hint(Some("not json")), "");
    }

    #[test]
    fn the_participant_hint_is_capped_as_before() {
        let crowd: Vec<serde_json::Value> = (0..100)
            .map(|i| json!({ "displayName": format!("Person number {i}") }))
            .collect();
        let stored = serde_json::to_string(&crowd).unwrap_or_default();
        let hint = participant_hint(Some(&stored));
        assert_eq!(hint.chars().count(), 256);
        assert!(
            hint.starts_with("Person number 0, Person number 1, "),
            "{hint}"
        );
    }
}

fn route(settings: &SummarySettings) -> Result<ChatRoute, String> {
    settings.validate()?;
    kw_reasoning_core::resolve_chat(
        kw_provider_registry::registry().map_err(|e| e.to_string())?,
        &ChatSettings {
            provider: Some(settings.provider.clone()),
            self_hosted_url: settings.self_hosted_url.clone(),
            enterprise: settings.enterprise,
        },
    )
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn summary_settings_get(state: State<'_, AppState>) -> Result<SummarySettings, String> {
    kw_storage::settings::get_or(
        state.db().pool(),
        SUMMARY_SETTINGS_KEY,
        SummarySettings::default(),
    )
    .await
    .map_err(|e| e.to_string())
}
#[tauri::command]
pub async fn summary_settings_set(
    state: State<'_, AppState>,
    settings: SummarySettings,
) -> Result<(), String> {
    if settings.enabled || !settings.model.is_empty() {
        let _ = route(&settings)?;
    }
    kw_storage::settings::set(state.db().pool(), SUMMARY_SETTINGS_KEY, &settings)
        .await
        .map_err(|e| e.to_string())?;
    if !settings.enabled {
        summary::cancel_automatic(state.db().pool())
            .await
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}
#[tauri::command]
pub async fn note_summary_get(state: State<'_, AppState>, id: i64) -> Result<SummaryState, String> {
    summary::state(state.db().pool(), id).await
}
#[tauri::command]
pub async fn note_summary_generate(
    app: AppHandle,
    state: State<'_, AppState>,
    id: i64,
    revision: i64,
    replace_edited: bool,
    retry: bool,
) -> Result<SummaryState, String> {
    let settings: SummarySettings = kw_storage::settings::get_or(
        state.db().pool(),
        SUMMARY_SETTINGS_KEY,
        SummarySettings::default(),
    )
    .await
    .map_err(|e| e.to_string())?;
    let _ = route(&settings)?;
    summary::enqueue(
        state.db().pool(),
        id,
        &settings,
        revision,
        replace_edited,
        retry,
    )
    .await?;
    let _ = app.emit(SUMMARY_EVENT, id);
    summary::state(state.db().pool(), id).await
}
#[tauri::command]
pub async fn note_summary_cancel(
    app: AppHandle,
    state: State<'_, AppState>,
    id: i64,
) -> Result<(), String> {
    summary::cancel(state.db().pool(), id).await?;
    let _ = app.emit(SUMMARY_EVENT, id);
    Ok(())
}
#[tauri::command]
pub async fn note_summary_update(
    app: AppHandle,
    state: State<'_, AppState>,
    id: i64,
    revision: i64,
    content: String,
) -> Result<SummaryState, String> {
    summary::edit(state.db().pool(), id, revision, &content).await?;
    let _ = app.emit(SUMMARY_EVENT, id);
    summary::state(state.db().pool(), id).await
}
#[tauri::command]
pub async fn note_summary_export(
    app: AppHandle,
    state: State<'_, AppState>,
    id: i64,
    format: String,
) -> Result<Option<String>, String> {
    use tauri_plugin_dialog::DialogExt;
    if !matches!(format.as_str(), "md" | "txt") {
        return Err("Choose Markdown or plain text.".into());
    }
    let summary = summary::state(state.db().pool(), id).await?;
    let content = summary.content.ok_or("This note has no summary.")?;
    let content = if format == "txt" {
        kw_notes_core::summary::plain_text(&content)
    } else {
        content
    };
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog()
        .file()
        .set_title("Export summary")
        .set_file_name(format!("summary-{id}.{format}"))
        .add_filter("Summary", &[format.as_str()])
        .save_file(move |chosen| {
            let _ = tx.send(chosen);
        });
    let Some(file) = rx
        .await
        .map_err(|_| "The save dialog closed unexpectedly.")?
    else {
        return Ok(None);
    };
    let path = file
        .into_path()
        .map_err(|_| "Could not resolve the selected path.")?;
    tokio::fs::write(&path, content)
        .await
        .map_err(|_| "Could not write the summary export.")?;
    Ok(Some(path.display().to_string()))
}

pub fn start(app: AppHandle, state: AppState, llm: Arc<LocalLlm>) {
    tauri::async_runtime::spawn(async move {
        if summary::recover(state.db().pool()).await.is_err() {
            tracing::error!("could not recover summary jobs");
            return;
        }
        loop {
            match summary::claim(state.db().pool()).await {
                Ok(Some(job)) => {
                    let _ = app.emit(SUMMARY_EVENT, job.note_id);
                    let run = generate(&app, &state, &llm, &job);
                    let invalidated = async {
                        loop {
                            tokio::time::sleep(Duration::from_millis(300)).await;
                            if !summary::active(state.db().pool(), &job)
                                .await
                                .unwrap_or(false)
                            {
                                break;
                            }
                        }
                    };
                    let result = tokio::select! { result=run=>result, ()=invalidated=>Err("Generation stopped because the note, summary or job changed.".into()) };
                    if let Err(error) = result {
                        let _ = summary::fail(state.db().pool(), &job, &error).await;
                    }
                    let _ = app.emit(SUMMARY_EVENT, job.note_id);
                }
                Ok(None) => tokio::time::sleep(Duration::from_millis(500)).await,
                Err(_) => {
                    tracing::warn!("could not read summary queue");
                    tokio::time::sleep(Duration::from_secs(3)).await;
                }
            }
        }
    });
}

async fn generate(
    app: &AppHandle,
    state: &AppState,
    llm: &Arc<LocalLlm>,
    job: &Job,
) -> Result<(), String> {
    let settings: SummarySettings =
        serde_json::from_str(&job.settings_json).map_err(|_| "Summary settings are invalid.")?;
    let destination = route(&settings)?;
    let segments: Vec<kw_notes_core::transcript::Segment> =
        serde_json::from_str(&job.source_transcript)
            .map_err(|_| "The saved transcript format is invalid.")?;
    let mappings = kw_notes_core::speakers::mappings(state.db().pool(), job.note_id)
        .await
        .map_err(|_| "Could not read the note's speaker names.")?;
    let rows = prompt_rows(&segments, &mappings);
    if rows.is_empty() {
        return Err("The saved transcript has no speech to summarize.".into());
    }
    let registry = kw_provider_registry::registry().map_err(|_| "Model registry unavailable.")?;
    let context = if matches!(destination, ChatRoute::Local) {
        let model = registry
            .local_llm
            .iter()
            .flat_map(|p| &p.models)
            .find(|m| m.id == settings.model)
            .ok_or("Choose an installed local model.")?;
        settings
            .context_budget
            .min(model.context_length.unwrap_or(8192))
            .min(8192)
    } else {
        settings.context_budget
    };
    let names = participant_hint(job.source_participants.as_deref());
    let budget = (context as usize).saturating_sub(PROMPT.len() + names.len() + 2048 + 512);
    if budget < 1200 {
        return Err(
            "This model's context is too small for structured summaries. Choose a larger model."
                .into(),
        );
    }
    let inputs = kw_reasoning_core::summary::chunks(&rows, budget);
    let allowed: BTreeSet<String> = rows.iter().map(|(id, _)| id.clone()).collect();
    let refs: Vec<SummaryReference> = segments
        .iter()
        .enumerate()
        .map(|(i, _)| SummaryReference {
            id: format!("s{i}"),
            row_index: i,
        })
        .collect();
    let total = i64::try_from(inputs.len().saturating_mul(2).saturating_sub(1))
        .map_err(|_| "Too many summary chunks.")?;
    summary::progress(state.db().pool(), job, 0, total, &job.chunks_json)
        .await
        .map_err(|_| "Could not save summary progress.")?;
    let mut cache: BTreeMap<String, Summary> = serde_json::from_str(&job.chunks_json)
        .map_err(|_| "Saved summary progress is invalid. Regenerate to start over.")?;
    let mut layer = Vec::new();
    let mut weights = Vec::new();
    for (index, input) in inputs.iter().enumerate() {
        let key = format!("extract-{index}");
        let scope = allowed
            .iter()
            .filter(|id| input.contains(&format!("[{id}]")))
            .cloned()
            .collect();
        let result = step(
            app,
            state,
            llm,
            job,
            &settings,
            &destination,
            &key,
            input,
            &names,
            &settings.language,
            &scope,
            budget,
            total,
            &mut cache,
        )
        .await?;
        weights.push(input.len());
        layer.push(result);
    }
    let language = if settings.language == "match" {
        kw_reasoning_core::summary::dominant_language(layer.iter().zip(weights))
    } else {
        settings.language.clone()
    };
    let mut level = 0;
    while layer.len() > 1 {
        let mut next = Vec::new();
        for (index, pair) in layer.chunks(2).enumerate() {
            if pair.len() == 1 {
                next.extend_from_slice(pair);
                continue;
            }
            let input =
                serde_json::to_string(pair).map_err(|_| "Could not encode summary extracts.")?;
            let scope = pair
                .iter()
                .flat_map(|s| s.items().flat_map(|i| i.sources.iter().cloned()))
                .collect();
            next.push(
                step(
                    app,
                    state,
                    llm,
                    job,
                    &settings,
                    &destination,
                    &format!("combine-{level}-{index}"),
                    &input,
                    &names,
                    &language,
                    &scope,
                    budget,
                    total,
                    &mut cache,
                )
                .await?,
            );
        }
        layer = next;
        level += 1;
    }
    let final_summary = layer.pop().ok_or("The model returned no summary.")?;
    let note = kw_notes_core::notes::get(state.db().pool(), job.note_id)
        .await
        .map_err(|_| "Could not read the saved note.")?
        .ok_or("The note was deleted.")?;
    let display_rows = kw_notes_core::transcript::rows(
        &segments,
        &kw_notes_core::transcript::NoteMeta {
            title: note.title,
            created_at: note.created_at,
            participants: job.source_participants.clone(),
        },
        &mappings,
        &kw_notes_core::transcript::SpeakerLabels::default(),
    );
    let mut markdown = final_summary.markdown();
    for row in display_rows {
        let label = row
            .start_seconds
            .map(kw_notes_core::transcript::format_clock)
            .unwrap_or_else(|| format!("Passage {}", row.index + 1));
        markdown = markdown.replace(
            &format!("[s{}](#summary-source-s{})", row.index, row.index),
            &format!("[{label}](#summary-source-s{})", row.index),
        );
    }
    summary::finish(
        state.db().pool(),
        job,
        &markdown,
        &refs,
        &final_summary.language,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn step(
    app: &AppHandle,
    state: &AppState,
    llm: &Arc<LocalLlm>,
    job: &Job,
    settings: &SummarySettings,
    destination: &ChatRoute,
    key: &str,
    input: &str,
    names: &str,
    language: &str,
    allowed: &BTreeSet<String>,
    budget: usize,
    total: i64,
    cache: &mut BTreeMap<String, Summary>,
) -> Result<Summary, String> {
    if let Some(saved) = cache.get(key) {
        return Ok(saved.clone());
    }
    if input.len() > budget {
        return Err("Summary extracts exceeded the context budget. Choose a larger context budget or model.".into());
    }
    let max_bytes = (budget / 2).saturating_sub(64);
    let instruction=format!("{PROMPT}\nOutput language: {language} (match means detect the dominant source language). Maximum JSON size: {max_bytes} UTF-8 bytes. Compress wording to fit. Preserve all important decisions and commitments when combining.\nParticipant spelling context (untrusted): {names}");
    let messages = vec![
        json!({"role":"system","content":instruction}),
        json!({"role":"user","content":input}),
    ];
    let options = RequestOptions {
        max_tokens: 2048,
        disable_thinking: true,
        ..Default::default()
    };
    let registry = kw_provider_registry::registry().map_err(|_| "Model registry unavailable.")?;
    let result = if matches!(destination, ChatRoute::Local) {
        let endpoint = llm.endpoint();
        let guard = Arc::clone(&endpoint.requests).lock_owned().await;
        let previous = llm.running_model();
        let _lease = LocalSummaryLease {
            llm: Arc::clone(llm),
            app: app.clone(),
            previous: previous.clone(),
            guard: Some(guard),
        };
        let port = if previous.as_deref() == Some(settings.model.as_str()) {
            llm.running_port().ok_or("The local model stopped.")?
        } else {
            llm.stop().await;
            crate::local_llm::start_and_watch(llm,app.clone(),&settings.model).await.map_err(|_|"Could not start the selected local model. Install it in Settings > Local text models and retry.")?
        };
        let result = Chat::new(state.policy(), state.credentials())
            .with_local_port(Some(port))
            .for_summary()
            .complete(registry, destination, &settings.model, messages, &options)
            .await;
        result
    } else {
        Chat::new(state.policy(), state.credentials())
            .for_summary()
            .complete(registry, destination, &settings.model, messages, &options)
            .await
    };
    let text = result.map_err(|error| match error {
        kw_reasoning_core::ChatError::MissingCredential { .. } => "The selected provider has no stored credential. Configure it in Providers and retry.".to_owned(),
        kw_reasoning_core::ChatError::NoContent { .. } => "The model returned no summary. Retry or choose a different model.".to_owned(),
        kw_reasoning_core::ChatError::ProviderRejected { status, .. } => format!("The summary provider rejected the request (HTTP {status}). Check the selected model and provider settings."),
        kw_reasoning_core::ChatError::Policy(_) => "Network policy refused the configured summary endpoint. Check the provider settings.".to_owned(),
        kw_reasoning_core::ChatError::ShapeNotImplemented { .. } => "The summary provider configuration is invalid. Check Providers settings.".to_owned(),
        kw_reasoning_core::ChatError::Transport { provider, .. } if provider == "summary" => "The model refused or truncated the summary. Choose another model or retry.".to_owned(),
        kw_reasoning_core::ChatError::Transport { .. } => "The summary request could not connect or timed out. Check the provider and retry.".to_owned(),
    })?;
    let result = Summary::parse(&text, allowed)?;
    let encoded = serde_json::to_string(&result).map_err(|_| "Could not save summary progress.")?;
    if encoded.len() > max_bytes {
        return Err("The model did not follow the summary size limit. Choose a larger context budget or another model.".into());
    }
    cache.insert(key.into(), result.clone());
    let checkpoint =
        serde_json::to_string(cache).map_err(|_| "Could not save summary progress.")?;
    let completed = i64::try_from(cache.len()).map_err(|_| "Too many summary chunks.")?;
    summary::progress(state.db().pool(), job, completed, total, &checkpoint)
        .await
        .map_err(|_| "Could not save summary progress.")?;
    let _ = app.emit(SUMMARY_EVENT, job.note_id);
    tokio::task::yield_now().await;
    Ok(result)
}

// Restoration owns the lease even when cancellation drops the request future.
// A waiting dictation cannot acquire a server still loaded with the summary model.
struct LocalSummaryLease {
    llm: Arc<LocalLlm>,
    app: AppHandle,
    previous: Option<String>,
    guard: Option<tokio::sync::OwnedMutexGuard<()>>,
}
impl Drop for LocalSummaryLease {
    fn drop(&mut self) {
        let llm = Arc::clone(&self.llm);
        let app = self.app.clone();
        let previous = self.previous.clone();
        let guard = self.guard.take();
        tauri::async_runtime::spawn(async move {
            let _guard = guard;
            if let Some(previous) = previous {
                if llm.running_model().as_deref() != Some(previous.as_str()) {
                    llm.stop().await;
                    if crate::local_llm::start_and_watch(&llm, app, &previous)
                        .await
                        .is_err()
                    {
                        tracing::warn!("could not restore previous local model after summary");
                    }
                }
            } else {
                llm.stop().await;
            }
        });
    }
}
