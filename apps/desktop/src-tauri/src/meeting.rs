//! Running a meeting: two captures, one recording on disk, live transcript.
//!
//! The same shape as [`crate::dictation`] and for the same reasons — the state
//! lives in Rust because the window can be closed to the tray mid-meeting, and
//! the outcome is emitted rather than returned because there is nobody to return
//! it to. What differs is that a meeting does not end: it produces transcript
//! while it runs, and a recording longer than memory.
//!
//! # Three moving parts
//!
//! **A recorder thread** owns both captures, because `cpal::Stream` is not
//! `Send` — the same reason `dictation::MicRecorder` is a thread rather than a
//! task. It drains on a tick, converts each source to mono at 16 kHz, appends the
//! mix to a [`kw_audio_core::WavWriter`], and forwards the two streams. Nothing
//! accumulates.
//!
//! **A pump task** owns the provider and feeds it what the recorder forwards,
//! emitting each transcript event as it arrives. It ends when the recorder closes
//! its channel, which is how "stop" travels from one to the other without a
//! second signal to get out of step.
//!
//! **A finishing step** transcribes the whole recording once and writes the note.
//!
//! # Why the note is not made of the live events
//!
//! The live events are partials, and what that costs depends on the provider:
//! `local`'s five-second windows overlap by two and repeat words, and
//! [`kw_meeting_core::local`] explains why deduplicating them properly is not
//! possible with what `/inference` returns; `local-streaming` revises one event
//! per utterance in place and does not repeat at all. Either way they are the
//! running transcription rather than a considered pass, so the note gets one
//! pass over the finished file, which is exactly the path a dictation takes.
//!
//! **Which engine that pass uses is [`crate::dictation::local_engine`]'s
//! answer, not this module's.** It used to build a `WhisperServer` directly and
//! so failed outright whenever a sherpa model was selected -- the documented
//! one-local-engine state -- and every note silently fell back to the partials
//! it exists to avoid. Found by a person reading a saved note on 2026-09-01.
//!
//! If that pass fails — an hour of audio is a large request — the partials are
//! saved instead, with the note saying so. A meeting nobody can read because the
//! final pass timed out would be the worse outcome, and a note that quietly
//! contains overlapping fragments without saying so would be worse still.
//!
//! # The user always knows it is recording
//!
//! `meeting_status` reports the state and the tray says which recordings are
//! live. Nothing here starts a recording on its own: detection suggests, and a
//! person clicks.
//!
//! An earlier version of this comment said the tray reflected the state before
//! `tray.rs` had any state to reflect. Left recorded rather than quietly
//! corrected, because a doc comment asserting an unbuilt feature is the same
//! defect as an interface reporting a value it never read, and this project has
//! now found that three times.
//!
//! # Detection offers; it never records
//!
//! [`Watcher`] polls the process table while idle and, when it finds a meeting
//! *in progress*, shows the notification window with an offer. There is no path
//! from it to [`MeetingRuntime::start`] that does not go through a click.
//!
//! **Only in-call evidence prompts**, and that is a measurement rather than a
//! preference. `meeting_detect_probe` on this machine found `ms-teams.exe`
//! running with no call in progress — new Teams runs the call inside its own
//! process, so there is no separate in-call signal to find. Prompting on "the
//! client is open" would therefore fire every launch for anyone whose Teams
//! starts with Windows, which is most corporate machines, and the feature would
//! be switched off within a day. Teams and FaceTime are reported on the screen
//! instead, with the reason, so the limit is visible rather than felt as a bug.

use std::sync::mpsc as sync_mpsc;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use kw_audio_core::meeting::MixFeed;
use kw_audio_core::{wav, Capture, SystemAudioSupport, SystemCapture, WavWriter};
use kw_meeting_core::local::LocalWindowProvider;
use kw_meeting_core::{
    AudioChunk, AudioSource, Client, MeetingError, MeetingProviderConfig, MeetingTranscript,
    MeetingTranscriptionProvider, TranscriptEvent,
};
use kw_network_policy::NetworkPolicy;
use kw_overlay::CaptureExclusion;
use kw_platform::meeting_detect::{self, CallSignal, Detected, Lists};
use kw_platform::microphone::MicrophoneSupport;
use kw_storage::Db;
use serde::Serialize;
use tauri::{AppHandle, Emitter as _, Manager as _, State};
use tokio::sync::{mpsc, Mutex};
use ts_rs::TS;

/// State changes.
pub const STATE_EVENT: &str = "meeting-state";
/// One piece of transcript, as it is produced.
pub const TRANSCRIPT_EVENT: &str = "meeting-transcript";
/// The meeting ended, with what came of it.
pub const OUTCOME_EVENT: &str = "meeting-outcome";
/// A meeting appears to be in progress. Carries an `Offer`, or null to clear it.
pub const OFFER_EVENT: &str = "meeting-offer";

/// How often the recorder drains the captures.
///
/// One second, which is well inside the three-second hop: the windowing decides
/// when audio is transcribed, and this only decides how often it is moved out of
/// the capture buffers. Draining more often would cost locks for nothing.
const DRAIN_INTERVAL: Duration = Duration::from_secs(1);

/// How often the process table is walked, while no meeting is being recorded.
///
/// Five seconds. A full walk is not free — 628 processes on the machine this was
/// written on — and five seconds still arrives while a call is being joined.
/// There is nothing to detect during a meeting, so the watcher skips the walk
/// entirely then rather than polling and discarding.
const DETECT_INTERVAL: Duration = Duration::from_secs(5);

/// Settings key: whether a detected meeting offers to record.
const PROMPT_KEY: &str = "meetings.promptOnDetect";
/// Settings key: the user's own process names.
const LISTS_KEY: &str = "meetings.detect";
/// Settings key: the language to give the provider, or the empty string for
/// automatic detection where the provider supports it.
const LANGUAGE_KEY: &str = "meetings.language";
/// Settings key: terms to bias recognition towards.
const KEYTERMS_KEY: &str = "meetings.keyterms";

/// Prompting is on by default.
///
/// A dismissible offer is the whole point of detection, and one that has to be
/// found in settings first would ship invisible. It only ever appears for a
/// meeting actually in progress — see the module documentation on why that
/// matters.
const PROMPT_BY_DEFAULT: bool = true;

/// What a meeting session is doing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum MeetingState {
    Idle,
    Recording,
    /// Still recording, and the live transcription has stopped.
    ///
    /// The state KNEG-95 was filed for. On 2026-08-28 both OpenAI Realtime
    /// sockets closed within milliseconds of connecting; the application went
    /// on saying **Recording**, kept the indicator up and captured both streams
    /// for as long as it was left running. The only clue was an empty live
    /// transcript, which is also what a quiet room looks like.
    ///
    /// **The recording continues, and that is the decision rather than an
    /// omission.** The WAV is what saved that session: `record` keeps writing
    /// when nothing is consuming the audio, on purpose and with a comment
    /// saying why; the whole-file pass at Stop reads the complete audio; and
    /// the saved note comes from that pass, with the live partials only ever
    /// its fallback. Stopping the recording to report a dead provider would
    /// destroy the valuable half in order to fix the misleading half.
    ///
    /// So the name leads with `Recording` -- in the enum, in the JSON and in a
    /// log line, the first word is the one that is still true.
    ///
    /// The one case where it would not have been is now [`MeetingState::
    /// Stalled`], which is KNEG-116. Until 2026-08-31 a recorder thread that
    /// panicked also landed here, and the first word was false; the sentence
    /// beside it said the true thing and the name did not.
    RecordingWithoutTranscript,
    /// Nothing is being recorded, and nothing asked for that.
    ///
    /// The recording thread ended with no Stop and no Discard behind it, which
    /// in practice means it panicked: `record` returns on `Finish`, on `Cancel`
    /// and on its command channel disconnecting, and all three of those run
    /// only after the session has been taken.
    ///
    /// **A running session, deliberately.** [`MeetingState::is_running`] stays
    /// true so Stop and Discard remain on screen: whatever was captured before
    /// the thread went is on disk, and the whole-file pass can still read it.
    /// A state that hid those buttons would throw the recording away to report
    /// that the recording stopped.
    ///
    /// Named for what is true rather than for what failed. It is not a variant
    /// of `Recording`, because the thing that word describes has stopped, and
    /// that mattered enough to be filed separately (KNEG-116) rather than left
    /// as a comment on the state above.
    ///
    /// **No recorder thread has ever been seen to panic in this project.** The
    /// case is reached by reading `record`'s exits, and now by
    /// [`RecorderEnd`], which is the thread saying so itself rather than the
    /// caller inferring it.
    Stalled,
    /// The recording has stopped and the whole-file pass is still running.
    ///
    /// A separate state because it can take minutes on a long meeting, and
    /// "Idle" while the transcript is still being produced would look like a
    /// meeting that vanished.
    Finishing,
}

impl MeetingState {
    /// Whether a meeting session exists at all.
    ///
    /// What the tray asks, extracted so a new variant has one place to be
    /// classified rather than an inequality to be noticed. Recording *and*
    /// finishing keep it lit: the microphone is closed once finishing starts,
    /// but a meeting that still has work to do has not finished, and an idle
    /// tray would say it had. A dead live transcript changes nothing here --
    /// the recording is still running, which is exactly what the tray is for.
    #[must_use]
    pub const fn is_running(self) -> bool {
        !matches!(self, Self::Idle)
    }
}

/// Which captures are actually running.
///
/// Reported rather than assumed, because a meeting works microphone-only and the
/// user should know which one they got. *No system capture at all* is a
/// different fact from a capture that produced nothing.
///
/// It used to say here that zero system-audio samples means nobody else spoke.
/// It does not, and that was KNEG-38: an output device with nothing playing
/// delivers no callback at all, so zero samples is equally what a capture that
/// never worked produces. [`Delivered`] carries which of the possible answers
/// applies, and says plainly that two of them cannot be told apart.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Sources {
    pub microphone: bool,
    pub system_audio: bool,
    /// Why system audio is not being captured, when it is not.
    pub system_audio_reason: Option<String>,
}

/// What can be said about the meeting session.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct MeetingStatus {
    pub state: MeetingState,
    /// Why the live transcription stopped, when it has.
    ///
    /// The provider's own sentence, naming which of the two sockets died --
    /// [`kw_meeting_core::MeetingError::ProviderFault`] renders it and
    /// [`live_transcript_ended`] formats it, so this is the same sentence the
    /// outcome at Stop will carry rather than a second wording of it.
    ///
    /// `None` while the transcription is running, and also for the one ending
    /// that has no sentence: a pump task that panicked or was aborted, which
    /// [`live_state`] still reports as
    /// [`MeetingState::RecordingWithoutTranscript`] because a state with no
    /// explanation is closer to the truth than a claim that the transcript is
    /// running.
    pub transcript_failure: Option<String>,
    /// The provider in use, while there is one.
    pub provider: Option<String>,
    pub elapsed_seconds: f64,
    pub sources: Option<Sources>,
    /// What this platform can do about system audio, whether or not a meeting is
    /// running.
    pub system_audio: SystemAudioSupport,
    /// Whether microphone activity can be read, from the last poll.
    ///
    /// `None` until the first poll has run. Reported rather than folded into
    /// `detected`, because "Teams is open and not in a call" and "Teams is open
    /// and nobody looked" are the same sentence with opposite meanings, and on
    /// Linux and macOS it is always the second one.
    pub microphone: Option<MicrophoneSupport>,
    /// Meeting applications seen running at the last poll.
    ///
    /// Everything found, not only what would prompt: the screen shows "Teams is
    /// open" even though Teams cannot be prompted for, because that is more
    /// useful than silence and it is where the reason lives.
    pub detected: Vec<Detected>,
    /// Whether a detected meeting offers to record.
    pub prompt_on_detect: bool,
    /// Whether the notification window is confirmed absent from screen shares.
    ///
    /// Reported rather than assumed. On a platform where exclusion fails the
    /// window still shows: an indicator visible in a screen share is a cost the
    /// user can be told about, and no indicator at all is the thing the brief
    /// forbids.
    pub notification_excluded: Option<CaptureExclusion>,
}

