//! The recorder thread: two captures, one WAV, nothing accumulating.
//!
//! Lifted out of `meeting.rs` on 2026-09-01 (KNEG-89). Nothing changed in the
//! move.
//!
//! A thread rather than a task because `cpal::Stream` is not `Send` — the same
//! reason `dictation::MicRecorder` is one. It drains on a tick, converts each
//! source to mono at 16 kHz, appends the mix to a [`kw_audio_core::WavWriter`]
//! and forwards both streams. A meeting can run longer than memory, so nothing
//! here holds the recording.
//!
//! Two things in this file exist because of defects rather than design.
//! [`Delivered`] folds only towards *having heard something*, because an idle
//! WASAPI loopback delivers no callback at all and "capture is broken" and "the
//! far end was quiet" would otherwise look identical (KNEG-38). And
//! [`RecorderExit`] is a `Drop` guard rather than a return value, because a
//! thread that panics returns nothing and the screen used to keep saying
//! "Recording" (KNEG-116).

use std::sync::mpsc as sync_mpsc;
use std::sync::{Arc, OnceLock};

use kw_audio_core::meeting::MixFeed;
use kw_audio_core::{wav, Capture, LevelReader, SystemCapture, WavWriter};
use serde::Serialize;
use ts_rs::TS;

use tokio::sync::mpsc;

use super::{Sources, DRAIN_INTERVAL};

/// Audio drained from both captures at one moment, mono.
///
/// At a rate the *recorder* chose, not the provider's. The recorder cannot know
/// what the session wants — OpenAI Realtime takes 24 kHz and whisper 16 — so it
/// brings both devices to one rate, says which, and leaves the last conversion
/// to the pump, which knows.
#[derive(Debug)]
pub(super) struct Drained {
    pub(super) mic: Vec<f32>,
    pub(super) system: Vec<f32>,
    /// The rate `mic` and `system` are at.
    pub(super) sample_rate: u32,
}

pub(super) enum RecorderCommand {
    /// Stop, patch the WAV header, and report the recording.
    Finish {
        reply: sync_mpsc::Sender<Result<Finished, String>>,
    },
    /// Stop and delete the recording.
    Cancel,
}

/// What a capture actually delivered over a meeting, as opposed to whether it
/// opened.
///
/// **The distinction KNEG-38 is about.** WASAPI loopback from an output device
/// with nothing playing delivers **no callback at all**, not a stream of zeros.
/// So at the API a capture that never worked and a call where nobody else spoke
/// are the same thing, and for a meeting recording that difference is the whole
/// point. `MeetingOutcome::NothingTranscribed` was where both landed, together
/// with a third case that is neither.
///
/// This does not make the first two distinguishable — nothing this side can,
/// and claiming otherwise would be the invention the rest of this file refuses.
/// What it does is *name* which of them happened, separate the third, and say
/// on screen why the first two look alike.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum Delivered {
    /// Never opened. [`Sources`] carries the reason.
    NotCaptured,
    /// Opened, and no callback ever arrived.
    ///
    /// On Windows this is what an idle output device produces, so it is the
    /// ordinary shape of a call where the other participants said nothing —
    /// and it is also what a capture that silently failed produces.
    Nothing,
    /// Delivered, and every sample was exactly zero.
    ///
    /// Distinct from [`Delivered::Nothing`]: something *was* playing, or the
    /// endpoint was producing buffers, and the content was digital silence.
    /// Muting at the application level looks like this; an idle device does
    /// not.
    Silence,
    /// Delivered, and at least one sample was not zero.
    ///
    /// The one that changes an outcome. Audio was captured and nothing was
    /// transcribed from it, which is not a quiet room and should not be
    /// reported as one.
    Audio,
}

impl Delivered {
    /// A neutral description for the saved meeting note.
    ///
    /// Facts only. In particular, `Nothing` does not say the capture failed:
    /// an idle WASAPI output endpoint produces the same result. The note keeps
    /// that ambiguity instead of turning an observation into a diagnosis.
    #[must_use]
    pub(super) const fn note_fact(self) -> &'static str {
        match self {
            Self::NotCaptured => "was not captured",
            Self::Nothing => "opened but delivered no audio blocks",
            Self::Silence => "delivered only digital silence",
            Self::Audio => "delivered non-silent audio",
        }
    }

    /// Fold one drained block into the running answer.
    ///
    /// Monotone by construction — `Nothing` to `Silence` to `Audio`, never
    /// back — so a single audible block over an hour of quiet still reports
    /// `Audio`. That is the direction that matters: the question is whether
    /// anything was ever heard, not whether the last few seconds were.
    #[must_use]
    fn plus(self, block: &[f32]) -> Self {
        match self {
            Self::NotCaptured | Self::Audio => self,
            Self::Nothing | Self::Silence => {
                if block.iter().any(|sample| *sample != 0.0) {
                    Self::Audio
                } else if block.is_empty() {
                    self
                } else {
                    Self::Silence
                }
            }
        }
    }
}

