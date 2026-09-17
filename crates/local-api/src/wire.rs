//! Request and response shapes for the local API, and the validation of them.
//!
//! Everything here is pure, so the awkward inputs — a negative id, a limit of
//! four billion, a search for whitespace, a dictionary update carrying a number
//! where a word should be — are covered without a socket.
//!
//! The error shape is `{"error": {"code": "...", "message": "..."}}`, kept from
//! upstream because a CLI that already parses it should keep working, and
//! because a stable machine-readable `code` is what lets a script branch without
//! matching on prose.

use serde::{Deserialize, Serialize};

/// The largest page a caller may ask for.
///
/// A limit exists at all because the alternative is a caller asking for
/// everything and this process holding every note in memory to serialise it.
pub const MAX_LIMIT: u32 = 500;

/// What a caller gets when they do not say.
pub const DEFAULT_LIMIT: u32 = 100;

/// The most a request body may be.
///
/// Upstream's figure. A dictionary update is the largest legitimate body and it
/// is a list of words.
pub const MAX_BODY_BYTES: usize = 1024 * 1024;

/// A machine-readable failure.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ApiError {
    pub code: ErrorCode,
    pub message: String,
}

/// The codes a client may branch on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    Unauthorized,
    Forbidden,
    NotFound,
    ValidationError,
    PayloadTooLarge,
    InternalError,
    Conflict,
    Unavailable,
}

impl ErrorCode {
    /// The HTTP status that goes with a code, so the two cannot disagree.
    #[must_use]
    pub const fn status(self) -> u16 {
        match self {
            Self::Unauthorized => 401,
            Self::Forbidden => 403,
            Self::NotFound => 404,
            Self::ValidationError => 400,
            Self::PayloadTooLarge => 413,
            Self::InternalError => 500,
            Self::Conflict => 409,
            Self::Unavailable => 503,
        }
    }
}

impl ApiError {
    #[must_use]
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    #[must_use]
    pub fn not_found(what: &str) -> Self {
        Self::new(ErrorCode::NotFound, format!("no {what} with that id"))
    }

    #[must_use]
    pub fn validation(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::ValidationError, message)
    }

    /// An internal failure, with the detail kept out of the response.
    ///
    /// The caller gets "something went wrong"; the detail goes to the log. A
    /// database error message can carry a file path or a column name, and this
    /// endpoint answers anything that can read the discovery file.
    #[must_use]
    pub fn internal() -> Self {
        Self::new(
            ErrorCode::InternalError,
            "the request could not be completed",
        )
    }
}

/// The envelope every successful list response uses.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct Page<T> {
    pub data: Vec<T>,
    /// Whether another page exists. Determined by asking for one more row than
    /// requested, so it is a fact rather than a guess.
    pub has_more: bool,
    /// The `after` value for the next page, when there is one.
    pub next_cursor: Option<i64>,
}

/// The envelope every successful single-item response uses.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Item<T> {
    pub data: T,
}

/// What `GET /v1/health` answers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Health {
    pub ok: bool,
    pub version: u32,
    /// The application version, so a client can tell what it is talking to.
    pub app_version: &'static str,
}

/// Parse a path id.
///
/// Refuses zero and negatives rather than passing them to the database, where
/// they would come back as "not found" anyway but only after a query.
pub fn parse_id(raw: &str) -> Result<i64, ApiError> {
    match raw.parse::<i64>() {
        Ok(id) if id > 0 => Ok(id),
        _ => Err(ApiError::new(
            ErrorCode::NotFound,
            format!("{raw:?} is not an id"),
        )),
    }
}

/// The query parameters for a list.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ListParams {
    pub note_type: Option<String>,
    pub limit: Option<u32>,
    pub folder_id: Option<i64>,
    /// Cursor: return rows with an id below this one.
    pub after: Option<i64>,
}

/// A validated list request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListRequest {
    pub note_type: Option<String>,
    pub limit: u32,
    pub folder_id: Option<i64>,
    pub after: Option<i64>,
}

