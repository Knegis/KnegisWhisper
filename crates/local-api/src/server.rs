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
    guard: Option<std::sync::Arc<crate::discovery::AccessGuard>>,
    runtime: Option<std::sync::Arc<dyn crate::runtime::Runtime>>,
    upload_directory: std::path::PathBuf,
}

impl ApiState {
    #[must_use]
    pub fn with_runtime(
        mut self,
        runtime: std::sync::Arc<dyn crate::runtime::Runtime>,
        uploads: std::path::PathBuf,
    ) -> Self {
        self.runtime = Some(runtime);
        self.upload_directory = uploads;
        self
    }

    fn runtime(&self) -> Result<&dyn crate::runtime::Runtime, ApiError> {
        self.runtime
            .as_deref()
            .ok_or_else(|| ApiError::new(ErrorCode::Unavailable, "desktop runtime is unavailable"))
    }
    pub fn with_access_guard(
        mut self,
        guard: std::sync::Arc<crate::discovery::AccessGuard>,
    ) -> Self {
        self.guard = Some(guard);
        self
    }
    #[must_use]
    pub fn new(pool: SqlitePool, token: LocalApiToken) -> Self {
        Self {
            pool,
            token,
            guard: None,
            runtime: None,
            upload_directory: std::path::PathBuf::new(),
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

/// The guarantee [`serve`] gives: a listener whose address is not loopback is
/// refused before anything is routed. Apart from the listener so it can be
/// tested without opening a socket on the network (KNEG-489).
fn require_loopback(address: SocketAddr) -> Result<(), ServeError> {
    if address.ip().is_loopback() {
        Ok(())
    } else {
        Err(ServeError::NotLoopback { address })
    }
}

/// Start serving on an already-bound loopback listener.
pub fn serve(listener: TcpListener, state: ApiState) -> Result<Served, ServeError> {
    let address = listener
        .local_addr()
        .map_err(|source| ServeError::Address { source })?;

    require_loopback(address)?;

    let port = address.port();
    let (shutdown, shutdown_rx) = oneshot::channel();

    let guard = state.guard.clone();
    let app = router(state);

    let task = tokio::spawn(async move {
        let served = axum::serve(listener, app).with_graceful_shutdown(async move {
            if let Some(guard) = guard {
                tokio::pin!(shutdown_rx);
                loop {
                    tokio::select! {
                        _ = &mut shutdown_rx => break,
                        _ = guard.changed.notified() => {},
                        _ = tokio::time::sleep(std::time::Duration::from_secs(5)) => { guard.check().await; }
                    }
                    if !guard.is_active() {
                        guard.repair_and_remove().await;
                        break;
                    }
                }
            } else { let _ = shutdown_rx.await; }
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
        .route("/v1/notes", post(create_note))
        .route("/v1/notes/{id}/export", get(export_note))
        .route("/v1/transcriptions/search", get(search_history))
        .route("/v1/status", get(app_status))
        .route("/v1/models", get(models))
        .route("/v1/recording/start", post(recording_start))
        .route("/v1/recording/stop", post(recording_stop))
        .route("/v1/jobs/{id}", get(get_job))
        .route("/v1/jobs/{id}/cancel", post(cancel_job))
        .route(
            "/v1/jobs/transcriptions",
            post(upload_transcription).layer(DefaultBodyLimit::max(
                crate::runtime::MAX_UPLOAD_BYTES + 65536,
            )),
        )
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

    if let Some(guard) = &state.guard {
        if !guard.check().await {
            return ApiError::new(
                ErrorCode::Unauthorized,
                "local API access was revoked; retry from Settings",
            )
            .into_response();
        }
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

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct NoteCreate {
    title: String,
    content: String,
    folder_id: Option<i64>,
}
async fn create_note(
    State(state): State<ApiState>,
    Json(input): Json<NoteCreate>,
) -> Result<Json<Item<notes::Note>>, ApiError> {
    if let Some(folder) = input.folder_id {
        if folder <= 0
            || !notes::folders(&state.pool)
                .await
                .map_err(|e| storage_failed("folders", &e))?
                .iter()
                .any(|f| f.id == folder)
        {
            return Err(ApiError::validation(
                "folder_id must name an existing folder",
            ));
        }
    }
    let id = notes::create(
        &state.pool,
        &notes::NewNote {
            title: input.title,
            content: input.content,
            folder_id: input.folder_id,
            ..Default::default()
        },
    )
    .await
    .map_err(|e| storage_failed("create", &e))?;
    get_note(State(state), Path(id.to_string())).await
}
#[derive(serde::Deserialize)]
struct ExportParams {
    #[serde(default = "markdown_format")]
    format: String,
    #[serde(default)]
    transcript: bool,
}
fn markdown_format() -> String {
    "markdown".into()
}
async fn export_note(
    State(state): State<ApiState>,
    Path(id): Path<String>,
    Query(params): Query<ExportParams>,
) -> Result<Json<Item<kw_notes_core::document::Document>>, ApiError> {
    let format = match params.format.as_str() {
        "markdown" | "md" => kw_notes_core::transcript::Format::Md,
        "text" | "txt" => kw_notes_core::transcript::Format::Txt,
        "json" => kw_notes_core::transcript::Format::Json,
        "srt" => kw_notes_core::transcript::Format::Srt,
        "vtt" => kw_notes_core::transcript::Format::Vtt,
        _ => return Err(ApiError::validation("unsupported export format")),
    };
    if !params.transcript && format != kw_notes_core::transcript::Format::Md {
        return Err(ApiError::validation(
            "editable notes export as markdown; use transcript=true for other formats",
        ));
    }
    let note = get_note(State(state.clone()), Path(id)).await?.0.data;
    let mappings = kw_notes_core::speakers::mappings(&state.pool, note.id)
        .await
        .map_err(|_| ApiError::internal())?;
    let document = kw_notes_core::document::render(&note, format, params.transcript, &mappings)
        .map_err(|_| ApiError::validation("the stored transcript cannot be exported"))?;
    if params.transcript && document.segment_count == 0 {
        return Err(ApiError::validation(
            "this note has no timed transcript to export",
        ));
    }
    Ok(Json(Item { data: document }))
}
#[derive(serde::Deserialize)]
struct HistorySearch {
    q: String,
    limit: Option<u32>,
    after: Option<i64>,
}
async fn search_history(
    State(state): State<ApiState>,
    Query(params): Query<HistorySearch>,
) -> Result<Json<Page<transcriptions::Transcription>>, ApiError> {
    let request = ListParams {
        limit: params.limit,
        after: params.after,
        ..Default::default()
    }
    .validate()?;
    let escaped = notes::escape_query(&params.q)
        .ok_or_else(|| ApiError::validation("a search needs something to search for"))?;
    let mut rows: Vec<transcriptions::Transcription> = sqlx::query_as("SELECT t.* FROM transcriptions t JOIN transcriptions_fts f ON f.rowid = t.id WHERE transcriptions_fts MATCH ? AND (? IS NULL OR t.id < ?) ORDER BY t.id DESC LIMIT ?")
        .bind(escaped).bind(request.after).bind(request.after).bind(i64::from(request.limit) + 1)
        .fetch_all(&state.pool).await.map_err(|_| ApiError::internal())?;
    let has_more = rows.len() > request.limit as usize;
    rows.truncate(request.limit as usize);
    let next_cursor = if has_more {
        rows.last().map(|r| r.id)
    } else {
        None
    };
    Ok(Json(Page {
        data: rows,
        has_more,
        next_cursor,
    }))
}
async fn app_status(
    State(state): State<ApiState>,
) -> Result<Json<Item<serde_json::Value>>, ApiError> {
    Ok(Json(Item {
        data: state.runtime()?.status().await?,
    }))
}
async fn models(State(state): State<ApiState>) -> Result<Json<Item<serde_json::Value>>, ApiError> {
    Ok(Json(Item {
        data: state.runtime()?.models().await?,
    }))
}
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct RecordingStart {
    provider: String,
    #[serde(default)]
    system_audio: bool,
}
async fn recording_start(
    State(state): State<ApiState>,
    Json(input): Json<RecordingStart>,
) -> Result<Json<Item<serde_json::Value>>, ApiError> {
    if input.provider.trim().is_empty() {
        return Err(ApiError::validation("provider is required"));
    }
    let runtime = state
        .runtime
        .clone()
        .ok_or_else(|| ApiError::new(ErrorCode::Unavailable, "desktop runtime is unavailable"))?;
    let data = tokio::spawn(async move {
        runtime
            .recording_start(&input.provider, input.system_audio)
            .await
    })
    .await
    .map_err(|_| ApiError::internal())??;
    Ok(Json(Item { data }))
}
async fn recording_stop(
    State(state): State<ApiState>,
) -> Result<Json<Item<serde_json::Value>>, ApiError> {
    // Own the stop task even when an HTTP client disconnects.
    let runtime = state
        .runtime
        .clone()
        .ok_or_else(|| ApiError::new(ErrorCode::Unavailable, "desktop runtime is unavailable"))?;
    let data = tokio::spawn(async move { runtime.recording_stop().await })
        .await
        .map_err(|_| ApiError::internal())??;
    Ok(Json(Item { data }))
}
async fn get_job(
    State(state): State<ApiState>,
    Path(id): Path<String>,
) -> Result<Json<Item<kw_storage::jobs::Job>>, ApiError> {
    let job = kw_storage::jobs::get(&state.pool, &id)
        .await
        .map_err(|_| ApiError::internal())?
        .ok_or_else(|| ApiError::not_found("job"))?;
    Ok(Json(Item { data: job }))
}
async fn cancel_job(
    State(state): State<ApiState>,
    Path(id): Path<String>,
) -> Result<Json<Item<serde_json::Value>>, ApiError> {
    Ok(Json(Item {
        data: state.runtime()?.cancel(&id).await?,
    }))
}
async fn upload_transcription(
    State(state): State<ApiState>,
    mut multipart: axum::extract::Multipart,
) -> Result<(StatusCode, Json<Item<serde_json::Value>>), ApiError> {
    use std::io::Write as _;
    use std::time::Duration;
    state.runtime()?;
    tokio::fs::create_dir_all(&state.upload_directory)
        .await
        .map_err(|_| ApiError::internal())?;
    let mut field = tokio::time::timeout(Duration::from_secs(60), multipart.next_field())
        .await
        .map_err(|_| ApiError::validation("upload timed out"))?
        .map_err(|_| ApiError::validation("invalid multipart upload"))?
        .ok_or_else(|| ApiError::validation("upload one file field"))?;
    if field.name() != Some("file") {
        return Err(ApiError::validation("upload one file field"));
    }
    let extension = field
        .file_name()
        .and_then(|s| std::path::Path::new(s).extension())
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if ![
        "wav", "mp3", "m4a", "mp4", "ogg", "flac", "webm", "aac", "wma", "mov", "mkv", "avi",
    ]
    .contains(&extension.as_str())
    {
        return Err(ApiError::validation("unsupported media extension"));
    }
    let upload = crate::runtime::Upload {
        name: field
            .file_name()
            .and_then(|s| std::path::Path::new(s).file_name())
            .and_then(|s| s.to_str())
            .unwrap_or("media")
            .to_owned(),
        path: state
            .upload_directory
            .join(format!("{}.{}", uuid::Uuid::new_v4(), extension)),
    };
    // Drop closes the file before Upload removes it on all error paths.
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(upload.path())
        .map_err(|_| ApiError::internal())?;
    let mut bytes = 0usize;
    loop {
        let chunk = tokio::time::timeout(Duration::from_secs(60), field.chunk())
            .await
            .map_err(|_| ApiError::validation("upload timed out"))?
            .map_err(|_| ApiError::validation("invalid or oversized upload"))?;
        let Some(chunk) = chunk else { break };
        bytes = bytes.saturating_add(chunk.len());
        if bytes > crate::runtime::MAX_UPLOAD_BYTES {
            return Err(ApiError::new(
                ErrorCode::PayloadTooLarge,
                "media exceeds 500 MiB",
            ));
        }
        file.write_all(&chunk).map_err(|_| ApiError::internal())?;
    }
    file.flush().map_err(|_| ApiError::internal())?;
    drop(file);
    if bytes == 0 {
        return Err(ApiError::validation("media is empty"));
    }
    drop(field);
    if tokio::time::timeout(Duration::from_secs(60), multipart.next_field())
        .await
        .map_err(|_| ApiError::validation("upload timed out"))?
        .map_err(|_| ApiError::validation("invalid multipart upload"))?
        .is_some()
    {
        return Err(ApiError::validation("upload exactly one file"));
    }
    // Once submitted, the operation belongs to the app, not the connection.
    let runtime = state.runtime.clone().ok_or_else(ApiError::internal)?;
    let data = tokio::spawn(async move { runtime.transcribe(upload).await })
        .await
        .map_err(|_| ApiError::internal())??;
    Ok((StatusCode::ACCEPTED, Json(Item { data })))
}

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

/// One note. A note in Trash is gone as far as the API is concerned
/// (KNEG-401): 404 here, and so for its export and a PATCH, and left out of
/// list and search. Only a person brings it back, from Trash in the app.
async fn get_note(
    State(state): State<ApiState>,
    Path(id): Path<String>,
) -> Result<Json<Item<notes::Note>>, ApiError> {
    let id = crate::wire::parse_id(&id)?;
    let note = notes::get(&state.pool, id)
        .await
        .map_err(|err| storage_failed("notes::get", &err))?
        .filter(|note| note.trashed_at.is_none())
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

/// Move a note to Trash, as Delete does in the app (KNEG-401): its text and
/// recording are kept until a person restores it or empties Trash there, and
/// from here on the API answers 404 for it. A note already in Trash is 404
/// too. Nothing the API does deletes a note for good.
async fn delete_note(
    State(state): State<ApiState>,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let id = crate::wire::parse_id(&id)?;
    let moved = kw_notes_core::trash::trash(&state.pool, id)
        .await
        .map_err(|err| storage_failed("trash::trash", &err))?;

    if moved {
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

    /// KNEG-401. DELETE moves the note to Trash, as Delete does in the app:
    /// its recording row and file stay, for Restore, and from then on the
    /// API treats it as gone -- GET, PATCH and export are 404, list and
    /// search leave it out, and a second DELETE is 404. An unauthenticated
    /// DELETE moves nothing.
    #[tokio::test]
    async fn authenticated_delete_moves_the_note_to_trash_and_unauthenticated_does_not() {
        let dir = tempfile::tempdir().unwrap();
        let db = kw_storage::Db::open_in_memory().await.unwrap();
        let id = notes::create(
            db.pool(),
            &notes::NewNote {
                title: "Recorded".to_owned(),
                content: "the warehouse plan".to_owned(),
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
        let state = ApiState::new(db.pool().clone(), token.clone());
        let server = serve(TcpListener::bind("127.0.0.1:0").await.unwrap(), state).unwrap();
        let base = format!("http://127.0.0.1:{}/v1", server.port());
        let url = format!("{base}/notes/{id}");
        let client = reqwest::Client::new();
        let authorised = |request: reqwest::RequestBuilder| {
            request.header("Authorization", token.header_value())
        };
        let status = |request: reqwest::RequestBuilder| async move {
            request.send().await.unwrap().status()
        };

        assert_eq!(status(client.delete(&url)).await, StatusCode::UNAUTHORIZED);
        assert!(notes::get(db.pool(), id)
            .await
            .unwrap()
            .unwrap()
            .trashed_at
            .is_none());

        assert_eq!(
            status(authorised(client.delete(&url))).await,
            StatusCode::NO_CONTENT
        );
        assert!(audio.exists(), "the recording waits in Trash with its note");
        assert!(kw_notes_core::recordings::get(db.pool(), id)
            .await
            .unwrap()
            .is_some());
        assert!(notes::get(db.pool(), id)
            .await
            .unwrap()
            .unwrap()
            .trashed_at
            .is_some());

        assert_eq!(
            status(authorised(client.get(&url))).await,
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            status(authorised(
                client
                    .patch(&url)
                    .json(&serde_json::json!({ "title": "Back" }))
            ))
            .await,
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            status(authorised(client.get(format!("{url}/export")))).await,
            StatusCode::NOT_FOUND
        );
        let listed: serde_json::Value = authorised(client.get(format!("{base}/notes/list")))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(listed.get("data"), Some(&serde_json::json!([])));
        let found: serde_json::Value =
            authorised(client.get(format!("{base}/notes/search?q=warehouse")))
                .send()
                .await
                .unwrap()
                .json()
                .await
                .unwrap();
        assert_eq!(found.get("data"), Some(&serde_json::json!([])));
        assert_eq!(
            status(authorised(client.delete(&url))).await,
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            notes::get(db.pool(), id).await.unwrap().unwrap().title,
            "Recorded",
            "the refused PATCH changed nothing"
        );
        server.shutdown().await;
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod bind_tests {
    use super::*;

    /// KNEG-489. The address rule `serve` applies, without a socket: a
    /// listener on 0.0.0.0 in a test raises a Windows Firewall prompt for
    /// every freshly built test binary. `tests/api.rs` still checks that
    /// `serve` applies it, on Linux and macOS.
    #[test]
    fn every_address_off_this_machine_is_refused() {
        for address in [
            "0.0.0.0:8200",
            "[::]:8200",
            "192.168.1.10:8200",
            "10.0.0.1:8200",
        ] {
            let address: SocketAddr = address.parse().unwrap();
            let err = require_loopback(address).unwrap_err();
            assert!(
                matches!(err, ServeError::NotLoopback { address: refused } if refused == address),
                "{address}: {err}"
            );
            assert!(
                err.to_string().contains("not safe to expose"),
                "{address}: {err}"
            );
        }
    }

    #[test]
    fn loopback_addresses_are_served() {
        for address in ["127.0.0.1:8200", "127.0.0.2:8200", "[::1]:8200"] {
            let address: SocketAddr = address.parse().unwrap();
            assert!(require_loopback(address).is_ok(), "{address} was refused");
        }
    }
}
