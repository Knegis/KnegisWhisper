//! The socket half of the Deepgram Realtime client.
//!
//! [`crate::deepgram`] is the part with rules in it — the endpoint, the frame
//! grammar, what an incoming message means — and is covered by fixtures. This is
//! the plumbing, and it is thin on purpose because almost no test here can reach
//! it. The exceptions arrived with [`crate::fault`]: a fault can be recorded into
//! the session's own cell exactly as the reader task would, with no socket, no
//! key and no network, and `send_audio` and `finish` can then be asked what they
//! do about it.
//!
//! # Two sockets, one per capture
//!
//! Exactly [`crate::assemblyai_socket`]'s shape, for the same reason: the
//! microphone and the system output go to **separate sessions**, so
//! [`AudioSource`] on every transcript event stays a fact this client knows
//! rather than a guess. Deepgram's diarization — when the wire carries a label;
//! see [`crate::deepgram`]'s module documentation — says who *within* a stream,
//! never *which* stream, so it adds information on top of `source` rather than
//! replacing the need for two connections.
//!
//! # No reconnect clock, and no rotation clock either
//!
//! `deepgram-realtime` declares `session_limit: None`, so there is nothing here
//! like [`crate::openai::SessionClock`] — a session runs until the meeting ends or
//! a socket fails on its own.
//!
//! It also declares `token_rotation: true`, and this struct still holds no clock
//! for it. That capability describes upstream minting short-lived temporary tokens
//! from its own hosted service and refreshing them mid-session; this application
//! mints nothing, so [`crate::deepgram::token`] resolves the user's own stored key
//! once, at the moment each socket opens, and nothing here schedules a re-read.
//! [`DeepgramRealtime`]'s fields below are exactly the two capture connections and
//! the bookkeeping every other client here also carries — there is deliberately no
//! third field for a rotation deadline, and the test at the bottom of this file
//! checks that this struct's own description of itself does not mention one.

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Instant;

use futures::{SinkExt as _, StreamExt as _};
use kw_security::CredentialStore;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::client::IntoClientRequest as _;
use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;
use tokio_tungstenite::tungstenite::Message;

use crate::capabilities::MeetingProviderCapabilities;
use crate::deepgram::{self, Incoming, AUTH_HEADER, AUTH_SCHEME, PROVIDER_ID, PURPOSE};
use crate::fault::{close_reason, SessionFault};
use crate::{
    AudioChunk, AudioSource, MeetingError, MeetingProviderConfig, MeetingTranscript,
    MeetingTranscriptionProvider, TranscriptEvent,
};

type Socket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;
type Sink = futures::stream::SplitSink<Socket, Message>;

// Provider responses are outside the application's trust boundary. Keep both
// individual allocations and session-long retention predictably bounded.
const MAX_MESSAGE_BYTES: usize = 1024 * 1024;
const MAX_TRANSCRIPT_BYTES: usize = 16 * 1024 * 1024;
const MAX_TRANSCRIPT_EVENTS: usize = 10_000;

/// What a [`TranscriptBudget::reserve`] call decided.
///
/// A two-variant answer rather than a `bool`, because the caller has to do
/// something different for the *first* refusal of a run than for the rest of
/// it: one log line per stretch of dropped transcript, not one per frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Reserved {
    /// There was room. Keep the text.
    Room,
    /// There was not. Drop the text and carry on reading.
    NoRoom {
        /// Whether this refusal began a new stretch for this capture.
        opens_a_gap: bool,
    },
}

/// How much transcript one session may retain, and what it cost to stop.
///
/// The ceilings are deliberate: provider responses are outside the trust
/// boundary, so session-long retention has to be bounded by something other
/// than the provider's good behaviour. Shared by both captures through an
/// `Arc`, because the bound is on the session and not on a socket.
///
/// **What the counters below are for.** Refusing is not free — the audio in
/// that stretch is never transcribed — and until KNEG-109 the cost was paid
/// invisibly: the reader task returned, the meeting kept recording, and
/// `deepgram::transcript` reported `gaps: 0`. `MeetingTranscript::gaps` exists
/// precisely so that "a transcript with a hole in it must not look complete",
/// and this is the thing that fills it in.
#[derive(Default)]
struct TranscriptBudget {
    bytes: AtomicUsize,
    events: AtomicUsize,
    /// Whether the last thing each capture offered was refused.
    ///
    /// One flag per capture rather than one for the session. A gap is a
    /// *stretch* of untranscribed audio and it belongs to the capture it
    /// happened in: the microphone can be dropping while the system audio is
    /// still being admitted, and a single shared flag would end the
    /// microphone's stretch on the strength of somebody else's frame.
    mic_dropping: AtomicBool,
    system_dropping: AtomicBool,
    /// How many stretches have begun, across both captures.
    gaps: AtomicU32,
}

