//! The full PKCE flow, exercised end to end against real loopback sockets.
//!
//! Two real servers stand in here for the two things a real OAuth provider
//! is on the other end of: the browser (played by a plain `reqwest` GET
//! against our own [`kw_calendar_core::LoopbackReceiver`]) and the
//! provider's token endpoint (played by [`StubTokenServer`], a plaintext
//! loopback listener).
//!
//! The token endpoint is reached through a test-only [`TokenSender`], not
//! through [`kw_calendar_core::NetworkPolicySender`]: `kw_network_policy`
//! allows a plaintext destination only for
//! `RequestPurpose::ExplicitlyConfiguredSelfHostedEndpoint`, and calendar
//! traffic is deliberately not that purpose, so a plaintext stub could never
//! be reached through the real boundary. `NetworkPolicySender`'s own test,
//! proving it refuses exactly that plaintext target, lives in
//! `src/exchange.rs`. What is exercised here -- PKCE, the state check, the
//! HTTP form actually sent, response parsing, and storage -- is identical in
//! both paths; only the transport differs, which is the entire point of the
//! `TokenSender` seam.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use kw_calendar_core::{
    begin_authorization, complete_authorization, ensure_fresh_access_token, store_tokens,
    CalendarError, CalendarProviderKind, ExchangeError, LoopbackReceiver, ProviderEndpoints,
    TokenSender,
};
use kw_security::{CredentialStore, MemoryCredentialStore};
use kw_storage::Db;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Mutex;

/// A plaintext loopback stand-in for a provider's token endpoint. Answers
/// every request with the same canned response, and records every request
/// body it received so a test can assert on exactly what was sent.
struct StubTokenServer {
    port: u16,
    requests: Arc<Mutex<Vec<String>>>,
}

impl StubTokenServer {
    async fn start(status: u16, body: &str) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let captured = requests.clone();
        let body = body.to_owned();

        tokio::spawn(async move {
            loop {
                let Ok((stream, _addr)) = listener.accept().await else {
                    break;
                };
                let captured = captured.clone();
                let body = body.clone();
                tokio::spawn(async move {
                    handle_one(stream, status, &body, captured).await;
                });
            }
        });

        Self { port, requests }
    }

    fn url(&self) -> String {
        format!("http://127.0.0.1:{}/token", self.port)
    }

    async fn request_count(&self) -> usize {
        self.requests.lock().await.len()
    }

    async fn last_request_body(&self) -> String {
        self.requests
            .lock()
            .await
            .last()
            .cloned()
            .unwrap_or_default()
    }
}

async fn handle_one(
    mut stream: TcpStream,
    status: u16,
    body: &str,
    captured: Arc<Mutex<Vec<String>>>,
) {
    let mut buf = Vec::new();
    let mut chunk = [0_u8; 512];
    let mut content_length = 0_usize;

    loop {
        let n = stream.read(&mut chunk).await.unwrap_or(0);
        if n == 0 {
            return;
        }
        buf.extend_from_slice(&chunk[..n]);

        let Some(header_end) = find_double_crlf(&buf) else {
            continue;
        };

        let head = String::from_utf8_lossy(&buf[..header_end]).to_string();
        for line in head.lines() {
            if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                content_length = value.trim().parse().unwrap_or(0);
            }
        }

        let mut request_body = buf[header_end + 4..].to_vec();
        while request_body.len() < content_length {
            let n = stream.read(&mut chunk).await.unwrap_or(0);
            if n == 0 {
                break;
            }
            request_body.extend_from_slice(&chunk[..n]);
        }
        captured
            .lock()
            .await
            .push(String::from_utf8_lossy(&request_body).to_string());
        break;
    }

    let reason = if status == 200 { "OK" } else { "Bad Request" };
    let response = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );
    let _ = stream.write_all(response.as_bytes()).await;
    let _ = stream.shutdown().await;
}

fn find_double_crlf(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|w| w == b"\r\n\r\n")
}

/// The test-only transport: talks straight to a loopback stub over plain
/// HTTP, bypassing `kw_network_policy` deliberately (see the module doc).
struct PlaintextLoopbackSender;

#[async_trait]
impl TokenSender for PlaintextLoopbackSender {
    async fn post_form(&self, url: &str, form: &[(&str, &str)]) -> Result<String, ExchangeError> {
        let response = reqwest::Client::new()
            .post(url)
            .form(form)
            .send()
            .await
            .map_err(ExchangeError::Http)?;
        let status = response.status();
        let text = response.text().await.map_err(ExchangeError::Http)?;
        if status.is_success() {
            Ok(text)
        } else {
            Err(ExchangeError::Rejected {
                error: "http_error".to_owned(),
                description: Some(text),
            })
        }
    }
}

