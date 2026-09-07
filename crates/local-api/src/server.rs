//! The HTTP server.
//!
//! Given a listener rather than a port, so the caller owns port allocation and
//! there is exactly one loopback port allocator in the workspace
//! (`kw_sidecar::port`). What this crate owns instead is the *guarantee*:
//! [`serve`] inspects the listener and refuses to serve on anything that is not
//! a loopback address.
//!
//! That is stronger than upstream's arrangement. Upstream binds `127.0.0.1` and
//! then also checks each request's remote address, because a later edit to the
//! bind address would otherwise silently expose everything. Here a listener that
//! is not loopback never reaches the router at all, so the per-request check has
//! nothing left to catch — and a future edit that widens the bind fails at
//! startup with an explanation rather than working.

use std::net::SocketAddr;

use axum::extract::{DefaultBodyLimit, Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, patch, post};
use axum::{Json, Router};
use kw_notes_core::notes;
use kw_security::LocalApiToken;
use kw_storage::{dictionary, transcriptions};
use sqlx::SqlitePool;
use tokio::net::TcpListener;
use tokio::sync::oneshot;

use crate::wire::{
    ApiError, DictionaryUpdate, ErrorCode, Health, Item, ListParams, NotePatch, Page, SearchParams,
    MAX_BODY_BYTES,
};
use crate::API_VERSION;

/// What the handlers need.
#[derive(Clone)]
pub struct ApiState {
    pool: SqlitePool,
    token: LocalApiToken,
    recording_directory: std::path::PathBuf,
}

impl ApiState {
    #[must_use]
    pub fn new(pool: SqlitePool, token: LocalApiToken) -> Self {
        Self {
            pool,
            token,
            recording_directory: kw_notes_core::recordings::recording_dir(),
        }
    }
}

impl std::fmt::Debug for ApiState {
    /// `LocalApiToken` redacts itself, but spelling this out means a future
    /// field cannot be added and printed by accident.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ApiState").finish_non_exhaustive()
    }
}

/// Why the server would not start.
#[derive(Debug, thiserror::Error)]
pub enum ServeError {
    #[error(
        "refusing to serve the local API on {address}: it is not a loopback address. This API \
         has no transport security and authenticates with a token from a file in your home \
         directory; it is not safe to expose beyond this machine."
    )]
    NotLoopback { address: SocketAddr },

    #[error("could not read the listener's address: {source}")]
    Address {
        #[source]
        source: std::io::Error,
    },
}

/// A running server.
#[derive(Debug)]
pub struct Served {
    port: u16,
    shutdown: oneshot::Sender<()>,
    task: tokio::task::JoinHandle<()>,
}

impl Served {
    #[must_use]
    pub const fn port(&self) -> u16 {
        self.port
    }

    /// Stop serving and wait for in-flight requests to finish.
    pub async fn shutdown(self) {
        // A closed receiver is what the graceful-shutdown future waits on, so
        // ignoring the send error is correct: it means the server already
        // stopped.
        let _ = self.shutdown.send(());
        let _ = self.task.await;
    }
}

/// Start serving on an already-bound loopback listener.
pub fn serve(listener: TcpListener, state: ApiState) -> Result<Served, ServeError> {
    let address = listener
        .local_addr()
        .map_err(|source| ServeError::Address { source })?;

    if !address.ip().is_loopback() {
        return Err(ServeError::NotLoopback { address });
    }

    let port = address.port();
    let (shutdown, shutdown_rx) = oneshot::channel();

    let app = router(state);

    let task = tokio::spawn(async move {
        let served = axum::serve(listener, app).with_graceful_shutdown(async move {
            let _ = shutdown_rx.await;
        });
        if let Err(err) = served.await {
            tracing::error!(error = %err, "the local API stopped unexpectedly");
        }
    });

    tracing::info!(port, "local API listening on loopback");

    Ok(Served {
        port,
        shutdown,
        task,
    })
}

/// The route table.
fn router(state: ApiState) -> Router {
    Router::new()
        .route("/v1/health", get(health))
        .route("/v1/notes/list", get(list_notes))
        .route("/v1/notes/search", get(search_notes))
        .route("/v1/notes/{id}", get(get_note))
        .route("/v1/notes/{id}", patch(patch_note))
        .route("/v1/notes/{id}", delete(delete_note))
        .route("/v1/folders/list", get(list_folders))
        .route("/v1/dictionary/list", get(list_dictionary))
        .route("/v1/dictionary/update", post(update_dictionary))
        .route("/v1/transcriptions/list", get(list_transcriptions))
        .route("/v1/transcriptions/{id}", get(get_transcription))
        .route("/v1/transcriptions/{id}", delete(delete_transcription))
        .route("/v1/transcriptions/{id}/audio", delete(delete_audio))
        .fallback(not_found)
        // Applied to every route, including the fallback: an unauthenticated
        // caller must not be able to learn which paths exist.
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            authenticate,
        ))
        .layer(DefaultBodyLimit::max(MAX_BODY_BYTES))
        .with_state(state)
}

