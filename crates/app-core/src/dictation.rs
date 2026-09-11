//! The dictation session: what happens between pressing the hotkey and text
//! appearing at the cursor.
//!
//! ```text
//! Idle --press--> Recording --press--> Transcribing --> Inserting --> Idle
//!                     |
//!                  cancel
//!                     v
//!                   Idle
//! ```
//!
//! Every collaborator is a trait, so the whole flow is exercised end to end in
//! this file's tests with no microphone, no network, no clipboard, and no
//! keychain. That is the point of the design rather than a side effect of it:
//! the Phase 2 acceptance criteria are mostly statements about *sequence* — the
//! clipboard is restored, history is written only when enabled, a skipped
//! recording does not reach a provider — and sequence is exactly what is
//! untestable once these are concrete calls into the OS.
//!
//! Two rules the state machine enforces rather than trusting callers with:
//!
//! * **A gate-skipped recording never reaches a provider.** Silence costs money
//!   and comes back as a hallucinated transcript often enough to matter.
//! * **A failure still produces a history row.** A dictation that vanished with
//!   no trace is the worst outcome; the row is how the user finds out why.

use std::sync::Arc;

use kw_audio_core::{GateDecision, GateReason};
use kw_transcription_core::{RouteError, Transport};
use serde::Serialize;
use ts_rs::TS;

/// What the session is doing. Drives the overlay.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum SessionState {
    Idle,
    /// The microphone is live. The overlay is visible in this state, always.
    Recording,
    Transcribing,
    Inserting,
}

impl SessionState {
    /// Whether the microphone is open, and therefore whether the user must be
    /// able to see that it is.
    #[must_use]
    pub const fn is_recording(self) -> bool {
        matches!(self, Self::Recording)
    }

    /// Whether a new dictation can start.
    #[must_use]
    pub const fn is_busy(self) -> bool {
        !matches!(self, Self::Idle)
    }
}

/// How a dictation ended, as reported to the UI.
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[serde(
    tag = "outcome",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum Outcome {
    /// Text was produced and inserted.
    Inserted {
        text: String,
        /// Whether the user's clipboard came back as it was.
        clipboard_restored: bool,
        history_id: Option<i64>,
    },
    /// The recording held no speech, so nothing was sent anywhere.
    NothingToTranscribe { reason: GateReason },
    /// The user cancelled.
    Cancelled,
    /// It failed. The message is safe to show.
    Failed {
        message: String,
        history_id: Option<i64>,
    },
}

/// Failures that stop a dictation.
#[derive(Debug, thiserror::Error)]
pub enum DictationError {
    #[error("a dictation is already in progress")]
    Busy,

    #[error("no dictation is in progress")]
    NotRecording,

    #[error("could not start the microphone: {0}")]
    Microphone(String),

    #[error(transparent)]
    Route(#[from] RouteError),

    #[error("transcription failed: {0}")]
    Transcription(String),

    #[error("could not insert the text: {0}")]
    Insertion(String),
}

/* --------------------------------------------------------------- the seams */

/// Records audio.
#[async_trait::async_trait]
pub trait Recorder: Send + Sync {
    /// Open the microphone.
    async fn start(&self) -> Result<(), DictationError>;
    /// Close it and return what was captured.
    async fn finish(&self) -> Result<Capture, DictationError>;
    /// Close it and discard.
    async fn cancel(&self);
}

/// A finished recording.
#[derive(Debug, Clone)]
pub struct Capture {
    /// 16-bit mono PCM at 16 kHz, in a WAV container.
    pub wav: Vec<u8>,
    pub duration_ms: u64,
    pub gate: GateDecision,
    pub truncated: bool,
}

/// Turns audio into text.
#[async_trait::async_trait]
pub trait Transcriber: Send + Sync {
    async fn transcribe(
        &self,
        transport: &Transport,
        wav: Vec<u8>,
    ) -> Result<Transcribed, DictationError>;
}

/// What a transcriber produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Transcribed {
    pub text: String,
    pub provider: String,
    pub model: Option<String>,
}

/// Optionally tidies a transcript before it is inserted.
#[async_trait::async_trait]
pub trait Cleanup: Send + Sync {
    /// Return the cleaned text, or the original if cleanup is off or fails.
    ///
    /// Deliberately infallible: a cleanup that errors must not lose the
    /// transcript. Returning the raw text is always an acceptable answer.
    async fn clean(&self, text: &str) -> String;
}

/// Puts text at the cursor.
#[async_trait::async_trait]
pub trait Inserter: Send + Sync {
    async fn insert(&self, text: &str) -> Result<InsertionResult, DictationError>;
}

/// What insertion achieved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InsertionResult {
    /// Whether the user's previous clipboard content was put back.
    pub clipboard_restored: bool,
}

/// Records what happened, if the user wants a history.
#[async_trait::async_trait]
pub trait History: Send + Sync {
    /// Whether history is switched on. Checked before anything is written.
    async fn enabled(&self) -> bool;

    /// Whether the pre-cleanup transcript is kept alongside the cleaned one.
    ///
    /// Asked of the sink rather than held on [`Dictation`], for two reasons.
    /// It is the same kind of question as [`Self::enabled`] -- what this store
    /// keeps -- and it is read per dictation, so switching it on takes effect at
    /// once. It used to be a `bool` fixed when `Dictation` was built and
    /// hardcoded `false` at the only real call site, which made it a capability
    /// with no way in: the field existed, the column existed, the IPC type
    /// carried it, and nothing could ever set it.
    ///
    /// **Defaults to `false`,** so an implementation that does not answer keeps
    /// one copy of what someone said rather than two. Storing a second copy of
    /// a person's speech is the kind of thing to be opted into, not out of.
    async fn keep_raw_text(&self) -> bool {
        false
    }

    /// Write a row and return its id.
    async fn record(&self, entry: HistoryEntry) -> Option<i64>;
}

/// One history row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryEntry {
    pub text: String,
    /// The pre-cleanup transcript, only when the user asked to keep it.
    pub raw_text: Option<String>,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub duration_ms: Option<i64>,
    pub error: Option<String>,
    pub failed: bool,
}