/// An offer to record a meeting that appears to be happening.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Offer {
    /// The `meeting-detect:*` id.
    pub provider: String,
    /// What to call it on screen.
    pub name: String,
    /// The process that gave it away, so the offer can say why it appeared.
    pub evidence: String,
    /// Which signal fired, so the offer can say how sure it is.
    ///
    /// Never [`CallSignal::None`]: `consider` only offers on a call. Carried as
    /// the enum rather than as a sentence because the wording belongs on the
    /// screen, and because a microphone is the weaker of the two -- an offer
    /// that appeared because Teams was running its own audio test should say
    /// that is what it saw.
    pub signal: CallSignal,
}

impl From<&Detected> for Offer {
    fn from(detected: &Detected) -> Self {
        Self {
            provider: detected.provider.to_owned(),
            name: detected.name.to_owned(),
            evidence: detected.evidence.clone(),
            signal: detected.signal,
        }
    }
}

/// How a meeting ended.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(
    tag = "outcome",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum MeetingOutcome {
    Saved {
        note_id: i64,
        duration_seconds: f64,
        /// Where the recording is, so the note is not the only copy.
        recording_path: String,
        /// Non-zero when audio was never transcribed. A transcript with a hole in
        /// it must not look complete.
        gaps: u32,
        /// True when the whole-file pass failed and the overlapping live
        /// partials were saved instead.
        transcript_is_partials: bool,
    },
    /// Nothing was transcribed, and what the two captures delivered.
    ///
    /// The captures are here because "nothing was said" is only one of the
    /// things that end up on this arm, and until KNEG-38 the screen could not
    /// tell them apart. A microphone that delivered audible audio and produced
    /// no transcript is not a quiet room; a system capture that delivered
    /// nothing at all is the ordinary shape of a call where nobody else spoke,
    /// *and* the shape of a loopback that never worked. Naming which happened
    /// is possible; separating the last two is not, and
    /// [`crate::meeting::Delivered`] says so rather than guessing.
    NothingTranscribed {
        recording_path: String,
        mic: Delivered,
        system: Delivered,
    },
    Cancelled,
    Failed {
        message: String,
    },
}

/* ------------------------------------------------------------ the recorder */

/// Audio drained from both captures at one moment, mono.
///
/// At a rate the *recorder* chose, not the provider's. The recorder cannot know
/// what the session wants — OpenAI Realtime takes 24 kHz and whisper 16 — so it
/// brings both devices to one rate, says which, and leaves the last conversion
/// to the pump, which knows.
#[derive(Debug)]
struct Drained {
    mic: Vec<f32>,
    system: Vec<f32>,
    /// The rate `mic` and `system` are at.
    sample_rate: u32,
}

enum RecorderCommand {
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
struct Finished {
    path: std::path::PathBuf,
    duration_seconds: f64,
    /// Samples that exceeded full scale and were clamped by the encoder. See
    /// [`MixFeed`] on why this is counted rather than corrected.
    clipped: u64,
    /// What the microphone actually delivered. See [`Delivered`].
    mic_delivered: Delivered,
    /// What the system-audio capture actually delivered. See [`Delivered`].
    system_delivered: Delivered,
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
enum RecorderEnd {
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
struct RecorderExit(Arc<OnceLock<RecorderEnd>>);

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
/// A named type because the tuple grew a fourth member and clippy is right that
/// four is where one stops reading it: the audio stream, the command channel,
/// what actually opened, and the thread's own account of how it ended.
type Recorder = (
    mpsc::UnboundedReceiver<Drained>,
    sync_mpsc::Sender<RecorderCommand>,
    Sources,
    Arc<OnceLock<RecorderEnd>>,
);

/// Start the capture thread.
///
/// Returns the audio channel and the command channel. Both captures are started
/// here, on the thread, because `cpal::Stream` is not `Send`.
fn spawn_recorder(path: std::path::PathBuf, system: bool) -> Result<Recorder, String> {
    let (audio_tx, audio_rx) = mpsc::unbounded_channel::<Drained>();
    let (command_tx, command_rx) = sync_mpsc::channel::<RecorderCommand>();
    // The thread reports whether it got its devices before anything else
    // happens, so a failure to open the microphone is a failure to start rather
    // than a meeting that records nothing.
    let (ready_tx, ready_rx) = sync_mpsc::channel::<Result<Sources, String>>();
    // The thread's own account of how it ended, rather than the caller's
    // inference from a closed channel. See [`RecorderEnd`].
    let ended = Arc::new(OnceLock::new());

    std::thread::Builder::new()
        .name("kw-meeting".to_owned())
        .spawn({
            let ended = Arc::clone(&ended);
            move || record(path, system, audio_tx, command_rx, ready_tx, &ended)
        })
        .map_err(|e| format!("could not start the recording thread: {e}"))?;

    let sources = ready_rx
        .recv()
        .map_err(|_| "the recording thread stopped before it started".to_owned())??;

    Ok((audio_rx, command_tx, sources, ended))
}

/// The recorder thread.
fn record(
    path: std::path::PathBuf,
    want_system: bool,
    audio: mpsc::UnboundedSender<Drained>,
    commands: sync_mpsc::Receiver<RecorderCommand>,
    ready: sync_mpsc::Sender<Result<Sources, String>>,
    ended: &Arc<OnceLock<RecorderEnd>>,
) {
    // First, so it is the last local dropped and therefore runs before `audio`
    // does. Every `return` below and every unwind goes through it.
    let _exit = RecorderExit(Arc::clone(ended));

    let mic = match Capture::start(None) {
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

    let _ = ready.send(Ok(Sources {
        microphone: true,
        system_audio: system.is_some(),
        system_audio_reason: system_reason,
    }));

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

/* -------------------------------------------------------------- the watcher */

/// Decides what to offer, and remembers what it has already offered.
///
/// Pure, and separate from the polling for that reason: "does this prompt, and
/// has it prompted already" is the whole of the behaviour worth testing, and none
/// of it needs a process table.
#[derive(Debug, Default)]
pub struct Watcher {
    /// Providers already offered and not yet gone away.
    ///
    /// Keyed on the provider rather than on a timestamp: an offer dismissed
    /// during a two-hour call must stay dismissed for that call, and must come
    /// back for the next one. A time-based suppression would either re-prompt
    /// mid-meeting or swallow the next meeting, depending which way it was tuned.
    offered: std::collections::BTreeSet<String>,
}

impl Watcher {
    /// What to offer for this poll, if anything.
    ///
    /// Returns at most one offer. Two meetings at once is possible and offering
    /// both would stack windows; the first is the one to ask about.
    pub fn consider(&mut self, detected: &[Detected]) -> Option<Offer> {
        // Anything that has stopped running can be offered again next time.
        let live: std::collections::BTreeSet<String> =
            detected.iter().map(|d| d.provider.to_owned()).collect();
        self.offered.retain(|provider| live.contains(provider));

        // Only a meeting actually in progress. Prompting on "the client is open"
        // would fire on every launch for most corporate machines -- and for
        // Teams, which has no in-call process, it would fire from login until
        // shutdown. What makes Teams offerable at all is the microphone signal
        // measured on 2026-09-01; see `meeting_detect`'s module documentation.
        //
        // And the first one *not already offered*, not simply the first. Finding
        // the first in-call provider and then discarding it for being a repeat
        // meant a second meeting running alongside the first was never offered at
        // all -- silently, which is the part that matters. A test caught it.
        let candidate = detected
            .iter()
            .find(|d| d.in_call() && !self.offered.contains(d.provider))?;

        self.offered.insert(candidate.provider.to_owned());
        Some(Offer::from(candidate))
    }

    /// Forget that a provider was offered, so it can be offered again.
    ///
    /// Used when a meeting *ends*: having recorded one Zoom call should not stop
    /// the next one being offered, even if Zoom never quit in between.
    pub fn forget(&mut self, provider: &str) {
        self.offered.remove(provider);
    }

    /// Whether `provider` has an outstanding offer.
    ///
    /// Test-only. The runtime never asks — it acts on what `consider` returns —
    /// and a `pub` method nothing in the binary calls is scaffolding. Kept
    /// because asserting that a suppression was *released* reads better than
    /// inferring it from a second call.
    #[cfg(test)]
    #[must_use]
    pub fn has_offered(&self, provider: &str) -> bool {
        self.offered.contains(provider)
    }
}

/// What the last detection poll saw.
///
/// A struct rather than the tuple this was, because it grew a third member and
/// `held.2` is not a name.
#[derive(Debug, Default)]
struct WatchState {
    /// Which providers have already been offered.
    watcher: Watcher,
    /// Everything running at the last poll, offerable or not.
    detected: Vec<Detected>,
    /// Whether microphone activity could be read at the last poll.
    ///
    /// `None` before the first one. Not defaulted to "unsupported": that would
    /// be a measurement, and no poll has happened yet.
    microphone: Option<MicrophoneSupport>,
}

/* -------------------------------------------------------------- the runtime */

/// One meeting at a time, and everything it needs.
pub struct MeetingRuntime {
    app: AppHandle,
    db: Db,
    policy: NetworkPolicy,
    /// Resolved only at the moment a cloud socket is opened. Nothing in the
    /// meeting configuration holds a secret.
    credentials: Arc<dyn kw_security::CredentialStore>,
    /// Where the local model server is listening, and what it loaded.
    local: crate::local_model::SharedLocal,
    /// Which sherpa model is selected for dictation, if any.
    ///
    /// Read by the whole-recording pass so it uses the same engine dictation
    /// would. Not for meetings -- a meeting's *live* provider is chosen on
    /// `Settings > Meetings` and has nothing to do with this.
    sherpa: crate::local_asr::Selection,
    /// The streaming model server, started on demand for `local-streaming`.
    ///
    /// The whole supervisor rather than a `SharedStream`, unlike `local` above:
    /// this one is *started* from here, because a 630 MB model held from launch
    /// for a user who never records a meeting is a cost nobody asked for. The
    /// windowed provider's server is started from its own settings screen and
    /// only read from here.
    stream: Arc<crate::sherpa_stream::SherpaStream>,
    /// The session in progress.
    ///
    /// Behind an `Arc` so the pump's own task can hold it too, and **the lock
    /// is the arbiter of who gets to announce that the live transcription has
    /// ended**. [`MeetingRuntime::stop`] and [`MeetingRuntime::cancel`] take
    /// the session out under it, so "there is still a session here" is exactly
    /// "stopping has not begun" -- one fact rather than a second flag that
    /// could drift out of step with it. See [`announce_transcript_ended`].
    live: Arc<Mutex<Option<Live>>>,
    /// Detection state: what was last seen, and what has been offered.
    watch: Mutex<WatchState>,
}

/// A meeting in progress.
struct Live {
    provider_id: String,
    commands: sync_mpsc::Sender<RecorderCommand>,
    pump: tokio::task::JoinHandle<Result<MeetingTranscript, MeetingError>>,
    /// Where the WAV is being written.
    ///
    /// Kept here as well as in `Finished` because the two paths that never
    /// reach a `Finished` -- the recorder thread having stopped, and its not
    /// answering -- still have to tell the user where the audio went. The
    /// recording is the artefact that cannot be recreated.
    recording_path: std::path::PathBuf,
    sources: Sources,
    started: std::time::Instant,
    /// How the recorder thread ended, once it has.
    ///
    /// Written by the thread itself rather than inferred from its audio channel
    /// closing, which is KNEG-116: the two look identical from outside and mean
    /// different things. Read beside `transcript_failure` by [`live_state`],
    /// because "is anything still being recorded" and "is anything still being
    /// transcribed" are separate questions and were being answered by one
    /// signal.
    recorder_end: Arc<OnceLock<RecorderEnd>>,

    /// Why the live transcription stopped, once it has.
    ///
    /// Written by the pump's own task before that task returns, so `pump` above
    /// cannot report `is_finished` before this is set. That ordering is what
    /// makes "finished, and nothing here" mean exactly one thing -- see
    /// [`live_state`].
    ///
    /// A `OnceLock` for the reason [`kw_meeting_core::fault::SessionFault`]
    /// uses one: the first write wins by construction rather than by
    /// convention, and there is no poisoned-mutex branch to write around this
    /// workspace's `unwrap_used = "deny"`.
    ///
    /// It is also this session's identity. Two sessions can exist at once for a
    /// moment -- `stop` takes `Live` out and then spends minutes on the
    /// whole-file pass, during which `start` will happily begin another -- so
    /// the ending task compares the cell it was given with the one it found
    /// rather than trusting that a session it can see is its own.
    transcript_failure: Arc<OnceLock<String>>,
}

impl std::fmt::Debug for MeetingRuntime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MeetingRuntime").finish_non_exhaustive()
    }
}

/// Resolve the credential reference `client` should connect with, refusing
/// with a message naming the settings screen when its slot has nothing
/// stored.
///
/// A free function taking `&dyn CredentialStore` rather than a
/// `MeetingRuntime` method, so it can be unit-tested against a
/// `MemoryCredentialStore` with no `AppHandle` in sight. It only ever calls
/// [`kw_security::CredentialStore::contains`], never `get`: this is the
/// controller deciding whether to proceed, not the moment a secret is
/// needed, so it must never hold one.
///
/// This is the fix for a real defect: `start` used to build its
/// `MeetingProviderConfig` with `key_ref: None` unconditionally, so
/// `openai::token` refused every OpenAI Realtime connection attempt no matter
/// what was stored, and the failure only ever surfaced from inside `connect`
/// -- a socket error, not this message.
fn resolve_key_ref(
    provider_id: &str,
    client: Client,
    credentials: &dyn kw_security::CredentialStore,
) -> Result<Option<kw_security::KeyRef>, String> {
    let Some(slot) = kw_meeting_core::credential_slot(client) else {
        return Ok(None);
    };

    let key_ref = kw_security::KeyRef::new(slot)
        .ok_or_else(|| format!("the credential slot {slot:?} for this provider is not valid"))?;

    if !credentials.contains(&key_ref) {
        let name = kw_meeting_core::capabilities(provider_id).map_or(provider_id, |c| c.name);
        return Err(format!(
            "no {name} key is stored. Add one under Settings > Providers."
        ));
    }

    Ok(Some(key_ref))
}

/// Resolve `provider_id` to the [`Client`] that implements it, refusing with
/// the declined reason or a generic "no client" message when there is none.
///
/// A free function for the same reason `resolve_key_ref` is: `start` needs
/// the result before it can look up a credential slot, so pulling it out of
/// `provider` keeps that lookup from happening twice.
fn resolve_client(provider_id: &str) -> Result<Client, String> {
    kw_meeting_core::client(provider_id).ok_or_else(|| {
        match kw_meeting_core::declined(provider_id) {
            Some(reason) => reason.to_owned(),
            None => format!("there is no meeting client for {provider_id:?}"),
        }
    })
}

/// Corti's environment and tenant, from the same settings the batch
/// transcription client reads -- reused rather than duplicated, because they
/// are properties of the Corti tenancy, not of the meeting feature. Refuses
/// by name when either is missing; see
/// [`kw_meeting_core::corti::scope`]'s doc comment for why this mirrors
/// `route.rs`'s `required` rather than defaulting.
///
/// A free function taking `&RouteSettings` rather than reading the database
/// itself, for the same reason `resolve_key_ref` and `resolve_client` are
/// free functions: the refusal can then be tested with no `AppHandle` and no
/// database in sight.
fn resolve_corti_scope(
    settings: &kw_transcription_core::RouteSettings,
) -> Result<(String, String), String> {
    kw_meeting_core::corti::scope(
        settings.corti_environment.as_deref().unwrap_or(""),
        settings.corti_tenant.as_deref().unwrap_or(""),
    )
    .map_err(|e| e.to_string())
}

/// A language setting normalised to what [`MeetingProviderConfig`] wants: a
/// blank field becomes `None` rather than the literal empty string, so
/// [`kw_meeting_core::validate`] reads it the same way whether the setting was
/// never written or was written and cleared.
fn normalize_language(language: &str) -> Option<String> {
    let trimmed = language.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_owned())
    }
}

/// Keyterms normalised the same way: each one trimmed, and blanks dropped so a
/// stray empty line from the editor does not count against a provider's cap.
fn normalize_keyterms(keyterms: &[String]) -> Vec<String> {
    keyterms
        .iter()
        .map(|k| k.trim().to_owned())
        .filter(|k| !k.is_empty())
        .collect()
}

/// Build the config a settings-screen preview should see for `provider_id`,
/// from raw form values.
///
/// A free function, taking the draft values directly rather than reading them
/// from settings, so the screen can preview a value before it is saved and so
/// this can be tested with no database in sight. Whether the result needs a
/// language is [`kw_meeting_core::validate`]'s answer to give, not this
/// function's -- it only normalises and looks up the provider's declared
/// sample rate, the same way [`MeetingRuntime::start`] does.
fn draft_config(
    provider_id: &str,
    language: &str,
    keyterms: &[String],
) -> Result<MeetingProviderConfig, String> {
    let caps = kw_meeting_core::capabilities(provider_id)
        .ok_or_else(|| format!("{provider_id:?} is not a described meeting provider"))?;
    let sample_rate = *caps.sample_rates.first().ok_or_else(|| {
        format!(
            "{} declares no sample rate, so nothing can be reviewed for it",
            caps.name
        )
    })?;

    Ok(MeetingProviderConfig {
        provider: provider_id.to_owned(),
        model: None,
        language: normalize_language(language),
        keyterms: normalize_keyterms(keyterms),
        key_ref: None,
        sample_rate,
    })
}

/// Build the config [`MeetingTranscriptionProvider::connect`] will use,
/// refusing first if `caps` needs a language and `language` gives it none.
///
/// A free function taking the provider's capabilities and the raw settings
/// values rather than reading them itself, for the same reason
/// `resolve_key_ref` is one: `start`'s refusal can then be tested with neither
/// a database nor an `AppHandle` in sight. [`kw_meeting_core::validate`] is
/// the one place the keyterm and language arithmetic lives; this only acts on
/// what it reports, and never picks a language on its own -- a guessed
/// language is a worse transcript that looks like a working one.
fn resolve_provider_config(
    provider_id: &str,
    caps: &kw_meeting_core::MeetingProviderCapabilities,
    key_ref: Option<kw_security::KeyRef>,
    sample_rate: u32,
    language: &str,
    keyterms: &[String],
) -> Result<MeetingProviderConfig, String> {
    let config = MeetingProviderConfig {
        provider: provider_id.to_owned(),
        model: None,
        language: normalize_language(language),
        keyterms: normalize_keyterms(keyterms),
        key_ref,
        sample_rate,
    };

    let review = kw_meeting_core::validate(&config).map_err(|e| e.to_string())?;
    if review.language_required {
        return Err(format!(
            "{} needs a concrete language, and none is set. Add one under Settings > Meetings.",
            caps.name
        ));
    }

    Ok(config)
}

impl MeetingRuntime {
    #[must_use]
    pub fn new(
        app: AppHandle,
        db: Db,
        policy: NetworkPolicy,
        credentials: Arc<dyn kw_security::CredentialStore>,
        local: crate::local_model::SharedLocal,
        sherpa: crate::local_asr::Selection,
        stream: Arc<crate::sherpa_stream::SherpaStream>,
    ) -> Self {
        Self {
            app,
            db,
            policy,
            credentials,
            live: Arc::new(Mutex::new(None)),
            local,
            sherpa,
            stream,
            watch: Mutex::new(WatchState::default()),
        }
    }

