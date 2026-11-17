//! Find application-owned meeting WAVs that never acquired a note.
//!
//! The renderer receives only bare file names. Recovery resolves them inside
//! the same recording directory used for playback and rechecks attachment
//! state immediately before opening a file.

use std::collections::HashSet;
use std::path::Path;

use kw_storage::Db;
use serde::Serialize;
use ts_rs::TS;

use super::draft;

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct RecoverableRecording {
    pub storage_key: String,
    pub has_notes: bool,
    /// The name the meeting was given before Start, from the plan beside it
    /// (KNEG-365), tidied as the recovered note's title will be. `None` when
    /// it was given none, or has no plan.
    pub title: Option<String>,
    /// The application never saw this meeting end: it crashed or was closed
    /// while the meeting ran or was being saved (`draft::unfinished_path`).
    /// `false` for a recording left with no note on purpose, and for any
    /// made before the marker existed.
    pub unfinished: bool,
}

pub async fn scan(db: &Db, directory: &Path) -> Result<Vec<RecoverableRecording>, String> {
    let owned: HashSet<String> = kw_notes_core::recordings::storage_keys(db.pool())
        .await
        .map_err(|error| format!("Could not inspect saved meeting recordings: {error}"))?
        .into_iter()
        .collect();
    let entries = match std::fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(format!("Could not list meeting recordings: {error}")),
    };
    let mut found = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| format!("Could not inspect a recording: {error}"))?;
        let path = entry.path();
        let Some(key) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if !key.ends_with(".wav")
            || !kw_notes_core::recordings::is_valid_storage_key(key)
            || owned.contains(key)
            || !entry
                .file_type()
                .map_err(|error| format!("Could not inspect {key}: {error}"))?
                .is_file()
        {
            continue;
        }
        let has_notes = std::fs::symlink_metadata(draft::sidecar_path(&path))
            .is_ok_and(|metadata| metadata.is_file());
        let title = draft::read_plan(&path)
            .and_then(|plan| plan.title)
            .and_then(|title| super::clean_title(&title));
        found.push(RecoverableRecording {
            storage_key: key.to_owned(),
            has_notes,
            title,
            unfinished: draft::is_unfinished(&path),
        });
    }
    found.sort_by(|a, b| b.storage_key.cmp(&a.storage_key));
    Ok(found)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn only_unattached_regular_wavs_are_offered() {
        let db = Db::open_in_memory().await.unwrap();
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("orphan.wav"), b"wav").unwrap();
        std::fs::write(dir.path().join("orphan.notes.md"), b"typed").unwrap();
        std::fs::write(dir.path().join("other.txt"), b"not audio").unwrap();
        let candidates = scan(&db, dir.path()).await.unwrap();
        assert_eq!(candidates.len(), 1);
        let candidate = candidates.first().unwrap();
        assert_eq!(candidate.storage_key, "orphan.wav");
        assert!(candidate.has_notes);

        let note = kw_notes_core::notes::NewNote {
            title: "saved".into(),
            content: "saved".into(),
            ..Default::default()
        };
        kw_notes_core::notes::create_meeting_with_recording(db.pool(), &note, "orphan.wav", 1.0)
            .await
            .unwrap();
        assert!(scan(&db, dir.path()).await.unwrap().is_empty());
        assert!(kw_notes_core::notes::create_meeting_with_recording(
            db.pool(),
            &note,
            "orphan.wav",
            1.0,
        )
        .await
        .is_err());
        assert_eq!(
            kw_notes_core::notes::list(db.pool(), None, None, 10)
                .await
                .unwrap()
                .len(),
            1
        );
    }

    /// KNEG-401. A note in Trash still owns its recording -- Restore brings
    /// it back with it -- so its WAV is not an interrupted meeting, and
    /// recovering it would make a second note of the same recording.
    #[tokio::test]
    async fn a_recording_whose_note_is_in_trash_is_not_offered() {
        let db = Db::open_in_memory().await.unwrap();
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("20261008-090000-a.wav"), b"wav").unwrap();
        let note = kw_notes_core::notes::NewNote {
            title: "Board review".into(),
            ..Default::default()
        };
        let (id, _) = kw_notes_core::notes::create_meeting_with_recording(
            db.pool(),
            &note,
            "20261008-090000-a.wav",
            1.0,
        )
        .await
        .unwrap();
        assert!(kw_notes_core::trash::trash(db.pool(), id).await.unwrap());

        assert!(scan(&db, dir.path()).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_meeting_the_application_never_saw_end_is_told_from_one_left_on_purpose() {
        let db = Db::open_in_memory().await.unwrap();
        let dir = tempfile::tempdir().unwrap();
        let crashed = dir.path().join("20261117-090000-a.wav");
        let quiet = dir.path().join("20261116-090000-b.wav");
        std::fs::write(&crashed, b"wav").unwrap();
        std::fs::write(&quiet, b"wav").unwrap();
        // Start marks both; only the quiet room's meeting reached its end.
        draft::mark_unfinished(&crashed);
        draft::mark_unfinished(&quiet);
        draft::clear_unfinished(&quiet);

        let found: Vec<_> = scan(&db, dir.path())
            .await
            .unwrap()
            .into_iter()
            .map(|c| (c.storage_key, c.unfinished))
            .collect();
        assert_eq!(
            found,
            [
                ("20261117-090000-a.wav".to_owned(), true),
                ("20261116-090000-b.wav".to_owned(), false),
            ]
        );

        // Recovered and saved, it is nobody's unfinished meeting.
        draft::remove_sidecars(&crashed);
        assert!(!draft::is_unfinished(&crashed));
    }

    #[tokio::test]
    async fn an_interrupted_recording_is_offered_under_the_name_it_was_given() {
        // KNEG-365: the name typed before Start is in the plan beside the WAV.
        let db = Db::open_in_memory().await.unwrap();
        let dir = tempfile::tempdir().unwrap();
        let named = dir.path().join("20261008-073000-a.wav");
        let unnamed = dir.path().join("20261008-063000-b.wav");
        std::fs::write(&named, b"wav").unwrap();
        std::fs::write(&unnamed, b"wav").unwrap();
        draft::write_plan(
            &named,
            &crate::meeting::MeetingPlan {
                title: Some("  Board review\n".into()),
                ..crate::meeting::MeetingPlan::default()
            },
        )
        .unwrap();

        let candidates = scan(&db, dir.path()).await.unwrap();
        let titles: Vec<_> = candidates
            .iter()
            .map(|c| (c.storage_key.as_str(), c.title.as_deref()))
            .collect();
        assert_eq!(
            titles,
            [
                ("20261008-073000-a.wav", Some("Board review")),
                ("20261008-063000-b.wav", None),
            ]
        );
    }
}
