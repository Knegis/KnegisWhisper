//! Real implementations of the six seams `kw_app_core::dictation` defines.
//!
//! The state machine in that crate is complete and tested against fakes. This is
//! the other half: the concrete microphone, provider, clipboard, and database
//! behind each trait. Nothing here contains sequence logic — if a decision about
//! *order* ends up in this file, it belongs in `app-core` where it can be tested
//! without a microphone.
//!
//! ## Why the microphone gets a thread
//!
//! `cpal::Stream` is not `Send`: on Windows it holds COM objects bound to the
//! thread that created them. The `Recorder` trait is `Send + Sync`, as it has to
//! be to live in Tauri's managed state. So [`MicRecorder`] owns a plain OS thread
//! that holds the stream and takes commands over a channel, and the trait seam is
//! what makes that invisible to everything upstream of it.
//!
//! That is the seam earning its place rather than a workaround: the state machine
//! never learns that the microphone is thread-bound, and the thread never learns
//! what a dictation is.

use std::sync::{mpsc, Arc, RwLock};
use ts_rs::TS;

use kw_app_core::dictation::{
    Capture, Cleanup, DictationError, History, HistoryEntry, Inserter, InsertionResult, Recorder,
    SessionState, StateSink, Transcribed, Transcriber,
};
use kw_network_policy::NetworkPolicy;
use kw_platform::insertion::{SystemClipboard, TextInserter};
use kw_security::CredentialStore;
use kw_sidecar::process::Health as _;
use kw_sidecar::HttpHealth;
use kw_storage::{transcriptions, Db};
use kw_transcription_core::local::{LocalTranscriber, WhisperServer};
use kw_transcription_core::{Executor, Transport};
use tauri::{AppHandle, Emitter as _, Manager as _};

/// The event the frontend listens to for dictation state.
pub const STATE_EVENT: &str = "dictation-state";

/// The event a finished dictation is reported on.
pub const OUTCOME_EVENT: &str = "dictation-outcome";

/* ------------------------------------------------------------------ recorder */

/// What the microphone thread is asked to do.
enum MicCommand {
    Start {
        device: Option<String>,
        reply: tokio::sync::oneshot::Sender<Result<(), String>>,
    },
    Finish {
        reply: tokio::sync::oneshot::Sender<Result<kw_audio_core::Recording, String>>,
    },
    Cancel,
}

/// Records from the microphone on a dedicated thread.
pub struct MicRecorder {
    commands: mpsc::Sender<MicCommand>,
    /// The input device the user chose, or `None` for the system default.
    device: RwLock<Option<String>>,
}

impl std::fmt::Debug for MicRecorder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MicRecorder").finish_non_exhaustive()
    }
}

impl MicRecorder {
    /// Start the microphone thread. It lives as long as the application.
    #[must_use]
    pub fn spawn() -> Self {
        let (tx, rx) = mpsc::channel::<MicCommand>();

        std::thread::Builder::new()
            .name("kw-microphone".to_owned())
            .spawn(move || Self::serve(rx))
            // A thread we cannot start means dictation cannot work, but it must
            // not stop the application launching: the settings screen is how the
            // user would find out why.
            .map_or_else(
                |err| tracing::error!(error = %err, "could not start the microphone thread"),
                |_| (),
            );

        Self {
            commands: tx,
            device: RwLock::new(None),
        }
    }

    /// Choose the input device. Takes effect on the next recording.
    pub fn set_device(&self, device: Option<String>) {
        if let Ok(mut held) = self.device.write() {
            *held = device;
        }
    }

    fn preferred_device(&self) -> Option<String> {
        self.device.read().ok().and_then(|held| held.clone())
    }

    /// The thread body. Owns the `cpal::Stream` for its whole life.
    fn serve(rx: mpsc::Receiver<MicCommand>) {
        let mut active: Option<kw_audio_core::Capture> = None;

        while let Ok(command) = rx.recv() {
            match command {
                MicCommand::Start { device, reply } => {
                    // Dropping any previous capture stops that stream first, so
                    // two starts in a row cannot leave the microphone open.
                    active = None;
                    let started = kw_audio_core::Capture::start(device.as_deref());
                    let answer = match started {
                        Ok(capture) => {
                            active = Some(capture);
                            Ok(())
                        }
                        Err(err) => Err(err.to_string()),
                    };
                    let _ = reply.send(answer);
                }

                MicCommand::Finish { reply } => {
                    let answer = active.take().map_or_else(
                        || Err("the microphone was not open".to_owned()),
                        |capture| Ok(capture.finish()),
                    );
                    let _ = reply.send(answer);
                }

                MicCommand::Cancel => {
                    // Drop stops the stream, which is what closes the operating
                    // system's own recording indicator.
                    active = None;
                }
            }
        }
    }

    async fn ask<T>(
        &self,
        make: impl FnOnce(tokio::sync::oneshot::Sender<Result<T, String>>) -> MicCommand,
    ) -> Result<T, String> {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.commands
            .send(make(reply_tx))
            .map_err(|_| "the microphone thread is not running".to_owned())?;
        reply_rx
            .await
            .map_err(|_| "the microphone thread stopped without answering".to_owned())?
    }
}

#[async_trait::async_trait]
impl Recorder for MicRecorder {
    async fn start(&self) -> Result<(), DictationError> {
        let device = self.preferred_device();
        self.ask(|reply| MicCommand::Start { device, reply })
            .await
            .map_err(DictationError::Microphone)
    }

