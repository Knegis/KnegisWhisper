//! The local API, served for real over a loopback socket.
//!
//! Not `#[ignore]`d: it binds an ephemeral loopback port and uses an in-memory
//! database, so it needs no network, no credentials, and nothing installed. The
//! properties being checked — that an unauthenticated caller learns nothing,
//! that a non-loopback listener is refused, that a body cap is enforced — cannot
//! be checked without a real socket, which is exactly why they are worth a real
//! socket.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use kw_local_api::{serve, ApiState, ServeError, Served};
use kw_notes_core::notes;
use kw_security::LocalApiToken;
use kw_storage::transcriptions;
use kw_storage::Db;
use tokio::net::TcpListener;

struct Harness {
    served: Served,
    token: LocalApiToken,
    client: reqwest::Client,
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

        Self {
            served,
            token,
            client: reqwest::Client::new(),
            _db: db,
        }
    }

    fn url(&self, path: &str) -> String {
        format!("http://127.0.0.1:{}{path}", self.served.port())
    }

    async fn get(&self, path: &str) -> reqwest::Response {
        self.client
            .get(self.url(path))
            .header("Authorization", self.token.header_value())
            .send()
            .await
            .unwrap()
    }

    async fn unauthenticated(&self, path: &str) -> reqwest::Response {
        self.client.get(self.url(path)).send().await.unwrap()
    }

    async fn send(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<&str>,
    ) -> reqwest::Response {
        let mut request = self
            .client
            .request(method, self.url(path))
            .header("Authorization", self.token.header_value());
        if let Some(body) = body {
            request = request
                .header("Content-Type", "application/json")
                .body(body.to_owned());
        }
        request.send().await.unwrap()
    }

    async fn stop(self) {
        self.served.shutdown().await;
    }
}

// ------------------------------------------------------------------------ bind

#[tokio::test]
async fn a_non_loopback_listener_is_refused() {
    // The guarantee this crate owns. Binding 0.0.0.0 would put an
    // unauthenticated-by-transport API with a token in a home-directory file on
    // the network.
    let listener = TcpListener::bind("0.0.0.0:0").await.unwrap();
    let db = Db::open_in_memory().await.unwrap();
    let state = ApiState::new(db.pool().clone(), LocalApiToken::generate().unwrap());

    let err = serve(listener, state).unwrap_err();
    assert!(matches!(err, ServeError::NotLoopback { .. }));
    assert!(err.to_string().contains("not safe to expose"));
}

#[tokio::test]
async fn a_loopback_listener_serves() {
    let harness = Harness::start().await;
    let response = harness.get("/v1/health").await;
    assert_eq!(response.status(), 200);

    let body: serde_json::Value = response.json().await.unwrap();
    assert_eq!(body["data"]["ok"], true);
    assert_eq!(body["data"]["version"], 1);
    assert!(!body["data"]["appVersion"].as_str().unwrap().is_empty());

    harness.stop().await;
}

// ------------------------------------------------------------------------ auth

#[tokio::test]
async fn every_route_needs_the_token() {
    let harness = Harness::start().await;

    for path in [
        "/v1/health",
        "/v1/notes/list",
        "/v1/notes/search?q=x",
        "/v1/notes/1",
        "/v1/folders/list",
        "/v1/dictionary/list",
        "/v1/transcriptions/list",
        "/v1/transcriptions/1",
    ] {
        let response = harness.unauthenticated(path).await;
        assert_eq!(response.status(), 401, "{path} answered without a token");
    }

    harness.stop().await;
}

#[tokio::test]
async fn an_unauthenticated_caller_cannot_learn_which_routes_exist() {
    // A 404 on an unknown path and a 401 on a real one would map the surface for
    // anyone who can reach the port. Both answer 401.
    let harness = Harness::start().await;

    let real = harness.unauthenticated("/v1/notes/list").await;
    let invented = harness.unauthenticated("/v1/admin/secrets").await;

    assert_eq!(real.status(), 401);
    assert_eq!(
        invented.status(),
        401,
        "the fallback must also require auth"
    );

    harness.stop().await;
}

#[tokio::test]
async fn a_wrong_token_is_refused_and_the_error_says_nothing_useful() {
    let harness = Harness::start().await;
    let other = LocalApiToken::generate().unwrap();

    let response = harness
        .client
        .get(harness.url("/v1/health"))
        .header("Authorization", other.header_value())
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 401);
    let body: serde_json::Value = response.json().await.unwrap();
    assert_eq!(body["error"]["code"], "unauthorized");
    // Not "wrong token" versus "no token" -- the same answer either way.
    assert!(!body["error"]["message"]
        .as_str()
        .unwrap()
        .to_lowercase()
        .contains("wrong"));

    harness.stop().await;
}

#[tokio::test]
async fn a_bare_token_without_the_bearer_scheme_is_refused() {
    let harness = Harness::start().await;
    let response = harness
        .client
        .get(harness.url("/v1/health"))
        .header("Authorization", harness.token.expose())
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 401);
    harness.stop().await;
}

// ----------------------------------------------------------------------- notes

