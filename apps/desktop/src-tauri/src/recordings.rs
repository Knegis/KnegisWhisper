//! Serve the application-owned WAV attached to a note.
//!
//! A custom `recording` scheme is used instead of Tauri's asset protocol because
//! the renderer names a note id, never a filesystem path. Rust looks up that id,
//! validates the stored bare file name, and resolves it only inside the
//! recordings directory. Chromium's media loader uses byte ranges to seek, so
//! ranged responses are capped at 1 MiB in the same fashion as Tauri's asset
//! protocol. This surface does not list recordings, accept renderer-supplied
//! paths, or serve anything outside the application-owned directory.

use std::ffi::OsStr;
use std::fs::File;
use std::io::{Read as _, Seek as _, SeekFrom};
use std::path::{Path, PathBuf};

use kw_notes_core::NoteRecording;
use tauri::http::header::{
    HeaderValue, ACCEPT_RANGES, CONTENT_LENGTH, CONTENT_RANGE, CONTENT_TYPE, RANGE,
};
use tauri::http::{Method, Request, Response, StatusCode};
use tauri::Manager as _;

use crate::state::AppState;

/// The URI scheme registered with Tauri.
pub(crate) const SCHEME: &str = "recording";

/// The maximum body returned for one range request, matching the intended
/// 1 MiB cap for this protocol.
const MAX_LEN: u64 = 1024 * 1024;

/// Where meeting recordings are kept.
pub(crate) fn recording_dir() -> PathBuf {
    kw_storage::paths::data_dir().join("meetings")
}

/// Whether `key` is one plain file name rather than a path.
pub(crate) fn is_valid_storage_key(key: &str) -> bool {
    !key.is_empty()
        && key != "."
        && key != ".."
        && key.trim() == key
        && !key.contains(['/', '\\', '\0'])
        && Path::new(key).file_name() == Some(OsStr::new(key))
}

/// Resolve a validated storage key inside the application-owned directory.
pub(crate) fn resolve(key: &str) -> Option<PathBuf> {
    if !is_valid_storage_key(key) {
        return None;
    }

    let directory = recording_dir();
    let joined = directory.join(key);
    (joined.parent() == Some(directory.as_path())).then_some(joined)
}

/// Derive the storage key from a path the recorder constructed itself.
pub(crate) fn storage_key_for(path: &Path) -> Option<String> {
    let key = path.file_name()?.to_str()?;
    is_valid_storage_key(key).then(|| key.to_owned())
}

/// Parse the note id from a custom-protocol request path.
pub(crate) fn note_id_from_path(path: &str) -> Option<i64> {
    let id = path.strip_prefix('/')?;
    (!id.is_empty() && id.bytes().all(|byte| byte.is_ascii_digit()))
        .then(|| id.parse::<i64>().ok())
        .flatten()
        .filter(|id| *id > 0)
}

fn response(status: StatusCode, body: Vec<u8>) -> Response<Vec<u8>> {
    let mut response = Response::new(body);
    *response.status_mut() = status;
    response
}

fn success_response(status: StatusCode, body: Vec<u8>, content_length: u64) -> Response<Vec<u8>> {
    let mut response = response(status, body);
    response
        .headers_mut()
        .insert(CONTENT_TYPE, HeaderValue::from_static("audio/wav"));
    response
        .headers_mut()
        .insert(ACCEPT_RANGES, HeaderValue::from_static("bytes"));
    response
        .headers_mut()
        .insert(CONTENT_LENGTH, HeaderValue::from(content_length));
    response
}

fn range_not_satisfiable(len: u64) -> Response<Vec<u8>> {
    let mut response = response(StatusCode::RANGE_NOT_SATISFIABLE, Vec::new());
    let value = format!("bytes */{len}");
    if let Ok(value) = value.parse::<HeaderValue>() {
        response.headers_mut().insert(CONTENT_RANGE, value);
    }
    response
}