    async fn finish(&self) -> Result<Capture, DictationError> {
        let recording = self
            .ask(|reply| MicCommand::Finish { reply })
            .await
            .map_err(DictationError::Microphone)?;

        // `Recording` already carries the encoded WAV, the gate decision, and
        // the truncation flag, so this is a field copy rather than a conversion.
        Ok(Capture {
            wav: recording.wav,
            duration_ms: recording.duration_ms,
            gate: recording.gate,
            truncated: recording.truncated,
        })
    }

    async fn cancel(&self) {
        let _ = self.commands.send(MicCommand::Cancel);
    }
}

/* --------------------------------------------------------------- transcriber */

/// Where the local model server is listening, when one is running.
///
/// Shared with the commands that start and stop it, so a dictation started a
/// moment after the server came up sees it.
pub type LocalPort = Arc<RwLock<Option<u16>>>;

/// Executes a resolved route.
pub struct RouteTranscriber {
    policy: NetworkPolicy,
    credentials: Arc<dyn CredentialStore>,
    local_port: LocalPort,
    local_model: Arc<RwLock<Option<String>>>,
    local_sherpa: crate::local_asr::Selection,
    /// How to start the whisper server again when the recorded one has gone.
    ///
    /// Not `Option`: a build that forgets to supply it is a build where a lost
    /// server silently stays lost, which is the defect this field exists to fix.
    /// Making it required means forgetting is a compile error instead.
    revive: Arc<dyn crate::local_model::Revive>,
}

impl std::fmt::Debug for RouteTranscriber {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RouteTranscriber").finish_non_exhaustive()
    }
}

impl RouteTranscriber {
    #[must_use]
    pub const fn new(
        policy: NetworkPolicy,
        credentials: Arc<dyn CredentialStore>,
        local_port: LocalPort,
        local_model: Arc<RwLock<Option<String>>>,
        local_sherpa: crate::local_asr::Selection,
        revive: Arc<dyn crate::local_model::Revive>,
    ) -> Self {
        Self {
            policy,
            credentials,
            local_port,
            local_model,
            local_sherpa,
            revive,
        }
    }

    /// The whisper server to send audio to, starting a new one if the
    /// recorded server has gone.
    ///
    /// Called only for [`Transport::Local`]. The health check is a loopback
    /// round trip and the recovery starts a process, and someone dictating to a
    /// cloud provider should pay for neither.
    async fn whisper_server(&self) -> Result<Option<WhisperServer>, String> {
        let recorded = self.local_port.read().ok().and_then(|held| *held);
        let answering = match recorded {
            Some(port) => {
                HttpHealth::new(
                    self.policy.clone(),
                    kw_transcription_core::local::HEALTH_PATH,
                )
                .is_ready(port)
                .await
            }
            None => false,
        };

        let port = match classify(recorded, answering) {
            Recorded::Absent => return Ok(None),
            Recorded::Live(port) => port,
            Recorded::Gone(dead) => {
                tracing::warn!(
                    port = dead,
                    "the local model server has gone; starting it again"
                );
                let port = self.revive.revive().await.map_err(|err| {
                    format!(
                        "the local model server on 127.0.0.1:{dead} has gone and could not be \
                         started again: {err}"
                    )
                })?;
                tracing::info!(port, "the local model server was started again");
                port
            }
        };

        let model = self.local_model.read().ok().and_then(|held| held.clone());
        Ok(Some(WhisperServer::new(self.policy.clone(), port, model)))
    }
}

/// What a recorded whisper port turned out to be.
///
/// Three states rather than `Option<u16>`, because "no server was ever started"
/// and "a server was started and has since gone" call for different answers and
/// used to get the same one: the recorded number, handed to a transcription that
/// then failed against a port with nothing behind it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Recorded {
    /// Nothing is recorded. No local server has run this session.
    Absent,
    /// Something answered on the recorded port.
    Live(u16),
    /// A port is recorded and nothing answers: the child has gone.
    Gone(u16),
}

/// Read a recorded port and a health-check result as one of three states.
///
/// Separated from the round trip that produces `answering` so the reading can be
/// tested without a server, which is the half that was wrong.
const fn classify(recorded: Option<u16>, answering: bool) -> Recorded {
    match recorded {
        None => Recorded::Absent,
        Some(port) if answering => Recorded::Live(port),
        Some(port) => Recorded::Gone(port),
    }
}

/// Which local engine a `Transport::Local` request should go to.
///
/// One engine, chosen by one rule: a selected sherpa model wins, otherwise the
/// whisper server if it is running, otherwise nothing. A sherpa model that is
/// selected but cannot be built is an error rather than a quiet fall through to
/// whisper -- audio going somewhere the screen did not name is the failure this
/// ordering exists to prevent.
///
/// **`pub(crate)` since 2026-09-01, because a second caller needed the same
/// rule and had been getting a different one.** `meeting.rs`'s whole-recording
/// pass asked `WhisperServer` directly, so on a machine where a sherpa model was
/// selected -- the documented "one local engine" state, and the one this
/// project ships people into -- the pass failed with "the local model server is
/// not running" and every meeting fell back to its live partials. Reusing this
/// is the fix; a second copy of the rule would have been the bug again.
pub(crate) fn local_engine(
    sherpa: &crate::local_asr::Selection,
    whisper: Option<WhisperServer>,
) -> Result<Option<Box<dyn LocalTranscriber>>, String> {
    match crate::local_asr::resolve(sherpa) {
        Some(Ok(offline)) => Ok(Some(Box::new(offline))),
        Some(Err(reason)) => Err(reason),
        None => Ok(whisper.map(|w| Box::new(w) as Box<dyn LocalTranscriber>)),
    }
}