impl TranscriptBudget {
    /// Which capture's dropping flag this is.
    const fn dropping(&self, source: AudioSource) -> &AtomicBool {
        match source {
            AudioSource::Mic => &self.mic_dropping,
            AudioSource::System => &self.system_dropping,
        }
    }

    /// Take `bytes` for one transcript event from `source`.
    ///
    /// Note that a refusal is not final. The byte ceiling is a total, so a
    /// short event can still fit after a long one was turned away -- which is
    /// why gaps are counted as runs rather than as a single flag flipped once.
    /// (The event ceiling *is* final once reached, since every event costs
    /// one.)
    fn reserve(&self, source: AudioSource, bytes: usize) -> Reserved {
        if self
            .bytes
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |used| {
                used.checked_add(bytes)
                    .filter(|total| *total <= MAX_TRANSCRIPT_BYTES)
            })
            .is_err()
        {
            return self.refuse(source);
        }

        if self
            .events
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |used| {
                used.checked_add(1)
                    .filter(|total| *total <= MAX_TRANSCRIPT_EVENTS)
            })
            .is_err()
        {
            self.bytes.fetch_sub(bytes, Ordering::Relaxed);
            return self.refuse(source);
        }

        // Admitting closes this capture's stretch, so the next refusal counts
        // as a new one.
        self.dropping(source).store(false, Ordering::Relaxed);
        Reserved::Room
    }

    /// Mark `source` as dropping, counting a gap if it was not already.
    fn refuse(&self, source: AudioSource) -> Reserved {
        let opens_a_gap = !self.dropping(source).swap(true, Ordering::Relaxed);
        if opens_a_gap {
            self.gaps.fetch_add(1, Ordering::Relaxed);
        }
        Reserved::NoRoom { opens_a_gap }
    }

    /// How many stretches of audio this session never transcribed.
    fn gaps(&self) -> u32 {
        self.gaps.load(Ordering::Relaxed)
    }
}

/// One provider session: two sockets, and everything read from them.
pub struct DeepgramRealtime {
    credentials: Arc<dyn CredentialStore>,
    config: Option<MeetingProviderConfig>,
    /// Set once in `connect`, from `deepgram::prepare`. Carried rather than
    /// re-looked-up so `open` never has to unwrap a lookup that could not
    /// fail in practice but is not provably infallible to the compiler.
    caps: Option<&'static MeetingProviderCapabilities>,
    /// One connection per capture. `None` until `connect`.
    mic: Option<Connection>,
    system: Option<Connection>,
    /// Everything both sockets have said, in arrival order.
    events: Arc<std::sync::Mutex<Vec<TranscriptEvent>>>,
    budget: Arc<TranscriptBudget>,
    /// Why the session is over, if it is. One cell for both sockets — see
    /// [`SessionFault`]. Neither a socket nor a clock; see
    /// `no_rotation_timer_exists_on_this_struct` at the bottom of this file.
    fault: SessionFault,
    live: Option<mpsc::UnboundedSender<TranscriptEvent>>,
    started: Option<Instant>,
    paused: bool,
}

/// One open socket and the task reading it.
struct Connection {
    sink: Sink,
    reader: tokio::task::JoinHandle<()>,
}

impl Connection {
    /// Close the socket and stop its reader.
    async fn close(mut self) {
        // The graceful termination message first, then the close frame -- see
        // `deepgram::terminate`'s doc comment on why sending it costs nothing
        // even if the server does not recognise it.
        let _ = self
            .sink
            .send(Message::Text(deepgram::terminate().to_string()))
            .await;
        let _ = self.sink.send(Message::Close(None)).await;
        let _ = self.sink.close().await;
        self.reader.abort();
    }
}

impl std::fmt::Debug for DeepgramRealtime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DeepgramRealtime")
            .field("mic", &self.mic.is_some())
            .field("system", &self.system.is_some())
            .finish_non_exhaustive()
    }
}

impl DeepgramRealtime {
    #[must_use]
    pub fn new(credentials: Arc<dyn CredentialStore>) -> Self {
        Self {
            credentials,
            config: None,
            caps: None,
            mic: None,
            system: None,
            events: Arc::new(std::sync::Mutex::new(Vec::new())),
            budget: Arc::new(TranscriptBudget::default()),
            fault: SessionFault::new(),
            live: None,
            started: None,
            paused: false,
        }
    }