/// What the recorder produced.
#[derive(Debug)]
pub(super) struct Finished {
    pub(super) path: std::path::PathBuf,
    pub(super) duration_seconds: f64,
    /// Samples that exceeded full scale and were clamped by the encoder. See
    /// [`MixFeed`] on why this is counted rather than corrected.
    pub(super) clipped: u64,
    /// What the microphone actually delivered. See [`Delivered`].
    pub(super) mic_delivered: Delivered,
    /// What the system-audio capture actually delivered. See [`Delivered`].
    pub(super) system_delivered: Delivered,
}

/// How the recorder thread ended, written by the thread itself on its way out.
///
/// **Inferring this from the audio channel closing was the whole of KNEG-116.**
/// `pump` returns `Ok` when its channel closes, which at Stop is exactly right
/// and at any other time means the thread went. That reading was true and not
/// specific enough: it could not tell a thread that returned on purpose from
/// one that unwound, so a panicked recorder was reported with a state whose
/// first word claimed a recording that had stopped.
///
/// A `Drop` guard rather than a line on each return path. `record` has five
/// exits and **a panic is not one of them**, so anything written at the returns
/// would miss the only case worth a name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RecorderEnd {
    /// Returned: `Finish`, `Cancel`, or the command channel disconnecting.
    OnPurpose,
    /// Unwound. Nothing is recording, and nothing asked for that.
    Panicked,
}

/// Writes [`RecorderEnd`] as the recorder thread leaves, however it leaves.
///
/// **The drop order is load-bearing, and it is the same trick `Live::
/// transcript_failure` relies on.** This guard is the first binding in
/// `record`, and `audio` is a parameter; bindings drop in reverse order of
/// declaration, so the guard drops before the sender does. The cell is
/// therefore already written by the time the pump observes its channel close,
/// which is what makes reading the two together meaningful rather than a race.
pub(super) struct RecorderExit(Arc<OnceLock<RecorderEnd>>);

impl Drop for RecorderExit {
    fn drop(&mut self) {
        let _ = self.0.set(if std::thread::panicking() {
            RecorderEnd::Panicked
        } else {
            RecorderEnd::OnPurpose
        });
    }
}

/// What [`spawn_recorder`] hands back.
///
/// A struct since the fifth member (KNEG-371). It was a named tuple of four --
/// the audio stream, the command channel, what actually opened, and the
/// thread's own account of how it ended -- and the level readers made it one
/// too many to read by position.
#[derive(Debug)]
pub(super) struct Recorder {
    pub(super) audio: mpsc::UnboundedReceiver<Drained>,
    pub(super) commands: sync_mpsc::Sender<RecorderCommand>,
    pub(super) sources: Sources,
    pub(super) ended: Arc<OnceLock<RecorderEnd>>,
    pub(super) levels: Levels,
}

/// How loud each capture is, for the Meetings page's meters (KNEG-371).
///
/// Each reader answers `None` once its capture is dropped, which happens when
/// the recorder thread leaves -- by Finish, Cancel, a dropped meeting or a
/// panic -- so a meter cannot outlive the recording it shows.
#[derive(Debug)]
pub(super) struct Levels {
    pub(super) microphone: LevelReader,
    /// `None` when system audio was not captured.
    pub(super) system: Option<LevelReader>,
}