/// Pull the `state` query parameter back out of a public authorize URL --
/// exactly what a real provider does before echoing it back on redirect, and
/// the only way a test should learn it (`PendingAuthorization`'s `state`
/// field is private).
fn state_from_authorize_url(authorize_url: &str) -> String {
    url::Url::parse(authorize_url)
        .unwrap()
        .query_pairs()
        .find(|(key, _)| key == "state")
        .map(|(_, value)| value.into_owned())
        .expect("an authorize url must carry a state parameter")
}

/// Play the browser: connect to the loopback receiver and send a redirect
/// carrying `code` and `state` (or `error`), as a real provider would.
async fn simulate_browser_redirect(port: u16, query: &str) {
    let mut stream = TcpStream::connect(format!("127.0.0.1:{port}"))
        .await
        .unwrap();
    stream
        .write_all(format!("GET /callback?{query} HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n").as_bytes())
        .await
        .unwrap();
    let mut discard = Vec::new();
    let _ = stream.read_to_end(&mut discard).await;
}

#[tokio::test]
async fn full_round_trip_from_authorize_to_stored_refresh_token() {
    let token_server = StubTokenServer::start(
        200,
        r#"{"access_token":"access-first","refresh_token":"refresh-first","expires_in":3600,"scope":"calendar.readonly"}"#,
    )
    .await;

    let provider = ProviderEndpoints {
        authorize_url: "https://example.invalid/authorize".to_owned(),
        token_url: token_server.url(),
        scopes: vec!["calendar.readonly".to_owned()],
        extra_authorize_params: Vec::new(),
    };

    let receiver = LoopbackReceiver::bind().await.unwrap();
    let redirect_uri = receiver.redirect_uri("/callback");
    let pending = begin_authorization(&provider, "test-client-id", &redirect_uri).unwrap();
    let real_state = state_from_authorize_url(&pending.authorize_url);
    let port = receiver.port();

    let browser = tokio::spawn(async move {
        simulate_browser_redirect(port, &format!("code=the-auth-code&state={real_state}")).await;
    });

    let callback = receiver.receive(Duration::from_secs(5)).await.unwrap();
    browser.await.unwrap();

    let tokens = complete_authorization(
        &pending,
        &callback,
        &provider,
        "test-client-id",
        &PlaintextLoopbackSender,
    )
    .await
    .unwrap();
    assert_eq!(tokens.access_token.expose(), "access-first");

    let db = Db::open_in_memory().await.unwrap();
    let creds = MemoryCredentialStore::new();
    let slot = store_tokens(
        db.pool(),
        &creds,
        CalendarProviderKind::Google,
        "user@example.com",
        &tokens,
    )
    .await
    .unwrap();

    // The refresh token landed in the credential store...
    let stored = creds.get(&slot).unwrap();
    assert!(stored.expose().contains("refresh-first"));
    assert!(stored.expose().contains("access-first"));

    // ...and only the slot name landed in the database.
    let row: (String, i64, String) = sqlx::query_as(
        "SELECT token_ref, expires_at, scope FROM google_calendar_tokens WHERE google_email = ?",
    )
    .bind("user@example.com")
    .fetch_one(db.pool())
    .await
    .unwrap();
    assert_eq!(row.0, slot.slot());

    // Nothing token-shaped anywhere in that table's data.
    let dump: String = format!("{row:?}");
    assert!(!dump.contains("access-first"));
    assert!(!dump.contains("refresh-first"));

    // The backstop still holds, structurally.
    kw_storage::assert_no_credential_columns(db.pool())
        .await
        .unwrap();

    // Exactly one request reached the provider, and it looked right.
    assert_eq!(token_server.request_count().await, 1);
    let sent = token_server.last_request_body().await;
    assert!(sent.contains("grant_type=authorization_code"));
    assert!(sent.contains("code_verifier="));
    assert!(sent.contains("code=the-auth-code"));
    assert!(!sent.contains("client_secret"));
}