    /// Deliver each event as it arrives, as well as keeping it.
    #[must_use]
    pub fn with_live_events(mut self, sink: mpsc::UnboundedSender<TranscriptEvent>) -> Self {
        self.live = Some(sink);
        self
    }

    /// Open one socket for `source`.
    async fn open(&self, source: AudioSource) -> Result<Connection, MeetingError> {
        let config = self
            .config
            .as_ref()
            .ok_or_else(|| MeetingError::Disconnected {
                provider: PROVIDER_ID.to_owned(),
                detail: "no session configuration".to_owned(),
            })?;
        let caps = self.caps.ok_or_else(|| MeetingError::Disconnected {
            provider: PROVIDER_ID.to_owned(),
            detail: "no session configuration".to_owned(),
        })?;

        let endpoint = deepgram::endpoint(
            config.sample_rate,
            config.language.as_deref(),
            &config.keyterms,
            caps,
        )?;

        // Through the boundary, every time a socket is opened -- not once at
        // startup. A blocked host must not become reachable by reconnecting.
        let url = kw_network_policy::validate_websocket(&endpoint, PURPOSE).map_err(|err| {
            MeetingError::Rejected {
                provider: PROVIDER_ID.to_owned(),
                detail: err.to_string(),
            }
        })?;

        // Resolved last, immediately before the handshake -- see
        // `deepgram::token`'s doc comment. `token_rotation: true` describes
        // upstream's minted temporary tokens; this resolves the user's own
        // stored key once per socket, with no refresh scheduled.
        let secret = deepgram::token(&self.credentials, config.key_ref.as_ref())?;

        let mut request =
            url.as_str()
                .into_client_request()
                .map_err(|err| MeetingError::Rejected {
                    provider: PROVIDER_ID.to_owned(),
                    detail: err.to_string(),
                })?;
        {
            let headers = request.headers_mut();
            // `expose` at the last possible moment, formatted directly into
            // the value handed to `.parse()` with no intermediate `let`
            // binding that could hold the exposed secret a moment longer
            // than necessary. Nothing above this line holds it at all.
            headers.insert(
                AUTH_HEADER,
                format!("{AUTH_SCHEME} {}", secret.expose())
                    .parse()
                    .map_err(|_| MeetingError::Rejected {
                        provider: PROVIDER_ID.to_owned(),
                        detail: "the stored credential is not a valid header value".to_owned(),
                    })?,
            );
        }

        let websocket_config = WebSocketConfig {
            max_frame_size: Some(MAX_MESSAGE_BYTES),
            max_message_size: Some(MAX_MESSAGE_BYTES),
            ..Default::default()
        };
        let (socket, _) =
            tokio_tungstenite::connect_async_with_config(request, Some(websocket_config), false)
                .await
                .map_err(|err| MeetingError::Rejected {
                    provider: PROVIDER_ID.to_owned(),
                    // The handshake failure, which for a bad key is worth
                    // repeating verbatim -- it is the difference between "your
                    // key is wrong" and "the network is down".
                    detail: err.to_string(),
                })?;

        let (sink, stream) = socket.split();

        // Nothing is sent after the handshake: every piece of session
        // configuration travelled in the query string. See
        // `deepgram::endpoint`'s doc comment.
        let reader = tokio::spawn(read(
            stream,
            source,
            Arc::clone(&self.events),
            Arc::clone(&self.budget),
            self.live.clone(),
            self.started.unwrap_or_else(Instant::now),
            self.fault.clone(),
        ));

        Ok(Connection { sink, reader })
    }

    fn connection(&mut self, source: AudioSource) -> Option<&mut Connection> {
        match source {
            AudioSource::Mic => self.mic.as_mut(),
            AudioSource::System => self.system.as_mut(),
        }
    }
}