    /// Poll for meetings and offer to record one, once per meeting.
    ///
    /// Called on a timer. Returns nothing: an offer is delivered by showing the
    /// notification window, and there is deliberately no path from here to
    /// [`Self::start`].
    pub async fn poll_for_meetings(&self) {
        // Nothing to detect while recording, and nothing to offer either.
        if self.live.lock().await.is_some() {
            return;
        }

        let lists: Lists =
            kw_storage::settings::get_or(self.db.pool(), LISTS_KEY, Lists::default())
                .await
                .unwrap_or_default();
        let prompt = kw_storage::settings::get_or(self.db.pool(), PROMPT_KEY, PROMPT_BY_DEFAULT)
            .await
            .unwrap_or(PROMPT_BY_DEFAULT);

        // Both readings from one process table, so the capture list and the
        // process list are the same instant.
        let survey = meeting_detect::survey();
        let detected = meeting_detect::detect(&survey.running, &survey.capturing, &lists);

        let offer = {
            let mut held = self.watch.lock().await;
            held.detected = detected.clone();
            held.microphone = Some(survey.microphone);
            // `consider` runs whether or not prompting is on, so its
            // already-offered bookkeeping stays correct if the setting is turned
            // on mid-call -- otherwise switching it on would immediately prompt
            // for a meeting that started an hour ago.
            let offer = held.watcher.consider(&detected);
            if prompt {
                offer
            } else {
                None
            }
        };

        if let Some(offer) = offer {
            tracing::info!(
                provider = %offer.provider,
                evidence = %offer.evidence,
                "a meeting appears to be in progress; offering to record it"
            );
            if let Err(err) = self.app.emit(OFFER_EVENT, &offer) {
                tracing::warn!(error = %err, "could not deliver the recording offer");
            }
            crate::meeting_window::set_visible(&self.app, true);
        }
    }

    /// Dismiss the outstanding offer without recording.
    ///
    /// The provider stays suppressed until its process goes away, which is what
    /// stops a dismissed offer coming back five seconds later.
    pub async fn dismiss(&self) {
        crate::meeting_window::set_visible(&self.app, false);
        if let Err(err) = self.app.emit(OFFER_EVENT, Option::<Offer>::None) {
            tracing::warn!(error = %err, "could not clear the recording offer");
        }
    }

    /// Start the detection timer.
    ///
    /// Spawned once at startup and runs for the life of the process. The interval
    /// ticks regardless of how long a poll took, and a poll that overran simply
    /// makes the next one immediate rather than accumulating.
    ///
    /// `tauri::async_runtime::spawn`, not `tokio::spawn`. This is called from
    /// the `setup` hook, which runs on the main thread with no Tokio runtime
    /// entered, and bare `tokio::spawn` panics there -- taking the whole
    /// process down before the first window is usable. Every other background
    /// task in this crate already spawns this way; this one did not, and no
    /// test caught it because the panic needs a real Tauri app to happen in.
    pub fn watch(self: &Arc<Self>) {
        let runtime = Arc::clone(self);
        tauri::async_runtime::spawn(async move {
            let mut ticker = tokio::time::interval(DETECT_INTERVAL);
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                ticker.tick().await;
                runtime.poll_for_meetings().await;
            }
        });
    }

