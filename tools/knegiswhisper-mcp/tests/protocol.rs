//! The JSON-RPC protocol, driven in process over an in-memory pipe.
//!
//! `kw_mcp::server::run` takes a reader and a writer rather than reaching for
//! real stdio, so a test can hand it one end of a `tokio::io::duplex` and
//! drive the other end exactly the way an MCP host would drive a real
//! process's stdin and stdout — without spawning the compiled binary, and
//! without anything touching a real running KnegisWhisper.
//!
//! The local API side is real, though: [`Harness`] starts the actual
//! `crates/local-api` server over an in-memory SQLite database, the same way
//! `tools/knegiswhisper/tests/end_to_end.rs` does for the CLI. That is what
//! makes the `tools/call` round trip a real HTTP request rather than a mock.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use kw_cli::Client;
use kw_local_api::{serve, ApiState, Served};
use kw_notes_core::notes;
use kw_security::LocalApiToken;
use kw_storage::{transcriptions, Db};
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, DuplexStream, ReadHalf, WriteHalf};
use tokio::net::TcpListener;

#[tokio::test(flavor = "multi_thread")]
async fn new_tools_round_trip_notes_and_report_unavailable_runtime_as_tool_errors() {
    let harness = Harness::start().await;
    let created = kw_mcp::tools::call(
        &harness.client,
        "notes_create",
        &json!({"title":"Meeting","content":"budget"}),
    )
    .await
    .unwrap()
    .into_value();
    assert_eq!(created["isError"], false);
    let note: Value =
        serde_json::from_str(created["content"][0]["text"].as_str().unwrap()).unwrap();
    let exported = kw_mcp::tools::call(
        &harness.client,
        "notes_export",
        &json!({"id":note["data"]["id"]}),
    )
    .await
    .unwrap()
    .into_value();
    assert_eq!(exported["isError"], false);
    let export: Value =
        serde_json::from_str(exported["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(export["data"]["content"], "# Meeting\n\nbudget\n");
    let search = kw_mcp::tools::call(
        &harness.client,
        "transcriptions_search",
        &json!({"query":"budget"}),
    )
    .await
    .unwrap()
    .into_value();
    assert_eq!(search["isError"], false);
    for (name, args) in [
        ("app_status", json!({})),
        ("models_list", json!({})),
        ("recording_start", json!({"provider":"local"})),
        ("recording_stop", json!({})),
        ("jobs_get", json!({"id":"missing"})),
        ("jobs_cancel", json!({"id":"missing"})),
        ("transcription_create", json!({"path":"missing.wav"})),
    ] {
        assert_eq!(
            kw_mcp::tools::call(&harness.client, name, &args)
                .await
                .unwrap()
                .into_value()["isError"],
            true,
            "{name}"
        );
    }
    for (name, args) in [
        ("notes_create", json!({"title":"missing content"})),
        (
            "recording_start",
            json!({"provider":"local","system_audio":"yes"}),
        ),
        ("notes_export", json!({"id":1,"format":"html"})),
        ("app_status", json!({"secret":"invalid"})),
    ] {
        assert!(kw_mcp::tools::call(&harness.client, name, &args)
            .await
            .is_err());
    }
    harness.stop().await;
}
use tokio::task::JoinHandle;

struct Harness {
    served: Served,
    client: Client,
    _db: Db,
}

impl Harness {
    async fn start() -> Self {
        Self::with_db(Db::open_in_memory().await.unwrap()).await
    }

    async fn with_db(db: Db) -> Self {
        let token = LocalApiToken::generate().unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let served = serve(listener, ApiState::new(db.pool().clone(), token.clone())).unwrap();
        let client = Client::new(served.port(), token);
        Self {
            served,
            client,
            _db: db,
        }
    }

    async fn stop(self) {
        self.served.shutdown().await;
    }
}

/// One end of the in-memory pipe the server runs over, plus the task running
/// it — the same shape `main` gives `kw_mcp::server::run`, except both ends
/// live in this process.
struct Session {
    write: WriteHalf<DuplexStream>,
    read: BufReader<ReadHalf<DuplexStream>>,
    task: JoinHandle<std::io::Result<()>>,
}

impl Session {
    fn start(client: Client) -> Self {
        let (client_side, server_side) = tokio::io::duplex(64 * 1024);
        let (server_read, server_write) = tokio::io::split(server_side);
        let (client_read, client_write) = tokio::io::split(client_side);

        let task = tokio::spawn(kw_mcp::server::run(
            BufReader::new(server_read),
            server_write,
            client,
        ));

        Self {
            write: client_write,
            read: BufReader::new(client_read),
            task,
        }
    }

    async fn send(&mut self, request: &Value) {
        let mut line = serde_json::to_string(request).unwrap();
        line.push('\n');
        self.write.write_all(line.as_bytes()).await.unwrap();
    }

    async fn send_raw(&mut self, line: &str) {
        let mut owned = line.to_owned();
        owned.push('\n');
        self.write.write_all(owned.as_bytes()).await.unwrap();
    }

    /// Read exactly one JSON-RPC response line and parse it.
    async fn recv(&mut self) -> Value {
        let mut line = String::new();
        let read = self.read.read_line(&mut line).await.unwrap();
        assert!(read > 0, "the server closed its writer with nothing sent");
        serde_json::from_str(&line).unwrap_or_else(|err| panic!("{err}: {line:?}"))
    }

    /// Close the input side, and confirm the loop noticed EOF and returned
    /// `Ok` rather than erroring or hanging.
    async fn finish(mut self) {
        self.write.shutdown().await.unwrap();
        drop(self.write);
        let result = tokio::time::timeout(std::time::Duration::from_secs(5), self.task)
            .await
            .expect("the server loop should exit once stdin closes")
            .unwrap();
        assert!(result.is_ok(), "{result:?}");
    }
}

// -------------------------------------------------------------- initialize

#[tokio::test(flavor = "multi_thread")]
async fn the_initialize_handshake_answers_with_server_info_and_tool_capability() {
    let harness = Harness::start().await;
    let mut session = Session::start(harness.client.clone());

    session
        .send(&json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": { "protocolVersion": "2024-11-05", "capabilities": {}, "clientInfo": { "name": "test", "version": "0" } }
        }))
        .await;

    let response = session.recv().await;
    assert_eq!(response["id"], 1);
    assert_eq!(response["result"]["protocolVersion"], "2024-11-05");
    assert_eq!(
        response["result"]["serverInfo"]["name"],
        "knegiswhisper-mcp"
    );
    assert!(response["result"]["capabilities"]["tools"].is_object());
    assert!(response.get("error").is_none());

    // The client's follow-up notification: no id, so no response line.
    session
        .send(&json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }))
        .await;

    session.finish().await;
    harness.stop().await;
}

