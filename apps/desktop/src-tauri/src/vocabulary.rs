//! Main-window vocabulary commands. The dictionary is shared with CLI/API clients.
use crate::state::AppState;
use kw_storage::dictionary;
use kw_transcription_core::vocabulary::{prepare, VocabularyReview};
use tauri::{State, WebviewWindow};

fn main_window<R: tauri::Runtime>(window: &WebviewWindow<R>) -> Result<(), String> {
    if window.label() == "main" {
        Ok(())
    } else {
        Err("Vocabulary is available only in the main window".into())
    }
}
#[tauri::command]
pub async fn vocabulary_list<R: tauri::Runtime>(
    window: WebviewWindow<R>,
    state: State<'_, AppState>,
) -> Result<Vec<dictionary::DictionaryWord>, String> {
    main_window(&window)?;
    dictionary::all(state.db().pool())
        .await
        .map_err(|e| e.to_string())
}
#[tauri::command]
pub async fn vocabulary_update<R: tauri::Runtime>(
    window: WebviewWindow<R>,
    state: State<'_, AppState>,
    add: Vec<String>,
    remove: Vec<String>,
) -> Result<dictionary::DictionaryChange, String> {
    main_window(&window)?;
    dictionary::update(state.db().pool(), &add, &remove)
        .await
        .map_err(|e| e.to_string())
}
#[tauri::command]
pub async fn vocabulary_edit<R: tauri::Runtime>(
    window: WebviewWindow<R>,
    state: State<'_, AppState>,
    id: i64,
    word: String,
) -> Result<(), String> {
    main_window(&window)?;
    dictionary::edit(state.db().pool(), id, &word)
        .await
        .map_err(|_| {
            "Could not save vocabulary entry. It may already exist or have been removed.".into()
        })
}
#[tauri::command]
pub async fn vocabulary_review<R: tauri::Runtime>(
    window: WebviewWindow<R>,
    state: State<'_, AppState>,
    provider: String,
    model: Option<String>,
) -> Result<VocabularyReview, String> {
    main_window(&window)?;
    let words = dictionary::words(state.db().pool())
        .await
        .map_err(|e| e.to_string())?;
    Ok(prepare(&provider, model.as_deref(), &words))
}

// One undo slot per application session. Values never cross diagnostic output.
static DELETED: tokio::sync::Mutex<Option<dictionary::DictionaryWord>> =
    tokio::sync::Mutex::const_new(None);
#[tauri::command]
pub async fn vocabulary_delete<R: tauri::Runtime>(
    window: WebviewWindow<R>,
    state: State<'_, AppState>,
    id: i64,
) -> Result<(), String> {
    main_window(&window)?;
    let mut deleted = DELETED.lock().await;
    *deleted = Some(
        dictionary::delete(state.db().pool(), id)
            .await
            .map_err(|e| e.to_string())?,
    );
    Ok(())
}
#[tauri::command]
pub async fn vocabulary_restore<R: tauri::Runtime>(
    window: WebviewWindow<R>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    main_window(&window)?;
    let mut deleted = DELETED.lock().await;
    let word = deleted
        .as_ref()
        .ok_or("There is no vocabulary deletion to undo")?;
    dictionary::restore(state.db().pool(), word)
        .await
        .map_err(|_| "Could not restore this entry; it may already exist")?;
    *deleted = None;
    Ok(())
}

#[cfg(all(test, feature = "tauri-test"))]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    fn invoke(
        window: &WebviewWindow<tauri::test::MockRuntime>,
        command: &str,
        body: Value,
    ) -> Result<Value, Value> {
        tauri::test::get_ipc_response(
            window,
            tauri::webview::InvokeRequest {
                cmd: command.into(),
                callback: tauri::ipc::CallbackFn(0),
                error: tauri::ipc::CallbackFn(1),
                url: window.url().unwrap(),
                body: tauri::ipc::InvokeBody::Json(body),
                headers: Default::default(),
                invoke_key: tauri::test::INVOKE_KEY.into(),
            },
        )
        .map(|response| response.deserialize().unwrap())
    }
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn main_window_crud_review_and_undo_share_storage_and_reject_other_windows() {
        let state = AppState::in_memory().await.unwrap();
        let db = state.db().clone();
        let app = tauri::test::mock_builder()
            .manage(state)
            .invoke_handler(tauri::generate_handler![
                vocabulary_list,
                vocabulary_update,
                vocabulary_edit,
                vocabulary_review,
                vocabulary_delete,
                vocabulary_restore
            ])
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .unwrap();
        let main = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
            .build()
            .unwrap();
        let overlay = tauri::WebviewWindowBuilder::new(&app, "overlay", Default::default())
            .build()
            .unwrap();
        assert!(invoke(
            &overlay,
            "vocabulary_update",
            json!({"add":["forbidden"], "remove":[]})
        )
        .is_err());
        invoke(
            &main,
            "vocabulary_update",
            json!({"add":["  Dyve  ", "KnegisWhisper"], "remove":[]}),
        )
        .unwrap();
        let words = dictionary::all(db.pool()).await.unwrap();
        assert_eq!(words.len(), 2);
        let id = words[0].id;
        invoke(
            &main,
            "vocabulary_edit",
            json!({"id":id,"word":"Dyve Group"}),
        )
        .unwrap();
        assert_eq!(dictionary::all(db.pool()).await.unwrap()[0].id, id);
        let review = invoke(
            &main,
            "vocabulary_review",
            json!({"provider":"whisper","model":"kb-whisper-base"}),
        )
        .unwrap();
        assert_eq!(review["included"], json!(["Dyve Group", "KnegisWhisper"]));
        invoke(&main, "vocabulary_delete", json!({"id":id})).unwrap();
        assert_eq!(
            dictionary::words(db.pool()).await.unwrap(),
            ["KnegisWhisper"]
        );
        invoke(&main, "vocabulary_restore", json!({})).unwrap();
        assert_eq!(dictionary::all(db.pool()).await.unwrap()[0].id, id);
        assert!(invoke(&main, "vocabulary_restore", json!({})).is_err());
        let listed = invoke(&main, "vocabulary_list", json!({})).unwrap();
        assert_eq!(listed.as_array().unwrap().len(), 2);
    }
}