    /// Where recordings are kept.
    fn recording_dir() -> std::path::PathBuf {
        kw_storage::paths::data_dir().join("meetings")
    }

    /// Build the provider for an already-resolved `client`.
    ///
    /// An exhaustive match on [`Client`], which is the point of it being data: a
    /// new client stops this compiling until it says how to build one, and a
    /// provider with no client cannot reach here at all. Resolving the id to a
    /// `Client` is the caller's job now, because `start` also needs it to look
    /// up the credential slot before this is called.
    async fn provider(
        &self,
        client: Client,
        events: mpsc::UnboundedSender<TranscriptEvent>,
    ) -> Result<Box<dyn MeetingTranscriptionProvider>, String> {
        match client {
            Client::LocalWindow => {
                let port =
                    *self.local.port.read().map_err(|_| {
                        "the local model server's state could not be read".to_owned()
                    })?;
                let port = port.ok_or_else(|| {
                    "the local model server is not running. Start a model under Settings > \
                     Transcription > Local first."
                        .to_owned()
                })?;
                let model = self
                    .local
                    .model
                    .read()
                    .map_err(|_| "the local model could not be read".to_owned())?
                    .clone();

                let server = kw_transcription_core::local::WhisperServer::new(
                    self.policy.clone(),
                    port,
                    model,
                );
                Ok(Box::new(
                    LocalWindowProvider::new(Arc::new(server)).with_live_events(events),
                ))
            }
            Client::LocalStreaming => {
                // Started here rather than at launch: the model is 630 MB in
                // memory and a user who never records a meeting should never
                // pay for it. `start` is a no-op when the same model is
                // already up, which is what makes calling it per meeting
                // cheap.
                let model_id = kw_storage::settings::get_or(
                    self.db.pool(),
                    crate::sherpa_stream::STREAM_MODEL_KEY,
                    String::new(),
                )
                .await
                .unwrap_or_default();
                if model_id.is_empty() {
                    return Err("no streaming model is chosen. Pick one under Settings > \
                                Transcription > Local."
                        .to_owned());
                }

                let port = self.stream.start(&model_id).await?;
                Ok(Box::new(
                    kw_meeting_core::nemotron_socket::LocalStreaming::new(
                        self.policy.clone(),
                        port,
                    )
                    .with_live_events(events),
                ))
            }
            Client::OpenAiRealtime => Ok(Box::new(
                kw_meeting_core::openai_socket::OpenAiRealtime::new(Arc::clone(&self.credentials))
                    .with_live_events(events),
            )),
            Client::AssemblyAi => Ok(Box::new(
                kw_meeting_core::assemblyai_socket::AssemblyAiRealtime::new(Arc::clone(
                    &self.credentials,
                ))
                .with_live_events(events),
            )),
            Client::DeepgramRealtime => Ok(Box::new(
                kw_meeting_core::deepgram_socket::DeepgramRealtime::new(Arc::clone(
                    &self.credentials,
                ))
                .with_live_events(events),
            )),
            Client::CortiRealtime => {
                // Read from the same settings the batch Corti client uses,
                // not a second pair of meeting-only ones -- see
                // `kw_meeting_core::corti`'s module documentation.
                // `resolve_corti_scope` refuses by name when either is
                // missing, exactly like `route.rs`'s `required` does for the
                // same reason.
                let settings: kw_transcription_core::RouteSettings =
                    kw_storage::settings::get(self.db.pool(), crate::dictation::ROUTE_SETTINGS_KEY)
                        .await
                        .ok()
                        .flatten()
                        .unwrap_or_default();
                let (environment, tenant) = resolve_corti_scope(&settings)?;

                Ok(Box::new(
                    kw_meeting_core::corti_socket::CortiRealtime::new(
                        Arc::clone(&self.credentials),
                        self.policy.clone(),
                        environment,
                        tenant,
                    )
                    .with_live_events(events),
                ))
            }
        }
    }

    fn emit_state(&self, state: MeetingState) {
        emit_state(&self.app, state);
    }

    /// Begin a meeting.
    pub async fn start(&self, provider_id: &str, system_audio: bool) -> Result<Sources, String> {
        let mut held = self.live.lock().await;
        if held.is_some() {
            return Err("a meeting is already being recorded".to_owned());
        }

        let client = resolve_client(provider_id)?;

        // Fail before anything is opened, not three seconds later at the
        // socket. See `resolve_key_ref`'s doc comment for the defect this
        // replaces.
        let key_ref = resolve_key_ref(provider_id, client, self.credentials.as_ref())?;

        let (events_tx, events_rx) = mpsc::unbounded_channel();
        let mut provider = self.provider(client, events_tx).await?;

        let caps = provider.capabilities();
        // The provider's own first declared rate, and the pump converts to it.
        // This used to be hardcoded to 16 kHz, which meant OpenAI Realtime -- 24
        // kHz only -- could never have connected at all: `connect` would have
        // refused every time, correctly, and the client would have looked broken.
        let session_rate = *caps.sample_rates.first().ok_or_else(|| {
            format!(
                "{} declares no sample rate, so nothing can be sent to it",
                caps.name
            )
        })?;

        let language: String =
            kw_storage::settings::get_or(self.db.pool(), LANGUAGE_KEY, String::new())
                .await
                .unwrap_or_default();
        let keyterms: Vec<String> =
            kw_storage::settings::get_or(self.db.pool(), KEYTERMS_KEY, Vec::new())
                .await
                .unwrap_or_default();

        // Refuses here, before anything is opened, when the provider cannot
        // detect a language and none is set -- see `resolve_provider_config`'s
        // doc comment. A guessed language would be a worse transcript that
        // looks like a working one, so this never picks one on its own.
        let config = resolve_provider_config(
            provider_id,
            caps,
            key_ref,
            session_rate,
            &language,
            &keyterms,
        )?;
        provider.connect(config).await.map_err(|e| e.to_string())?;

        let path = Self::recording_dir().join(format!(
            "{}.wav",
            chrono::Utc::now().format("%Y%m%d-%H%M%S")
        ));
        let (audio, commands, sources, recorder_end) = spawn_recorder(path.clone(), system_audio)?;

        // The pump announces its own ending, which is the whole of KNEG-95's
        // fix: until now the failure was detected at second zero and reported
        // whenever the user happened to press Stop. What it must not announce
        // is the ending Stop asked for -- and since a provider that faults on
        // the final frame ends the pump *during* `finish_recording`, which can
        // run for minutes, that is the common case rather than a rare race.
        // `announce_transcript_ended` decides it under the `live` lock.
        let transcript_failure = Arc::new(OnceLock::new());
        let pump = tokio::spawn({
            let app = self.app.clone();
            let live = Arc::clone(&self.live);
            let mine = Arc::clone(&transcript_failure);
            let ended = Arc::clone(&recorder_end);
            async move {
                let result = pump(provider, session_rate, audio, events_rx, app.clone()).await;
                // Read after the pump has ended, which is after the audio
                // channel closed, which is after the recorder's own guard ran.
                // See [`RecorderExit`] for why that ordering holds.
                let reason =
                    live_transcript_ended(result.as_ref().map(|_| ()), ended.get().copied());
                announce_transcript_ended(&live, &app, &mine, reason).await;
                result
            }
        });

        *held = Some(Live {
            provider_id: provider_id.to_owned(),
            commands,
            pump,
            recorder_end,
            recording_path: path,
            sources: sources.clone(),
            started: std::time::Instant::now(),
            transcript_failure,
        });

        tracing::info!(
            provider = provider_id,
            system_audio = sources.system_audio,
            "meeting started"
        );
        self.emit_state(MeetingState::Recording);
        // The window changes from an offer to an indicator, and stays up. A
        // recording with nothing on screen is what the brief forbids.
        crate::meeting_window::set_visible(&self.app, true);

        Ok(sources)
    }

    /// Stop, transcribe the recording, and keep a note.
    ///
    /// **Everything below the `take` runs, whatever goes wrong.** This used to
    /// `?` out of three places -- the recorder thread having stopped, the
    /// recorder not answering, and the pump failing -- each of which left `Live`
    /// already taken and skipped the whole tail: no `OUTCOME_EVENT`, no
    /// `MeetingState::Idle`, no `meeting_window::set_visible(false)`, no
    /// `watch.forget`. The runtime was idle and the screen still said
    /// "Recording", with nothing left to press: `guard` in `useRefresh.ts` does
    /// not reload when a mutation throws, so the frontend stayed on `finishing`,
    /// where `SettingsMeetings.tsx` renders no Stop or Discard and disables
    /// Start. Only restarting the application cleared it.
    ///
    /// A failure is an outcome now, not an early return -- `MeetingOutcome::
    /// Failed`, naming the WAV, which is already on disk by the time any of this
    /// can go wrong and is the one artefact that cannot be recreated.
    pub async fn stop(&self) -> Result<MeetingOutcome, String> {
        let Some(live) = self.live.lock().await.take() else {
            return Err("no meeting is being recorded".to_owned());
        };
        let Live {
            provider_id,
            commands,
            pump,
            recording_path,
            ..
        } = live;

        self.emit_state(MeetingState::Finishing);

        let outcome = self
            .finish_recording(&provider_id, &commands, pump, &recording_path)
            .await;

        self.emit_state(MeetingState::Idle);
        crate::meeting_window::set_visible(&self.app, false);
        // Recording one Zoom call must not stop the next being offered, even if
        // Zoom never quit in between.
        self.watch.lock().await.watcher.forget(&provider_id);

        if let Err(err) = self.app.emit(OUTCOME_EVENT, &outcome) {
            tracing::warn!(error = %err, "could not report the meeting outcome");
        }
        Ok(outcome)
    }

    /// Close the recorder, wait for the pump, and turn both into one outcome.
    ///
    /// Split out of [`MeetingRuntime::stop`] precisely so that `stop`'s tail
    /// runs on every path through here. Nothing in this function returns an
    /// `Err`: every failure it can meet becomes a `MeetingOutcome`.
    async fn finish_recording(
        &self,
        provider_id: &str,
        commands: &sync_mpsc::Sender<RecorderCommand>,
        pump: tokio::task::JoinHandle<Result<MeetingTranscript, MeetingError>>,
        recording_path: &std::path::Path,
    ) -> MeetingOutcome {
        let path = recording_path.display().to_string();
        let closed = close_recorder(commands);

        // Awaited on every path, including the two failures above it. The
        // recorder thread has gone either way, so it has dropped its audio
        // sender, the pump sees the channel close, finishes the provider and
        // returns. Aborting instead would leave two sockets open until the
        // process noticed.
        let live_transcript = pump_result(pump.await);

        let finished = match closed {
            Ok(finished) => finished,
            Err(reason) => {
                tracing::error!(provider = provider_id, %reason, "the recording could not be closed");
                return MeetingOutcome::Failed {
                    message: failed_message(&path, &reason),
                };
            }
        };

        match live_transcript {
            Ok(transcript) => self.keep(provider_id, finished, transcript, None).await,
            Err(reason) => {
                // Still `keep`, rather than returning here. The whole-file local
                // pass has not run yet and it is what produces the saved note --
                // the live transcript is only ever its fallback. A cloud
                // provider that died at second zero costs the live transcript,
                // not the meeting. Skipping the pass entirely, which is what the
                // old `?` did, was a pure loss.
                tracing::error!(provider = provider_id, %reason, "the live transcription failed");
                self.keep(
                    provider_id,
                    finished,
                    MeetingTranscript::default(),
                    Some(&reason),
                )
                .await
            }
        }
    }

