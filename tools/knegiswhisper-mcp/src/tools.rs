//! The MCP tool surface: one tool per local API route, and nothing else.
//!
//! [`definitions`] is what `tools/list` answers, and [`call`] is what
//! `tools/call` dispatches through. Both are driven by the same list of
//! names on purpose — a tool `call` knows how to run is a tool `definitions`
//! must have advertised, and there is exactly one place, [`TOOLS`], where
//! that list is written down.
//!
//! Every tool is a thin HTTP request through [`kw_cli::Client`] — the same
//! client, and so the same discovery file and the same bearer token, that
//! `knegiswhisper` the CLI already uses. Nothing here reads SQLite, and
//! nothing here invents an endpoint the local API does not serve.
//!
//! ## What is validated here, and what is left to the local API
//!
//! This module checks only the *shape* of an argument: an `id` that is
//! supposed to be an integer must be one, a `query` that is required must be
//! present. Business rules — a limit of zero, a folder id that does not
//! exist, a patch that changes nothing — are the local API's job, because it
//! already has exactly one implementation of each of those decisions and a
//! second one here would be a second place to keep in sync. A shape problem
//! comes back as a JSON-RPC error (this crate's fault, for describing the
//! tool wrong or the caller's, for calling it wrong); a business rule the
//! local API refuses comes back as a normal tool result with `isError: true`
//! (not a protocol failure — the answer to the question asked).

use reqwest::Method;
use serde_json::{json, Value};

use kw_cli::{CliError, Client};

use crate::protocol::ErrorObject;

/// Every tool this server exposes, in the order `tools/list` reports them.
///
/// Each entry names one local API route. There is no tool here without a
/// route behind it, and no route with two tools in front of it.
const TOOLS: &[&str] = &[
    "health",
    "notes_list",
    "notes_get",
    "notes_search",
    "notes_patch",
    "notes_delete",
    "folders_list",
    "dictionary_list",
    "dictionary_update",
    "transcriptions_list",
    "transcriptions_get",
    "transcriptions_delete",
    "transcriptions_forget_audio",
];

/// The `tools` array for a `tools/list` response.
#[must_use]
pub fn definitions() -> Vec<Value> {
    TOOLS.iter().map(|name| definition(name)).collect()
}

