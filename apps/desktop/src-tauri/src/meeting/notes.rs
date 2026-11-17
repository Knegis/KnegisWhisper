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
use kw_meeting_core::{AudioSource, Client, MeetingTranscript};
use kw_network_policy::NetworkPolicy;
use kw_transcription_core::local::WhisperServer;

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
        // A live caption is timed as a whole, never word by word.
        words: Vec::new(),
    }
}

#[derive(Debug)]
pub(super) struct SavedTranscript {
    pub(super) segments: Vec<kw_notes_core::transcript::Segment>,
    pub(super) speaker_note: Option<String>,
    pub(super) from_partials: bool,
    pub(super) used_streaming: bool,
}

/// Only the windowed provider needs its overlapping previews replaced. A
/// successful local streaming session already holds the transcript to save. Keep the
/// offline future lazy so choosing streaming never reads or transcribes the WAV.
pub(super) async fn select_transcript(
    provider_id: &str,
    transcript: &MeetingTranscript,
    live_failure: Option<&str>,
    has_audio: bool,
    offline: impl std::future::Future<
        Output = Result<(Vec<kw_notes_core::transcript::Segment>, Option<String>), String>,
    >,
) -> SavedTranscript {
    let streaming = match kw_meeting_core::client(provider_id) {
        Some(Client::LocalStreaming) => true,
        // These clients currently close their readers without waiting for the
        // last utterance. Keep their whole-file pass until they have a verified
        // finish handshake; otherwise switching to finals would lose the tail.
        Some(
            Client::LocalWindow
            | Client::OpenAiRealtime
            | Client::AssemblyAi
            | Client::DeepgramRealtime
            | Client::CortiRealtime,
        )
        | None => false,
    };
    let finals: Vec<_> = transcript
        .events
        .iter()
        .filter(|event| event.is_final && !event.text.trim().is_empty())
        .collect();
    // Local streaming revises events in place. An unsettled event means the
    // finish handshake did not complete; retain recovery in that case too.
    let unsettled =
        provider_id == "local-streaming" && transcript.events.iter().any(|event| !event.is_final);
    if streaming
        && live_failure.is_none()
        && transcript.gaps == 0
        && !unsettled
        && (!finals.is_empty() || !has_audio)
    {
        let mut segments: Vec<_> = finals
            .into_iter()
            .map(|event| {
                let mut saved = segment(&event.text, event.timestamp, Some(event.source));
                // Speaker IDs are local to each socket, so mic speaker 0 and
                // system speaker 0 must not become the same person.
                saved.speaker = event.speaker.as_ref().map(|speaker| {
                    format!("{}:{speaker}", saved.source.as_deref().unwrap_or_default())
                });
                saved
            })
            .collect();
        segments.sort_by(|a, b| {
            a.timestamp
                .unwrap_or_default()
                .total_cmp(&b.timestamp.unwrap_or_default())
        });
        return SavedTranscript {
            segments,
            speaker_note: None,
            from_partials: false,
            used_streaming: true,
        };
    }

    match offline.await {
        Ok((segments, speaker_note)) => SavedTranscript {
            segments,
            speaker_note,
            from_partials: false,
            used_streaming: false,
        },
        Err(error) => {
            tracing::warn!(%error, "the whole-recording pass failed; keeping the live transcript");
            let events: Vec<_> = transcript
                .events
                .iter()
                .filter(|event| !event.text.trim().is_empty())
                .collect();
            SavedTranscript {
                segments: events
                    .into_iter()
                    .map(|event| {
                        let mut saved = segment(&event.text, event.timestamp, Some(event.source));
                        saved.speaker = event.speaker.clone();
                        saved
                    })
                    .collect(),
                speaker_note: None,
                from_partials: true,
                used_streaming: false,
            }
        }
    }
}