    /// Abandon the meeting and delete the recording.
    pub async fn cancel(&self) -> Result<(), String> {
        let Some(live) = self.live.lock().await.take() else {
            return Err("no meeting is being recorded".to_owned());
        };

        let _ = live.commands.send(RecorderCommand::Cancel);
        live.pump.abort();
        self.emit_state(MeetingState::Idle);
        crate::meeting_window::set_visible(&self.app, false);
        self.watch.lock().await.watcher.forget(&live.provider_id);

        if let Err(err) = self.app.emit(OUTCOME_EVENT, MeetingOutcome::Cancelled) {
            tracing::warn!(error = %err, "could not report the cancellation");
        }
        Ok(())
    }

    /// What is happening, for the screen.
    pub async fn status(&self) -> MeetingStatus {
        let held = self.live.lock().await;
        let system_audio = kw_audio_core::system_audio::support();
        let (detected, microphone) = {
            let held = self.watch.lock().await;
            (held.detected.clone(), held.microphone.clone())
        };
        let prompt_on_detect =
            kw_storage::settings::get_or(self.db.pool(), PROMPT_KEY, PROMPT_BY_DEFAULT)
                .await
                .unwrap_or(PROMPT_BY_DEFAULT);

        // Asked of the window rather than remembered, so the screen reports what
        // the platform actually did rather than what was attempted at startup.
        let notification_excluded = self
            .app
            .get_webview_window(crate::meeting_window::LABEL)
            .map(|window| crate::meeting_window::apply_treatments(&window).capture_exclusion);

        match held.as_ref() {
            Some(live) => MeetingStatus {
                state: live_state(
                    live.transcript_failure.get().is_some(),
                    live.pump.is_finished(),
                    live.recorder_end.get().copied(),
                ),
                transcript_failure: live.transcript_failure.get().cloned(),
                provider: Some(live.provider_id.clone()),
                elapsed_seconds: live.started.elapsed().as_secs_f64(),
                sources: Some(live.sources.clone()),
                system_audio,
                microphone,
                detected,
                prompt_on_detect,
                notification_excluded,
            },
            None => MeetingStatus {
                state: MeetingState::Idle,
                transcript_failure: None,
                provider: None,
                elapsed_seconds: 0.0,
                sources: None,
                system_audio,
                microphone,
                detected,
                prompt_on_detect,
                notification_excluded,
            },
        }
    }

    /// Transcribe the recording properly and write the note.
    ///
    /// `live_failure` carries why the streaming transcription stopped, when it
    /// did. It changes two things and nothing else: an empty result becomes
    /// `Failed` naming the provider's own words rather than `NothingTranscribed`
    /// -- which is the sentence a genuinely quiet room gets, and the confusion
    /// KNEG-79 is about -- and a note that *was* produced says in its own text
    /// that the live half failed.
    async fn keep(
        &self,
        provider_id: &str,
        finished: Finished,
        transcript: MeetingTranscript,
        live_failure: Option<&str>,
    ) -> MeetingOutcome {
        let path = finished.path.display().to_string();

        if finished.clipped > 0 {
            // Counted rather than corrected -- see `MixFeed`. A number here is
            // what would justify building a limiter.
            tracing::warn!(
                clipped = finished.clipped,
                "the mixed recording exceeded full scale and was clamped"
            );
        }

        // One pass over the whole file, which is the same path a dictation takes
        // and produces a transcript with no overlap to deduplicate.
        let mut speaker_note: Option<String> = None;
        let (segments, from_partials) = match self.transcribe_whole(&finished.path).await {
            Ok(Some(pass)) => {
                let (segments, note) = self.with_speakers(pass, &finished.path).await;
                speaker_note = note;
                (segments, false)
            }
            Ok(None) => (Vec::new(), false),
            Err(err) => {
                // Not fatal, and not silent. The partials overlap and are worse,
                // and a meeting nobody can read would be worse still.
                tracing::warn!(error = %err, "the whole-recording pass failed; keeping the partials");
                (
                    transcript
                        .events
                        .iter()
                        .map(|e| segment(&e.text, e.timestamp, Some(e.source)))
                        .collect(),
                    true,
                )
            }
        };

        if segments.is_empty() {
            // The distinction this whole change exists for. An empty result used
            // to become `NothingTranscribed` whatever caused it, and that is
            // what a quiet room produces -- so a provider that rejected the
            // session and a room with nobody talking arrived on screen as the
            // same sentence. A provider that said something was wrong now says
            // it here instead.
            return match live_failure {
                Some(reason) => MeetingOutcome::Failed {
                    message: failed_message(&path, reason),
                },
                None => MeetingOutcome::NothingTranscribed {
                    recording_path: path,
                    mic: finished.mic_delivered,
                    system: finished.system_delivered,
                },
            };
        }

        let text = segments
            .iter()
            .map(|s| s.text.as_str())
            .collect::<Vec<&str>>()
            .join("\n");

        let mut content = text.clone();
        if from_partials {
            content.push_str(
                "\n\n(The whole-recording pass did not complete, so this is the running \
                 transcript from during the meeting rather than a single pass over the \
                 finished audio.)",
            );
        }
        if let Some(reason) = live_failure {
            // Said in the note rather than only in a log, for the same reason
            // `from_partials` is: a transcript that came about differently from
            // how the reader expects should say so. This one is the good case --
            // the whole-recording pass ran afterwards over the complete audio,
            // so nothing is missing from what follows.
            content.push_str(&format!(
                "\n\n(The live transcription stopped early: {reason} This note is the \
                 whole-recording pass, which ran afterwards over the complete audio.)"
            ));
        }
        if let Some(note) = &speaker_note {
            // Only ever says something went differently from what the setting
            // asked for. A run that produced labels adds nothing here -- the
            // labels are visible in the note itself, and a sentence announcing
            // that a feature worked is noise.
            content.push_str(&format!("\n\n({note})"));
        }
        if transcript.gaps > 0 {
            // Two causes, and `MeetingTranscript::gaps` is one number that does
            // not distinguish them: the local model falling behind the meeting,
            // or a session past the limit on how much transcript is retained.
            // Naming both is honest where naming one was accurate until
            // KNEG-109 gave the count a second source.
            content.push_str(&format!(
                "\n\n({} stretch(es) of audio were never transcribed: either the transcriber \
                 could not keep up with the meeting, or the meeting ran past the limit on how \
                 much transcript one session keeps.)",
                transcript.gaps
            ));
        }

        let note = kw_notes_core::notes::NewNote {
            title: format!(
                "Meeting — {}",
                chrono::Local::now().format("%Y-%m-%d %H:%M")
            ),
            content,
            note_type: Some("meeting".to_owned()),
            folder_id: None,
            transcript: serde_json::to_string(&segments).ok(),
            participants: None,
            audio_duration_seconds: Some(finished.duration_seconds),
        };

        match kw_notes_core::notes::create(self.db.pool(), &note).await {
            Ok(note_id) => MeetingOutcome::Saved {
                note_id,
                duration_seconds: finished.duration_seconds,
                recording_path: path,
                gaps: transcript.gaps,
                transcript_is_partials: from_partials,
            },
            Err(err) => {
                tracing::error!(error = %err, provider = provider_id, "could not save the meeting note");
                MeetingOutcome::Failed {
                    message: format!(
                        "The recording is at {path} but the note could not be saved: {err}"
                    ),
                }
            }
        }
    }

    /// Run the diarizer if it was asked for, and label the pass.
    ///
    /// Failure here is never fatal: a note with no speaker labels is worth far
    /// more than no note, so a diarization that could not run degrades to the
    /// unlabelled transcript **and a sentence saying so**. Silence would be the
    /// defect -- a user who turned the setting on would read an unlabelled note
    /// and conclude the meeting had one speaker.
    async fn with_speakers(
        &self,
        pass: WholePass,
        recording: &std::path::Path,
    ) -> (Vec<kw_notes_core::transcript::Segment>, Option<String>) {
        let wanted = crate::diarization::is_enabled(&self.db).await;
        if !wanted {
            return labelled(&pass, None, false);
        }
        match crate::diarization::turns(recording).await {
            Ok(Some(turns)) => labelled(&pass, Some(&turns), true),
            Ok(None) => {
                let (segments, _) = labelled(&pass, None, false);
                (
                    segments,
                    Some(
                        "Speaker labels are switched on but could not run: the diarization \
                         models or runtime are not installed."
                            .to_owned(),
                    ),
                )
            }
            Err(err) => {
                tracing::warn!(error = %err, "the diarization pass failed; keeping the transcript");
                let (segments, _) = labelled(&pass, None, false);
                (
                    segments,
                    Some(format!("Speaker labels could not be produced: {err}")),
                )
            }
        }
    }

    /// Transcribe the finished recording in one request.
    async fn transcribe_whole(&self, path: &std::path::Path) -> Result<Option<WholePass>, String> {
        use kw_transcription_core::local::Inference;

        // The same engine dictation would use, through the same function, and
        // that is the whole of the 2026-09-01 fix. This used to build a
        // `WhisperServer` directly and fail with "the local model server is not
        // running" whenever a sherpa model was selected -- which is the
        // documented one-local-engine state, so the whole-recording pass was
        // failing for anybody who had chosen Parakeet, and every meeting note
        // silently became the live partials. Found by a person reading a saved
        // note, not by a test.
        let whisper = {
            let port = *self
                .local
                .port
                .read()
                .map_err(|_| "the local model server's state could not be read".to_owned())?;
            let model = self
                .local
                .model
                .read()
                .map_err(|_| "the local model could not be read".to_owned())?
                .clone();
            port.map(|port| {
                kw_transcription_core::local::WhisperServer::new(self.policy.clone(), port, model)
            })
        };

        let engine = crate::dictation::local_engine(&self.sherpa, whisper)?.ok_or_else(|| {
            "no local engine is available: start a model under Settings > Transcription > Local"
                .to_owned()
        })?;

        let wav = std::fs::read(path).map_err(|e| format!("could not read the recording: {e}"))?;

        match engine.transcribe(wav, None, None).await? {
            // Timings are kept when the engine produced them and the note is
            // no worse without them. Until 2026-09-01 both arms collapsed to
            // one string, with a comment saying this pass "is not asked for
            // timings" -- true then, and the reason speaker labels had nothing
            // to attach to. `Text` still arrives from a server started with
            // `--no-timestamps`, so an empty `timings` is an ordinary state
            // rather than a fault, and `labelled` says so on the note.
            Inference::Timed { text, segments } if !text.trim().is_empty() => Ok(Some(WholePass {
                text,
                timings: segments,
            })),
            Inference::Text(text) if !text.trim().is_empty() => Ok(Some(WholePass {
                text,
                timings: Vec::new(),
            })),
            Inference::Text(_) | Inference::Timed { .. } | Inference::Empty => Ok(None),
            Inference::Rejected { status, body } => {
                Err(format!("the local server answered {status}: {body}"))
            }
        }
    }
}

/// Ask the recorder to stop, and wait for what it wrote.
///
/// A free function taking the command channel rather than a `MeetingRuntime`
/// method, for the reason `resolve_key_ref` is one: it can then be reasoned
/// about, and its three failures named, with no `AppHandle` in sight. Each of
/// those three used to be a `?` in `stop`.
fn close_recorder(commands: &sync_mpsc::Sender<RecorderCommand>) -> Result<Finished, String> {
    let (reply_tx, reply_rx) = sync_mpsc::channel();
    if commands
        .send(RecorderCommand::Finish { reply: reply_tx })
        .is_err()
    {
        return Err("the recording thread had already stopped".to_owned());
    }
    match reply_rx.recv() {
        Ok(result) => result,
        Err(_) => Err("the recording thread did not answer".to_owned()),
    }
}