/// One tool's `{name, description, inputSchema}`, or a schema so restrictive
/// nothing can call it — which would only happen if [`TOOLS`] and this
/// function's `match` fell out of sync, and a test below catches that.
fn definition(name: &str) -> Value {
    let (description, schema) = match name {
        "health" => (
            "Check whether KnegisWhisper is running and report its version. Maps to GET \
             /v1/health.",
            empty_schema(),
        ),
        "notes_list" => (
            "List notes, most recently edited first. Maps to GET /v1/notes/list.",
            json!({
                "type": "object",
                "properties": {
                    "limit": {
                        "type": "integer",
                        "minimum": 1,
                        "description": "Maximum notes to return (server default 100, max 500)."
                    },
                    "folder_id": { "type": "integer", "description": "Only notes in this folder." },
                    "note_type": { "type": "string", "description": "Only notes of this type." }
                },
                "additionalProperties": false
            }),
        ),
        "notes_get" => (
            "Show one note in full, including its content. Maps to GET /v1/notes/{id}.",
            id_schema("The note's id."),
        ),
        "notes_search" => (
            "Full-text search over notes. Maps to GET /v1/notes/search.",
            json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "The search text." },
                    "limit": { "type": "integer", "minimum": 1 }
                },
                "required": ["query"],
                "additionalProperties": false
            }),
        ),
        "notes_patch" => (
            "Change a note's title, content, or folder. Send only the fields to change. Maps \
             to PATCH /v1/notes/{id}.",
            json!({
                "type": "object",
                "properties": {
                    "id": { "type": "integer", "description": "The note's id." },
                    "title": { "type": "string" },
                    "content": { "type": "string" },
                    "folder_id": { "type": "integer" }
                },
                "required": ["id"],
                "additionalProperties": false
            }),
        ),
        "notes_delete" => (
            "Delete a note. Maps to DELETE /v1/notes/{id}.",
            id_schema("The note's id."),
        ),
        "folders_list" => (
            "List note folders. Maps to GET /v1/folders/list.",
            empty_schema(),
        ),
        "dictionary_list" => (
            "List the custom dictionary words that bias speech recognition. Maps to GET \
             /v1/dictionary/list.",
            empty_schema(),
        ),
        "dictionary_update" => (
            "Add or remove custom dictionary words. Maps to POST /v1/dictionary/update.",
            json!({
                "type": "object",
                "properties": {
                    "add": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Words to add."
                    },
                    "remove": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Words to remove."
                    }
                },
                "additionalProperties": false
            }),
        ),
        "transcriptions_list" => (
            "List dictation history, newest first. Maps to GET /v1/transcriptions/list.",
            json!({
                "type": "object",
                "properties": {
                    "limit": { "type": "integer", "minimum": 1 },
                    "after": {
                        "type": "integer",
                        "description": "Cursor: continue after this dictation id."
                    }
                },
                "additionalProperties": false
            }),
        ),
        "transcriptions_get" => (
            "Show one dictation's transcript. Maps to GET /v1/transcriptions/{id}.",
            id_schema("The dictation's id."),
        ),
        "transcriptions_delete" => (
            "Delete a dictation and its transcript. Maps to DELETE /v1/transcriptions/{id}.",
            id_schema("The dictation's id."),
        ),
        "transcriptions_forget_audio" => (
            "Delete only the kept recording for a dictation, leaving the transcript. Maps to \
             DELETE /v1/transcriptions/{id}/audio.",
            id_schema("The dictation's id."),
        ),
        // Unreachable from `definitions`, which only ever calls this with a
        // name out of `TOOLS`. Kept as a real schema rather than panicking,
        // in case that invariant is ever broken by an edit.
        _ => ("", empty_schema()),
    };

    json!({ "name": name, "description": description, "inputSchema": schema })
}

fn empty_schema() -> Value {
    json!({ "type": "object", "properties": {}, "additionalProperties": false })
}

fn id_schema(id_description: &str) -> Value {
    json!({
        "type": "object",
        "properties": { "id": { "type": "integer", "description": id_description } },
        "required": ["id"],
        "additionalProperties": false
    })
}

/// The MCP `content` a `tools/call` answers with: text, and whether it
/// describes a failure.
#[derive(Debug)]
pub struct CallResult {
    text: String,
    is_error: bool,
}

impl CallResult {
    fn ok(text: String) -> Self {
        Self {
            text,
            is_error: false,
        }
    }

    fn err(text: String) -> Self {
        Self {
            text,
            is_error: true,
        }
    }

    #[must_use]
    pub fn into_value(self) -> Value {
        json!({
            "content": [{ "type": "text", "text": self.text }],
            "isError": self.is_error,
        })
    }
}

/// Run one tool call.
///
/// `Err` here means the call never reached the local API: an unknown tool
/// name, or an argument that does not match the tool's declared shape. Once
/// a request is well-formed, whatever the local API answers — success or
/// failure — comes back as `Ok`, because reaching it and getting a "no" is
/// not a protocol error.
pub async fn call(client: &Client, name: &str, args: &Value) -> Result<CallResult, ErrorObject> {
    match name {
        "health" => Ok(respond(client.get("/v1/health").await)),
        "notes_list" => notes_list(client, args).await,
        "notes_get" => notes_get(client, args).await,
        "notes_search" => notes_search(client, args).await,
        "notes_patch" => notes_patch(client, args).await,
        "notes_delete" => notes_delete(client, args).await,
        "folders_list" => Ok(respond(client.get("/v1/folders/list").await)),
        "dictionary_list" => Ok(respond(client.get("/v1/dictionary/list").await)),
        "dictionary_update" => dictionary_update(client, args).await,
        "transcriptions_list" => transcriptions_list(client, args).await,
        "transcriptions_get" => transcriptions_get(client, args).await,
        "transcriptions_delete" => transcriptions_delete(client, args).await,
        "transcriptions_forget_audio" => transcriptions_forget_audio(client, args).await,
        other => Err(ErrorObject::invalid_params(format!(
            "unknown tool {other:?}"
        ))),
    }
}