impl ListParams {
    /// Validate and apply defaults.
    ///
    /// A limit above the maximum is clamped rather than refused: the caller
    /// asked for as much as possible and gets as much as possible, which is a
    /// more useful answer than an error. A limit of zero *is* refused, because
    /// it is more likely a bug in the caller than a request for nothing.
    pub fn validate(self) -> Result<ListRequest, ApiError> {
        let limit = match self.limit {
            None => DEFAULT_LIMIT,
            Some(0) => {
                return Err(ApiError::validation(
                    "'limit' of 0 returns nothing; omit it for the default",
                ))
            }
            Some(requested) => requested.min(MAX_LIMIT),
        };

        if let Some(folder_id) = self.folder_id {
            if folder_id <= 0 {
                return Err(ApiError::validation("'folder_id' must be a positive id"));
            }
        }
        if let Some(after) = self.after {
            if after <= 0 {
                return Err(ApiError::validation("'after' must be a positive id"));
            }
        }
        if let Some(note_type) = &self.note_type {
            if note_type.trim().is_empty() {
                return Err(ApiError::validation("'note_type' cannot be blank"));
            }
        }

        Ok(ListRequest {
            note_type: self.note_type,
            limit,
            folder_id: self.folder_id,
            after: self.after,
        })
    }
}

/// The query parameters for a search.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct SearchParams {
    pub q: Option<String>,
    pub limit: Option<u32>,
}

/// A validated search request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchRequest {
    pub query: String,
    pub limit: u32,
}

impl SearchParams {
    pub fn validate(self) -> Result<SearchRequest, ApiError> {
        let query = self.q.unwrap_or_default();
        if query.trim().is_empty() {
            return Err(ApiError::validation("'q' is required"));
        }

        let limit = match self.limit {
            None => DEFAULT_LIMIT,
            Some(0) => return Err(ApiError::validation("'limit' of 0 returns nothing")),
            Some(requested) => requested.min(MAX_LIMIT),
        };

        Ok(SearchRequest { query, limit })
    }
}

/// The body of `PATCH /v1/notes/{id}`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct NotePatch {
    pub title: Option<String>,
    pub content: Option<String>,
    pub folder_id: Option<i64>,
}

impl NotePatch {
    /// Refuse a patch that would change nothing.
    ///
    /// An empty body is almost always a client bug — a mistyped field name, a
    /// body that did not get sent — and answering 200 to it hides that.
    pub fn validate(&self) -> Result<(), ApiError> {
        if self.title.is_none() && self.content.is_none() && self.folder_id.is_none() {
            return Err(ApiError::validation(
                "nothing to change: send at least one of 'title', 'content', or 'folder_id'",
            ));
        }
        if let Some(folder_id) = self.folder_id {
            if folder_id <= 0 {
                return Err(ApiError::validation("'folder_id' must be a positive id"));
            }
        }
        if let Some(title) = &self.title {
            if title.len() > 1000 {
                return Err(ApiError::validation("'title' is too long"));
            }
        }
        Ok(())
    }
}

/// The body of `POST /v1/dictionary/update`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct DictionaryUpdate {
    #[serde(default)]
    pub add: Vec<String>,
    #[serde(default)]
    pub remove: Vec<String>,
}

/// A validated dictionary update.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DictionaryRequest {
    pub add: Vec<String>,
    pub remove: Vec<String>,
}

impl DictionaryUpdate {
    /// Validate, trim, and drop duplicates.
    ///
    /// A word appearing in both lists is a contradiction rather than something to
    /// resolve silently: whichever order it was applied in, the caller would get
    /// a result they did not ask for.
    pub fn validate(self) -> Result<DictionaryRequest, ApiError> {
        let add = clean(self.add, "add")?;
        let remove = clean(self.remove, "remove")?;

        if add.is_empty() && remove.is_empty() {
            return Err(ApiError::validation(
                "nothing to do: send words in 'add' or 'remove'",
            ));
        }

        if let Some(both) = add.iter().find(|word| remove.contains(*word)) {
            return Err(ApiError::validation(format!(
                "{both:?} is in both 'add' and 'remove'"
            )));
        }

        Ok(DictionaryRequest { add, remove })
    }
}