/// A parsed single byte range, or `None` when the header asks for multiple
/// ranges and should therefore be ignored.
fn parse_range(value: &str, len: u64) -> Result<Option<(u64, u64)>, ()> {
    if value.contains(',') {
        // Nothing here needs multipart ranges. HTTP permits a server to ignore
        // the header, so multiple ranges take the ordinary whole-file path.
        return Ok(None);
    }

    let value = value.strip_prefix("bytes=").ok_or(())?;
    let (start, end) = value.split_once('-').ok_or(())?;
    if len == 0 {
        return Err(());
    }

    if start.is_empty() {
        let suffix = end.parse::<u64>().map_err(|_| ())?;
        if suffix == 0 {
            return Err(());
        }
        return Ok(Some((len.saturating_sub(suffix.min(len)), len - 1)));
    }

    let start = start.parse::<u64>().map_err(|_| ())?;
    if start >= len {
        return Err(());
    }
    let end = if end.is_empty() {
        len - 1
    } else {
        end.parse::<u64>().map_err(|_| ())?.min(len - 1)
    };
    if end < start {
        return Err(());
    }

    Ok(Some((start, end)))
}

fn read_range(file: &mut File, start: u64, end: u64) -> std::io::Result<Vec<u8>> {
    let count = end + 1 - start;
    let mut bytes = vec![0; usize::try_from(count).map_err(std::io::Error::other)?];
    file.seek(SeekFrom::Start(start))?;
    file.read_exact(&mut bytes)?;
    Ok(bytes)
}

/// Build a response for an application-owned WAV.
///
/// This is pure apart from opening and reading `path`; the protocol handler can
/// therefore run it away from the webview thread and unit tests need no Tauri
/// runtime.
pub(crate) fn response_for(request: &Request<Vec<u8>>, path: &Path) -> Response<Vec<u8>> {
    let mut file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return response(StatusCode::NOT_FOUND, Vec::new());
        }
        Err(error) => {
            tracing::error!(path = %path.display(), error = %error, "could not open a note recording");
            return response(StatusCode::INTERNAL_SERVER_ERROR, Vec::new());
        }
    };
    let len = match file.metadata() {
        Ok(metadata) => metadata.len(),
        Err(error) => {
            tracing::error!(path = %path.display(), error = %error, "could not inspect a note recording");
            return response(StatusCode::INTERNAL_SERVER_ERROR, Vec::new());
        }
    };

    let range = match request.headers().get(RANGE) {
        Some(header) => match header.to_str() {
            Ok(value) => match parse_range(value, len) {
                Ok(range) => range,
                Err(()) => return range_not_satisfiable(len),
            },
            Err(_) => return range_not_satisfiable(len),
        },
        None => None,
    };

    let is_head = request.method() == Method::HEAD;
    let response = if let Some((start, requested_end)) = range {
        let end = start + (requested_end - start).min(len - start).min(MAX_LEN - 1);
        let content_length = end + 1 - start;
        let body = if is_head {
            Vec::new()
        } else {
            match read_range(&mut file, start, end) {
                Ok(bytes) => bytes,
                Err(error) => {
                    tracing::error!(path = %path.display(), error = %error, "could not read a note recording range");
                    return response(StatusCode::INTERNAL_SERVER_ERROR, Vec::new());
                }
            }
        };
        let mut response = success_response(StatusCode::PARTIAL_CONTENT, body, content_length);
        let value = format!("bytes {start}-{end}/{len}");
        if let Ok(value) = value.parse::<HeaderValue>() {
            response.headers_mut().insert(CONTENT_RANGE, value);
        }
        response
    } else if is_head {
        success_response(StatusCode::OK, Vec::new(), len)
    } else {
        // Chromium's documented media-loading behaviour is to request ranges.
        // This fallback mirrors Tauri and reads the whole file into memory; the
        // crate has not measured whether Chromium ever takes it for audio.
        let mut body = Vec::new();
        if let Err(error) = file.read_to_end(&mut body) {
            tracing::error!(path = %path.display(), error = %error, "could not read a note recording");
            return response(StatusCode::INTERNAL_SERVER_ERROR, Vec::new());
        }
        success_response(StatusCode::OK, body, len)
    };

    // Deliberately no Access-Control-Allow-Origin: an <audio src> may play
    // cross-origin media, and renderer-script fetches are not part of this API.
    response
}

