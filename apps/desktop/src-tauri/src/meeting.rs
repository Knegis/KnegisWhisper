//! Running a meeting: two captures, one recording on disk, live transcript.
//!
//! The same shape as [`crate::dictation`] and for the same reasons — the state
//! lives in Rust because the window can be closed to the tray mid-meeting, and
//! the outcome is emitted rather than returned because there is nobody to return
//! it to. What differs is that a meeting does not end: it produces transcript
//! while it runs, and a recording longer than memory.
//!
//! # Where the pieces live
//!
//! Split on 2026-09-01 (KNEG-89), at 3,303 lines. The issue had been noted
//! three times and filed at ~1,600; it had doubled while the issue sat, and
//! every addition was reasonable on its own, which is the whole difficulty
//! with a file like this — no single change is ever the one that justifies
//! splitting it.
//!
//! What moved is what has no session state, so each piece is testable on its
//! own terms:
//!
//! - [`recorder`] — the thread that owns both captures, and everything about
//!   what it delivered and how it ended.
//! - [`watcher`] — noticing a meeting and offering to record it. Detection
//!   suggests; it never records.
//! - [`config`] — deciding what to connect with, before anything connects.
//!   Free functions over plain arguments, so "may this start, and with what" is
//!   exercised with no `AppHandle` anywhere.
//! - [`notes`] — turning a finished pass into the segments a note is made of,
//!   speaker labels included.
//!
//! What stayed is the part that genuinely holds state: [`MeetingRuntime`], the
//! session it runs, and the commands. Nothing changed in the move — same
//! functions, same tests, same names — and the test count either side of it is
//! the evidence, not a claim about care taken.
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

mod config;
mod notes;
mod recorder;
mod watcher;

use config::{
    draft_config, resolve_client, resolve_corti_scope, resolve_key_ref, resolve_provider_config,
};

use notes::{labelled, pcm_bytes, segment, WholePass};
pub use recorder::Delivered;
use recorder::{spawn_recorder, Drained, Finished, RecorderCommand, RecorderEnd};
use watcher::WatchState;

use std::sync::mpsc as sync_mpsc;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use kw_audio_core::{wav, SystemAudioSupport};
use kw_meeting_core::local::LocalWindowProvider;
use kw_meeting_core::{
    AudioChunk, AudioSource, Client, MeetingError, MeetingTranscript, MeetingTranscriptionProvider,
    TranscriptEvent,
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

    #[test]
    fn a_declined_provider_is_refused_with_its_reason_and_not_a_generic_message() {
        // The screen must not offer "no client for tinfoil-realtime" when the
        // truth is that it is refused on purpose.
        let reason = kw_meeting_core::declined("tinfoil-realtime").unwrap();
        assert!(reason.contains("attestation"), "{reason}");
        assert!(kw_meeting_core::client("tinfoil-realtime").is_none());
    }
}
