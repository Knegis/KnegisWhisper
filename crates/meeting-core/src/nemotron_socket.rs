//! The socket half of the sherpa-onnx streaming client.
//!
//! [`crate::nemotron`] is the part with rules in it and is covered by fixtures
//! taken off the real wire. This is the plumbing, and it is thin on purpose
//! because almost no test here can reach it — the same split, and the same
//! reason, as [`crate::openai_socket`].
//!
//! # Two sockets, one per capture, even though nothing leaves the machine
//!
//! The same shape as the four cloud clients, for the reason that has nothing to
//! do with the network: this server labels no speakers, so `AudioSource` is the
//! only thing that can attribute a meeting, and one socket carrying both
//! captures would make that field a guess. It would also hand one recogniser two
//! overlapping speakers and ask it to find sentence boundaries in the result.
//!
//! Two sockets to a local process costs two decoder instances of memory, which
//! is the price of a transcript that knows who said what.
//!
//! # There is nothing to reconnect around, and none is implemented
//!
//! No session limit, no token to rotate, no billing clock. A socket that dies is
//! a [`crate::fault::FaultKind::Transport`] fault and the session is over;
//! `capabilities.reconnect` is `false` and says so rather than implying a
//! recovery path that does not exist.
//!
//! # The finish handshake is the part worth watching
//!
//! **The server does not close the socket.** Measured. So `finish` cannot wait
//! for an end of stream — it sends TEXT `Done`, waits for the server's TEXT
//! `Done!`, and gives up after [`nemotron::FINISH_TIMEOUT`] rather than blocking
//! for as long as the meeting was long. A timeout there is not a fault: the
//! audio was sent and the transcript is whatever arrived, so it is logged and
//! the transcript is returned.

use std::sync::Arc;
use std::time::Instant;

use futures::{SinkExt as _, StreamExt as _};
use kw_network_policy::NetworkPolicy;
use tokio::sync::{mpsc, Notify};
use tokio_tungstenite::tungstenite::Message;

use crate::capabilities::MeetingProviderCapabilities;
use crate::fault::{close_reason, SessionFault};
use crate::nemotron::{
    self, Incoming, Pending, Segments, Step, CLIENT_DONE, FINISH_TIMEOUT, PROVIDER_ID,
};
use crate::{
    AudioChunk, AudioSource, MeetingError, MeetingProviderConfig, MeetingTranscript,
    MeetingTranscriptionProvider, TranscriptEvent,
};

type Socket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;
type Sink = futures::stream::SplitSink<Socket, Message>;

/// Everything transcribed, in arrival order, shared by both readers.
///
/// A `Vec` the readers **mutate** rather than only append to, which is unlike
/// every other client here and is forced by the protocol: `text` is cumulative,
/// so an update revises the event it belongs to. Indices stay valid because
/// nothing is ever removed.
type Events = Arc<std::sync::Mutex<Vec<TranscriptEvent>>>;

/// One provider session against a local sherpa server.
pub struct LocalStreaming {
    policy: NetworkPolicy,
    /// Where the server this application started is listening.
    port: u16,
    config: Option<MeetingProviderConfig>,
    mic: Option<Connection>,
    system: Option<Connection>,
    events: Events,
    /// Why the session is over, if it is.
    ///
    /// **Only ever [`crate::fault::FaultKind::Transport`] or
    /// [`crate::fault::FaultKind::Unreadable`] for this provider**, because the
    /// server has no error frame — measured, see [`crate::nemotron`]. Nothing
    /// here calls `SessionFault::reported`, and there is a test that no parse
    /// result can mean the provider complained.
    fault: SessionFault,
    live: Option<mpsc::UnboundedSender<TranscriptEvent>>,
    started: Option<Instant>,
    paused: bool,
}

/// One open socket, the task reading it, and the audio waiting for a full frame.
struct Connection {
    sink: Sink,
    reader: tokio::task::JoinHandle<()>,
    /// Audio held back so every frame but the last is the measured size.
    pending: Pending,
    /// Fired by the reader when the server says `Done!`.
    finished: Arc<Notify>,
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
        // A close frame rather than a dropped connection. The server does not
        // close first, so if this client simply walked away the server would
        // hold the session and its decoder until the process ended.
        let _ = self.sink.send(Message::Close(None)).await;
        let _ = self.sink.close().await;
        self.reader.abort();
    }
}

