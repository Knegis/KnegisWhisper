//! Explicitly enrolled local voices. Automatic matches never update enrollment.
use kw_storage::StorageError;
use serde::{Deserialize, Serialize};
use sqlx::{Row, SqliteConnection, SqlitePool};

pub const ENABLED_KEY: &str = "meetings.knownVoices";
pub const FORMAT: &str = "f32-le-normalized-512-v1";
pub const DIM: usize = 512;
pub const MODEL_ID: &str = "sherpa-1.13.4/campplus-357a834f702b80161e5b981182c038e18553c1f2ca752ed6cec2052365d4129b/turn-centroid-v1";

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Policy {
    pub threshold: f32,
    pub margin: f32,
    pub min_seconds: f64,
}
// Replace only with a frozen policy backed by the held-out evaluation report.
pub const RELEASE_POLICY: Option<Policy> = None;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Sample {
    pub speaker_id: String,
    pub embedding: Vec<f32>,
    pub seconds: f64,
}

#[derive(Debug, Clone, Serialize, ts_rs::TS, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct KnownVoice {
    pub id: i64,
    pub display_name: String,
    pub compatible: bool,
}

fn invalid(message: &str) -> StorageError {
    StorageError::Import(message.into())
}
pub fn normalize(values: &[f32]) -> Option<Vec<f32>> {
    if values.len() != DIM || values.iter().any(|v| !v.is_finite()) {
        return None;
    }
    let norm = values
        .iter()
        .map(|v| f64::from(*v).powi(2))
        .sum::<f64>()
        .sqrt();
    if !norm.is_finite() || norm <= 1e-12 {
        return None;
    }
    Some(
        values
            .iter()
            .map(|v| (f64::from(*v) / norm) as f32)
            .collect(),
    )
}
fn encode(values: &[f32]) -> Option<Vec<u8>> {
    Some(
        normalize(values)?
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect(),
    )
}
fn decode(bytes: &[u8]) -> Option<Vec<f32>> {
    if bytes.len() != DIM * 4 {
        return None;
    }
    normalize(
        &bytes
            .chunks_exact(4)
            .map(|c| <[u8; 4]>::try_from(c).ok().map(f32::from_le_bytes))
            .collect::<Option<Vec<_>>>()?,
    )
}
pub fn choose(sample: &[f32], profiles: &[(i64, Vec<f32>)], policy: Policy) -> Option<i64> {
    if !policy.threshold.is_finite()
        || !(0.0..=1.0).contains(&policy.threshold)
        || !policy.margin.is_finite()
        || !(0.0..=2.0).contains(&policy.margin)
        || policy.margin <= 0.0
    {
        return None;
    }
    let sample = normalize(sample)?;
    let mut scores: Vec<_> = profiles
        .iter()
        .filter_map(|(id, v)| {
            let v = normalize(v)?;
            Some((
                *id,
                sample
                    .iter()
                    .zip(v)
                    .map(|(a, b)| a * b)
                    .sum::<f32>()
                    .clamp(-1.0, 1.0),
            ))
        })
        .collect();
    scores.sort_by(|a, b| b.1.total_cmp(&a.1));
    let best = scores.first()?;
    let second = scores.get(1).map(|s| s.1).unwrap_or(-1.0);
    (best.1 >= policy.threshold && best.1 - second >= policy.margin).then_some(best.0)
}

pub async fn enabled(pool: &SqlitePool) -> bool {
    kw_storage::settings::get_or(pool, ENABLED_KEY, false)
        .await
        .unwrap_or(false)
}
pub async fn list(pool: &SqlitePool) -> Result<Vec<KnownVoice>, StorageError> {
    Ok(sqlx::query_as("SELECT id, display_name, COALESCE(model_id = ? AND embedding_format = ?, 0) AS compatible FROM speaker_profiles ORDER BY display_name, id")
        .bind(MODEL_ID).bind(FORMAT).fetch_all(pool).await?)
}

/// Called inside the note-save transaction, after the note exists. Match against
/// current profiles so a concurrent deletion cannot restore a forgotten voice.
pub async fn persist(
    conn: &mut SqliteConnection,
    note_id: i64,
    samples: &[Sample],
    policy: Policy,
) -> Result<(), StorageError> {
    let opted: Option<String> = sqlx::query_scalar("SELECT value FROM settings WHERE key = ?")
        .bind(ENABLED_KEY)
        .fetch_optional(&mut *conn)
        .await?;
    if opted.as_deref() != Some("true") {
        return Ok(());
    }
    if !policy.min_seconds.is_finite() || policy.min_seconds <= 0.0 {
        return Err(invalid("Invalid voice policy"));
    }
    let rows = sqlx::query("SELECT id, display_name, embedding FROM speaker_profiles WHERE model_id=? AND embedding_format=?")
        .bind(MODEL_ID).bind(FORMAT).fetch_all(&mut *conn).await?;
    let profiles: Vec<_> = rows
        .iter()
        .filter_map(|r| decode(r.get("embedding")).map(|v| (r.get::<i64, _>("id"), v)))
        .collect();
    for sample in samples {
        if !sample.seconds.is_finite() || sample.seconds < policy.min_seconds {
            continue;
        }
        let Some(bytes) = encode(&sample.embedding) else {
            continue;
        };
        sqlx::query("INSERT INTO note_speaker_embeddings(note_id,speaker_id,embedding,model_id,embedding_format,speech_seconds) VALUES(?,?,?,?,?,?) ON CONFLICT(note_id,speaker_id) DO NOTHING")
            .bind(note_id).bind(&sample.speaker_id).bind(bytes).bind(MODEL_ID).bind(FORMAT).bind(sample.seconds).execute(&mut *conn).await?;
        if let Some(id) = choose(&sample.embedding, &profiles, policy) {
            let name: String = rows
                .iter()
                .find(|r| r.get::<i64, _>("id") == id)
                .ok_or_else(|| invalid("Missing voice"))?
                .get("display_name");
            sqlx::query("INSERT INTO speaker_mappings(note_id,speaker_id,profile_id,display_name,origin) SELECT ?,?,?,?,'automatic' WHERE NOT EXISTS(SELECT 1 FROM speaker_manual_overrides WHERE note_id=? AND speaker_id=?) ON CONFLICT(note_id,speaker_id) DO NOTHING")
                .bind(note_id).bind(&sample.speaker_id).bind(id).bind(name).bind(note_id).bind(&sample.speaker_id).execute(&mut *conn).await?;
        }
    }
    Ok(())
}