/// Reports state changes so the overlay can follow them.
pub trait StateSink: Send + Sync {
    fn state_changed(&self, state: SessionState);
}

/* ------------------------------------------------------------ the machine */

/// Everything a session needs.
pub struct Dictation {
    recorder: Arc<dyn Recorder>,
    transcriber: Arc<dyn Transcriber>,
    cleanup: Arc<dyn Cleanup>,
    inserter: Arc<dyn Inserter>,
    history: Arc<dyn History>,
    sink: Arc<dyn StateSink>,
    state: std::sync::Mutex<SessionState>,
}

impl std::fmt::Debug for Dictation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Dictation")
            .field("state", &self.state())
            .finish_non_exhaustive()
    }
}

impl Dictation {
    pub fn new(
        recorder: Arc<dyn Recorder>,
        transcriber: Arc<dyn Transcriber>,
        cleanup: Arc<dyn Cleanup>,
        inserter: Arc<dyn Inserter>,
        history: Arc<dyn History>,
        sink: Arc<dyn StateSink>,
    ) -> Self {
        Self {
            recorder,
            transcriber,
            cleanup,
            inserter,
            history,
            sink,
            state: std::sync::Mutex::new(SessionState::Idle),
        }
    }

    #[must_use]
    pub fn state(&self) -> SessionState {
        *self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn set_state(&self, next: SessionState) {
        *self.state.lock().unwrap_or_else(|e| e.into_inner()) = next;
        self.sink.state_changed(next);
    }

    /// Start recording.
    pub async fn start(&self) -> Result<(), DictationError> {
        if self.state().is_busy() {
            return Err(DictationError::Busy);
        }

        self.recorder.start().await?;
        // The state is set only after the microphone is actually open, so the
        // overlay never shows "recording" over a failed start.
        self.set_state(SessionState::Recording);
        Ok(())
    }

    /// Discard the recording without transcribing.
    pub async fn cancel(&self) -> Outcome {
        if !self.state().is_recording() {
            return Outcome::Cancelled;
        }
        self.recorder.cancel().await;
        self.set_state(SessionState::Idle);
        Outcome::Cancelled
    }

    /// Stop recording and run the rest of the pipeline.
    ///
    /// Always returns to [`SessionState::Idle`], on every path, so a failure
    /// cannot leave the session wedged and the hotkey dead.
    pub async fn finish(&self, route: &Transport) -> Outcome {
        if !self.state().is_recording() {
            return Outcome::Failed {
                message: DictationError::NotRecording.to_string(),
                history_id: None,
            };
        }

        let outcome = self.run(route).await;
        self.set_state(SessionState::Idle);
        outcome
    }

    async fn run(&self, route: &Transport) -> Outcome {
        let capture = match self.recorder.finish().await {
            Ok(capture) => capture,
            Err(err) => return self.fail(err.to_string(), None, None).await,
        };

        if capture.truncated {
            // Not an error -- the audio up to the cap is still transcribed --
            // but the user should know their recording was cut short.
            tracing::warn!(
                duration_ms = capture.duration_ms,
                "the recording reached the maximum length and was truncated"
            );
        }

        // A gate-skipped recording never reaches a provider.
        if capture.gate.skip {
            tracing::info!(
                reason = ?capture.gate.reason,
                duration_ms = capture.duration_ms,
                "no speech detected; nothing was sent"
            );
            return Outcome::NothingToTranscribe {
                reason: capture.gate.reason,
            };
        }

        self.set_state(SessionState::Transcribing);

        let transcribed = match self.transcriber.transcribe(route, capture.wav).await {
            Ok(t) => t,
            Err(err) => {
                #[allow(clippy::cast_possible_wrap)]
                let duration = capture.duration_ms as i64;
                return self.fail(err.to_string(), Some(duration), None).await;
            }
        };

        if transcribed.text.trim().is_empty() {
            // The provider answered and heard nothing. Not a failure, and not
            // worth pasting an empty string over the user's selection.
            tracing::info!(provider = transcribed.provider, "the transcript was empty");
            return Outcome::NothingToTranscribe {
                reason: GateReason::InsufficientSpeech,
            };
        }

        let raw = transcribed.text.clone();
        let cleaned = self.cleanup.clean(&transcribed.text).await;

        self.set_state(SessionState::Inserting);

        #[allow(clippy::cast_possible_wrap)]
        let duration = capture.duration_ms as i64;

        let insertion = match self.inserter.insert(&cleaned).await {
            Ok(result) => result,
            Err(err) => {
                // The text exists; only the paste failed. Recording it means the
                // user can still copy it out of the history rather than losing
                // what they said.
                return self
                    .fail(
                        err.to_string(),
                        Some(duration),
                        Some((cleaned, raw, transcribed.provider, transcribed.model)),
                    )
                    .await;
            }
        };

        let history_id = self
            .write_history(HistoryEntry {
                text: cleaned.clone(),
                raw_text: self.raw_for_history(&raw, &cleaned).await,
                provider: Some(transcribed.provider),
                model: transcribed.model,
                duration_ms: Some(duration),
                error: None,
                failed: false,
            })
            .await;

        Outcome::Inserted {
            text: cleaned,
            clipboard_restored: insertion.clipboard_restored,
            history_id,
        }
    }

    /// The pre-cleanup transcript, if it is worth keeping.
    ///
    /// Only when the user asked for it *and* cleanup actually changed something;
    /// storing an identical copy is pure duplication of user speech. The second
    /// half of that is why this is worth having at all: a row that carries a raw
    /// text is a row where cleanup demonstrably did something.
    async fn raw_for_history(&self, raw: &str, cleaned: &str) -> Option<String> {
        if raw == cleaned {
            return None;
        }
        self.history.keep_raw_text().await.then(|| raw.to_owned())
    }

    /// Record a failure and return the outcome.
    ///
    /// A failed dictation still gets a history row: a dictation that vanished
    /// without a trace is the worst outcome, because the user cannot tell
    /// whether the microphone, the network, or the provider was at fault.
    async fn fail(
        &self,
        message: String,
        duration_ms: Option<i64>,
        recovered: Option<(String, String, String, Option<String>)>,
    ) -> Outcome {
        let (text, raw, provider, model) = match recovered {
            Some((text, raw, provider, model)) => (text, Some(raw), Some(provider), model),
            None => (String::new(), None, None, None),
        };

        let raw_text = match raw {
            Some(raw) => self.raw_for_history(&raw, &text).await,
            None => None,
        };

        let history_id = self
            .write_history(HistoryEntry {
                text,
                raw_text,
                provider,
                model,
                duration_ms,
                error: Some(message.clone()),
                failed: true,
            })
            .await;

        tracing::warn!(error = %message, "dictation failed");

        Outcome::Failed {
            message,
            history_id,
        }
    }

    /// Write a history row if history is enabled.
    async fn write_history(&self, entry: HistoryEntry) -> Option<i64> {
        if !self.history.enabled().await {
            // Nothing is written "just in case". Disabling history means no row.
            return None;
        }
        self.history.record(entry).await
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
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::Mutex;

    /* ------------------------------------------------------------ the fakes */

    #[derive(Default)]
    struct FakeRecorder {
        started: AtomicUsize,
        cancelled: AtomicUsize,
        capture: Mutex<Option<Capture>>,
        start_fails: bool,
        finish_fails: bool,
    }

    impl FakeRecorder {
        fn with(gate: GateDecision) -> Self {
            Self {
                capture: Mutex::new(Some(Capture {
                    wav: vec![1, 2, 3],
                    duration_ms: 1_500,
                    gate,
                    truncated: false,
                })),
                ..Default::default()
            }
        }
    }

    fn speech() -> GateDecision {
        GateDecision {
            skip: false,
            reason: GateReason::SpeechDetected,
            peak_rms: 0.2,
            peak_amplitude: 0.5,
            window_count: 30,
            speech_window_count: 25,
            max_consecutive_speech_windows: 20,
        }
    }

    fn silence() -> GateDecision {
        GateDecision {
            skip: true,
            reason: GateReason::Silence,
            peak_rms: 0.0001,
            peak_amplitude: 0.0002,
            window_count: 30,
            speech_window_count: 0,
            max_consecutive_speech_windows: 0,
        }
    }

    #[async_trait::async_trait]
    impl Recorder for FakeRecorder {
        async fn start(&self) -> Result<(), DictationError> {
            if self.start_fails {
                return Err(DictationError::Microphone("device busy".to_owned()));
            }
            self.started.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }

        async fn finish(&self) -> Result<Capture, DictationError> {
            if self.finish_fails {
                return Err(DictationError::Microphone("stream died".to_owned()));
            }
            self.capture
                .lock()
                .unwrap()
                .clone()
                .ok_or_else(|| DictationError::Microphone("nothing recorded".to_owned()))
        }

        async fn cancel(&self) {
            self.cancelled.fetch_add(1, Ordering::SeqCst);
        }
    }

    #[derive(Default)]
    struct FakeTranscriber {
        calls: AtomicUsize,
        text: Mutex<String>,
        fails: bool,
    }

    impl FakeTranscriber {
        fn saying(text: &str) -> Self {
            Self {
                text: Mutex::new(text.to_owned()),
                ..Default::default()
            }
        }
    }

    #[async_trait::async_trait]
    impl Transcriber for FakeTranscriber {
        async fn transcribe(
            &self,
            _transport: &Transport,
            _wav: Vec<u8>,
        ) -> Result<Transcribed, DictationError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            if self.fails {
                return Err(DictationError::Transcription(
                    "provider returned 500".to_owned(),
                ));
            }
            Ok(Transcribed {
                text: self.text.lock().unwrap().clone(),
                provider: "openai".to_owned(),
                model: Some("whisper-1".to_owned()),
            })
        }
    }

    #[derive(Default)]
    struct FakeCleanup {
        uppercase: bool,
    }

    #[async_trait::async_trait]
    impl Cleanup for FakeCleanup {
        async fn clean(&self, text: &str) -> String {
            if self.uppercase {
                text.to_uppercase()
            } else {
                text.to_owned()
            }
        }
    }

    #[derive(Default)]
    struct FakeInserter {
        inserted: Mutex<Vec<String>>,
        restored: bool,
        fails: bool,
    }

    #[async_trait::async_trait]
    impl Inserter for FakeInserter {
        async fn insert(&self, text: &str) -> Result<InsertionResult, DictationError> {
            if self.fails {
                return Err(DictationError::Insertion("no foreground window".to_owned()));
            }
            self.inserted.lock().unwrap().push(text.to_owned());
            Ok(InsertionResult {
                clipboard_restored: self.restored,
            })
        }
    }

    #[derive(Default)]
    struct FakeHistory {
        on: AtomicBool,
        keep_raw: AtomicBool,
        rows: Mutex<Vec<HistoryEntry>>,
    }

    impl FakeHistory {
        fn enabled() -> Self {
            Self {
                on: AtomicBool::new(true),
                ..Default::default()
            }
        }
    }

    #[async_trait::async_trait]
    impl History for FakeHistory {
        async fn enabled(&self) -> bool {
            self.on.load(Ordering::SeqCst)
        }

        async fn keep_raw_text(&self) -> bool {
            self.keep_raw.load(Ordering::SeqCst)
        }

        async fn record(&self, entry: HistoryEntry) -> Option<i64> {
            let mut rows = self.rows.lock().unwrap();
            rows.push(entry);
            Some(rows.len() as i64)
        }
    }

    #[derive(Default)]
    struct FakeSink {
        states: Mutex<Vec<SessionState>>,
    }

    impl StateSink for FakeSink {
        fn state_changed(&self, state: SessionState) {
            self.states.lock().unwrap().push(state);
        }
    }

    struct Harness {
        dictation: Dictation,
        recorder: Arc<FakeRecorder>,
        transcriber: Arc<FakeTranscriber>,
        inserter: Arc<FakeInserter>,
        history: Arc<FakeHistory>,
        sink: Arc<FakeSink>,
    }

    fn harness(
        recorder: FakeRecorder,
        transcriber: FakeTranscriber,
        inserter: FakeInserter,
        history: FakeHistory,
        cleanup: FakeCleanup,
        keep_raw: bool,
    ) -> Harness {
        let recorder = Arc::new(recorder);
        let transcriber = Arc::new(transcriber);
        let inserter = Arc::new(inserter);
        // Set on the sink now that keeping the raw text is the sink's business
        // rather than a value fixed when the session is built. The nineteen
        // callers of `harness` are unchanged, which is the point of putting it
        // here rather than in their argument lists.
        history.keep_raw.store(keep_raw, Ordering::SeqCst);
        let history = Arc::new(history);
        let sink = Arc::new(FakeSink::default());

        let dictation = Dictation::new(
            Arc::clone(&recorder) as Arc<dyn Recorder>,
            Arc::clone(&transcriber) as Arc<dyn Transcriber>,
            Arc::new(cleanup),
            Arc::clone(&inserter) as Arc<dyn Inserter>,
            Arc::clone(&history) as Arc<dyn History>,
            Arc::clone(&sink) as Arc<dyn StateSink>,
        );

        Harness {
            dictation,
            recorder,
            transcriber,
            inserter,
            history,
            sink,
        }
    }

    fn route() -> Transport {
        Transport::Local { languages: vec![] }
    }

    /// A [`History`] that answers nothing but `record`, to exercise the trait's
    /// default rather than `FakeHistory`'s override.
    #[derive(Default)]
    struct BareHistory {
        rows: Mutex<Vec<HistoryEntry>>,
    }

    #[async_trait::async_trait]
    impl History for BareHistory {
        async fn enabled(&self) -> bool {
            true
        }

        async fn record(&self, entry: HistoryEntry) -> Option<i64> {
            let mut rows = self.rows.lock().unwrap();
            rows.push(entry);
            Some(rows.len() as i64)
        }
    }

    #[tokio::test]
    async fn a_history_that_does_not_answer_keeps_one_copy() {
        // The trait's default, and it must be the private one. An
        // implementation that forgets to answer should store one copy of what
        // someone said, not two -- a second copy of a person's speech is opted
        // into, never out of.
        let history = BareHistory::default();
        assert!(!history.keep_raw_text().await);
    }

    /* ------------------------------------------------------- the happy path */

    #[tokio::test]
    async fn a_dictation_records_transcribes_inserts_and_is_recorded() {
        let h = harness(
            FakeRecorder::with(speech()),
            FakeTranscriber::saying("hello there"),
            FakeInserter {
                restored: true,
                ..Default::default()
            },
            FakeHistory::enabled(),
            FakeCleanup::default(),
            false,
        );

        h.dictation.start().await.unwrap();
        assert_eq!(h.dictation.state(), SessionState::Recording);

        let outcome = h.dictation.finish(&route()).await;

        match outcome {
            Outcome::Inserted {
                text,
                clipboard_restored,
                history_id,
            } => {
                assert_eq!(text, "hello there");
                assert!(clipboard_restored);
                assert_eq!(history_id, Some(1));
            }
            other => panic!("expected Inserted, got {other:?}"),
        }

        assert_eq!(
            h.inserter.inserted.lock().unwrap().clone(),
            vec!["hello there"]
        );
        assert_eq!(h.dictation.state(), SessionState::Idle);
    }

    #[tokio::test]
    async fn the_state_sequence_is_what_the_overlay_expects() {
        let h = harness(
            FakeRecorder::with(speech()),
            FakeTranscriber::saying("hi"),
            FakeInserter::default(),
            FakeHistory::enabled(),
            FakeCleanup::default(),
            false,
        );

        h.dictation.start().await.unwrap();
        h.dictation.finish(&route()).await;

        assert_eq!(
            h.sink.states.lock().unwrap().clone(),
            vec![
                SessionState::Recording,
                SessionState::Transcribing,
                SessionState::Inserting,
                SessionState::Idle,
            ]
        );
    }

    /* ------------------------------------------------------------- the gate */

    #[tokio::test]
    async fn a_silent_recording_never_reaches_the_provider() {
        // Silence costs money and comes back as a hallucinated transcript.
        let h = harness(
            FakeRecorder::with(silence()),
            FakeTranscriber::saying("should never be called"),
            FakeInserter::default(),
            FakeHistory::enabled(),
            FakeCleanup::default(),
            false,
        );

        h.dictation.start().await.unwrap();
        let outcome = h.dictation.finish(&route()).await;

        assert_eq!(
            outcome,
            Outcome::NothingToTranscribe {
                reason: GateReason::Silence
            }
        );
        assert_eq!(
            h.transcriber.calls.load(Ordering::SeqCst),
            0,
            "the transcriber must not have been called"
        );
        assert!(h.inserter.inserted.lock().unwrap().is_empty());
        // Nothing happened, so nothing is recorded.
        assert!(h.history.rows.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_gate_that_could_not_measure_still_transcribes() {
        // Fails open: the analyser stalling must not discard a real dictation.
        let unavailable = GateDecision {
            skip: false,
            reason: GateReason::Unavailable,
            peak_rms: 0.0,
            peak_amplitude: 0.0,
            window_count: 0,
            speech_window_count: 0,
            max_consecutive_speech_windows: 0,
        };

        let h = harness(
            FakeRecorder::with(unavailable),
            FakeTranscriber::saying("recovered"),
            FakeInserter::default(),
            FakeHistory::enabled(),
            FakeCleanup::default(),
            false,
        );

        h.dictation.start().await.unwrap();
        h.dictation.finish(&route()).await;
        assert_eq!(h.transcriber.calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn an_empty_transcript_does_not_paste_over_the_users_selection() {
        let h = harness(
            FakeRecorder::with(speech()),
            FakeTranscriber::saying("   "),
            FakeInserter::default(),
            FakeHistory::enabled(),
            FakeCleanup::default(),
            false,
        );

        h.dictation.start().await.unwrap();
        let outcome = h.dictation.finish(&route()).await;

        assert!(matches!(outcome, Outcome::NothingToTranscribe { .. }));
        assert!(
            h.inserter.inserted.lock().unwrap().is_empty(),
            "pasting an empty string would wipe a selection"
        );
    }

    /* ---------------------------------------------------------------- history */

    #[tokio::test]
    async fn disabling_history_writes_nothing_at_all() {
        let h = harness(
            FakeRecorder::with(speech()),
            FakeTranscriber::saying("private"),
            FakeInserter::default(),
            FakeHistory::default(), // off
            FakeCleanup::default(),
            false,
        );

        h.dictation.start().await.unwrap();
        let outcome = h.dictation.finish(&route()).await;

        match outcome {
            Outcome::Inserted { history_id, .. } => assert_eq!(history_id, None),
            other => panic!("{other:?}"),
        }
        assert!(h.history.rows.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_failure_is_still_recorded_so_the_user_can_see_why() {
        let h = harness(
            FakeRecorder::with(speech()),
            FakeTranscriber {
                fails: true,
                ..Default::default()
            },
            FakeInserter::default(),
            FakeHistory::enabled(),
            FakeCleanup::default(),
            false,
        );

        h.dictation.start().await.unwrap();
        let outcome = h.dictation.finish(&route()).await;

        match outcome {
            Outcome::Failed {
                message,
                history_id,
            } => {
                assert!(message.contains("500"));
                assert_eq!(history_id, Some(1));
            }
            other => panic!("{other:?}"),
        }

        let rows = h.history.rows.lock().unwrap();
        assert_eq!(rows.len(), 1);
        assert!(rows[0].failed);
        assert!(rows[0].error.is_some());
    }

    #[tokio::test]
    async fn a_failed_paste_keeps_the_transcript_in_history() {
        // The text exists and only the paste failed, so the user must be able to
        // recover what they said rather than losing it.
        let h = harness(
            FakeRecorder::with(speech()),
            FakeTranscriber::saying("important sentence"),
            FakeInserter {
                fails: true,
                ..Default::default()
            },
            FakeHistory::enabled(),
            FakeCleanup::default(),
            false,
        );

        h.dictation.start().await.unwrap();
        let outcome = h.dictation.finish(&route()).await;
        assert!(matches!(outcome, Outcome::Failed { .. }));

        let rows = h.history.rows.lock().unwrap();
        assert_eq!(
            rows[0].text, "important sentence",
            "the transcript must survive"
        );
        assert!(rows[0].failed);
    }

    #[tokio::test]
    async fn the_raw_transcript_is_kept_only_when_asked_for_and_only_if_it_differs() {
        // Keeping an identical copy is pure duplication of user speech.
        let h = harness(
            FakeRecorder::with(speech()),
            FakeTranscriber::saying("hello"),
            FakeInserter::default(),
            FakeHistory::enabled(),
            FakeCleanup { uppercase: true },
            true,
        );
        h.dictation.start().await.unwrap();
        h.dictation.finish(&route()).await;
        assert_eq!(
            h.history.rows.lock().unwrap()[0].raw_text.as_deref(),
            Some("hello")
        );

        // Cleanup changed nothing: no raw copy.
        let h = harness(
            FakeRecorder::with(speech()),
            FakeTranscriber::saying("hello"),
            FakeInserter::default(),
            FakeHistory::enabled(),
            FakeCleanup::default(),
            true,
        );
        h.dictation.start().await.unwrap();
        h.dictation.finish(&route()).await;
        assert_eq!(h.history.rows.lock().unwrap()[0].raw_text, None);

        // Not asked for: no raw copy even though cleanup changed it.
        let h = harness(
            FakeRecorder::with(speech()),
            FakeTranscriber::saying("hello"),
            FakeInserter::default(),
            FakeHistory::enabled(),
            FakeCleanup { uppercase: true },
            false,
        );
        h.dictation.start().await.unwrap();
        h.dictation.finish(&route()).await;
        assert_eq!(h.history.rows.lock().unwrap()[0].raw_text, None);
    }

    /* -------------------------------------------------------------- cleanup */

    #[tokio::test]
    async fn the_cleaned_text_is_what_gets_inserted() {
        let h = harness(
            FakeRecorder::with(speech()),
            FakeTranscriber::saying("hello there"),
            FakeInserter::default(),
            FakeHistory::enabled(),
            FakeCleanup { uppercase: true },
            false,
        );

        h.dictation.start().await.unwrap();
        h.dictation.finish(&route()).await;
        assert_eq!(
            h.inserter.inserted.lock().unwrap().clone(),
            vec!["HELLO THERE"]
        );
    }

    /* ------------------------------------------------------- state discipline */

    #[tokio::test]
    async fn a_second_start_while_recording_is_refused() {
        let h = harness(
            FakeRecorder::with(speech()),
            FakeTranscriber::saying("x"),
            FakeInserter::default(),
            FakeHistory::enabled(),
            FakeCleanup::default(),
            false,
        );

        h.dictation.start().await.unwrap();
        assert!(matches!(
            h.dictation.start().await,
            Err(DictationError::Busy)
        ));
        assert_eq!(h.recorder.started.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn a_failed_microphone_start_leaves_the_session_idle() {
        // Otherwise the overlay shows "recording" over a microphone that never
        // opened, and the hotkey is dead until restart.
        let h = harness(
            FakeRecorder {
                start_fails: true,
                ..Default::default()
            },
            FakeTranscriber::default(),
            FakeInserter::default(),
            FakeHistory::enabled(),
            FakeCleanup::default(),
            false,
        );

        assert!(h.dictation.start().await.is_err());
        assert_eq!(h.dictation.state(), SessionState::Idle);
        assert!(
            h.sink.states.lock().unwrap().is_empty(),
            "no state was announced"
        );
    }

    #[tokio::test]
    async fn every_failure_path_returns_to_idle() {
        // The session must never wedge; a wedged session is a dead hotkey.
        for (recorder, transcriber, inserter) in [
            (
                FakeRecorder {
                    finish_fails: true,
                    ..Default::default()
                },
                FakeTranscriber::default(),
                FakeInserter::default(),
            ),
            (
                FakeRecorder::with(speech()),
                FakeTranscriber {
                    fails: true,
                    ..Default::default()
                },
                FakeInserter::default(),
            ),
            (
                FakeRecorder::with(speech()),
                FakeTranscriber::saying("x"),
                FakeInserter {
                    fails: true,
                    ..Default::default()
                },
            ),
        ] {
            let h = harness(
                recorder,
                transcriber,
                inserter,
                FakeHistory::enabled(),
                FakeCleanup::default(),
                false,
            );
            h.dictation.start().await.unwrap();
            h.dictation.finish(&route()).await;
            assert_eq!(h.dictation.state(), SessionState::Idle);
        }
    }

    #[tokio::test]
    async fn finishing_when_not_recording_is_an_error_not_a_transcription() {
        let h = harness(
            FakeRecorder::with(speech()),
            FakeTranscriber::saying("x"),
            FakeInserter::default(),
            FakeHistory::enabled(),
            FakeCleanup::default(),
            false,
        );

        let outcome = h.dictation.finish(&route()).await;
        assert!(matches!(outcome, Outcome::Failed { .. }));
        assert_eq!(h.transcriber.calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn cancelling_closes_the_microphone_and_transcribes_nothing() {
        let h = harness(
            FakeRecorder::with(speech()),
            FakeTranscriber::saying("never sent"),
            FakeInserter::default(),
            FakeHistory::enabled(),
            FakeCleanup::default(),
            false,
        );

        h.dictation.start().await.unwrap();
        assert_eq!(h.dictation.cancel().await, Outcome::Cancelled);

        assert_eq!(h.recorder.cancelled.load(Ordering::SeqCst), 1);
        assert_eq!(h.transcriber.calls.load(Ordering::SeqCst), 0);
        assert_eq!(h.dictation.state(), SessionState::Idle);
        assert!(h.history.rows.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn cancelling_when_idle_is_harmless() {
        let h = harness(
            FakeRecorder::with(speech()),
            FakeTranscriber::default(),
            FakeInserter::default(),
            FakeHistory::enabled(),
            FakeCleanup::default(),
            false,
        );
        assert_eq!(h.dictation.cancel().await, Outcome::Cancelled);
        assert_eq!(h.recorder.cancelled.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn only_the_recording_state_has_the_microphone_open() {
        assert!(SessionState::Recording.is_recording());
        for state in [
            SessionState::Idle,
            SessionState::Transcribing,
            SessionState::Inserting,
        ] {
            assert!(!state.is_recording(), "{state:?}");
        }

        assert!(!SessionState::Idle.is_busy());
        assert!(SessionState::Recording.is_busy());
        assert!(SessionState::Transcribing.is_busy());
    }
}