/// Start the capture thread.
///
/// Returns the audio channel and the command channel. Both captures are started
/// here, on the thread, because `cpal::Stream` is not `Send`.
///
/// `microphone` is the saved choice ([`crate::dictation::load_device`]), or
/// `None` for the system default; a choice that is not connected falls back to
/// the default (`kw_audio_core::Capture::start`).
pub(super) fn spawn_recorder(
    path: std::path::PathBuf,
    system: bool,
    microphone: Option<String>,
) -> Result<Recorder, String> {
    let (audio_tx, audio_rx) = mpsc::unbounded_channel::<Drained>();
    let (command_tx, command_rx) = sync_mpsc::channel::<RecorderCommand>();
    // The thread reports whether it got its devices before anything else
    // happens, so a failure to open the microphone is a failure to start rather
    // than a meeting that records nothing. With the captures' levels, which
    // can only be read from the captures, and those live on the thread.
    let (ready_tx, ready_rx) = sync_mpsc::channel::<Result<(Sources, Levels), String>>();
    // The thread's own account of how it ended, rather than the caller's
    // inference from a closed channel. See [`RecorderEnd`].
    let ended = Arc::new(OnceLock::new());

    std::thread::Builder::new()
        .name("kw-meeting".to_owned())
        .spawn({
            let ended = Arc::clone(&ended);
            move || {
                record(
                    path, system, microphone, audio_tx, command_rx, ready_tx, &ended,
                )
            }
        })
        .map_err(|e| format!("could not start the recording thread: {e}"))?;

    let (sources, levels) = ready_rx
        .recv()
        .map_err(|_| "the recording thread stopped before it started".to_owned())??;

    Ok(Recorder {
        audio: audio_rx,
        commands: command_tx,
        sources,
        ended,
        levels,
    })
}