pub async fn enroll(
    pool: &SqlitePool,
    note_id: i64,
    speaker: &str,
    name: &str,
    replace_id: Option<i64>,
    policy: Policy,
) -> Result<i64, StorageError> {
    let name = name.split_whitespace().collect::<Vec<_>>().join(" ");
    if name.is_empty() {
        return Err(invalid("Name this speaker before remembering their voice"));
    }
    let mut tx = pool.begin().await?;
    // Serialize enrollment with settings/profile writes before taking a snapshot.
    sqlx::query("UPDATE settings SET value=value WHERE key=?")
        .bind(ENABLED_KEY)
        .execute(&mut *tx)
        .await?;
    let opted: Option<String> = sqlx::query_scalar("SELECT value FROM settings WHERE key=?")
        .bind(ENABLED_KEY)
        .fetch_optional(&mut *tx)
        .await?;
    if opted.as_deref() != Some("true") {
        return Err(invalid("Enable Known voices first"));
    }
    let row = sqlx::query("SELECT e.embedding,e.speech_seconds,e.enrolled_profile_id FROM note_speaker_embeddings e JOIN notes n ON n.id=e.note_id WHERE e.note_id=? AND e.speaker_id=? AND n.trashed_at IS NULL AND e.model_id=? AND e.embedding_format=?")
        .bind(note_id).bind(speaker).bind(MODEL_ID).bind(FORMAT).fetch_optional(&mut *tx).await?.ok_or_else(|| invalid("No usable voice sample. Record a meeting with Known voices enabled first."))?;
    let bytes: Vec<u8> = row.get("embedding");
    let seconds: f64 = row.get("speech_seconds");
    if decode(&bytes).is_none() || !seconds.is_finite() || seconds < policy.min_seconds {
        return Err(invalid("Not enough usable speech to remember this voice"));
    }
    let existing: Option<i64> = row.get("enrolled_profile_id");
    if replace_id.is_none() {
        if let Some(id) = existing {
            let previous: String =
                sqlx::query_scalar("SELECT display_name FROM speaker_profiles WHERE id=?")
                    .bind(id)
                    .fetch_one(&mut *tx)
                    .await?;
            if previous != name {
                return Err(invalid("This sample already belongs to a remembered voice. Select that profile explicitly to replace it."));
            }
        }
    }
    let id = if let Some(id) = replace_id.or(existing) {
        let changed = sqlx::query("UPDATE speaker_profiles SET display_name=?,embedding=?,model_id=?,embedding_format=?,speech_seconds=?,sample_count=1,updated_at=CURRENT_TIMESTAMP WHERE id=?")
            .bind(&name).bind(&bytes).bind(MODEL_ID).bind(FORMAT).bind(seconds).bind(id).execute(&mut *tx).await?;
        if changed.rows_affected() != 1 {
            return Err(invalid("This voice was deleted. Reload the voice list."));
        }
        id
    } else {
        sqlx::query("INSERT INTO speaker_profiles(display_name,embedding,model_id,embedding_format,speech_seconds) VALUES(?,?,?,?,?)")
            .bind(&name).bind(&bytes).bind(MODEL_ID).bind(FORMAT).bind(seconds).execute(&mut *tx).await?.last_insert_rowid()
    };
    sqlx::query(
        "UPDATE note_speaker_embeddings SET enrolled_profile_id=? WHERE note_id=? AND speaker_id=?",
    )
    .bind(id)
    .bind(note_id)
    .bind(speaker)
    .execute(&mut *tx)
    .await?;
    sqlx::query("INSERT INTO speaker_mappings(note_id,speaker_id,profile_id,display_name,origin) VALUES(?,?,?,?,'manual') ON CONFLICT(note_id,speaker_id) DO UPDATE SET profile_id=excluded.profile_id,display_name=excluded.display_name,origin='manual'")
        .bind(note_id).bind(speaker).bind(id).bind(name).execute(&mut *tx).await?;
    sqlx::query("INSERT OR IGNORE INTO speaker_manual_overrides VALUES(?,?)")
        .bind(note_id)
        .bind(speaker)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(id)
}
pub async fn rename(pool: &SqlitePool, id: i64, name: &str) -> Result<(), StorageError> {
    let name = name.split_whitespace().collect::<Vec<_>>().join(" ");
    if name.is_empty() {
        return Err(invalid("A voice needs a name"));
    }
    sqlx::query(
        "UPDATE speaker_profiles SET display_name=?,updated_at=CURRENT_TIMESTAMP WHERE id=?",
    )
    .bind(name)
    .bind(id)
    .execute(pool)
    .await?;
    Ok(())
}
pub async fn delete(pool: &SqlitePool, id: i64) -> Result<(), StorageError> {
    let mut tx = pool.begin().await?;
    sqlx::query("DELETE FROM note_speaker_embeddings WHERE enrolled_profile_id=?")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM speaker_profiles WHERE id=?")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(())
}

#[cfg(test)]
#[path = "known_voices_tests.rs"]
mod tests;
