//! The JSON-RPC 2.0 envelope MCP runs over stdio.
//!
//! A response always carries either `result` or `error`, never both, and
//! always carries `id` — `null` when the request's own id could not be
//! determined (an unparsable line) or was not present (a notification, which
//! [`crate::server`] never answers at all; see there for why).
//!
//! The codes below are the JSON-RPC reserved range, not something this crate
//! invented.

use serde::Serialize;
use serde_json::Value;

/// The request could not be parsed as JSON at all.
pub const PARSE_ERROR: i64 = -32700;
/// The request was valid JSON but not a valid JSON-RPC request (for example,
/// no `method`).
pub const INVALID_REQUEST: i64 = -32600;
/// `method` named something this server does not implement.
pub const METHOD_NOT_FOUND: i64 = -32601;
/// The method exists but the params — or, for `tools/call`, the tool name or
/// its arguments — were wrong in a way that is this caller's mistake rather
/// than something the local API could answer.
pub const INVALID_PARAMS: i64 = -32602;

/// A JSON-RPC error object.
#[derive(Debug, Clone, Serialize)]
pub struct ErrorObject {
    pub code: i64,
    pub message: String,
}

impl ErrorObject {
    #[must_use]
    pub fn new(code: i64, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    /// `-32602`, for a `tools/call` this crate refused before it ever reached
    /// the local API: an unknown tool name, a missing required argument, or
    /// an argument of the wrong JSON type.
    #[must_use]
    pub fn invalid_params(message: impl Into<String>) -> Self {
        Self::new(INVALID_PARAMS, message)
    }

    #[must_use]
    pub fn method_not_found(method: &str) -> Self {
        Self::new(METHOD_NOT_FOUND, format!("unknown method {method:?}"))
    }
}

/// A JSON-RPC 2.0 response line.
#[derive(Debug, Clone, Serialize)]
pub struct Response {
    pub jsonrpc: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ErrorObject>,
    pub id: Value,
}

impl Response {
    #[must_use]
    pub const fn success(id: Value, result: Value) -> Self {
        Self {
            jsonrpc: "2.0",
            result: Some(result),
            error: None,
            id,
        }
    }

    #[must_use]
    pub const fn failure(id: Value, error: ErrorObject) -> Self {
        Self {
            jsonrpc: "2.0",
            result: None,
            error: Some(error),
            id,
        }
    }
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
    fn a_success_response_carries_no_error_field() {
        let response = Response::success(Value::from(1), Value::from("ok"));
        let json = serde_json::to_value(&response).unwrap();
        assert!(json.get("error").is_none());
        assert_eq!(json.get("result"), Some(&Value::from("ok")));
        assert_eq!(json.get("jsonrpc"), Some(&Value::from("2.0")));
    }

    #[test]
    fn a_failure_response_carries_no_result_field() {
        let response = Response::failure(Value::Null, ErrorObject::invalid_params("nope"));
        let json = serde_json::to_value(&response).unwrap();
        assert!(json.get("result").is_none());
        assert_eq!(
            json.get("error").and_then(|e| e.get("code")),
            Some(&Value::from(INVALID_PARAMS))
        );
        assert!(json.get("id").unwrap().is_null());
    }

    #[test]
    fn method_not_found_names_the_method() {
        let error = ErrorObject::method_not_found("tools/frobnicate");
        assert_eq!(error.code, METHOD_NOT_FOUND);
        assert!(error.message.contains("tools/frobnicate"));
    }
}
