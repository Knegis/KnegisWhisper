//! Transcript export: plain text, Markdown, JSON, SRT, and WebVTT.
//!
//! Ported from upstream's `transcriptFormatter.js`, which has more judgement in
//! it than its size suggests. Four behaviours are worth naming, because each one
//! is the difference between a usable export and an irritating one.
//!
//! **Consecutive segments from the same speaker are merged** when they are less
//! than two seconds apart. Streaming providers emit a segment per utterance, so
//! without this a paragraph of speech becomes forty one-line entries.
//!
//! **The merge key covers every field the speaker name is derived from.** A
//! segment the user has manually named must not absorb the unnamed segment next
//! to it, which is what a key of "speaker id only" would do.
//!
//! **Timestamps are normalised.** Meeting segments carry epoch milliseconds;
//! subtitle formats need seconds from the start of the recording. The tell is a
//! value above 10^9, which no plausible recording duration reaches.
//!
//! **Anonymous segments omit speaker labels in every format.** An uploaded
//! audio file has no diarisation, so prefixing every cue with "Unknown
//! speaker:" adds no information. Identity is structural rather than a
//! comparison with the localised placeholder: a typed name, diarisation label,
//! or known capture source is retained; a segment carrying none gets no label.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use ts_rs::TS;

/// One piece of transcribed speech.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Segment {
    pub text: String,
    /// Seconds from the start of the recording, or epoch milliseconds — see
    /// [`normalise_timestamps`].
    pub timestamp: Option<f64>,
    pub end_timestamp: Option<f64>,
    /// A diarisation label such as `speaker_0`, or `you`.
    pub speaker: Option<String>,
    /// A name the user typed. Wins over the label.
    pub speaker_name: Option<String>,
    /// True when `speaker_name` is a generated placeholder rather than a real
    /// name, so it must not win.
    pub speaker_is_placeholder: bool,
    /// `mic` or `system`, for meetings with no diarisation.
    pub source: Option<String>,
}

/// The words used for speakers with no name.
///
/// Passed in rather than hardcoded so the caller owns localisation; this crate
/// should not reach for an i18n table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpeakerLabels {
    pub you: String,
    pub others: String,
    pub unknown: String,
    /// A format for a numbered speaker, with `{n}` replaced.
    pub numbered: String,
}

impl Default for SpeakerLabels {
    fn default() -> Self {
        Self {
            you: "You".to_owned(),
            others: "Others".to_owned(),
            unknown: "Unknown speaker".to_owned(),
            numbered: "Speaker {n}".to_owned(),
        }
    }
}

/// The note a transcript belongs to.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NoteMeta {
    pub title: String,
    /// As stored: SQLite `CURRENT_TIMESTAMP` is UTC with no suffix.
    pub created_at: Option<String>,
    /// A JSON array of `{displayName, email}`, as stored.
    pub participants: Option<String>,
}

/// Which format to emit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
pub enum Format {
    Txt,
    Md,
    Json,
    Srt,
    Vtt,
}

impl Format {
    #[must_use]
    pub const fn extension(self) -> &'static str {
        match self {
            Self::Txt => "txt",
            Self::Md => "md",
            Self::Json => "json",
            Self::Srt => "srt",
            Self::Vtt => "vtt",
        }
    }

    /// Every format, so a picker cannot miss one.
    #[must_use]
    pub const fn all() -> [Self; 5] {
        [Self::Txt, Self::Md, Self::Json, Self::Srt, Self::Vtt]
    }
}

/// Resolve a segment's display name.
///
/// Order matters: a name the user typed beats a diarisation label, which beats
/// the capture source, which beats "unknown".
#[must_use]
pub fn speaker_name(
    segment: &Segment,
    mappings: &BTreeMap<String, String>,
    labels: &SpeakerLabels,
) -> String {
    if let Some(name) = segment.speaker_name.as_deref() {
        if !segment.speaker_is_placeholder && !name.trim().is_empty() {
            return name.to_owned();
        }
    }

    if let Some(speaker) = segment.speaker.as_deref() {
        if let Some(mapped) = mappings.get(speaker) {
            return mapped.clone();
        }
        if speaker == "you" {
            return labels.you.clone();
        }
        // `speaker_0` is the first speaker, so show it as 1.
        if let Some(index) = speaker
            .strip_prefix("speaker_")
            .and_then(|n| n.parse::<u32>().ok())
        {
            return labels.numbered.replace("{n}", &(index + 1).to_string());
        }
        return speaker.to_owned();
    }

    match segment.source.as_deref() {
        Some("mic") => labels.you.clone(),
        Some("system") => labels.others.clone(),
        _ => labels.unknown.clone(),
    }
}