/// Reject anything without the right bearer token.
async fn authenticate(
    State(state): State<ApiState>,
    headers: HeaderMap,
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> Response {
    let presented = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();

    if !state.token.matches_header(presented) {
        // No detail about why. "Wrong token" and "no token" are the same answer
        // to a caller who should not be here.
        return ApiError::new(ErrorCode::Unauthorized, "a valid bearer token is required")
            .into_response();
    }

    next.run(request).await
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status =
            StatusCode::from_u16(self.code.status()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        (status, Json(serde_json::json!({ "error": self }))).into_response()
    }
}

/// Turn a storage failure into a response, keeping the detail out of it.
fn storage_failed(context: &str, err: &kw_storage::StorageError) -> ApiError {
    // The detail goes to the log, not to the caller: a database error can carry
    // a file path or a column name.
    tracing::error!(context, error = %err, "local API request failed");
    ApiError::internal()
}

// -------------------------------------------------------------------- handlers

async fn health() -> Json<Item<Health>> {
    Json(Item {
        data: Health {
            ok: true,
            version: API_VERSION,
            app_version: env!("CARGO_PKG_VERSION"),
        },
    })
}

async fn not_found() -> ApiError {
    ApiError::new(ErrorCode::NotFound, "no such route")
}

async fn list_notes(
    State(state): State<ApiState>,
    Query(params): Query<ListParams>,
) -> Result<Json<Page<notes::Note>>, ApiError> {
    let request = params.validate()?;

    // Notes are ordered by `updated_at`, so an id cursor would not page them
    // stably — it would skip or repeat rows as notes are edited. Refused rather
    // than accepted and quietly ignored, which is how a caller ends up trusting
    // a cursor that does nothing. `has_more` is still answered truthfully, so a
    // caller can tell there is more even without a cursor to fetch it with.
    if request.after.is_some() {
        return Err(ApiError::validation(
            "'after' is not supported for notes, which are ordered by when they were last \
             edited. Use 'limit', or page transcriptions instead.",
        ));
    }

    // One more than asked for, so `has_more` is a fact rather than a guess.
    let probe = i64::from(request.limit.saturating_add(1));
    let mut rows = notes::list(
        &state.pool,
        request.note_type.as_deref(),
        request.folder_id,
        probe,
    )
    .await
    .map_err(|err| storage_failed("notes::list", &err))?;

    let has_more = rows.len() > request.limit as usize;
    rows.truncate(request.limit as usize);

    Ok(Json(Page {
        data: rows,
        has_more,
        next_cursor: None,
    }))
}

async fn search_notes(
    State(state): State<ApiState>,
    Query(params): Query<SearchParams>,
) -> Result<Json<Page<notes::SearchHit>>, ApiError> {
    let request = params.validate()?;

    let rows = notes::search(&state.pool, &request.query, i64::from(request.limit))
        .await
        .map_err(|err| storage_failed("notes::search", &err))?;

    Ok(Json(Page {
        data: rows,
        has_more: false,
        next_cursor: None,
    }))
}

async fn get_note(
    State(state): State<ApiState>,
    Path(id): Path<String>,
) -> Result<Json<Item<notes::Note>>, ApiError> {
    let id = crate::wire::parse_id(&id)?;
    let note = notes::get(&state.pool, id)
        .await
        .map_err(|err| storage_failed("notes::get", &err))?
        .ok_or_else(|| ApiError::not_found("note"))?;
    Ok(Json(Item { data: note }))
}

async fn patch_note(
    State(state): State<ApiState>,
    Path(id): Path<String>,
    Json(patch): Json<NotePatch>,
) -> Result<Json<Item<notes::Note>>, ApiError> {
    let id = crate::wire::parse_id(&id)?;
    patch.validate()?;

    let changed = notes::update(
        &state.pool,
        id,
        &notes::NoteUpdate {
            title: patch.title,
            content: patch.content,
            folder_id: patch.folder_id,
        },
    )
    .await
    .map_err(|err| storage_failed("notes::update", &err))?;

    if !changed {
        return Err(ApiError::not_found("note"));
    }

    // Read back rather than echoing the patch: the caller gets what the note
    // actually is now, including the fields they did not touch.
    let note = notes::get(&state.pool, id)
        .await
        .map_err(|err| storage_failed("notes::get", &err))?
        .ok_or_else(|| ApiError::not_found("note"))?;

    Ok(Json(Item { data: note }))
}

async fn delete_note(
    State(state): State<ApiState>,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let id = crate::wire::parse_id(&id)?;
    let removed = kw_notes_core::recordings::delete_with_recording(
        &state.pool,
        id,
        &state.recording_directory,
    )
    .await
    .map_err(|err| storage_failed("notes::delete", &err))?;

    if removed {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::not_found("note"))
    }
}