/// Serve one recording by note id without blocking the webview thread on the
/// database or filesystem.
pub(crate) fn serve<R: tauri::Runtime>(
    ctx: tauri::UriSchemeContext<'_, R>,
    request: Request<Vec<u8>>,
    responder: tauri::UriSchemeResponder,
) {
    if request.method() != Method::GET && request.method() != Method::HEAD {
        responder.respond(response(StatusCode::METHOD_NOT_ALLOWED, Vec::new()));
        return;
    }
    let Some(note_id) = note_id_from_path(request.uri().path()) else {
        responder.respond(response(StatusCode::BAD_REQUEST, Vec::new()));
        return;
    };

    let app = ctx.app_handle().clone();
    tauri::async_runtime::spawn(async move {
        let Some(state) = app.try_state::<AppState>() else {
            responder.respond(response(StatusCode::SERVICE_UNAVAILABLE, Vec::new()));
            return;
        };
        let recording = match kw_notes_core::recordings::get(state.db().pool(), note_id).await {
            Ok(Some(recording)) => recording,
            Ok(None) => {
                responder.respond(response(StatusCode::NOT_FOUND, Vec::new()));
                return;
            }
            Err(error) => {
                tracing::error!(note_id, error = %error, "could not look up a note recording");
                responder.respond(response(StatusCode::INTERNAL_SERVER_ERROR, Vec::new()));
                return;
            }
        };
        let Some(path) = resolve(&recording.storage_key) else {
            tracing::error!(note_id, key = %recording.storage_key, "stored note recording key is invalid");
            responder.respond(response(StatusCode::FORBIDDEN, Vec::new()));
            return;
        };

        responder.respond(response_for(&request, &path));
    });
}

async fn remove_at(path: &Path, key: &str) {
    match tokio::fs::remove_file(path).await {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            tracing::warn!(storage_key = key, error = %error, "could not remove a deleted note's recording");
        }
    }
}

