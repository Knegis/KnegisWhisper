//! The socket half of the AssemblyAI Realtime client.
//!
//! [`crate::assemblyai`] is the part with rules in it — the endpoint, the
//! frame grammar, what an incoming message means — and is covered by
//! fixtures. This is the plumbing, and it is thin on purpose because almost no
//! test here can reach it. The exceptions arrived with [`crate::fault`]: a fault
//! can be recorded into the session's own cell exactly as the reader task would,
//! with no socket, no key and no network, and `send_audio` and `finish` can then
//! be asked what they do about it.
//!
//! # Two sockets, one per capture
//!
//! Exactly [`crate::openai_socket`]'s shape, for the same reason: the
//! microphone and the system output go to **separate sessions**, so
//! [`AudioSource`] on every transcript event stays a fact this client knows
//! rather than a guess. AssemblyAI's speaker labels — when the wire carries
//! one; see [`crate::assemblyai`]'s module documentation — say who *within* a
//! stream, never *which* stream, so they add information on top of `source`
//! rather than replacing the need for two connections. This overrides an
//! earlier plan that argued one socket would do.
//!
//! # No reconnect clock
//!
//! `assemblyai-realtime` declares `session_limit: None` in
//! [`crate::capabilities::PROVIDERS`], so there is nothing here like
//! [`crate::openai::SessionClock`]. A session runs until the meeting ends or a
//! socket fails on its own; nothing in this client schedules a rebuild.

use std::sync::Arc;
use std::time::Instant;

use futures::{SinkExt as _, StreamExt as _};
use kw_security::CredentialStore;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::client::IntoClientRequest as _;
use tokio_tungstenite::tungstenite::Message;

use crate::assemblyai::{self, Incoming, AUTH_HEADER, PROVIDER_ID, PURPOSE};
use crate::capabilities::MeetingProviderCapabilities;
use crate::fault::{close_reason, SessionFault};
use crate::{
    AudioChunk, AudioSource, MeetingError, MeetingProviderConfig, MeetingTranscript,
    MeetingTranscriptionProvider, TranscriptEvent,
};

type Socket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;
type Sink = futures::stream::SplitSink<Socket, Message>;

/// One provider session: two sockets, and everything read from them.
pub struct AssemblyAiRealtime {
    credentials: Arc<dyn CredentialStore>,
    config: Option<MeetingProviderConfig>,
    /// Set once in `connect`, from `assemblyai::prepare`. Carried rather than
    /// re-looked-up so `open` never has to unwrap a lookup that could not fail
    /// in practice but is not provably infallible to the compiler.
    caps: Option<&'static MeetingProviderCapabilities>,
    /// One connection per capture. `None` until `connect`.
    mic: Option<Connection>,
    system: Option<Connection>,
    /// Everything both sockets have said, in arrival order.
    events: Arc<std::sync::Mutex<Vec<TranscriptEvent>>>,
    /// Why the session is over, if it is. One cell for both sockets — see
    /// [`SessionFault`].
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
        // `assemblyai::terminate`'s doc comment on why sending it costs
        // nothing even if the server does not recognise it.
        let _ = self
            .sink
            .send(Message::Text(assemblyai::terminate().to_string()))
            .await;
        let _ = self.sink.send(Message::Close(None)).await;
        let _ = self.sink.close().await;
        self.reader.abort();
    }
}

impl std::fmt::Debug for AssemblyAiRealtime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AssemblyAiRealtime")
            .field("mic", &self.mic.is_some())
            .field("system", &self.system.is_some())
            .finish_non_exhaustive()
    }
}

