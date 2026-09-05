//! The socket half of the OpenAI Realtime client.
//!
//! [`crate::openai`] is the part with rules in it — what to send, what a frame
//! means, when to reconnect — and is covered by fixtures. This is the plumbing,
//! and it is thin on purpose because almost no test here can reach it. The two
//! exceptions arrived with [`crate::fault`]: a fault can be recorded into the
//! session's own cell exactly as the reader task would, with no socket, no key
//! and no network, and `send_audio` and `finish` can then be asked what they do
//! about it. That is the one path a live socket used to be the only way to
//! reach, and it is the path this client got wrong.
//!
//! # Two sockets, one per capture
//!
//! The microphone and the system output go to **separate sessions**. One socket
//! carrying both would make [`AudioSource`] on every transcript event a guess,
//! and that field exists precisely so a meeting can be attributed when the
//! provider labels no speakers — which this one does not. It would also hand the
//! server's voice-activity detection two overlapping speakers and ask it to find
//! sentence boundaries in the result.
//!
//! The same reasoning as the local provider running two window feeds. It costs
//! two connections for the same audio, which is the price of a transcript that
//! knows who said what.
//!
//! # The reconnect is the part worth watching
//!
//! Sixty minutes is a hard server-side cap, so a longer meeting must rebuild
//! both sockets. [`crate::openai::SessionClock`] decides when — two minutes
//! early, so the gap lands where this client chooses rather than mid-sentence —
//! and the count reaches `MeetingTranscript::reconnects`. This is the only
//! provider here where that path is reachable at all.

use std::sync::Arc;
use std::time::Instant;

use futures::{SinkExt as _, StreamExt as _};
use kw_security::CredentialStore;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::client::IntoClientRequest as _;
use tokio_tungstenite::tungstenite::Message;

use crate::capabilities::MeetingProviderCapabilities;
use crate::fault::{close_reason, SessionFault};
use crate::openai::{self, Incoming, SessionClock, DEFAULT_MODEL, ENDPOINT, PROVIDER_ID, PURPOSE};
use crate::{
    AudioChunk, AudioSource, MeetingError, MeetingProviderConfig, MeetingTranscript,
    MeetingTranscriptionProvider, TranscriptEvent,
};

type Socket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;
type Sink = futures::stream::SplitSink<Socket, Message>;

/// One provider session: two sockets, and everything read from them.
pub struct OpenAiRealtime {
    credentials: Arc<dyn CredentialStore>,
    config: Option<MeetingProviderConfig>,
    /// One connection per capture. `None` until `connect`.
    mic: Option<Connection>,
    system: Option<Connection>,
    /// Everything both sockets have said, in arrival order.
    events: Arc<std::sync::Mutex<Vec<TranscriptEvent>>>,
    /// Why the session is over, if it is. One cell for both sockets — see
    /// [`SessionFault`]. Distinct from `Connection::closed` below, which answers
    /// a different question.
    fault: SessionFault,
    live: Option<mpsc::UnboundedSender<TranscriptEvent>>,
    clock: Option<SessionClock>,
    started: Option<Instant>,
    paused: bool,
}

/// Why a socket stopped, as the provider said it, shared with the writer.
///
/// The reader and the writer are two halves of a split socket in different
/// tasks, so a provider-initiated close reaches the reader and the writer finds
/// out only by failing. Without this it fails with tungstenite's own words --
/// `Sending after closing is not allowed` -- which describes our call, not the
/// reason, and is what a real meeting produced on 2026-08-28. The provider's
/// close frame carries a code and usually a sentence saying what was wrong;
/// this keeps it so the failure can be reported in those terms instead.
///
/// **Not the same thing as [`SessionFault`], and deliberately not folded into
/// it.** This one supplies better *words* for a send that has already failed;
/// that one decides whether the session is over at all. Folding them would put
/// this behaviour at risk for a tidiness gain: it is per-socket, and a
/// session-wide first-wins cell would be poisoned by the close frame
/// [`Connection::close`] provokes from the socket this client's own reconnect
/// retires. Two cells, two jobs, neither needing to be cleared.
type Closed = Arc<std::sync::Mutex<Option<String>>>;