#[async_trait::async_trait]
impl Transcriber for RouteTranscriber {
    async fn transcribe(
        &self,
        transport: &Transport,
        wav: Vec<u8>,
    ) -> Result<Transcribed, DictationError> {
        let executor = Executor::new(&self.policy, self.credentials.as_ref());

        // Resolved only for a local route. The executor consults the local
        // engine in its `Transport::Local` arm and nowhere else, so building one
        // for a dictation bound for a provider bought nothing and cost two
        // things: a loopback health check on every cloud dictation, and — worse
        // — a *cloud* dictation failing outright when a sherpa model was
        // selected but could not be built, because that error was raised before
        // anything looked at the transport.
        //
        // Attached only when an engine is actually available. Without that the
        // executor answers `LocalNotAvailable`, which is the honest state — a
        // local route with no server is not something to paper over.
        let local = match transport {
            Transport::Local => {
                let whisper = self
                    .whisper_server()
                    .await
                    .map_err(DictationError::Transcription)?;
                local_engine(&self.local_sherpa, whisper).map_err(DictationError::Transcription)?
            }
            _ => None,
        };
        let executor = match &local {
            Some(engine) => executor.with_local(engine.as_ref()),
            None => executor,
        };

        let transcript = executor
            .transcribe(transport, wav)
            .await
            .map_err(|err| DictationError::Transcription(err.to_string()))?;

        Ok(Transcribed {
            text: transcript.text,
            provider: transcript.provider,
            model: transcript.model,
        })
    }
}

/* ------------------------------------------------------------------ cleanup */

/// Tidies a transcript with a reasoning model, when the user has asked for it.
///
/// Holds the policy and the credential store rather than a `Chat`, because
/// `Chat` borrows both and is a two-reference `const fn` to build — cheaper to
/// construct per call than to keep a self-referential struct alive.
pub struct ModelCleanup {
    policy: NetworkPolicy,
    credentials: Arc<dyn CredentialStore>,
    /// Set while cleanup is switched on; holds the route and model to use.
    configured: Arc<RwLock<Option<(kw_reasoning_core::ChatRoute, String)>>>,
    /// Where a local model is listening, if one is.
    ///
    /// Reading it is also what tells the idle timer the server is wanted, so
    /// cleanup running every minute keeps it alive and cleanup switched off
    /// lets it stop.
    local: crate::local_llm::LocalEndpoint,
}

impl std::fmt::Debug for ModelCleanup {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ModelCleanup").finish_non_exhaustive()
    }
}

impl ModelCleanup {
    #[must_use]
    pub const fn new(
        policy: NetworkPolicy,
        credentials: Arc<dyn CredentialStore>,
        configured: Arc<RwLock<Option<(kw_reasoning_core::ChatRoute, String)>>>,
        local: crate::local_llm::LocalEndpoint,
    ) -> Self {
        Self {
            policy,
            credentials,
            configured,
            local,
        }
    }
}

#[async_trait::async_trait]
impl Cleanup for ModelCleanup {
    async fn clean(&self, text: &str) -> String {
        let Some((route, model)) = self.configured.read().ok().and_then(|held| held.clone()) else {
            // Off. The raw transcript is the answer, not a failure.
            return text.to_owned();
        };

        let Ok(registry) = kw_provider_registry::registry() else {
            return text.to_owned();
        };

        // `Chat::cleanup` is already infallible by contract: it returns the
        // original on any failure, because a cleanup that errors must not lose
        // the transcript.
        kw_reasoning_core::Chat::new(&self.policy, self.credentials.as_ref())
            .with_local_port(self.local.port())
            .cleanup(registry, &route, &model, text)
            .await
    }
}

/* ----------------------------------------------------------------- inserter */

/// Puts text at the cursor by borrowing the clipboard.
#[derive(Debug)]
pub struct ClipboardInserter {
    inserter:
        Option<TextInserter<SystemClipboard, Box<dyn kw_platform::insertion::PasteKeystroke>>>,
}

impl ClipboardInserter {
    /// Build one, or `None` where this platform has no synthetic paste.
    ///
    /// `None` rather than a no-op: a caller that gets it must tell the user text
    /// insertion will not work here, instead of pasting into the void.
    #[must_use]
    pub fn new() -> Self {
        Self {
            inserter: kw_platform::paste_keystroke().map(|keystroke| {
                TextInserter::new(
                    SystemClipboard,
                    keystroke,
                    kw_platform::insertion::restore_delay(),
                )
            }),
        }
    }

    /// Whether text insertion is possible on this platform at all.
    #[must_use]
    pub const fn is_supported(&self) -> bool {
        self.inserter.is_some()
    }
}

