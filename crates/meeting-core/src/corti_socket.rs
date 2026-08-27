//! The socket half of the Corti Realtime client.
//!
//! [`crate::corti`] is the part with rules in it -- the endpoints, the frame
//! grammar, what an incoming message means, the OAuth2 form shape -- and is
//! covered by fixtures. This is the plumbing: two network calls (the token
//! exchange, then the sockets) with nothing here that a fixture could reach,
//! because no key exists to make a live one.
//!
//! # Two boundary crossings
//!
//! `connect` makes an HTTPS POST through `kw_network_policy::NetworkPolicy`
//! before either socket opens -- the OAuth2 exchange -- and then opens both
//! sockets through [`kw_network_policy::validate_websocket`]. Both travel
//! under [`crate::corti::PURPOSE`]. See [`crate::corti`]'s module
//! documentation for why this client crosses the network twice where every
//! client before it crosses it once.
//!
//! # One exchange, shared by both sockets
//!
//! [`CortiRealtime::exchange_token`] runs exactly once, in `connect`, before
//! either socket opens; [`CortiRealtime::open`] takes the resulting access
//! token as a parameter rather than fetching its own. See [`crate::corti`]'s
//! module documentation for why one exchange is correct here rather than a
//! corner cut.
//!
//! # Two sockets, one per capture
//!
//! Exactly [`crate::deepgram_socket`]'s shape, for the same reason: the
//! microphone and the system output go to **separate sessions**, so
//! [`AudioSource`] on every transcript event stays a fact this client knows
//! rather than a guess. A speaker label Corti's wire might carry says who
//! *within* a stream, never *which* stream.
//!
//! # No reconnect clock
//!
//! `corti-realtime` declares `session_limit: None` in
//! [`crate::capabilities::PROVIDERS`], so there is nothing here like
//! `openai_socket::SessionClock`. A session runs until the meeting ends or a
//! socket fails on its own.

use std::sync::Arc;
use std::time::Instant;

use futures::{SinkExt as _, StreamExt as _};
use kw_network_policy::NetworkPolicy;
use kw_security::CredentialStore;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::client::IntoClientRequest as _;
use tokio_tungstenite::tungstenite::Message;

use crate::capabilities::MeetingProviderCapabilities;
use crate::corti::{self, Incoming, AUTH_HEADER, PROVIDER_ID, PURPOSE, TENANT_HEADER};
use crate::{
    AudioChunk, AudioSource, MeetingError, MeetingProviderConfig, MeetingTranscript,
    MeetingTranscriptionProvider, TranscriptEvent,
};

type Socket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;
type Sink = futures::stream::SplitSink<Socket, Message>;

/// One provider session: the OAuth2 exchange, two sockets, and everything
/// read from them.
pub struct CortiRealtime {
    credentials: Arc<dyn CredentialStore>,
    /// For the token exchange only -- the sockets go through
    /// `validate_websocket` and `tokio_tungstenite` directly, exactly like
    /// every other client here.
    policy: NetworkPolicy,
    /// Read from the batch Corti settings, not stored twice -- see
    /// `crate::corti`'s module documentation.
    environment: String,
    tenant: String,
    config: Option<MeetingProviderConfig>,
    /// Set once in `connect`, from `corti::prepare`. Carried rather than
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
        // `corti::terminate`'s doc comment on why sending it costs nothing
        // even if the server does not recognise it.
        let _ = self
            .sink
            .send(Message::Text(corti::terminate().to_string()))
            .await;
        let _ = self.sink.send(Message::Close(None)).await;
        let _ = self.sink.close().await;
        self.reader.abort();
    }
}

impl std::fmt::Debug for CortiRealtime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CortiRealtime")
            .field("mic", &self.mic.is_some())
            .field("system", &self.system.is_some())
            .finish_non_exhaustive()
    }
}

