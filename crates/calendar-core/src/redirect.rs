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
use tokio::net::TcpListener;
use tokio::time::timeout;

/// Upper bound on how much of the request line this will buffer before
/// giving up. The only client that can reach this listener is the user's own
/// browser, but the bound is cheap insurance against a malformed connection
/// (or a port collision with something unexpected) tying up memory instead of
/// failing promptly.
const MAX_REQUEST_LINE_BYTES: usize = 8 * 1024;

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

    /// Accept exactly one connection, answer it, and return what it carried.
    ///
    /// `wait` bounds how long this waits for the browser to show up at all --
    /// a user who closes the tab instead of finishing consent must not hang
    /// the application forever.
    pub async fn receive(self, wait: Duration) -> Result<CallbackParams, RedirectError> {
        let (mut stream, _) = timeout(wait, self.listener.accept())
            .await
            .map_err(|_elapsed| RedirectError::TimedOut)?
            .map_err(RedirectError::Accept)?;

        let line = read_request_line(&mut stream).await?;
        let params = parse_callback(&line)?;

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

        Ok(params)
    }
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
    for (key, value) in url.query_pairs() {
        match key.as_ref() {
            "code" => code = Some(value.into_owned()),
            "state" => state = Some(value.into_owned()),
            "error" => error = Some(value.into_owned()),
            _ => {}
        }
    }

    Ok(CallbackParams { code, state, error })
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
}