/// Remove the application-owned file after its note has been deleted.
pub(crate) async fn remove_file_for(recording: &NoteRecording) {
    let Some(path) = resolve(&recording.storage_key) else {
        tracing::warn!(storage_key = %recording.storage_key, "refused to remove an invalid recording key");
        return;
    };
    remove_at(&path, &recording.storage_key).await;
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

    fn wav() -> Vec<u8> {
        kw_audio_core::wav::encode_wav(&vec![0.25; 256], 16_000)
    }

    fn request(method: Method, range: Option<&str>) -> Request<Vec<u8>> {
        let mut builder = Request::builder()
            .method(method)
            .uri("recording://localhost/1");
        if let Some(range) = range {
            builder = builder.header(RANGE, range);
        }
        builder.body(Vec::new()).unwrap()
    }

    fn temporary_file(bytes: &[u8]) -> (tempfile::TempDir, PathBuf) {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("recording.wav");
        std::fs::write(&path, bytes).unwrap();
        (directory, path)
    }

    #[test]
    fn storage_keys_refuse_paths_and_ambiguous_names() {
        for refused in [
            "../x", "a/b", "a\\b", "", ".", "..", "C:\\x", " x", "x ", "a\0b",
        ] {
            assert!(!is_valid_storage_key(refused), "accepted {refused:?}");
        }
        assert!(is_valid_storage_key("20260902-141530.wav"));
    }

    #[test]
    fn note_ids_are_positive_ascii_digits_after_one_slash() {
        assert_eq!(note_id_from_path("/12"), Some(12));
        for refused in ["/", "", "/12/x", "/-1", "/0", "/abc", "/12?x=1"] {
            assert_eq!(note_id_from_path(refused), None, "accepted {refused:?}");
        }
    }

    #[test]
    fn a_full_get_returns_the_exact_wav_and_length() {
        let bytes = wav();
        let (_directory, path) = temporary_file(&bytes);
        let response = response_for(&request(Method::GET, None), &path);

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[CONTENT_LENGTH], bytes.len().to_string());
        assert_eq!(response.body(), &bytes);
    }

    #[test]
    fn an_explicit_range_returns_its_hundred_bytes() {
        let bytes = wav();
        let (_directory, path) = temporary_file(&bytes);
        let response = response_for(&request(Method::GET, Some("bytes=0-99")), &path);

        assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
        assert_eq!(response.body(), &bytes[0..100]);
        assert_eq!(
            response.headers()[CONTENT_RANGE],
            format!("bytes 0-99/{}", bytes.len())
        );
    }

    #[test]
    fn an_open_range_runs_to_the_end() {
        let bytes = wav();
        let (_directory, path) = temporary_file(&bytes);
        let response = response_for(&request(Method::GET, Some("bytes=100-")), &path);

        assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
        assert_eq!(response.body(), &bytes[100..]);
    }

    #[test]
    fn a_suffix_range_returns_the_last_hundred_bytes() {
        let bytes = wav();
        let (_directory, path) = temporary_file(&bytes);
        let response = response_for(&request(Method::GET, Some("bytes=-100")), &path);

        assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
        assert_eq!(response.body(), &bytes[bytes.len() - 100..]);
    }

    #[test]
    fn a_range_start_at_the_file_length_is_not_satisfiable() {
        let bytes = wav();
        let (_directory, path) = temporary_file(&bytes);
        let range = format!("bytes={}-", bytes.len());
        let response = response_for(&request(Method::GET, Some(&range)), &path);

        assert_eq!(response.status(), StatusCode::RANGE_NOT_SATISFIABLE);
        assert_eq!(
            response.headers()[CONTENT_RANGE],
            format!("bytes */{}", bytes.len())
        );
        assert!(response.body().is_empty());
    }

    #[test]
    fn a_range_whose_end_precedes_its_start_is_not_satisfiable() {
        let bytes = wav();
        let (_directory, path) = temporary_file(&bytes);
        let response = response_for(&request(Method::GET, Some("bytes=50-10")), &path);

        assert_eq!(response.status(), StatusCode::RANGE_NOT_SATISFIABLE);
    }

    #[test]
    fn an_unparseable_range_is_not_satisfiable() {
        let bytes = wav();
        let (_directory, path) = temporary_file(&bytes);
        let response = response_for(&request(Method::GET, Some("bytes=zzz")), &path);

        assert_eq!(response.status(), StatusCode::RANGE_NOT_SATISFIABLE);
    }

    #[test]
    fn multiple_ranges_are_ignored_and_return_the_whole_file() {
        let bytes = wav();
        let (_directory, path) = temporary_file(&bytes);
        let response = response_for(&request(Method::GET, Some("bytes=0-1,5-9")), &path);

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.body(), &bytes);
    }

    #[test]
    fn an_open_range_is_capped_at_one_mebibyte() {
        let bytes = vec![7; 3 * 1024 * 1024];
        let (_directory, path) = temporary_file(&bytes);
        let response = response_for(&request(Method::GET, Some("bytes=0-")), &path);

        assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
        assert_eq!(response.body().len(), 1024 * 1024);
        assert_eq!(
            response.headers()[CONTENT_RANGE],
            format!("bytes 0-1048575/{}", bytes.len())
        );
    }

    #[test]
    fn head_returns_get_headers_without_a_body() {
        let bytes = wav();
        let (_directory, path) = temporary_file(&bytes);
        let response = response_for(&request(Method::HEAD, None), &path);

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[CONTENT_TYPE], "audio/wav");
        assert_eq!(response.headers()[ACCEPT_RANGES], "bytes");
        assert_eq!(response.headers()[CONTENT_LENGTH], bytes.len().to_string());
        assert!(response.body().is_empty());
    }

    #[test]
    fn a_missing_recording_returns_not_found() {
        let directory = tempfile::tempdir().unwrap();
        let response = response_for(
            &request(Method::GET, None),
            &directory.path().join("missing.wav"),
        );

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        assert!(response.body().is_empty());
    }

    #[tokio::test]
    async fn remove_at_accepts_absent_files_and_removes_present_files() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("remove.wav");

        remove_at(&path, "remove.wav").await;
        std::fs::write(&path, wav()).unwrap();
        remove_at(&path, "remove.wav").await;
        assert!(!path.exists());
    }
}