/// The key that decides whether two segments may merge.
///
/// Covers every field [`speaker_name`] reads. A key of "speaker id only" would
/// let a manually named segment absorb the unnamed one beside it, silently
/// attributing words to the wrong person.
fn merge_key(segment: &Segment) -> String {
    let named = match segment.speaker_name.as_deref() {
        Some(name) if !segment.speaker_is_placeholder => name,
        _ => "",
    };
    let source = if segment.speaker.is_some() {
        ""
    } else {
        segment.source.as_deref().unwrap_or("")
    };

    // NUL separated: no speaker name or source can contain one, so distinct
    // triples cannot collide.
    format!(
        "{}\u{0}{}\u{0}{}",
        segment.speaker.as_deref().unwrap_or(""),
        named,
        source
    )
}

/// How close two segments must be to merge, in seconds.
const MERGE_GAP_SECONDS: f64 = 2.0;

/// A timestamp above this is epoch milliseconds, not seconds into a recording.
///
/// 10^9 seconds is over thirty years, so no recording reaches it.
const EPOCH_THRESHOLD: f64 = 1e9;

/// Convert epoch-millisecond timestamps to seconds from the recording's start.
///
/// Left alone when the timestamps are already relative, so calling this twice is
/// harmless.
#[must_use]
pub fn normalise_timestamps(segments: &[Segment], note: &NoteMeta) -> Vec<Segment> {
    let start_ms = recording_start_ms(segments, note);

    let Some(start_ms) = start_ms.filter(|ms| *ms > EPOCH_THRESHOLD) else {
        return segments.to_vec();
    };

    segments
        .iter()
        .map(|segment| Segment {
            timestamp: segment.timestamp.map(|t| (t - start_ms) / 1000.0),
            end_timestamp: segment.end_timestamp.map(|t| (t - start_ms) / 1000.0),
            ..segment.clone()
        })
        .collect()
}

/// When the recording started, in whatever unit the segments use.
fn recording_start_ms(segments: &[Segment], note: &NoteMeta) -> Option<f64> {
    let earliest = segments
        .iter()
        .filter_map(|s| s.timestamp)
        .filter(|t| t.is_finite())
        .fold(None::<f64>, |acc, t| Some(acc.map_or(t, |a| a.min(t))));

    if earliest.is_some() {
        return earliest;
    }

    // No segment timestamps: fall back to the note's creation time.
    parse_created_at(note.created_at.as_deref()?).map(|dt| dt.timestamp_millis() as f64)
}

/// Parse a stored timestamp.
///
/// SQLite's `CURRENT_TIMESTAMP` is UTC with no timezone suffix, so a naive value
/// is read as UTC rather than local time. Getting that wrong shifts every
/// exported timestamp by the machine's offset.
#[must_use]
pub fn parse_created_at(value: &str) -> Option<chrono::DateTime<chrono::Utc>> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }

    // An explicit offset or Z: trust it.
    if let Ok(parsed) = chrono::DateTime::parse_from_rfc3339(trimmed) {
        return Some(parsed.with_timezone(&chrono::Utc));
    }

    // Naive: SQLite wrote it, so it is UTC.
    let naive = chrono::NaiveDateTime::parse_from_str(trimmed, "%Y-%m-%d %H:%M:%S")
        .or_else(|_| chrono::NaiveDateTime::parse_from_str(trimmed, "%Y-%m-%dT%H:%M:%S"))
        .ok()?;

    Some(naive.and_utc())
}

