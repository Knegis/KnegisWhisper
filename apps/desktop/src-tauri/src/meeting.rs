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
//! The live events are partials: windows overlap, so they repeat words, and
//! [`kw_meeting_core::local`] explains why deduplicating them properly is not
//! possible with what `/inference` returns. So the note gets one pass over the
//! finished file, which is exactly the path a dictation takes.
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
use std::sync::Arc;
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
use kw_platform::meeting_detect::{self, Detected, Lists};
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
    /// The recording has stopped and the whole-file pass is still running.
    ///
    /// A separate state because it can take minutes on a long meeting, and
    /// "Idle" while the transcript is still being produced would look like a
    /// meeting that vanished.
    Finishing,
}

/// Which captures are actually running.
///
/// Reported rather than assumed, because a meeting works microphone-only and the
/// user should know which one they got. Zero system-audio samples means nobody
/// else spoke; *no system capture at all* is a different fact.
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
    /// The provider in use, while there is one.
    pub provider: Option<String>,
    pub elapsed_seconds: f64,
    pub sources: Option<Sources>,
    /// What this platform can do about system audio, whether or not a meeting is
    /// running.
    pub system_audio: SystemAudioSupport,
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
}

impl From<&Detected> for Offer {
    fn from(detected: &Detected) -> Self {
        Self {
            provider: detected.provider.to_owned(),
            name: detected.name.to_owned(),
            evidence: detected.evidence.clone(),
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
    /// Nothing was said, so there is nothing to keep.
    NothingTranscribed {
        recording_path: String,
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

/// What the recorder produced.
#[derive(Debug)]
struct Finished {
    path: std::path::PathBuf,
    duration_seconds: f64,
    /// Samples that exceeded full scale and were clamped by the encoder. See
    /// [`MixFeed`] on why this is counted rather than corrected.
    clipped: u64,
}

/// Start the capture thread.
///
/// Returns the audio channel and the command channel. Both captures are started
/// here, on the thread, because `cpal::Stream` is not `Send`.
fn spawn_recorder(
    path: std::path::PathBuf,
    system: bool,
) -> Result<
    (
        mpsc::UnboundedReceiver<Drained>,
        sync_mpsc::Sender<RecorderCommand>,
        Sources,
    ),
    String,
> {
    let (audio_tx, audio_rx) = mpsc::unbounded_channel::<Drained>();
    let (command_tx, command_rx) = sync_mpsc::channel::<RecorderCommand>();
    // The thread reports whether it got its devices before anything else
    // happens, so a failure to open the microphone is a failure to start rather
    // than a meeting that records nothing.
    let (ready_tx, ready_rx) = sync_mpsc::channel::<Result<Sources, String>>();

    std::thread::Builder::new()
        .name("kw-meeting".to_owned())
        .spawn(move || record(path, system, audio_tx, command_rx, ready_tx))
        .map_err(|e| format!("could not start the recording thread: {e}"))?;

    let sources = ready_rx
        .recv()
        .map_err(|_| "the recording thread stopped before it started".to_owned())??;

    Ok((audio_rx, command_tx, sources))
}

/// The recorder thread.
fn record(
    path: std::path::PathBuf,
    want_system: bool,
    audio: mpsc::UnboundedSender<Drained>,
    commands: sync_mpsc::Receiver<RecorderCommand>,
    ready: sync_mpsc::Sender<Result<Sources, String>>,
) {
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

    // A closure so the tick and the final drain cannot diverge.
    let drain_once = |writer: &mut WavWriter, mixer: &mut MixFeed| {
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
                let _ = drain_once(&mut writer, &mut mixer);
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
                if drain_once(&mut writer, &mut mixer).is_err() {
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

        // Only a meeting actually in progress. See the module documentation: new
        // Teams has no in-call process, so prompting on "the client is open"
        // would fire on every launch for most corporate machines.
        //
        // And the first one *not already offered*, not simply the first. Finding
        // the first in-call provider and then discarding it for being a repeat
        // meant a second meeting running alongside the first was never offered at
        // all -- silently, which is the part that matters. A test caught it.
        let candidate = detected
            .iter()
            .find(|d| d.in_call && !self.offered.contains(d.provider))?;

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
    live: Mutex<Option<Live>>,
    /// Detection state: what was last seen, and what has been offered.
    watch: Mutex<(Watcher, Vec<Detected>)>,
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
    ) -> Self {
        Self {
            app,
            db,
            policy,
            credentials,
            live: Mutex::new(None),
            local,
            watch: Mutex::new((Watcher::default(), Vec::new())),
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

        let detected = meeting_detect::detect(&meeting_detect::running(), &lists);

        let offer = {
            let mut held = self.watch.lock().await;
            held.1 = detected.clone();
            // `consider` runs whether or not prompting is on, so its
            // already-offered bookkeeping stays correct if the setting is turned
            // on mid-call -- otherwise switching it on would immediately prompt
            // for a meeting that started an hour ago.
            let offer = held.0.consider(&detected);
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
        // Recording *and* finishing keep the tray saying so: the microphone is
        // closed once finishing starts, but a meeting that still has work to do
        // has not finished, and an idle tray would say it had.
        crate::tray::meeting(&self.app, state != MeetingState::Idle);

        if let Err(err) = self.app.emit(STATE_EVENT, state) {
            tracing::warn!(error = %err, "could not report the meeting state");
        }
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
        let (audio, commands, sources) = spawn_recorder(path.clone(), system_audio)?;

        let pump = tokio::spawn(pump(
            provider,
            session_rate,
            audio,
            events_rx,
            self.app.clone(),
        ));

        *held = Some(Live {
            provider_id: provider_id.to_owned(),
            commands,
            pump,
            recording_path: path,
            sources: sources.clone(),
            started: std::time::Instant::now(),
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
        self.watch.lock().await.0.forget(&provider_id);

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
        self.watch.lock().await.0.forget(&live.provider_id);

        if let Err(err) = self.app.emit(OUTCOME_EVENT, MeetingOutcome::Cancelled) {
            tracing::warn!(error = %err, "could not report the cancellation");
        }
        Ok(())
    }

    /// What is happening, for the screen.
    pub async fn status(&self) -> MeetingStatus {
        let held = self.live.lock().await;
        let system_audio = kw_audio_core::system_audio::support();
        let detected = self.watch.lock().await.1.clone();
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
                state: MeetingState::Recording,
                provider: Some(live.provider_id.clone()),
                elapsed_seconds: live.started.elapsed().as_secs_f64(),
                sources: Some(live.sources.clone()),
                system_audio,
                detected,
                prompt_on_detect,
                notification_excluded,
            },
            None => MeetingStatus {
                state: MeetingState::Idle,
                provider: None,
                elapsed_seconds: 0.0,
                sources: None,
                system_audio,
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
        let (segments, from_partials) = match self.transcribe_whole(&finished.path).await {
            Ok(Some(text)) => (vec![segment(&text, 0.0, None)], false),
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
                "\n\n(The whole-recording pass did not complete, so this is the live transcript. \
                 Its windows overlap, so some phrases repeat.)",
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

    /// Transcribe the finished recording in one request.
    async fn transcribe_whole(&self, path: &std::path::Path) -> Result<Option<String>, String> {
        use kw_transcription_core::local::{Inference, LocalTranscriber as _};

        let port = *self
            .local
            .port
            .read()
            .map_err(|_| "the local model server's state could not be read".to_owned())?;
        let port = port.ok_or_else(|| "the local model server is not running".to_owned())?;
        let model = self
            .local
            .model
            .read()
            .map_err(|_| "the local model could not be read".to_owned())?
            .clone();

        let wav = std::fs::read(path).map_err(|e| format!("could not read the recording: {e}"))?;
        let server =
            kw_transcription_core::local::WhisperServer::new(self.policy.clone(), port, model);

        match server.transcribe(wav, None, None).await? {
            // This is the whole-file fallback: one string is what it wants,
            // and it is not asked for timings. `Timed` is accepted anyway
            // because the text is the same text -- refusing it would turn a
            // server configuration change into a lost recording, which is the
            // opposite of what a fallback is for.
            Inference::Text(text) | Inference::Timed { text, .. } if !text.trim().is_empty() => {
                Ok(Some(text))
            }
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
        // own send call complained about afterwards.
        Ok(Err(err)) => Err(format!("{err}.")),
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

    #[test]
    fn the_states_serialise_as_the_frontend_reads_them() {
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
        Detected {
            provider,
            name: "Something",
            evidence: format!("{provider}.exe"),
            in_call,
        }
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
}
