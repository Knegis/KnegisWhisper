//! The socket half of the OpenAI Realtime client.
//!
//! [`crate::openai`] is the part with rules in it — what to send, what a frame
//! means, when to reconnect — and is covered by fixtures. This is the plumbing,
//! and it is thin on purpose because no test here can reach it.
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
type Closed = Arc<std::sync::Mutex<Option<String>>>;

/// One open socket and the task reading it.
struct Connection {
    sink: Sink,
    reader: tokio::task::JoinHandle<()>,
    /// Set by the reader if the provider closed the socket first.
    closed: Closed,
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

impl Drop for Connection {
    fn drop(&mut self) {
        // Dropping a JoinHandle detaches its task. The reader owns the stream
        // half, so detaching it would also keep the socket alive when a caller
        // is aborted before it can await `close`.
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
        let update = openai::session_update(model, config.language.as_deref());
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

        let mic = self.open(AudioSource::Mic).await?;
        let system = match self.open(AudioSource::System).await {
            Ok(system) => system,
            Err(err) => {
                mic.close().await;
                return Err(err);
            }
        };
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
) {
    while let Some(frame) = stream.next().await {
        let text = match frame {
            Ok(Message::Text(text)) => text,
            Ok(Message::Close(frame)) => {
                // The code and reason are the useful part and used to be
                // discarded. `warn`, not `debug`: the provider ending a session
                // we did not end is worth seeing without turning logging up.
                let said = frame.map_or_else(
                    || "the provider closed the connection without saying why".to_owned(),
                    |f| {
                        let reason = f.reason.trim();
                        if reason.is_empty() {
                            format!("the provider closed the connection (code {})", f.code)
                        } else {
                            format!(
                                "the provider closed the connection: {reason} (code {})",
                                f.code
                            )
                        }
                    },
                );
                tracing::warn!(?source, %said, "the transcription socket was closed");
                if let Ok(mut held) = closed.lock() {
                    *held = Some(said);
                }
                return;
            }
            // Ping, pong and binary. tungstenite answers pings itself.
            Ok(_) => continue,
            Err(err) => {
                tracing::warn!(error = %err, ?source, "the transcription socket failed");
                if let Ok(mut held) = closed.lock() {
                    *held = Some(err.to_string());
                }
                return;
            }
        };

        match openai::interpret(&text) {
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
                // Logged rather than swallowed. A meeting quietly producing no
                // transcript is the failure this guards against.
                tracing::warn!(?source, %message, "the provider reported an error");
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

        // Both, before returning. A session with only the microphone open would
        // transcribe half the meeting and report success.
        let mic = self.open(AudioSource::Mic).await?;
        let system = match self.open(AudioSource::System).await {
            Ok(system) => system,
            Err(err) => {
                mic.close().await;
                return Err(err);
            }
        };
        self.mic = Some(mic);
        self.system = Some(system);
        self.paused = false;
        Ok(())
    }

    async fn send_audio(&mut self, chunk: AudioChunk) -> Result<(), MeetingError> {
        if self.paused {
            // Discarded, which is what pausing a recording means. Buffering would
            // make "pause" mean "delay".
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
        for connection in [self.mic.take(), self.system.take()].into_iter().flatten() {
            connection.close().await;
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

    #[test]
    fn the_declared_capabilities_are_the_tables() {
        let provider = OpenAiRealtime::new(store());
        assert_eq!(provider.capabilities().id, PROVIDER_ID);
        assert_eq!(provider.capabilities().sample_rates, &[24_000]);
    }
}