/// Turn the local API's answer into a tool result: the JSON on success, the
/// error's message — the same one a CLI user would see — on failure.
fn respond(outcome: Result<Value, CliError>) -> CallResult {
    match outcome {
        Ok(value) => CallResult::ok(kw_cli::render::as_json(&value)),
        Err(err) => CallResult::err(err.to_string()),
    }
}

async fn deleted(client: &Client, path: String, extra: Value) -> CallResult {
    match client.request(Method::DELETE, &path, None).await {
        Ok(_) => {
            let mut body = extra;
            if let Some(map) = body.as_object_mut() {
                map.insert("deleted".to_owned(), Value::Bool(true));
            }
            CallResult::ok(kw_cli::render::as_json(&body))
        }
        Err(err) => CallResult::err(err.to_string()),
    }
}

async fn notes_list(client: &Client, args: &Value) -> Result<CallResult, ErrorObject> {
    let mut pairs = Vec::new();
    if let Some(limit) = optional_u32(args, "limit")? {
        pairs.push(("limit".to_owned(), limit.to_string()));
    }
    if let Some(folder_id) = optional_i64(args, "folder_id")? {
        pairs.push(("folder_id".to_owned(), folder_id.to_string()));
    }
    if let Some(note_type) = optional_string(args, "note_type")? {
        pairs.push(("note_type".to_owned(), note_type));
    }
    Ok(respond(
        client.get(&query_path("/v1/notes/list", &pairs)).await,
    ))
}

async fn notes_get(client: &Client, args: &Value) -> Result<CallResult, ErrorObject> {
    let id = required_i64(args, "id", "notes_get")?;
    Ok(respond(client.get(&format!("/v1/notes/{id}")).await))
}

async fn notes_search(client: &Client, args: &Value) -> Result<CallResult, ErrorObject> {
    let query = required_string(args, "query", "notes_search")?;
    let mut pairs = vec![("q".to_owned(), query)];
    if let Some(limit) = optional_u32(args, "limit")? {
        pairs.push(("limit".to_owned(), limit.to_string()));
    }
    Ok(respond(
        client.get(&query_path("/v1/notes/search", &pairs)).await,
    ))
}

async fn notes_patch(client: &Client, args: &Value) -> Result<CallResult, ErrorObject> {
    let id = required_i64(args, "id", "notes_patch")?;

    let mut body = serde_json::Map::new();
    if let Some(title) = optional_string(args, "title")? {
        body.insert("title".to_owned(), Value::String(title));
    }
    if let Some(content) = optional_string(args, "content")? {
        body.insert("content".to_owned(), Value::String(content));
    }
    if let Some(folder_id) = optional_i64(args, "folder_id")? {
        body.insert("folder_id".to_owned(), json!(folder_id));
    }

    Ok(respond(
        client
            .request(
                Method::PATCH,
                &format!("/v1/notes/{id}"),
                Some(Value::Object(body)),
            )
            .await,
    ))
}

async fn notes_delete(client: &Client, args: &Value) -> Result<CallResult, ErrorObject> {
    let id = required_i64(args, "id", "notes_delete")?;
    Ok(deleted(client, format!("/v1/notes/{id}"), json!({ "id": id })).await)
}

async fn dictionary_update(client: &Client, args: &Value) -> Result<CallResult, ErrorObject> {
    let add = optional_string_array(args, "add")?;
    let remove = optional_string_array(args, "remove")?;
    Ok(respond(
        client
            .request(
                Method::POST,
                "/v1/dictionary/update",
                Some(json!({ "add": add, "remove": remove })),
            )
            .await,
    ))
}

async fn transcriptions_list(client: &Client, args: &Value) -> Result<CallResult, ErrorObject> {
    let mut pairs = Vec::new();
    if let Some(limit) = optional_u32(args, "limit")? {
        pairs.push(("limit".to_owned(), limit.to_string()));
    }
    if let Some(after) = optional_i64(args, "after")? {
        pairs.push(("after".to_owned(), after.to_string()));
    }
    Ok(respond(
        client
            .get(&query_path("/v1/transcriptions/list", &pairs))
            .await,
    ))
}