#[tokio::test]
async fn notes_are_listed_read_patched_and_deleted() {
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

    // List
    let body: serde_json::Value = harness.get("/v1/notes/list").await.json().await.unwrap();
    assert_eq!(body["data"].as_array().unwrap().len(), 1);
    assert_eq!(body["has_more"], false);
    assert_eq!(body["next_cursor"], serde_json::Value::Null);

    // Read
    let body: serde_json::Value = harness
        .get(&format!("/v1/notes/{id}"))
        .await
        .json()
        .await
        .unwrap();
    assert_eq!(body["data"]["title"], "Kickoff");

    // Patch one field, and get the whole note back rather than the patch.
    let response = harness
        .send(
            reqwest::Method::PATCH,
            &format!("/v1/notes/{id}"),
            Some(r#"{"title":"Kickoff (revised)"}"#),
        )
        .await;
    assert_eq!(response.status(), 200);
    let body: serde_json::Value = response.json().await.unwrap();
    assert_eq!(body["data"]["title"], "Kickoff (revised)");
    assert_eq!(
        body["data"]["content"], "we discussed the warehouse",
        "a partial patch must not blank the fields it did not mention"
    );

    // Search finds the patched note.
    let body: serde_json::Value = harness
        .get("/v1/notes/search?q=warehouse")
        .await
        .json()
        .await
        .unwrap();
    assert_eq!(body["data"].as_array().unwrap().len(), 1);

    // Delete: the note goes to Trash, and the API treats it as gone (KNEG-401).
    let response = harness
        .send(reqwest::Method::DELETE, &format!("/v1/notes/{id}"), None)
        .await;
    assert_eq!(response.status(), 204);

    let response = harness.get(&format!("/v1/notes/{id}")).await;
    assert_eq!(response.status(), 404);

    harness.stop().await;
}

#[tokio::test]
async fn has_more_is_computed_rather_than_guessed() {
    // Upstream always answers has_more: false. A caller that trusts it stops
    // paging early.
    let db = Db::open_in_memory().await.unwrap();
    for index in 0..5 {
        notes::create(
            db.pool(),
            &notes::NewNote {
                title: format!("note {index}"),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    }

    let harness = Harness::with_db(db).await;

    let body: serde_json::Value = harness
        .get("/v1/notes/list?limit=2")
        .await
        .json()
        .await
        .unwrap();
    assert_eq!(body["data"].as_array().unwrap().len(), 2);
    assert_eq!(body["has_more"], true);

    let body: serde_json::Value = harness
        .get("/v1/notes/list?limit=50")
        .await
        .json()
        .await
        .unwrap();
    assert_eq!(body["data"].as_array().unwrap().len(), 5);
    assert_eq!(body["has_more"], false);

    harness.stop().await;
}

#[tokio::test]
async fn an_unsupported_cursor_on_notes_is_refused_rather_than_ignored() {
    // Accepting it and doing nothing is how a caller comes to trust a cursor
    // that pages nothing.
    let harness = Harness::start().await;
    let response = harness.get("/v1/notes/list?after=5").await;
    assert_eq!(response.status(), 400);

    let body: serde_json::Value = response.json().await.unwrap();
    assert_eq!(body["error"]["code"], "validation_error");
    assert!(body["error"]["message"]
        .as_str()
        .unwrap()
        .contains("not supported for notes"));

    harness.stop().await;
}

#[tokio::test]
async fn a_patch_that_changes_nothing_is_refused() {
    let harness = Harness::start().await;
    let response = harness
        .send(reqwest::Method::PATCH, "/v1/notes/1", Some("{}"))
        .await;
    assert_eq!(response.status(), 400);
    harness.stop().await;
}

#[tokio::test]
async fn a_patch_with_an_unknown_field_is_refused() {
    let harness = Harness::start().await;
    let response = harness
        .send(
            reqwest::Method::PATCH,
            "/v1/notes/1",
            Some(r#"{"titel":"typo"}"#),
        )
        .await;
    assert_eq!(
        response.status(),
        422,
        "axum rejects the body before we see it"
    );
    harness.stop().await;
}

#[tokio::test]
async fn a_search_without_a_query_is_refused() {
    let harness = Harness::start().await;
    let response = harness.get("/v1/notes/search").await;
    assert_eq!(response.status(), 400);
    harness.stop().await;
}

#[tokio::test]
async fn a_non_numeric_id_is_a_404_not_a_crash() {
    let harness = Harness::start().await;
    for path in ["/v1/notes/abc", "/v1/notes/0", "/v1/transcriptions/-1"] {
        assert_eq!(harness.get(path).await.status(), 404, "{path}");
    }
    harness.stop().await;
}

// -------------------------------------------------------------- transcriptions

#[tokio::test]
async fn transcriptions_page_by_cursor() {
    let db = Db::open_in_memory().await.unwrap();
    for text in ["one", "two", "three", "four"] {
        transcriptions::record(
            db.pool(),
            &transcriptions::NewTranscription {
                text: text.to_owned(),
                ..Default::default()
            },
            transcriptions::Status::Completed,
        )
        .await
        .unwrap();
    }

    let harness = Harness::with_db(db).await;

    let first: serde_json::Value = harness
        .get("/v1/transcriptions/list?limit=2")
        .await
        .json()
        .await
        .unwrap();
    assert_eq!(first["has_more"], true);
    let cursor = first["next_cursor"].as_i64().expect("a cursor");

    let second: serde_json::Value = harness
        .get(&format!("/v1/transcriptions/list?limit=2&after={cursor}"))
        .await
        .json()
        .await
        .unwrap();
    assert_eq!(second["has_more"], false);

    let texts: Vec<&str> = second["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["text"].as_str().unwrap())
        .collect();
    assert_eq!(texts, ["two", "one"]);

    harness.stop().await;
}

#[tokio::test]
async fn deleting_a_recording_is_idempotent() {
    // The user asked for the recording to be gone. Answering 404 the second time
    // would suggest it is still there.
    let db = Db::open_in_memory().await.unwrap();
    let id = transcriptions::record(
        db.pool(),
        &transcriptions::NewTranscription {
            text: "spoken".to_owned(),
            has_audio: true,
            audio_duration_ms: Some(3000),
            ..Default::default()
        },
        transcriptions::Status::Completed,
    )
    .await
    .unwrap();

    let harness = Harness::with_db(db).await;
    let path = format!("/v1/transcriptions/{id}/audio");

    assert_eq!(
        harness
            .send(reqwest::Method::DELETE, &path, None)
            .await
            .status(),
        204
    );
    assert_eq!(
        harness
            .send(reqwest::Method::DELETE, &path, None)
            .await
            .status(),
        204
    );

    // But a row that never existed is still a 404.
    assert_eq!(
        harness
            .send(
                reqwest::Method::DELETE,
                "/v1/transcriptions/999/audio",
                None
            )
            .await
            .status(),
        404
    );

    // The transcript itself survives; only the recording went.
    let body: serde_json::Value = harness
        .get(&format!("/v1/transcriptions/{id}"))
        .await
        .json()
        .await
        .unwrap();
    assert_eq!(body["data"]["text"], "spoken");
    assert_eq!(body["data"]["hasAudio"], false);

    harness.stop().await;
}

// ------------------------------------------------------------------ dictionary

#[tokio::test]
async fn the_dictionary_is_listed_and_updated() {
    let harness = Harness::start().await;

    let response = harness
        .send(
            reqwest::Method::POST,
            "/v1/dictionary/update",
            Some(r#"{"add":["Knegis","Karolinska"]}"#),
        )
        .await;
    assert_eq!(response.status(), 200);
    let body: serde_json::Value = response.json().await.unwrap();
    assert_eq!(body["data"]["added"], 2);

    let body: serde_json::Value = harness
        .get("/v1/dictionary/list")
        .await
        .json()
        .await
        .unwrap();
    let words: Vec<&str> = body["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|w| w["word"].as_str().unwrap())
        .collect();
    assert_eq!(words, ["Karolinska", "Knegis"]);

    // A repeat reports nothing new rather than failing.
    let body: serde_json::Value = harness
        .send(
            reqwest::Method::POST,
            "/v1/dictionary/update",
            Some(r#"{"add":["Knegis"]}"#),
        )
        .await
        .json()
        .await
        .unwrap();
    assert_eq!(body["data"]["added"], 0);
    assert_eq!(body["data"]["alreadyPresent"], 1);

    harness.stop().await;
}

#[tokio::test]
async fn a_contradictory_dictionary_update_is_refused() {
    let harness = Harness::start().await;
    let response = harness
        .send(
            reqwest::Method::POST,
            "/v1/dictionary/update",
            Some(r#"{"add":["x"],"remove":["x"]}"#),
        )
        .await;
    assert_eq!(response.status(), 400);
    harness.stop().await;
}

// ------------------------------------------------------------------- limits

#[tokio::test]
async fn an_oversized_body_is_rejected_rather_than_buffered() {
    // Without a cap, anything that can reach the port can make this process
    // allocate without bound.
    let harness = Harness::start().await;

    let huge = format!(
        r#"{{"add":["{}"]}}"#,
        "a".repeat(kw_local_api::MAX_BODY_BYTES + 1024)
    );
    let response = harness
        .send(reqwest::Method::POST, "/v1/dictionary/update", Some(&huge))
        .await;

    assert!(
        response.status() == 413 || response.status() == 400,
        "expected the body cap to bite, got {}",
        response.status()
    );

    harness.stop().await;
}

#[tokio::test]
async fn an_enormous_limit_is_clamped_rather_than_refused() {
    let harness = Harness::start().await;
    let response = harness.get("/v1/notes/list?limit=999999").await;
    assert_eq!(response.status(), 200);
    harness.stop().await;
}

#[tokio::test]
async fn the_server_stops_when_asked() {
    let harness = Harness::start().await;
    let url = harness.url("/v1/health");
    let client = harness.client.clone();
    let header = harness.token.header_value();

    assert!(client
        .get(&url)
        .header("Authorization", &header)
        .send()
        .await
        .is_ok());

    harness.stop().await;

    // Nothing is listening any more.
    assert!(client
        .get(&url)
        .header("Authorization", &header)
        .send()
        .await
        .is_err());
}