impl Default for ClipboardInserter {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl Inserter for ClipboardInserter {
    async fn insert(&self, text: &str) -> Result<InsertionResult, DictationError> {
        let Some(inserter) = &self.inserter else {
            return Err(DictationError::Insertion(
                "this platform has no synthetic paste, so dictated text cannot be placed at the \
                 cursor. The transcript is in your history."
                    .to_owned(),
            ));
        };

        let insertion = inserter
            .insert(text)
            .await
            .map_err(|err| DictationError::Insertion(err.to_string()))?;

        Ok(InsertionResult {
            clipboard_restored: matches!(
                insertion.clipboard,
                kw_platform::insertion::ClipboardRestore::Restored
                    | kw_platform::insertion::ClipboardRestore::NothingToRestore
            ),
        })
    }
}

/* ------------------------------------------------------------------ history */

/// Writes history rows, when history is switched on.
#[derive(Debug)]
pub struct DbHistory {
    db: Db,
}

/// The settings key that switches history off.
///
/// On by default, and read per dictation by [`DbHistory::enabled`], so switching
/// it off stops the next thing said from being written down rather than taking
/// effect after a relaunch.
///
/// **Switching it off writes nothing new and removes nothing.** Rows already in
/// the table stay until `history_clear` deletes them. The distinction matters
/// because the two controls sit next to each other on the Dictation screen and
/// sound alike; the screen says which is which, and so does this.
///
/// Until 2026-08-31 nothing could set this key. The gate was honoured on every
/// write and the only way past it was to edit SQLite by hand -- the third
/// instance of the same pattern in this repo, after the four hotkeys that
/// registered and dispatched nowhere and `keep_raw_text` below.
pub const HISTORY_ENABLED_KEY: &str = "history.enabled";

/// The settings key that keeps the pre-cleanup transcript.
///
/// Off by default: it stores a second copy of what someone said, and the point
/// of cleanup is that the tidy version is the one wanted. Switched on, a history
/// row that carries a raw text is proof cleanup changed something -- which is
/// the question a bench check on 2026-08-28 needed a second transcriber to
/// answer, because this setting existed in `kw_app_core` and had no way in.
pub const KEEP_RAW_TEXT_KEY: &str = "history.keepRawText";

impl DbHistory {
    #[must_use]
    pub const fn new(db: Db) -> Self {
        Self { db }
    }
}

#[async_trait::async_trait]
impl History for DbHistory {
    async fn enabled(&self) -> bool {
        // Defaults to on, matching upstream. Switching it off is a deliberate
        // act, and a user who has never opened settings expects to be able to
        // find what they just dictated.
        kw_storage::settings::get_or(self.db.pool(), HISTORY_ENABLED_KEY, true)
            .await
            .unwrap_or(true)
    }

    async fn keep_raw_text(&self) -> bool {
        // Read per dictation, like `enabled` above, so switching it on applies
        // to the next thing said rather than after a relaunch.
        kw_storage::settings::get_or(self.db.pool(), KEEP_RAW_TEXT_KEY, false)
            .await
            .unwrap_or(false)
    }

    async fn record(&self, entry: HistoryEntry) -> Option<i64> {
        let status = if entry.failed {
            transcriptions::Status::Failed
        } else {
            transcriptions::Status::Completed
        };

        let row = transcriptions::NewTranscription {
            text: entry.text,
            raw_text: entry.raw_text,
            has_audio: false,
            audio_duration_ms: entry.duration_ms,
            provider: entry.provider,
            model: entry.model,
            error_message: entry.error,
            error_code: None,
            route_kind: Some("dictation".to_owned()),
        };

        match transcriptions::record(self.db.pool(), &row, status).await {
            Ok(id) => Some(id),
            Err(err) => {
                // Not fatal: the text is already at the cursor. Losing the
                // history row is a smaller harm than failing the dictation.
                tracing::error!(error = %err, "could not write the history row");
                None
            }
        }
    }
}

/* --------------------------------------------------------------- state sink */

/// Emits state changes to the frontend and keeps the overlay honest.
///
/// The overlay rule is enforced here rather than left to the UI: whenever the
/// session is recording, the overlay is shown. The local user always knows the
/// microphone is open.
pub struct EventSink {
    app: AppHandle,
    state: Arc<RwLock<SessionState>>,
}

impl std::fmt::Debug for EventSink {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EventSink").finish_non_exhaustive()
    }
}

impl EventSink {
    #[must_use]
    pub const fn new(app: AppHandle, state: Arc<RwLock<SessionState>>) -> Self {
        Self { app, state }
    }
}

impl StateSink for EventSink {
    fn state_changed(&self, state: SessionState) {
        if let Ok(mut held) = self.state.write() {
            *held = state;
        }

        // Shown for the whole of a recording, hidden as soon as it is over.
        // `OverlayStatus::recording_indicator_visible` reads the same value, so
        // what the settings screen reports is what actually happened.
        crate::overlay::set_visible(&self.app, state.is_recording());

        // The tray is the only indicator that is always on screen, so it says so
        // too. Through `dictating` rather than a setter, because a meeting may
        // also be recording and this runtime knows nothing about that.
        crate::tray::dictating(&self.app, state.is_recording());

        if let Err(err) = self.app.emit(STATE_EVENT, state) {
            tracing::warn!(error = %err, "could not report the dictation state");
        }
    }
}

/* ----------------------------------------------------------------- runtime */

/// The settings key holding the whole transcription route configuration.
///
/// One key rather than ten, because `RouteSettings` already deserialises as a
/// unit and splitting it would mean ten chances for the stored shape and the
/// struct to drift apart.
pub const ROUTE_SETTINGS_KEY: &str = "transcription.route";

/// The three places local work can go, gathered.
///
/// They travel together and always have: a dictation either goes to the whisper
/// server on this machine or to a provider, and its cleanup either goes to the
/// llama server on this machine or to a provider. Passing them as three
/// parameters made `build` an eight-argument function, which is a shape that
/// invites the next one — and the next one is the sherpa server.
#[derive(Debug, Clone)]
pub struct LocalTargets {
    /// Where the whisper server is listening, if it is.
    pub transcription_port: LocalPort,
    /// Which model that server loaded.
    pub transcription_model: Arc<RwLock<Option<String>>>,
    /// Which sherpa model is selected, if any. Outranks the whisper server --
    /// see `local_asr`'s module doc on why there is only ever one local engine.
    pub transcription_sherpa: crate::local_asr::Selection,
    /// How to start the whisper server again if it has gone. See
    /// `local_model::Revive`.
    pub transcription_revive: Arc<dyn crate::local_model::Revive>,
    /// Where the llama server is listening, if it is.
    pub generation: crate::local_llm::LocalEndpoint,
}