async fn transcriptions_get(client: &Client, args: &Value) -> Result<CallResult, ErrorObject> {
    let id = required_i64(args, "id", "transcriptions_get")?;
    Ok(respond(
        client.get(&format!("/v1/transcriptions/{id}")).await,
    ))
}

async fn transcriptions_delete(client: &Client, args: &Value) -> Result<CallResult, ErrorObject> {
    let id = required_i64(args, "id", "transcriptions_delete")?;
    Ok(deleted(
        client,
        format!("/v1/transcriptions/{id}"),
        json!({ "id": id }),
    )
    .await)
}

async fn transcriptions_forget_audio(
    client: &Client,
    args: &Value,
) -> Result<CallResult, ErrorObject> {
    let id = required_i64(args, "id", "transcriptions_forget_audio")?;
    Ok(deleted(
        client,
        format!("/v1/transcriptions/{id}/audio"),
        json!({ "id": id, "audio_forgotten": true }),
    )
    .await)
}

// -------------------------------------------------------------- argument shape

fn required_i64(args: &Value, field: &str, tool: &str) -> Result<i64, ErrorObject> {
    match args.get(field) {
        Some(value) => value.as_i64().ok_or_else(|| {
            ErrorObject::invalid_params(format!("{tool} requires '{field}' to be an integer"))
        }),
        None => Err(ErrorObject::invalid_params(format!(
            "{tool} requires an integer '{field}'"
        ))),
    }
}

fn required_string(args: &Value, field: &str, tool: &str) -> Result<String, ErrorObject> {
    match args.get(field) {
        Some(Value::String(value)) => Ok(value.clone()),
        Some(_) => Err(ErrorObject::invalid_params(format!(
            "{tool} requires '{field}' to be a string"
        ))),
        None => Err(ErrorObject::invalid_params(format!(
            "{tool} requires a string '{field}'"
        ))),
    }
}

fn optional_i64(args: &Value, field: &str) -> Result<Option<i64>, ErrorObject> {
    match args.get(field) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => value
            .as_i64()
            .map(Some)
            .ok_or_else(|| ErrorObject::invalid_params(format!("'{field}' must be an integer"))),
    }
}

fn optional_u32(args: &Value, field: &str) -> Result<Option<u32>, ErrorObject> {
    match args.get(field) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => value
            .as_u64()
            .and_then(|n| u32::try_from(n).ok())
            .map(Some)
            .ok_or_else(|| {
                ErrorObject::invalid_params(format!("'{field}' must be a non-negative integer"))
            }),
    }
}

fn optional_string(args: &Value, field: &str) -> Result<Option<String>, ErrorObject> {
    match args.get(field) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => Ok(Some(value.clone())),
        Some(_) => Err(ErrorObject::invalid_params(format!(
            "'{field}' must be a string"
        ))),
    }
}

fn optional_string_array(args: &Value, field: &str) -> Result<Vec<String>, ErrorObject> {
    match args.get(field) {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(Value::Array(items)) => items
            .iter()
            .map(|item| {
                item.as_str().map(str::to_owned).ok_or_else(|| {
                    ErrorObject::invalid_params(format!("'{field}' must be an array of strings"))
                })
            })
            .collect(),
        Some(_) => Err(ErrorObject::invalid_params(format!(
            "'{field}' must be an array of strings"
        ))),
    }
}

/// Percent-encode a query value.
///
/// Hand-rolled for the same reason `tools/knegiswhisper`'s own `urlencode`
/// is: the alphabet that needs escaping in a query value is short and fixed,
/// and the encoder is not exported from that crate's binary for this one to
/// reuse. Encoding everything outside the unreserved set is conservative and
/// cannot under-escape.
fn urlencode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(char::from(*byte));
            }
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