/// Read one socket until it closes.
async fn read(
    mut stream: futures::stream::SplitStream<Socket>,
    source: AudioSource,
    events: Arc<std::sync::Mutex<Vec<TranscriptEvent>>>,
    budget: Arc<TranscriptBudget>,
    live: Option<mpsc::UnboundedSender<TranscriptEvent>>,
    started: Instant,
    fault: SessionFault,
) {
    // Whether this socket has ever produced a frame this client could parse.
    // Only this loop knows it, and `fault::ends_the_session` needs it to tell
    // garbage-where-a-handshake-belonged from a keepalive nobody recognises.
    let mut understood = false;

    while let Some(frame) = stream.next().await {
        let text = match frame {
            Ok(Message::Text(text)) => text,
            Ok(Message::Close(frame)) => {
                // The code and the reason are the useful part and used to be
                // discarded here -- this arm was a `debug!` with neither in it.
                // `openai_socket` learned that lesson on 2026-08-28 against a
                // live server; this client has never met one, which is the
                // reason to have the words ready rather than a reason not to.
                let said = close_reason(
                    frame.as_ref().map(|f| u16::from(f.code)),
                    frame.as_ref().map_or("", |f| f.reason.as_ref()),
                );
                tracing::warn!(?source, %said, "the transcription socket was closed");
                fault.closed(source, &said);
                return;
            }
            // Ping, pong and binary. tungstenite answers pings itself.
            Ok(_) => continue,
            Err(err) => {
                tracing::warn!(error = %err, ?source, "the transcription socket failed");
                fault.transport(source, &err.to_string());
                return;
            }
        };

        let incoming = deepgram::interpret(&text);
        if !matches!(incoming, Incoming::Unreadable { .. }) {
            // Parsed, whatever it turned out to mean -- a transcript, a
            // readiness event, an error frame, even an event this client does
            // not act on. From here on, a frame that will not parse is a
            // keepalive this parser does not know rather than a wire it cannot
            // read at all, and the two get different answers.
            understood = true;
        }

        match incoming {
            Incoming::Transcript {
                text,
                is_final,
                speaker,
            } if !text.trim().is_empty() => {
                // Dropped, and counted, but the reader keeps going. It used
                // to `return` here, which was three problems in one line.
                //
                // The session's *other* frames still matter: an error frame
                // arriving after the ceiling has to reach `SessionFault`, and a
                // dead reader cannot record one -- so the budget silently
                // switched off KNEG-79's fix for the rest of the meeting. The
                // socket also has to keep draining, or the provider is left
                // pushing into a buffer nobody reads. And because the budget is
                // shared, whichever capture happened to hit the ceiling first
                // was the one that died, leaving the other transcribing: a
                // meeting with the microphone kept and the system audio not,
                // decided by arrival order.
                //
                // What the ceiling is for is bounding retention, and dropping
                // the text does that on its own.
                if let Reserved::NoRoom { opens_a_gap } = budget.reserve(source, text.len()) {
                    if opens_a_gap {
                        // Once per stretch. Per frame, this would be the
                        // loudest line in the log for the rest of the meeting.
                        tracing::warn!(
                            ?source,
                            "the transcript session budget is full; this capture is no longer \
                             being transcribed, and the saved note will say so"
                        );
                    }
                    continue;
                }
                let event = deepgram::event(
                    text,
                    is_final,
                    speaker,
                    started.elapsed().as_secs_f64(),
                    source,
                );
                if let Some(sink) = &live {
                    // A closed receiver is not a reason to stop reading: the
                    // transcript is still being kept.
                    let _ = sink.send(event.clone());
                }
                if let Ok(mut held) = events.lock() {
                    held.push(event);
                }
            }
            // An empty transcript, which is most frames while nobody is
            // speaking.
            Incoming::Transcript { .. } | Incoming::Ready => {}
            Incoming::Failed { message } => {
                // Recorded, not only logged. Until 2026-08-31 this arm *was*
                // the defect: the warning went to a log nobody in the room can
                // see and `send_audio` carried on succeeding, so a provider
                // that rejected the session and kept the socket open produced
                // a meeting that "completed successfully" holding nothing.
                //
                // This client has never been run against a live provider and no
                // key for it exists on any machine this project has touched, so
                // the wiring below is verified by the tests at the bottom of
                // this file and the behaviour against a real provider error is
                // not verified at all.
                tracing::warn!(?source, %message, "the provider reported an error");
                fault.reported(source, &message);
                // Reading continues even though the session is over: a provider
                // often follows an error frame with a close frame, and seeing
                // the whole sequence in the log is what made the five OpenAI
                // faults of 2026-08-28 diagnosable one at a time. First-wins
                // keeps the better sentence whatever follows.
            }
            Incoming::Unreadable { preview } => {
                if fault.unreadable(source, &preview, understood) {
                    tracing::warn!(
                        ?source,
                        %preview,
                        "the provider's first frame could not be read -- ending the session"
                    );
                    // Unlike the arm above, this one stops: by definition
                    // nothing this socket sends can be read, so there is
                    // nothing to stay for.
                    return;
                }
                // A frame this parser does not recognise on a socket that has
                // been speaking its language. Worth seeing; not worth ending a
                // working meeting over -- see `fault::ends_the_session`.
                tracing::warn!(?source, %preview, "a frame from the provider could not be read");
            }
            Incoming::Other { kind } => {
                tracing::trace!(?source, %kind, "an event this client does not act on");
            }
        }
    }
}