impl std::fmt::Debug for LocalStreaming {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LocalStreaming")
            .field("port", &self.port)
            .field("mic", &self.mic.is_some())
            .field("system", &self.system.is_some())
            .finish_non_exhaustive()
    }
}

impl LocalStreaming {
    /// A session against the server listening on `port`.
    ///
    /// The port is passed in rather than discovered: starting the server is the
    /// desktop crate's job, and a client that could go looking for one would be
    /// a client that could find somebody else's.
    #[must_use]
    pub fn new(policy: NetworkPolicy, port: u16) -> Self {
        Self {
            policy,
            port,
            config: None,
            mic: None,
            system: None,
            events: Arc::new(std::sync::Mutex::new(Vec::new())),
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

    /// Open one socket.
    async fn open(&self, source: AudioSource) -> Result<Connection, MeetingError> {
        // Through the boundary every time, not once at startup -- the same rule
        // the cloud clients follow. Here it is narrower than a check: the
        // address is *built* from a port, so there is no host for this call to
        // get wrong.
        let url =
            self.policy
                .loopback_websocket(self.port)
                .map_err(|err| MeetingError::Rejected {
                    provider: PROVIDER_ID.to_owned(),
                    detail: err.to_string(),
                })?;

        let (socket, _) = tokio_tungstenite::connect_async(url.as_str())
            .await
            .map_err(|err| MeetingError::Rejected {
                provider: PROVIDER_ID.to_owned(),
                // Repeated verbatim. For a local server the likely causes are
                // "it is not running" and "it is still loading the model", and
                // the connect error is what distinguishes them.
                detail: err.to_string(),
            })?;

        let (sink, stream) = socket.split();
        let finished = Arc::new(Notify::new());

        let reader = tokio::spawn(read(
            stream,
            source,
            Arc::clone(&self.events),
            self.live.clone(),
            self.started.unwrap_or_else(Instant::now),
            self.fault.clone(),
            Arc::clone(&finished),
        ));

        Ok(Connection {
            sink,
            reader,
            pending: Pending::new(),
            finished,
        })
    }

    fn connection(&mut self, source: AudioSource) -> Option<&mut Connection> {
        match source {
            AudioSource::Mic => self.mic.as_mut(),
            AudioSource::System => self.system.as_mut(),
        }
    }

    /// Send the held-back tail, say `Done`, and wait for `Done!`.
    ///
    /// Separated from [`MeetingTranscriptionProvider::finish`] so the two
    /// sockets can do it one after the other with the same words about what a
    /// timeout means.
    async fn drain_and_wait(connection: &mut Connection, source: AudioSource) {
        if let Some(tail) = connection.pending.drain() {
            if let Err(err) = connection.sink.send(Message::Binary(tail)).await {
                tracing::warn!(error = %err, ?source, "the last audio frame could not be sent");
                return;
            }
        }

        if let Err(err) = connection
            .sink
            .send(Message::Text(CLIENT_DONE.to_owned()))
            .await
        {
            tracing::warn!(error = %err, ?source, "could not tell the server the audio was over");
            return;
        }

        // Bounded, because the server never closes and there is therefore no
        // end of stream to wait on. A timeout here is not a fault: every sample
        // reached the server, and the transcript is whatever it managed to
        // send.
        if tokio::time::timeout(FINISH_TIMEOUT, connection.finished.notified())
            .await
            .is_err()
        {
            tracing::warn!(
                ?source,
                seconds = FINISH_TIMEOUT.as_secs(),
                "the local model server did not finish in time; keeping what it sent"
            );
        }
    }
}

/// Read one socket until it closes or the reader is aborted.
async fn read(
    mut stream: futures::stream::SplitStream<Socket>,
    source: AudioSource,
    events: Events,
    live: Option<mpsc::UnboundedSender<TranscriptEvent>>,
    started: Instant,
    fault: SessionFault,
    finished: Arc<Notify>,
) {
    let mut segments = Segments::new();
    // Where this socket's current segment lives in the shared `events`. Per
    // reader, because the two sockets interleave and "the last event" is not
    // the same thing as "the last event from this capture".
    let mut current: Option<usize> = None;
    // Whether this socket has ever produced a frame this client could parse.
    let mut understood = false;

    while let Some(frame) = stream.next().await {
        let text = match frame {
            Ok(Message::Text(text)) => text,
            Ok(Message::Close(frame)) => {
                // Never observed: the server was measured leaving the socket
                // open and waiting for the client to go. Handled anyway, and
                // as a transport fault rather than a reported one -- a close
                // frame from this server carries no reason it chose, so
                // calling it "what the provider said" would be inventing a
                // sentence.
                let said = close_reason(
                    frame.as_ref().map(|f| u16::from(f.code)),
                    frame.as_ref().map_or("", |f| f.reason.as_ref()),
                );
                tracing::warn!(?source, %said, "the local model server closed the socket");
                fault.transport(source, &said);
                return;
            }
            // Ping, pong, binary. tungstenite answers pings itself, and this
            // server was never seen to send a binary frame.
            Ok(_) => continue,
            Err(err) => {
                tracing::warn!(error = %err, ?source, "the local model socket failed");
                fault.transport(source, &err.to_string());
                return;
            }
        };

        match nemotron::interpret(&text) {
            Incoming::Finished => {
                understood = true;
                // Whatever was open is settled: the server has said it is done
                // with everything it was sent.
                if let (Some(index), Ok(mut held)) = (current, events.lock()) {
                    if let Some(event) = held.get_mut(index) {
                        event.is_final = true;
                    }
                }
                finished.notify_one();
                // Reading continues. The socket is still open -- this server
                // does not close it -- and `finish` is what tears it down.
            }
            Incoming::Update {
                segment,
                text,
                is_final,
            } => {
                understood = true;
                let step = segments.step(segment, &text);
                let elapsed = started.elapsed().as_secs_f64();

                let event = match step {
                    Step::Idle => continue,
                    Step::Begin(text) => {
                        let event = nemotron::event(text, is_final, elapsed, source);
                        if let Ok(mut held) = events.lock() {
                            current = Some(held.len());
                            held.push(event.clone());
                        }
                        event
                    }
                    Step::Revise(text) => {
                        // Replace, never append -- the rule the whole protocol
                        // turns on. `Segments` guarantees an event exists to
                        // revise; the `else` below is the belt to its braces
                        // and pushes rather than dropping the words.
                        let mut revised = None;
                        if let Ok(mut held) = events.lock() {
                            match current.and_then(|index| held.get_mut(index)) {
                                Some(event) => {
                                    event.text = text;
                                    event.is_final = is_final;
                                    revised = Some(event.clone());
                                }
                                None => {
                                    let event = nemotron::event(text, is_final, elapsed, source);
                                    current = Some(held.len());
                                    held.push(event.clone());
                                    revised = Some(event);
                                }
                            }
                        }
                        match revised {
                            Some(event) => event,
                            None => continue,
                        }
                    }
                };

                if let Some(sink) = &live {
                    // Every update, including the revisions: a live view wants
                    // the growing sentence, and a closed receiver is not a
                    // reason to stop keeping the transcript.
                    let _ = sink.send(event);
                }
            }
            Incoming::Unreadable { preview } => {
                if fault.unreadable(source, &preview, understood) {
                    tracing::warn!(
                        ?source,
                        %preview,
                        "the local model server's first frame could not be read -- ending the \
                         session"
                    );
                    return;
                }
                tracing::warn!(?source, %preview, "a frame from the local model server could not be read");
            }
        }
    }

    // The stream ended without a close frame. Transport, for the same reason
    // as the close arm above.
    tracing::warn!(?source, "the local model socket ended");
    fault.transport(source, "the local model server stopped responding");
}

#[async_trait::async_trait]
impl MeetingTranscriptionProvider for LocalStreaming {
    async fn connect(&mut self, config: MeetingProviderConfig) -> Result<(), MeetingError> {
        nemotron::prepare(&config)?;

        self.config = Some(config);
        self.started = Some(Instant::now());
        // A fresh cell per session: `SessionFault` cannot be cleared, so a
        // second meeting on a reused provider must not inherit the first's
        // verdict.
        self.fault = SessionFault::new();

        // Both, before returning. A session with only the microphone open would
        // transcribe half the meeting and report success.
        self.mic = Some(self.open(AudioSource::Mic).await?);
        self.system = Some(self.open(AudioSource::System).await?);
        self.paused = false;
        Ok(())
    }

    async fn send_audio(&mut self, chunk: AudioChunk) -> Result<(), MeetingError> {
        // First, before everything else, and testable with no socket. For the
        // cloud clients this check exists because a provider can reject a
        // session and keep the socket open; here it is the *only* way a dead
        // session is noticed at all, because a server that has gone produces a
        // send error on the next frame and nothing before it.
        if let Some(fault) = self.fault.take() {
            return Err(fault.into_error(PROVIDER_ID));
        }

        if self.paused {
            // Discarded, which is what pausing a recording means. Buffering
            // would make "pause" mean "delay".
            return Ok(());
        }

        let samples = nemotron::chunk_samples(&chunk);
        let source = chunk.source;
        let connection = self
            .connection(source)
            .ok_or_else(|| MeetingError::Disconnected {
                provider: PROVIDER_ID.to_owned(),
                detail: "the session is not open".to_owned(),
            })?;

        for frame in connection.pending.push(&samples) {
            connection
                .sink
                .send(Message::Binary(frame))
                .await
                .map_err(|err| MeetingError::Disconnected {
                    provider: PROVIDER_ID.to_owned(),
                    detail: err.to_string(),
                })?;
        }

        Ok(())
    }

    async fn pause(&mut self) -> Result<(), MeetingError> {
        // The sockets stay open. Tearing them down would lose the recogniser's
        // context mid-sentence, which is the reason `pause` exists separately
        // from `finish`.
        self.paused = true;
        Ok(())
    }

    async fn resume(&mut self) -> Result<(), MeetingError> {
        self.paused = false;
        Ok(())
    }

    async fn finish(&mut self) -> Result<MeetingTranscript, MeetingError> {
        // Read before anything is torn down, so a fault that arrived after the
        // last chunk is reported rather than lost to this client's own close.
        if let Some(fault) = self.fault.take() {
            for connection in [self.mic.take(), self.system.take()].into_iter().flatten() {
                connection.close().await;
            }
            // Rather than an empty transcript, which reaches the screen as
            // `NothingTranscribed` -- the sentence a quiet room gets.
            return Err(fault.into_error(PROVIDER_ID));
        }

        if let Some(connection) = self.mic.as_mut() {
            Self::drain_and_wait(connection, AudioSource::Mic).await;
        }
        if let Some(connection) = self.system.as_mut() {
            Self::drain_and_wait(connection, AudioSource::System).await;
        }

        for connection in [self.mic.take(), self.system.take()].into_iter().flatten() {
            connection.close().await;
        }

        // Checked again: `drain_and_wait` gives the server up to a minute to
        // finish, and a socket that died during it is a real fault that the
        // first check could not have seen.
        if let Some(fault) = self.fault.take() {
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

        Ok(nemotron::transcript(events, duration))
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

/// Never reached: `local-streaming` is in [`crate::PROVIDERS`] and a test says
/// so. Present only because this workspace forbids unwrapping in library code.
static FALLBACK: MeetingProviderCapabilities = MeetingProviderCapabilities {
    id: PROVIDER_ID,
    name: "Local streaming model",
    audio_leaves_device: false,
    keyterms: crate::KeytermSupport::Unsupported,
    session_limit: None,
    token_rotation: false,
    sample_rates: &[nemotron::SAMPLE_RATE],
    speaker_labels: false,
    partials: true,
    language_detection: false,
    reconnect: false,
    batch_fallback: true,
};

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    fn provider() -> LocalStreaming {
        // Port 9 is the discard port. Nothing here connects; the tests that
        // matter are the ones a socket cannot reach.
        LocalStreaming::new(NetworkPolicy::new().expect("a policy"), 9)
    }

    fn config() -> MeetingProviderConfig {
        MeetingProviderConfig {
            provider: PROVIDER_ID.to_owned(),
            model: None,
            language: Some("en".to_owned()),
            keyterms: Vec::new(),
            key_ref: None,
            sample_rate: nemotron::SAMPLE_RATE,
        }
    }

    #[tokio::test]
    async fn a_transport_fault_reaches_send_audio_as_an_error() {
        // The path a live socket used to be the only way to reach, and the one
        // KNEG-79 was filed about. Recorded into the session's own cell exactly
        // as the reader task would, with no socket and no server.
        let mut client = provider();
        client
            .fault
            .transport(AudioSource::Mic, "the server stopped responding");

        let err = client
            .send_audio(AudioChunk {
                pcm: vec![0; 32],
                source: AudioSource::Mic,
            })
            .await
            .expect_err("a dead session must not accept audio");

        match err {
            MeetingError::Disconnected { provider, detail } => {
                assert_eq!(provider, PROVIDER_ID);
                assert!(detail.contains("stopped responding"), "{detail}");
                assert!(detail.contains("microphone"), "which capture: {detail}");
            }
            other => panic!("{other:?}"),
        }
    }

    #[tokio::test]
    async fn a_fault_makes_finish_fail_rather_than_return_an_empty_transcript() {
        // An empty transcript becomes `NothingTranscribed`, which is the
        // sentence a quiet room gets. A session that died must not read as one.
        let mut client = provider();
        client
            .fault
            .transport(AudioSource::System, "connection reset");

        let err = client.finish().await.expect_err("a dead session must fail");
        assert!(matches!(err, MeetingError::Disconnected { .. }), "{err:?}");
    }

    #[tokio::test]
    async fn a_paused_session_discards_audio_rather_than_buffering_it() {
        let mut client = provider();
        client.pause().await.unwrap();
        // No connection is open, so reaching the connection lookup would be an
        // error. Succeeding proves the pause check came first.
        client
            .send_audio(AudioChunk {
                pcm: vec![0; 32],
                source: AudioSource::Mic,
            })
            .await
            .expect("a paused session accepts and discards");
    }

    #[tokio::test]
    async fn a_dead_session_is_reported_even_while_paused() {
        // Ordering that matters: `paused` must not swallow a fault, or a
        // meeting paused after the server died would finish looking fine.
        let mut client = provider();
        client.pause().await.unwrap();
        client.fault.transport(AudioSource::Mic, "gone");
        assert!(client
            .send_audio(AudioChunk {
                pcm: vec![0; 32],
                source: AudioSource::Mic,
            })
            .await
            .is_err());
    }

    #[tokio::test]
    async fn audio_before_connect_is_refused_rather_than_dropped() {
        let mut client = provider();
        let err = client
            .send_audio(AudioChunk {
                pcm: vec![0; 32],
                source: AudioSource::Mic,
            })
            .await
            .expect_err("there is no session to send to");
        assert!(matches!(err, MeetingError::Disconnected { .. }), "{err:?}");
    }

    #[tokio::test]
    async fn a_language_is_required_because_the_model_is_one_language() {
        // `language_detection: false` is the one capability that differs from
        // `local` in a way a user feels, and `validate` is where it bites.
        let mut config = config();
        config.language = None;
        let review = crate::validate(&config).unwrap();
        assert!(
            review.language_required,
            "a sherpa online model is built for one language"
        );
    }

    #[test]
    fn the_fallback_matches_the_table_it_stands_in_for() {
        // The fallback is unreachable, and an unreachable copy that disagreed
        // with the table would be worse than none.
        let real = crate::capabilities(PROVIDER_ID).expect("the provider is in the table");
        assert_eq!(real.id, FALLBACK.id);
        assert_eq!(real.name, FALLBACK.name);
        assert_eq!(real.audio_leaves_device, FALLBACK.audio_leaves_device);
        assert_eq!(real.sample_rates, FALLBACK.sample_rates);
        assert_eq!(real.language_detection, FALLBACK.language_detection);
        assert_eq!(real.reconnect, FALLBACK.reconnect);
        assert_eq!(real.session_limit, FALLBACK.session_limit);
    }

    #[test]
    fn the_address_is_loopback_and_the_caller_cannot_change_it() {
        // The property that lets this client claim audio never leaves the
        // machine: the address is built from a port and nothing else.
        let policy = NetworkPolicy::new().expect("a policy");
        let url = policy.loopback_websocket(8_251).expect("an address");
        assert_eq!(url.scheme(), "ws");
        assert_eq!(url.host_str(), Some("127.0.0.1"));
        assert_eq!(url.port(), Some(8_251));
    }
}
