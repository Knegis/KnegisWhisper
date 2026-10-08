//! Durable summary state. Generated prose never replaces the original note.
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::{FromRow, SqliteConnection, SqlitePool};
use ts_rs::TS;

pub const SETTINGS_KEY: &str = "reasoning.summaries";

#[derive(Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
pub struct SummarySettings {
    pub enabled: bool,
    pub provider: String,
    pub model: String,
    pub self_hosted_url: Option<String>,
    pub enterprise: bool,
    pub language: String,
    /// Total context budget, including instructions and output; never a secret.
    pub context_budget: u32,
}
impl Default for SummarySettings {
    fn default() -> Self {
        Self {
            enabled: false,
            provider: String::new(),
            model: String::new(),
            self_hosted_url: None,
            enterprise: false,
            language: "match".into(),
            context_budget: 8192,
        }
    }
}
impl SummarySettings {
    pub fn validate(&self) -> Result<(), String> {
        if self.model.trim().is_empty()
            || (self.provider.trim().is_empty() && self.self_hosted_url.is_none())
        {
            return Err("Choose a summary provider and model.".into());
        }
        if !matches!(self.language.as_str(), "match" | "sv" | "en") {
            return Err("Choose Match meeting, Swedish or English.".into());
        }
        if !(4096..=262_144).contains(&self.context_budget) {
            return Err("Context budget must be between 4096 and 262144.".into());
        }
        Ok(())
    }
}

#[derive(Clone, Serialize, Deserialize, TS, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SummaryReference {
    pub id: String,
    pub row_index: usize,
}

#[derive(Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SummaryState {
    pub note_id: i64,
    pub content: Option<String>,
    pub revision: i64,
    pub edited: bool,
    pub outdated: bool,
    pub legacy: bool,
    pub status: Option<String>,
    pub completed: i64,
    pub total: i64,
    pub error: Option<String>,
    pub language: Option<String>,
    pub generated_at: Option<String>,
    /// The model that wrote the summary, as the settings named it when it
    /// succeeded: an id such as `qwen3.5-9b-q4_k_m`, not a display name.
    /// `None` until a generation succeeds. Not where the transcript went:
    /// `provider` alone can't say that (a typed URL wins over it), so the
    /// summary names only the model (KNEG-367).
    pub model: Option<String>,
    pub references: Vec<SummaryReference>,
}

#[derive(Clone, FromRow)]
pub struct Job {
    pub note_id: i64,
    pub generation: i64,
    pub settings_json: String,
    pub source_transcript: String,
    pub source_participants: Option<String>,
    pub expected_revision: i64,
    pub chunks_json: String,
}

pub fn source_hash(transcript: &str, participants: Option<&str>) -> String {
    let mut digest = Sha256::new();
    digest.update(transcript.as_bytes());
    digest.update([0]);
    digest.update(participants.unwrap_or_default().as_bytes());
    format!("{:x}", digest.finalize())
}