/// One open socket and the task reading it.
struct Connection {
    sink: Sink,
    reader: tokio::task::JoinHandle<()>,
    /// Set by the reader if the provider closed the socket first.
    closed: Closed,
}

impl Drop for Connection {
    fn drop(&mut self) {
        // A dropped JoinHandle detaches its task, which still owns the socket's
        // read half. This also covers aborted connects and timed-out cleanup.
        self.reader.abort();
    }
}

impl Connection {
    /// Close the socket and stop its reader.
    async fn close(mut self) {
        // A close frame rather than a dropped connection: the server bills a
        // session until it knows the client has gone.
        let _ = self.sink.send(Message::Close(None)).await;
        let _ = self.sink.close().await;
        self.reader.abort();
    }
}

impl std::fmt::Debug for OpenAiRealtime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OpenAiRealtime")
            .field("mic", &self.mic.is_some())
            .field("system", &self.system.is_some())
            .field(
                "reconnects",
                &self.clock.as_ref().map_or(0, SessionClock::reconnects),
            )
            .finish_non_exhaustive()
    }
}

impl OpenAiRealtime {
    #[must_use]
    pub fn new(credentials: Arc<dyn CredentialStore>) -> Self {
        Self {
            credentials,
            config: None,
            mic: None,
            system: None,
            events: Arc::new(std::sync::Mutex::new(Vec::new())),
            fault: SessionFault::new(),
            live: None,
            clock: None,
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

    /// Open one socket and configure its session.
    async fn open(&self, source: AudioSource) -> Result<Connection, MeetingError> {
        let config = self
            .config
            .as_ref()
            .ok_or_else(|| MeetingError::Disconnected {
                provider: PROVIDER_ID.to_owned(),
                detail: "no session configuration".to_owned(),
            })?;

        // Through the boundary, every time a socket is opened -- not once at
        // startup. A blocked host must not become reachable by reconnecting.
        let url = kw_network_policy::validate_websocket(ENDPOINT, PURPOSE).map_err(|err| {
            MeetingError::Rejected {
                provider: PROVIDER_ID.to_owned(),
                detail: err.to_string(),
            }
        })?;

        let secret = openai::token(&self.credentials, config.key_ref.as_ref())?;

        let mut request =
            url.as_str()
                .into_client_request()
                .map_err(|err| MeetingError::Rejected {
                    provider: PROVIDER_ID.to_owned(),
                    detail: err.to_string(),
                })?;
        {
            let headers = request.headers_mut();
            // `expose` at the last possible moment, into a header that is not
            // logged. Nothing above this line holds the secret.
            let bearer = format!("Bearer {}", secret.expose());
            headers.insert(
                "Authorization",
                bearer.parse().map_err(|_| MeetingError::Rejected {
                    provider: PROVIDER_ID.to_owned(),
                    detail: "the stored credential is not a valid header value".to_owned(),
                })?,
            );
        }

        let (socket, _) = tokio_tungstenite::connect_async(request)
            .await
            .map_err(|err| MeetingError::Rejected {
                provider: PROVIDER_ID.to_owned(),
                // The handshake failure, which for a bad key is a 401 and worth
                // repeating verbatim -- it is the difference between "your key is
                // wrong" and "the network is down".
                detail: err.to_string(),
            })?;

        let (mut sink, stream) = socket.split();

        let model = config.model.as_deref().unwrap_or(DEFAULT_MODEL);
        let update = openai::session_update(model, config.language.as_deref(), config.sample_rate);
        sink.send(Message::Text(update.to_string()))
            .await
            .map_err(|err| MeetingError::Disconnected {
                provider: PROVIDER_ID.to_owned(),
                detail: err.to_string(),
            })?;

        let closed: Closed = Arc::new(std::sync::Mutex::new(None));
        let reader = tokio::spawn(read(
            stream,
            source,
            Arc::clone(&self.events),
            self.live.clone(),
            self.started.unwrap_or_else(Instant::now),
            Arc::clone(&closed),
            self.fault.clone(),
        ));

        Ok(Connection {
            sink,
            reader,
            closed,
        })
    }

    /// Rebuild both sockets, keeping everything transcribed so far.
    async fn reconnect(&mut self) -> Result<(), MeetingError> {
        tracing::info!(
            provider = PROVIDER_ID,
            "the session limit is close -- rebuilding both sockets"
        );

        if let Some(connection) = self.mic.take() {
            connection.close().await;
        }
        if let Some(connection) = self.system.take() {
            connection.close().await;
        }

        // A new cell for the new pair of sockets, and this is not tidiness.
        // `Connection::close` above sends a close frame and only then aborts the
        // reader, so a retired socket can still read the server's reply -- and a
        // close frame is a `FaultKind::Reported`. Kept across generations, that
        // would fail every meeting over sixty minutes at its own reconnect. The
        // retired readers keep clones of the old cell and write into it; nothing
        // reads it again.
        //
        // What that costs: a genuine fault reported in the moments before a
        // scheduled rebuild is discarded. The rebuild will meet the same
        // provider a line later and be refused by it, so the loss is a worse
        // sentence rather than a lost failure.
        self.fault = SessionFault::new();

        let mic = self.open(AudioSource::Mic).await?;
        let system = self.open(AudioSource::System).await?;
        // Keep the first socket local until both opens succeed. Its Drop guard
        // closes the reader if the second open fails or this future is cancelled.
        self.mic = Some(mic);
        self.system = Some(system);

        if let Some(clock) = self.clock.as_mut() {
            clock.reconnected(Instant::now());
        }
        Ok(())
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
    live: Option<mpsc::UnboundedSender<TranscriptEvent>>,
    started: Instant,
    closed: Closed,
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
                // The code and reason are the useful part and used to be
                // discarded. `warn`, not `debug`: the provider ending a session
                // we did not end is worth seeing without turning logging up.
                let said = close_reason(
                    frame.as_ref().map(|f| u16::from(f.code)),
                    frame.as_ref().map_or("", |f| f.reason.as_ref()),
                );
                tracing::warn!(?source, %said, "the transcription socket was closed");
                // Both cells, and they are not the same cell -- see `Closed`.
                if let Ok(mut held) = closed.lock() {
                    *held = Some(said.clone());
                }
                fault.closed(source, &said);
                return;
            }
            // Ping, pong and binary. tungstenite answers pings itself.
            Ok(_) => continue,
            Err(err) => {
                tracing::warn!(error = %err, ?source, "the transcription socket failed");
                let said = err.to_string();
                if let Ok(mut held) = closed.lock() {
                    *held = Some(said.clone());
                }
                fault.transport(source, &said);
                return;
            }
        };

        let incoming = openai::interpret(&text);
        if !matches!(incoming, Incoming::Unreadable { .. }) {
            // Parsed, whatever it turned out to mean -- a transcript, a
            // readiness event, an error frame, even an event this client does
            // not act on. From here on, a frame that will not parse is a
            // keepalive this parser does not know rather than a wire it cannot
            // read at all, and the two get different answers.
            understood = true;
        }

        match incoming {
            Incoming::Transcript { text, is_final } if !text.trim().is_empty() => {
                let event = openai::event(text, is_final, started.elapsed().as_secs_f64(), source);
                if let Some(sink) = &live {
                    // A closed receiver is not a reason to stop reading: the
                    // transcript is still being kept.
                    let _ = sink.send(event.clone());
                }
                if let Ok(mut held) = events.lock() {
                    held.push(event);
                }
            }
            // An empty delta between words, which is most frames.
            Incoming::Transcript { .. } | Incoming::Ready => {}
            Incoming::Failed { message } => {
                // Recorded, not only logged. Until 2026-08-31 this arm *was*
                // the defect: the warning went to a log nobody in the room can
                // see and `send_audio` carried on succeeding, so a provider
                // that rejected the session and kept the socket open produced a
                // meeting that "completed successfully" holding nothing.
                tracing::warn!(?source, %message, "the provider reported an error");
                fault.reported(source, &message);
                // Reading continues even though the session is over. The
                // provider often follows an error frame with a close frame, and
                // seeing the whole sequence in the log is what made the five
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
impl MeetingTranscriptionProvider for OpenAiRealtime {
    async fn connect(&mut self, config: MeetingProviderConfig) -> Result<(), MeetingError> {
        let caps = openai::prepare(&config)?;

        let now = Instant::now();
        self.config = Some(config);
        self.started = Some(now);
        self.clock = Some(SessionClock::new(caps.session_limit, now));
        // A fresh cell per session. `SessionFault` cannot be cleared -- an
        // `OnceLock` behind an `Arc` has no way to be unset -- so a second
        // meeting on a reused provider gets its own rather than inheriting the
        // first one's verdict.
        self.fault = SessionFault::new();

        // Both, before returning. A session with only the microphone open would
        // transcribe half the meeting and report success.
        let mic = self.open(AudioSource::Mic).await?;
        let system = self.open(AudioSource::System).await?;
        // Keep the first socket local until both opens succeed. Its Drop guard
        // closes the reader if the second open fails or this future is cancelled.
        self.mic = Some(mic);
        self.system = Some(system);
        self.paused = false;
        Ok(())
    }

    async fn send_audio(&mut self, chunk: AudioChunk) -> Result<(), MeetingError> {
        // First: before `paused`, before the reconnect clock, and before the
        // connection lookup. A provider that reported an error and kept the
        // socket open produces a *successful* send, so nothing about the send
        // itself can be asked -- this is the check that was missing, and its
        // absence is why a rejected session-config frame let a meeting finish
        // looking successful. Being first is also what makes it testable with
        // no socket.
        if let Some(fault) = self.fault.take() {
            return Err(fault.into_error(PROVIDER_ID));
        }

        if self.paused {
            // Discarded, which is what pausing a recording means. Buffering would
            // make "pause" mean "delay". A session already dead is reported
            // above rather than paused into silence.
            return Ok(());
        }

        // Checked here rather than on a timer, so the rebuild happens between
        // frames instead of in the middle of one.
        let due = self
            .clock
            .as_ref()
            .is_some_and(|clock| clock.should_reconnect(Instant::now()));
        if due {
            self.reconnect().await?;
        }

        let frame = openai::frame_for(&chunk);
        let connection =
            self.connection(chunk.source)
                .ok_or_else(|| MeetingError::Disconnected {
                    provider: PROVIDER_ID.to_owned(),
                    detail: "the session is not open".to_owned(),
                })?;

        let recorded = connection.closed.lock().ok().and_then(|held| held.clone());

        connection
            .sink
            .send(Message::Text(frame.to_string()))
            .await
            .map_err(|err| MeetingError::Disconnected {
                provider: PROVIDER_ID.to_owned(),
                // What the provider said, if it said anything. A send that
                // fails because the socket is already closed reports our own
                // call (`Sending after closing is not allowed`) and tells
                // nobody why the session ended; the reader has the reason.
                detail: recorded.unwrap_or_else(|| err.to_string()),
            })
    }

    async fn pause(&mut self) -> Result<(), MeetingError> {
        // The sockets stay open. Tearing them down and rebuilding would lose the
        // session's context, which is the whole reason `pause` exists separately
        // from `finish`.
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
            // quiet room gets; a provider that said something was wrong must not
            // reach the screen as silence.
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
        let reconnects = self.clock.as_ref().map_or(0, SessionClock::reconnects);

        Ok(openai::transcript(events, duration, reconnects))
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

/// Never reached: `openai-realtime` is in [`crate::PROVIDERS`] and a test says
/// so. Present only because this workspace forbids unwrapping in library code.
static FALLBACK: MeetingProviderCapabilities = MeetingProviderCapabilities {
    id: PROVIDER_ID,
    name: "OpenAI Realtime",
    audio_leaves_device: true,
    keyterms: crate::KeytermSupport::Unsupported,
    session_limit: Some(std::time::Duration::from_secs(3_600)),
    token_rotation: false,
    sample_rates: &[24_000],
    speaker_labels: false,
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
            language: None,
            keyterms: Vec::new(),
            key_ref: kw_security::KeyRef::new("openai"),
            sample_rate: 24_000,
        }
    }

    #[tokio::test]
    async fn a_missing_credential_is_refused_before_any_socket_is_opened() {
        // The order matters: a handshake attempted with no key would spend a
        // round trip to be told 401, and would report a network failure rather
        // than a missing key.
        let mut provider = OpenAiRealtime::new(store());
        let err = provider.connect(config()).await.unwrap_err();
        assert!(
            matches!(err, MeetingError::MissingCredential { .. }),
            "{err:?}"
        );
    }

    #[tokio::test]
    async fn the_wrong_sample_rate_is_refused_before_the_credential_is_read() {
        let mut provider = OpenAiRealtime::new(store());
        let mut cfg = config();
        cfg.sample_rate = 16_000;

        let err = provider.connect(cfg).await.unwrap_err();
        assert!(matches!(err, MeetingError::SampleRate { .. }), "{err:?}");
    }

    #[tokio::test]
    async fn audio_before_connect_is_refused_rather_than_buffered() {
        let mut provider = OpenAiRealtime::new(store());
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
        let mut provider = OpenAiRealtime::new(store());
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
        // The words are the real ones, from the meeting of 2026-08-28.
        let mut provider = OpenAiRealtime::new(store());
        provider.fault.reported(
            AudioSource::System,
            "The Realtime Beta API is no longer supported.",
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
                assert!(detail.contains("Realtime Beta API"), "{detail}");
                assert!(
                    detail.contains("system audio"),
                    "one cell for both sockets, so the sentence has to say which: {detail}"
                );
            }
            other => panic!("{other:?}"),
        }
    }

    #[tokio::test]
    async fn the_socket_a_reconnect_retires_cannot_fail_the_session_it_was_rebuilt_for() {
        // This client is the only one with a reconnect, and it is the only one
        // where a session-wide fault cell can be poisoned by this client's own
        // tidying up: `Connection::close` sends a close frame and only then
        // aborts the reader, so the retired socket can still read the server's
        // reply -- and a close frame is a `FaultKind::Reported`. Shared across
        // generations, that would fail every meeting over sixty minutes at its
        // own reconnect.
        //
        // `reconnect` therefore swaps in a new cell, and the retired reader's
        // clone writes into the discarded one. This exercises the swap rather
        // than the socket rebuild -- nothing here can open a socket -- so it
        // checks the mechanism and not the timing.
        let mut provider = OpenAiRealtime::new(store());
        let retired = provider.fault.clone();
        provider.fault = SessionFault::new();
        retired.closed(
            AudioSource::Mic,
            "the provider closed the connection (code 1000)",
        );

        assert!(provider.fault.take().is_none());
        assert!(provider.finish().await.unwrap().events.is_empty());
    }

    #[tokio::test]
    async fn a_fault_arriving_after_the_last_chunk_is_not_lost_at_finish() {
        // A provider that says something is wrong between the final chunk and
        // Stop used to produce a successful, empty transcript -- and an empty
        // transcript becomes `MeetingOutcome::NothingTranscribed`, which is
        // exactly the sentence a quiet room gets.
        let mut provider = OpenAiRealtime::new(store());
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
        let provider = OpenAiRealtime::new(store());
        assert_eq!(provider.capabilities().id, PROVIDER_ID);
        assert_eq!(provider.capabilities().sample_rates, &[24_000]);
    }
}

#[cfg(test)]
mod connection_drop_tests {
    use super::*;

    #[tokio::test]
    async fn dropping_connection_closes_the_remote_socket() -> Result<(), Box<dyn std::error::Error>>
    {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let client = tokio::net::TcpStream::connect(listener.local_addr()?).await?;
        let (mut server, _) = listener.accept().await?;
        let socket = tokio_tungstenite::WebSocketStream::from_raw_socket(
            tokio_tungstenite::MaybeTlsStream::Plain(client),
            tokio_tungstenite::tungstenite::protocol::Role::Client,
            None,
        )
        .await;
        let (sink, mut stream) = socket.split();
        let reader = tokio::spawn(async move { while stream.next().await.is_some() {} });
        let connection = Connection {
            sink,
            reader,
            closed: Arc::new(std::sync::Mutex::new(None)),
        };
        drop(connection);
        use tokio::io::AsyncReadExt as _;
        let mut byte = [0];
        let read = tokio::time::timeout(std::time::Duration::from_secs(2), server.read(&mut byte))
            .await??;
        assert_eq!(
            read, 0,
            "the detached reader must not keep the TCP connection open"
        );
        Ok(())
    }
}
