//! The JSON-RPC loop: one message per line in, at most one line out.
//!
//! [`run`] takes a reader and a writer rather than reaching for
//! `tokio::io::stdin`/`stdout` itself, which is what makes it testable: a
//! test drives it over an in-memory pipe (see `tests/protocol.rs`), and
//! `main` is the only place that ever hands it the real stdio handles.
//!
//! MCP's stdio transport is newline-delimited JSON-RPC, not the
//! `Content-Length`-framed style LSP uses. A message is exactly one line, and
//! this loop enforces that on the way out as much as it expects it on the
//! way in.
//!
//! ## The rule this file exists to keep
//!
//! **Only a JSON-RPC response line is ever written to `writer`.** Every
//! diagnostic — a line that failed to parse, an unknown method, a tool that
//! does not exist — goes to `tracing`, which `main` wires to stderr. A
//! notification (a request with no `id`) is never answered at all, per the
//! JSON-RPC spec: answering one would put a line on stdout that no request
//! asked for. `tests/protocol.rs` has a test that plays several malformed and
//! well-formed lines through the loop and checks that literally everything
//! read back from `writer` is a valid JSON-RPC response — nothing else ever
//! reached it.

use kw_cli::Client;
use serde_json::{json, Value};
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWrite, AsyncWriteExt};

use crate::protocol::{ErrorObject, Response, INVALID_REQUEST, PARSE_ERROR};
use crate::tools;

/// The protocol version this server was written against.
///
/// Echoed back only when the client did not send one; when it did, its own
/// value is echoed instead, since nothing in this server's behaviour actually
/// varies by MCP protocol revision.
const PROTOCOL_VERSION: &str = "2024-11-05";

/// A response that could not be encoded — which would mean `serde_json`
/// failed on this crate's own, entirely serialisable [`Response`] type. That
/// should never happen; this is what keeps "never" true even if it somehow
/// did, rather than the alternative of panicking with a JSON-RPC connection
/// half written.
const ENCODING_FAILED: &str = r#"{"jsonrpc":"2.0","id":null,"error":{"code":-32603,"message":"could not encode the response"}}"#;

/// Read JSON-RPC requests from `reader` one line at a time, and write at most
/// one JSON-RPC response line to `writer` for each one that expects an
/// answer. Returns once `reader` reaches EOF.
pub async fn run<R, W>(reader: R, mut writer: W, client: Client) -> std::io::Result<()>
where
    R: AsyncBufRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut lines = reader.lines();
    while let Some(line) = lines.next_line().await? {
        if line.trim().is_empty() {
            // Blank lines are not part of the protocol either way; MCP hosts
            // do not send them, but a keepalive newline should not be treated
            // as a parse error.
            continue;
        }
        if let Some(response) = handle_line(&line, &client).await {
            write_response(&mut writer, &response).await?;
        }
    }
    Ok(())
}

async fn write_response<W: AsyncWrite + Unpin>(
    writer: &mut W,
    response: &Response,
) -> std::io::Result<()> {
    let mut text = serde_json::to_string(response).unwrap_or_else(|err| {
        tracing::error!(error = %err, "could not encode a JSON-RPC response");
        ENCODING_FAILED.to_owned()
    });
    text.push('\n');
    writer.write_all(text.as_bytes()).await?;
    writer.flush().await
}

/// What one line produced: a value to answer with, an error to answer with,
/// or nothing — because the line was a notification, which the JSON-RPC spec
/// says never gets a response, success or failure.
enum Outcome {
    Result(Value),
    Error(ErrorObject),
    Silent,
}

/// Turn one line of input into at most one response.
///
/// Never panics: a line that is not JSON, or JSON that is not a request,
/// becomes a JSON-RPC error object rather than an unwrap. That is the
/// property `tests/protocol.rs`'s malformed-request test locks down.
async fn handle_line(line: &str, client: &Client) -> Option<Response> {
    let value: Value = match serde_json::from_str(line) {
        Ok(value) => value,
        Err(err) => {
            tracing::warn!(error = %err, "a line on stdin was not valid JSON");
            return Some(Response::failure(
                Value::Null,
                ErrorObject::new(PARSE_ERROR, "the request was not valid JSON"),
            ));
        }
    };

    // Present at all — even `null` — means "please answer me"; absent means
    // "notification, do not answer". The two are different per the JSON-RPC
    // spec, and only the object case can come from `.get`.
    let id = value.get("id").cloned();

    let Some(method) = value.get("method").and_then(Value::as_str) else {
        tracing::warn!("a request had no 'method'");
        return id.map(|id| {
            Response::failure(
                id,
                ErrorObject::new(INVALID_REQUEST, "'method' is required and must be a string"),
            )
        });
    };

    let params = value.get("params").cloned().unwrap_or(Value::Null);
    let outcome = dispatch(method, &params, client).await;

    match (id, outcome) {
        (_, Outcome::Silent) => None,
        // No id at all: a notification. Never answered, even with an error —
        // there is nothing wrong with a notification this server does not
        // recognise; it is simply not for this server to react to.
        (None, _) => None,
        (Some(id), Outcome::Result(result)) => Some(Response::success(id, result)),
        (Some(id), Outcome::Error(error)) => Some(Response::failure(id, error)),
    }
}