fn query_path(base: &str, pairs: &[(String, String)]) -> String {
    if pairs.is_empty() {
        return base.to_owned();
    }
    let query = pairs
        .iter()
        .map(|(key, value)| format!("{key}={}", urlencode(value)))
        .collect::<Vec<_>>()
        .join("&");
    format!("{base}?{query}")
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
    fn every_advertised_tool_has_a_real_schema() {
        // Guards against `TOOLS` and `definition`'s `match` falling out of
        // sync: the catch-all arm returns an empty description, which would
        // otherwise ship silently.
        for value in definitions() {
            let name = value.get("name").and_then(Value::as_str).unwrap();
            let description = value.get("description").and_then(Value::as_str).unwrap();
            assert!(!description.is_empty(), "{name} has no description");
        }
    }

    #[test]
    fn the_tool_count_matches_the_local_apis_routes() {
        // health, notes: list/get/search/patch/delete, folders: list,
        // dictionary: list/update, transcriptions: list/get/delete/forget-audio.
        assert_eq!(definitions().len(), 13);
    }

    #[test]
    fn an_id_tool_requires_id() {
        let schema = id_schema("x");
        assert_eq!(schema["required"], json!(["id"]));
    }

    #[test]
    fn required_i64_reports_which_tool_and_field() {
        let err = required_i64(&json!({}), "id", "notes_get").unwrap_err();
        assert_eq!(err.code, crate::protocol::INVALID_PARAMS);
        assert!(err.message.contains("notes_get"));
        assert!(err.message.contains("id"));
    }

    #[test]
    fn required_i64_rejects_the_wrong_type() {
        let err = required_i64(&json!({"id": "abc"}), "id", "notes_get").unwrap_err();
        assert!(err.message.contains("integer"));
    }

    #[test]
    fn optional_fields_absent_are_none_not_an_error() {
        assert_eq!(optional_i64(&json!({}), "folder_id").unwrap(), None);
        assert_eq!(optional_u32(&json!({}), "limit").unwrap(), None);
        assert_eq!(optional_string(&json!({}), "title").unwrap(), None);
    }

    #[test]
    fn optional_fields_of_the_wrong_type_are_refused() {
        assert!(optional_i64(&json!({"folder_id": "x"}), "folder_id").is_err());
        assert!(optional_u32(&json!({"limit": -1}), "limit").is_err());
        assert!(optional_string(&json!({"title": 5}), "title").is_err());
    }

    #[test]
    fn a_negative_limit_is_a_shape_error_not_silently_dropped() {
        // as_u64 fails on a negative i64, which must not be read as "absent".
        let err = optional_u32(&json!({"limit": -5}), "limit").unwrap_err();
        assert!(err.message.contains("non-negative"));
    }

    #[test]
    fn dictionary_arrays_must_be_strings() {
        assert!(optional_string_array(&json!({"add": [1, 2]}), "add").is_err());
        assert_eq!(
            optional_string_array(&json!({"add": ["a", "b"]}), "add").unwrap(),
            vec!["a".to_owned(), "b".to_owned()]
        );
        assert_eq!(
            optional_string_array(&json!({}), "add").unwrap(),
            Vec::<String>::new()
        );
    }

    #[test]
    fn query_values_are_percent_encoded() {
        assert_eq!(urlencode("warehouse plan"), "warehouse%20plan");
        assert_eq!(urlencode("a&b=c"), "a%26b%3Dc");
    }

    #[test]
    fn an_empty_pair_list_leaves_the_path_bare() {
        assert_eq!(query_path("/v1/notes/list", &[]), "/v1/notes/list");
    }

    #[tokio::test]
    async fn an_unknown_tool_name_is_invalid_params_not_a_network_call() {
        // A bogus port that nothing listens on: if this test passes, the
        // unknown-tool check happened before any request was attempted.
        let client = Client::new(1, kw_security::LocalApiToken::generate().unwrap());
        let err = call(&client, "not_a_real_tool", &json!({}))
            .await
            .unwrap_err();
        assert_eq!(err.code, crate::protocol::INVALID_PARAMS);
        assert!(err.message.contains("not_a_real_tool"));
    }
}