/// Called only by native note creation, not the foreign database importer.
pub async fn enqueue_automatic(
    connection: &mut SqliteConnection,
    id: i64,
) -> Result<(), sqlx::Error> {
    sqlx::query("INSERT INTO note_summary_jobs(note_id,status,automatic,settings_json,source_transcript,source_participants,expected_revision)
        SELECT n.id,'queued',1,s.value,n.transcript,n.participants,0 FROM notes n JOIN settings s ON s.key='reasoning.summaries'
        WHERE n.id=? AND n.note_type IN ('meeting','import') AND n.transcript IS NOT NULL AND length(trim(n.transcript))>2
        AND n.enhanced_content IS NULL AND json_valid(s.value) AND json_extract(s.value,'$.enabled')=1")
        .bind(id).execute(connection).await?;
    Ok(())
}

pub async fn state(pool: &SqlitePool, id: i64) -> Result<SummaryState, String> {
    let note = crate::notes::get(pool, id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or("This note no longer exists.")?;
    type Meta = (
        i64,
        bool,
        Option<String>,
        String,
        Option<String>,
        Option<String>,
        Option<String>,
    );
    let meta: Option<Meta> = sqlx::query_as("SELECT revision,edited,source_hash,references_json,language,generated_at,model FROM note_summary_meta WHERE note_id=?").bind(id).fetch_optional(pool).await.map_err(|e|e.to_string())?;
    let legacy = note.enhanced_content.is_some() && meta.as_ref().is_none_or(|m| m.2.is_none());
    let (revision, edited, hash, refs, language, generated_at, model) =
        meta.unwrap_or((0, legacy, None, "[]".into(), None, None, None));
    let job: Option<(String, i64, i64, Option<String>)> = sqlx::query_as(
        "SELECT status,completed,total,error FROM note_summary_jobs WHERE note_id=?",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(|e| e.to_string())?;
    let (status, completed, total, error) =
        job.map_or((None, 0, 0, None), |(s, c, t, e)| (Some(s), c, t, e));
    let outdated = hash.is_some_and(|h| {
        h != source_hash(
            note.transcript.as_deref().unwrap_or_default(),
            note.participants.as_deref(),
        )
    });
    Ok(SummaryState {
        note_id: id,
        content: note.enhanced_content,
        revision,
        edited,
        outdated,
        legacy,
        status,
        completed,
        total,
        error,
        language,
        generated_at,
        model: model.filter(|m| !m.trim().is_empty()),
        references: serde_json::from_str(&refs).unwrap_or_default(),
    })
}

pub async fn enqueue(
    pool: &SqlitePool,
    id: i64,
    settings: &SummarySettings,
    expected_revision: i64,
    replace_edited: bool,
    retry: bool,
) -> Result<(), String> {
    settings.validate()?;
    let mut tx = pool.begin().await.map_err(|e| e.to_string())?;
    // Acquire the writer lock before reading revisions.
    sqlx::query("INSERT OR IGNORE INTO note_summary_meta(note_id,edited) SELECT id,enhanced_content IS NOT NULL FROM notes WHERE id=?").bind(id).execute(&mut *tx).await.map_err(|e|e.to_string())?;
    let (source,participants,revision,edited): (Option<String>,Option<String>,i64,bool)=sqlx::query_as("SELECT n.transcript,n.participants,m.revision,m.edited FROM notes n JOIN note_summary_meta m ON n.id=m.note_id WHERE n.id=?").bind(id).fetch_one(&mut *tx).await.map_err(|_|"This note no longer exists.")?;
    if revision != expected_revision {
        return Err("Summary changed. Reload before regenerating.".into());
    }
    if edited && !replace_edited {
        return Err("Confirm replacing the edited summary first.".into());
    }
    let source = source
        .filter(|s| !s.trim().is_empty() && s.trim() != "[]")
        .ok_or("This note has no transcript to summarize.")?;
    let config = serde_json::to_string(settings).map_err(|e| e.to_string())?;
    let previous:Option<(String,String,Option<String>,String,String)>=sqlx::query_as("SELECT status,source_transcript,source_participants,settings_json,chunks_json FROM note_summary_jobs WHERE note_id=?").bind(id).fetch_optional(&mut *tx).await.map_err(|e|e.to_string())?;
    if previous
        .as_ref()
        .is_some_and(|p| matches!(p.0.as_str(), "queued" | "processing"))
    {
        return Err("A summary is already queued or processing.".into());
    }
    let chunks = previous
        .filter(|p| retry && p.1 == source && p.2 == participants && p.3 == config)
        .map_or_else(|| "{}".into(), |p| p.4);
    sqlx::query("INSERT INTO note_summary_jobs(note_id,status,settings_json,source_transcript,source_participants,expected_revision,chunks_json) VALUES (?,'queued',?,?,?,?,?) ON CONFLICT(note_id) DO UPDATE SET generation=generation+1,status='queued',automatic=0,settings_json=excluded.settings_json,source_transcript=excluded.source_transcript,source_participants=excluded.source_participants,expected_revision=excluded.expected_revision,chunks_json=excluded.chunks_json,error=NULL,completed=0,total=0,created_at=CURRENT_TIMESTAMP")
      .bind(id).bind(config).bind(source).bind(participants).bind(revision).bind(chunks).execute(&mut *tx).await.map_err(|e|e.to_string())?;
    tx.commit().await.map_err(|e| e.to_string())
}

pub async fn edit(pool: &SqlitePool, id: i64, revision: i64, content: &str) -> Result<(), String> {
    let mut tx = pool.begin().await.map_err(|e| e.to_string())?;
    sqlx::query("INSERT OR IGNORE INTO note_summary_meta(note_id) SELECT id FROM notes WHERE id=?")
        .bind(id)
        .execute(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;
    let changed = sqlx::query(
        "UPDATE note_summary_meta SET revision=revision+1,edited=1 WHERE note_id=? AND revision=?",
    )
    .bind(id)
    .bind(revision)
    .execute(&mut *tx)
    .await
    .map_err(|e| e.to_string())?
    .rows_affected();
    if changed != 1 {
        return Err("Summary changed. Reload before saving.".into());
    }
    sqlx::query("UPDATE notes SET enhanced_content=?,updated_at=CURRENT_TIMESTAMP WHERE id=?")
        .bind(content)
        .bind(id)
        .execute(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;
    tx.commit().await.map_err(|e| e.to_string())
}

pub async fn cancel(pool: &SqlitePool, id: i64) -> Result<(), String> {
    sqlx::query("UPDATE note_summary_jobs SET status='cancelled',generation=generation+1,error=NULL WHERE note_id=? AND status IN ('queued','processing')").bind(id).execute(pool).await.map_err(|e|e.to_string())?;
    Ok(())
}
pub async fn cancel_automatic(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE note_summary_jobs SET status='cancelled',generation=generation+1 WHERE automatic=1 AND status='queued'").execute(pool).await?;
    Ok(())
}
/// Readable export: remove generated citation links and heading markers.
pub fn plain_text(markdown: &str) -> String {
    let mut text = markdown.to_owned();
    while let Some(start) = text.find("](#summary-source-") {
        let Some(end) = text[start..].find(')') else {
            break;
        };
        text.replace_range(start..=start + end, "]");
    }
    text.lines()
        .map(|line| line.trim_start_matches('#').trim_start())
        .collect::<Vec<_>>()
        .join("\n")
}
pub async fn recover(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE note_summary_jobs SET status='interrupted',error='Summary generation was interrupted. Retry to continue.' WHERE status='processing'").execute(pool).await?;
    Ok(())
}
pub async fn claim(pool: &SqlitePool) -> Result<Option<Job>, sqlx::Error> {
    sqlx::query_as("UPDATE note_summary_jobs SET status='processing',error=NULL WHERE note_id=(SELECT note_id FROM note_summary_jobs WHERE status='queued' ORDER BY created_at,note_id LIMIT 1) RETURNING note_id,generation,settings_json,source_transcript,source_participants,expected_revision,chunks_json").fetch_optional(pool).await
}
pub async fn active(pool: &SqlitePool, job: &Job) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM note_summary_jobs j JOIN notes n ON n.id=j.note_id LEFT JOIN note_summary_meta m ON m.note_id=n.id WHERE j.note_id=? AND j.generation=? AND j.status='processing' AND n.transcript=j.source_transcript AND n.participants IS j.source_participants AND COALESCE(m.revision,0)=j.expected_revision)").bind(job.note_id).bind(job.generation).fetch_one(pool).await
}
pub async fn progress(
    pool: &SqlitePool,
    job: &Job,
    completed: i64,
    total: i64,
    chunks: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE note_summary_jobs SET completed=?,total=?,chunks_json=? WHERE note_id=? AND generation=? AND status='processing'").bind(completed).bind(total).bind(chunks).bind(job.note_id).bind(job.generation).execute(pool).await?;
    Ok(())
}
pub async fn fail(pool: &SqlitePool, job: &Job, error: &str) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE note_summary_jobs SET status='failed',error=? WHERE note_id=? AND generation=? AND status='processing'").bind(error).bind(job.note_id).bind(job.generation).execute(pool).await?;
    Ok(())
}
pub async fn finish(
    pool: &SqlitePool,
    job: &Job,
    content: &str,
    refs: &[SummaryReference],
    language: &str,
) -> Result<(), String> {
    let settings: SummarySettings =
        serde_json::from_str(&job.settings_json).map_err(|e| e.to_string())?;
    let mut tx = pool.begin().await.map_err(|e| e.to_string())?;
    let changed=sqlx::query("UPDATE note_summary_jobs SET status='succeeded',chunks_json='{}',completed=total WHERE note_id=? AND generation=? AND status='processing' AND EXISTS(SELECT 1 FROM notes n LEFT JOIN note_summary_meta m ON m.note_id=n.id WHERE n.id=note_summary_jobs.note_id AND n.transcript=note_summary_jobs.source_transcript AND n.participants IS note_summary_jobs.source_participants AND COALESCE(m.revision,0)=note_summary_jobs.expected_revision)").bind(job.note_id).bind(job.generation).execute(&mut *tx).await.map_err(|e|e.to_string())?.rows_affected();
    if changed != 1 {
        return Err("The source or summary changed; this result was not saved.".into());
    }
    let refs = serde_json::to_string(refs).map_err(|e| e.to_string())?;
    sqlx::query("INSERT INTO note_summary_meta(note_id,revision,edited,source_hash,source_transcript,source_participants,references_json,provider,model,language,prompt_version,generated_at) VALUES (?,?,0,?,?,?,?,?,?,?,1,CURRENT_TIMESTAMP) ON CONFLICT(note_id) DO UPDATE SET revision=excluded.revision,edited=0,source_hash=excluded.source_hash,source_transcript=excluded.source_transcript,source_participants=excluded.source_participants,references_json=excluded.references_json,provider=excluded.provider,model=excluded.model,language=excluded.language,prompt_version=1,generated_at=CURRENT_TIMESTAMP")
      .bind(job.note_id).bind(job.expected_revision+1).bind(source_hash(&job.source_transcript,job.source_participants.as_deref())).bind(&job.source_transcript).bind(&job.source_participants).bind(refs).bind(&settings.provider).bind(&settings.model).bind(language).execute(&mut *tx).await.map_err(|e|e.to_string())?;
    sqlx::query("UPDATE notes SET enhanced_content=?,updated_at=CURRENT_TIMESTAMP WHERE id=?")
        .bind(content)
        .bind(job.note_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;
    tx.commit().await.map_err(|e| e.to_string())
}

impl std::fmt::Debug for SummarySettings {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SummarySettings").finish_non_exhaustive()
    }
}

impl std::fmt::Debug for SummaryReference {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SummaryReference").finish_non_exhaustive()
    }
}

impl std::fmt::Debug for SummaryState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SummaryState").finish_non_exhaustive()
    }
}