/// The route of the text `select_transcript` chose to save (KNEG-368): the
/// live provider's when it is the live text -- a finished local stream, or the
/// live partials kept because the whole-recording pass failed -- and `local`
/// when it is that pass, which always runs on this computer. `None` when
/// nothing was transcribed.
pub(super) fn saved_provider<'a>(provider_id: &'a str, saved: &SavedTranscript) -> Option<&'a str> {
    if saved.segments.is_empty() {
        None
    } else if saved.used_streaming || saved.from_partials {
        Some(provider_id)
    } else {
        Some("local")
    }
}

/// Where a meeting's saved transcript was made. A whole-recording pass after
/// live captions from a provider off this computer names that provider as
/// `live_name` (the ruling on KNEG-368): "on this computer" alone could read
/// as a claim that no audio left it.
pub(super) fn transcribed_by(
    provider_id: &str,
    saved: &SavedTranscript,
) -> Option<kw_notes_core::TranscribedBy> {
    crate::transcribed_by::with_live_provider(
        saved_provider(provider_id, saved)?,
        Some(provider_id),
    )
}

/// What one pass over the finished recording produced.
///
/// `timings` is empty when the engine returned none. Whisper answers that way
/// to a plain `json` request, which is what the pass sent until 2026-09-26
/// (KNEG-326), and an engine may report no timings at all -- so it is a state
/// the note has to be able to describe rather than a failure.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct WholePass {
    pub(super) text: String,
    pub(super) timings: Vec<kw_transcription_core::local::TimedSegment>,
}

