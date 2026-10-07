//! The one meeting provider that needs no credentials and no network.
//!
//! # What this is, and what it is not
//!
//! It is **not** streaming transcription. Upstream's local meeting provider is
//! `parakeetWsServer.js`, a sherpa-onnx WebSocket server that emits words as they
//! are recognised; that runtime is blocked on three separate things (see chunk
//! B4 in `docs/BUILD_PLAN.md`) and is not here.
//!
//! What is here is a `whisper-server` on loopback, already installed and already
//! verified on hardware, handed a five-second window every three seconds. The
//! result reads as live transcript and arrives about three seconds late. Calling
//! that "streaming" would be the kind of overclaim this project keeps finding
//! upstream, so the capability entry, the type name and the interface all say
//! *window*.
//!
//! # Why the transcript that gets saved is not made of these windows
//!
//! Windows overlap by two seconds, so consecutive windows repeat the words in
//! the overlap. Deduplicating that needs word-level timestamps and alignment,
//! and `/inference` is asked only for text. So every event here is a **partial**
//! — `is_final: false`, meaning the provider may still revise it — which a live
//! view renders by replacement, and there is nothing to deduplicate.
//!
//! The transcript that gets kept comes from one pass over the finished recording,
//! run by the caller that owns the file. That is not a workaround: it is the same
//! path a dictation takes, so a meeting transcript is exactly as good as a
//! dictation transcript rather than a stitched-together approximation of one.
//!
//! # Falling behind is a rate, not a backlog
//!
//! `tiny` transcribes five seconds in well under three. A larger model may not,
//! and then the "live" transcript drifts further behind the meeting with every
//! window until it is useless.
//!
//! The signal for that is **wall-clock time against audio time**, and getting
//! this wrong was the first version: it compared the amount of audio waiting to a
//! threshold, which meant a burst arriving all at once looked identical to a
//! transcriber that could not keep up. It is not the same thing at all — a burst
//! processed faster than it arrived is fine, and a test with an instant
//! transcriber proved the check wrong by throwing away fifty-five seconds of
//! audio it could have transcribed.
//!
//! So: one window advances the session by one hop, and spending longer than a
//! hop transcribing it means the meeting is outrunning us. The excess accumulates
//! as a deficit and time spent under a hop pays it back down. Only a sustained
//! deficit — four hops' worth — drops audio, via
//! [`WindowFeed::skip_to_live`], and the count lands in
//! [`MeetingTranscript::gaps`] because a transcript missing thirty seconds must
//! not look complete.
//!
//! # A window with no speech in it is not sent (KNEG-410)
//!
//! Each source is silent for much of a meeting: the microphone while others
//! speak, the system audio while you do. Every one of those windows was a
//! whisper request for nothing, about 1.3 s of `ggml-base` each on the
//! machine that measured it. So each window is searched for speech first, by
//! the same [`FindSpeech`] the whole-recording pass uses, and one with none
//! is skipped. The search took 51 ms (median of 33 windows; slowest 163 ms)
//! against a three-second hop. It counts against the hop like the
//! transcription does, and a skipped window costs only the search, so a quiet
//! stretch lets a slow model catch up rather than drop audio. On 102 s with
//! 21 silent windows, the live pass took 17.9 s of wall clock instead of
//! 44.3 s, with the same events (`tests/live_windows.rs`, 2026-10-07).
//!
//! This was filed to stop invented text: 40 s of noise sent to whisper whole
//! comes back as some, different each time. Five-second windows did not.
//! The same 40 s cut into windows, and 47 other silent windows under white
//! and pink noise, all came back empty from `ggml-base` on 2026-10-07, with
//! the application's decoder thresholds. The gain measured is the time.
//! Whether another model or a real room invents text on a silent window was
//! not tried; a window the finder hears no speech in is no longer sent to find
//! out.
//!
//! A window with any speech in it goes to whisper whole, as before, not cut
//! down to its speech: the event is placed by the window's start either way.
//! A word clipped at an edge is safe because the windows overlap. At five
//! seconds every three, the last two seconds of one window are the first two
//! of the next, so speech up to two seconds long lies whole inside some
//! window, and longer speech has at least two seconds inside one. A search
//! that fails sends the window, as before: a worse live view, never an empty
//! one.

use std::sync::Arc;
use std::time::Duration;

use kw_audio_core::meeting::WindowFeed;
use kw_audio_core::wav;
use kw_transcription_core::local::{Inference, LocalTranscriber};
use kw_transcription_core::speech::FindSpeech;
use tokio::sync::mpsc;

use crate::capabilities::{capabilities, MeetingProviderCapabilities};
use crate::{
    AudioChunk, AudioSource, MeetingError, MeetingProviderConfig, MeetingTranscript,
    MeetingTranscriptionProvider, TranscriptEvent,
};

/// The id this provider answers to, and its entry in the capability table.
pub const PROVIDER_ID: &str = "local";

/// How many hops' worth of lost ground is tolerated before audio is dropped.
///
/// Four, so twelve seconds at the standard three-second hop: long enough that one
/// slow window does not throw audio away, short enough that a "live" transcript
/// is still recognisably live. Expressed in hops rather than seconds so it scales
/// with the windowing instead of having to be tuned alongside it.
const MAX_DEFICIT_HOPS: u32 = 4;

