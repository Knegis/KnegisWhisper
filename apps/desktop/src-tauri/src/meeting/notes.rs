//! Turning a finished meeting into the segments a note is made of.
//!
//! Lifted out of `meeting.rs` on 2026-09-01 (KNEG-89), which had reached 3,303
//! lines. Nothing here changed in the move — same functions, same tests, same
//! names — and the reason it is the first piece to leave is that it is the only
//! part of a meeting with no state at all: every function is a pure map from
//! what a pass produced to what the note holds, so a test needs no recorder, no
//! provider, no database and no `AppHandle`.
//!
//! `media.rs:1334-1340` is the argument for keeping it that way: nothing
//! outside a running application can construct a `tauri::AppHandle`, so logic
//! reachable only through one is untestable by construction.

use kw_audio_core::wav;
use kw_meeting_core::AudioSource;

/// Mono `f32` as the 16-bit PCM a provider takes.
///
/// The rate is only carried in the header this then strips, so it changes
/// nothing about the bytes — but passing it means the call site says what rate
/// the samples are, which is the thing that was wrong here before.
pub(super) fn pcm_bytes(samples: &[f32], sample_rate: u32) -> Vec<u8> {
    // Through the encoder and back out, so there is one clamp-and-scale in the
    // workspace rather than a second copy that could round differently.
    let wav = wav::encode_wav(samples, sample_rate);
    wav.get(44..).unwrap_or_default().to_vec()
}

/// One transcript segment in the shape `notes_core::transcript` already reads.
///
/// Reusing that shape means the existing export path — SRT, VTT, Markdown —
/// works on a meeting note with no further work.
pub(super) fn segment(
    text: &str,
    timestamp: f64,
    source: Option<AudioSource>,
) -> kw_notes_core::transcript::Segment {
    kw_notes_core::transcript::Segment {
        text: text.trim().to_owned(),
        timestamp: Some(timestamp),
        end_timestamp: None,
        speaker: None,
        speaker_name: None,
        speaker_is_placeholder: false,
        source: source.map(|s| match s {
            AudioSource::Mic => "mic".to_owned(),
            AudioSource::System => "system".to_owned(),
        }),
    }
}

/// What one pass over the finished recording produced.
///
/// `timings` is empty when the engine returned none -- a whisper server started
/// with `--no-timestamps` does exactly that -- which is a state the note has to
/// be able to describe rather than a failure.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct WholePass {
    pub(super) text: String,
    pub(super) timings: Vec<kw_transcription_core::local::TimedSegment>,
}

/// Turn one pass into note segments, with speaker labels where they were asked
/// for and could be produced.
///
/// Pure: the diarizer has already run, or has already been decided against, by
/// the time this is called. Everything it decides is testable with no process,
/// no database and no `AppHandle`, which is the split `media.rs:1334-1340`
/// argues for.
///
/// The second half of the answer is a sentence for the note, and it is only
/// ever present when the result differs from what the setting asked for.
/// Announcing that a feature worked is noise; not saying that it silently did
/// not is the defect.
pub(super) fn labelled(
    pass: &WholePass,
    turns: Option<&[kw_meeting_core::diarization::Turn]>,
    wanted: bool,
) -> (Vec<kw_notes_core::transcript::Segment>, Option<String>) {
    // No timings means nothing to attach a speaker to. One segment holding the
    // whole transcript, which is what this pass produced before timings were
    // kept at all.
    if pass.timings.is_empty() {
        let note = wanted.then(|| {
            "Speaker labels were asked for and not applied: the transcription pass returned no \
             timings, so there was nothing to attach a speaker to."
                .to_owned()
        });
        return (vec![segment(&pass.text, 0.0, None)], note);
    }

    let spans: Vec<(f64, f64)> = pass.timings.iter().map(|s| (s.start, s.end)).collect();
    let assigned = turns.map(|turns| kw_meeting_core::diarization::assign(turns, &spans));

    let mut segments = Vec::with_capacity(pass.timings.len());
    let mut unattributed = 0_usize;
    for (at, timed) in pass.timings.iter().enumerate() {
        let speaker = assigned
            .as_ref()
            .and_then(|all| all.get(at).copied().flatten());
        if assigned.is_some() && speaker.is_none() {
            unattributed = unattributed.saturating_add(1);
        }
        segments.push(kw_notes_core::transcript::Segment {
            text: timed.text.trim().to_owned(),
            timestamp: Some(timed.start),
            end_timestamp: Some(timed.end),
            speaker: speaker.map(|n| format!("speaker_{n}")),
            // A generated placeholder, and flagged as one so a name somebody
            // types later wins over it. `Speaker 1` is not a name and this is
            // the field that says so.
            speaker_name: speaker.map(|n| {
                kw_meeting_core::diarization::Turn {
                    start_seconds: timed.start,
                    end_seconds: timed.end,
                    speaker: n,
                }
                .label()
            }),
            speaker_is_placeholder: speaker.is_some(),
            source: None,
        });
    }

    let note = match (turns, wanted) {
        // Asked for, ran, and left some of the transcript unattributed. Said
        // out loud because a reader seeing most segments labelled and some not
        // would otherwise read the blanks as a person who said nothing.
        (Some(_), _) if unattributed > 0 => Some(format!(
            "{unattributed} of {} passages carry no speaker: the diarization pass found no \
             voice covering them.",
            segments.len()
        )),
        (Some(_), _) => None,
        // Asked for and nothing ran. The caller has already decided why; this
        // only reports that it did not happen.
        (None, true) => Some(
            "Speaker labels were asked for and not applied: the diarization step did not run. \
             Check Settings > Meetings."
                .to_owned(),
        ),
        (None, false) => None,
    };
    (segments, note)
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::float_cmp
)]
mod tests {
    use super::*;