/// Merge consecutive segments from the same speaker.
#[must_use]
pub fn merge(segments: &[Segment]) -> Vec<Segment> {
    let mut merged: Vec<Segment> = Vec::new();
    let mut last_timestamp: Option<f64> = None;

    for segment in segments {
        let text = segment.text.trim();
        if text.is_empty() {
            continue;
        }

        let timestamp = segment.timestamp.unwrap_or(0.0);

        let can_merge = merged.last().is_some_and(|last| {
            merge_key(last) == merge_key(segment)
                && last_timestamp.is_some_and(|previous| timestamp - previous < MERGE_GAP_SECONDS)
        });

        if can_merge {
            if let Some(last) = merged.last_mut() {
                last.text.push(' ');
                last.text.push_str(text);
                last.end_timestamp = Some(timestamp);
            }
        } else {
            merged.push(Segment {
                text: text.to_owned(),
                timestamp: Some(timestamp),
                end_timestamp: Some(timestamp),
                ..segment.clone()
            });
        }

        last_timestamp = Some(timestamp);
    }

    merged
}

/// `HH:MM:SS`.
#[must_use]
pub fn format_clock(seconds: f64) -> String {
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let total = seconds.max(0.0).floor() as u64;
    format!(
        "{:02}:{:02}:{:02}",
        total / 3600,
        (total % 3600) / 60,
        total % 60
    )
}

/// `HH:MM:SS,mmm` for SRT, or `HH:MM:SS.mmm` for WebVTT.
#[must_use]
pub fn format_cue(seconds: f64, decimal: char) -> String {
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let total_ms = (seconds.max(0.0) * 1000.0).round() as u64;
    let ms = total_ms % 1000;
    let total = total_ms / 1000;
    format!(
        "{:02}:{:02}:{:02}{decimal}{:03}",
        total / 3600,
        (total % 3600) / 60,
        total % 60,
        ms
    )
}

/// Participant display names, from the stored JSON.
#[must_use]
pub fn participants(note: &NoteMeta) -> Vec<String> {
    let Some(raw) = note.participants.as_deref() else {
        return Vec::new();
    };

    let Ok(parsed) = serde_json::from_str::<Vec<serde_json::Value>>(raw) else {
        // Malformed participant data is not worth failing an export over.
        return Vec::new();
    };

    parsed
        .iter()
        .filter_map(|p| {
            p.get("displayName")
                .and_then(serde_json::Value::as_str)
                .or_else(|| p.get("email").and_then(serde_json::Value::as_str))
        })
        .filter(|name| !name.trim().is_empty())
        .map(str::to_owned)
        .collect()
}

/// A human-readable date for a header line.
#[must_use]
pub fn format_date(note: &NoteMeta) -> String {
    parse_created_at(note.created_at.as_deref().unwrap_or_default())
        .map_or_else(String::new, |dt| {
            dt.format("%-d %B %Y %H:%M UTC").to_string()
        })
}

/// The speaker to render, only when the segment carries a real identity.
///
/// Do not decide this by comparing [`speaker_name`] with `labels.unknown`: that
/// word is localised, and a user is allowed to type the same words as a name.
/// The stored fields already say whether identity exists. A generated
/// placeholder is not one, and only the two documented capture sources are.
fn identified_speaker(
    segment: &Segment,
    mappings: &BTreeMap<String, String>,
    labels: &SpeakerLabels,
) -> Option<String> {
    let typed = segment
        .speaker_name
        .as_deref()
        .is_some_and(|name| !segment.speaker_is_placeholder && !name.trim().is_empty());
    let diarized = segment
        .speaker
        .as_deref()
        .is_some_and(|speaker| !speaker.trim().is_empty());
    let sourced = matches!(segment.source.as_deref(), Some("mic" | "system"));

    (typed || diarized || sourced)
        .then(|| speaker_name(segment, mappings, labels))
        .filter(|name| !name.trim().is_empty())
}

/// How long an unterminated final cue lasts, in seconds.
const TRAILING_CUE_SECONDS: f64 = 3.0;

/// Export a transcript.
#[must_use]
pub fn export(
    format: Format,
    note: &NoteMeta,
    segments: &[Segment],
    mappings: &BTreeMap<String, String>,
    labels: &SpeakerLabels,
) -> String {
    let normalised = normalise_timestamps(segments, note);
    let merged = merge(&normalised);

    match format {
        Format::Txt => export_txt(note, &merged, mappings, labels),
        Format::Md => export_md(note, &merged, mappings, labels),
        Format::Json => export_json(note, &merged, mappings, labels),
        Format::Srt => export_cues(&merged, mappings, labels, CueStyle::Srt),
        Format::Vtt => export_cues(&merged, mappings, labels, CueStyle::Vtt),
    }
}