async fn dispatch(method: &str, params: &Value, client: &Client) -> Outcome {
    match method {
        "initialize" => Outcome::Result(handle_initialize(params)),
        "tools/list" => Outcome::Result(handle_tools_list()),
        "tools/call" => match handle_tools_call(params, client).await {
            Ok(result) => Outcome::Result(result),
            Err(error) => Outcome::Error(error),
        },
        // "notifications/initialized" and any other notification-shaped
        // method: nothing to do, and nothing to answer.
        name if name.starts_with("notifications/") => Outcome::Silent,
        other => {
            tracing::warn!(method = other, "received an unknown method");
            Outcome::Error(ErrorObject::method_not_found(other))
        }
    }
}

fn handle_initialize(params: &Value) -> Value {
    let protocol_version = params
        .get("protocolVersion")
        .and_then(Value::as_str)
        .unwrap_or(PROTOCOL_VERSION);

    json!({
        "protocolVersion": protocol_version,
        "capabilities": { "tools": {} },
        "serverInfo": {
            "name": "knegiswhisper-mcp",
            "version": env!("CARGO_PKG_VERSION"),
        },
    })
}

fn handle_tools_list() -> Value {
    json!({ "tools": tools::definitions() })
}

async fn handle_tools_call(params: &Value, client: &Client) -> Result<Value, ErrorObject> {
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| ErrorObject::invalid_params("'name' is required and must be a string"))?;
    let arguments = params
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| json!({}));

    let result = tools::call(client, name, &arguments).await?;
    Ok(result.into_value())
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

    fn bogus_client() -> Client {
        // A port nothing listens on. These tests never let a request reach
        // the network — that is exactly what they are checking.
        Client::new(1, kw_security::LocalApiToken::generate().unwrap())
    }

    #[tokio::test]
    async fn invalid_json_becomes_a_parse_error_with_a_null_id() {
        let response = handle_line("not json at all", &bogus_client())
            .await
            .unwrap();
        assert_eq!(response.error.unwrap().code, PARSE_ERROR);
        assert!(response.id.is_null());
    }

    #[tokio::test]
    async fn a_request_missing_method_is_an_invalid_request_with_the_original_id() {
        let response = handle_line(r#"{"jsonrpc":"2.0","id":7}"#, &bogus_client())
            .await
            .unwrap();
        assert_eq!(response.error.unwrap().code, INVALID_REQUEST);
        assert_eq!(response.id, Value::from(7));
    }

    #[tokio::test]
    async fn a_notification_is_never_answered_even_when_malformed() {
        // No "id" key at all: per JSON-RPC, this must not get a response,
        // regardless of whether the method is recognised.
        let response = handle_line(
            r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
            &bogus_client(),
        )
        .await;
        assert!(response.is_none());

        let response = handle_line(
            r#"{"jsonrpc":"2.0","method":"notifications/anything"}"#,
            &bogus_client(),
        )
        .await;
        assert!(response.is_none());
    }

    #[tokio::test]
    async fn an_unknown_method_with_an_id_is_method_not_found() {
        let response = handle_line(
            r#"{"jsonrpc":"2.0","id":1,"method":"tools/frobnicate"}"#,
            &bogus_client(),
        )
        .await
        .unwrap();
        assert_eq!(
            response.error.unwrap().code,
            crate::protocol::METHOD_NOT_FOUND
        );
    }

    #[tokio::test]
    async fn initialize_echoes_the_clients_protocol_version() {
        let response = handle_line(
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2099-01-01"}}"#,
            &bogus_client(),
        )
        .await
        .unwrap();
        let result = response.result.unwrap();
        assert_eq!(result["protocolVersion"], "2099-01-01");
        assert_eq!(result["serverInfo"]["name"], "knegiswhisper-mcp");
    }

    #[tokio::test]
    async fn initialize_without_a_version_falls_back_to_this_servers_own() {
        let response = handle_line(
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
            &bogus_client(),
        )
        .await
        .unwrap();
        let result = response.result.unwrap();
        assert_eq!(result["protocolVersion"], PROTOCOL_VERSION);
    }

    #[tokio::test]
    async fn tools_list_reports_every_defined_tool() {
        let response = handle_line(
            r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#,
            &bogus_client(),
        )
        .await
        .unwrap();
        let tools = response.result.unwrap()["tools"].as_array().unwrap().len();
        assert_eq!(tools, tools::definitions().len());
    }

    #[tokio::test]
    async fn tools_call_without_a_name_is_invalid_params() {
        let response = handle_line(
            r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{}}"#,
            &bogus_client(),
        )
        .await
        .unwrap();
        assert_eq!(
            response.error.unwrap().code,
            crate::protocol::INVALID_PARAMS
        );
    }
}