/// The whisper server the whole-recording pass talks to, when whisper is the
/// local engine.
///
/// `.timed()`, because the note needs cues: a speaker label attaches to a
/// span, and a transcript row's clock and playback come from the same timings.
/// Until 2026-09-26 (KNEG-326) this was built bare in `meeting.rs`, so the
/// request asked for `json`, whisper answered one string, and every meeting
/// saved on a whisper engine with speaker labels on reported that the pass
/// "returned no timings" -- after the diarizer had run and found nothing to
/// attach its turns to. Media import had paid for timings since 2026-08-31;
/// the meeting pass never asked. About 45% more wall clock on the pass,
/// measured in `local.rs`'s module doc.
///
/// `.speech_first()` when `speech` is given (KNEG-404): sent whole, a
/// meeting's silences came back as rows stamped tens of seconds from their
/// speech, and noise as invented text. Without the tools the recording is
/// sent whole, as before. See `kw_transcription_core::speech`.
///
/// Here rather than inline in `meeting.rs` so a test can pin it: nothing in a
/// running application can be asked what it sent, which is how the bare server
/// went unnoticed for 25 days.
pub(super) fn whisper_for_note(
    policy: NetworkPolicy,
    port: u16,
    model: Option<String>,
    speech: Option<std::sync::Arc<dyn kw_transcription_core::speech::FindSpeech>>,
) -> WhisperServer {
    let server = WhisperServer::new(policy, port, model).timed();
    match speech {
        Some(finder) => server.speech_first(finder),
        None => server,
    }
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

    let mut segments: Vec<_> = pass
        .timings
        .iter()
        .map(|timed| kw_notes_core::transcript::Segment {
            text: timed.text.trim().to_owned(),
            timestamp: Some(timed.start),
            end_timestamp: Some(timed.end),
            speaker: None,
            speaker_name: None,
            speaker_is_placeholder: false,
            source: None,
            // Whisper's words for the passage, so its row highlights word by
            // word as the recording plays (KNEG-408). The text is trimmed
            // here and the words are not; the row compares them with
            // whitespace aside, so the trim costs nothing.
            words: crate::transcript_words::stored_words(&timed.words),
        })
        .collect();
    // One implementation of attaching turns to passages: the one that labels
    // a saved note afterwards.
    let unattributed = turns.map_or(0, |turns| {
        crate::speaker_labels::apply_turns(&mut segments, turns, 0.0)
    });

    let note = match (turns, wanted) {
        // Asked for, ran, and left some of the transcript unattributed. Said
        // out loud because a reader seeing most segments labelled and some not
        // would otherwise read the blanks as a person who said nothing.
        (Some(_), _) if unattributed > 0 => Some(crate::speaker_labels::unattributed_sentence(
            unattributed,
            segments.len(),
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

/// The whole-recording pass as the note saves it: without speakers, which
/// are added once the note exists (`crate::speaker_labels`). The sentence is
/// present only when they were asked for and this pass has no timings to
/// attach them to, so there is nothing to add later either.
pub(super) fn unlabelled(
    pass: &WholePass,
    speakers_wanted: bool,
) -> (Vec<kw_notes_core::transcript::Segment>, Option<String>) {
    labelled(pass, None, speakers_wanted && pass.timings.is_empty())
}

/// Whether speaker labels can be added to `saved` after its note is saved:
/// a whole transcript with timings, and no speakers of its provider's own.
/// The running transcript kept because the whole-recording pass failed is
/// overlapping windows, which never were labelled.
pub(super) fn can_label(saved: &SavedTranscript) -> bool {
    !saved.from_partials
        && saved.speaker_note.is_none()
        && !saved.segments.is_empty()
        && saved
            .segments
            .iter()
            .all(|segment| segment.speaker.is_none())
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

    fn stream_transcript() -> MeetingTranscript {
        MeetingTranscript {
            events: vec![
                kw_meeting_core::TranscriptEvent {
                    text: " remote words ".into(),
                    is_final: true,
                    timestamp: 20.0,
                    speaker: Some("0".into()),
                    source: AudioSource::System,
                },
                kw_meeting_core::TranscriptEvent {
                    text: "local words".into(),
                    is_final: true,
                    timestamp: 10.0,
                    speaker: Some("0".into()),
                    source: AudioSource::Mic,
                },
            ],
            duration_seconds: 7200.0,
            reconnects: 0,
            gaps: 0,
        }
    }

    #[tokio::test]
    async fn long_streaming_meetings_save_existing_text_without_polling_offline_asr() {
        let provider = "local-streaming";
        let transcript = stream_transcript();
        let saved = tokio::time::timeout(
            std::time::Duration::from_secs(1),
            select_transcript(provider, &transcript, None, true, std::future::pending()),
        )
        .await
        .expect("must not wait for offline ASR");
        assert!(saved.used_streaming, "{provider}");
        assert!(!saved.from_partials);
        assert_eq!(saved.segments.len(), 2);
        assert_eq!(saved.segments[0].text, "local words");
        assert_eq!(saved.segments[0].timestamp, Some(10.0));
        assert_eq!(saved.segments[0].source.as_deref(), Some("mic"));
        assert_eq!(saved.segments[0].speaker.as_deref(), Some("mic:0"));
        assert_eq!(saved.segments[1].text, "remote words");
        assert_eq!(saved.segments[1].speaker.as_deref(), Some("system:0"));
    }

    #[tokio::test]
    async fn providers_without_a_verified_final_flush_keep_the_whole_recording_pass() {
        for provider in [
            "local",
            "openai-realtime",
            "assemblyai-realtime",
            "deepgram-realtime",
            "corti-realtime",
            "unknown",
        ] {
            let saved = select_transcript(provider, &stream_transcript(), None, true, async {
                Ok((vec![segment("whole recording", 0.0, None)], None))
            })
            .await;
            assert!(!saved.used_streaming, "{provider}");
            assert_eq!(saved.segments[0].text, "whole recording");
        }
    }

    #[tokio::test]
    async fn windowed_failed_gapped_and_empty_audio_sessions_still_use_recovery() {
        for case in 0..6 {
            let mut transcript = stream_transcript();
            let provider = if case == 0 {
                "local"
            } else {
                "local-streaming"
            };
            let failure = (case == 1).then_some("socket failed");
            match case {
                2 => transcript.gaps = 1,
                3 => transcript.events.clear(),
                4 => transcript.events[0].is_final = false,
                5 => transcript
                    .events
                    .iter_mut()
                    .for_each(|event| event.text.clear()),
                _ => {}
            }
            let saved = select_transcript(provider, &transcript, failure, true, async {
                Ok((vec![segment("recovered recording", 0.0, None)], None))
            })
            .await;
            assert!(!saved.used_streaming, "case {case}");
            assert!(!saved.from_partials);
            assert_eq!(saved.segments[0].text, "recovered recording");
        }
    }

    #[tokio::test]
    async fn a_quiet_streaming_meeting_needs_no_offline_pass() {
        let saved = select_transcript(
            "local-streaming",
            &MeetingTranscript::default(),
            None,
            false,
            async { panic!("quiet streaming must not retranscribe") },
        )
        .await;
        assert!(saved.used_streaming);
        assert!(saved.segments.is_empty());
    }

    #[tokio::test]
    async fn failed_recovery_keeps_streaming_text_and_marks_it_incomplete() {
        let mut transcript = stream_transcript();
        transcript.gaps = 1;
        let saved = select_transcript("local-streaming", &transcript, None, true, async {
            Err("offline engine unavailable".into())
        })
        .await;
        assert!(saved.from_partials);
        assert_eq!(saved.segments.len(), 2);
    }

    /* ------------------------------------- where it was transcribed (KNEG-368) */

    #[tokio::test]
    async fn a_streamed_local_transcript_was_made_on_this_computer() {
        let saved = select_transcript(
            "local-streaming",
            &stream_transcript(),
            None,
            true,
            std::future::pending(),
        )
        .await;
        assert_eq!(
            saved_provider("local-streaming", &saved),
            Some("local-streaming")
        );
        let by = transcribed_by("local-streaming", &saved).unwrap();
        assert!(by.on_this_computer);
        assert_eq!(by.name, "this computer");
        assert_eq!(by.live_name, None);
    }

    #[tokio::test]
    async fn the_whole_pass_after_cloud_captions_is_local_and_names_the_captions() {
        // The ruling: "on this computer" alone could read as a claim that no
        // audio left it, when the live captions came from a cloud provider.
        let saved = select_transcript(
            "deepgram-realtime",
            &stream_transcript(),
            None,
            true,
            async { Ok((vec![segment("whole recording", 0.0, None)], None)) },
        )
        .await;
        assert_eq!(saved_provider("deepgram-realtime", &saved), Some("local"));
        let by = transcribed_by("deepgram-realtime", &saved).unwrap();
        assert_eq!(by.provider, "local");
        assert!(by.on_this_computer);
        assert_eq!(by.live_name.as_deref(), Some("Deepgram Realtime"));

        // Captions made on this computer too add nothing.
        let saved = select_transcript("local", &stream_transcript(), None, true, async {
            Ok((vec![segment("whole recording", 0.0, None)], None))
        })
        .await;
        let by = transcribed_by("local", &saved).unwrap();
        assert!(by.on_this_computer);
        assert_eq!(by.live_name, None);
    }

    #[tokio::test]
    async fn cloud_captions_kept_because_the_whole_pass_failed_name_the_cloud() {
        let saved = select_transcript(
            "deepgram-realtime",
            &stream_transcript(),
            None,
            true,
            async { Err("offline engine unavailable".into()) },
        )
        .await;
        assert!(saved.from_partials);
        assert_eq!(
            saved_provider("deepgram-realtime", &saved),
            Some("deepgram-realtime")
        );
        let by = transcribed_by("deepgram-realtime", &saved).unwrap();
        assert_eq!(by.name, "Deepgram Realtime");
        assert!(!by.on_this_computer);
        assert_eq!(by.live_name, None);
    }

    #[tokio::test]
    async fn nothing_transcribed_says_nothing_about_where() {
        let quiet = select_transcript(
            "local-streaming",
            &MeetingTranscript::default(),
            None,
            false,
            async { panic!("quiet streaming must not retranscribe") },
        )
        .await;
        let empty_pass = select_transcript(
            "deepgram-realtime",
            &MeetingTranscript::default(),
            None,
            true,
            async { Ok((Vec::new(), None)) },
        )
        .await;
        for (provider, saved) in [
            ("local-streaming", &quiet),
            ("deepgram-realtime", &empty_pass),
        ] {
            assert_eq!(saved_provider(provider, saved), None, "{provider}");
            assert_eq!(transcribed_by(provider, saved), None, "{provider}");
        }
    }

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
            words: Vec::new(),
        }
    }

    fn timed_word(text: &str, start: f64, end: f64) -> kw_transcription_core::local::TimedWord {
        kw_transcription_core::local::TimedWord {
            text: text.to_owned(),
            start,
            end,
        }
    }

    #[test]
    fn a_passages_words_stay_with_its_segment_labelled_or_not() {
        // KNEG-408: the meeting's whole-recording pass keeps the words
        // whisper timed, so its note's rows highlight word by word.
        let mut pass = two_utterances();
        pass.timings[0].words = vec![
            timed_word("Morning", 0.0, 0.6),
            timed_word("all.", 0.7, 1.9),
        ];
        let expected = vec![
            kw_notes_core::Word {
                text: "Morning".to_owned(),
                start: 0.0,
                end: 0.6,
            },
            kw_notes_core::Word {
                text: "all.".to_owned(),
                start: 0.7,
                end: 1.9,
            },
        ];
        let turns = [turn(0, 0.0, 2.2), turn(1, 2.4, 5.1)];
        for (segments, _) in [
            labelled(&pass, None, false),
            labelled(&pass, Some(&turns), true),
        ] {
            assert_eq!(segments[0].words, expected);
            assert!(segments[1].words.is_empty());
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
    fn the_whole_recording_pass_asks_whisper_for_timings() {
        // KNEG-326. Bare, the request is `json`, whisper answers one string,
        // and `labelled` writes "returned no timings" on every whisper meeting
        // with speaker labels on. The default is the cheap one for dictation's
        // sake, so the meeting pass has to say otherwise, here.
        let policy = kw_network_policy::NetworkPolicy::new().unwrap();
        assert!(whisper_for_note(policy, 8178, None, None).asks_for_timings());
    }

    #[test]
    fn the_whole_recording_pass_finds_the_speech_first_when_it_can() {
        // KNEG-404. Sent whole, a meeting's silences came back as rows stamped
        // tens of seconds from their speech, and noise as invented text.
        let policy = kw_network_policy::NetworkPolicy::new().unwrap();
        let finder: std::sync::Arc<dyn kw_transcription_core::speech::FindSpeech> =
            std::sync::Arc::new(kw_transcription_core::speech::SpeechFinder::new(
                "vad.exe".into(),
                "silero.bin".into(),
                kw_audio_core::vad::VadConfig::default(),
                "scratch".into(),
            ));
        let server = whisper_for_note(policy.clone(), 8178, None, Some(finder));
        assert!(server.finds_speech_first());
        assert!(server.asks_for_timings());
        // Without the tools, whole, as before -- never a failed note.
        assert!(!whisper_for_note(policy, 8178, None, None).finds_speech_first());
    }

    #[test]
    fn a_pass_with_no_timings_is_still_one_readable_segment() {
        // Whisper produces this when asked for `json` rather than
        // `verbose_json` -- which the meeting pass did until 2026-09-26
        // (KNEG-326) -- and it is the shape every meeting note had before
        // timings were kept. It must keep working, unlabelled, rather than
        // becoming an error.
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

    /// Nothing asked for means nothing said.
    ///
    /// The rule the other way round: a person who has this switched off must not
    /// find sentences about diarization in their notes.
    ///
    /// **This discarded the segments entirely**, and `None` is also what a
    /// `labelled` that produced nothing at all would return — so the claim
    /// "no note was written" was being made about a function that may have done
    /// no work. That is the shape KNEG-88 cost four hundred runs to find once.
    ///
    /// **Mutation:** return `(Vec::new(), None)` from the head of `labelled`.
    /// The control fails; without it the test passes.
    #[test]
    fn a_note_is_never_written_when_nothing_was_asked_for() {
        for pass in [
            two_utterances(),
            WholePass {
                text: "x".to_owned(),
                timings: Vec::new(),
            },
        ] {
            let (segments, note) = labelled(&pass, None, false);
            assert!(
                !segments.is_empty(),
                "the control: nothing was segmented, so there was no note to \
                 withhold and the line below says nothing about the rule"
            );
            assert_eq!(note, None);
        }
    }
}