fn export_txt(
    note: &NoteMeta,
    merged: &[Segment],
    mappings: &BTreeMap<String, String>,
    labels: &SpeakerLabels,
) -> String {
    let mut lines = vec![note.title.clone(), format_date(note)];

    let people = participants(note);
    if !people.is_empty() {
        lines.push(format!("Participants: {}", people.join(", ")));
    }

    lines.push(String::new());
    lines.push("─".repeat(34));
    lines.push(String::new());

    for segment in merged {
        let clock = format_clock(segment.timestamp.unwrap_or(0.0));
        lines.push(identified_speaker(segment, mappings, labels).map_or_else(
            || format!("[{clock}]"),
            |speaker| format!("[{clock}] {speaker}:"),
        ));
        lines.push(segment.text.clone());
        lines.push(String::new());
    }

    lines.join("\n")
}

fn export_md(
    note: &NoteMeta,
    merged: &[Segment],
    mappings: &BTreeMap<String, String>,
    labels: &SpeakerLabels,
) -> String {
    let mut lines = vec![
        format!("# {}", note.title),
        String::new(),
        format!("**Date:** {}", format_date(note)),
    ];

    let people = participants(note);
    if !people.is_empty() {
        lines.push(format!("**Participants:** {}", people.join(", ")));
    }

    lines.push(String::new());
    lines.push("---".to_owned());
    lines.push(String::new());

    for segment in merged {
        let clock = format_clock(segment.timestamp.unwrap_or(0.0));
        lines.push(identified_speaker(segment, mappings, labels).map_or_else(
            || format!("`{clock}`"),
            |speaker| format!("**{speaker}** `{clock}`"),
        ));
        lines.push(segment.text.clone());
        lines.push(String::new());
    }

    lines.join("\n")
}