/// A tail shorter than this at the end of the session is discarded rather than
/// transcribed. See [`WindowFeed::flush`].
const MINIMUM_TAIL: Duration = Duration::from_millis(500);

/// The window and hop a feed uses.
///
/// A parameter rather than a constant because a test cannot wait three seconds
/// per window, and because measuring a larger model may want a longer window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Windowing {
    pub window: Duration,
    pub hop: Duration,
}

impl Default for Windowing {
    /// Five seconds every three, which is what the local client ships with.
    fn default() -> Self {
        Self {
            window: Duration::from_secs(5),
            hop: Duration::from_secs(3),
        }
    }
}

impl Windowing {
    /// The sustained shortfall that justifies dropping audio.
    #[must_use]
    fn max_deficit(self) -> Duration {
        self.hop.saturating_mul(MAX_DEFICIT_HOPS)
    }
}

/// Repeated short passes over a local `whisper-server`.
pub struct LocalWindowProvider {
    transcriber: Arc<dyn LocalTranscriber>,
    /// What says whether a window has speech in it. `None` sends every window.
    speech: Option<Arc<dyn FindSpeech>>,
    /// Whether the last search failed, so a broken tool is reported once per
    /// run of failures rather than every three seconds.
    search_failing: bool,
    /// One feed per source. The two are transcribed separately and never mixed,
    /// because `whisper` cannot separate speakers — mixing them would make
    /// [`AudioSource`] on every event a guess, and that field exists precisely so
    /// a transcript can be attributed when the provider does not label speakers.
    mic: WindowFeed,
    system: WindowFeed,
    /// Events as they were produced, returned by [`Self::finish`].
    events: Vec<TranscriptEvent>,
    /// Where to deliver each event the moment it exists, for a live view.
    ///
    /// Optional because a test does not need one, and because a dropped receiver
    /// must not end the meeting: the transcript is still being accumulated.
    live: Option<mpsc::UnboundedSender<TranscriptEvent>>,
    language: Option<String>,
    gaps: u32,
    windowing: Windowing,
    /// Wall-clock time spent transcribing beyond the audio it covered.
    ///
    /// Paid down by windows that finish inside their hop, so a single slow window
    /// costs nothing and a slow *model* accumulates.
    deficit: Duration,
    state: State,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Fresh,
    Running,
    Paused,
    Finished,
}

impl std::fmt::Debug for LocalWindowProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LocalWindowProvider")
            .field("state", &self.state)
            .field("events", &self.events.len())
            .field("gaps", &self.gaps)
            .field("searches_for_speech", &self.searches_for_speech())
            .finish_non_exhaustive()
    }
}

impl LocalWindowProvider {
    /// Build a provider over `transcriber`.
    ///
    /// Taking the trait rather than a `WhisperServer` is what makes the whole
    /// window-to-transcript path testable without a server running. It is also
    /// how a future sherpa-onnx runtime slots in.
    #[must_use]
    pub fn new(transcriber: Arc<dyn LocalTranscriber>) -> Self {
        Self {
            transcriber,
            speech: None,
            search_failing: false,
            mic: WindowFeed::standard(),
            system: WindowFeed::standard(),
            events: Vec::new(),
            live: None,
            language: None,
            gaps: 0,
            windowing: Windowing::default(),
            deficit: Duration::ZERO,
            state: State::Fresh,
        }
    }

    /// Use a different window and hop.
    ///
    /// Must be called before [`MeetingTranscriptionProvider::connect`]; the feeds
    /// are rebuilt here.
    #[must_use]
    pub fn with_windowing(mut self, windowing: Windowing) -> Self {
        self.mic = WindowFeed::new(windowing.window, windowing.hop);
        self.system = WindowFeed::new(windowing.window, windowing.hop);
        self.windowing = windowing;
        self
    }

    /// Deliver each event as it is produced, as well as accumulating it.
    #[must_use]
    pub fn with_live_events(mut self, sink: mpsc::UnboundedSender<TranscriptEvent>) -> Self {
        self.live = Some(sink);
        self
    }

    /// Search each window for speech, and send only those that have some.
    /// See the module doc.
    #[must_use]
    pub fn with_speech_finder(mut self, finder: Arc<dyn FindSpeech>) -> Self {
        self.speech = Some(finder);
        self
    }

    /// Whether windows are searched for speech before they are sent.
    ///
    /// Read-only, for a caller's test to pin what it built, as
    /// `WhisperServer::finds_speech_first` is.
    #[must_use]
    pub const fn searches_for_speech(&self) -> bool {
        self.speech.is_some()
    }

    /// How many stretches of audio were dropped to keep up.
    #[must_use]
    pub const fn gaps(&self) -> u32 {
        self.gaps
    }

    fn feed(&mut self, source: AudioSource) -> &mut WindowFeed {
        match source {
            AudioSource::Mic => &mut self.mic,
            AudioSource::System => &mut self.system,
        }
    }

