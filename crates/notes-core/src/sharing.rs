//! Portable folder snapshots. File bytes and local database identities never mix.
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::{FromRow, Row, SqlitePool};
use std::collections::{BTreeMap, HashSet};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use ts_rs::TS;
use uuid::Uuid;

pub type ShareResult<T> = Result<T, String>;
pub const MAX_NOTES: usize = 10_000;
pub const MAX_TOTAL: u64 = 32 * 1024 * 1024 * 1024;
pub const MAX_AUDIO: u64 = 4 * 1024 * 1024 * 1024;
pub const MAX_NOTE: u64 = 16 * 1024 * 1024;
pub const MAX_DOCUMENTS: u64 = 256 * 1024 * 1024;
pub const MAX_MANIFEST: u64 = 8 * 1024 * 1024;
fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}
fn cancelled(cancel: &AtomicBool) -> ShareResult<()> {
    if cancel.load(Ordering::Relaxed) {
        Err("Sharing cancelled.".into())
    } else {
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ShareOptions {
    pub personal_notes: bool,
    pub summaries: bool,
    pub transcripts: bool,
    pub recordings: bool,
}
impl Default for ShareOptions {
    fn default() -> Self {
        Self {
            personal_notes: true,
            summaries: true,
            transcripts: true,
            recordings: true,
        }
    }
}
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ShareNotePreview {
    pub id: i64,
    pub title: String,
    pub recording_bytes: u64,
    pub recording_missing: bool,
    pub text_bytes: u64,
}
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SharePreview {
    pub folder_name: String,
    pub notes: Vec<ShareNotePreview>,
    pub options: ShareOptions,
    pub total_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct PortableSummary {
    pub revision: i64,
    pub edited: bool,
    pub source_hash: Option<String>,
    pub source_transcript: Option<String>,
    pub source_participants: Option<String>,
    pub references_json: String,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub language: Option<String>,
    pub prompt_version: Option<i64>,
    pub generated_at: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortableRecording {
    pub entry: String,
    pub duration_seconds: Option<f64>,
    pub original_filename: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortableNote {
    pub title: String,
    pub content: String,
    pub note_type: String,
    pub enhanced_content: Option<String>,
    pub transcript: Option<String>,
    pub participants: Option<String>,
    pub audio_duration_seconds: Option<f64>,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
    pub speakers: BTreeMap<String, String>,
    pub summary: Option<PortableSummary>,
    pub recording: Option<PortableRecording>,
}
#[derive(Debug)]
pub struct ExportNote {
    pub entry: String,
    pub note: PortableNote,
    pub audio_source: Option<PathBuf>,
}
#[derive(Debug)]
pub struct ExportSnapshot {
    pub folder_name: String,
    pub options: ShareOptions,
    pub notes: Vec<ExportNote>,
}
#[derive(Debug, Serialize, Deserialize)]
struct InventoryEntry {
    path: String,
    bytes: u64,
    sha256: String,
}
#[derive(Debug, Serialize, Deserialize)]
struct Manifest {
    format_version: u32,
    package_id: String,
    exported_at: String,
    folder_name: String,
    options: ShareOptions,
    notes: Vec<String>,
    entries: Vec<InventoryEntry>,
}
#[derive(Debug)]
pub struct StagedFolder {
    pub preview: SharePreview,
    pub notes: Vec<PortableNote>,
    pub directory: PathBuf,
}

pub async fn preview(
    pool: &SqlitePool,
    recordings: &Path,
    folder_id: i64,
) -> ShareResult<SharePreview> {
    let folder_name: String = sqlx::query_scalar("SELECT name FROM folders WHERE id=?")
        .bind(folder_id)
        .fetch_optional(pool)
        .await
        .map_err(err)?
        .ok_or("This folder no longer exists.")?;
    let rows = sqlx::query("SELECT n.id,n.title,r.storage_key,LENGTH(CAST(n.content AS BLOB))+COALESCE(LENGTH(CAST(n.transcript AS BLOB)),0)+COALESCE(LENGTH(CAST(n.enhanced_content AS BLOB)),0)+COALESCE(LENGTH(CAST(m.source_transcript AS BLOB)),0) AS text_bytes FROM notes n LEFT JOIN note_recordings r ON r.note_id=n.id LEFT JOIN note_summary_meta m ON m.note_id=n.id WHERE n.folder_id=? ORDER BY n.created_at,n.id LIMIT 10001")
        .bind(folder_id).fetch_all(pool).await.map_err(err)?;
    if rows.len() > MAX_NOTES {
        return Err("A shared folder can contain at most 10,000 notes.".into());
    }
    let mut notes = Vec::new();
    for row in rows {
        let key: Option<String> = row.get("storage_key");
        let metadata = key
            .as_deref()
            .and_then(|k| crate::recordings::resolve(recordings, k))
            .and_then(|p| fs::symlink_metadata(p).ok());
        let missing = key.is_some()
            && !metadata
                .as_ref()
                .is_some_and(|m| m.is_file() && !m.file_type().is_symlink());
        notes.push(ShareNotePreview {
            id: row.get("id"),
            title: row.get("title"),
            recording_bytes: metadata.map_or(0, |m| m.len()),
            recording_missing: missing,
            text_bytes: row.get::<i64, _>("text_bytes").max(0) as u64,
        });
    }
    let total_bytes = notes.iter().map(|n| n.recording_bytes + n.text_bytes).sum();
    Ok(SharePreview {
        folder_name,
        notes,
        options: ShareOptions::default(),
        total_bytes,
    })
}

/// All selected database content is captured on the same read transaction.
pub async fn capture(
    pool: &SqlitePool,
    recordings: &Path,
    folder_id: i64,
    ids: &[i64],
    options: ShareOptions,
) -> ShareResult<ExportSnapshot> {
    if ids.is_empty()
        || ids.len() > MAX_NOTES
        || ids.iter().collect::<HashSet<_>>().len() != ids.len()
    {
        return Err("Choose between 1 and 10,000 different notes.".into());
    }
    let mut tx = pool.begin().await.map_err(err)?;
    let folder_name = sqlx::query_scalar("SELECT name FROM folders WHERE id=?")
        .bind(folder_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(err)?
        .ok_or("This folder no longer exists.")?;
    let mut notes = Vec::new();
    let mut document_bytes = 0;
    for id in ids {
        let note: crate::Note = sqlx::query_as("SELECT id,title,content,recording_details,note_type,enhanced_content,folder_id,transcript,participants,audio_duration_seconds,created_at,updated_at FROM notes WHERE id=? AND folder_id=?")
            .bind(id).bind(folder_id).fetch_optional(&mut *tx).await.map_err(err)?.ok_or("A selected note was moved or deleted. Reopen Share folder and try again.")?;
        let speakers = if options.transcripts {
            sqlx::query_as::<_, (String, String)>(
                "SELECT speaker_id,display_name FROM speaker_mappings WHERE note_id=?",
            )
            .bind(id)
            .fetch_all(&mut *tx)
            .await
            .map_err(err)?
            .into_iter()
            .collect()
        } else {
            BTreeMap::new()
        };
        let summary = if options.summaries && options.transcripts && note.enhanced_content.is_some()
        {
            sqlx::query_as("SELECT revision,edited,source_hash,source_transcript,source_participants,references_json,provider,model,language,prompt_version,generated_at FROM note_summary_meta WHERE note_id=?").bind(id).fetch_optional(&mut *tx).await.map_err(err)?
        } else {
            None
        };
        let enhanced_content = if options.summaries {
            note.enhanced_content.map(|s| {
                if options.transcripts {
                    s
                } else {
                    crate::summary::plain_text(&s)
                }
            })
        } else {
            None
        };
        let recording: Option<(String, Option<f64>, Option<String>)> = if options.recordings {
            sqlx::query_as("SELECT storage_key,duration_seconds,original_filename FROM note_recordings WHERE note_id=?").bind(id).fetch_optional(&mut *tx).await.map_err(err)?
        } else {
            None
        };
        let portable_id = Uuid::new_v4();
        let mut audio_source = None;
        let recording = if let Some((key, duration_seconds, original_filename)) = recording {
            audio_source = Some(
                crate::recordings::resolve(recordings, &key)
                    .ok_or("A recording filename is invalid.")?,
            );
            Some(PortableRecording {
                entry: format!("recordings/{portable_id}.wav"),
                duration_seconds,
                original_filename: original_filename
                    .and_then(|s| s.rsplit(['/', '\\']).next().map(str::to_owned)),
            })
        } else {
            None
        };
        let portable = PortableNote {
            title: note.title,
            content: if options.personal_notes {
                note.content
            } else {
                String::new()
            },
            note_type: note.note_type,
            enhanced_content,
            transcript: if options.transcripts {
                note.transcript
            } else {
                None
            },
            participants: note.participants,
            audio_duration_seconds: note.audio_duration_seconds,
            created_at: note.created_at,
            updated_at: note.updated_at,
            speakers,
            summary,
            recording,
        };
        let size = serde_json::to_vec(&portable).map_err(err)?.len() as u64;
        validate_note(&portable, options)?;
        document_bytes += size;
        if size > MAX_NOTE || document_bytes > MAX_DOCUMENTS {
            return Err("The selected note text exceeds the sharing limit (16 MiB per note, 256 MiB total).".into());
        }
        notes.push(ExportNote {
            entry: format!("notes/{portable_id}.json"),
            note: portable,
            audio_source,
        });
    }
    tx.commit().await.map_err(err)?;
    Ok(ExportSnapshot {
        folder_name,
        options,
        notes,
    })
}

fn copy_checked(
    reader: &mut impl Read,
    writer: &mut impl Write,
    limit: u64,
    cancel: &AtomicBool,
    mut progress: impl FnMut(u64),
) -> ShareResult<(u64, String)> {
    let mut buffer = [0u8; 64 * 1024];
    let mut count = 0;
    let mut hash = Sha256::new();
    loop {
        cancelled(cancel)?;
        let n = reader.read(&mut buffer).map_err(err)?;
        if n == 0 {
            break;
        }
        count += n as u64;
        if count > limit {
            return Err("The package exceeds the allowed size.".into());
        }
        let data = buffer.get(..n).ok_or("Could not read package bytes.")?;
        writer.write_all(data).map_err(err)?;
        hash.update(data);
        progress(n as u64);
    }
    Ok((count, format!("{:x}", hash.finalize())))
}

/// Writes a temporary file supplied by the desktop; publication is separate.
pub fn write_package(
    file: &mut File,
    snapshot: ExportSnapshot,
    cancel: &AtomicBool,
    mut progress: impl FnMut(u64),
) -> ShareResult<()> {
    let mut zip = zip::ZipWriter::new(file);
    let mut entries = Vec::new();
    let mut total = 0;
    let mut note_paths = Vec::new();
    for item in snapshot.notes {
        let bytes = serde_json::to_vec(&item.note).map_err(err)?;
        zip.start_file(
            &item.entry,
            zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated),
        )
        .map_err(err)?;
        let (size, sha256) = copy_checked(
            &mut bytes.as_slice(),
            &mut zip,
            MAX_NOTE,
            cancel,
            &mut progress,
        )?;
        total += size;
        note_paths.push(item.entry.clone());
        entries.push(InventoryEntry {
            path: item.entry,
            bytes: size,
            sha256,
        });
        if let (Some(source), Some(recording)) = (item.audio_source, item.note.recording) {
            let metadata = fs::symlink_metadata(&source).map_err(|_|format!("The recording for ‘{}’ is missing. Retry without recordings or restore the file.",item.note.title))?;
            if !metadata.is_file() || metadata.file_type().is_symlink() {
                return Err("A recording is not a regular file.".into());
            }
            if metadata.len() > MAX_AUDIO || metadata.len() > MAX_TOTAL.saturating_sub(total) {
                return Err("A recording exceeds the sharing size limit.".into());
            }
            validate_audio(&source, cancel)?;
            zip.start_file(
                &recording.entry,
                zip::write::SimpleFileOptions::default()
                    .compression_method(zip::CompressionMethod::Stored)
                    .large_file(true),
            )
            .map_err(err)?;
            let mut audio = File::open(source).map_err(err)?;
            let (size, sha256) = copy_checked(
                &mut audio,
                &mut zip,
                MAX_AUDIO.min(MAX_TOTAL.saturating_sub(total)),
                cancel,
                &mut progress,
            )?;
            total += size;
            entries.push(InventoryEntry {
                path: recording.entry,
                bytes: size,
                sha256,
            });
        }
        if total > MAX_TOTAL {
            return Err("A shared package can contain at most 32 GiB.".into());
        }
    }
    let manifest = Manifest {
        format_version: 1,
        package_id: Uuid::new_v4().to_string(),
        exported_at: chrono::Utc::now().to_rfc3339(),
        folder_name: snapshot.folder_name,
        options: snapshot.options,
        notes: note_paths,
        entries,
    };
    let bytes = serde_json::to_vec(&manifest).map_err(err)?;
    if bytes.len() as u64 > MAX_MANIFEST || total + bytes.len() as u64 > MAX_TOTAL {
        return Err("The package manifest exceeds the sharing limit.".into());
    }
    cancelled(cancel)?;
    zip.start_file(
        "manifest.json",
        zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated),
    )
    .map_err(err)?;
    zip.write_all(&bytes).map_err(err)?;
    zip.finish().map_err(err)?.sync_all().map_err(err)?;
    Ok(())
}

fn valid_entry(path: &str, prefix: &str, suffix: &str) -> bool {
    path.strip_prefix(prefix)
        .and_then(|s| s.strip_suffix(suffix))
        .is_some_and(|s| Uuid::parse_str(s).is_ok() && !s.contains(['/', '\\']))
}
fn validate_note(note: &PortableNote, options: ShareOptions) -> ShareResult<()> {
    if (!options.personal_notes && !note.content.is_empty())
        || (!options.summaries && (note.enhanced_content.is_some() || note.summary.is_some()))
        || (!options.transcripts
            && (note.transcript.is_some() || !note.speakers.is_empty() || note.summary.is_some()))
        || (!options.recordings && note.recording.is_some())
    {
        return Err("The package contains content its manifest says is excluded.".into());
    }
    if let Some(summary) = &note.summary {
        let references: Vec<crate::summary::SummaryReference> =
            serde_json::from_str(&summary.references_json).map_err(err)?;
        if !references.is_empty() {
            let source = summary
                .source_transcript
                .as_deref()
                .ok_or("Summary references have no source transcript.")?;
            let segments: Vec<crate::transcript::Segment> =
                serde_json::from_str(source).map_err(err)?;
            if references.iter().any(|r| r.row_index >= segments.len()) {
                return Err("A summary source reference is invalid.".into());
            }
        }
    }
    Ok(())
}
fn validate_audio(path: &Path, cancel: &AtomicBool) -> ShareResult<()> {
    let mut reader = hound::WavReader::open(path)
        .map_err(|e| format!("A shared recording is not a valid WAV file: {e}"))?;
    let spec = reader.spec();
    if spec.sample_format != hound::SampleFormat::Int
        || spec.bits_per_sample != 16
        || !(1..=2).contains(&spec.channels)
        || spec.sample_rate == 0
        || reader.duration() == 0
    {
        return Err(
            "Shared recordings must be nonempty 16-bit PCM WAV audio with one or two channels."
                .into(),
        );
    }
    for sample in reader.samples::<i16>() {
        cancelled(cancel)?;
        sample.map_err(err)?;
    }
    Ok(())
}

/// Extract only enumerated entries into an application-owned operation directory.
pub fn stage_package(
    source: &Path,
    directory: &Path,
    cancel: &AtomicBool,
    mut progress: impl FnMut(u64),
) -> ShareResult<StagedFolder> {
    let mut zip = zip::ZipArchive::new(File::open(source).map_err(err)?).map_err(err)?;
    if zip.len() > 2 * MAX_NOTES + 1 {
        return Err("A package can contain at most 20,001 entries.".into());
    }
    let mut actual = HashSet::new();
    for i in 0..zip.len() {
        let entry = zip.by_index(i).map_err(err)?;
        if entry.encrypted()
            || entry.is_dir()
            || entry.unix_mode().is_some_and(|m| m & 0o170000 == 0o120000)
            || !actual.insert(entry.name().to_owned())
        {
            return Err(
                "Encrypted, duplicate, directory and symbolic-link entries are not supported."
                    .into(),
            );
        }
    }
    let mut bytes = Vec::new();
    let manifest_bytes = copy_checked(
        &mut zip.by_name("manifest.json").map_err(err)?,
        &mut bytes,
        MAX_MANIFEST,
        cancel,
        |_| {},
    )?
    .0;
    let manifest: Manifest = serde_json::from_slice(&bytes).map_err(err)?;
    if manifest.format_version != 1 {
        return Err(
            "This sharing format is not supported. Update KnegisWhisper and try again.".into(),
        );
    }
    Uuid::parse_str(&manifest.package_id).map_err(err)?;
    if manifest.notes.is_empty()
        || manifest.notes.len() > MAX_NOTES
        || manifest.folder_name.trim().is_empty()
    {
        return Err("The package must contain a named folder with 1 to 10,000 notes.".into());
    }
    let mut expected = HashSet::from(["manifest.json".to_owned()]);
    let mut total = manifest_bytes;
    let mut documents = 0;
    fs::create_dir_all(directory).map_err(err)?;
    let mut inventory_notes = HashSet::new();
    let mut inventory_audio = HashSet::new();
    for entry in &manifest.entries {
        let note = valid_entry(&entry.path, "notes/", ".json");
        let audio = valid_entry(&entry.path, "recordings/", ".wav");
        if (!note && !audio) || !expected.insert(entry.path.clone()) {
            return Err("The package inventory contains an unsafe or duplicate filename.".into());
        }
        let limit = if note { MAX_NOTE } else { MAX_AUDIO };
        if entry.bytes > limit || entry.sha256.len() != 64 {
            return Err("An entry exceeds its size limit or has an invalid digest.".into());
        }
        let mut input = zip.by_name(&entry.path).map_err(err)?;
        if input.size() != entry.bytes {
            return Err("An entry size disagrees with the manifest.".into());
        }
        let filename = entry.path.split('/').next_back().ok_or("Invalid entry.")?;
        let path = directory.join(filename);
        let mut output = File::options()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(err)?;
        let (size, digest) = copy_checked(
            &mut input,
            &mut output,
            limit.min(MAX_TOTAL.saturating_sub(total)),
            cancel,
            &mut progress,
        )?;
        output.sync_all().map_err(err)?;
        if size != entry.bytes || digest != entry.sha256 {
            return Err("A shared file is damaged (size or digest mismatch).".into());
        }
        total += size;
        if note {
            documents += size;
            inventory_notes.insert(entry.path.clone());
        } else {
            inventory_audio.insert(entry.path.clone());
            validate_audio(&path, cancel)?;
        }
        if documents > MAX_DOCUMENTS || total > MAX_TOTAL {
            return Err("The package exceeds its total text or audio size limit.".into());
        }
    }
    if actual != expected
        || manifest.notes.iter().collect::<HashSet<_>>().len() != manifest.notes.len()
        || inventory_notes != manifest.notes.iter().cloned().collect()
    {
        return Err("The archive files disagree with its inventory.".into());
    }
    let mut notes = Vec::new();
    let mut previews = Vec::new();
    let mut used_audio = HashSet::new();
    for (index, entry) in manifest.notes.iter().enumerate() {
        let filename = entry.split('/').next_back().ok_or("Invalid note entry.")?;
        let note: PortableNote =
            serde_json::from_reader(File::open(directory.join(filename)).map_err(err)?)
                .map_err(err)?;
        validate_note(&note, manifest.options)?;
        let recording_bytes = if let Some(audio) = &note.recording {
            if !inventory_audio.contains(&audio.entry) || !used_audio.insert(audio.entry.clone()) {
                return Err("A recording reference is missing or duplicated.".into());
            }
            manifest
                .entries
                .iter()
                .find(|e| e.path == audio.entry)
                .map_or(0, |e| e.bytes)
        } else {
            0
        };
        previews.push(ShareNotePreview {
            id: index as i64,
            title: note.title.clone(),
            recording_bytes,
            recording_missing: false,
            text_bytes: fs::metadata(directory.join(filename)).map_err(err)?.len(),
        });
        notes.push(note);
    }
    if used_audio != inventory_audio {
        return Err("The package contains an unreferenced recording.".into());
    }
    Ok(StagedFolder {
        preview: SharePreview {
            folder_name: manifest.folder_name,
            notes: previews,
            options: manifest.options,
            total_bytes: total,
        },
        notes,
        directory: directory.to_owned(),
    })
}

pub async fn journal(
    pool: &SqlitePool,
    id: &str,
    direction: &str,
    staging: &Path,
) -> ShareResult<()> {
    sqlx::query("INSERT INTO folder_share_operations(id,direction,stage,staging_path) VALUES (?,?,'preparing',?)").bind(id).bind(direction).bind(staging.to_string_lossy().as_ref()).execute(pool).await.map_err(err)?;
    Ok(())
}
pub async fn journal_stage(
    pool: &SqlitePool,
    id: &str,
    stage: &str,
    error: Option<&str>,
) -> ShareResult<()> {
    sqlx::query("UPDATE folder_share_operations SET stage=?,error=? WHERE id=?")
        .bind(stage)
        .bind(error)
        .bind(id)
        .execute(pool)
        .await
        .map_err(err)?;
    Ok(())
}

#[derive(Debug, FromRow)]
pub struct SharingReceipt {
    pub id: String,
    pub direction: String,
    pub stage: String,
    pub result_folder_id: Option<i64>,
    pub result_path: Option<String>,
    pub error: Option<String>,
}
pub async fn last_receipt(pool: &SqlitePool) -> ShareResult<Option<SharingReceipt>> {
    sqlx::query_as("SELECT id,direction,stage,result_folder_id,result_path,error FROM folder_share_operations ORDER BY rowid DESC LIMIT 1").fetch_optional(pool).await.map_err(err)
}

pub async fn restore(
    pool: &SqlitePool,
    recordings: &Path,
    id: &str,
    folder: StagedFolder,
    requested_name: &str,
    cancel: &AtomicBool,
) -> ShareResult<i64> {
    let requested_name = requested_name.trim();
    if requested_name.is_empty() || requested_name.len() > 1024 {
        return Err("Choose a folder name between 1 and 1,024 bytes.".into());
    }
    cancelled(cancel)?;
    let keys: Vec<String> = folder
        .notes
        .iter()
        .enumerate()
        .filter(|(_, n)| n.recording.is_some())
        .map(|(i, _)| format!("shared-{id}-{i}.wav"))
        .collect();
    sqlx::query("UPDATE folder_share_operations SET stage='saving',recording_keys=? WHERE id=?")
        .bind(serde_json::to_string(&keys).map_err(err)?)
        .bind(id)
        .execute(pool)
        .await
        .map_err(err)?;
    fs::create_dir_all(recordings).map_err(err)?;
    for (i, note) in folder.notes.iter().enumerate() {
        if let Some(audio) = &note.recording {
            let source = folder.directory.join(
                audio
                    .entry
                    .split('/')
                    .next_back()
                    .ok_or("Invalid recording entry.")?,
            );
            let destination = recordings.join(format!("shared-{id}-{i}.wav"));
            if destination.exists() {
                return Err("A shared recording destination already exists.".into());
            }
            fs::rename(source, destination).map_err(err)?;
        }
    }
    let mut tx = pool.begin().await.map_err(err)?;
    // The first write holds the SQLite writer lock before choosing a unique name.
    sqlx::query("UPDATE folder_share_operations SET stage='saving' WHERE id=?")
        .bind(id)
        .execute(&mut *tx)
        .await
        .map_err(err)?;
    let mut name = requested_name.to_owned();
    let mut suffix = 2;
    while sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM folders WHERE name=?)")
        .bind(&name)
        .fetch_one(&mut *tx)
        .await
        .map_err(err)?
    {
        name = format!("{requested_name} ({suffix})");
        suffix += 1;
    }
    let folder_id: i64 = sqlx::query_scalar("INSERT INTO folders(name) VALUES (?) RETURNING id")
        .bind(name)
        .fetch_one(&mut *tx)
        .await
        .map_err(err)?;
    for (i, note) in folder.notes.into_iter().enumerate() {
        let note_id:i64=sqlx::query_scalar("INSERT INTO notes(title,content,note_type,enhanced_content,folder_id,transcript,participants,audio_duration_seconds,created_at,updated_at) VALUES (?,?,?,?,?,?,?,?,?,?) RETURNING id")
            .bind(note.title).bind(note.content).bind(note.note_type).bind(note.enhanced_content).bind(folder_id).bind(note.transcript).bind(note.participants).bind(note.audio_duration_seconds).bind(note.created_at).bind(note.updated_at).fetch_one(&mut *tx).await.map_err(err)?;
        for (speaker, name) in note.speakers {
            sqlx::query(
                "INSERT INTO speaker_mappings(note_id,speaker_id,display_name) VALUES (?,?,?)",
            )
            .bind(note_id)
            .bind(speaker)
            .bind(name)
            .execute(&mut *tx)
            .await
            .map_err(err)?;
        }
        if let Some(s) = note.summary {
            sqlx::query("INSERT INTO note_summary_meta(note_id,revision,edited,source_hash,source_transcript,source_participants,references_json,provider,model,language,prompt_version,generated_at) VALUES (?,?,?,?,?,?,?,?,?,?,?,?)")
                .bind(note_id).bind(s.revision).bind(s.edited).bind(s.source_hash).bind(s.source_transcript).bind(s.source_participants).bind(s.references_json).bind(s.provider).bind(s.model).bind(s.language).bind(s.prompt_version).bind(s.generated_at).execute(&mut *tx).await.map_err(err)?;
        }
        if let Some(audio) = note.recording {
            sqlx::query("INSERT INTO note_recordings(note_id,storage_key,duration_seconds,original_filename) VALUES (?,?,?,?)").bind(note_id).bind(format!("shared-{id}-{i}.wav")).bind(audio.duration_seconds).bind(audio.original_filename).execute(&mut *tx).await.map_err(err)?;
        }
    }
    sqlx::query(
        "UPDATE folder_share_operations SET stage='succeeded',result_folder_id=? WHERE id=?",
    )
    .bind(folder_id)
    .bind(id)
    .execute(&mut *tx)
    .await
    .map_err(err)?;
    tx.commit().await.map_err(err)?;
    Ok(folder_id)
}

/// Cleanup is limited to this operation's generated filenames and staging path.
pub async fn cleanup(pool: &SqlitePool, recordings: &Path, id: &str) -> ShareResult<()> {
    Uuid::parse_str(id).map_err(err)?;
    let row=sqlx::query("SELECT direction,staging_path,recording_keys,stage FROM folder_share_operations WHERE id=?").bind(id).fetch_optional(pool).await.map_err(err)?;
    let Some(row) = row else {
        return Ok(());
    };
    let keys: Vec<String> = serde_json::from_str(row.get("recording_keys")).map_err(err)?;
    if row.get::<String, _>("stage") != "succeeded" {
        for key in keys {
            if !crate::recordings::is_valid_storage_key(&key)
                || !key.starts_with(&format!("shared-{id}-"))
            {
                return Err("Refused to clean an unrelated recording.".into());
            }
            let attached: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM note_recordings WHERE storage_key=?)",
            )
            .bind(&key)
            .fetch_one(pool)
            .await
            .map_err(err)?;
            if !attached {
                remove_if_present(&recordings.join(key))?;
            }
        }
    }
    let staging = PathBuf::from(row.get::<String, _>("staging_path"));
    if row.get::<String, _>("direction") == "import" {
        if staging != recordings.join(".sharing").join(id) {
            return Err("Refused to clean an unrelated staging directory.".into());
        }
        match fs::symlink_metadata(&staging) {
            Ok(m) if m.is_dir() && !m.file_type().is_symlink() => {
                fs::remove_dir_all(staging).map_err(err)?
            }
            Ok(_) => return Err("Refused to clean a linked staging directory.".into()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(err(e)),
        }
    } else {
        if !staging
            .file_name()
            .and_then(|s| s.to_str())
            .is_some_and(|s| s.starts_with(&format!(".knegis-share-{id}-")))
        {
            return Err("Refused to clean an unrelated export file.".into());
        }
        remove_if_present(&staging)?;
    }
    Ok(())
}
fn remove_if_present(path: &Path) -> ShareResult<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(err(e)),
    }
}
pub fn package_digest(path: &Path, cancel: &AtomicBool) -> ShareResult<String> {
    Ok(copy_checked(
        &mut File::open(path).map_err(err)?,
        &mut std::io::sink(),
        MAX_TOTAL + MAX_MANIFEST + 16 * 1024 * 1024,
        cancel,
        |_| {},
    )?
    .1)
}
pub async fn publishing(
    pool: &SqlitePool,
    id: &str,
    destination: &Path,
    digest: &str,
) -> ShareResult<()> {
    sqlx::query("UPDATE folder_share_operations SET stage='publishing',result_path=?,result_digest=? WHERE id=?")
        .bind(destination.to_string_lossy().as_ref()).bind(digest).bind(id).execute(pool).await.map_err(err)?;
    Ok(())
}
pub async fn recover(pool: &SqlitePool, recordings: &Path) -> ShareResult<()> {
    let rows: Vec<(String, String)> =
        sqlx::query_as("SELECT id,stage FROM folder_share_operations")
            .fetch_all(pool)
            .await
            .map_err(err)?;
    for (id, stage) in rows {
        if stage == "publishing" {
            let result: (Option<String>, Option<String>) = sqlx::query_as(
                "SELECT result_path,result_digest FROM folder_share_operations WHERE id=?",
            )
            .bind(&id)
            .fetch_one(pool)
            .await
            .map_err(err)?;
            if let (Some(path), Some(digest)) = result {
                if package_digest(Path::new(&path), &AtomicBool::new(false))
                    .is_ok_and(|actual| actual == digest)
                {
                    journal_stage(pool, &id, "succeeded", None).await?;
                    cleanup(pool, recordings, &id).await?;
                    continue;
                }
            }
        }
        cleanup(pool, recordings, &id).await?;
        if !matches!(
            stage.as_str(),
            "succeeded" | "failed" | "cancelled" | "interrupted" | "dismissed"
        ) {
            journal_stage(pool,&id,"interrupted",Some("The app closed before sharing finished. Choose the folder or package again to retry.")).await?;
        }
    }
    Ok(())
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
    use crate::{notes, recordings, speakers};
    async fn fixture(root: &Path) -> (kw_storage::Db, i64, i64) {
        fs::create_dir_all(root).unwrap();
        let db = kw_storage::Db::open_in_memory().await.unwrap();
        let folder = notes::create_folder(db.pool(), "Projekt Åäö")
            .await
            .unwrap();
        let transcript =
            r#"[{"text":"Sökord Göteborg","speaker":"speaker_0","start":0,"end":0.1}]"#;
        let id = notes::create(
            db.pool(),
            &crate::NewNote {
                title: "Möte 日本語".into(),
                content: "PERSONAL-PRIVATE".into(),
                folder_id: Some(folder),
                transcript: Some(transcript.into()),
                participants: Some("[\"Axel\"]".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        speakers::rename(db.pool(), id, "speaker_0", "Åsa")
            .await
            .unwrap();
        sqlx::query("UPDATE notes SET enhanced_content='SUMMARY [source](#summary-source-s1)',created_at='2026-01-02 03:04:05',updated_at='2026-02-03 04:05:06' WHERE id=?").bind(id).execute(db.pool()).await.unwrap();
        sqlx::query("INSERT INTO note_summary_meta(note_id,edited,source_hash,source_transcript,references_json,provider,model) VALUES (?,1,'older-source',?,'[{\"id\":\"s1\",\"rowIndex\":0}]','local','example')").bind(id).bind(transcript).execute(db.pool()).await.unwrap();
        let mut writer = hound::WavWriter::create(
            root.join("original.wav"),
            hound::WavSpec {
                channels: 1,
                sample_rate: 16000,
                bits_per_sample: 16,
                sample_format: hound::SampleFormat::Int,
            },
        )
        .unwrap();
        for _ in 0..1600 {
            writer.write_sample(7i16).unwrap();
        }
        writer.finalize().unwrap();
        recordings::attach_and_get(db.pool(), id, "original.wav", Some(0.1), None)
            .await
            .unwrap();
        (db, folder, id)
    }
    async fn export(
        db: &kw_storage::Db,
        root: &Path,
        folder: i64,
        id: i64,
        options: ShareOptions,
        path: &Path,
    ) {
        let snapshot = capture(db.pool(), root, folder, &[id], options)
            .await
            .unwrap();
        write_package(
            &mut File::create(path).unwrap(),
            snapshot,
            &AtomicBool::new(false),
            |_| {},
        )
        .unwrap();
    }
    async fn stage(db: &kw_storage::Db, root: &Path, package: &Path) -> (String, StagedFolder) {
        let id = Uuid::new_v4().to_string();
        let directory = root.join(".sharing").join(&id);
        journal(db.pool(), &id, "import", &directory).await.unwrap();
        (
            id,
            stage_package(package, &directory, &AtomicBool::new(false), |_| {}).unwrap(),
        )
    }
    #[tokio::test]
    async fn round_trip_preserves_sources_audio_search_dates_and_independent_copies() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("audio");
        let (db, folder, note) = fixture(&root).await;
        let package = temp.path().join("folder.knegis");
        export(&db, &root, folder, note, ShareOptions::default(), &package).await;
        let (operation, staged) = stage(&db, &root, &package).await;
        let imported = restore(
            db.pool(),
            &root,
            &operation,
            staged,
            "Projekt Åäö",
            &AtomicBool::new(false),
        )
        .await
        .unwrap();
        assert_ne!(imported, folder);
        assert_eq!(
            notes::folders(db.pool())
                .await
                .unwrap()
                .iter()
                .find(|f| f.id == imported)
                .unwrap()
                .name,
            "Projekt Åäö (2)"
        );
        let imported_note = notes::list(db.pool(), None, Some(imported), 10)
            .await
            .unwrap()
            .pop()
            .unwrap();
        assert_ne!(imported_note.id, note);
        assert_eq!(imported_note.title, "Möte 日本語");
        assert_eq!(imported_note.content, "PERSONAL-PRIVATE");
        assert_eq!(
            imported_note.created_at.as_deref(),
            Some("2026-01-02 03:04:05")
        );
        assert_eq!(
            speakers::mappings(db.pool(), imported_note.id)
                .await
                .unwrap()
                .get("speaker_0")
                .unwrap(),
            "Åsa"
        );
        let audio = recordings::get(db.pool(), imported_note.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            fs::read(root.join(&audio.storage_key)).unwrap(),
            fs::read(root.join("original.wav")).unwrap()
        );
        let summary = crate::summary::state(db.pool(), imported_note.id)
            .await
            .unwrap();
        assert!(summary.edited);
        assert!(summary.outdated);
        assert_eq!(summary.references.len(), 1);
        assert!(notes::search(db.pool(), "Göteborg", 10)
            .await
            .unwrap()
            .iter()
            .any(|n| n.id == imported_note.id));
        let jobs: i64 = sqlx::query_scalar("SELECT count(*) FROM note_summary_jobs")
            .fetch_one(db.pool())
            .await
            .unwrap();
        assert_eq!(jobs, 0);
        recover(db.pool(), &root).await.unwrap();
        assert!(root.join(&audio.storage_key).exists());
        let (again, staged) = stage(&db, &root, &package).await;
        let duplicate = restore(
            db.pool(),
            &root,
            &again,
            staged,
            "Projekt Åäö",
            &AtomicBool::new(false),
        )
        .await
        .unwrap();
        assert_ne!(duplicate, imported);
        assert!(root.join("original.wav").exists());
    }
    #[tokio::test]
    async fn all_content_combinations_exclude_private_data_and_restore_without_jobs() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("audio");
        let (source, folder, note) = fixture(&root).await;
        for mask in 0..16 {
            let options = ShareOptions {
                personal_notes: mask & 1 != 0,
                summaries: mask & 2 != 0,
                transcripts: mask & 4 != 0,
                recordings: mask & 8 != 0,
            };
            let package = temp.path().join(format!("{mask}.knegis"));
            export(&source, &root, folder, note, options, &package).await;
            let receiver = kw_storage::Db::open_in_memory().await.unwrap();
            let target = temp.path().join(format!("target-{mask}"));
            let (operation, staged) = stage(&receiver, &target, &package).await;
            let json = serde_json::to_string(&staged.notes).unwrap();
            assert_eq!(json.contains("PERSONAL-PRIVATE"), options.personal_notes);
            assert_eq!(json.contains("SUMMARY"), options.summaries);
            assert_eq!(json.contains("Göteborg"), options.transcripts);
            assert_eq!(staged.notes[0].recording.is_some(), options.recordings);
            if !options.transcripts {
                assert!(!json.contains("summary-source"));
                assert!(!json.contains("source_transcript"));
            }
            let imported = restore(
                receiver.pool(),
                &target,
                &operation,
                staged,
                "Imported",
                &AtomicBool::new(false),
            )
            .await
            .unwrap();
            assert_eq!(
                notes::list(receiver.pool(), None, Some(imported), 10)
                    .await
                    .unwrap()
                    .len(),
                1
            );
        }
    }
    #[tokio::test]
    async fn rollback_and_startup_recovery_remove_only_uncommitted_files() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("audio");
        let (db, folder, note) = fixture(&root).await;
        let package = temp.path().join("folder.knegis");
        export(&db, &root, folder, note, ShareOptions::default(), &package).await;
        let (operation, staged) = stage(&db, &root, &package).await;
        sqlx::query("CREATE TRIGGER fail_import BEFORE INSERT ON notes BEGIN SELECT RAISE(ABORT,'forced failure'); END").execute(db.pool()).await.unwrap();
        assert!(restore(
            db.pool(),
            &root,
            &operation,
            staged,
            "Imported",
            &AtomicBool::new(false)
        )
        .await
        .is_err());
        assert!(root.join(format!("shared-{operation}-0.wav")).exists());
        recover(db.pool(), &root).await.unwrap();
        assert!(!root.join(format!("shared-{operation}-0.wav")).exists());
        assert!(root.join("original.wav").exists());
        assert_eq!(notes::folders(db.pool()).await.unwrap().len(), 1);
        let stage: String =
            sqlx::query_scalar("SELECT stage FROM folder_share_operations WHERE id=?")
                .bind(operation)
                .fetch_one(db.pool())
                .await
                .unwrap();
        assert_eq!(stage, "interrupted");
    }
    #[tokio::test]
    async fn recovery_distinguishes_published_exports_from_incomplete_publication() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("audio");
        let (db, folder, note) = fixture(&root).await;
        let destination = temp.path().join("published.knegis");
        export(
            &db,
            &root,
            folder,
            note,
            ShareOptions::default(),
            &destination,
        )
        .await;
        let digest = package_digest(&destination, &AtomicBool::new(false)).unwrap();
        for published in [true, false] {
            let operation = Uuid::new_v4().to_string();
            let staging = temp
                .path()
                .join(format!(".knegis-share-{operation}-staging"));
            fs::write(&staging, b"unfinished temporary file").unwrap();
            journal(db.pool(), &operation, "export", &staging)
                .await
                .unwrap();
            let expected = if published {
                digest.as_str()
            } else {
                "incorrect-digest"
            };
            publishing(db.pool(), &operation, &destination, expected)
                .await
                .unwrap();
            recover(db.pool(), &root).await.unwrap();
            assert!(!staging.exists());
            assert!(destination.exists());
            let receipt = last_receipt(db.pool()).await.unwrap().unwrap();
            assert_eq!(receipt.id, operation);
            assert_eq!(
                receipt.stage,
                if published {
                    "succeeded"
                } else {
                    "interrupted"
                }
            );
            assert_eq!(
                package_digest(&destination, &AtomicBool::new(false)).unwrap(),
                digest
            );
        }
    }
    #[tokio::test]
    async fn selection_missing_recording_and_cancel_are_explicit() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("audio");
        let (db, folder, note) = fixture(&root).await;
        let other = notes::create_folder(db.pool(), "Other").await.unwrap();
        notes::create(
            db.pool(),
            &crate::NewNote {
                title: "Deselected private note".into(),
                content: "DESELECTED-PRIVATE".into(),
                folder_id: Some(folder),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(
            preview(db.pool(), &root, folder).await.unwrap().notes.len(),
            2
        );
        let selected = capture(db.pool(), &root, folder, &[note], ShareOptions::default())
            .await
            .unwrap();
        assert_eq!(selected.notes.len(), 1);
        assert!(!serde_json::to_string(&selected.notes[0].note)
            .unwrap()
            .contains("DESELECTED-PRIVATE"));
        assert!(
            capture(db.pool(), &root, other, &[note], ShareOptions::default())
                .await
                .is_err()
        );
        assert!(capture(
            db.pool(),
            &root,
            folder,
            &[note, note],
            ShareOptions::default()
        )
        .await
        .is_err());
        fs::remove_file(root.join("original.wav")).unwrap();
        assert!(preview(db.pool(), &root, folder).await.unwrap().notes[0].recording_missing);
        let snapshot = capture(db.pool(), &root, folder, &[note], ShareOptions::default())
            .await
            .unwrap();
        assert!(write_package(
            &mut File::create(temp.path().join("missing")).unwrap(),
            snapshot,
            &AtomicBool::new(false),
            |_| {}
        )
        .is_err());
        let options = ShareOptions {
            recordings: false,
            ..Default::default()
        };
        let package = temp.path().join("no-audio");
        export(&db, &root, folder, note, options, &package).await;
        assert!(stage_package(
            &package,
            &temp.path().join("cancelled"),
            &AtomicBool::new(true),
            |_| {}
        )
        .is_err());
    }
    fn rewrite(package: &Path, mutate: impl FnOnce(&mut Manifest, &mut Vec<(String, Vec<u8>)>)) {
        let mut zip = zip::ZipArchive::new(File::open(package).unwrap()).unwrap();
        let mut entries = Vec::new();
        for i in 0..zip.len() {
            let mut entry = zip.by_index(i).unwrap();
            let mut bytes = Vec::new();
            entry.read_to_end(&mut bytes).unwrap();
            entries.push((entry.name().to_owned(), bytes));
        }
        let manifest_index = entries
            .iter()
            .position(|(n, _)| n == "manifest.json")
            .unwrap();
        let mut manifest: Manifest = serde_json::from_slice(&entries[manifest_index].1).unwrap();
        drop(zip);
        mutate(&mut manifest, &mut entries);
        entries[manifest_index].1 = serde_json::to_vec(&manifest).unwrap();
        let mut writer = zip::ZipWriter::new(File::create(package).unwrap());
        for (name, bytes) in entries {
            writer
                .start_file(name, zip::write::SimpleFileOptions::default())
                .unwrap();
            writer.write_all(&bytes).unwrap();
        }
        writer.finish().unwrap();
    }
    #[tokio::test]
    async fn corrupt_and_hostile_archives_cannot_escape_staging_or_import() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("audio");
        let (db, folder, note) = fixture(&root).await;
        for case in 0..9 {
            let package = temp.path().join(format!("bad-{case}"));
            export(&db, &root, folder, note, ShareOptions::default(), &package).await;
            rewrite(&package, |manifest, entries| match case {
                0 => manifest.format_version = 999,
                1 => manifest.entries[0].sha256 = "0".repeat(64),
                2 => {
                    entries.push(("../outside".into(), b"bad".to_vec()));
                }
                3 => manifest.notes.push(manifest.notes[0].clone()),
                4 => manifest.entries[0].bytes = MAX_NOTE + 1,
                5 => {
                    manifest.entries[0].path = "../outside".into();
                }
                _ => {
                    let suffix = if case == 6 { ".wav" } else { ".json" };
                    let (name, bytes) = entries
                        .iter_mut()
                        .find(|(name, _)| name.ends_with(suffix))
                        .unwrap();
                    *bytes = match case {
                        6 => b"not WAV audio".to_vec(),
                        7 => b"not JSON".to_vec(),
                        _ => vec![b' '; (MAX_NOTE + 1) as usize],
                    };
                    let inventory = manifest
                        .entries
                        .iter_mut()
                        .find(|item| &item.path == name)
                        .unwrap();
                    inventory.bytes = bytes.len() as u64;
                    inventory.sha256 = format!("{:x}", Sha256::digest(bytes));
                }
            });
            assert!(stage_package(
                &package,
                &temp.path().join(format!("stage-{case}")),
                &AtomicBool::new(false),
                |_| {}
            )
            .is_err());
        }
        assert!(!temp.path().join("outside").exists());
        assert_eq!(notes::folders(db.pool()).await.unwrap().len(), 1);
    }
}