fn export_json(
    note: &NoteMeta,
    merged: &[Segment],
    mappings: &BTreeMap<String, String>,
    labels: &SpeakerLabels,
) -> String {
    let mut speakers: Vec<String> = Vec::new();
    for segment in merged {
        if let Some(name) = identified_speaker(segment, mappings, labels) {
            if !speakers.contains(&name) {
                speakers.push(name);
            }
        }
    }

    let duration = merged
        .last()
        .and_then(|s| s.end_timestamp)
        .unwrap_or(0.0)
        .round();

    let document = serde_json::json!({
        "metadata": {
            "title": note.title,
            "date": format_date(note),
            "durationSeconds": duration,
            "speakerCount": speakers.len(),
            "segmentCount": merged.len(),
        },
        "speakers": speakers,
        "segments": merged.iter().map(|segment| {
            let speaker = identified_speaker(segment, mappings, labels);
            serde_json::json!({
                "speaker": speaker,
                "timestamp": segment.timestamp.unwrap_or(0.0),
                "text": segment.text,
            })
        }).collect::<Vec<_>>(),
    });

    // Pretty-printed: an exported transcript is read by people.
    serde_json::to_string_pretty(&document).unwrap_or_else(|_| "{}".to_owned())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CueStyle {
    Srt,
    Vtt,
}

fn export_cues(
    merged: &[Segment],
    mappings: &BTreeMap<String, String>,
    labels: &SpeakerLabels,
    style: CueStyle,
) -> String {
    let decimal = if style == CueStyle::Srt { ',' } else { '.' };
    let mut out = Vec::new();
    if style == CueStyle::Vtt {
        out.push("WEBVTT".to_owned());
        out.push(String::new());
    }

    for (index, segment) in merged.iter().enumerate() {
        let start = segment.timestamp.unwrap_or(0.0);
        // A cue runs until the next one starts; the last gets a fixed tail.
        let end = merged
            .get(index + 1)
            .and_then(|next| next.timestamp)
            .unwrap_or_else(|| segment.end_timestamp.unwrap_or(start) + TRAILING_CUE_SECONDS);

        out.push((index + 1).to_string());
        out.push(format!(
            "{} --> {}",
            format_cue(start, decimal),
            format_cue(end, decimal)
        ));
        out.push(identified_speaker(segment, mappings, labels).map_or_else(
            || segment.text.clone(),
            |speaker| format!("{speaker}: {}", segment.text),
        ));
        out.push(String::new());
    }

    out.join("\n")
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

    fn labels() -> SpeakerLabels {
        SpeakerLabels::default()
    }

    fn no_mappings() -> BTreeMap<String, String> {
        BTreeMap::new()
    }

    fn note() -> NoteMeta {
        NoteMeta {
            title: "Quarterly review".to_owned(),
            created_at: Some("2026-08-25 09:30:00".to_owned()),
            participants: None,
        }
    }

    fn segment(text: &str, at: f64, speaker: Option<&str>) -> Segment {
        Segment {
            text: text.to_owned(),
            timestamp: Some(at),
            end_timestamp: Some(at),
            speaker: speaker.map(str::to_owned),
            ..Default::default()
        }
    }

    /* ------------------------------------------------------------- merging */

    #[test]
    fn consecutive_segments_from_one_speaker_merge() {
        // A streaming provider emits one segment per utterance, so without this
        // a paragraph becomes forty one-line entries.
        let segments = vec![
            segment("Good morning", 0.0, Some("speaker_0")),
            segment("everyone", 0.5, Some("speaker_0")),
            segment("shall we start", 1.2, Some("speaker_0")),
        ];
        let merged = merge(&segments);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].text, "Good morning everyone shall we start");
        assert_eq!(merged[0].end_timestamp, Some(1.2));
    }

    #[test]
    fn a_gap_of_two_seconds_or_more_starts_a_new_segment() {
        let segments = vec![
            segment("First point", 0.0, Some("speaker_0")),
            segment("Second point", 2.0, Some("speaker_0")),
        ];
        assert_eq!(merge(&segments).len(), 2);
    }

    #[test]
    fn different_speakers_never_merge() {
        let segments = vec![
            segment("Yes", 0.0, Some("speaker_0")),
            segment("No", 0.1, Some("speaker_1")),
        ];
        assert_eq!(merge(&segments).len(), 2);
    }

    #[test]
    fn a_named_segment_does_not_absorb_an_unnamed_one() {
        // The reason the merge key covers every field the speaker name is
        // derived from: otherwise words get attributed to the wrong person.
        let named = Segment {
            speaker_name: Some("Ada".to_owned()),
            ..segment("I agree", 0.0, Some("speaker_0"))
        };
        let unnamed = segment("and so do I", 0.3, Some("speaker_0"));

        let merged = merge(&[named, unnamed]);
        assert_eq!(merged.len(), 2, "a named segment absorbed an unnamed one");
    }

    #[test]
    fn a_placeholder_name_does_not_block_merging() {
        // A generated placeholder is not a real name, so it must not split what
        // would otherwise be one utterance.
        let a = Segment {
            speaker_name: Some("Speaker 1".to_owned()),
            speaker_is_placeholder: true,
            ..segment("Half a", 0.0, Some("speaker_0"))
        };
        let b = Segment {
            speaker_name: Some("Speaker 1".to_owned()),
            speaker_is_placeholder: true,
            ..segment("sentence", 0.3, Some("speaker_0"))
        };
        assert_eq!(merge(&[a, b]).len(), 1);
    }

    #[test]
    fn empty_segments_are_dropped() {
        let segments = vec![
            segment("Real", 0.0, Some("speaker_0")),
            segment("   ", 0.5, Some("speaker_0")),
            segment("", 0.7, Some("speaker_0")),
        ];
        let merged = merge(&segments);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].text, "Real");
    }

    /* ---------------------------------------------------------- timestamps */

    #[test]
    fn epoch_millisecond_timestamps_become_seconds_from_the_start() {
        // Meeting segments carry epoch ms; subtitles need relative seconds.
        let base = 1_756_000_000_000.0;
        let segments = vec![
            segment("start", base, Some("speaker_0")),
            segment("later", base + 5_000.0, Some("speaker_0")),
        ];
        let normalised = normalise_timestamps(&segments, &note());

        assert_eq!(normalised[0].timestamp, Some(0.0));
        assert_eq!(normalised[1].timestamp, Some(5.0));
    }

    #[test]
    fn already_relative_timestamps_are_left_alone() {
        let segments = vec![segment("x", 12.5, None), segment("y", 30.0, None)];
        let normalised = normalise_timestamps(&segments, &note());
        assert_eq!(normalised[0].timestamp, Some(12.5));
        assert_eq!(normalised[1].timestamp, Some(30.0));
    }

    #[test]
    fn normalising_twice_is_harmless() {
        let base = 1_756_000_000_000.0;
        let segments = vec![segment("x", base, None)];
        let once = normalise_timestamps(&segments, &note());
        let twice = normalise_timestamps(&once, &note());
        assert_eq!(once, twice);
    }

    #[test]
    fn a_naive_stored_timestamp_is_read_as_utc_not_local() {
        // SQLite CURRENT_TIMESTAMP has no suffix. Reading it as local time
        // shifts every exported timestamp by the machine's offset.
        let parsed = parse_created_at("2026-08-25 09:30:00").unwrap();
        assert_eq!(parsed.to_rfc3339(), "2026-08-25T09:30:00+00:00");

        // An explicit offset is trusted.
        let offset = parse_created_at("2026-08-25T09:30:00+02:00").unwrap();
        assert_eq!(offset.to_rfc3339(), "2026-08-25T07:30:00+00:00");
    }

    #[test]
    fn an_unparseable_date_does_not_fail_the_export() {
        assert_eq!(parse_created_at(""), None);
        assert_eq!(parse_created_at("not a date"), None);
        let bad = NoteMeta {
            created_at: Some("nonsense".to_owned()),
            ..note()
        };
        assert_eq!(format_date(&bad), "");
        // And a full export still works.
        assert!(!export(
            Format::Txt,
            &bad,
            &[segment("x", 0.0, None)],
            &no_mappings(),
            &labels()
        )
        .is_empty());
    }

    /* ------------------------------------------------------ speaker naming */

    #[test]
    fn speaker_zero_is_displayed_as_speaker_one() {
        let s = segment("x", 0.0, Some("speaker_0"));
        assert_eq!(speaker_name(&s, &no_mappings(), &labels()), "Speaker 1");
        let s = segment("x", 0.0, Some("speaker_3"));
        assert_eq!(speaker_name(&s, &no_mappings(), &labels()), "Speaker 4");
    }

    #[test]
    fn a_mapped_name_beats_the_label_and_a_typed_name_beats_everything() {
        let mut mappings = BTreeMap::new();
        mappings.insert("speaker_0".to_owned(), "Ada".to_owned());

        let plain = segment("x", 0.0, Some("speaker_0"));
        assert_eq!(speaker_name(&plain, &mappings, &labels()), "Ada");

        let typed = Segment {
            speaker_name: Some("Grace".to_owned()),
            ..plain.clone()
        };
        assert_eq!(speaker_name(&typed, &mappings, &labels()), "Grace");

        let placeholder = Segment {
            speaker_name: Some("Speaker 1".to_owned()),
            speaker_is_placeholder: true,
            ..plain
        };
        assert_eq!(
            speaker_name(&placeholder, &mappings, &labels()),
            "Ada",
            "a placeholder must not beat a real mapping"
        );
    }

    #[test]
    fn the_capture_source_names_a_speaker_when_nothing_else_does() {
        let mic = Segment {
            source: Some("mic".to_owned()),
            ..segment("x", 0.0, None)
        };
        assert_eq!(speaker_name(&mic, &no_mappings(), &labels()), "You");

        let system = Segment {
            source: Some("system".to_owned()),
            ..segment("x", 0.0, None)
        };
        assert_eq!(speaker_name(&system, &no_mappings(), &labels()), "Others");

        let nothing = segment("x", 0.0, None);
        assert_eq!(
            speaker_name(&nothing, &no_mappings(), &labels()),
            "Unknown speaker"
        );
    }

    /* ------------------------------------------------------------- formats */

    #[test]
    fn srt_cues_run_until_the_next_one_starts() {
        let segments = vec![
            segment("First", 0.0, Some("speaker_0")),
            segment("Second", 10.0, Some("speaker_1")),
        ];
        let srt = export(Format::Srt, &note(), &segments, &no_mappings(), &labels());

        assert!(srt.contains("00:00:00,000 --> 00:00:10,000"), "{srt}");
        // The last cue gets a fixed tail.
        assert!(srt.contains("00:00:10,000 --> 00:00:13,000"), "{srt}");
        assert!(srt.starts_with("1\n"));
    }

    #[test]
    fn subtitles_omit_speaker_labels_when_there_is_no_speaker_identity() {
        // An uploaded file has no diarisation; prefixing every cue with
        // "Unknown speaker:" defeats the point of exporting subtitles.
        let anonymous = vec![segment("Just the words", 0.0, None)];
        let srt = export(Format::Srt, &note(), &anonymous, &no_mappings(), &labels());
        assert!(!srt.contains("Unknown speaker"), "{srt}");
        assert!(srt.contains("Just the words"));

        // With identity, labels come back.
        let identified = vec![segment("Attributed", 0.0, Some("speaker_0"))];
        let srt = export(Format::Srt, &note(), &identified, &no_mappings(), &labels());
        assert!(srt.contains("Speaker 1: Attributed"), "{srt}");
    }

    #[test]
    fn anonymous_segments_never_repeat_the_unknown_placeholder() {
        let anonymous = vec![
            segment("First anonymous turn", 0.0, None),
            segment("Second anonymous turn", 10.0, None),
        ];

        for format in [Format::Txt, Format::Md, Format::Srt, Format::Vtt] {
            let out = export(format, &note(), &anonymous, &no_mappings(), &labels());
            assert!(!out.contains("Unknown speaker"), "{format:?}: {out}");
            assert!(out.contains("First anonymous turn"), "{format:?}: {out}");
            assert!(out.contains("Second anonymous turn"), "{format:?}: {out}");
        }

        let json = export(Format::Json, &note(), &anonymous, &no_mappings(), &labels());
        assert!(!json.contains("Unknown speaker"), "{json}");
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["metadata"]["speakerCount"], 0);
        assert_eq!(parsed["metadata"]["segmentCount"], 2);
        assert!(parsed["speakers"].as_array().unwrap().is_empty());
        assert!(parsed["segments"][0]["speaker"].is_null());
        assert!(parsed["segments"][1]["speaker"].is_null());
    }

    #[test]
    fn mixed_transcripts_keep_real_identities_and_leave_only_anonymous_turns_unlabelled() {
        let segments = vec![
            segment("Diarized", 0.0, Some("speaker_0")),
            segment("Anonymous", 10.0, None),
            Segment {
                source: Some("mic".to_owned()),
                ..segment("From the microphone", 20.0, None)
            },
            Segment {
                speaker_name: Some("Grace".to_owned()),
                ..segment("Named by a person", 30.0, None)
            },
        ];

        for format in [Format::Txt, Format::Md, Format::Srt, Format::Vtt] {
            let out = export(format, &note(), &segments, &no_mappings(), &labels());
            assert!(out.contains("Speaker 1"), "{format:?}: {out}");
            assert!(out.contains("You"), "{format:?}: {out}");
            assert!(out.contains("Grace"), "{format:?}: {out}");
            assert!(!out.contains("Unknown speaker"), "{format:?}: {out}");
            assert!(out.contains("Anonymous"), "{format:?}: {out}");
        }

        let json = export(Format::Json, &note(), &segments, &no_mappings(), &labels());
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["metadata"]["speakerCount"], 3);
        assert_eq!(parsed["metadata"]["segmentCount"], 4);
        assert_eq!(parsed["speakers"].as_array().unwrap().len(), 3);
        assert_eq!(parsed["segments"][0]["speaker"], "Speaker 1");
        assert!(parsed["segments"][1]["speaker"].is_null());
        assert_eq!(parsed["segments"][2]["speaker"], "You");
        assert_eq!(parsed["segments"][3]["speaker"], "Grace");
    }

    #[test]
    fn vtt_has_the_required_header_and_a_dot_decimal_separator() {
        let segments = vec![segment("Hello", 1.5, Some("speaker_0"))];
        let vtt = export(Format::Vtt, &note(), &segments, &no_mappings(), &labels());

        assert!(
            vtt.starts_with("WEBVTT\n"),
            "a VTT file must start with WEBVTT"
        );
        assert!(vtt.contains("00:00:01.500 -->"), "{vtt}");
        assert!(!vtt.contains(",500"), "VTT uses a dot, not a comma");
    }

    #[test]
    fn srt_uses_a_comma_decimal_separator() {
        let segments = vec![segment("Hello", 1.5, Some("speaker_0"))];
        let srt = export(Format::Srt, &note(), &segments, &no_mappings(), &labels());
        assert!(srt.contains("00:00:01,500 -->"), "{srt}");
        assert!(!srt.starts_with("WEBVTT"));
    }

    #[test]
    fn json_reports_counts_that_match_its_own_contents() {
        let segments = vec![
            segment("One", 0.0, Some("speaker_0")),
            segment("Two", 10.0, Some("speaker_1")),
            segment("Three", 20.0, Some("speaker_0")),
        ];
        let json = export(Format::Json, &note(), &segments, &no_mappings(), &labels());
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed["metadata"]["segmentCount"], 3);
        assert_eq!(parsed["metadata"]["speakerCount"], 2);
        assert_eq!(parsed["speakers"].as_array().unwrap().len(), 2);
        assert_eq!(parsed["segments"].as_array().unwrap().len(), 3);
        assert_eq!(parsed["metadata"]["title"], "Quarterly review");
    }

    #[test]
    fn markdown_and_text_carry_the_title_date_and_participants() {
        let with_people = NoteMeta {
            participants: Some(
                r#"[{"displayName":"Ada Lovelace"},{"email":"grace@example.com"}]"#.to_owned(),
            ),
            ..note()
        };
        let segments = vec![segment("Hello", 0.0, Some("speaker_0"))];

        for format in [Format::Txt, Format::Md] {
            let out = export(format, &with_people, &segments, &no_mappings(), &labels());
            assert!(out.contains("Quarterly review"), "{format:?}: {out}");
            assert!(out.contains("25 August 2026"), "{format:?}: {out}");
            assert!(out.contains("Ada Lovelace"), "{format:?}: {out}");
            assert!(
                out.contains("grace@example.com"),
                "an email should stand in for a missing display name"
            );
            assert!(out.contains("Speaker 1"), "{format:?}");
        }
    }

    #[test]
    fn malformed_participant_data_does_not_fail_an_export() {
        let broken = NoteMeta {
            participants: Some("not json".to_owned()),
            ..note()
        };
        assert!(participants(&broken).is_empty());
        assert!(!export(
            Format::Md,
            &broken,
            &[segment("x", 0.0, None)],
            &no_mappings(),
            &labels()
        )
        .is_empty());
    }

    #[test]
    fn every_format_produces_output_for_an_empty_transcript() {
        // Exporting a note with no segments should give a header, not a panic.
        for format in Format::all() {
            let out = export(format, &note(), &[], &no_mappings(), &labels());
            match format {
                Format::Srt => assert!(out.is_empty(), "an empty SRT is empty"),
                Format::Vtt => assert!(out.starts_with("WEBVTT")),
                _ => assert!(!out.is_empty(), "{format:?} produced nothing"),
            }
        }
    }

    #[test]
    fn extensions_are_distinct_and_match_the_format() {
        let extensions: Vec<&str> = Format::all().iter().map(|f| f.extension()).collect();
        assert_eq!(extensions, vec!["txt", "md", "json", "srt", "vtt"]);
    }

    /* ------------------------------------------------------------- clocks */

    #[test]
    fn clock_formatting_handles_hours_and_rounding() {
        assert_eq!(format_clock(0.0), "00:00:00");
        assert_eq!(format_clock(59.9), "00:00:59");
        assert_eq!(format_clock(60.0), "00:01:00");
        assert_eq!(format_clock(3_661.0), "01:01:01");
        assert_eq!(
            format_clock(-5.0),
            "00:00:00",
            "a negative time must not wrap"
        );
    }

    #[test]
    fn cue_formatting_keeps_milliseconds() {
        assert_eq!(format_cue(0.0, ','), "00:00:00,000");
        assert_eq!(format_cue(1.5, ','), "00:00:01,500");
        assert_eq!(
            format_cue(1.0005, ','),
            "00:00:01,001",
            "rounds to the nearest ms"
        );
        assert_eq!(format_cue(3_661.25, '.'), "01:01:01.250");
        assert_eq!(format_cue(-1.0, ','), "00:00:00,000");
    }
}