    /// Transcribe every window `source` has ready, dropping audio only if the
    /// transcriber is losing ground.
    async fn drain_windows(&mut self, source: AudioSource) -> Result<(), MeetingError> {
        let hop = self.windowing.hop;

        while let Some(window) = self.feed(source).next_window() {
            let started = std::time::Instant::now();
            self.transcribe(&window.samples, window.start_seconds, source, false)
                .await?;
            let spent = started.elapsed();

            // One window advances the session by one hop. Under budget pays the
            // deficit down; over budget adds to it. A burst of audio is therefore
            // not a problem, which is the distinction the first version of this
            // missed.
            if spent > hop {
                self.deficit = self.deficit.saturating_add(spent - hop);
            } else {
                self.deficit = self.deficit.saturating_sub(hop - spent);
            }

            if self.deficit > self.windowing.max_deficit() {
                let skipped = self.feed(source).skip_to_live();
                #[allow(clippy::cast_possible_truncation)]
                let skipped = skipped as u32;
                self.gaps = self.gaps.saturating_add(skipped);
                self.deficit = Duration::ZERO;
                tracing::warn!(
                    ?source,
                    skipped,
                    "local transcription cannot keep up with the meeting; audio was dropped"
                );
                break;
            }
        }

        Ok(())
    }

    /// Whether `wav` goes to whisper: only a search that found nothing says no.
    async fn has_speech(&mut self, wav: &[u8]) -> bool {
        let Some(finder) = self.speech.clone() else {
            return true;
        };
        match finder.find(wav).await {
            Ok(spans) => {
                self.search_failing = false;
                !spans.is_empty()
            }
            Err(detail) => {
                if self.search_failing {
                    tracing::debug!(%detail, "still could not look for speech in a window");
                } else {
                    tracing::warn!(%detail, "could not look for speech in a window; sending every window until it can");
                }
                self.search_failing = true;
                true
            }
        }
    }

    /// Transcribe one window and record what came back.
    async fn transcribe(
        &mut self,
        samples: &[f32],
        start_seconds: f64,
        source: AudioSource,
        is_final: bool,
    ) -> Result<(), MeetingError> {
        let wav = wav::encode_wav(samples, wav::TARGET_SAMPLE_RATE);

        // No speech, no request (KNEG-410).
        if !self.has_speech(&wav).await {
            return Ok(());
        }

        let inference = self
            .transcriber
            .transcribe(wav, self.language.as_deref(), None)
            .await
            .map_err(|detail| MeetingError::Disconnected {
                provider: PROVIDER_ID.to_owned(),
                detail,
            })?;

        match inference {
            // Timings are not asked for here, but the text is the same text
            // either way -- one window's transcript, placed in the meeting by
            // `start_seconds` rather than by anything the server said. An
            // or-pattern rather than a second arm, because a copy of this body
            // would be a second place for the live-sink rule to drift.
            Inference::Text(text) | Inference::Timed { text, .. } if !text.trim().is_empty() => {
                let event = TranscriptEvent {
                    text,
                    is_final,
                    timestamp: start_seconds,
                    speaker: None,
                    source,
                };
                if let Some(sink) = &self.live {
                    // A closed receiver is not a reason to stop: the window was
                    // transcribed and the transcript is still being kept.
                    if sink.send(event.clone()).is_err() {
                        tracing::debug!("nothing is listening for live transcript events");
                    }
                }
                self.events.push(event);
            }
            // A window of silence between sentences, which is most windows in a
            // meeting. Not an event, and not an error.
            Inference::Text(_) | Inference::Timed { .. } | Inference::Empty => {}
            Inference::Rejected { status, body } => {
                return Err(MeetingError::Rejected {
                    provider: PROVIDER_ID.to_owned(),
                    detail: format!("the local server answered {status}: {body}"),
                });
            }
        }

        Ok(())
    }

    /// The length of the session, from the furthest either source reached.
    fn duration_seconds(&self) -> f64 {
        let mic = self.mic.lag().as_secs_f64() + self.mic.consumed_seconds();
        let system = self.system.lag().as_secs_f64() + self.system.consumed_seconds();
        mic.max(system)
    }
}

#[async_trait::async_trait]
impl MeetingTranscriptionProvider for LocalWindowProvider {
    async fn connect(&mut self, config: MeetingProviderConfig) -> Result<(), MeetingError> {
        if config.provider != PROVIDER_ID {
            return Err(MeetingError::UnknownProvider {
                id: config.provider,
            });
        }

        // Checked against the declared rate rather than resampled here: the
        // caller converts, and a mismatch means the caller and this provider
        // disagree about what they are exchanging.
        let caps = self.capabilities();
        if !caps.sample_rates.contains(&config.sample_rate) {
            return Err(MeetingError::SampleRate {
                provider: PROVIDER_ID.to_owned(),
                offered: config.sample_rate,
                accepted: caps.sample_rates,
            });
        }

        // No connection to open. Saying so plainly is the point: there is no
        // handshake to fake and nothing to be refused by.
        self.language = config.language;
        self.state = State::Running;
        Ok(())
    }

