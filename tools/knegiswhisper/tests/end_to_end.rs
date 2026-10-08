//! The compiled `knegiswhisper` binary, driven against a real server.
//!
//! Not `#[ignore]`d: the server binds an ephemeral loopback port over an
//! in-memory database, so this needs nothing installed and no network.
//!
//! It exists for one reason the unit tests cannot cover: **exit codes are only
//! real when a process exits with them.** Everything else about the CLI is
//! tested as functions; a script, though, branches on `$?`, and that comes from
//! `main` — the one place with no unit test.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::path::{Path, PathBuf};
use std::process::Output;

use kw_local_api::{discovery, serve, ApiState, Served};
use kw_notes_core::notes;
use kw_security::LocalApiToken;
use kw_storage::transcriptions;
use kw_storage::Db;
use tokio::net::TcpListener;

#[tokio::test(flavor = "multi_thread")]
async fn automation_commands_create_export_and_refuse_unavailable_runtime() {
    let harness = Harness::start().await;
    let out = harness.run(&[
        "notes",
        "create",
        "--title",
        "Meeting",
        "--content",
        "**Budget**",
        "--json",
    ]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let note: serde_json::Value = serde_json::from_str(&stdout(&out)).unwrap();
    let id = note["data"]["id"].to_string();
    let out = harness.run(&["notes", "export", &id]);
    assert_eq!(code(&out), 0);
    assert!(stdout(&out).contains("**Budget**"));
    let path = harness._dir.path().join("note.md");
    let name = path.to_str().unwrap();
    assert_eq!(
        code(&harness.run(&["notes", "export", &id, "--output", name])),
        0
    );
    assert_eq!(
        code(&harness.run(&["notes", "export", &id, "--output", name])),
        1
    );
    assert_eq!(
        code(&harness.run(&["notes", "export", &id, "--output", name, "--force"])),
        0
    );
    assert_eq!(
        std::fs::read_to_string(path).unwrap(),
        "# Meeting\n\n**Budget**\n"
    );
    assert_eq!(
        code(&harness.run(&["history", "search", "budget", "--json"])),
        0
    );
    for argv in [
        vec!["status"],
        vec!["models", "list"],
        vec!["recording", "start", "--provider", "local"],
        vec!["recording", "stop"],
        vec!["jobs", "cancel", "unknown"],
    ] {
        let out = harness.run(&argv);
        assert_eq!(code(&out), 1, "{argv:?}: {}", stderr(&out));
        assert!(stdout(&out).is_empty());
    }
    assert_eq!(code(&harness.run(&["jobs", "status", "unknown"])), 3);
    assert_eq!(
        code(&harness.run(&["transcribe", "file", "missing.wav"])),
        2
    );
    harness.stop().await;
}

struct Harness {
    served: Served,
    discovery_path: PathBuf,
    _dir: tempfile::TempDir,
    _db: Db,
}

impl Harness {
    async fn start() -> Self {
        let db = Db::open_in_memory().await.unwrap();
        Self::with_db(db).await
    }

    async fn with_db(db: Db) -> Self {
        let token = LocalApiToken::generate().unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let served = serve(listener, ApiState::new(db.pool().clone(), token.clone())).unwrap();

        let dir = tempfile::tempdir().unwrap();
        let discovery_path = dir.path().join("local-api.json");
        discovery::write(&discovery_path, served.port(), &token).unwrap();

        Self {
            served,
            discovery_path,
            _dir: dir,
            _db: db,
        }
    }

    /// Run the real binary against this server.
    fn run(&self, args: &[&str]) -> Output {
        run_with_discovery(&self.discovery_path, args)
    }

    async fn stop(self) {
        self.served.shutdown().await;
    }
}

fn run_with_discovery(discovery_path: &Path, args: &[&str]) -> Output {
    std::process::Command::new(env!("CARGO_BIN_EXE_knegiswhisper"))
        .arg("--discovery")
        .arg(discovery_path)
        .args(args)
        .output()
        .expect("the binary should run")
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn code(output: &Output) -> i32 {
    output.status.code().expect("an exit code, not a signal")
}

// -------------------------------------------------------------- exit codes

#[tokio::test(flavor = "multi_thread")]
async fn success_exits_zero() {
    let harness = Harness::start().await;
    let output = harness.run(&["health"]);

    assert_eq!(code(&output), 0, "stderr: {}", stderr(&output));
    assert!(stdout(&output).contains("is running"));

    harness.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_missing_thing_exits_three() {
    let harness = Harness::start().await;
    let output = harness.run(&["notes", "get", "999"]);

    assert_eq!(code(&output), 3, "stderr: {}", stderr(&output));
    assert!(stderr(&output).contains("not found"), "{}", stderr(&output));

    harness.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_server_side_refusal_exits_one() {
    // A limit of 0 is refused by the API as a probable caller bug. It reaches
    // the server, so it is a failure rather than a usage error.
    let harness = Harness::start().await;
    let output = harness.run(&["notes", "list", "--limit", "0"]);

    assert_eq!(code(&output), 1, "stderr: {}", stderr(&output));
    assert!(stderr(&output).contains("limit"), "{}", stderr(&output));

    harness.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_bad_command_line_exits_two() {
    let harness = Harness::start().await;

    for args in [
        vec!["notez", "list"],
        vec!["notes", "list", "--limitt", "5"],
        vec!["notes", "get", "not-a-number"],
    ] {
        let output = harness.run(&args);
        assert_eq!(code(&output), 2, "{args:?} — {}", stderr(&output));
    }

    // And a usage error this crate raises itself, rather than clap.
    let output = harness.run(&["notes", "set", "1"]);
    assert_eq!(code(&output), 2);
    assert!(stderr(&output).contains("--title"), "{}", stderr(&output));

    harness.stop().await;
}

#[test]
fn a_missing_discovery_file_exits_four_and_says_to_start_it() {
    // The most common failure, and the only one with an obvious fix — which is
    // why it has its own code rather than sharing 1.
    let dir = tempfile::tempdir().unwrap();
    let output = run_with_discovery(&dir.path().join("absent.json"), &["health"]);

    assert_eq!(code(&output), 4);
    let message = stderr(&output);
    assert!(
        message.contains("does not appear to be running"),
        "{message}"
    );
    assert!(message.contains("Start it"), "{message}");
}

#[test]
fn a_corrupt_discovery_file_exits_four_and_says_to_restart() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("local-api.json");
    std::fs::write(&path, "{ not json at all").unwrap();

    let output = run_with_discovery(&path, &["health"]);
    assert_eq!(code(&output), 4);
    assert!(
        stderr(&output).contains("restarting it will rewrite"),
        "{}",
        stderr(&output)
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_stale_token_exits_one_rather_than_four() {
    // The application is running, so "not running" would be wrong. A token that
    // no longer matches means restart, and the message says so.
    let harness = Harness::start().await;
    discovery::write(
        &harness.discovery_path,
        harness.served.port(),
        &LocalApiToken::generate().unwrap(),
    )
    .unwrap();

    let output = harness.run(&["health"]);
    assert_eq!(code(&output), 1, "stderr: {}", stderr(&output));
    assert!(stderr(&output).contains("Restart"), "{}", stderr(&output));

    harness.stop().await;
}

#[test]
fn a_discovery_file_pointing_at_nothing_exits_four() {
    // The application stopped without removing its file. "Not running" is
    // exactly right, and it must not surface as a raw connection error.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("local-api.json");
    // Port 9 is the discard service, which nothing listens on locally.
    discovery::write(&path, 9, &LocalApiToken::generate().unwrap()).unwrap();

    let output = run_with_discovery(&path, &["health"]);
    assert_eq!(code(&output), 4, "stderr: {}", stderr(&output));
}

// ------------------------------------------------------------------ output

#[tokio::test(flavor = "multi_thread")]
async fn notes_are_listed_read_changed_and_deleted_through_the_binary() {
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

    let listed = harness.run(&["notes", "list"]);
    assert_eq!(code(&listed), 0);
    assert!(stdout(&listed).contains("Kickoff"));

    let shown = harness.run(&["notes", "get", &id.to_string()]);
    assert!(stdout(&shown).contains("we discussed the warehouse"));

    let changed = harness.run(&["notes", "set", &id.to_string(), "--title", "Kickoff II"]);
    assert_eq!(code(&changed), 0, "stderr: {}", stderr(&changed));
    assert!(stdout(&changed).contains("Kickoff II"));
    assert!(
        stdout(&changed).contains("we discussed the warehouse"),
        "a partial change must not blank the content"
    );

    let found = harness.run(&["notes", "search", "warehouse"]);
    assert!(stdout(&found).contains("Kickoff II"));

    // KNEG-401: a delete moves the note to Trash in the app, and says so.
    let deleted = harness.run(&["notes", "delete", &id.to_string()]);
    assert_eq!(code(&deleted), 0);
    assert!(
        stdout(&deleted).contains(&format!("moved note {id} to Trash")),
        "stdout: {}",
        stdout(&deleted)
    );

    // From then on the API treats it as gone.
    assert_eq!(code(&harness.run(&["notes", "get", &id.to_string()])), 3);
    assert!(!stdout(&harness.run(&["notes", "list"])).contains("Kickoff"));

    harness.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_multi_word_search_needs_no_quoting() {
    let db = Db::open_in_memory().await.unwrap();
    notes::create(
        db.pool(),
        &notes::NewNote {
            title: "March plan".to_owned(),
            content: "the warehouse in March".to_owned(),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    notes::create(
        db.pool(),
        &notes::NewNote {
            title: "Other".to_owned(),
            content: "the warehouse only".to_owned(),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let harness = Harness::with_db(db).await;

    let one_word = harness.run(&["notes", "search", "warehouse", "--json"]);
    let parsed: serde_json::Value = serde_json::from_str(&stdout(&one_word)).unwrap();
    assert_eq!(parsed["data"].as_array().unwrap().len(), 2);

    // Two bare words narrow the search, without the user quoting anything.
    let two_words = harness.run(&["notes", "search", "warehouse", "March", "--json"]);
    let parsed: serde_json::Value = serde_json::from_str(&stdout(&two_words)).unwrap();
    assert_eq!(parsed["data"].as_array().unwrap().len(), 1);

    harness.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_search_with_fts_syntax_in_it_does_not_fail() {
    // "C++ (draft)" is a syntax error in raw FTS5. The escaping happens in
    // notes-core; this checks it survives the whole round trip.
    let harness = Harness::start().await;
    let output = harness.run(&["notes", "search", "C++ (draft)"]);
    assert_eq!(code(&output), 0, "stderr: {}", stderr(&output));
    harness.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn the_dictionary_is_managed_through_the_binary() {
    let harness = Harness::start().await;

    let added = harness.run(&["dictionary", "add", "Knegis", "Karolinska"]);
    assert_eq!(code(&added), 0, "stderr: {}", stderr(&added));
    assert!(stdout(&added).contains("added 2"));

    let again = harness.run(&["dictionary", "add", "Knegis"]);
    assert!(stdout(&again).contains("already there"));

    let listed = harness.run(&["dictionary", "list"]);
    assert!(stdout(&listed).contains("Karolinska"));

    let removed = harness.run(&["dictionary", "remove", "Knegis"]);
    assert!(stdout(&removed).contains("removed 1"));

    // No words at all is a usage error, named by flag rather than by JSON field.
    let empty = harness.run(&["dictionary", "add"]);
    assert_eq!(code(&empty), 2);

    harness.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn history_is_paged_and_a_recording_can_be_deleted_on_its_own() {
    let db = Db::open_in_memory().await.unwrap();
    let mut last = 0;
    for text in ["one", "two", "three"] {
        last = transcriptions::record(
            db.pool(),
            &transcriptions::NewTranscription {
                text: text.to_owned(),
                has_audio: true,
                audio_duration_ms: Some(1000),
                provider: Some("local".to_owned()),
                ..Default::default()
            },
            transcriptions::Status::Completed,
        )
        .await
        .unwrap();
    }

    let harness = Harness::with_db(db).await;

    let page = harness.run(&["history", "list", "--limit", "2", "--json"]);
    let parsed: serde_json::Value = serde_json::from_str(&stdout(&page)).unwrap();
    assert_eq!(parsed["has_more"], true);

    let human = harness.run(&["history", "list", "--limit", "2"]);
    assert!(stdout(&human).contains("there are more"));

    let one = harness.run(&["history", "get", &last.to_string()]);
    assert_eq!(stdout(&one).trim(), "three");

    let forgotten = harness.run(&["history", "forget-audio", &last.to_string()]);
    assert_eq!(code(&forgotten), 0);
    assert!(stdout(&forgotten).contains("transcript is still there"));

    // The transcript survived; only the recording went.
    let still_there = harness.run(&["history", "get", &last.to_string()]);
    assert_eq!(stdout(&still_there).trim(), "three");

    harness.stop().await;
}

/// A failed dictation reaches the terminal as a failure (KNEG-36).
///
/// The renderer's own tests build the JSON by hand, which proves the table can
/// show a status and a reason and says nothing about whether the server sends
/// them. This drives the real `/v1/transcriptions/list` over a real socket, so
/// the two halves of the claim -- the API has the information, the CLI shows it
/// -- are checked against each other rather than separately.
#[tokio::test(flavor = "multi_thread")]
async fn a_failed_dictation_is_visible_from_the_command_line() {
    let db = Db::open_in_memory().await.unwrap();

    transcriptions::record(
        db.pool(),
        &transcriptions::NewTranscription {
            text: "the quick brown fox".to_owned(),
            provider: Some("local".to_owned()),
            ..Default::default()
        },
        transcriptions::Status::Completed,
    )
    .await
    .unwrap();

    // No provider and no text, which is what made this row two dashes.
    let failed = transcriptions::record(
        db.pool(),
        &transcriptions::NewTranscription {
            text: String::new(),
            error_message: Some("provider returned 429".to_owned()),
            ..Default::default()
        },
        transcriptions::Status::Failed,
    )
    .await
    .unwrap();

    let harness = Harness::with_db(db).await;

    let listed = stdout(&harness.run(&["history", "list"]));
    assert!(listed.contains("failed"), "{listed}");
    assert!(listed.contains("provider returned 429"), "{listed}");
    // The successful row is still a successful row, not collateral.
    assert!(listed.contains("the quick brown fox"), "{listed}");

    // And the single-row command no longer answers with a blank line.
    let one = stdout(&harness.run(&["history", "get", &failed.to_string()]));
    assert!(!one.trim().is_empty(), "an empty answer is the defect");
    assert!(one.contains("provider returned 429"), "{one}");

    harness.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn json_output_is_the_servers_answer_and_nothing_else() {
    // A script must be able to pipe this straight into a JSON parser, with no
    // banner, no progress line, and nothing on stdout but the document.
    let harness = Harness::start().await;
    let output = harness.run(&["health", "--json"]);

    let text = stdout(&output);
    let parsed: serde_json::Value =
        serde_json::from_str(&text).expect("stdout should be exactly one JSON document");
    assert_eq!(parsed["data"]["ok"], true);

    harness.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn errors_go_to_stderr_and_leave_stdout_empty() {
    // Otherwise a script doing `knegiswhisper ... --json | jq` gets prose in the
    // pipe on failure.
    let harness = Harness::start().await;
    let output = harness.run(&["notes", "get", "999", "--json"]);

    assert_eq!(code(&output), 3);
    assert!(stdout(&output).trim().is_empty(), "{}", stdout(&output));
    assert!(!stderr(&output).trim().is_empty());

    harness.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn an_empty_list_says_so_rather_than_printing_nothing() {
    let harness = Harness::start().await;
    let output = harness.run(&["notes", "list"]);
    assert_eq!(code(&output), 0);
    assert!(stdout(&output).contains("no notes"));
    harness.stop().await;
}