/// What awaiting the pump produced: the live transcript, or one sentence saying
/// why there is not one.
///
/// A free function so the sentence can be checked with no `AppHandle`, no `Db`
/// and no `NetworkPolicy` in sight -- the same reason `resolve_key_ref` is one,
/// and the reason there is no fake `MeetingTranscriptionProvider` here. `pump`
/// itself already propagates its `Err` correctly; what was wrong was what
/// `stop` did with it.
fn pump_result(
    joined: Result<Result<MeetingTranscript, MeetingError>, tokio::task::JoinError>,
) -> Result<MeetingTranscript, String> {
    match joined {
        Ok(Ok(transcript)) => Ok(transcript),
        // The provider's own words. Since `MeetingError::ProviderFault` exists
        // that includes what the provider said was wrong, rather than what our
        // own send call complained about afterwards. Through
        // `live_transcript_ended` so that the sentence the indicator showed
        // mid-meeting and the sentence the outcome ends with are the same one.
        // The recorder is irrelevant on this arm: the error came from the
        // provider, and it is what the user needs to read.
        Ok(Err(err)) => Err(live_transcript_ended(Err(&err), None)),
        // A panic in the pump, or the task being aborted. Neither should happen;
        // both used to leave the screen stuck on "Recording" for ever.
        Err(err) => Err(format!("the transcription task failed: {err}.")),
    }
}

/// The sentence a meeting that could not be transcribed ends with.
///
/// The recording leads, because it is the artefact that cannot be recreated and
/// the note can be rebuilt from it by hand. Same shape as the one failure
/// message that already existed, in `keep`, for a note that could not be saved.
fn failed_message(recording_path: &str, reason: &str) -> String {
    format!(
        "The recording is at {recording_path} but the meeting could not be transcribed: {reason}"
    )
}

/// Tell the screens and the tray what the session is doing.
///
/// A free function because the pump's own task has to emit one too, and it has
/// an `AppHandle` and nothing else. [`MeetingRuntime::emit_state`] is the same
/// call with `self.app` filled in.
fn emit_state(app: &AppHandle, state: MeetingState) {
    crate::tray::meeting(app, state.is_running());

    if let Err(err) = app.emit(STATE_EVENT, state) {
        tracing::warn!(error = %err, "could not report the meeting state");
    }
}

/// Why the live transcription is no longer running, in the words a screen uses.
///
/// One formatter for the two places that need the sentence -- the announcement
/// the moment it happens, and [`pump_result`] at Stop -- so the indicator and
/// the outcome cannot describe one failure two ways. Pure, so the sentence can
/// be checked with no `AppHandle` in sight.
///
/// **The `Ok` arm is the one worth reading twice.** The pump returns `Ok` when
/// the audio channel closes, and at Stop that is exactly what is supposed to
/// happen -- `close_recorder` drops the sender. Reaching here with `Ok` instead
/// means the channel closed with no Stop behind it, and `record` has no path
/// that ends by itself: it returns on `Finish`, on `Cancel`, and on the command
/// channel disconnecting, all three of which run only after `stop` or `cancel`
/// has taken the session. So this arm means the recording thread panicked, and
/// the recording has stopped along with the transcription.
///
/// That is the one case where [`MeetingState::RecordingWithoutTranscript`] is
/// wrong in its first word, and it is reported anyway, because the alternative
/// is the silence this whole change exists to end -- and because the sentence,
/// which is what a person actually reads, says the true thing. Filed as
/// KNEG-116 rather than left here: a fifth state for a case reachable only by a
/// panic was more machinery than the fact justified, and that is a judgement
/// somebody should be able to revisit.
fn live_transcript_ended(
    outcome: Result<(), &MeetingError>,
    recorder: Option<RecorderEnd>,
) -> String {
    match (outcome, recorder) {
        // The thread's own account, which is the sentence this used to have to
        // hedge. Before KNEG-116 every `Ok` arm read "the recording thread
        // stopped" whether it had returned or unwound, because a closed audio
        // channel looks the same either way.
        (Ok(()), Some(RecorderEnd::Panicked)) => {
            "the recording thread stopped unexpectedly, so nothing further is being recorded \
             or transcribed. What was captured before it stopped is still on disk, and Stop \
             will transcribe it."
                .to_owned()
        }
        (Ok(()), Some(RecorderEnd::OnPurpose)) => {
            "the recording finished, so nothing further is being recorded or transcribed."
                .to_owned()
        }
        // The guard has not run yet. Reachable only in the instant between the
        // sender dropping and the thread's frame unwinding past it, and the
        // old wording is right for exactly that: something ended, and this does
        // not yet know what.
        (Ok(()), None) => {
            "the recording thread stopped, so nothing further is being recorded or transcribed."
                .to_owned()
        }
        (Err(err), _) => format!("{err}."),
    }
}

/// What to say about a session that has not been stopped.
///
/// `announced` is whether the pump's ending has been recorded; `pump_finished`
/// is whether the task has ended at all.
///
/// **The second is not redundant, and the ordering is why.** The announcement
/// is written by the pump's own task *before* it returns, so the join handle
/// cannot report `is_finished` first. "Finished, and nothing announced" therefore
/// means exactly one thing: the task panicked, or it was aborted. There is no
/// sentence for that -- a panic has no provider behind it -- and the screen
/// should still not claim a transcript is being produced.
///
/// KNEG-95 named `status()` not consulting `pump.is_finished()` as part of the
/// gap. This is that, narrowed to the case the fault cell cannot cover.
const fn live_state(
    announced: bool,
    pump_finished: bool,
    recorder: Option<RecorderEnd>,
) -> MeetingState {
    // First, because it is the strongest claim available: the thread said so
    // itself. `announced` and `pump_finished` are both about the transcription,
    // and neither can tell whether anything is still being recorded.
    if matches!(recorder, Some(RecorderEnd::Panicked)) {
        return MeetingState::Stalled;
    }
    if announced || pump_finished {
        MeetingState::RecordingWithoutTranscript
    } else {
        MeetingState::Recording
    }
}

/// Whether the session found under the lock is the one this ending belongs to.
///
/// Pure, and the whole of the race it settles. `stop` takes `Live` out and then
/// spends the whole-file pass -- minutes, on a long meeting -- outside the lock,
/// and `start` refuses only while a session is *held*, so a second meeting can
/// begin while the first is still finishing. A pump task that lost the lock to
/// that `start` would otherwise find a session, believe it to be its own, and
/// mark a healthy new meeting as having lost its transcript.
///
/// Identity is the cell itself rather than a counter, because the cell is
/// already per-session and already shared with exactly the task that needs it.
fn is_this_session(found: Option<&Arc<OnceLock<String>>>, mine: &Arc<OnceLock<String>>) -> bool {
    found.is_some_and(|cell| Arc::ptr_eq(cell, mine))
}

/// Say that the live transcription has ended, if the meeting is still running.
///
/// **The lock is the arbiter.** `stop` and `cancel` take the session out under
/// it, so holding it while both deciding and emitting is what keeps this from
/// overwriting `Finishing` with a state the user would then be stuck looking at
/// until the whole-file pass returned. Emitting after releasing the lock would
/// reopen exactly that window.
///
/// No deadlock with `stop`: it takes the session in one statement and drops the
/// guard before it awaits this task, so the lock is free by the time it does.
///
/// `start` holds the guard across its whole body, and that turns out to be the
/// other half of the ordering. A provider that refuses on the first chunk ends
/// the pump while `start` is still running, and this task then waits -- so
/// `start`'s own `emit_state(Recording)` lands first and this one lands after
/// it. Blocked on the lock is the correct place for it to be: the answer is the
/// same one, delivered in an order the screen can follow.
async fn announce_transcript_ended(
    live: &Mutex<Option<Live>>,
    app: &AppHandle,
    mine: &Arc<OnceLock<String>>,
    reason: String,
) {
    let held = live.lock().await;
    if !is_this_session(held.as_ref().map(|live| &live.transcript_failure), mine) {
        // Stop or Discard already took the session, so this ending is the one
        // that was asked for. `pump_result` reports it from there.
        return;
    }

    tracing::error!(
        %reason,
        "the live transcription ended while the meeting was still recording"
    );
    let _ = mine.set(reason);
    emit_state(app, MeetingState::RecordingWithoutTranscript);
}

/// Feed the provider what the recorder forwards, emitting transcript as it comes.
async fn pump(
    mut provider: Box<dyn MeetingTranscriptionProvider>,
    session_rate: u32,
    mut audio: mpsc::UnboundedReceiver<Drained>,
    mut events: mpsc::UnboundedReceiver<TranscriptEvent>,
    app: AppHandle,
) -> Result<MeetingTranscript, MeetingError> {
    while let Some(drained) = audio.recv().await {
        for (samples, source) in [
            (drained.mic, AudioSource::Mic),
            (drained.system, AudioSource::System),
        ] {
            if samples.is_empty() {
                continue;
            }
            // The last conversion, and the only place that knows both rates.
            // Sending 16 kHz audio to a provider expecting 24 would transcribe a
            // stream of chipmunks rather than fail, which is the worst kind of
            // wrong.
            let at_rate = wav::resample(&samples, drained.sample_rate, session_rate);
            let chunk = AudioChunk {
                pcm: pcm_bytes(&at_rate, session_rate),
                source,
            };
            provider.send_audio(chunk).await?;
        }

        // Drained after sending, so a window transcribed during this call is
        // reported in this call rather than one tick later.
        while let Ok(event) = events.try_recv() {
            if let Err(err) = app.emit(TRANSCRIPT_EVENT, &event) {
                tracing::warn!(error = %err, "could not deliver a transcript event");
            }
        }
    }

    let transcript = provider.finish().await?;
    while let Ok(event) = events.try_recv() {
        if let Err(err) = app.emit(TRANSCRIPT_EVENT, &event) {
            tracing::warn!(error = %err, "could not deliver a transcript event");
        }
    }

    Ok(transcript)
}

/// Mono `f32` as the 16-bit PCM a provider takes.
///
/// The rate is only carried in the header this then strips, so it changes
/// nothing about the bytes — but passing it means the call site says what rate
/// the samples are, which is the thing that was wrong here before.
fn pcm_bytes(samples: &[f32], sample_rate: u32) -> Vec<u8> {
    // Through the encoder and back out, so there is one clamp-and-scale in the
    // workspace rather than a second copy that could round differently.
    let wav = wav::encode_wav(samples, sample_rate);
    wav.get(44..).unwrap_or_default().to_vec()
}