async fn list_folders(
    State(state): State<ApiState>,
) -> Result<Json<Page<notes::Folder>>, ApiError> {
    let rows = notes::folders(&state.pool)
        .await
        .map_err(|err| storage_failed("notes::folders", &err))?;
    Ok(Json(Page {
        data: rows,
        has_more: false,
        next_cursor: None,
    }))
}

async fn list_dictionary(
    State(state): State<ApiState>,
) -> Result<Json<Page<dictionary::DictionaryWord>>, ApiError> {
    let rows = dictionary::all(&state.pool)
        .await
        .map_err(|err| storage_failed("dictionary::all", &err))?;
    Ok(Json(Page {
        data: rows,
        has_more: false,
        next_cursor: None,
    }))
}

async fn update_dictionary(
    State(state): State<ApiState>,
    Json(update): Json<DictionaryUpdate>,
) -> Result<Json<Item<dictionary::DictionaryChange>>, ApiError> {
    let request = update.validate()?;
    let change = dictionary::update(&state.pool, &request.add, &request.remove)
        .await
        .map_err(|err| storage_failed("dictionary::update", &err))?;
    Ok(Json(Item { data: change }))
}

async fn list_transcriptions(
    State(state): State<ApiState>,
    Query(params): Query<ListParams>,
) -> Result<Json<Page<transcriptions::Transcription>>, ApiError> {
    let request = params.validate()?;

    let probe = request.limit.saturating_add(1);
    let mut rows = transcriptions::recent(&state.pool, i64::from(probe), request.after)
        .await
        .map_err(|err| storage_failed("transcriptions::recent", &err))?;

    let has_more = rows.len() > request.limit as usize;
    rows.truncate(request.limit as usize);
    let next_cursor = if has_more {
        rows.last().map(|row| row.id)
    } else {
        None
    };

    Ok(Json(Page {
        data: rows,
        has_more,
        next_cursor,
    }))
}

async fn get_transcription(
    State(state): State<ApiState>,
    Path(id): Path<String>,
) -> Result<Json<Item<transcriptions::Transcription>>, ApiError> {
    let id = crate::wire::parse_id(&id)?;
    let row = transcriptions::get(&state.pool, id)
        .await
        .map_err(|err| storage_failed("transcriptions::get", &err))?
        .ok_or_else(|| ApiError::not_found("transcription"))?;
    Ok(Json(Item { data: row }))
}

async fn delete_transcription(
    State(state): State<ApiState>,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let id = crate::wire::parse_id(&id)?;
    let removed = transcriptions::delete(&state.pool, id)
        .await
        .map_err(|err| storage_failed("transcriptions::delete", &err))?;

    if removed {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::not_found("transcription"))
    }
}

async fn delete_audio(
    State(state): State<ApiState>,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let id = crate::wire::parse_id(&id)?;
    match transcriptions::forget_audio(&state.pool, id)
        .await
        .map_err(|err| storage_failed("transcriptions::forget_audio", &err))?
    {
        // Idempotent: the caller asked for the recording to be gone, and it is.
        transcriptions::AudioForgotten::Cleared
        | transcriptions::AudioForgotten::NoAudioToForget => Ok(StatusCode::NO_CONTENT),
        transcriptions::AudioForgotten::NoSuchRow => Err(ApiError::not_found("transcription")),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod recording_deletion_tests {
    use super::*;

    #[tokio::test]
    async fn authenticated_delete_removes_recording_but_unauthenticated_does_not() {
        let dir = tempfile::tempdir().unwrap();
        let db = kw_storage::Db::open_in_memory().await.unwrap();
        let id = notes::create(
            db.pool(),
            &notes::NewNote {
                title: "Recorded".to_owned(),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let audio = dir.path().join("owned.wav");
        std::fs::write(&audio, b"audio").unwrap();
        kw_notes_core::recordings::attach(db.pool(), id, "owned.wav", None, None)
            .await
            .unwrap();
        let token = LocalApiToken::generate().unwrap();
        let mut state = ApiState::new(db.pool().clone(), token.clone());
        state.recording_directory = dir.path().to_owned();
        let server = serve(TcpListener::bind("127.0.0.1:0").await.unwrap(), state).unwrap();
        let url = format!("http://127.0.0.1:{}/v1/notes/{id}", server.port());
        let client = reqwest::Client::new();
        assert_eq!(
            client.delete(&url).send().await.unwrap().status(),
            StatusCode::UNAUTHORIZED
        );
        assert!(audio.exists());
        assert_eq!(
            client
                .delete(&url)
                .header("Authorization", token.header_value())
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::NO_CONTENT
        );
        assert!(!audio.exists());
        assert!(kw_notes_core::recordings::get(db.pool(), id)
            .await
            .unwrap()
            .is_none());
        assert_eq!(
            client
                .delete(&url)
                .header("Authorization", token.header_value())
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::NOT_FOUND
        );
        server.shutdown().await;
    }
}