impl AssemblyAiRealtime {
    #[must_use]
    pub fn new(credentials: Arc<dyn CredentialStore>) -> Self {
        Self {
            credentials,
            config: None,
            caps: None,
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

        let endpoint = assemblyai::endpoint(
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

        let secret = assemblyai::token(&self.credentials, config.key_ref.as_ref())?;

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
            headers.insert(
                AUTH_HEADER,
                secret
                    .expose()
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
                // repeating verbatim -- it is the difference between "your key
                // is wrong" and "the network is down".
                detail: err.to_string(),
            })?;

        let (sink, stream) = socket.split();

        // Nothing is sent after the handshake: every piece of session
        // configuration travelled in the query string. See
        // `assemblyai::endpoint`'s doc comment.
        let reader = tokio::spawn(read(
            stream,
            source,
            Arc::clone(&self.events),
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

        let incoming = assemblyai::interpret(&text);
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
                let event = assemblyai::event(
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
            // An empty transcript mid-turn, which is most frames.
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
impl MeetingTranscriptionProvider for AssemblyAiRealtime {
    async fn connect(&mut self, config: MeetingProviderConfig) -> Result<(), MeetingError> {
        let caps = assemblyai::prepare(&config)?;

        self.caps = Some(caps);
        self.config = Some(config);
        self.started = Some(Instant::now());
        // A fresh cell per session. `SessionFault` cannot be cleared -- an
        // `OnceLock` behind an `Arc` has no way to be unset -- so a second
        // meeting on a reused provider gets its own rather than inheriting the
        // first one's verdict.
        self.fault = SessionFault::new();
        // A fresh cell per session. `SessionFault` cannot be cleared -- an
        // `OnceLock` behind an `Arc` has no way to be unset -- so a second
        // meeting on a reused provider gets its own rather than inheriting the
        // first one's verdict.
        self.fault = SessionFault::new();
        // A fresh cell per session. `SessionFault` cannot be cleared -- an
        // `OnceLock` behind an `Arc` has no way to be unset -- so a second
        // meeting on a reused provider gets its own rather than inheriting the
        // first one's verdict.
        self.fault = SessionFault::new();

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
        // `assemblyai::frame_bytes`'s doc comment.
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

        Ok(assemblyai::transcript(events, duration))
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

/// Never reached: `assemblyai-realtime` is in [`crate::PROVIDERS`] and a test
/// says so. Present only because this workspace forbids unwrapping in library
/// code.
static FALLBACK: MeetingProviderCapabilities = MeetingProviderCapabilities {
    id: PROVIDER_ID,
    name: "AssemblyAI Realtime",
    audio_leaves_device: true,
    keyterms: crate::KeytermSupport::Supported {
        max: Some(100),
        wire: crate::KeytermWire::JsonQueryParam,
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

    fn config() -> MeetingProviderConfig {
        MeetingProviderConfig {
            provider: PROVIDER_ID.to_owned(),
            model: None,
            language: Some("en".to_owned()),
            keyterms: Vec::new(),
            key_ref: kw_security::KeyRef::new("assemblyai"),
            sample_rate: 16_000,
        }
    }

    #[tokio::test]
    async fn a_missing_credential_is_refused_before_any_socket_is_opened() {
        // The order matters: a handshake attempted with no key would spend a
        // round trip only to fail, and would report a network failure rather
        // than a missing key.
        let mut provider = AssemblyAiRealtime::new(store());
        let err = provider.connect(config()).await.unwrap_err();
        assert!(
            matches!(err, MeetingError::MissingCredential { .. }),
            "{err:?}"
        );
    }

    #[tokio::test]
    async fn the_wrong_sample_rate_is_refused_before_the_credential_is_read() {
        let mut provider = AssemblyAiRealtime::new(store());
        let mut cfg = config();
        cfg.sample_rate = 48_000;

        let err = provider.connect(cfg).await.unwrap_err();
        assert!(matches!(err, MeetingError::SampleRate { .. }), "{err:?}");
    }

    #[tokio::test]
    async fn audio_before_connect_is_refused_rather_than_buffered() {
        let mut provider = AssemblyAiRealtime::new(store());
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
        let mut provider = AssemblyAiRealtime::new(store());
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
        let mut provider = AssemblyAiRealtime::new(store());
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
        let mut provider = AssemblyAiRealtime::new(store());
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
        let provider = AssemblyAiRealtime::new(store());
        assert_eq!(provider.capabilities().id, PROVIDER_ID);
        assert_eq!(provider.capabilities().sample_rates, &[16_000]);
        assert!(provider.capabilities().speaker_labels);
    }
}