/// The dictation session, assembled.
pub struct DictationRuntime {
    session: Arc<kw_app_core::dictation::Dictation>,
    recorder: Arc<MicRecorder>,
    db: Db,
    /// The live cleanup destination, shared with the `ModelCleanup` inside
    /// `session`. Held here so `reasoning::reasoning_set_cleanup` can change it
    /// without a restart — a setting that only takes effect after a relaunch is
    /// not what "Save" means.
    cleanup: crate::reasoning::CleanupSlot,
}

impl std::fmt::Debug for DictationRuntime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DictationRuntime")
            .field("state", &self.session.state())
            .finish_non_exhaustive()
    }
}

impl DictationRuntime {
    /// Assemble the session from the real adapters.
    ///
    /// Built in `setup` rather than in `AppState::open`, because the state sink
    /// needs an `AppHandle` and that does not exist until Tauri has started.
    #[must_use]
    pub fn build(
        app: AppHandle,
        db: Db,
        credentials: Arc<dyn CredentialStore>,
        policy: NetworkPolicy,
        local: LocalTargets,
        cleanup: Arc<RwLock<Option<(kw_reasoning_core::ChatRoute, String)>>>,
    ) -> Self {
        let LocalTargets {
            transcription_port: local_port,
            transcription_model: local_model,
            transcription_sherpa: local_sherpa,
            transcription_revive: local_revive,
            generation: local_llm,
        } = local;

        let recorder = Arc::new(MicRecorder::spawn());
        let state = Arc::new(RwLock::new(SessionState::Idle));

        let session = kw_app_core::dictation::Dictation::new(
            Arc::clone(&recorder) as Arc<dyn Recorder>,
            Arc::new(RouteTranscriber::new(
                policy.clone(),
                Arc::clone(&credentials),
                local_port,
                local_model,
                local_sherpa,
                local_revive,
            )),
            Arc::new(ModelCleanup::new(
                policy,
                credentials,
                Arc::clone(&cleanup),
                local_llm,
            )),
            Arc::new(ClipboardInserter::new()),
            Arc::new(DbHistory::new(db.clone())),
            Arc::new(EventSink::new(app, state)),
        );

        Self {
            session: Arc::new(session),
            recorder,
            db,
            cleanup,
        }
    }

    /// The shared cleanup destination, for the command that changes it.
    #[must_use]
    pub fn cleanup_slot(&self) -> &crate::reasoning::CleanupSlot {
        &self.cleanup
    }

    /// What the session is doing.
    #[must_use]
    pub fn state(&self) -> SessionState {
        self.session.state()
    }

    /// Choose the input device for the next recording.
    pub fn set_device(&self, device: Option<String>) {
        self.recorder.set_device(device);
    }

    /// Read the stored route configuration.
    async fn route_settings(&self) -> kw_transcription_core::RouteSettings {
        // A missing or unreadable setting means "never configured", which the
        // resolver already handles by defaulting to OpenAI — and which fails
        // closed later if no credential is stored.
        kw_storage::settings::get(self.db.pool(), ROUTE_SETTINGS_KEY)
            .await
            .ok()
            .flatten()
            .unwrap_or_default()
    }

    /// Start if idle, finish if recording.
    ///
    /// One entry point because that is what a single hotkey means. A press while
    /// transcribing is ignored rather than queued: the user pressed it again
    /// because nothing had happened yet, and starting a second recording on top
    /// of an in-flight one is never what they wanted.
    pub async fn toggle(&self) -> Result<kw_app_core::dictation::Outcome, String> {
        match self.state() {
            SessionState::Idle => {
                self.session.start().await.map_err(|err| err.to_string())?;
                Ok(kw_app_core::dictation::Outcome::Cancelled)
            }

            SessionState::Recording => {
                let registry = kw_provider_registry::registry().map_err(|err| err.to_string())?;
                let settings = self.route_settings().await;

                let route = match kw_transcription_core::route::resolve(
                    registry,
                    &settings,
                    &kw_transcription_core::RouteRequest::default(),
                ) {
                    Ok(route) => route,
                    Err(err) => {
                        // The recording has to be dealt with either way: leaving
                        // the microphone open because the route was
                        // misconfigured would be the worst of both.
                        self.session.cancel().await;
                        return Err(err.to_string());
                    }
                };

                Ok(self.session.finish(&route).await)
            }

            busy => Err(format!(
                "a dictation is already {}",
                match busy {
                    SessionState::Transcribing => "being transcribed",
                    _ => "being inserted",
                }
            )),
        }
    }

    /// Throw away the recording.
    pub async fn cancel(&self) -> kw_app_core::dictation::Outcome {
        self.session.cancel().await
    }
}

/// Where a dictation would go right now.
///
/// The *resolved* route rather than the stored setting, because the two are not
/// the same thing: a mode of "local" with no server running, or a provider with
/// no credential, resolves differently from what the setting says. Showing the
/// setting would be showing an intention.
#[derive(Debug, Clone, serde::Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct RouteReport {
    /// A short phrase for the UI: `local model`, `openai`, and so on.
    pub destination: String,
    /// Whether the resolved route is the local model server.
    ///
    /// A boolean rather than leaving the screen to match `destination` against
    /// the string `"local model"`: that phrase exists to be read by a person
    /// and rewording it must not silently change what a control does.
    pub is_local: bool,
    /// Whether audio leaves this machine on this route.
    pub leaves_device: bool,
    /// Present when the route cannot be resolved at all.
    pub problem: Option<String>,
}