/// One transcript segment in the shape `notes_core::transcript` already reads.
///
/// Reusing that shape means the existing export path — SRT, VTT, Markdown —
/// works on a meeting note with no further work.
fn segment(
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
struct WholePass {
    text: String,
    timings: Vec<kw_transcription_core::local::TimedSegment>,
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
fn labelled(
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

/* ------------------------------------------------------------------ commands */

/// Every meeting provider, with whether it can be used.
///
/// Credential-aware: a provider whose client exists but whose key is not stored
/// comes back as `NeedsCredential` rather than `Available`, so the screen offers
/// exactly what [`MeetingRuntime::start`] would accept. `contains` is asked, not
/// `get` -- knowing whether a key is there does not require reading it.
#[tauri::command]
pub async fn meeting_providers(
    runtime: State<'_, Arc<MeetingRuntime>>,
) -> Result<Vec<kw_meeting_core::MeetingProviderSummary>, String> {
    let credentials = Arc::clone(&runtime.credentials);
    Ok(kw_meeting_core::listing_with_credentials(&|slot| {
        kw_security::KeyRef::new(slot).is_some_and(|key| credentials.contains(&key))
    }))
}

#[tauri::command]
pub async fn meeting_status(
    runtime: State<'_, Arc<MeetingRuntime>>,
) -> Result<MeetingStatus, String> {
    Ok(runtime.status().await)
}

#[tauri::command]
pub async fn meeting_start(
    runtime: State<'_, Arc<MeetingRuntime>>,
    provider: String,
    system_audio: bool,
) -> Result<Sources, String> {
    runtime.start(&provider, system_audio).await
}

#[tauri::command]
pub async fn meeting_stop(
    runtime: State<'_, Arc<MeetingRuntime>>,
) -> Result<MeetingOutcome, String> {
    runtime.stop().await
}

#[tauri::command]
pub async fn meeting_cancel(runtime: State<'_, Arc<MeetingRuntime>>) -> Result<(), String> {
    runtime.cancel().await
}

/// Turn away an offer to record, without recording.
#[tauri::command]
pub async fn meeting_dismiss(runtime: State<'_, Arc<MeetingRuntime>>) -> Result<(), String> {
    runtime.dismiss().await;
    Ok(())
}

/// Whether a detected meeting offers to record.
#[tauri::command]
pub async fn meeting_set_prompt(
    state: State<'_, crate::state::AppState>,
    enabled: bool,
) -> Result<(), String> {
    kw_storage::settings::set(state.db().pool(), PROMPT_KEY, &enabled)
        .await
        .map_err(|e| format!("could not save the setting: {e}"))
}

/// The user's own process names, for `meeting-detect:generic`.
#[tauri::command]
pub async fn meeting_detect_lists(
    state: State<'_, crate::state::AppState>,
) -> Result<Lists, String> {
    kw_storage::settings::get_or(state.db().pool(), LISTS_KEY, Lists::default())
        .await
        .map_err(|e| format!("could not read the detection lists: {e}"))
}

#[tauri::command]
pub async fn meeting_set_detect_lists(
    state: State<'_, crate::state::AppState>,
    lists: Lists,
) -> Result<(), String> {
    kw_storage::settings::set(state.db().pool(), LISTS_KEY, &lists)
        .await
        .map_err(|e| format!("could not save the detection lists: {e}"))
}

/// The language to give the provider, or the empty string for automatic
/// detection where the provider supports it.
#[tauri::command]
pub async fn meeting_language(state: State<'_, crate::state::AppState>) -> Result<String, String> {
    kw_storage::settings::get_or(state.db().pool(), LANGUAGE_KEY, String::new())
        .await
        .map_err(|e| format!("could not read the language setting: {e}"))
}

#[tauri::command]
pub async fn meeting_set_language(
    state: State<'_, crate::state::AppState>,
    language: String,
) -> Result<(), String> {
    kw_storage::settings::set(state.db().pool(), LANGUAGE_KEY, &language)
        .await
        .map_err(|e| format!("could not save the language setting: {e}"))
}

/// Terms to bias recognition towards. How many a provider actually uses
/// depends on its cap -- see [`meeting_review_config`].
#[tauri::command]
pub async fn meeting_keyterms(
    state: State<'_, crate::state::AppState>,
) -> Result<Vec<String>, String> {
    kw_storage::settings::get_or(state.db().pool(), KEYTERMS_KEY, Vec::new())
        .await
        .map_err(|e| format!("could not read the keyterms: {e}"))
}

#[tauri::command]
pub async fn meeting_set_keyterms(
    state: State<'_, crate::state::AppState>,
    keyterms: Vec<String>,
) -> Result<(), String> {
    kw_storage::settings::set(state.db().pool(), KEYTERMS_KEY, &keyterms)
        .await
        .map_err(|e| format!("could not save the keyterms: {e}"))
}

/// Preview what `provider` will actually do with `language` and `keyterms`,
/// without saving anything or starting a meeting.
///
/// Takes the draft values directly rather than reading the settings, so the
/// screen can show a warning as someone edits a field, before they save it.
/// [`kw_meeting_core::validate`] does the arithmetic; this only carries its
/// answer across the wire.
#[tauri::command]
pub async fn meeting_review_config(
    provider: String,
    language: String,
    keyterms: Vec<String>,
) -> Result<kw_meeting_core::ConfigReview, String> {
    let config = draft_config(&provider, &language, &keyterms)?;
    kw_meeting_core::validate(&config).map_err(|e| e.to_string())
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
    // Only for `resolve_key_ref`'s tests, to call `set`/`contains` on a
    // `MemoryCredentialStore` by trait method rather than a fully-qualified
    // path.
    use kw_security::CredentialStore as _;

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

    /* --------------------------------------- the transcript ending mid-meeting */

    /// The four strings the frontend compares against, asserted here because a
    /// renamed variant is a silent no-match over there rather than a type error.
    #[test]
    fn the_states_serialise_as_the_frontend_reads_them() {
        assert_eq!(
            serde_json::to_value(MeetingState::RecordingWithoutTranscript).unwrap(),
            "recording-without-transcript"
        );
        assert_eq!(
            serde_json::to_value(MeetingState::Finishing).unwrap(),
            "finishing"
        );
        let outcome = MeetingOutcome::Saved {
            note_id: 7,
            duration_seconds: 61.0,
            recording_path: "C:/x.wav".to_owned(),
            gaps: 2,
            transcript_is_partials: true,
        };
        let json = serde_json::to_value(&outcome).unwrap();
        assert_eq!(json["outcome"], "saved");
        assert_eq!(json["noteId"], 7);
        assert_eq!(json["gaps"], 2);
        assert_eq!(json["transcriptIsPartials"], true);
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

    #[test]
    fn every_client_can_be_built_by_the_dispatch() {
        // The exhaustive match in `provider` is what makes a new client a compile
        // error until it is wired. This asserts the other half: that every
        // allowed provider has a `Client` the match covers.
        for provider in kw_meeting_core::PROVIDERS {
            let allowed = kw_meeting_core::is_allowed(provider.id);
            let client = kw_meeting_core::client(provider.id);
            assert_eq!(allowed, client.is_some(), "{}", provider.id);
        }
    }

    #[test]
    fn resolve_client_resolves_the_clients_that_exist() {
        assert_eq!(resolve_client("local"), Ok(Client::LocalWindow));
        assert_eq!(
            resolve_client("openai-realtime"),
            Ok(Client::OpenAiRealtime)
        );
        assert_eq!(
            resolve_client("assemblyai-realtime"),
            Ok(Client::AssemblyAi)
        );
        assert_eq!(
            resolve_client("deepgram-realtime"),
            Ok(Client::DeepgramRealtime)
        );
        assert_eq!(resolve_client("corti-realtime"), Ok(Client::CortiRealtime));
    }

    #[test]
    fn resolve_client_names_the_declined_reason_rather_than_a_generic_message() {
        let err = resolve_client("tinfoil-realtime").unwrap_err();
        assert!(err.contains("attestation"), "{err}");
    }

    #[test]
    fn resolve_client_refuses_an_unknown_provider_id() {
        let err = resolve_client("not-a-real-provider").unwrap_err();
        assert!(err.contains("no meeting client"), "{err}");
    }

    // `resolve_key_ref` is a free function taking `&dyn CredentialStore`
    // precisely so these can run with no `AppHandle`, no Tauri runtime, and no
    // real keychain -- see its doc comment for why holding a reference to
    // Tauri-dependent code in a test is not an option here.

    #[test]
    fn local_needs_no_credential_even_when_the_store_is_empty() {
        let store = kw_security::MemoryCredentialStore::new();
        let key_ref = resolve_key_ref("local", Client::LocalWindow, &store).unwrap();
        assert_eq!(key_ref, None);
    }

    #[test]
    fn openai_realtime_refuses_to_start_naming_the_settings_screen_when_nothing_is_stored() {
        // The tripwire for the defect this task fixes: `key_ref: None` used to
        // reach `connect` regardless, so the failure a user saw was a socket
        // error with no mention of where to fix it.
        let store = kw_security::MemoryCredentialStore::new();
        let err = resolve_key_ref("openai-realtime", Client::OpenAiRealtime, &store).unwrap_err();
        assert!(err.contains("Settings > Providers"), "{err}");
    }

    #[test]
    fn openai_realtime_gets_past_the_check_once_a_key_is_stored() {
        let store = kw_security::MemoryCredentialStore::new();
        store
            .set(
                &kw_security::KeyRef::new("openai").unwrap(),
                &kw_security::Secret::new("sk-test".to_owned()),
            )
            .unwrap();

        let key_ref = resolve_key_ref("openai-realtime", Client::OpenAiRealtime, &store).unwrap();
        assert_eq!(key_ref, kw_security::KeyRef::new("openai"));
    }

    #[test]
    fn a_blank_stored_credential_is_treated_as_absent() {
        // `CredentialStore::contains` already treats whitespace-only as empty;
        // this asserts the controller's refusal actually depends on that, not
        // on some looser check of its own.
        let store = kw_security::MemoryCredentialStore::new();
        store
            .set(
                &kw_security::KeyRef::new("openai").unwrap(),
                &kw_security::Secret::new("   ".to_owned()),
            )
            .unwrap();

        let err = resolve_key_ref("openai-realtime", Client::OpenAiRealtime, &store).unwrap_err();
        assert!(err.contains("Settings > Providers"), "{err}");
    }

    #[test]
    fn assemblyai_realtime_refuses_to_start_naming_the_settings_screen_when_nothing_is_stored() {
        let store = kw_security::MemoryCredentialStore::new();
        let err = resolve_key_ref("assemblyai-realtime", Client::AssemblyAi, &store).unwrap_err();
        assert!(err.contains("Settings > Providers"), "{err}");
    }

    #[test]
    fn assemblyai_realtime_gets_past_the_check_once_a_key_is_stored() {
        let store = kw_security::MemoryCredentialStore::new();
        store
            .set(
                &kw_security::KeyRef::new("assemblyai").unwrap(),
                &kw_security::Secret::new("aai-test".to_owned()),
            )
            .unwrap();

        let key_ref = resolve_key_ref("assemblyai-realtime", Client::AssemblyAi, &store).unwrap();
        assert_eq!(key_ref, kw_security::KeyRef::new("assemblyai"));
    }

    #[test]
    fn deepgram_realtime_refuses_to_start_naming_the_settings_screen_when_nothing_is_stored() {
        let store = kw_security::MemoryCredentialStore::new();
        let err =
            resolve_key_ref("deepgram-realtime", Client::DeepgramRealtime, &store).unwrap_err();
        assert!(err.contains("Settings > Providers"), "{err}");
    }

    #[test]
    fn deepgram_realtime_gets_past_the_check_once_a_key_is_stored() {
        let store = kw_security::MemoryCredentialStore::new();
        store
            .set(
                &kw_security::KeyRef::new("deepgram").unwrap(),
                &kw_security::Secret::new("dg-test".to_owned()),
            )
            .unwrap();

        let key_ref =
            resolve_key_ref("deepgram-realtime", Client::DeepgramRealtime, &store).unwrap();
        assert_eq!(key_ref, kw_security::KeyRef::new("deepgram"));
    }

    #[test]
    fn corti_realtime_refuses_to_start_naming_the_settings_screen_when_nothing_is_stored() {
        let store = kw_security::MemoryCredentialStore::new();
        let err = resolve_key_ref("corti-realtime", Client::CortiRealtime, &store).unwrap_err();
        assert!(err.contains("Settings > Providers"), "{err}");
    }

    #[test]
    fn corti_realtime_gets_past_the_check_once_a_key_is_stored() {
        let store = kw_security::MemoryCredentialStore::new();
        store
            .set(
                &kw_security::KeyRef::new("corti").unwrap(),
                &kw_security::Secret::new("corti-test".to_owned()),
            )
            .unwrap();

        let key_ref = resolve_key_ref("corti-realtime", Client::CortiRealtime, &store).unwrap();
        assert_eq!(key_ref, kw_security::KeyRef::new("corti"));
    }

    #[test]
    fn corti_realtime_refuses_to_build_naming_which_of_environment_or_tenant_is_missing() {
        // `resolve_corti_scope` is what `provider`'s `Client::CortiRealtime`
        // arm calls before it ever constructs a socket -- this is the same
        // refusal `route.rs::required` fires for batch transcription, reached
        // from a second caller rather than duplicated.
        let settings = kw_transcription_core::RouteSettings::default();
        let err = resolve_corti_scope(&settings).unwrap_err();
        assert!(err.contains("environment"), "{err}");

        let settings = kw_transcription_core::RouteSettings {
            corti_environment: Some("eu".to_owned()),
            ..Default::default()
        };
        let err = resolve_corti_scope(&settings).unwrap_err();
        assert!(err.contains("tenant"), "{err}");
    }

    #[test]
    fn corti_realtime_scope_resolves_once_both_are_set() {
        let settings = kw_transcription_core::RouteSettings {
            corti_environment: Some(" eu ".to_owned()),
            corti_tenant: Some(" acme ".to_owned()),
            ..Default::default()
        };
        let (environment, tenant) = resolve_corti_scope(&settings).unwrap();
        assert_eq!(environment, "eu");
        assert_eq!(tenant, "acme");
    }

    #[test]
    fn every_clients_credential_slot_is_a_valid_key_ref() {
        // `resolve_key_ref`'s `KeyRef::new(slot).ok_or_else(...)` branch stayed
        // unreachable and untested while only two `Client` variants existed --
        // both slots happened to be valid strings, so nothing ever exercised
        // the refusal. A third arm is exactly where a typo producing an
        // invalid slot name (stray punctuation, a capital letter) would first
        // show up, so this checks every arm directly against the validation
        // `resolve_key_ref` depends on, rather than trusting that the slots
        // that happen to exist today stay well-formed by accident.
        for client in [
            Client::LocalWindow,
            Client::OpenAiRealtime,
            Client::AssemblyAi,
            Client::DeepgramRealtime,
            Client::CortiRealtime,
        ] {
            if let Some(slot) = kw_meeting_core::credential_slot(client) {
                assert!(
                    kw_security::KeyRef::new(slot).is_some(),
                    "{slot:?} is not a valid credential slot name"
                );
            }
        }
    }

    // `resolve_provider_config`, `draft_config` and the settings keys are all
    // free functions and a database, precisely so the properties below can be
    // asserted with no `AppHandle` in sight -- see their doc comments.

    #[tokio::test]
    async fn the_language_and_keyterms_settings_round_trip() {
        let db = Db::open_in_memory().await.unwrap();
        let pool = db.pool();

        // Nothing stored yet: language is unset and there are no keyterms.
        assert_eq!(
            kw_storage::settings::get_or(pool, LANGUAGE_KEY, String::new())
                .await
                .unwrap(),
            ""
        );
        assert!(
            kw_storage::settings::get_or(pool, KEYTERMS_KEY, Vec::<String>::new())
                .await
                .unwrap()
                .is_empty()
        );

        kw_storage::settings::set(pool, LANGUAGE_KEY, &"sv".to_owned())
            .await
            .unwrap();
        let keyterms = vec!["Sprint".to_owned(), "Q3 roadmap".to_owned()];
        kw_storage::settings::set(pool, KEYTERMS_KEY, &keyterms)
            .await
            .unwrap();

        assert_eq!(
            kw_storage::settings::get_or(pool, LANGUAGE_KEY, String::new())
                .await
                .unwrap(),
            "sv"
        );
        assert_eq!(
            kw_storage::settings::get_or(pool, KEYTERMS_KEY, Vec::<String>::new())
                .await
                .unwrap(),
            keyterms
        );
    }

    #[test]
    fn corti_refuses_to_start_naming_the_provider_and_the_field_when_no_language_is_set() {
        // The tripwire for the property this task exists to add: `start` used
        // to build its config with `language: None` unconditionally and no
        // check at all, so a provider that cannot detect a language would have
        // failed three seconds later at `connect`, or worse, produced a
        // transcript in the wrong language with nothing said about it.
        let caps = kw_meeting_core::capabilities("corti-realtime").unwrap();
        let err =
            resolve_provider_config("corti-realtime", caps, None, 16_000, "", &[]).unwrap_err();
        assert!(err.contains("Corti Realtime"), "{err}");
        assert!(err.contains("language"), "{err}");
        assert!(err.contains("Settings > Meetings"), "{err}");
    }

    #[test]
    fn corti_proceeds_once_a_language_is_given() {
        let caps = kw_meeting_core::capabilities("corti-realtime").unwrap();
        let config =
            resolve_provider_config("corti-realtime", caps, None, 16_000, "sv", &[]).unwrap();
        assert_eq!(config.language, Some("sv".to_owned()));
    }

    #[test]
    fn a_provider_that_detects_language_is_not_blocked_by_a_blank_field() {
        let caps = kw_meeting_core::capabilities("openai-realtime").unwrap();
        let config =
            resolve_provider_config("openai-realtime", caps, None, 24_000, "", &[]).unwrap();
        assert_eq!(config.language, None);
    }

    #[test]
    fn the_local_provider_is_not_blocked_by_a_blank_field_either() {
        // The regression this task's own change to `capabilities.rs` guards
        // against: the one provider that needs no credentials and no network
        // must keep working with nothing configured.
        let caps = kw_meeting_core::capabilities("local").unwrap();
        let config = resolve_provider_config("local", caps, None, 16_000, "", &[]).unwrap();
        assert_eq!(config.language, None);
    }

    #[test]
    fn a_blank_language_setting_is_not_sent_as_the_literal_empty_string() {
        assert_eq!(normalize_language("   "), None);
        assert_eq!(normalize_language("  sv  "), Some("sv".to_owned()));
    }

    #[test]
    fn blank_keyterms_are_dropped_before_they_reach_a_providers_cap() {
        let keyterms = vec!["  ".to_owned(), "Sprint".to_owned(), String::new()];
        assert_eq!(normalize_keyterms(&keyterms), vec!["Sprint".to_owned()]);
    }

    #[test]
    fn a_keyterm_count_over_the_cap_is_reported_rather_than_silently_truncated() {
        // What the settings screen renders: AssemblyAI caps at 100 and
        // truncates without complaint on the wire, so this is the data that
        // makes the truncation visible instead.
        let keyterms: Vec<String> = (0..150).map(|i| format!("term{i}")).collect();
        let config = draft_config("assemblyai-realtime", "en", &keyterms).unwrap();
        let review = kw_meeting_core::validate(&config).unwrap();
        assert_eq!(review.keyterms_requested, 150);
        assert_eq!(review.keyterms_used, 100);
        assert_eq!(review.keyterms_dropped, 50);
        assert!(review.has_warnings());
    }

    #[test]
    fn draft_config_refuses_a_provider_nothing_describes() {
        let err = draft_config("not-a-real-provider", "", &[]).unwrap_err();
        assert!(err.contains("not-a-real-provider"), "{err}");
    }

    fn detected(provider: &'static str, in_call: bool) -> Detected {
        // `CallProcess` for the in-call cases because these tests are about the
        // offer bookkeeping and not about which signal fired. The signal that
        // gets Teams here at all is exercised in `kw_platform::meeting_detect`,
        // against the real rules.
        Detected::new(
            provider,
            "Something",
            format!("{provider}.exe"),
            if in_call {
                CallSignal::CallProcess
            } else {
                CallSignal::None
            },
        )
    }

    #[test]
    fn an_offer_carries_the_signal_that_produced_it() {
        // The offer window says why it appeared, and "Teams is holding your
        // microphone" and "Zoom started a call process" are different claims
        // with different chances of being wrong. Losing the distinction between
        // `Detected` and `Offer` would leave the window with nothing to say.
        let mut watcher = Watcher::default();
        let mic = Detected::new(
            "teams",
            "Microsoft Teams",
            "ms-teams.exe".to_owned(),
            CallSignal::Microphone,
        );
        let offer = watcher.consider(&[mic]).expect("a call is offered");
        assert_eq!(offer.signal, CallSignal::Microphone);
        assert_eq!(offer.evidence, "ms-teams.exe");
    }

    #[test]
    fn a_meeting_in_progress_is_offered_once_and_not_again() {
        // The property that decides whether this feature is tolerable. Polling is
        // every five seconds; a second offer would be twelve windows a minute.
        let mut watcher = Watcher::default();
        let running = vec![detected("zoom", true)];

        let offer = watcher.consider(&running).expect("the first poll offers");
        assert_eq!(offer.provider, "zoom");
        assert_eq!(offer.evidence, "zoom.exe");

        for _ in 0..10 {
            assert!(watcher.consider(&running).is_none(), "offered twice");
        }
    }

    #[test]
    fn a_client_merely_being_open_is_not_offered() {
        // Measured, not preferred: `meeting_detect_probe` found ms-teams.exe
        // running with no call in progress, because new Teams runs the call
        // inside its own process. Offering on "the client is open" would fire on
        // every launch for anyone whose Teams starts with Windows.
        let mut watcher = Watcher::default();
        assert!(watcher.consider(&[detected("teams", false)]).is_none());
        assert!(!watcher.has_offered("teams"));
    }

    #[test]
    fn the_next_meeting_is_offered_after_the_application_quits() {
        let mut watcher = Watcher::default();
        let running = vec![detected("zoom", true)];

        assert!(watcher.consider(&running).is_some());
        // Zoom quits.
        assert!(watcher.consider(&[]).is_none());
        assert!(!watcher.has_offered("zoom"), "the suppression was released");
        // And a new call is offered.
        assert!(watcher.consider(&running).is_some());
    }

    #[test]
    fn recording_one_call_does_not_suppress_the_next() {
        // `forget` is what the end of a meeting calls. Without it, a second call
        // in the same Zoom session would never be offered, because Zoom never
        // quit in between.
        let mut watcher = Watcher::default();
        let running = vec![detected("zoom", true)];

        assert!(watcher.consider(&running).is_some());
        watcher.forget("zoom");
        assert!(watcher.consider(&running).is_some());
    }

    #[test]
    fn only_one_offer_is_made_when_two_meetings_are_running() {
        // Two windows stacked on top of each other is worse than one question.
        let mut watcher = Watcher::default();
        let running = vec![detected("zoom", true), detected("webex", true)];

        assert!(watcher.consider(&running).is_some());
        // The second is offered on the next poll, not simultaneously.
        let second = watcher.consider(&running).expect("the other one follows");
        assert_eq!(second.provider, "webex");
        assert!(watcher.consider(&running).is_none());
    }

    #[test]
    fn a_call_starting_inside_an_already_open_client_is_offered() {
        // The sequence a real Zoom meeting produces: the client is open first and
        // must not be offered, then the call starts and must be.
        let mut watcher = Watcher::default();
        assert!(watcher.consider(&[detected("zoom", false)]).is_none());
        assert!(watcher.consider(&[detected("zoom", true)]).is_some());
    }

    #[test]
    fn a_declined_provider_is_refused_with_its_reason_and_not_a_generic_message() {
        // The screen must not offer "no client for tinfoil-realtime" when the
        // truth is that it is refused on purpose.
        let reason = kw_meeting_core::declined("tinfoil-realtime").unwrap();
        assert!(reason.contains("attestation"), "{reason}");
        assert!(kw_meeting_core::client("tinfoil-realtime").is_none());
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