    async fn send_audio(&mut self, chunk: AudioChunk) -> Result<(), MeetingError> {
        match self.state {
            State::Fresh | State::Finished => {
                return Err(MeetingError::Disconnected {
                    provider: PROVIDER_ID.to_owned(),
                    detail: "the session is not open".to_owned(),
                })
            }
            // Discarded, which is what pausing a recording means. Buffering it
            // would make "pause" mean "delay", and the meeting would catch up in
            // a rush on resume.
            State::Paused => return Ok(()),
            State::Running => {}
        }

        let samples = wav::from_pcm(&chunk.pcm);
        self.feed(chunk.source).push(&samples);
        self.drain_windows(chunk.source).await
    }

    async fn pause(&mut self) -> Result<(), MeetingError> {
        if self.state == State::Running {
            self.state = State::Paused;
        }
        Ok(())
    }

    async fn resume(&mut self) -> Result<(), MeetingError> {
        if self.state == State::Paused {
            self.state = State::Running;
        }
        Ok(())
    }

    async fn finish(&mut self) -> Result<MeetingTranscript, MeetingError> {
        // The tail of each source, marked final because nothing will revise it.
        for source in [AudioSource::Mic, AudioSource::System] {
            if let Some(window) = self.feed(source).flush(MINIMUM_TAIL) {
                self.transcribe(&window.samples, window.start_seconds, source, true)
                    .await?;
            }
        }

        self.state = State::Finished;

        Ok(MeetingTranscript {
            events: std::mem::take(&mut self.events),
            duration_seconds: self.duration_seconds(),
            // Nothing to reconnect to.
            reconnects: 0,
            gaps: self.gaps,
        })
    }

    async fn cancel(&mut self) -> Result<(), MeetingError> {
        self.events.clear();
        self.state = State::Finished;
        Ok(())
    }

    fn capabilities(&self) -> &'static MeetingProviderCapabilities {
        // Unwrapping is not an option under this workspace's lints, and a
        // fallback would be a lie. The table is a compile-time constant and the
        // test below asserts the entry exists, which is where this is enforced.
        capabilities(PROVIDER_ID).unwrap_or(&FALLBACK)
    }
}