/// Describe the resolved transcription route.
#[tauri::command]
pub async fn dictation_route(
    runtime: tauri::State<'_, Arc<DictationRuntime>>,
) -> Result<RouteReport, String> {
    let registry = kw_provider_registry::registry().map_err(|err| err.to_string())?;
    let settings = runtime.route_settings().await;

    match kw_transcription_core::route::resolve(
        registry,
        &settings,
        &kw_transcription_core::RouteRequest::default(),
    ) {
        Ok(Transport::Local) => Ok(RouteReport {
            destination: "local model".to_owned(),
            is_local: true,
            leaves_device: false,
            problem: None,
        }),
        Ok(Transport::ProviderClient { provider, .. }) => Ok(RouteReport {
            destination: format!("{provider:?}").to_lowercase(),
            is_local: false,
            leaves_device: true,
            problem: None,
        }),
        Ok(Transport::HttpBatch {
            provider, endpoint, ..
        }) => Ok(RouteReport {
            // The endpoint matters: a self-hosted route on the user's own
            // network leaves this machine but not their control, and lumping it
            // in with a cloud provider would be misleading either way.
            leaves_device: !kw_network_policy::is_private_host(
                url::Url::parse(&endpoint)
                    .ok()
                    .and_then(|u| u.host_str().map(str::to_owned))
                    .unwrap_or_default()
                    .as_str(),
            ),
            destination: provider,
            is_local: false,
            problem: None,
        }),
        Err(err) => Ok(RouteReport {
            destination: "nowhere".to_owned(),
            is_local: false,
            leaves_device: false,
            problem: Some(err.to_string()),
        }),
    }
}

/// Send dictations to the local model, or stop doing so.
///
/// Explicit rather than implied by the server running: a server that is up is
/// not the same as a route that points at it, and a button that starts one while
/// silently rewriting the other would be doing two things under one label.
#[tauri::command]
pub async fn dictation_use_local(
    runtime: tauri::State<'_, Arc<DictationRuntime>>,
    use_local: bool,
) -> Result<(), String> {
    let mut settings = runtime.route_settings().await;
    settings.use_local_whisper = use_local;
    settings.transcription_mode = Some(if use_local { "local" } else { "providers" }.to_owned());

    kw_storage::settings::set(runtime.db.pool(), ROUTE_SETTINGS_KEY, &settings)
        .await
        .map_err(|err| err.to_string())
}

/// Point dictation at a cloud provider and model.
///
/// Both together, never one at a time. A provider change that left the previous
/// provider's model behind would resolve to a model the new provider does not
/// offer, and `resolve_model` would refuse — correctly, but for a reason the
/// user never chose. The frontend picks the model from the registry's list for
/// that provider, so an id is never typed.
///
/// Switching here also switches the mode off local, because choosing a cloud
/// provider while dictation is routed to a local model would change a setting
/// that has no effect.
#[tauri::command]
pub async fn dictation_set_cloud_provider(
    runtime: tauri::State<'_, Arc<DictationRuntime>>,
    provider: String,
    model: String,
    // Corti only, and ignored for every other provider.
    corti_environment: Option<String>,
    corti_tenant: Option<String>,
) -> Result<(), String> {
    let registry = kw_provider_registry::registry().map_err(|err| err.to_string())?;

    // Fail closed on an unknown id rather than storing it and discovering the
    // problem at the end of someone's next dictation.
    let resolved = registry
        .resolve_transcription_model(&provider, &model)
        .map_err(|err| err.to_string())?;

    let mut settings = runtime.route_settings().await;
    settings.cloud_transcription_provider = Some(provider.clone());
    settings.cloud_transcription_model = Some(resolved);
    settings.use_local_whisper = false;
    settings.transcription_mode = Some("providers".to_owned());

    // Corti alone needs two more values, and neither has a safe default: the
    // environment is in the hostname and the tenant scopes the account, so a
    // guess reaches a real endpoint under somebody else's tenancy. Refused here
    // rather than stored and discovered at the end of a dictation.
    if provider == "corti" {
        let environment = corti_environment
            .as_deref()
            .map(str::trim)
            .filter(|v| kw_transcription_core::clients::is_valid_corti_environment(v))
            .ok_or("Corti needs an environment that is one DNS label, for example \"eu\".")?;
        let tenant = corti_tenant
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .ok_or("Corti needs a tenant name; it scopes the request to your account.")?;

        settings.corti_environment = Some(environment.to_owned());
        settings.corti_tenant = Some(tenant.to_owned());
    }

    kw_storage::settings::set(runtime.db.pool(), ROUTE_SETTINGS_KEY, &settings)
        .await
        .map_err(|err| err.to_string())
}

/* ---------------------------------------------------------------- commands */

/// What the session is doing, for the settings screen.
#[tauri::command]
pub fn dictation_state(runtime: tauri::State<'_, Arc<DictationRuntime>>) -> SessionState {
    runtime.state()
}

/// Start or finish a dictation.
#[tauri::command]
pub async fn dictation_toggle(
    runtime: tauri::State<'_, Arc<DictationRuntime>>,
) -> Result<kw_app_core::dictation::Outcome, String> {
    runtime.toggle().await
}

/// Abandon the recording.
#[tauri::command]
pub async fn dictation_cancel(
    runtime: tauri::State<'_, Arc<DictationRuntime>>,
) -> Result<kw_app_core::dictation::Outcome, String> {
    Ok(runtime.cancel().await)
}

/// The microphones this machine offers.
#[tauri::command]
pub fn input_devices() -> Result<Vec<kw_audio_core::InputDevice>, String> {
    kw_audio_core::input_devices().map_err(|err| err.to_string())
}

/// Choose the microphone for the next recording.
#[tauri::command]
pub fn dictation_set_device(
    runtime: tauri::State<'_, Arc<DictationRuntime>>,
    device: Option<String>,
) -> Result<(), String> {
    runtime.set_device(device);
    Ok(())
}

