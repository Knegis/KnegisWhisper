//! The one-shot loopback redirect receiver.
//!
//! A public client with no server component has nowhere to receive an OAuth
//! redirect except itself. This binds an OS-assigned port on `127.0.0.1`,
//! accepts exactly one connection -- the browser, after the user approves or
//! denies access -- reads the request line it sent, answers with one static
//! HTML page, and closes. That is the whole job, and it is deliberately not
//! served by a router: `axum` lives in this workspace, but pulling it in for
//! a single throwaway response would be more surface than the job, and every
//! extra route would be one more thing that could end up echoing a query
//! parameter back into the page.
//!
//! Binds `127.0.0.1` only, and only that -- never `0.0.0.0`. The entire point
//! of the loopback redirect flow (RFC 8252) is that nothing outside this
//! machine can ever complete it.

use std::time::Duration;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::time::timeout;

/// Bound on the best-effort courtesy-page write, once the callback has
/// already been parsed. Independent of the caller's own `wait` deadline
/// (`LoopbackReceiver::receive`) -- see that function's doc for why the two
/// must not be the same budget. Generous for a page under 200 bytes on
/// loopback; short enough that a peer holding the connection open without
/// reading cannot delay returning the already-secured result for long.
const RESPONSE_WRITE_BUDGET: Duration = Duration::from_secs(2);

/// Upper bound on how much of the request line this will buffer before
/// giving up. The only client that can reach this listener is the user's own
/// browser, but the bound is cheap insurance against a malformed connection
/// (or a port collision with something unexpected) tying up memory instead of
/// failing promptly.
const MAX_REQUEST_LINE_BYTES: usize = 8 * 1024;
const MAX_HEADER_BYTES: usize = 16 * 1024;

/// The page shown after the browser lands here. Static on purpose: nothing
/// from the request -- not the code, not the state, not any `error` the
/// provider sent -- is ever echoed back into it.
const RESPONSE_BODY: &str = "<!doctype html>\n<html><head><meta charset=\"utf-8\"><title>KnegisWhisper</title></head>\n<body><p>You can close this window and return to KnegisWhisper.</p></body></html>\n";

