//! Shared note and transcript rendering without a file dialog.
use crate::{notes::Note, transcript};
use serde::Serialize;
#[derive(Debug, Serialize)]
pub struct Document {
    pub filename: String,
    pub content: String,
    pub segment_count: usize,
}
pub fn filename(title: &str, extension: &str) -> String {
    let stem = title
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    let stem = if stem.is_empty() { "transcript" } else { &stem };
    format!(
        "{}.{}",
        stem.chars().take(80).collect::<String>(),
        extension
    )
}
pub fn render(
    note: &Note,
    format: transcript::Format,
    timed: bool,
) -> Result<Document, serde_json::Error> {
    if !timed {
        return Ok(Document {
            filename: filename(&note.title, "md"),
            content: format!(
                "# {}\n\n{}\n",
                note.title.replace(['\r', '\n'], " "),
                note.content
            ),
            segment_count: 0,
        });
    }
    let segments: Vec<transcript::Segment> = match note.transcript.as_deref() {
        Some(raw) if !raw.trim().is_empty() => serde_json::from_str(raw)?,
        _ => Vec::new(),
    };
    Ok(Document {
        filename: filename(&note.title, format.extension()),
        content: transcript::export(
            format,
            &transcript::NoteMeta {
                title: note.title.clone(),
                created_at: note.created_at.clone(),
                participants: note.participants.clone(),
            },
            &segments,
            &Default::default(),
            &transcript::SpeakerLabels::default(),
        ),
        segment_count: segments.len(),
    })
}