    #[test]
    fn pcm_round_trips_through_the_one_encoder() {
        // A second clamp-and-scale would be a second thing to get wrong, so this
        // goes through `encode_wav` and strips the header. If that ever stops
        // producing 44 bytes of header, this catches it.
        let samples = vec![0.0_f32, 0.5, -0.5, 1.0, -1.0];
        let pcm = pcm_bytes(&samples, wav::TARGET_SAMPLE_RATE);
        assert_eq!(pcm.len(), samples.len() * 2);
        assert_eq!(wav::from_pcm(&pcm).len(), samples.len());

        let back = wav::from_pcm(&pcm);
        for (before, after) in samples.iter().zip(back.iter()) {
            assert!((before - after).abs() < 1e-3, "{before} became {after}");
        }
    }

    #[test]
    fn a_segment_carries_the_source_the_export_path_expects() {
        // `notes_core::transcript::Segment::source` is documented as `mic` or
        // `system`, so a meeting note exports without any further work.
        let mic = segment("hello", 1.5, Some(AudioSource::Mic));
        assert_eq!(mic.source.as_deref(), Some("mic"));
        assert_eq!(mic.timestamp, Some(1.5));

        let system = segment(" spaced  ", 0.0, Some(AudioSource::System));
        assert_eq!(system.source.as_deref(), Some("system"));
        assert_eq!(system.text, "spaced", "trimmed for the export");

        assert!(segment("x", 0.0, None).source.is_none());
    }

    /* ------------------------------------------------- speaker labels */

    fn timed(text: &str, start: f64, end: f64) -> kw_transcription_core::local::TimedSegment {
        kw_transcription_core::local::TimedSegment {
            text: text.to_owned(),
            start,
            end,
        }
    }

    fn turn(speaker: u32, start: f64, end: f64) -> kw_meeting_core::diarization::Turn {
        kw_meeting_core::diarization::Turn {
            start_seconds: start,
            end_seconds: end,
            speaker,
        }
    }

    fn two_utterances() -> WholePass {
        WholePass {
            text: "Morning all. Right, shall we start?".to_owned(),
            timings: vec![
                timed("Morning all.", 0.0, 2.0),
                timed("Right, shall we start?", 2.5, 5.0),
            ],
        }
    }