/// The recorder thread.
fn record(
    path: std::path::PathBuf,
    want_system: bool,
    microphone: Option<String>,
    audio: mpsc::UnboundedSender<Drained>,
    commands: sync_mpsc::Receiver<RecorderCommand>,
    ready: sync_mpsc::Sender<Result<(Sources, Levels), String>>,
    ended: &Arc<OnceLock<RecorderEnd>>,
) {
    // First, so it is the last local dropped and therefore runs before `audio`
    // does. Every `return` below and every unwind goes through it.
    let _exit = RecorderExit(Arc::clone(ended));

    let _microphone_use = match crate::capture_interlock::CaptureInterlock::shared().recording() {
        Ok(lease) => lease,
        Err(error) => {
            let _ = ready.send(Err(error));
            return;
        }
    };
    let mic = match Capture::start(microphone.as_deref()) {
        Ok(capture) => capture,
        Err(err) => {
            let _ = ready.send(Err(format!("the microphone could not be opened: {err}")));
            return;
        }
    };

    // System audio is optional. A meeting with only the microphone is a real
    // meeting; one that refuses to start because the platform has no loopback
    // device is not an improvement.
    let (system, system_reason) = if want_system {
        match SystemCapture::start() {
            Ok(capture) => (Some(capture), None),
            Err(err) => {
                tracing::warn!(error = %err, "recording the microphone only");
                (None, Some(err.to_string()))
            }
        }
    } else {
        (
            None,
            Some("system audio was switched off for this meeting".to_owned()),
        )
    };

    let mut writer = match WavWriter::create(&path, wav::TARGET_SAMPLE_RATE) {
        Ok(writer) => writer,
        Err(err) => {
            let _ = ready.send(Err(format!("could not open the recording file: {err}")));
            return;
        }
    };

    let levels = Levels {
        microphone: mic.level(),
        system: system.as_ref().map(SystemCapture::level),
    };
    let _ = ready.send(Ok((
        Sources {
            microphone: true,
            microphone_name: mic.device_name().map(str::to_owned),
            system_audio: system.is_some(),
            system_audio_reason: system_reason,
        },
        levels,
    )));

    let mut mixer = MixFeed::new();

    // What each capture has actually delivered so far. Started from what was
    // opened rather than from `Nothing`, so a source that was never asked for
    // is never reported as one that fell silent.
    let mut mic_delivered = Delivered::Nothing;
    let mut system_delivered = if system.is_some() {
        Delivered::Nothing
    } else {
        Delivered::NotCaptured
    };

    // A closure so the tick and the final drain cannot diverge.
    let drain_once = |writer: &mut WavWriter,
                      mixer: &mut MixFeed,
                      mic_delivered: &mut Delivered,
                      system_delivered: &mut Delivered| {
        // Downmixed and brought to one rate here, because the two devices need
        // not agree: a 44.1 kHz microphone and a 48 kHz output endpoint is an
        // ordinary pairing, and mixing them without a common rate would drift.
        // 16 kHz because that is what the file on disk is for -- the
        // whole-recording pass at the end -- and a provider wanting something
        // else gets it from the pump.
        let mic_track = mic.drain();
        let mic_mono = wav::resample_to_target(
            &wav::to_mono(&mic_track.samples, mic_track.channels),
            mic_track.sample_rate,
        );

        let system_mono = system.as_ref().map_or_else(Vec::new, |capture| {
            let track = capture.drain();
            wav::resample_to_target(
                &wav::to_mono(&track.samples, track.channels),
                track.sample_rate,
            )
        });

        // Folded from the per-source blocks rather than from the mix, because
        // the mix is the sum: a silent system feed under a live microphone is
        // indistinguishable there, and telling those apart is the point.
        *mic_delivered = mic_delivered.plus(&mic_mono);
        *system_delivered = system_delivered.plus(&system_mono);

        mixer.push_a(&mic_mono);
        mixer.push_b(&system_mono);
        // With no system capture there is nothing to wait for, so the microphone
        // goes straight through rather than sitting in the mixer forever.
        let mixed = if system.is_some() {
            mixer.take()
        } else {
            mixer.flush()
        };
        if let Err(err) = writer.append(&mixed) {
            tracing::error!(error = %err, "could not append to the meeting recording");
        }

        audio.send(Drained {
            mic: mic_mono,
            system: system_mono,
            sample_rate: wav::TARGET_SAMPLE_RATE,
        })
    };

    loop {
        match commands.recv_timeout(DRAIN_INTERVAL) {
            Ok(RecorderCommand::Finish { reply }) => {
                let _ = drain_once(
                    &mut writer,
                    &mut mixer,
                    &mut mic_delivered,
                    &mut system_delivered,
                );
                let tail = mixer.flush();
                if let Err(err) = writer.append(&tail) {
                    tracing::error!(error = %err, "could not append the last of the recording");
                }

                let clipped = mixer.clipped();
                let result = writer
                    .finish()
                    .map_err(|e| format!("could not close the recording: {e}"))
                    .map(|samples| Finished {
                        path: path.clone(),
                        duration_seconds: wav::duration_ms(
                            usize::try_from(samples).unwrap_or(usize::MAX),
                            wav::TARGET_SAMPLE_RATE,
                        ) as f64
                            / 1_000.0,
                        clipped,
                        mic_delivered,
                        system_delivered,
                    });
                let _ = reply.send(result);
                return;
            }
            Ok(RecorderCommand::Cancel) => {
                // The header still says zero samples, so even a file left behind
                // by a failed delete reads as empty.
                drop(writer);
                if let Err(err) = std::fs::remove_file(&path) {
                    tracing::warn!(error = %err, "could not delete the cancelled recording");
                }
                return;
            }
            Err(sync_mpsc::RecvTimeoutError::Timeout) => {
                if drain_once(
                    &mut writer,
                    &mut mixer,
                    &mut mic_delivered,
                    &mut system_delivered,
                )
                .is_err()
                {
                    // Nothing is reading the audio, which means the pump has
                    // gone. Keep writing the file: the recording is still worth
                    // having even if the transcript is not coming.
                    tracing::debug!("nothing is consuming the meeting audio");
                }
            }
            Err(sync_mpsc::RecvTimeoutError::Disconnected) => {
                tracing::warn!("the meeting was dropped without being stopped");
                let _ = writer.finish();
                return;
            }
        }
    }
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

    // The state machine and the outcome live in the parent: what the recorder
    // reports and what the screen says about it are two facts, and these tests
    // are about the seam between them.
    use super::super::{
        close_recorder, failed_message, is_this_session, live_state, live_transcript_ended,
        pump_result, MeetingState,
    };
    use kw_meeting_core::{MeetingError, MeetingTranscript};

    /* ----------------------------------------------------------- the meters */

    /// The levels the ready message carries are the open captures' own, and
    /// they end with the recorder thread, so a meeting's meters cannot
    /// outlive it (KNEG-371); and `Sources` names the microphone the capture
    /// opened (KNEG-364). Opens the real default microphone, as
    /// `dictation`'s microphone-thread tests do, into a temporary file that
    /// Cancel deletes; on a machine with no microphone the start fails and
    /// there is nothing to measure. Ignored so `cargo test` never opens the
    /// microphone on its own: run it by hand with `--ignored`.
    #[test]
    #[ignore = "opens the real microphone"]
    fn the_meters_are_the_captures_own_and_end_with_the_recorder() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("meters.wav");
        let Ok(recorder) = spawn_recorder(path.clone(), false, None) else {
            return;
        };

        assert!(
            recorder.levels.system.is_none(),
            "system audio was not asked for, so it has no meter"
        );
        assert!(
            recorder
                .sources
                .microphone_name
                .as_deref()
                .is_some_and(|name| !name.is_empty()),
            "the microphone that opened is named: {:?}",
            recorder.sources.microphone_name
        );
        assert!(
            recorder.levels.microphone.take().is_some(),
            "the microphone's capture is open"
        );

        recorder.commands.send(RecorderCommand::Cancel).unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while recorder.levels.microphone.take().is_some() {
            assert!(
                std::time::Instant::now() < deadline,
                "the microphone's meter outlived the cancelled recording"
            );
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(!path.exists(), "Cancel deleted the recording");
    }

    /* --------------------------------------------------- what was delivered */

    /// An empty block is not silence, and that is the whole distinction.
    ///
    /// WASAPI loopback from an idle output device produces no callback, so the
    /// drain finds nothing to fold. Reading that as "a block of silence" would
    /// collapse the two cases KNEG-38 exists to keep apart before anything
    /// downstream could see them.
    #[test]
    fn nothing_delivered_is_not_the_same_as_silence_delivered() {
        assert_eq!(Delivered::Nothing.plus(&[]), Delivered::Nothing);
        assert_eq!(Delivered::Nothing.plus(&[0.0, 0.0]), Delivered::Silence);
    }

    /// Once something has been heard, it stays heard.
    ///
    /// The question is whether anything was ever audible over the meeting, not
    /// whether the last few seconds were. An hour of silence after one spoken
    /// word must still report `Audio`, or a meeting that went quiet at the end
    /// would be reported as one nobody spoke in.
    #[test]
    fn hearing_something_once_is_not_undone_by_silence_after_it() {
        let heard = Delivered::Nothing.plus(&[0.0, 0.2, 0.0]);
        assert_eq!(heard, Delivered::Audio);
        assert_eq!(heard.plus(&[0.0; 16]), Delivered::Audio);
        assert_eq!(heard.plus(&[]), Delivered::Audio);
    }

    /// A source that was never opened is never reported as one that fell
    /// silent.
    ///
    /// `NotCaptured` is a different answer from `Nothing`, and the difference
    /// is whether there is anything to look into: a meeting recorded without
    /// system audio has a reason on `Sources` and no defect behind it.
    #[test]
    fn a_source_that_was_not_captured_stays_that_way() {
        assert_eq!(Delivered::NotCaptured.plus(&[]), Delivered::NotCaptured);
        assert_eq!(
            Delivered::NotCaptured.plus(&[0.0, 0.5]),
            Delivered::NotCaptured
        );
    }

    /// Silence then sound is `Audio`; the fold only ever moves one way.
    #[test]
    fn the_fold_only_moves_towards_having_heard_something() {
        let quiet = Delivered::Nothing.plus(&[0.0; 8]);
        assert_eq!(quiet, Delivered::Silence);
        assert_eq!(quiet.plus(&[0.0, -0.4]), Delivered::Audio);
    }

    /// A live transcript that died must not put the tray out.
    ///
    /// The recording is still running, so the one indicator the brief actually
    /// requires has to stay lit. This is the rule `emit_state` asks, in the one
    /// place a new variant has to answer it.
    #[test]
    fn every_state_but_idle_is_a_running_session() {
        assert!(!MeetingState::Idle.is_running());
        assert!(MeetingState::Recording.is_running());
        assert!(MeetingState::RecordingWithoutTranscript.is_running());
        // Especially this one. Whatever was captured before the thread went is
        // on disk, and a state that hid Stop would throw it away in order to
        // report that the recording stopped.
        assert!(MeetingState::Stalled.is_running());
        assert!(MeetingState::Finishing.is_running());
    }

    #[test]
    fn a_healthy_session_is_reported_as_recording() {
        assert_eq!(live_state(false, false, None), MeetingState::Recording);
    }

    /// A recorder that returned on purpose is not a stalled meeting.
    ///
    /// Reachable while a session is held only in the instant `stop` has asked
    /// the thread to finish and has not yet taken the session. Reporting that
    /// as `Stalled` would put a failure on screen for a Stop the user pressed.
    #[test]
    fn a_recorder_that_left_on_purpose_is_not_a_failure() {
        assert_eq!(
            live_state(false, false, Some(RecorderEnd::OnPurpose)),
            MeetingState::Recording
        );
    }

    #[test]
    fn an_announced_failure_changes_what_the_screen_says() {
        assert_eq!(
            live_state(true, false, None),
            MeetingState::RecordingWithoutTranscript
        );
    }

    /// KNEG-116 in one assertion: the word that led the state was false.
    ///
    /// A panicked recorder ends the audio channel, the pump returns `Ok`, and
    /// before this the answer was `RecordingWithoutTranscript` — whose first
    /// word claims the one thing that had definitely stopped. The recorder's
    /// own report is what makes the difference sayable.
    #[test]
    fn a_recorder_that_panicked_is_not_still_recording() {
        assert_eq!(
            live_state(false, false, Some(RecorderEnd::Panicked)),
            MeetingState::Stalled
        );
        // And it wins over the transcription signals, because those are about
        // the transcript and this is about whether anything is being captured.
        assert_eq!(
            live_state(true, true, Some(RecorderEnd::Panicked)),
            MeetingState::Stalled
        );
    }

    /// The backstop, and the reason `pump.is_finished()` is consulted at all.
    ///
    /// The announcement is written by the pump's own task before it returns, so
    /// the handle cannot finish first. A finished pump with nothing announced
    /// therefore means one thing only: the task panicked, or it was aborted.
    /// There is no provider sentence for either, and "recording, transcript
    /// running" would still be the wrong thing to say.
    #[test]
    fn a_pump_that_vanished_without_a_word_is_still_not_a_working_transcript() {
        assert_eq!(
            live_state(false, true, None),
            MeetingState::RecordingWithoutTranscript
        );
    }

    /// The mid-meeting sentence and the sentence at Stop are the same sentence.
    ///
    /// Two call sites, one formatter -- the point being that a user who reads
    /// the indicator and then reads the outcome is not told two different
    /// stories about one failure.
    #[test]
    fn the_indicator_and_the_outcome_use_one_sentence() {
        let fault = MeetingError::ProviderFault {
            provider: "openai-realtime".to_owned(),
            detail: "on the system audio stream the provider reported: The Realtime \
                     Beta API is no longer supported."
                .to_owned(),
        };

        let announced = live_transcript_ended(Err(&fault), None);
        let at_stop = pump_result(Ok(Err(fault))).unwrap_err();
        assert_eq!(announced, at_stop);
        assert!(announced.contains("Realtime Beta API"), "{announced}");
    }

    /// The `Ok` arm names the recorder, because that is what it means.
    ///
    /// Three sentences, not one, and the difference is the whole of KNEG-116:
    /// the thread saying *how* it left is a different fact from its channel
    /// having closed, and only one of the three is a failure.
    #[test]
    fn what_ended_the_recorder_changes_what_is_said_about_it() {
        let unknown = live_transcript_ended(Ok(()), None);
        assert!(unknown.contains("recording thread"), "{unknown}");
        assert!(unknown.contains("nothing further"), "{unknown}");

        let panicked = live_transcript_ended(Ok(()), Some(RecorderEnd::Panicked));
        assert!(panicked.contains("unexpectedly"), "{panicked}");
        // The recording is the artefact that cannot be recreated, so the
        // sentence has to say it survived and how to get it.
        assert!(panicked.contains("still on disk"), "{panicked}");
        assert!(panicked.contains("Stop"), "{panicked}");

        let on_purpose = live_transcript_ended(Ok(()), Some(RecorderEnd::OnPurpose));
        assert!(!on_purpose.contains("unexpectedly"), "{on_purpose}");

        // All three are distinct: a formatter that collapsed two of them would
        // pass every assertion above.
        assert_ne!(unknown, panicked);
        assert_ne!(unknown, on_purpose);
        assert_ne!(panicked, on_purpose);
    }

    /// The guard writes the reason, and writes a different one when unwinding.
    ///
    /// The mechanism the rest of this rests on, tested on a real thread rather
    /// than reasoned about: a `Drop` impl that reads `std::thread::panicking()`
    /// is easy to write backwards and impossible to see is backwards.
    #[test]
    fn the_recorder_guard_can_tell_a_panic_from_a_return() {
        let calm: Arc<OnceLock<RecorderEnd>> = Arc::new(OnceLock::new());
        {
            let _exit = RecorderExit(Arc::clone(&calm));
        }
        assert_eq!(calm.get().copied(), Some(RecorderEnd::OnPurpose));

        let panicked: Arc<OnceLock<RecorderEnd>> = Arc::new(OnceLock::new());
        let cell = Arc::clone(&panicked);
        let joined = std::thread::spawn(move || {
            let _exit = RecorderExit(cell);
            #[allow(clippy::panic)]
            {
                panic!("the recorder went");
            }
        })
        .join();
        assert!(joined.is_err(), "the thread was supposed to unwind");
        assert_eq!(panicked.get().copied(), Some(RecorderEnd::Panicked));
    }

    /// The race `stop` opens, and the reason identity is checked rather than
    /// presence.
    ///
    /// `stop` takes the session out and then spends the whole-file pass outside
    /// the lock, so `start` can begin a second meeting while the first is still
    /// finishing. A pump task arriving late would otherwise find *a* session and
    /// mark a healthy new meeting as having lost its transcript.
    #[test]
    fn a_late_ending_cannot_be_pinned_on_the_next_meeting() {
        let mine: Arc<OnceLock<String>> = Arc::new(OnceLock::new());
        let next: Arc<OnceLock<String>> = Arc::new(OnceLock::new());

        assert!(is_this_session(Some(&mine), &mine), "its own session");
        assert!(
            !is_this_session(Some(&next), &mine),
            "a session that started while this one was finishing"
        );
        assert!(
            !is_this_session(None, &mine),
            "stop or cancel already took it, so the ending is the one they asked for"
        );
    }

    #[test]
    fn a_pump_that_finished_hands_its_transcript_straight_back() {
        let transcript = MeetingTranscript {
            duration_seconds: 12.0,
            reconnects: 1,
            ..MeetingTranscript::default()
        };
        assert_eq!(pump_result(Ok(Ok(transcript.clone()))).unwrap(), transcript);
    }

    #[tokio::test]
    async fn a_provider_fault_becomes_a_sentence_rather_than_taking_stop_out_early() {
        // `stop` used to `?` here. That left `Live` already taken, the runtime
        // idle, and the screen on "Recording" with no working button on it --
        // `guard` in `useRefresh.ts` does not reload when a mutation throws, so
        // the frontend stayed on `finishing`, where Stop and Discard are not
        // rendered and Start is disabled. Only a restart cleared it.
        //
        // The words are the real ones, from the meeting of 2026-08-28.
        let reason = pump_result(Ok(Err(MeetingError::ProviderFault {
            provider: "openai-realtime".to_owned(),
            detail: "on the system audio stream the provider reported: The Realtime Beta API \
                     is no longer supported."
                .to_owned(),
        })))
        .unwrap_err();

        assert!(reason.contains("openai-realtime"), "{reason}");
        assert!(reason.contains("Realtime Beta API"), "{reason}");
        assert!(
            reason.contains("system audio"),
            "which of the two sockets died is part of the answer: {reason}"
        );
    }

    #[tokio::test]
    async fn a_pump_that_panicked_is_a_sentence_too_rather_than_a_stuck_screen() {
        let handle: tokio::task::JoinHandle<Result<MeetingTranscript, MeetingError>> =
            tokio::spawn(async { panic!("the pump panicked") });
        let reason = pump_result(handle.await).unwrap_err();
        assert!(reason.contains("the transcription task failed"), "{reason}");
    }

    #[test]
    fn a_failed_meeting_still_says_where_the_recording_is() {
        // The WAV is the artefact that cannot be recreated -- a note can be
        // rebuilt from it by hand, and the meeting cannot be held again. Same
        // shape as the failure message `keep` already had for a note that could
        // not be saved.
        let message = failed_message(
            "C:/x/20260828-135901.wav",
            "openai-realtime accepted the connection and then stopped transcribing: boom.",
        );
        assert!(message.starts_with("The recording is at "), "{message}");
        assert!(message.contains("20260828-135901.wav"), "{message}");
        assert!(message.contains("stopped transcribing"), "{message}");
    }

    #[test]
    fn a_recorder_that_has_already_stopped_is_reported_rather_than_waited_for() {
        // The channel's receiver is dropped with the thread, which is what a
        // recorder that died looks like from here. This was the first of `stop`'s
        // three `?`s.
        let (commands, receiver) = sync_mpsc::channel::<RecorderCommand>();
        drop(receiver);
        let reason = close_recorder(&commands).unwrap_err();
        assert!(reason.contains("already stopped"), "{reason}");
    }
}