#[async_trait::async_trait]
impl MeetingTranscriptionProvider for DeepgramRealtime {
    async fn connect(&mut self, config: MeetingProviderConfig) -> Result<(), MeetingError> {
        let caps = deepgram::prepare(&config)?;

        self.caps = Some(caps);
        self.config = Some(config);
        self.started = Some(Instant::now());
        // A fresh cell per session. `SessionFault` cannot be cleared -- an
        // `OnceLock` behind an `Arc` has no way to be unset -- so a second
        // meeting on a reused provider gets its own rather than inheriting the
        // first one's verdict.
        self.fault = SessionFault::new();
        self.budget = Arc::new(TranscriptBudget::default());

        // Both, before returning. A session with only the microphone open
        // would transcribe half the meeting and report success.
        self.mic = Some(self.open(AudioSource::Mic).await?);
        self.system = Some(self.open(AudioSource::System).await?);
        self.paused = false;
        Ok(())
    }

    async fn send_audio(&mut self, chunk: AudioChunk) -> Result<(), MeetingError> {
        // First: before `paused`, and before the connection lookup. A provider
        // that reported an error and kept the socket open produces a
        // *successful* send, so nothing about the send itself can be asked --
        // this is the check that was missing, and its absence is why a rejected
        // session-config frame let a meeting finish looking successful. Being
        // first is also what makes it testable with no socket.
        if let Some(fault) = self.fault.take() {
            return Err(fault.into_error(PROVIDER_ID));
        }

        if self.paused {
            // Discarded, which is what pausing a recording means. Buffering
            // would make "pause" mean "delay". A session already dead is
            // reported above rather than paused into silence.
            return Ok(());
        }

        let source = chunk.source;
        let connection = self
            .connection(source)
            .ok_or_else(|| MeetingError::Disconnected {
                provider: PROVIDER_ID.to_owned(),
                detail: "the session is not open".to_owned(),
            })?;

        // Raw bytes as a binary frame, not JSON-plus-base64 -- see
        // `deepgram::frame_bytes`'s doc comment.
        connection
            .sink
            .send(Message::Binary(chunk.pcm))
            .await
            .map_err(|err| MeetingError::Disconnected {
                provider: PROVIDER_ID.to_owned(),
                detail: err.to_string(),
            })
    }

    async fn pause(&mut self) -> Result<(), MeetingError> {
        // The sockets stay open. Tearing them down and rebuilding would lose
        // the session's context, which is the whole reason `pause` exists
        // separately from `finish`.
        self.paused = true;
        Ok(())
    }

    async fn resume(&mut self) -> Result<(), MeetingError> {
        self.paused = false;
        Ok(())
    }

    async fn finish(&mut self) -> Result<MeetingTranscript, MeetingError> {
        // Read before the sockets are closed, so a fault that arrived after the
        // last audio chunk is reported rather than lost to this client's own
        // close frame.
        let fault = self.fault.take();

        for connection in [self.mic.take(), self.system.take()].into_iter().flatten() {
            connection.close().await;
        }

        if let Some(fault) = fault {
            // Rather than an empty transcript. An empty one becomes
            // `MeetingOutcome::NothingTranscribed`, which is the sentence a
            // quiet room gets; a provider that said something was wrong must
            // not reach the screen as silence.
            return Err(fault.into_error(PROVIDER_ID));
        }

        let events = self
            .events
            .lock()
            .map(|mut held| std::mem::take(&mut *held))
            .unwrap_or_default();

        let duration = self
            .started
            .map_or(0.0, |started| started.elapsed().as_secs_f64());

        Ok(deepgram::transcript(events, duration, self.budget.gaps()))
    }

    async fn cancel(&mut self) -> Result<(), MeetingError> {
        for connection in [self.mic.take(), self.system.take()].into_iter().flatten() {
            connection.close().await;
        }
        if let Ok(mut held) = self.events.lock() {
            held.clear();
        }
        Ok(())
    }