fn clean(words: Vec<String>, field: &str) -> Result<Vec<String>, ApiError> {
    let mut cleaned: Vec<String> = Vec::with_capacity(words.len());
    for word in words {
        let normalized = kw_storage::dictionary::normalize(&word);
        let trimmed = normalized.as_str();
        if trimmed.is_empty() {
            return Err(ApiError::validation(format!(
                "'{field}' contains a blank word"
            )));
        }
        if trimmed.chars().count() > 100 {
            return Err(ApiError::validation(format!(
                "'{field}' contains a word longer than 100 characters"
            )));
        }
        let owned = trimmed.to_owned();
        if !cleaned.contains(&owned) {
            cleaned.push(owned);
        }
    }
    Ok(cleaned)
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

    // ----------------------------------------------------------------- errors

    #[test]
    fn each_code_maps_to_one_status() {
        assert_eq!(ErrorCode::Unauthorized.status(), 401);
        assert_eq!(ErrorCode::Forbidden.status(), 403);
        assert_eq!(ErrorCode::NotFound.status(), 404);
        assert_eq!(ErrorCode::ValidationError.status(), 400);
        assert_eq!(ErrorCode::PayloadTooLarge.status(), 413);
        assert_eq!(ErrorCode::InternalError.status(), 500);
    }

    #[test]
    fn the_error_shape_is_the_one_a_client_parses() {
        let json = serde_json::to_value(ApiError::not_found("note")).unwrap();
        assert_eq!(json["code"], "not_found");
        assert!(json["message"].as_str().unwrap().contains("note"));
    }

    #[test]
    fn an_internal_error_never_carries_the_detail() {
        // This endpoint answers anything that can read the discovery file, and a
        // database error can carry a file path or a column name.
        let rendered = serde_json::to_string(&ApiError::internal()).unwrap();
        assert!(!rendered.contains("sqlite"));
        assert!(!rendered.contains("no such column"));
        assert!(rendered.contains("could not be completed"));
    }

    // --------------------------------------------------------------- path ids

    #[test]
    fn a_positive_id_parses() {
        assert_eq!(parse_id("42").unwrap(), 42);
    }

    #[test]
    fn anything_that_is_not_a_positive_id_is_a_404() {
        for raw in [
            "0",
            "-1",
            "abc",
            "",
            " 1",
            "1.5",
            "1e3",
            "99999999999999999999",
        ] {
            let err = parse_id(raw).unwrap_err();
            assert_eq!(err.code, ErrorCode::NotFound, "{raw:?}");
        }
    }

    // ------------------------------------------------------------------ lists

    #[test]
    fn an_absent_limit_gets_the_default() {
        let request = ListParams::default().validate().unwrap();
        assert_eq!(request.limit, DEFAULT_LIMIT);
    }

    #[test]
    fn an_enormous_limit_is_clamped_rather_than_refused() {
        // The caller asked for as much as possible, and gets as much as
        // possible. An error here would be pedantry.
        let request = ListParams {
            limit: Some(u32::MAX),
            ..Default::default()
        }
        .validate()
        .unwrap();
        assert_eq!(request.limit, MAX_LIMIT);
    }

    #[test]
    fn a_limit_of_zero_is_refused_as_a_probable_bug() {
        let err = ListParams {
            limit: Some(0),
            ..Default::default()
        }
        .validate()
        .unwrap_err();
        assert_eq!(err.code, ErrorCode::ValidationError);
        assert!(err.message.contains("omit it"));
    }

    #[test]
    fn a_non_positive_folder_or_cursor_is_refused() {
        for params in [
            ListParams {
                folder_id: Some(0),
                ..Default::default()
            },
            ListParams {
                folder_id: Some(-3),
                ..Default::default()
            },
            ListParams {
                after: Some(0),
                ..Default::default()
            },
        ] {
            assert_eq!(
                params.clone().validate().unwrap_err().code,
                ErrorCode::ValidationError,
                "{params:?}"
            );
        }
    }

    #[test]
    fn a_blank_note_type_is_refused_rather_than_matching_nothing() {
        let err = ListParams {
            note_type: Some("   ".to_owned()),
            ..Default::default()
        }
        .validate()
        .unwrap_err();
        assert!(err.message.contains("note_type"));
    }

    #[test]
    fn list_params_deserialize_from_snake_case_query_keys() {
        // The names a shell script would use, and the ones upstream's CLI sends.
        let params: ListParams =
            serde_json::from_str(r#"{"note_type":"dictation","limit":10,"folder_id":3}"#).unwrap();
        let request = params.validate().unwrap();
        assert_eq!(request.note_type.as_deref(), Some("dictation"));
        assert_eq!(request.limit, 10);
        assert_eq!(request.folder_id, Some(3));
    }

    // ----------------------------------------------------------------- search

    #[test]
    fn a_search_needs_a_query() {
        for q in [None, Some(String::new()), Some("   ".to_owned())] {
            let err = SearchParams {
                q,
                ..Default::default()
            }
            .validate()
            .unwrap_err();
            assert!(err.message.contains("'q' is required"));
        }
    }

    #[test]
    fn a_search_query_is_kept_verbatim_for_the_search_layer_to_escape() {
        // notes_core::escape_query owns quoting. Doing it here as well would
        // mean two places deciding what FTS5 syntax means.
        let request = SearchParams {
            q: Some("\"quoted\" AND thing*".to_owned()),
            limit: None,
        }
        .validate()
        .unwrap();
        assert_eq!(request.query, "\"quoted\" AND thing*");
    }

    // ------------------------------------------------------------ note patch

    #[test]
    fn a_patch_that_changes_nothing_is_refused() {
        // Almost always a mistyped field name or a body that never got sent.
        let err = NotePatch::default().validate().unwrap_err();
        assert!(err.message.contains("nothing to change"));
    }

    #[test]
    fn a_patch_with_any_one_field_is_accepted() {
        for patch in [
            NotePatch {
                title: Some("t".to_owned()),
                ..Default::default()
            },
            NotePatch {
                content: Some("c".to_owned()),
                ..Default::default()
            },
            NotePatch {
                folder_id: Some(1),
                ..Default::default()
            },
        ] {
            patch.validate().unwrap();
        }
    }

    #[test]
    fn a_patch_with_an_unknown_field_is_refused_rather_than_ignored() {
        // Silently ignoring `titel` would answer 200 while changing nothing.
        assert!(serde_json::from_str::<NotePatch>(r#"{"titel":"typo"}"#).is_err());
    }

    #[test]
    fn a_patch_may_set_content_to_empty_but_not_omit_everything() {
        // Clearing a note is a real thing to want; changing nothing is not.
        NotePatch {
            content: Some(String::new()),
            ..Default::default()
        }
        .validate()
        .unwrap();
    }

    // ------------------------------------------------------------- dictionary

    #[test]
    fn a_dictionary_update_trims_and_deduplicates() {
        let request = DictionaryUpdate {
            add: vec![
                " Knegis ".to_owned(),
                "Knegis".to_owned(),
                "Whisper".to_owned(),
            ],
            remove: Vec::new(),
        }
        .validate()
        .unwrap();
        assert_eq!(request.add, ["Knegis", "Whisper"]);
    }

    #[test]
    fn a_word_in_both_lists_is_a_contradiction_not_something_to_resolve() {
        // Whichever order it was applied in, the caller gets a result they did
        // not ask for.
        let err = DictionaryUpdate {
            add: vec!["Knegis".to_owned()],
            remove: vec!["Knegis".to_owned()],
        }
        .validate()
        .unwrap_err();
        assert!(err.message.contains("both"));
    }

    #[test]
    fn an_empty_dictionary_update_is_refused() {
        let err = DictionaryUpdate::default().validate().unwrap_err();
        assert!(err.message.contains("nothing to do"));
    }

    #[test]
    fn a_blank_word_is_refused_and_the_field_is_named() {
        let err = DictionaryUpdate {
            add: vec!["ok".to_owned(), "  ".to_owned()],
            remove: Vec::new(),
        }
        .validate()
        .unwrap_err();
        assert!(err.message.contains("'add'"), "{}", err.message);
    }

    #[test]
    fn an_absurdly_long_word_is_refused() {
        let err = DictionaryUpdate {
            add: vec!["a".repeat(101)],
            remove: Vec::new(),
        }
        .validate()
        .unwrap_err();
        assert!(err.message.contains("longer than 100"));
    }

    #[test]
    fn a_dictionary_update_with_a_number_where_a_word_belongs_is_refused() {
        assert!(serde_json::from_str::<DictionaryUpdate>(r#"{"add":[1,2]}"#).is_err());
    }

    #[test]
    fn one_sided_updates_are_fine() {
        let only_add = DictionaryUpdate {
            add: vec!["a".to_owned()],
            remove: Vec::new(),
        }
        .validate()
        .unwrap();
        assert!(only_add.remove.is_empty());

        let only_remove = DictionaryUpdate {
            add: Vec::new(),
            remove: vec!["b".to_owned()],
        }
        .validate()
        .unwrap();
        assert!(only_remove.add.is_empty());
    }

    // ---------------------------------------------------------------- shapes

    #[test]
    fn a_page_serialises_with_snake_case_keys() {
        let page = Page {
            data: vec![1, 2, 3],
            has_more: true,
            next_cursor: Some(7),
        };
        let json = serde_json::to_value(&page).unwrap();
        assert_eq!(json["has_more"], true);
        assert_eq!(json["next_cursor"], 7);
        assert_eq!(json["data"][0], 1);
    }

    #[test]
    fn health_reports_the_application_version() {
        // So a client can tell what it is talking to rather than assuming.
        let health = Health {
            ok: true,
            version: 1,
            app_version: env!("CARGO_PKG_VERSION"),
        };
        let json = serde_json::to_value(&health).unwrap();
        assert_eq!(json["ok"], true);
        assert!(!json["appVersion"].as_str().unwrap().is_empty());
    }
}