impl std::fmt::Debug for Job {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Job").finish_non_exhaustive()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::notes::{self, NewNote};
    async fn fixture(enabled: bool) -> (kw_storage::Db, SummarySettings, i64) {
        let db = kw_storage::Db::open_in_memory().await.unwrap();
        let settings = SummarySettings {
            enabled,
            provider: "local".into(),
            model: "fixture".into(),
            ..Default::default()
        };
        kw_storage::settings::set(db.pool(), SETTINGS_KEY, &settings)
            .await
            .unwrap();
        let id = notes::create(
            db.pool(),
            &NewNote {
                title: "Fixture".into(),
                content: "Original notes".into(),
                note_type: Some("meeting".into()),
                transcript: Some(
                    r#"[{"text":"Åsa will send the report Friday.","timestamp":0}]"#.into(),
                ),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        (db, settings, id)
    }
    #[tokio::test]
    async fn automatic_jobs_follow_enablement_and_ignore_personal_notes() {
        let (db, _, id) = fixture(true).await;
        let job = claim(db.pool()).await.unwrap().unwrap();
        assert_eq!(job.note_id, id);
        let personal = notes::create(
            db.pool(),
            &NewNote {
                transcript: Some("[]".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert!(state(db.pool(), personal).await.unwrap().status.is_none());
        let (disabled, _, _) = fixture(false).await;
        assert!(claim(disabled.pool()).await.unwrap().is_none());
    }
    #[tokio::test]
    async fn imported_recording_and_job_roll_back_together() {
        let (db, _, _) = fixture(true).await;
        let before: i64 = sqlx::query_scalar("SELECT count(*) FROM notes")
            .fetch_one(db.pool())
            .await
            .unwrap();
        let result = notes::create_with_recording_job(
            db.pool(),
            &NewNote {
                note_type: Some("import".into()),
                transcript: Some("[]".into()),
                ..Default::default()
            },
            "fixture.wav",
            1.0,
            "fixture.wav",
            Some("nonexistent-job"),
        )
        .await;
        assert!(result.is_err());
        let after: i64 = sqlx::query_scalar("SELECT count(*) FROM notes")
            .fetch_one(db.pool())
            .await
            .unwrap();
        assert_eq!(before, after);
    }
    #[tokio::test]
    async fn restart_preserves_checkpoints_and_retry_deduplicates() {
        let (db, settings, id) = fixture(true).await;
        let job = claim(db.pool()).await.unwrap().unwrap();
        progress(db.pool(), &job, 1, 3, r#"{"extract-0":{}}"#)
            .await
            .unwrap();
        recover(db.pool()).await.unwrap();
        assert_eq!(
            state(db.pool(), id).await.unwrap().status.as_deref(),
            Some("interrupted")
        );
        enqueue(db.pool(), id, &settings, 0, false, true)
            .await
            .unwrap();
        assert!(enqueue(db.pool(), id, &settings, 0, false, true)
            .await
            .is_err());
        let retry = claim(db.pool()).await.unwrap().unwrap();
        assert_eq!(retry.chunks_json, r#"{"extract-0":{}}"#);
        assert!(retry.generation > job.generation);
    }
    /// The model that wrote it, as stored at success, so the summary can name
    /// it (KNEG-367). A note whose summary row exists without one -- created
    /// by an edit before any generation -- names none.
    #[tokio::test]
    async fn a_finished_summary_reports_its_model() {
        let (db, _, id) = fixture(true).await;
        assert_eq!(state(db.pool(), id).await.unwrap().model, None);
        let job = claim(db.pool()).await.unwrap().unwrap();
        finish(db.pool(), &job, "Generated", &[], "en")
            .await
            .unwrap();
        assert_eq!(
            state(db.pool(), id).await.unwrap().model.as_deref(),
            Some("fixture")
        );

        let (db, _, id) = fixture(false).await;
        edit(db.pool(), id, 0, "Written by hand").await.unwrap();
        assert_eq!(state(db.pool(), id).await.unwrap().model, None);
    }

    #[tokio::test]
    async fn failed_regeneration_keeps_edits_and_original_notes() {
        let (db, settings, id) = fixture(true).await;
        let job = claim(db.pool()).await.unwrap().unwrap();
        finish(db.pool(), &job, "Generated", &[], "en")
            .await
            .unwrap();
        edit(db.pool(), id, 1, "My edit").await.unwrap();
        assert!(enqueue(db.pool(), id, &settings, 2, false, false)
            .await
            .is_err());
        enqueue(db.pool(), id, &settings, 2, true, false)
            .await
            .unwrap();
        let retry = claim(db.pool()).await.unwrap().unwrap();
        fail(db.pool(), &retry, "Provider unavailable")
            .await
            .unwrap();
        let note = notes::get(db.pool(), id).await.unwrap().unwrap();
        assert_eq!(note.content, "Original notes");
        assert_eq!(note.enhanced_content.as_deref(), Some("My edit"));
    }
    #[tokio::test]
    async fn edits_cancellation_and_source_changes_reject_late_results() {
        let (db, settings, id) = fixture(true).await;
        let job = claim(db.pool()).await.unwrap().unwrap();
        edit(db.pool(), id, 0, "Human edit").await.unwrap();
        assert!(!active(db.pool(), &job).await.unwrap());
        assert!(finish(db.pool(), &job, "Late result", &[], "en")
            .await
            .is_err());
        cancel(db.pool(), id).await.unwrap();
        enqueue(db.pool(), id, &settings, 1, true, false)
            .await
            .unwrap();
        let next = claim(db.pool()).await.unwrap().unwrap();
        cancel(db.pool(), id).await.unwrap();
        assert!(finish(db.pool(), &next, "Late", &[], "en").await.is_err());
        enqueue(db.pool(), id, &settings, 1, true, false)
            .await
            .unwrap();
        let changed = claim(db.pool()).await.unwrap().unwrap();
        sqlx::query("UPDATE notes SET transcript='[]' WHERE id=?")
            .bind(id)
            .execute(db.pool())
            .await
            .unwrap();
        assert!(!active(db.pool(), &changed).await.unwrap());
        assert!(finish(db.pool(), &changed, "Late", &[], "en")
            .await
            .is_err());
    }
    /// KNEG-362. A note's people are part of what its summary is made from,
    /// and since they became editable they can change after it: a job queued
    /// or running when they change is stopped rather than finished from the
    /// old people, and a finished summary is marked out of date.
    #[tokio::test]
    async fn changing_a_note_s_people_stops_its_summary_and_outdates_a_finished_one() {
        let (db, settings, id) = fixture(true).await;
        let anna = [crate::people::Participant {
            display_name: Some("Anna".into()),
            email: Some("anna@example.com".into()),
        }];
        crate::people::set_participants(db.pool(), id, &anna)
            .await
            .unwrap();
        let queued_before = claim(db.pool()).await.unwrap().unwrap();
        assert!(!active(db.pool(), &queued_before).await.unwrap());
        assert!(finish(db.pool(), &queued_before, "Late", &[], "en")
            .await
            .is_err());
        fail(db.pool(), &queued_before, "stopped").await.unwrap();

        enqueue(db.pool(), id, &settings, 0, false, true)
            .await
            .unwrap();
        let job = claim(db.pool()).await.unwrap().unwrap();
        assert!(active(db.pool(), &job).await.unwrap());
        finish(db.pool(), &job, "Generated", &[], "en")
            .await
            .unwrap();
        assert!(!state(db.pool(), id).await.unwrap().outdated);

        crate::people::set_participants(db.pool(), id, &[])
            .await
            .unwrap();
        assert!(state(db.pool(), id).await.unwrap().outdated);
    }

    #[tokio::test]
    async fn summaries_are_searchable_and_deleted_with_notes() {
        let (db, _, id) = fixture(true).await;
        let job = claim(db.pool()).await.unwrap().unwrap();
        finish(db.pool(), &job, "UniqueSummaryWord", &[], "en")
            .await
            .unwrap();
        assert_eq!(
            notes::search(db.pool(), "UniqueSummaryWord", 10)
                .await
                .unwrap()
                .len(),
            1
        );
        sqlx::query("DELETE FROM notes WHERE id=?")
            .bind(id)
            .execute(db.pool())
            .await
            .unwrap();
        let count:i64=sqlx::query_scalar("SELECT (SELECT count(*) FROM note_summary_jobs)+(SELECT count(*) FROM note_summary_meta)").fetch_one(db.pool()).await.unwrap();
        assert_eq!(count, 0);
    }
    #[tokio::test]
    async fn disabling_cancels_only_queued_automatic_jobs() {
        let (db, settings, id) = fixture(true).await;
        cancel_automatic(db.pool()).await.unwrap();
        assert_eq!(
            state(db.pool(), id).await.unwrap().status.as_deref(),
            Some("cancelled")
        );
        enqueue(db.pool(), id, &settings, 0, false, false)
            .await
            .unwrap();
        cancel_automatic(db.pool()).await.unwrap();
        assert_eq!(
            state(db.pool(), id).await.unwrap().status.as_deref(),
            Some("queued")
        );
    }
    #[test]
    fn debug_output_never_contains_prose_or_endpoints() {
        let settings = SummarySettings {
            self_hosted_url: Some("https://private.example".into()),
            ..Default::default()
        };
        assert!(!format!("{settings:?}").contains("private"));
    }
}
