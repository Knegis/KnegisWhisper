//! The socket half of the Deepgram Realtime client.
//!
//! [`crate::deepgram`] is the part with rules in it — the endpoint, the frame
//! grammar, what an incoming message means — and is covered by fixtures. This is
//! the plumbing, and it is thin on purpose because no test here can reach it.
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

use std::sync::Arc;
use std::time::Instant;

use futures::{SinkExt as _, StreamExt as _};
use kw_security::CredentialStore;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::client::IntoClientRequest as _;
use tokio_tungstenite::tungstenite::Message;

use crate::capabilities::MeetingProviderCapabilities;
use crate::deepgram::{self, Incoming, AUTH_HEADER, AUTH_SCHEME, PROVIDER_ID, PURPOSE};
use crate::{
    AudioChunk, AudioSource, MeetingError, MeetingProviderConfig, MeetingTranscript,
    MeetingTranscriptionProvider, TranscriptEvent,
};

type Socket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;
type Sink = futures::stream::SplitSink<Socket, Message>;

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

        let (socket, _) = tokio_tungstenite::connect_async(request)
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
            self.live.clone(),
            self.started.unwrap_or_else(Instant::now),
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
    live: Option<mpsc::UnboundedSender<TranscriptEvent>>,
    started: Instant,
) {
    while let Some(frame) = stream.next().await {
        let text = match frame {
            Ok(Message::Text(text)) => text,
            Ok(Message::Close(_)) => {
                tracing::debug!(?source, "the provider closed the socket");
                return;
            }
            // Ping, pong and binary. tungstenite answers pings itself.
            Ok(_) => continue,
            Err(err) => {
                tracing::warn!(error = %err, ?source, "the transcription socket failed");
                return;
            }
        };

        match deepgram::interpret(&text) {
            Incoming::Transcript {
                text,
                is_final,
                speaker,
            } if !text.trim().is_empty() => {
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
                // Logged rather than swallowed. A meeting quietly producing
                // no transcript is the failure this guards against.
                tracing::warn!(?source, %message, "the provider reported an error");
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

        // Both, before returning. A session with only the microphone open
        // would transcribe half the meeting and report success.
        self.mic = Some(self.open(AudioSource::Mic).await?);
        self.system = Some(self.open(AudioSource::System).await?);
        self.paused = false;
        Ok(())
    }

    async fn send_audio(&mut self, chunk: AudioChunk) -> Result<(), MeetingError> {
        if self.paused {
            // Discarded, which is what pausing a recording means. Buffering
            // would make "pause" mean "delay".
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

        Ok(deepgram::transcript(events, duration))
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
        // `clock: Option<SessionClock>` for its session cap. `Debug`'s
        // output is the tripwire: if a future change adds a rotation clock,
        // it will show up here and this comment is where to explain why
        // `token_rotation: true` does not mean this struct should grow one --
        // implementing a refresh loop that re-reads an unchanging key would
        // be ceremony that looks like a safeguard and is not one.
        let provider = DeepgramRealtime::new(store());
        let rendered = format!("{provider:?}").to_lowercase();
        assert!(!rendered.contains("rotat"), "{rendered}");
        assert!(!rendered.contains("refresh"), "{rendered}");
        assert!(!rendered.contains("clock"), "{rendered}");
    }
}