#[tokio::test]
async fn the_code_exchange_sends_no_secret_and_parses_the_real_response() {
    // "The code exchange" in isolation: a live POST to a real loopback
    // server, checked both for what it sent and what it returned.
    let token_server = StubTokenServer::start(
        200,
        r#"{"access_token":"access-xyz","refresh_token":"refresh-xyz","expires_in":1800,"scope":"calendar.readonly"}"#,
    )
    .await;

    let tokens = kw_calendar_core::exchange::exchange_authorization_code(
        &PlaintextLoopbackSender,
        &token_server.url(),
        "test-client-id",
        "http://127.0.0.1:0/callback",
        &kw_security::Secret::new("the-code".to_owned()),
        &kw_security::Secret::new("the-verifier".to_owned()),
        1_000,
    )
    .await
    .unwrap();

    assert_eq!(tokens.access_token.expose(), "access-xyz");
    assert_eq!(tokens.expires_at, 1_000 + 1_800);

    let sent = token_server.last_request_body().await;
    assert!(sent.contains("code=the-code"));
    assert!(sent.contains("code_verifier=the-verifier"));
    assert!(sent.contains("client_id=test-client-id"));
    assert!(!sent.contains("client_secret"));
}

#[tokio::test]
async fn a_mismatched_state_is_refused_and_the_provider_is_never_contacted() {
    let token_server =
        StubTokenServer::start(200, r#"{"access_token":"should-not-be-issued"}"#).await;

    let provider = ProviderEndpoints {
        authorize_url: "https://example.invalid/authorize".to_owned(),
        token_url: token_server.url(),
        scopes: vec!["calendar.readonly".to_owned()],
        extra_authorize_params: Vec::new(),
    };

    let receiver = LoopbackReceiver::bind().await.unwrap();
    let redirect_uri = receiver.redirect_uri("/callback");
    let pending = begin_authorization(&provider, "test-client-id", &redirect_uri).unwrap();
    let port = receiver.port();

    let browser = tokio::spawn(async move {
        simulate_browser_redirect(
            port,
            "code=irrelevant-code&state=an-attacker-supplied-state",
        )
        .await;
    });

    let callback = receiver.receive(Duration::from_secs(5)).await.unwrap();
    browser.await.unwrap();

    let err = complete_authorization(
        &pending,
        &callback,
        &provider,
        "test-client-id",
        &PlaintextLoopbackSender,
    )
    .await
    .unwrap_err();
    assert!(matches!(err, CalendarError::StateMismatch));

    // No exchange was attempted.
    assert_eq!(token_server.request_count().await, 0);
}

#[tokio::test]
async fn an_expired_access_token_is_refreshed_using_the_stored_refresh_token() {
    let token_server = StubTokenServer::start(
        200,
        r#"{"access_token":"access-refreshed","refresh_token":"refresh-rotated","expires_in":3600,"scope":"calendar.readonly"}"#,
    )
    .await;

    let provider = ProviderEndpoints {
        authorize_url: "https://example.invalid/authorize".to_owned(),
        token_url: token_server.url(),
        scopes: vec!["calendar.readonly".to_owned()],
        extra_authorize_params: Vec::new(),
    };

    let db = Db::open_in_memory().await.unwrap();
    let creds = MemoryCredentialStore::new();

    // Seed a connection whose access token already expired.
    let expired = kw_calendar_core::TokenSet {
        access_token: kw_security::Secret::new("access-stale".to_owned()),
        refresh_token: Some(kw_security::Secret::new("refresh-original".to_owned())),
        expires_at: 1, // 1970, unambiguously in the past
        scope: "calendar.readonly".to_owned(),
    };
    store_tokens(
        db.pool(),
        &creds,
        CalendarProviderKind::Google,
        "user@example.com",
        &expired,
    )
    .await
    .unwrap();

    let access = ensure_fresh_access_token(
        db.pool(),
        &creds,
        &PlaintextLoopbackSender,
        &provider,
        "test-client-id",
        CalendarProviderKind::Google,
        "user@example.com",
    )
    .await
    .unwrap();

    assert_eq!(access.expose(), "access-refreshed");
    assert_eq!(token_server.request_count().await, 1);
    let sent = token_server.last_request_body().await;
    assert!(sent.contains("grant_type=refresh_token"));
    assert!(sent.contains("refresh_token=refresh-original"));
    assert!(!sent.contains("client_secret"));

    // The database's expiry moved into the future, and the refresh token
    // rotated in the credential store.
    let row: (String, i64) = sqlx::query_as(
        "SELECT token_ref, expires_at FROM google_calendar_tokens WHERE google_email = ?",
    )
    .bind("user@example.com")
    .fetch_one(db.pool())
    .await
    .unwrap();
    assert!(row.1 > 1);

    let slot = kw_security::KeyRef::new(&row.0).unwrap();
    let stored = creds.get(&slot).unwrap();
    assert!(stored.expose().contains("refresh-rotated"));
    assert!(!stored.expose().contains("refresh-original"));
}