/// Never reached: `local` is in [`crate::PROVIDERS`] and a test says so. Present
/// only because `capabilities()` returns an `Option` and this workspace forbids
/// unwrapping in library code.
static FALLBACK: MeetingProviderCapabilities = MeetingProviderCapabilities {
    id: PROVIDER_ID,
    name: "Local windowed model",
    audio_leaves_device: false,
    keyterms: crate::KeytermSupport::Unsupported,
    session_limit: None,
    token_rotation: false,
    sample_rates: &[16_000],
    speaker_labels: false,
    partials: true,
    // Mirrors the real entry in `PROVIDERS` -- see the comment there.
    language_detection: true,
    reconnect: true,
    batch_fallback: true,
};

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use std::sync::Mutex;

    use kw_transcription_core::speech::Span;

    use super::*;

    const RATE: usize = 16_000;

    /// A transcriber that answers from a script, and records what it was asked.
    #[derive(Debug, Default)]
    struct Scripted {
        replies: Mutex<Vec<Inference>>,
        /// Sample count of each WAV it was handed, so a test can check the
        /// window arithmetic reached it intact.
        seen: Mutex<Vec<usize>>,
        /// How long each call takes, for reproducing a model that cannot keep up.
        delay: Duration,
    }

    impl Scripted {
        fn saying(replies: Vec<Inference>) -> Arc<Self> {
            Arc::new(Self {
                replies: Mutex::new(replies),
                seen: Mutex::new(Vec::new()),
                delay: Duration::ZERO,
            })
        }

        /// A transcriber that always answers, and always too slowly.
        fn taking(delay: Duration) -> Arc<Self> {
            Arc::new(Self {
                replies: Mutex::new(Vec::new()),
                seen: Mutex::new(Vec::new()),
                delay,
            })
        }

        fn calls(&self) -> usize {
            self.seen.lock().unwrap().len()
        }
    }

    #[async_trait::async_trait]
    impl LocalTranscriber for Scripted {
        async fn transcribe(
            &self,
            wav: Vec<u8>,
            _language: Option<&str>,
            _prompt: Option<&str>,
        ) -> Result<Inference, String> {
            if !self.delay.is_zero() {
                tokio::time::sleep(self.delay).await;
            }
            // 44-byte header, two bytes a sample.
            self.seen.lock().unwrap().push((wav.len() - 44) / 2);

            let mut replies = self.replies.lock().unwrap();
            Ok(if replies.is_empty() {
                Inference::Empty
            } else {
                replies.remove(0)
            })
        }

        fn model_name(&self) -> Option<String> {
            Some("scripted".to_owned())
        }
    }

    /// A speech finder that answers from a script, and counts what it was asked.
    /// Answers "no speech" once the script runs out.
    #[derive(Debug, Default)]
    struct Finder {
        answers: Mutex<Vec<Result<Vec<Span>, String>>>,
        asked: Mutex<usize>,
        /// How long each search takes, for pinning that it counts against the hop.
        delay: Duration,
    }

    impl Finder {
        fn answering(answers: Vec<Result<Vec<Span>, String>>) -> Arc<Self> {
            Arc::new(Self {
                answers: Mutex::new(answers),
                ..Self::default()
            })
        }

        fn hearing_nothing() -> Arc<Self> {
            Self::answering(Vec::new())
        }

        fn taking(delay: Duration) -> Arc<Self> {
            Arc::new(Self {
                delay,
                ..Self::default()
            })
        }

        fn asked(&self) -> usize {
            *self.asked.lock().unwrap()
        }
    }

    #[async_trait::async_trait]
    impl FindSpeech for Finder {
        async fn find(&self, _wav: &[u8]) -> Result<Vec<Span>, String> {
            if !self.delay.is_zero() {
                tokio::time::sleep(self.delay).await;
            }
            *self.asked.lock().unwrap() += 1;
            let mut answers = self.answers.lock().unwrap();
            if answers.is_empty() {
                Ok(Vec::new())
            } else {
                answers.remove(0)
            }
        }
    }

    fn config() -> MeetingProviderConfig {
        MeetingProviderConfig {
            provider: PROVIDER_ID.to_owned(),
            model: None,
            language: Some("en".to_owned()),
            keyterms: Vec::new(),
            key_ref: None,
            sample_rate: 16_000,
        }
    }

    /// `seconds` of silence as 16-bit PCM, which is what a capture hands over.
    fn pcm(seconds: usize, source: AudioSource) -> AudioChunk {
        AudioChunk {
            pcm: vec![0_u8; RATE * seconds * 2],
            source,
        }
    }

    #[tokio::test]
    async fn a_window_is_transcribed_once_five_seconds_have_arrived() {
        let scripted = Scripted::saying(vec![Inference::Text("hello there".to_owned())]);
        let mut provider = LocalWindowProvider::new(Arc::clone(&scripted) as Arc<_>);
        provider.connect(config()).await.unwrap();

        provider.send_audio(pcm(4, AudioSource::Mic)).await.unwrap();
        assert_eq!(scripted.calls(), 0, "four seconds is not a window");

        provider.send_audio(pcm(1, AudioSource::Mic)).await.unwrap();
        assert_eq!(scripted.calls(), 1);
        assert_eq!(scripted.seen.lock().unwrap()[0], RATE * 5);

        let transcript = provider.finish().await.unwrap();
        assert_eq!(transcript.events.len(), 1);
        assert_eq!(transcript.events[0].text, "hello there");
        assert_eq!(transcript.events[0].source, AudioSource::Mic);
    }

    #[tokio::test]
    async fn every_event_is_a_partial_because_the_windows_overlap() {
        // The overlap repeats words, and deduplicating that needs word-level
        // timestamps `/inference` is not asked for. So these are partials, a
        // live view replaces rather than appends, and the saved transcript comes
        // from one pass over the finished recording.
        let scripted = Scripted::saying(vec![
            Inference::Text("one".to_owned()),
            Inference::Text("one two".to_owned()),
        ]);
        let mut provider = LocalWindowProvider::new(Arc::clone(&scripted) as Arc<_>);
        provider.connect(config()).await.unwrap();
        provider.send_audio(pcm(8, AudioSource::Mic)).await.unwrap();

        let transcript = provider.finish().await.unwrap();
        let windows: Vec<&TranscriptEvent> =
            transcript.events.iter().filter(|e| !e.is_final).collect();
        assert_eq!(windows.len(), 2);
        assert_eq!(windows[0].timestamp, 0.0);
        assert_eq!(windows[1].timestamp, 3.0);
    }

    #[tokio::test]
    async fn the_two_sources_are_transcribed_separately_and_stay_attributed() {
        // Mixing them would make `source` on every event a guess, and that field
        // exists so a transcript can be attributed when the provider does not
        // label speakers.
        let scripted = Scripted::saying(vec![
            Inference::Text("i said this".to_owned()),
            Inference::Text("they said that".to_owned()),
        ]);
        let mut provider = LocalWindowProvider::new(Arc::clone(&scripted) as Arc<_>);
        provider.connect(config()).await.unwrap();

        provider.send_audio(pcm(5, AudioSource::Mic)).await.unwrap();
        provider
            .send_audio(pcm(5, AudioSource::System))
            .await
            .unwrap();

        let transcript = provider.finish().await.unwrap();
        assert_eq!(transcript.events.len(), 2);
        assert_eq!(transcript.events[0].source, AudioSource::Mic);
        assert_eq!(transcript.events[1].source, AudioSource::System);
    }

    #[tokio::test]
    async fn a_meeting_with_no_system_audio_still_transcribes_the_microphone() {
        // WASAPI loopback from an idle output device delivers no callback at all,
        // measured on 2026-08-26. Nobody else speaking is not a failure.
        let scripted = Scripted::saying(vec![Inference::Text("just me".to_owned())]);
        let mut provider = LocalWindowProvider::new(Arc::clone(&scripted) as Arc<_>);
        provider.connect(config()).await.unwrap();
        provider.send_audio(pcm(5, AudioSource::Mic)).await.unwrap();

        let transcript = provider.finish().await.unwrap();
        assert_eq!(transcript.events.len(), 1);
        assert_eq!(transcript.gaps, 0);
    }

    #[tokio::test]
    async fn silence_produces_no_event_rather_than_an_empty_one() {
        // Most windows in a meeting are the gap between sentences.
        let scripted = Scripted::saying(vec![Inference::Empty, Inference::Text("  ".to_owned())]);
        let mut provider = LocalWindowProvider::new(Arc::clone(&scripted) as Arc<_>);
        provider.connect(config()).await.unwrap();
        provider.send_audio(pcm(8, AudioSource::Mic)).await.unwrap();

        assert_eq!(scripted.calls(), 2);
        assert!(provider.finish().await.unwrap().events.is_empty());
    }

    #[tokio::test]
    async fn a_window_with_no_speech_in_it_is_never_sent_to_whisper() {
        // KNEG-410. Each of these was a whisper request for nothing, about
        // 1.3 s of `ggml-base` a window.
        let scripted = Scripted::saying(vec![
            Inference::Text("invented".to_owned()),
            Inference::Text("invented".to_owned()),
        ]);
        let finder = Finder::hearing_nothing();
        let mut provider = LocalWindowProvider::new(Arc::clone(&scripted) as Arc<_>)
            .with_speech_finder(Arc::clone(&finder) as Arc<_>);
        provider.connect(config()).await.unwrap();
        provider.send_audio(pcm(8, AudioSource::Mic)).await.unwrap();

        assert_eq!(finder.asked(), 2, "both windows were searched");
        assert_eq!(scripted.calls(), 0, "and neither was sent");
        assert!(provider.finish().await.unwrap().events.is_empty());
    }

    #[tokio::test]
    async fn a_window_with_any_speech_in_it_is_sent_whole() {
        // Not trimmed to its speech: the event is placed by the window's start,
        // and a clipped word at an edge is better heard than cut.
        let scripted = Scripted::saying(vec![Inference::Text("at the edge".to_owned())]);
        let finder = Finder::answering(vec![Ok(vec![Span::new(4.8, 5.0)])]);
        let mut provider = LocalWindowProvider::new(Arc::clone(&scripted) as Arc<_>)
            .with_speech_finder(Arc::clone(&finder) as Arc<_>);
        provider.connect(config()).await.unwrap();
        provider.send_audio(pcm(5, AudioSource::Mic)).await.unwrap();

        assert_eq!(scripted.calls(), 1);
        assert_eq!(scripted.seen.lock().unwrap()[0], RATE * 5);
        let transcript = provider.finish().await.unwrap();
        assert_eq!(transcript.events.len(), 1);
        assert_eq!(transcript.events[0].text, "at the edge");
    }

    #[tokio::test]
    async fn a_window_that_could_not_be_searched_is_sent_as_before() {
        // A failed search is not "no speech". Skipping on it would blank the
        // live view for a whole meeting because a tool crashed.
        let scripted = Scripted::saying(vec![
            Inference::Text("one".to_owned()),
            Inference::Text("two".to_owned()),
        ]);
        let finder = Finder::answering(vec![
            Err("the speech finder exited with 3".to_owned()),
            Err("the speech finder exited with 3".to_owned()),
        ]);
        let mut provider = LocalWindowProvider::new(Arc::clone(&scripted) as Arc<_>)
            .with_speech_finder(Arc::clone(&finder) as Arc<_>);
        provider.connect(config()).await.unwrap();
        provider.send_audio(pcm(8, AudioSource::Mic)).await.unwrap();

        assert_eq!(scripted.calls(), 2);
        assert_eq!(provider.finish().await.unwrap().events.len(), 2);
    }

    #[tokio::test]
    async fn the_tail_is_searched_too() {
        // The tail is a final event, the one a live view commits as a row.
        let scripted = Scripted::saying(vec![Inference::Text("invented".to_owned())]);
        let finder = Finder::hearing_nothing();
        let mut provider = LocalWindowProvider::new(Arc::clone(&scripted) as Arc<_>)
            .with_speech_finder(Arc::clone(&finder) as Arc<_>);
        provider.connect(config()).await.unwrap();
        provider.send_audio(pcm(2, AudioSource::Mic)).await.unwrap();

        let transcript = provider.finish().await.unwrap();
        assert_eq!(finder.asked(), 1);
        assert_eq!(scripted.calls(), 0);
        assert!(transcript.events.is_empty());
    }

    #[test]
    fn it_says_whether_it_searches_for_speech() {
        // For the caller's test. Without a finder -- no tool or no model on
        // this computer -- every window is sent, as before KNEG-410, which the
        // tests above without one already pin.
        let provider = LocalWindowProvider::new(Arc::new(Scripted::default()) as Arc<_>);
        assert!(!provider.searches_for_speech());
        let provider = provider.with_speech_finder(Finder::hearing_nothing() as Arc<_>);
        assert!(provider.searches_for_speech());
    }

    #[tokio::test]
    async fn silence_lets_a_slow_model_catch_up() {
        // The windowing of the test below, where a 200 ms transcriber falls
        // behind. Windows with no speech cost only the search, so a quiet
        // stretch pays the deficit down instead of adding to it.
        let windowing = Windowing {
            window: Duration::from_millis(100),
            hop: Duration::from_millis(60),
        };
        let scripted = Scripted::taking(Duration::from_millis(200));
        let mut provider = LocalWindowProvider::new(Arc::clone(&scripted) as Arc<_>)
            .with_windowing(windowing)
            .with_speech_finder(Finder::hearing_nothing() as Arc<_>);
        provider.connect(config()).await.unwrap();
        provider.send_audio(pcm(2, AudioSource::Mic)).await.unwrap();

        assert_eq!(scripted.calls(), 0);
        assert_eq!(provider.finish().await.unwrap().gaps, 0);
    }

    #[tokio::test]
    async fn the_search_counts_against_the_hop() {
        // What keeps the live view live is the time a window costs, and the
        // search is part of it. A finder slower than the meeting falls behind
        // like a slow model does, and says so.
        let windowing = Windowing {
            window: Duration::from_millis(100),
            hop: Duration::from_millis(60),
        };
        let mut provider = LocalWindowProvider::new(Arc::new(Scripted::default()) as Arc<_>)
            .with_windowing(windowing)
            .with_speech_finder(Finder::taking(Duration::from_millis(200)) as Arc<_>);
        provider.connect(config()).await.unwrap();
        provider.send_audio(pcm(2, AudioSource::Mic)).await.unwrap();

        assert!(provider.finish().await.unwrap().gaps > 0);
    }

    #[tokio::test]
    async fn a_burst_of_audio_is_not_falling_behind() {
        // This test found the bug. The first version compared the amount of
        // audio waiting against a threshold, so twenty windows arriving in one
        // push looked exactly like a transcriber that could not keep up -- and it
        // threw away fifty-five of the sixty seconds it could have transcribed.
        // A burst processed faster than it arrived is not a problem.
        let scripted = Scripted::default();
        let mut provider = LocalWindowProvider::new(Arc::new(scripted) as Arc<_>);
        provider.connect(config()).await.unwrap();

        provider
            .send_audio(pcm(60, AudioSource::Mic))
            .await
            .unwrap();

        let transcript = provider.finish().await.unwrap();
        assert_eq!(
            transcript.gaps, 0,
            "an instant transcriber never falls behind, however much arrives at once"
        );
    }

    #[tokio::test]
    async fn a_transcriber_slower_than_the_meeting_drops_audio_and_counts_it() {
        // The real signal: wall-clock time against audio time. Windowed at 100 ms
        // with a 60 ms hop against a transcriber taking 200 ms, so each window
        // loses 140 ms of ground and four hops' worth -- 240 ms -- is the budget.
        // A transcript missing audio must say how much.
        let windowing = Windowing {
            window: Duration::from_millis(100),
            hop: Duration::from_millis(60),
        };
        let scripted = Scripted::taking(Duration::from_millis(200));
        let mut provider =
            LocalWindowProvider::new(Arc::clone(&scripted) as Arc<_>).with_windowing(windowing);
        provider.connect(config()).await.unwrap();

        // Two seconds of audio is far more than it can process in real time.
        provider.send_audio(pcm(2, AudioSource::Mic)).await.unwrap();

        let transcript = provider.finish().await.unwrap();
        assert!(
            transcript.gaps > 0,
            "it fell behind and said nothing about it"
        );
        assert!(
            scripted.calls() < 33,
            "it should have stopped rather than transcribing all {} windows",
            scripted.calls()
        );
    }

    #[tokio::test]
    async fn one_slow_window_costs_nothing() {
        // The deficit is paid back down by windows that finish inside their hop,
        // so a single slow call -- a model loading, a scheduler hiccup -- does not
        // throw audio away.
        let windowing = Windowing {
            window: Duration::from_millis(100),
            hop: Duration::from_millis(60),
        };
        let scripted = Scripted::taking(Duration::from_millis(150));
        let mut provider =
            LocalWindowProvider::new(Arc::clone(&scripted) as Arc<_>).with_windowing(windowing);
        provider.connect(config()).await.unwrap();

        // One window's worth, once: 90 ms over budget, well inside the 240 ms
        // allowance.
        provider
            .send_audio(AudioChunk {
                pcm: vec![0_u8; RATE / 10 * 2],
                source: AudioSource::Mic,
            })
            .await
            .unwrap();

        assert_eq!(scripted.calls(), 1);
        assert_eq!(provider.finish().await.unwrap().gaps, 0);
    }

    #[tokio::test]
    async fn audio_sent_while_paused_is_discarded_rather_than_delayed() {
        // Buffering it would make "pause" mean "delay", and the meeting would
        // catch up in a rush on resume.
        let scripted = Scripted::saying(vec![Inference::Text("after".to_owned())]);
        let mut provider = LocalWindowProvider::new(Arc::clone(&scripted) as Arc<_>);
        provider.connect(config()).await.unwrap();

        provider.pause().await.unwrap();
        provider
            .send_audio(pcm(10, AudioSource::Mic))
            .await
            .unwrap();
        assert_eq!(scripted.calls(), 0);

        provider.resume().await.unwrap();
        provider.send_audio(pcm(5, AudioSource::Mic)).await.unwrap();
        assert_eq!(scripted.calls(), 1);

        let transcript = provider.finish().await.unwrap();
        assert_eq!(transcript.events[0].timestamp, 0.0, "the clock did not run");
    }

    #[tokio::test]
    async fn audio_before_connect_is_refused_rather_than_buffered() {
        let mut provider = LocalWindowProvider::new(Arc::new(Scripted::default()) as Arc<_>);
        let err = provider
            .send_audio(pcm(5, AudioSource::Mic))
            .await
            .unwrap_err();
        assert!(matches!(err, MeetingError::Disconnected { .. }), "{err:?}");
    }

    #[tokio::test]
    async fn a_server_refusal_is_reported_rather_than_read_as_silence() {
        let scripted = Scripted::saying(vec![Inference::Rejected {
            status: 503,
            body: "model not loaded".to_owned(),
        }]);
        let mut provider = LocalWindowProvider::new(Arc::clone(&scripted) as Arc<_>);
        provider.connect(config()).await.unwrap();

        let err = provider
            .send_audio(pcm(5, AudioSource::Mic))
            .await
            .unwrap_err();
        match err {
            MeetingError::Rejected { detail, .. } => {
                assert!(detail.contains("503"), "{detail}");
                assert!(detail.contains("model not loaded"), "{detail}");
            }
            other => panic!("{other:?}"),
        }
    }

    #[tokio::test]
    async fn the_wrong_sample_rate_is_refused_with_the_one_it_takes() {
        let mut provider = LocalWindowProvider::new(Arc::new(Scripted::default()) as Arc<_>);
        let mut cfg = config();
        cfg.sample_rate = 48_000;

        match provider.connect(cfg).await.unwrap_err() {
            MeetingError::SampleRate {
                offered, accepted, ..
            } => {
                assert_eq!(offered, 48_000);
                assert_eq!(accepted, &[16_000]);
            }
            other => panic!("{other:?}"),
        }
    }

    #[tokio::test]
    async fn another_providers_configuration_is_refused() {
        // Fail closed. A configuration for Deepgram must not be transcribed
        // locally just because this is the provider that happened to be built.
        let mut provider = LocalWindowProvider::new(Arc::new(Scripted::default()) as Arc<_>);
        let mut cfg = config();
        cfg.provider = "deepgram-realtime".to_owned();

        match provider.connect(cfg).await.unwrap_err() {
            MeetingError::UnknownProvider { id } => assert_eq!(id, "deepgram-realtime"),
            other => panic!("{other:?}"),
        }
    }

    #[tokio::test]
    async fn cancelling_discards_the_transcript() {
        let scripted = Scripted::saying(vec![Inference::Text("forget this".to_owned())]);
        let mut provider = LocalWindowProvider::new(Arc::clone(&scripted) as Arc<_>);
        provider.connect(config()).await.unwrap();
        provider.send_audio(pcm(5, AudioSource::Mic)).await.unwrap();

        provider.cancel().await.unwrap();
        assert!(provider.events.is_empty());
    }

    #[tokio::test]
    async fn live_events_arrive_before_the_meeting_ends() {
        // The whole reason for windowing. If these only appeared at `finish`,
        // batch transcription of the recording would be simpler and better.
        let (tx, mut rx) = mpsc::unbounded_channel();
        let scripted = Scripted::saying(vec![Inference::Text("live".to_owned())]);
        let mut provider =
            LocalWindowProvider::new(Arc::clone(&scripted) as Arc<_>).with_live_events(tx);
        provider.connect(config()).await.unwrap();
        provider.send_audio(pcm(5, AudioSource::Mic)).await.unwrap();

        let event = rx.try_recv().unwrap();
        assert_eq!(event.text, "live");
        assert!(!event.is_final);
    }

    #[tokio::test]
    async fn a_closed_live_receiver_does_not_end_the_meeting() {
        let (tx, rx) = mpsc::unbounded_channel();
        drop(rx);

        let scripted = Scripted::saying(vec![Inference::Text("still kept".to_owned())]);
        let mut provider =
            LocalWindowProvider::new(Arc::clone(&scripted) as Arc<_>).with_live_events(tx);
        provider.connect(config()).await.unwrap();
        provider.send_audio(pcm(5, AudioSource::Mic)).await.unwrap();

        let transcript = provider.finish().await.unwrap();
        assert_eq!(transcript.events.len(), 1, "the transcript is still kept");
    }

    #[test]
    fn the_capability_entry_this_provider_names_exists() {
        // What makes the fallback in `capabilities()` unreachable rather than a
        // silent second source of truth.
        assert!(capabilities(PROVIDER_ID).is_some());
        assert_eq!(crate::client(PROVIDER_ID), Some(crate::Client::LocalWindow));
    }

    #[test]
    fn the_declared_capabilities_describe_windowing_and_not_streaming() {
        let caps = capabilities(PROVIDER_ID).unwrap();
        assert!(!caps.audio_leaves_device, "that is the reason to choose it");
        assert!(caps.partials, "a window is a partial");
        assert_eq!(caps.sample_rates, &[16_000]);
        assert!(!caps.keyterms.is_supported(), "whisper takes no keyterms");
        // `transcribe` above sends "auto" whenever `self.language` is `None`,
        // via `WhisperServer::transcribe`. Declaring this `false` would make
        // `validate` refuse to start the one provider that needs no
        // credentials and no network just because nobody typed a language.
        assert!(
            caps.language_detection,
            "whisper is asked for \"auto\" when no language is given"
        );
    }
}