/// Whether text insertion works on this platform at all.
///
/// Answered up front so the settings screen can say so, rather than the user
/// finding out at the end of their first dictation.
#[tauri::command]
pub fn insertion_supported() -> bool {
    ClipboardInserter::new().is_supported()
}

/// Drive a dictation from the global hotkey.
///
/// Spawned rather than awaited: this is called from the shortcut plugin's
/// synchronous callback, which must return promptly or the whole hotkey system
/// stalls. The outcome is emitted, not returned, because there is nobody here to
/// return it to.
pub fn on_hotkey(app: &AppHandle) {
    let Some(runtime) = app
        .try_state::<Arc<DictationRuntime>>()
        .map(|s| Arc::clone(&s))
    else {
        tracing::error!("a dictation hotkey fired before the session was ready");
        return;
    };

    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        match runtime.toggle().await {
            Ok(outcome) => {
                if let Err(err) = app.emit(OUTCOME_EVENT, outcome) {
                    tracing::warn!(error = %err, "could not report the dictation outcome");
                }
            }
            Err(message) => {
                tracing::warn!(%message, "the dictation could not proceed");
                if let Err(err) = app.emit(
                    OUTCOME_EVENT,
                    kw_app_core::dictation::Outcome::Failed {
                        message,
                        history_id: None,
                    },
                ) {
                    tracing::warn!(error = %err, "could not report the dictation failure");
                }
            }
        }
    });
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

    /* --------------------------------------------- which local engine */

    #[test]
    fn a_selected_sherpa_model_never_falls_through_to_whisper() {
        // The rule two callers now share, and the reason sharing it was worth
        // doing: `meeting.rs`'s whole-recording pass had its own version that
        // knew only about `WhisperServer`, so on a machine with a Parakeet
        // model selected -- the documented one-local-engine state -- the pass
        // failed and every meeting note silently became the live partials.
        // Found by a person reading a saved note, not by anything here.
        use std::sync::{Arc, RwLock};

        let policy = || kw_network_policy::NetworkPolicy::new().expect("a policy");
        let server = || WhisperServer::new(policy(), 1, None);

        let nothing: crate::local_asr::Selection = Arc::new(RwLock::new(None));
        assert!(local_engine(&nothing, None)
            .expect("no engine at all is a state, not an error")
            .is_none());
        assert!(
            local_engine(&nothing, Some(server()))
                .expect("whisper alone is fine")
                .is_some(),
            "with no sherpa selection the whisper server is the engine"
        );

        // The assertion that matters. A selected model that cannot be built is
        // an error **even when whisper is available**, because audio going
        // somewhere the screen did not name is worse than a refusal.
        let broken: crate::local_asr::Selection =
            Arc::new(RwLock::new(Some("not-a-model".to_owned())));
        let err = local_engine(&broken, None).expect_err("a broken selection is an error");
        assert!(err.contains("not-a-model"), "{err}");
        assert!(
            local_engine(&broken, Some(server())).is_err(),
            "a broken sherpa selection must not quietly become a whisper dictation"
        );
    }

    /* ------------------------------------------- the recorded local port */

    #[test]
    fn no_recorded_port_means_no_local_engine() {
        // The state a user who dictates to a cloud provider is in for the whole
        // session. Nothing to check and nothing to start.
        assert_eq!(classify(None, false), Recorded::Absent);
        // `answering` cannot be true without a port, but if a future caller
        // manages it the answer is still "there is no server".
        assert_eq!(classify(None, true), Recorded::Absent);
    }

    #[test]
    fn a_recorded_port_that_answers_is_used_as_it_stands() {
        assert_eq!(classify(Some(8178), true), Recorded::Live(8178));
    }

    #[test]
    fn a_recorded_port_that_answers_nothing_is_gone_not_live() {
        // The defect this file's `whisper_server` exists to fix, in one line.
        // On 2026-08-28 the application held 8178 with no listener and no
        // process behind it, and every dictation was sent there anyway --
        // `start_model` had learnt to notice this and the dictation path had
        // not. The port travels with the state so the message can name it.
        assert_eq!(classify(Some(8178), false), Recorded::Gone(8178));
    }

    #[tokio::test]
    async fn history_is_on_unless_switched_off() {
        // A user who has never opened settings expects to find what they just
        // dictated. The command layer defaults to `true` for the same key, and
        // the two defaults have to stay equal or the screen reports the
        // opposite of what the sink is doing.
        let db = Db::open_in_memory().await.unwrap();
        let history = DbHistory::new(db.clone());
        assert!(history.enabled().await);

        kw_storage::settings::set(db.pool(), HISTORY_ENABLED_KEY, &false)
            .await
            .unwrap();
        // Read per dictation, so the next one is not written down -- no
        // relaunch. Until 2026-08-31 this was the only way to get here, because
        // nothing in the application could write the key.
        assert!(!history.enabled().await);

        kw_storage::settings::set(db.pool(), HISTORY_ENABLED_KEY, &true)
            .await
            .unwrap();
        // Back on. A switch that only goes one way is not a switch, and the one
        // this replaced went neither.
        assert!(history.enabled().await);
    }

    #[tokio::test]
    async fn switching_history_off_removes_nothing_already_written() {
        // The claim the Dictation screen makes in words next to the toggle, and
        // the whole difference between it and Clear history. Off stops new
        // rows; it is not a delete.
        let db = Db::open_in_memory().await.unwrap();
        let history = DbHistory::new(db.clone());

        let id = history
            .record(HistoryEntry {
                text: "said before it was switched off".to_owned(),
                raw_text: None,
                provider: None,
                model: None,
                duration_ms: None,
                error: None,
                failed: false,
            })
            .await
            .expect("an id");

        kw_storage::settings::set(db.pool(), HISTORY_ENABLED_KEY, &false)
            .await
            .unwrap();

        assert!(!history.enabled().await);
        let row = transcriptions::get(db.pool(), id).await.unwrap();
        assert_eq!(
            row.map(|r| r.text).as_deref(),
            Some("said before it was switched off"),
        );
    }

    #[tokio::test]
    async fn the_pre_cleanup_transcript_is_not_kept_unless_asked_for() {
        // The default, and the reason it is the default: switching it on stores
        // a second copy of what someone said. Before this setting existed the
        // answer was `false` with no way to change it, which is how a bench
        // check ended up needing a second transcriber to find out whether
        // cleanup had done anything.
        let db = Db::open_in_memory().await.unwrap();
        let history = DbHistory::new(db.clone());
        assert!(!history.keep_raw_text().await);

        kw_storage::settings::set(db.pool(), KEEP_RAW_TEXT_KEY, &true)
            .await
            .unwrap();
        // Read per dictation, so the next one keeps it -- no relaunch.
        assert!(history.keep_raw_text().await);
    }

    #[tokio::test]
    async fn a_recorded_entry_round_trips_with_its_status() {
        let db = Db::open_in_memory().await.unwrap();
        let history = DbHistory::new(db.clone());

        let id = history
            .record(HistoryEntry {
                text: "hello there".to_owned(),
                raw_text: None,
                provider: Some("local".to_owned()),
                model: Some("tiny".to_owned()),
                duration_ms: Some(1500),
                error: None,
                failed: false,
            })
            .await
            .expect("an id");

        let row = transcriptions::get(db.pool(), id).await.unwrap().unwrap();
        assert_eq!(row.text, "hello there");
        assert_eq!(row.status, "completed");
        assert_eq!(row.route_kind.as_deref(), Some("dictation"));
        assert!(!row.has_audio);
    }

    #[tokio::test]
    async fn a_failure_is_recorded_as_failed_with_its_reason() {
        // The row is how a user finds out why nothing was inserted.
        let db = Db::open_in_memory().await.unwrap();
        let history = DbHistory::new(db.clone());

        let id = history
            .record(HistoryEntry {
                text: String::new(),
                raw_text: None,
                provider: Some("openai".to_owned()),
                model: None,
                duration_ms: Some(900),
                error: Some("provider returned 429".to_owned()),
                failed: true,
            })
            .await
            .expect("an id");

        let row = transcriptions::get(db.pool(), id).await.unwrap().unwrap();
        assert_eq!(row.status, "failed");
        assert_eq!(row.error_message.as_deref(), Some("provider returned 429"));
    }

    #[tokio::test]
    async fn history_never_claims_to_have_kept_audio() {
        // Nothing on the dictation path retains the recording, so `has_audio`
        // must not say otherwise -- a row claiming audio that is not there sends
        // the user looking for a file that never existed.
        let db = Db::open_in_memory().await.unwrap();
        let id = DbHistory::new(db.clone())
            .record(HistoryEntry {
                text: "x".to_owned(),
                raw_text: None,
                provider: None,
                model: None,
                duration_ms: None,
                error: None,
                failed: false,
            })
            .await
            .unwrap();

        assert!(
            !transcriptions::get(db.pool(), id)
                .await
                .unwrap()
                .unwrap()
                .has_audio
        );
    }

    #[test]
    fn insertion_reports_whether_this_platform_can_do_it_at_all() {
        // On Windows this is supported; elsewhere it is honestly not, and the
        // settings screen has to be able to say so rather than failing at the
        // end of a dictation.
        let inserter = ClipboardInserter::new();
        assert_eq!(inserter.is_supported(), cfg!(target_os = "windows"));
    }

    #[tokio::test]
    async fn an_unsupported_platform_refuses_insertion_and_says_where_the_text_went() {
        if ClipboardInserter::new().is_supported() {
            return;
        }
        let err = ClipboardInserter::new().insert("hello").await.unwrap_err();
        assert!(err.to_string().contains("in your history"));
    }

    #[tokio::test]
    async fn cleanup_returns_the_transcript_unchanged_when_it_is_switched_off() {
        // Off is the default, and the raw transcript is the right answer rather
        // than an error.
        let policy = NetworkPolicy::new().unwrap();
        let credentials: Arc<dyn CredentialStore> =
            Arc::new(kw_security::MemoryCredentialStore::new());
        let cleanup = ModelCleanup::new(
            policy,
            credentials,
            Arc::new(RwLock::new(None)),
            crate::local_llm::LocalEndpoint::default(),
        );

        assert_eq!(cleanup.clean("um, hello there").await, "um, hello there");
    }

    #[tokio::test]
    async fn the_microphone_thread_answers_even_with_no_microphone() {
        // CI has no input device. The point is that the thread replies rather
        // than hanging, so a dictation fails with a message instead of wedging
        // the hotkey forever.
        let recorder = MicRecorder::spawn();
        match recorder.start().await {
            Ok(()) => {
                // A real device exists on this machine; close it again.
                recorder.cancel().await;
            }
            Err(err) => assert!(matches!(err, DictationError::Microphone(_))),
        }
    }

    #[tokio::test]
    async fn finishing_without_starting_is_an_error_rather_than_a_hang() {
        let recorder = MicRecorder::spawn();
        let err = recorder.finish().await.unwrap_err();
        assert!(err.to_string().contains("not open"), "{err}");
    }

    #[tokio::test]
    async fn cancelling_twice_is_harmless() {
        let recorder = MicRecorder::spawn();
        recorder.cancel().await;
        recorder.cancel().await;
    }
}