impl CortiRealtime {
    #[must_use]
    pub fn new(
        credentials: Arc<dyn CredentialStore>,
        policy: NetworkPolicy,
        environment: String,
        tenant: String,
    ) -> Self {
        Self {
            credentials,
            policy,
            environment,
            tenant,
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

    /// Exchange the stored client secret for an access token.
    ///
    /// Called once, from `connect`, before either socket opens -- see the
    /// module documentation. The client secret is resolved first and never
    /// bound to a variable that outlives this call; it is exposed exactly
    /// once, into the request body `corti::token_request` builds.
    async fn exchange_token(
        &self,
        config: &MeetingProviderConfig,
    ) -> Result<kw_security::Secret, MeetingError> {
        // Validate the destination before resolving the credential, so even a
        // directly constructed client cannot expose it through a crafted host.
        let endpoint = corti::token_endpoint(&self.environment)?;
        let secret = corti::client_secret(&self.credentials, config.key_ref.as_ref())?;
        let form = corti::token_request(&self.tenant, &secret);
        // The first of this client's two boundary crossings: an HTTPS POST
        // through `NetworkPolicy`, which validates `endpoint` against
        // `PURPOSE` before anything is sent.
        let response = self
            .policy
            .post(PURPOSE, &endpoint)
            .map_err(|err| MeetingError::Rejected {
                provider: PROVIDER_ID.to_owned(),
                detail: err.to_string(),
            })?
            .header("Content-Type", "application/x-www-form-urlencoded")
            // `expose` at the last possible moment, straight into the request
            // body with no intermediate binding that could hold the client
            // secret a moment longer than necessary.
            .body(form.expose().to_owned())
            .send()
            .await
            .map_err(|err| MeetingError::Rejected {
                provider: PROVIDER_ID.to_owned(),
                detail: err.to_string(),
            })?;

        let status = response.status().as_u16();
        let body = response
            .text()
            .await
            .map_err(|err| MeetingError::Rejected {
                provider: PROVIDER_ID.to_owned(),
                detail: err.to_string(),
            })?;

        // The access token this returns is a secret too -- see
        // `crate::corti`'s module documentation. `interpret_token`'s own
        // error never carries it, only the token endpoint's response body on
        // a failure status, exactly as it already behaves for batch
        // transcription.
        kw_transcription_core::clients::interpret_token(status, &body).map_err(|err| {
            MeetingError::Rejected {
                provider: PROVIDER_ID.to_owned(),
                detail: err.to_string(),
            }
        })
    }

    /// Open one socket for `source`, carrying an access token exchanged once
    /// for the whole session.
    async fn open(
        &self,
        source: AudioSource,
        access_token: &kw_security::Secret,
    ) -> Result<Connection, MeetingError> {
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

        let endpoint = corti::endpoint(&self.environment)?;

        // The second boundary crossing: the socket itself, every time one is
        // opened -- not once at startup. A blocked host must not become
        // reachable by reconnecting.
        let url = kw_network_policy::validate_websocket(&endpoint, PURPOSE).map_err(|err| {
            MeetingError::Rejected {
                provider: PROVIDER_ID.to_owned(),
                detail: err.to_string(),
            }
        })?;

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
                format!("Bearer {}", access_token.expose())
                    .parse()
                    .map_err(|_| MeetingError::Rejected {
                        provider: PROVIDER_ID.to_owned(),
                        detail: "the access token is not a valid header value".to_owned(),
                    })?,
            );
            // The tenant is not a secret, but it is still resolved from this
            // struct's own field rather than from anything the caller passed
            // in per-socket, matching the "read once, shared by both" shape
            // the access token follows.
            headers.insert(
                TENANT_HEADER,
                self.tenant.parse().map_err(|_| MeetingError::Rejected {
                    provider: PROVIDER_ID.to_owned(),
                    detail: "the stored tenant is not a valid header value".to_owned(),
                })?,
            );
        }

        let (socket, _) = tokio_tungstenite::connect_async(request)
            .await
            .map_err(|err| MeetingError::Rejected {
                provider: PROVIDER_ID.to_owned(),
                // The handshake failure, which for a bad token is worth
                // repeating verbatim -- it is the difference between "your
                // access token is wrong" and "the network is down".
                detail: err.to_string(),
            })?;

        let (mut sink, stream) = socket.split();

        // The session configuration -- audio format, the primary language,
        // and keyterms -- travels in a message after the handshake, not the
        // query string. See `corti::session_config`'s doc comment.
        let config_frame = corti::session_config(
            config.sample_rate,
            config.language.as_deref(),
            &config.keyterms,
            caps,
        );
        sink.send(Message::Text(config_frame.to_string()))
            .await
            .map_err(|err| MeetingError::Rejected {
                provider: PROVIDER_ID.to_owned(),
                detail: err.to_string(),
            })?;

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

        match corti::interpret(&text) {
            Incoming::Transcript {
                text,
                is_final,
                speaker,
            } if !text.trim().is_empty() => {
                let event = corti::event(
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
impl MeetingTranscriptionProvider for CortiRealtime {
    async fn connect(&mut self, config: MeetingProviderConfig) -> Result<(), MeetingError> {
        let caps = corti::prepare(&config)?;
        self.caps = Some(caps);

        // Exchanged once, before either socket opens, and shared by both --
        // see the module documentation on why this is correct rather than a
        // corner cut.
        let access_token = self.exchange_token(&config).await?;

        self.config = Some(config);
        self.started = Some(Instant::now());

        // Both, before returning. A session with only the microphone open
        // would transcribe half the meeting and report success.
        self.mic = Some(self.open(AudioSource::Mic, &access_token).await?);
        self.system = Some(self.open(AudioSource::System, &access_token).await?);
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
        // `corti::frame_bytes`'s doc comment.
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

        Ok(corti::transcript(events, duration))
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

/// Never reached: `corti-realtime` is in [`crate::PROVIDERS`] and a test says
/// so. Present only because this workspace forbids unwrapping in library
/// code.
static FALLBACK: MeetingProviderCapabilities = MeetingProviderCapabilities {
    id: PROVIDER_ID,
    name: "Corti Realtime",
    audio_leaves_device: true,
    keyterms: crate::KeytermSupport::Supported {
        max: None,
        wire: crate::KeytermWire::SessionConfig,
    },
    session_limit: None,
    token_rotation: false,
    sample_rates: &[16_000],
    speaker_labels: true,
    partials: true,
    language_detection: false,
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

    fn provider() -> CortiRealtime {
        CortiRealtime::new(
            store(),
            NetworkPolicy::new().unwrap(),
            "eu".to_owned(),
            "acme".to_owned(),
        )
    }

    fn config() -> MeetingProviderConfig {
        MeetingProviderConfig {
            provider: PROVIDER_ID.to_owned(),
            model: None,
            language: Some("en".to_owned()),
            keyterms: Vec::new(),
            key_ref: kw_security::KeyRef::new("corti"),
            sample_rate: 16_000,
        }
    }

    #[tokio::test]
    async fn a_missing_credential_is_refused_before_any_socket_is_opened() {
        // The order matters: a handshake attempted with no secret would spend
        // a round trip only to fail, and would report a network failure
        // rather than a missing credential. Here it is worse than that for
        // the sibling clients: it would also have spent the token exchange's
        // round trip on a request with no client secret.
        let mut provider = provider();
        let err = provider.connect(config()).await.unwrap_err();
        assert!(
            matches!(err, MeetingError::MissingCredential { .. }),
            "{err:?}"
        );
    }

    #[tokio::test]
    async fn the_wrong_sample_rate_is_refused_before_the_credential_is_read() {
        let mut provider = provider();
        let mut cfg = config();
        cfg.sample_rate = 48_000;

        let err = provider.connect(cfg).await.unwrap_err();
        assert!(matches!(err, MeetingError::SampleRate { .. }), "{err:?}");
    }

    #[tokio::test]
    async fn audio_before_connect_is_refused_rather_than_buffered() {
        let mut provider = provider();
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
        let mut provider = provider();
        let transcript = provider.finish().await.unwrap();
        assert!(transcript.events.is_empty());
        assert_eq!(transcript.reconnects, 0);
        assert_eq!(transcript.gaps, 0);
    }

    #[test]
    fn the_declared_capabilities_are_the_tables() {
        let provider = provider();
        assert_eq!(provider.capabilities().id, PROVIDER_ID);
        assert_eq!(provider.capabilities().sample_rates, &[16_000]);
        assert!(provider.capabilities().speaker_labels);
        assert!(
            !provider.capabilities().token_rotation,
            "corti is not the provider that rotates tokens -- deepgram is"
        );
    }
}