    #[test]
    fn a_pass_with_no_timings_is_still_one_readable_segment() {
        // A whisper server started with `--no-timestamps` produces this, and it
        // is the shape every meeting note had before timings were kept. It must
        // keep working, unlabelled, rather than becoming an error.
        let pass = WholePass {
            text: "  the whole thing  ".to_owned(),
            timings: Vec::new(),
        };
        let (segments, note) = labelled(&pass, None, false);
        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0].text, "the whole thing");
        assert_eq!(segments[0].speaker, None);
        assert_eq!(note, None);
    }

    #[test]
    fn asking_for_labels_with_no_timings_says_so_rather_than_going_quiet() {
        // The failure this whole `Option<String>` exists for: a user turns the
        // setting on, the pass returns no timings, and an unlabelled note reads
        // as a meeting with one speaker in it.
        let pass = WholePass {
            text: "the whole thing".to_owned(),
            timings: Vec::new(),
        };
        let (_, note) = labelled(&pass, None, true);
        let note = note.expect("a note is written when the request could not be honoured");
        assert!(note.contains("no timings"), "{note}");
    }

    #[test]
    fn timings_without_diarization_become_timed_segments_with_no_speaker() {
        let (segments, note) = labelled(&two_utterances(), None, false);
        assert_eq!(segments.len(), 2);
        assert_eq!(segments[0].timestamp, Some(0.0));
        assert_eq!(segments[0].end_timestamp, Some(2.0));
        assert!(segments.iter().all(|s| s.speaker.is_none()));
        assert!(segments.iter().all(|s| !s.speaker_is_placeholder));
        assert_eq!(note, None);
    }

    #[test]
    fn two_voices_land_on_the_passages_they_spoke() {
        let turns = [turn(0, 0.0, 2.2), turn(1, 2.4, 5.1)];
        let (segments, note) = labelled(&two_utterances(), Some(&turns), true);

        assert_eq!(segments[0].speaker.as_deref(), Some("speaker_0"));
        assert_eq!(segments[0].speaker_name.as_deref(), Some("Speaker 1"));
        assert_eq!(segments[1].speaker.as_deref(), Some("speaker_1"));
        assert_eq!(segments[1].speaker_name.as_deref(), Some("Speaker 2"));
        // Nothing to report: it was asked for and it worked.
        assert_eq!(note, None);
    }

    #[test]
    fn a_generated_label_is_flagged_so_a_typed_name_wins_over_it() {
        // `Speaker 1` is not a name. `speaker_is_placeholder` is the field that
        // says so, and without it a name somebody types into the note would
        // lose to a number the clusterer chose.
        let turns = [turn(0, 0.0, 2.2)];
        let (segments, _) = labelled(&two_utterances(), Some(&turns), true);
        assert!(segments[0].speaker_is_placeholder);
        // And an unattributed passage carries no placeholder at all, rather
        // than a placeholder saying "unknown".
        assert!(!segments[1].speaker_is_placeholder);
        assert_eq!(segments[1].speaker_name, None);
    }

    #[test]
    fn passages_no_voice_covers_are_counted_in_the_note() {
        // Half the transcript labelled and half not would otherwise read as a
        // participant who said nothing.
        let turns = [turn(0, 0.0, 2.2)];
        let (segments, note) = labelled(&two_utterances(), Some(&turns), true);
        assert_eq!(segments[1].speaker, None);
        let note = note.expect("an unattributed passage is reported");
        assert!(note.starts_with("1 of 2"), "{note}");
    }

    #[test]
    fn a_diarization_that_never_ran_is_not_the_same_as_one_that_found_nobody() {
        // `turns: None` with `wanted: true` means the step did not happen;
        // `turns: Some(&[])` means it ran and attributed nothing. They produce
        // the same segments and must not produce the same sentence.
        let (_, never_ran) = labelled(&two_utterances(), None, true);
        let (_, found_nobody) = labelled(&two_utterances(), Some(&[]), true);

        let never_ran = never_ran.expect("a step that did not run is reported");
        let found_nobody = found_nobody.expect("a step that attributed nothing is reported");
        assert!(never_ran.contains("did not run"), "{never_ran}");
        assert!(found_nobody.starts_with("2 of 2"), "{found_nobody}");
        assert_ne!(never_ran, found_nobody);
    }

    #[test]
    fn a_note_is_never_written_when_nothing_was_asked_for() {
        // The rule the other way round: a person who has this switched off
        // must not find sentences about diarization in their notes.
        for pass in [
            two_utterances(),
            WholePass {
                text: "x".to_owned(),
                timings: Vec::new(),
            },
        ] {
            assert_eq!(labelled(&pass, None, false).1, None);
        }
    }
}
