#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
use kw_local_api::{
    runtime::{Runtime, Upload},
    serve, ApiError, ApiState,
};
use kw_security::LocalApiToken;
use kw_storage::Db;
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
struct FakeRuntime {
    paths: Mutex<Vec<std::path::PathBuf>>,
}

#[tokio::test]
async fn disconnected_upload_removes_partial_file_without_submitting_job() {
    use tokio::io::AsyncWriteExt as _;
    let db = Db::open_in_memory().await.unwrap();
    let dir = tempfile::tempdir().unwrap();
    let uploads = dir.path().join("uploads");
    let runtime = Arc::new(FakeRuntime {
        paths: Mutex::new(Vec::new()),
    });
    let token = LocalApiToken::generate().unwrap();
    let server = serve(
        tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap(),
        ApiState::new(db.pool().clone(), token.clone())
            .with_runtime(runtime.clone(), uploads.clone()),
    )
    .unwrap();
    let mut connection = tokio::net::TcpStream::connect(("127.0.0.1", server.port()))
        .await
        .unwrap();
    let request = format!("POST /v1/jobs/transcriptions HTTP/1.1\r\nHost: localhost\r\nAuthorization: {}\r\nContent-Type: multipart/form-data; boundary=smoke\r\nContent-Length: 100000\r\n\r\n--smoke\r\nContent-Disposition: form-data; name=\"file\"; filename=\"partial.wav\"\r\n\r\n", token.header_value());
    connection.write_all(request.as_bytes()).await.unwrap();
    connection.write_all(&vec![7; 4096]).await.unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            if std::fs::read_dir(&uploads).is_ok_and(|mut entries| entries.next().is_some()) {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    drop(connection);
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            if std::fs::read_dir(&uploads).unwrap().next().is_none() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert!(runtime.paths.lock().unwrap().is_empty());
    server.shutdown().await;
}
#[async_trait::async_trait]
impl Runtime for FakeRuntime {
    async fn transcribe(&self, upload: Upload) -> Result<Value, ApiError> {
        let bytes = tokio::fs::read(upload.path()).await.unwrap();
        self.paths.lock().unwrap().push(upload.path().to_owned());
        Ok(json!({"id":"fixture", "bytes":bytes.len()}))
    }
    async fn cancel(&self, id: &str) -> Result<Value, ApiError> {
        Ok(json!({"id":id,"cancellation_requested":true}))
    }
    async fn recording_start(&self, provider: &str, system_audio: bool) -> Result<Value, ApiError> {
        Ok(json!({"provider":provider,"system_audio":system_audio}))
    }
    async fn recording_stop(&self) -> Result<Value, ApiError> {
        Ok(json!({"stopped":true}))
    }
    async fn status(&self) -> Result<Value, ApiError> {
        Ok(json!({"state":"idle"}))
    }
    async fn models(&self) -> Result<Value, ApiError> {
        Ok(json!({"connectivity_verified":false}))
    }
}
#[tokio::test]
async fn uploads_stream_beyond_json_limit_and_do_not_use_client_paths() {
    let db = Db::open_in_memory().await.unwrap();
    let dir = tempfile::tempdir().unwrap();
    let runtime = Arc::new(FakeRuntime {
        paths: Mutex::new(Vec::new()),
    });
    let token = LocalApiToken::generate().unwrap();
    let server = serve(
        tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap(),
        ApiState::new(db.pool().clone(), token.clone())
            .with_runtime(runtime.clone(), dir.path().join("uploads")),
    )
    .unwrap();
    let base = format!("http://127.0.0.1:{}", server.port());
    let client = reqwest::Client::new();
    let upload = || {
        reqwest::multipart::Form::new().part(
            "file",
            reqwest::multipart::Part::bytes(vec![7; 2 * 1024 * 1024])
                .file_name("../../private.wav"),
        )
    };
    let denied = client
        .post(format!("{base}/v1/jobs/transcriptions"))
        .multipart(reqwest::multipart::Form::new().part(
            "file",
            reqwest::multipart::Part::bytes(vec![7; 16]).file_name("denied.wav"),
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(denied.status(), 401);
    assert!(!dir.path().join("uploads").exists());
    let response = client
        .post(format!("{base}/v1/jobs/transcriptions"))
        .header("Authorization", token.header_value())
        .multipart(upload())
        .send()
        .await
        .unwrap();
    let status = response.status();
    let body: Value = response.json().await.unwrap();
    assert_eq!(status, 202, "{body}");
    assert_eq!(body["data"]["bytes"], 2 * 1024 * 1024);
    let paths = runtime.paths.lock().unwrap().clone();
    assert_eq!(paths.len(), 1);
    assert_eq!(
        paths[0].parent(),
        Some(dir.path().join("uploads").as_path())
    );
    assert!(!paths[0].exists());
    assert_ne!(paths[0].file_name().unwrap(), "private.wav");
    for (method, path, body) in [
        (reqwest::Method::GET, "/v1/status", json!(null)),
        (reqwest::Method::GET, "/v1/models", json!(null)),
        (
            reqwest::Method::POST,
            "/v1/recording/start",
            json!({"provider":"local","system_audio":true}),
        ),
        (reqwest::Method::POST, "/v1/recording/stop", json!(null)),
        (
            reqwest::Method::POST,
            "/v1/jobs/fixture/cancel",
            json!(null),
        ),
    ] {
        assert_eq!(
            client
                .request(method.clone(), format!("{base}{path}"))
                .json(&body)
                .send()
                .await
                .unwrap()
                .status(),
            401
        );
        assert!(client
            .request(method, format!("{base}{path}"))
            .header("Authorization", token.header_value())
            .json(&body)
            .send()
            .await
            .unwrap()
            .status()
            .is_success());
    }
    for filename in ["private.txt", "empty.wav"] {
        let part = reqwest::multipart::Part::bytes(Vec::new()).file_name(filename);
        assert_eq!(
            client
                .post(format!("{base}/v1/jobs/transcriptions"))
                .header("Authorization", token.header_value())
                .multipart(reqwest::multipart::Form::new().part("file", part))
                .send()
                .await
                .unwrap()
                .status(),
            400
        );
    }
    let wrong = client
        .post(format!("{base}/v1/jobs/transcriptions"))
        .header("Authorization", token.header_value())
        .json(&json!({"path":"C:/private.wav"}))
        .send()
        .await
        .unwrap();
    assert!(!wrong.status().is_success());
    assert_eq!(
        std::fs::read_dir(dir.path().join("uploads"))
            .unwrap()
            .count(),
        0
    );
    server.shutdown().await;
}

#[tokio::test]
async fn create_export_search_and_durable_restart() {
    let db = Db::open_in_memory().await.unwrap();
    let token = LocalApiToken::generate().unwrap();
    let server = serve(
        tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap(),
        ApiState::new(db.pool().clone(), token.clone()),
    )
    .unwrap();
    let base = format!("http://127.0.0.1:{}", server.port());
    let client = reqwest::Client::new();
    let response = client
        .post(format!("{base}/v1/notes"))
        .header("Authorization", token.header_value())
        .json(&json!({"title":"Möte","content":"**Budget**","folder_id":null}))
        .send()
        .await
        .unwrap();
    assert!(response.status().is_success());
    let note: Value = response.json().await.unwrap();
    let id = note["data"]["id"].as_i64().unwrap();
    assert!(note["data"]["transcript"].is_null());
    let export: Value = client
        .get(format!("{base}/v1/notes/{id}/export?format=markdown"))
        .header("Authorization", token.header_value())
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(export["data"]["content"], "# Möte\n\n**Budget**\n");
    assert_eq!(
        client
            .get(format!(
                "{base}/v1/notes/{id}/export?format=srt&transcript=true"
            ))
            .header("Authorization", token.header_value())
            .send()
            .await
            .unwrap()
            .status(),
        400
    );
    sqlx::query("UPDATE notes SET transcript = ? WHERE id = ?")
        .bind(r#"[{"speaker":"speaker_0","text":"Original speech","timestamp":0.0}]"#)
        .bind(id)
        .execute(db.pool())
        .await
        .unwrap();
    for format in ["markdown", "text", "json", "srt", "vtt"] {
        let response = client
            .get(format!(
                "{base}/v1/notes/{id}/export?format={format}&transcript=true"
            ))
            .header("Authorization", token.header_value())
            .send()
            .await
            .unwrap();
        assert!(response.status().is_success(), "{format}");
        let exported: Value = response.json().await.unwrap();
        assert!(exported["data"]["content"]
            .as_str()
            .unwrap()
            .contains("Original speech"));
        assert!(!exported["data"]["content"]
            .as_str()
            .unwrap()
            .contains("**Budget**"));
    }
    for text in ["Håkan budget", "Håkan budget draft", "unrelated"] {
        sqlx::query("INSERT INTO transcriptions (text) VALUES (?)")
            .bind(text)
            .execute(db.pool())
            .await
            .unwrap();
    }
    let search: Value = client
        .get(format!("{base}/v1/transcriptions/search"))
        .query(&[("q", "Håkan budget"), ("limit", "1")])
        .header("Authorization", token.header_value())
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(search["data"][0]["id"], 2);
    assert_eq!(search["next_cursor"], 2);
    let next: Value = client
        .get(format!("{base}/v1/transcriptions/search"))
        .query(&[("q", "Håkan budget"), ("after", "2")])
        .header("Authorization", token.header_value())
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(next["data"][0]["id"], 1);
    sqlx::query("DELETE FROM transcriptions WHERE id = 1")
        .execute(db.pool())
        .await
        .unwrap();
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM transcriptions_fts WHERE transcriptions_fts MATCH 'budget'",
    )
    .fetch_one(db.pool())
    .await
    .unwrap();
    assert_eq!(count, 1);
    for query in ["C++ (draft)", "NEAR", "Håkan"] {
        assert!(client
            .get(format!("{base}/v1/transcriptions/search"))
            .query(&[("q", query)])
            .header("Authorization", token.header_value())
            .send()
            .await
            .unwrap()
            .status()
            .is_success());
    }
    kw_storage::jobs::create(db.pool(), "interrupted")
        .await
        .unwrap();
    kw_storage::jobs::interrupt_unfinished(db.pool())
        .await
        .unwrap();
    let job: Value = client
        .get(format!("{base}/v1/jobs/interrupted"))
        .header("Authorization", token.header_value())
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(job["data"]["stage"], "failed");
    assert_eq!(
        client
            .get(format!("{base}/v1/status"))
            .header("Authorization", token.header_value())
            .send()
            .await
            .unwrap()
            .status(),
        503
    );
    server.shutdown().await;
}