// ---------------------------------------------------------------- tools/list

#[tokio::test(flavor = "multi_thread")]
async fn tools_list_matches_the_local_apis_surface() {
    let harness = Harness::start().await;
    let mut session = Session::start(harness.client.clone());

    session
        .send(&json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list" }))
        .await;
    let response = session.recv().await;

    let names: Vec<String> = response["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tool| tool["name"].as_str().unwrap().to_owned())
        .collect();

    // One tool per route `crates/local-api/src/server.rs` actually serves —
    // nothing invented, nothing missing.
    let expected = [
        "notes_create",
        "transcription_create",
        "jobs_get",
        "jobs_cancel",
        "transcriptions_search",
        "notes_export",
        "recording_start",
        "recording_stop",
        "app_status",
        "models_list",
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
    assert_eq!(names, expected, "{names:?}");

    // Every tool declares a schema an MCP host can validate arguments
    // against.
    for tool in response["result"]["tools"].as_array().unwrap() {
        assert_eq!(tool["inputSchema"]["type"], "object");
    }

    session.finish().await;
    harness.stop().await;
}

// ---------------------------------------------------------------- tools/call

#[tokio::test(flavor = "multi_thread")]
async fn a_tools_call_round_trips_through_the_real_local_api() {
    let db = Db::open_in_memory().await.unwrap();
    let id = notes::create(
        db.pool(),
        &notes::NewNote {
            title: "Kickoff".to_owned(),
            content: "we discussed the warehouse".to_owned(),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let harness = Harness::with_db(db).await;
    let mut session = Session::start(harness.client.clone());

    session
        .send(&json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "tools/call",
            "params": { "name": "notes_get", "arguments": { "id": id } }
        }))
        .await;
    let response = session.recv().await;

    assert_eq!(response["id"], 3);
    assert_eq!(response["result"]["isError"], false);
    let text = response["result"]["content"][0]["text"].as_str().unwrap();
    let body: Value = serde_json::from_str(text).unwrap();
    assert_eq!(body["data"]["title"], "Kickoff");
    assert_eq!(body["data"]["content"], "we discussed the warehouse");

    // A note that does not exist reaches the local API and gets a real "no" —
    // reported as a tool result, not a protocol error.
    session
        .send(&json!({
            "jsonrpc": "2.0",
            "id": 4,
            "method": "tools/call",
            "params": { "name": "notes_get", "arguments": { "id": 999_999 } }
        }))
        .await;
    let missing = session.recv().await;
    assert_eq!(missing["result"]["isError"], true);
    assert!(missing.get("error").is_none());

    session.finish().await;
    harness.stop().await;
}

// ---------------------------------------------------------- permanent deletes

/// A database holding one dictation with a kept recording.
async fn db_with_a_dictation() -> (Db, i64) {
    let db = Db::open_in_memory().await.unwrap();
    let id = transcriptions::record(
        db.pool(),
        &transcriptions::NewTranscription {
            text: "keep this".to_owned(),
            has_audio: true,
            audio_duration_ms: Some(1000),
            provider: Some("local".to_owned()),
            ..Default::default()
        },
        transcriptions::Status::Completed,
    )
    .await
    .unwrap();
    (db, id)
}

fn tools_call(id: i64, name: &str, arguments: &Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "tools/call",
        "params": { "name": name, "arguments": arguments }
    })
}

/// Anything but a literal `true` is refused before a request is made, so the
/// dictation and its recording are both still there afterwards.
#[tokio::test(flavor = "multi_thread")]
async fn a_permanent_delete_without_confirm_true_changes_nothing() {
    let (db, id) = db_with_a_dictation().await;
    let harness = Harness::with_db(db).await;
    let mut session = Session::start(harness.client.clone());

    let mut request_id = 10;
    for tool in ["transcriptions_delete", "transcriptions_forget_audio"] {
        for arguments in [
            json!({ "id": id }),
            json!({ "id": id, "confirm": false }),
            json!({ "id": id, "confirm": "true" }),
            json!({ "id": id, "confirm": 1 }),
            json!({ "id": id, "confirm": null }),
        ] {
            request_id += 1;
            session
                .send(&tools_call(request_id, tool, &arguments))
                .await;
            let response = session.recv().await;
            assert_eq!(
                response["error"]["code"], -32602,
                "{tool} {arguments}: {response}"
            );
            let message = response["error"]["message"].as_str().unwrap();
            assert!(message.contains(tool), "{message}");
            assert!(message.contains("confirm"), "{message}");
        }
    }

    let kept = transcriptions::get(harness._db.pool(), id)
        .await
        .unwrap()
        .expect("the dictation should still exist");
    assert_eq!(kept.text, "keep this");
    assert!(kept.has_audio, "the recording should still be recorded");

    session.finish().await;
    harness.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn forgetting_audio_with_confirm_true_goes_through_the_real_local_api() {
    let (db, id) = db_with_a_dictation().await;
    let harness = Harness::with_db(db).await;
    let mut session = Session::start(harness.client.clone());

    session
        .send(&tools_call(
            20,
            "transcriptions_forget_audio",
            &json!({ "id": id, "confirm": true }),
        ))
        .await;
    let response = session.recv().await;
    assert_eq!(response["result"]["isError"], false, "{response}");

    let kept = transcriptions::get(harness._db.pool(), id)
        .await
        .unwrap()
        .expect("forgetting the audio should keep the transcript");
    assert_eq!(kept.text, "keep this");
    assert!(!kept.has_audio, "the recording should be gone");

    session.finish().await;
    harness.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn deleting_a_dictation_with_confirm_true_goes_through_the_real_local_api() {
    let (db, id) = db_with_a_dictation().await;
    let harness = Harness::with_db(db).await;
    let mut session = Session::start(harness.client.clone());

    session
        .send(&tools_call(
            21,
            "transcriptions_delete",
            &json!({ "id": id, "confirm": true }),
        ))
        .await;
    let response = session.recv().await;
    assert_eq!(response["result"]["isError"], false, "{response}");

    let gone = transcriptions::get(harness._db.pool(), id).await.unwrap();
    assert!(gone.is_none(), "the dictation should be deleted");

    session.finish().await;
    harness.stop().await;
}

/// Trash already makes this one undoable, so it stays a plain call.
#[tokio::test(flavor = "multi_thread")]
async fn moving_a_note_to_trash_still_needs_no_confirmation() {
    let db = Db::open_in_memory().await.unwrap();
    let id = notes::create(
        db.pool(),
        &notes::NewNote {
            title: "Kickoff".to_owned(),
            content: "we discussed the warehouse".to_owned(),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let harness = Harness::with_db(db).await;
    let mut session = Session::start(harness.client.clone());

    session
        .send(&tools_call(22, "notes_delete", &json!({ "id": id })))
        .await;
    let response = session.recv().await;
    assert_eq!(response["result"]["isError"], false, "{response}");

    session.finish().await;
    harness.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn only_the_permanent_tools_declare_confirm_and_a_destructive_hint() {
    let harness = Harness::start().await;
    let mut session = Session::start(harness.client.clone());

    session
        .send(&json!({ "jsonrpc": "2.0", "id": 23, "method": "tools/list" }))
        .await;
    let response = session.recv().await;
    let tools = response["result"]["tools"].as_array().unwrap();

    for tool in tools {
        let name = tool["name"].as_str().unwrap();
        if ["transcriptions_delete", "transcriptions_forget_audio"].contains(&name) {
            assert_eq!(tool["inputSchema"]["required"], json!(["id", "confirm"]));
            assert_eq!(tool["inputSchema"]["properties"]["confirm"]["const"], true);
            assert_eq!(tool["inputSchema"]["additionalProperties"], false);
            assert_eq!(tool["annotations"]["destructiveHint"], true);
            assert_eq!(tool["annotations"]["readOnlyHint"], false);
            let description = tool["description"].as_str().unwrap();
            assert!(description.contains("Permanently"), "{description}");
            assert!(description.contains("ask the person"), "{description}");
        } else {
            assert!(tool.get("annotations").is_none(), "{name}");
            assert!(
                tool["inputSchema"]["properties"].get("confirm").is_none(),
                "{name}"
            );
        }
    }

    session.finish().await;
    harness.stop().await;
}

// ------------------------------------------------------------- malformed input

#[tokio::test(flavor = "multi_thread")]
async fn a_malformed_request_gets_a_json_rpc_error_rather_than_a_panic() {
    let harness = Harness::start().await;
    let mut session = Session::start(harness.client.clone());

    session.send_raw("not json at all").await;
    let response = session.recv().await;
    assert_eq!(response["error"]["code"], -32700);
    assert!(response["id"].is_null());

    session.send(&json!({ "jsonrpc": "2.0", "id": 5 })).await;
    let response = session.recv().await;
    assert_eq!(response["error"]["code"], -32600);
    assert_eq!(response["id"], 5);

    // The loop must still be alive and answering after both malformed lines.
    session
        .send(&json!({ "jsonrpc": "2.0", "id": 6, "method": "tools/list" }))
        .await;
    let response = session.recv().await;
    assert!(response["result"]["tools"].is_array());

    session.finish().await;
    harness.stop().await;
}

// -------------------------------------------------------- stdout is protocol only

#[tokio::test(flavor = "multi_thread")]
async fn nothing_but_protocol_ever_reaches_the_writer() {
    // The one property the brief calls out as most worth locking down: every
    // byte written to the writer must be a newline-terminated JSON-RPC
    // response, for a mix of well-formed, malformed, and notification lines
    // that each exercise a different path through `handle_line`.
    let harness = Harness::start().await;
    let mut session = Session::start(harness.client.clone());

    session.send_raw("{{{ not json").await;
    session
        .send(&json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }))
        .await;
    session
        .send(&json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {} }))
        .await;
    session
        .send(&json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list" }))
        .await;
    session
        .send(&json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "tools/call",
            "params": { "name": "not_a_real_tool" }
        }))
        .await;
    session
        .send(&json!({ "jsonrpc": "2.0", "id": 4, "method": "unknown/method" }))
        .await;

    // Three ids expect a response (1, 2, 3, 4) plus the parse-error line: five
    // lines total. The notification gets none.
    let mut lines = Vec::new();
    for _ in 0..5 {
        let mut raw = String::new();
        session.read.read_line(&mut raw).await.unwrap();
        lines.push(raw);
    }

    for raw in &lines {
        assert!(raw.ends_with('\n'), "{raw:?} is not newline-terminated");
        let value: Value = serde_json::from_str(raw)
            .unwrap_or_else(|err| panic!("non-protocol byte reached the writer: {err}: {raw:?}"));
        assert_eq!(value["jsonrpc"], "2.0");
        assert!(
            value.get("result").is_some() || value.get("error").is_some(),
            "{raw:?} has neither result nor error"
        );
    }

    session.finish().await;
    harness.stop().await;
}