    fn capabilities(&self) -> &'static MeetingProviderCapabilities {
        crate::capabilities::capabilities(PROVIDER_ID).unwrap_or(&FALLBACK)
    }
}

/// Never reached: `deepgram-realtime` is in [`crate::PROVIDERS`] and a test
/// says so. Present only because this workspace forbids unwrapping in library
/// code.
static FALLBACK: MeetingProviderCapabilities = MeetingProviderCapabilities {
    id: PROVIDER_ID,
    name: "Deepgram Realtime",
    audio_leaves_device: true,
    keyterms: crate::KeytermSupport::Supported {
        max: None,
        wire: crate::KeytermWire::RepeatedQueryParam,
    },
    session_limit: None,
    token_rotation: true,
    sample_rates: &[16_000, 24_000, 48_000],
    speaker_labels: true,
    partials: true,
    language_detection: true,
    reconnect: true,
    batch_fallback: true,
};

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    fn store() -> Arc<dyn CredentialStore> {
        Arc::new(kw_security::MemoryCredentialStore::new())
    }

    fn config() -> MeetingProviderConfig {
        MeetingProviderConfig {
            provider: PROVIDER_ID.to_owned(),
            model: None,
            language: Some("en".to_owned()),
            keyterms: Vec::new(),
            key_ref: kw_security::KeyRef::new("deepgram"),
            sample_rate: 16_000,
        }
    }

    #[tokio::test]
    async fn a_missing_credential_is_refused_before_any_socket_is_opened() {
        // The order matters: a handshake attempted with no key would spend a
        // round trip only to fail, and would report a network failure rather
        // than a missing key.
        let mut provider = DeepgramRealtime::new(store());
        let err = provider.connect(config()).await.unwrap_err();
        assert!(
            matches!(err, MeetingError::MissingCredential { .. }),
            "{err:?}"
        );
    }

    #[tokio::test]
    async fn the_wrong_sample_rate_is_refused_before_the_credential_is_read() {
        let mut provider = DeepgramRealtime::new(store());
        let mut cfg = config();
        cfg.sample_rate = 8_000;

        let err = provider.connect(cfg).await.unwrap_err();
        assert!(matches!(err, MeetingError::SampleRate { .. }), "{err:?}");
    }

    #[tokio::test]
    async fn audio_before_connect_is_refused_rather_than_buffered() {
        let mut provider = DeepgramRealtime::new(store());
        let err = provider
            .send_audio(AudioChunk {
                pcm: vec![0; 64],
                source: AudioSource::Mic,
            })
            .await
            .unwrap_err();
        assert!(matches!(err, MeetingError::Disconnected { .. }), "{err:?}");
    }

    #[tokio::test]
    async fn finishing_a_session_that_never_opened_returns_an_empty_transcript() {
        // Rather than an error. Nothing was said, and that is not a failure.
        let mut provider = DeepgramRealtime::new(store());
        let transcript = provider.finish().await.unwrap();
        assert!(transcript.events.is_empty());
        assert_eq!(transcript.reconnects, 0);
        assert_eq!(transcript.gaps, 0);
    }

    #[tokio::test]
    async fn a_reported_fault_fails_the_next_chunk_rather_than_being_swallowed() {
        // The first test in this file to reach past the guard clauses, and the
        // one this whole change exists for. No socket and no key: the fault goes
        // into the session's own cell exactly as the reader task would put it
        // there, and `send_audio` is asked what it does about it. Before
        // 2026-08-31 the answer was `Ok(())`, for as long as the meeting was
        // left running.
        //
        // What this verifies is the wiring, and only the wiring. No key for this
        // provider exists on any machine this project has touched, so how it
        // behaves against a real provider error is still unverified.
        let mut provider = DeepgramRealtime::new(store());
        provider.fault.reported(
            AudioSource::System,
            "the session configuration was rejected",
        );

        let err = provider
            .send_audio(AudioChunk {
                pcm: vec![0; 64],
                source: AudioSource::Mic,
            })
            .await
            .unwrap_err();
        match err {
            MeetingError::ProviderFault { provider, detail } => {
                assert_eq!(provider, PROVIDER_ID);
                assert!(
                    detail.contains("session configuration was rejected"),
                    "{detail}"
                );
                assert!(
                    detail.contains("system audio"),
                    "one cell for both sockets, so the sentence has to say which: {detail}"
                );
            }
            other => panic!("{other:?}"),
        }
    }

    #[tokio::test]
    async fn a_fault_arriving_after_the_last_chunk_is_not_lost_at_finish() {
        // A provider that says something is wrong between the final chunk and
        // Stop used to produce a successful, empty transcript -- and an empty
        // transcript becomes `MeetingOutcome::NothingTranscribed`, which is
        // exactly the sentence a quiet room gets.
        let mut provider = DeepgramRealtime::new(store());
        assert!(
            provider
                .fault
                .unreadable(AudioSource::Mic, "<html>502 Bad Gateway</html>", false),
            "garbage where a session-accepted frame belonged ends the session"
        );

        let err = provider.finish().await.unwrap_err();
        match err {
            MeetingError::ProviderFault { detail, .. } => {
                assert!(detail.contains("502 Bad Gateway"), "{detail}");
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn the_declared_capabilities_are_the_tables() {
        let provider = DeepgramRealtime::new(store());
        assert_eq!(provider.capabilities().id, PROVIDER_ID);
        assert_eq!(
            provider.capabilities().sample_rates,
            &[16_000, 24_000, 48_000]
        );
        assert!(provider.capabilities().speaker_labels);
        assert!(
            provider.capabilities().token_rotation,
            "the table still names Deepgram's upstream rotation, even though this client does not implement it"
        );
    }

    #[test]
    fn no_rotation_timer_exists_on_this_struct() {
        // `token_rotation: true` in the capability table describes upstream
        // minting short-lived temporary tokens from its own hosted service
        // and refreshing them mid-session -- see the module documentation.
        // This client resolves the user's own stored key once per socket and
        // schedules no refresh, so there is deliberately no field here for a
        // rotation deadline the way `openai_socket::OpenAiRealtime` carries a
        // `clock: Option<SessionClock>` for its session cap.
        //
        // A string scan of `Debug`'s output is not a real tripwire for that:
        // this struct's `Debug` impl is hand-written from individual
        // `.field(...)` calls plus `finish_non_exhaustive()`, so it prints
        // only `mic` and `system` and would say nothing about a field added
        // anywhere else. `OpenAiRealtime` already demonstrates the failure
        // mode -- it holds a real `clock: Option<SessionClock>` field, and
        // its own `Debug` impl relabels it `"reconnects"`, so a scan for
        // "clock" against its rendered output finds nothing even though the
        // clock is right there.
        //
        // Destructuring the struct here instead, field by field with no `..`
        // rest pattern, is a compile-time tripwire rather than a runtime
        // string match: adding any field to `DeepgramRealtime` fails this to
        // compile until someone updates this pattern and states what the new
        // field is, which is what actually forces the "is this a rotation
        // clock?" question to be answered rather than merely hoped past.
        // `credentials`, `caps` and `events` are not asserted on -- their
        // values are not what this test is about -- but they still have to
        // be named for the pattern to be exhaustive, which is the point.
        //
        // Every field that does exist is either a socket, the pending
        // configuration, or plain bookkeeping. `started: Option<Instant>` is
        // the one field shaped like part of a clock, and it is not one: it
        // is a single instant recorded once, used only for
        // `elapsed().as_secs_f64()` event timestamps, with no paired
        // `Duration` limit the way `SessionClock` pairs `opened: Instant`
        // with `limit: Duration` to know when to act. A freshly constructed
        // provider has never connected, so every optional field below starts
        // empty and `paused` starts false.
        //
        // `fault: SessionFault` was added on 2026-08-31 and this tripwire is
        // what forced the question to be answered here. It is neither a socket
        // nor a clock: it holds one `Option<SocketFault>` in a `OnceLock`,
        // written once by whichever reader sees a fault first and read by
        // `send_audio` and `finish`. Nothing about it is time-based -- it has
        // no deadline, schedules nothing, and is never re-read on a timer --
        // so it does not make this struct's answer to "is there a rotation
        // clock here?" any less "no".
        let provider = DeepgramRealtime::new(store());
        let DeepgramRealtime {
            credentials: _,
            config,
            caps: _,
            mic,
            system,
            events: _,
            budget: _,
            fault,
            live,
            started,
            paused,
        } = provider;

        assert!(config.is_none());
        assert!(mic.is_none());
        assert!(system.is_none());
        assert!(live.is_none());
        assert!(
            fault.take().is_none(),
            "a provider that has never connected has nothing to have failed at"
        );
        assert!(
            started.is_none(),
            "session-start bookkeeping, not a rotation deadline -- see the comment above"
        );
        assert!(!paused);
    }

    /// `Reserved::Room`, ignoring whether a refusal opened a gap.
    fn admitted(reserved: Reserved) -> bool {
        reserved == Reserved::Room
    }

    #[test]
    fn transcript_budget_limits_bytes_across_both_sockets() {
        let budget = TranscriptBudget::default();
        assert!(admitted(
            budget.reserve(AudioSource::Mic, MAX_TRANSCRIPT_BYTES)
        ));
        assert!(!admitted(budget.reserve(AudioSource::System, 1)));
        assert_eq!(budget.bytes.load(Ordering::Relaxed), MAX_TRANSCRIPT_BYTES);
        assert_eq!(budget.events.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn transcript_budget_limits_event_overhead() {
        let budget = TranscriptBudget::default();
        for _ in 0..MAX_TRANSCRIPT_EVENTS {
            assert!(admitted(budget.reserve(AudioSource::Mic, 1)));
        }
        assert!(!admitted(budget.reserve(AudioSource::Mic, 1)));
        assert_eq!(budget.bytes.load(Ordering::Relaxed), MAX_TRANSCRIPT_EVENTS);
        assert_eq!(budget.events.load(Ordering::Relaxed), MAX_TRANSCRIPT_EVENTS);
    }

    /* --------------------------------------------- what the ceiling costs */

    #[test]
    fn a_run_of_refusals_is_one_gap_and_not_one_per_frame() {
        // The count reaches `MeetingTranscript::gaps`, which `keep` renders as
        // "N stretch(es) of audio were never transcribed". Counting per frame
        // would report a number nobody could act on -- a long meeting past the
        // ceiling would claim thousands of holes where there is one.
        let budget = TranscriptBudget::default();
        assert!(admitted(
            budget.reserve(AudioSource::Mic, MAX_TRANSCRIPT_BYTES)
        ));
        assert_eq!(budget.gaps(), 0, "nothing has been refused yet");

        assert_eq!(
            budget.reserve(AudioSource::Mic, 10),
            Reserved::NoRoom { opens_a_gap: true }
        );
        for _ in 0..50 {
            assert_eq!(
                budget.reserve(AudioSource::Mic, 10),
                Reserved::NoRoom { opens_a_gap: false },
                "still the same stretch"
            );
        }
        assert_eq!(budget.gaps(), 1);
    }

    #[test]
    fn admitting_again_ends_the_stretch_so_the_next_refusal_is_a_new_one() {
        // A refusal is not final: the byte ceiling is a total, so a short event
        // can fit after a long one was turned away. Two holes with transcript
        // between them are two holes.
        let budget = TranscriptBudget::default();
        assert!(admitted(
            budget.reserve(AudioSource::Mic, MAX_TRANSCRIPT_BYTES - 4)
        ));

        assert_eq!(
            budget.reserve(AudioSource::Mic, 100),
            Reserved::NoRoom { opens_a_gap: true }
        );
        assert!(
            admitted(budget.reserve(AudioSource::Mic, 4)),
            "a short event still fits"
        );
        assert_eq!(
            budget.reserve(AudioSource::Mic, 100),
            Reserved::NoRoom { opens_a_gap: true },
            "the stretch closed when something got through, so this opens a new one"
        );
        assert_eq!(budget.gaps(), 2);
    }

    #[test]
    fn each_capture_has_its_own_stretch() {
        // One shared flag would end the microphone's stretch on the strength of
        // a system-audio frame, and the two captures are not the same hole.
        let budget = TranscriptBudget::default();
        for _ in 0..MAX_TRANSCRIPT_EVENTS {
            assert!(admitted(budget.reserve(AudioSource::Mic, 1)));
        }

        assert_eq!(
            budget.reserve(AudioSource::Mic, 1),
            Reserved::NoRoom { opens_a_gap: true }
        );
        assert_eq!(
            budget.reserve(AudioSource::System, 1),
            Reserved::NoRoom { opens_a_gap: true },
            "the system capture has its own stretch to open"
        );
        assert_eq!(budget.gaps(), 2);

        assert_eq!(
            budget.reserve(AudioSource::Mic, 1),
            Reserved::NoRoom { opens_a_gap: false },
            "and the microphone is still in the one it opened"
        );
        assert_eq!(budget.gaps(), 2);
    }

    #[test]
    fn a_session_that_never_filled_the_budget_reports_no_gaps() {
        let budget = TranscriptBudget::default();
        assert!(admitted(budget.reserve(AudioSource::Mic, 1_000)));
        assert!(admitted(budget.reserve(AudioSource::System, 1_000)));
        assert_eq!(budget.gaps(), 0);
    }
}