/// Why the redirect could not be received.
#[derive(Debug, thiserror::Error)]
pub enum RedirectError {
    #[error("could not bind the loopback listener: {0}")]
    Bind(#[source] std::io::Error),

    #[error("could not read the bound port: {0}")]
    LocalAddr(#[source] std::io::Error),

    #[error("timed out waiting for the browser to redirect back")]
    TimedOut,

    #[error("could not accept the connection: {0}")]
    Accept(#[source] std::io::Error),

    #[error("could not read the request: {0}")]
    Read(#[source] std::io::Error),

    #[error("the request did not carry a request line this server understands")]
    MalformedRequest,

    #[error("could not write the response: {0}")]
    Write(#[source] std::io::Error),
}

/// What the provider's redirect carried in its query string.
#[derive(Clone, PartialEq, Eq)]
pub struct CallbackParams {
    pub code: Option<String>,
    pub state: Option<String>,
    /// The OAuth2 `error` parameter -- set when the user denied consent, or
    /// the provider otherwise refused to issue a code.
    pub error: Option<String>,
    /// Retained only in Rust; never echoed to the callback page or diagnostics.
    pub error_description: Option<String>,
}

impl std::fmt::Debug for CallbackParams {
    /// `code` and `state` are shown only as present/absent. The code is
    /// exactly the kind of value the brief says must never be logged, and
    /// printing it through a struct that happens to derive `Debug` is the
    /// accident this guards against.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CallbackParams")
            .field("code", &self.code.as_ref().map(|_| "<redacted>"))
            .field("state", &self.state.as_ref().map(|_| "<redacted>"))
            .field("error", &self.error)
            .field("error_description", &self.error_description.is_some())
            .finish()
    }
}

/// A bound, not-yet-listening loopback receiver.
pub struct LoopbackReceiver {
    listener: TcpListener,
    port: u16,
}

impl std::fmt::Debug for LoopbackReceiver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LoopbackReceiver")
            .field("port", &self.port)
            .finish()
    }
}

impl LoopbackReceiver {
    /// Bind an ephemeral loopback port.
    ///
    /// The address is a literal, not configuration, so there is no path by
    /// which this could end up bound to `0.0.0.0`.
    pub async fn bind() -> Result<Self, RedirectError> {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .map_err(RedirectError::Bind)?;
        let port = listener
            .local_addr()
            .map_err(RedirectError::LocalAddr)?
            .port();
        Ok(Self { listener, port })
    }

    #[must_use]
    pub const fn port(&self) -> u16 {
        self.port
    }

    /// The `redirect_uri` to register with the provider and send in the
    /// authorize request, given the callback path this instance will accept.
    #[must_use]
    pub fn redirect_uri(&self, path: &str) -> String {
        format!("http://127.0.0.1:{}{path}", self.port)
    }

    /// Accept exactly one connection, extract what it carried, and return it.
    ///
    /// `wait` is a single deadline over accepting the connection, reading the
    /// request line, and parsing it -- that is where the unbounded hang this
    /// type exists to avoid actually lived. A connection that is accepted and
    /// then stalls before sending anything (a hung browser, or any other
    /// local process that wins the race to this port) is exactly as much of a
    /// hang as nobody connecting at all: the listener is one-shot, its single
    /// connection slot is already spent, and there is no recovery short of
    /// restarting the application. Two independent timeouts here -- one for
    /// `accept`, a separate one for the read -- would still let a stalled
    /// read consume its own full allowance on top of whatever the accept wait
    /// already used, so this is one `timeout` around accept-read-parse.
    ///
    /// Once the callback parses, the authorization code (if any) is already
    /// obtained, and nothing after that point may take it away: the courtesy
    /// page is written back on a *separate*, best-effort budget
    /// ([`RESPONSE_WRITE_BUDGET`]), and this returns `Ok` regardless of
    /// whether that write succeeds, times out, or errors. A browser tab left
    /// showing a blank page instead of "you can close this window" is
    /// cosmetic; reporting a completed handshake as a timeout because the
    /// courtesy page was slow to send is not.
    pub async fn receive(self, wait: Duration) -> Result<CallbackParams, RedirectError> {
        let listener = self.listener;
        let (mut stream, params) = timeout(wait, accept_and_parse(listener))
            .await
            .map_err(|_elapsed| RedirectError::TimedOut)??;

        let _ = timeout(RESPONSE_WRITE_BUDGET, async {
            // Closing with unread HTTP headers can reset the connection on
            // macOS. Drain only the bounded header section, never a body.
            read_headers(&mut stream).await?;
            write_courtesy_page(&mut stream).await
        })
        .await;

        Ok(params)
    }
}

/// Consume headers without retaining their potentially sensitive contents.
async fn read_headers(stream: &mut (impl AsyncRead + Unpin)) -> Result<(), RedirectError> {
    let mut line_bytes = 0;
    for _ in 0..MAX_HEADER_BYTES {
        let byte = stream.read_u8().await.map_err(RedirectError::Read)?;
        if byte == b'\n' {
            if line_bytes == 0 {
                return Ok(());
            }
            line_bytes = 0;
        } else if byte != b'\r' {
            line_bytes += 1;
        }
    }
    Err(RedirectError::MalformedRequest)
}

/// Accept one connection, read its request line, and parse the callback --
/// the part of [`LoopbackReceiver::receive`] that sits under its single
/// deadline.
async fn accept_and_parse(
    listener: TcpListener,
) -> Result<(TcpStream, CallbackParams), RedirectError> {
    let (mut stream, _addr) = listener.accept().await.map_err(RedirectError::Accept)?;
    let line = read_request_line(&mut stream).await?;
    let params = parse_callback(&line)?;
    Ok((stream, params))
}

/// Write the static courtesy page, best-effort. Called only after the
/// caller's result is already secured, so its own errors are for the caller
/// of *this* function to decide whether to ignore -- and
/// [`LoopbackReceiver::receive`] does.
async fn write_courtesy_page(stream: &mut TcpStream) -> Result<(), RedirectError> {
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        RESPONSE_BODY.len(),
        RESPONSE_BODY
    );
    stream
        .write_all(response.as_bytes())
        .await
        .map_err(RedirectError::Write)?;
    stream.flush().await.map_err(RedirectError::Write)?;
    let _ = stream.shutdown().await;
    Ok(())
}

/// Read up to the first `\n`, never buffering more than
/// [`MAX_REQUEST_LINE_BYTES`].
async fn read_request_line(stream: &mut (impl AsyncRead + Unpin)) -> Result<String, RedirectError> {
    let mut buf = Vec::new();
    loop {
        let byte = match stream.read_u8().await {
            Ok(byte) => byte,
            Err(err) if err.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(err) => return Err(RedirectError::Read(err)),
        };
        if byte == b'\n' {
            break;
        }
        buf.push(byte);
        if buf.len() > MAX_REQUEST_LINE_BYTES {
            return Err(RedirectError::MalformedRequest);
        }
    }
    if buf.last() == Some(&b'\r') {
        buf.pop();
    }
    String::from_utf8(buf).map_err(|_utf8_error| RedirectError::MalformedRequest)
}

/// Parse `GET /path?code=...&state=... HTTP/1.1` into its query parameters.
fn parse_callback(request_line: &str) -> Result<CallbackParams, RedirectError> {
    let target = request_line
        .split_whitespace()
        .nth(1)
        .ok_or(RedirectError::MalformedRequest)?;

    // A fixed, local base: the target is a request path, never a full URL,
    // so nothing about where this resolves is influenced by the request.
    let base =
        url::Url::parse("http://127.0.0.1").map_err(|_err| RedirectError::MalformedRequest)?;
    let url = base
        .join(target)
        .map_err(|_err| RedirectError::MalformedRequest)?;

    let mut code = None;
    let mut state = None;
    let mut error = None;
    let mut error_description = None;
    for (key, value) in url.query_pairs() {
        match key.as_ref() {
            "code" => code = Some(value.into_owned()),
            "state" => state = Some(value.into_owned()),
            "error" => error = Some(value.into_owned()),
            "error_description" => error_description = Some(value.into_owned()),
            _ => {}
        }
    }

    Ok(CallbackParams {
        code,
        state,
        error,
        error_description,
    })
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

    #[test]
    fn callback_params_never_show_the_code_or_state_in_debug_output() {
        let params = CallbackParams {
            code: Some("super-secret-code".to_owned()),
            state: Some("the-state-value".to_owned()),
            error: None,
            error_description: Some("private description".to_owned()),
        };
        let rendered = format!("{params:?}");
        assert!(!rendered.contains("super-secret-code"));
        assert!(!rendered.contains("the-state-value"));
        assert!(rendered.contains("redacted"));
    }

    #[test]
    fn parses_code_and_state_from_a_realistic_request_line() {
        let params = parse_callback("GET /callback?code=abc123&state=xyz789 HTTP/1.1").unwrap();
        assert_eq!(params.code, Some("abc123".to_owned()));
        assert_eq!(params.state, Some("xyz789".to_owned()));
        assert_eq!(params.error, None);
    }

    #[test]
    fn parses_an_authorization_denial() {
        let params =
            parse_callback("GET /callback?error=access_denied&state=xyz HTTP/1.1").unwrap();
        assert_eq!(params.error, Some("access_denied".to_owned()));
        assert_eq!(params.code, None);
    }

    #[test]
    fn a_request_with_no_query_string_parses_to_all_none() {
        let params = parse_callback("GET /callback HTTP/1.1").unwrap();
        assert_eq!(params.code, None);
        assert_eq!(params.state, None);
        assert_eq!(params.error, None);
    }

    #[test]
    fn a_request_line_with_no_target_is_malformed() {
        assert!(matches!(
            parse_callback("GET"),
            Err(RedirectError::MalformedRequest)
        ));
        assert!(matches!(
            parse_callback(""),
            Err(RedirectError::MalformedRequest)
        ));
    }

    #[tokio::test]
    async fn binds_loopback_and_reports_a_nonzero_port() {
        let receiver = LoopbackReceiver::bind().await.unwrap();
        assert_ne!(receiver.port(), 0);
        assert_eq!(
            receiver.redirect_uri("/callback"),
            format!("http://127.0.0.1:{}/callback", receiver.port())
        );
    }

    #[tokio::test]
    async fn receives_exactly_one_real_connection_and_answers_it() {
        let receiver = LoopbackReceiver::bind().await.unwrap();
        let port = receiver.port();

        let client = tokio::spawn(async move {
            let mut stream = tokio::net::TcpStream::connect(format!("127.0.0.1:{port}"))
                .await
                .unwrap();
            stream
                .write_all(b"GET /callback?code=the-code&state=the-state HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n")
                .await
                .unwrap();
            let mut body = Vec::new();
            stream.read_to_end(&mut body).await.unwrap();
            String::from_utf8(body).unwrap()
        });

        let params = receiver.receive(Duration::from_secs(5)).await.unwrap();
        assert_eq!(params.code, Some("the-code".to_owned()));
        assert_eq!(params.state, Some("the-state".to_owned()));

        let response = client.await.unwrap();
        assert!(response.starts_with("HTTP/1.1 200"));
        assert!(response.contains("close this window"));
        let (headers, body) = response.split_once("\r\n\r\n").unwrap();
        assert!(headers.contains(&format!("Content-Length: {}", body.len())));
        assert_eq!(body, RESPONSE_BODY);
    }

    #[tokio::test]
    async fn a_browser_that_never_arrives_times_out_rather_than_hanging() {
        let receiver = LoopbackReceiver::bind().await.unwrap();
        let err = receiver
            .receive(Duration::from_millis(50))
            .await
            .unwrap_err();
        assert!(matches!(err, RedirectError::TimedOut));
    }

    #[tokio::test]
    async fn a_connection_that_stalls_after_accepting_times_out_too() {
        // The case a deadline on `accept` alone does not cover: something
        // wins the race to the loopback port, connects, and then never sends
        // a byte. The listener's single connection slot is already spent at
        // that point, so if the read had no deadline of its own this would
        // hang forever rather than time out.
        let receiver = LoopbackReceiver::bind().await.unwrap();
        let port = receiver.port();

        // Holds the connection open, sending nothing, for well longer than
        // the receiver's deadline below.
        let stalled_client = tokio::spawn(async move {
            let stream = tokio::net::TcpStream::connect(format!("127.0.0.1:{port}"))
                .await
                .unwrap();
            tokio::time::sleep(Duration::from_millis(300)).await;
            drop(stream);
        });

        let started = std::time::Instant::now();
        let err = receiver
            .receive(Duration::from_millis(50))
            .await
            .unwrap_err();
        assert!(matches!(err, RedirectError::TimedOut));
        assert!(
            started.elapsed() < Duration::from_millis(250),
            "the deadline must bound the stalled read, not just the wait for a connection: took {:?}",
            started.elapsed()
        );

        stalled_client.await.unwrap();
    }

    #[tokio::test]
    async fn a_valid_callback_is_not_lost_when_the_client_never_reads_the_response() {
        // The regression this guards against: if returning `Ok(params)` were
        // gated on the courtesy-page write succeeding, a client that never
        // reads the response could turn a completed handshake into a
        // reported timeout, discarding a valid authorization code -- a far
        // worse outcome than the browser tab showing a blank page.
        let receiver = LoopbackReceiver::bind().await.unwrap();
        let port = receiver.port();

        let client = tokio::spawn(async move {
            let mut stream = tokio::net::TcpStream::connect(format!("127.0.0.1:{port}"))
                .await
                .unwrap();
            stream
                .write_all(b"GET /callback?code=the-code&state=the-state HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n")
                .await
                .unwrap();
            // Deliberately never reads the response, and holds the
            // connection open well past when the server has already parsed
            // the callback and moved on to (or past) the courtesy write.
            tokio::time::sleep(Duration::from_millis(200)).await;
        });

        let params = receiver.receive(Duration::from_secs(5)).await.unwrap();
        assert_eq!(params.code, Some("the-code".to_owned()));
        assert_eq!(params.state, Some("the-state".to_owned()));

        client.await.unwrap();
    }
    #[tokio::test]
    async fn headers_are_bounded_and_may_arrive_in_fragments() {
        let (mut writer, mut reader) = tokio::io::duplex(64);
        let task = tokio::spawn(async move {
            writer.write_all(b"Host: local\r").await.unwrap();
            tokio::task::yield_now().await;
            writer.write_all(b"\nX-Test: yes\r\n\r\n").await.unwrap();
        });
        read_headers(&mut reader).await.unwrap();
        task.await.unwrap();
        let bytes = vec![b'x'; MAX_HEADER_BYTES];
        assert!(matches!(
            read_headers(&mut bytes.as_slice()).await,
            Err(RedirectError::MalformedRequest)
        ));
        assert!(matches!(
            read_headers(&mut b"Host: local".as_slice()).await,
            Err(RedirectError::Read(_))
        ));
    }

    #[tokio::test]
    async fn a_parsed_callback_survives_incomplete_or_oversized_headers() {
        for suffix in [Vec::new(), vec![b'x'; MAX_HEADER_BYTES + 1]] {
            let receiver = LoopbackReceiver::bind().await.unwrap();
            let mut client = TcpStream::connect(("127.0.0.1", receiver.port()))
                .await
                .unwrap();
            client
                .write_all(b"GET /callback?code=saved HTTP/1.1\r\n")
                .await
                .unwrap();
            client.write_all(&suffix).await.unwrap();
            client.shutdown().await.unwrap();
            assert_eq!(
                receiver
                    .receive(Duration::from_secs(5))
                    .await
                    .unwrap()
                    .code
                    .as_deref(),
                Some("saved")
            );
        }
    }

    #[tokio::test]
    async fn stalled_headers_cannot_lose_a_parsed_callback() {
        let receiver = LoopbackReceiver::bind().await.unwrap();
        let mut client = TcpStream::connect(("127.0.0.1", receiver.port()))
            .await
            .unwrap();
        client
            .write_all(b"GET /callback?code=saved HTTP/1.1\r\n")
            .await
            .unwrap();
        let params = timeout(
            RESPONSE_WRITE_BUDGET + Duration::from_secs(1),
            receiver.receive(Duration::from_secs(5)),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(params.code.as_deref(), Some("saved"));
    }

    #[test]
    fn error_description_is_retained_but_not_displayed_or_echoed() {
        let callback = parse_callback("GET /callback?error=access_denied&error_description=private-description&state=state HTTP/1.1").unwrap();
        assert_eq!(
            callback.error_description.as_deref(),
            Some("private-description")
        );
        assert!(!format!("{callback:?}").contains("private-description"));
        assert!(!RESPONSE_BODY.contains("private-description"));
    }
}
