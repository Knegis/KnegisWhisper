//! Turning API responses into something to read.
//!
//! Two output modes, and the distinction matters more than it looks. `--json`
//! prints the server's answer through unchanged, so a script never depends on
//! this module's formatting choices; the human form is free to change because
//! nothing machine-readable is built on it.
//!
//! Everything here is pure — it takes a `serde_json::Value` and returns a
//! `String` — so the table alignment, the truncation, and the empty-result
//! wording are all tested without a server.

use serde_json::Value;

/// How wide a text column may get before it is shortened.
///
/// Chosen so a title and a snippet still fit an 80-column terminal side by side.
const COLUMN: usize = 48;

/// Print the server's answer verbatim, for scripts.
#[must_use]
pub fn as_json(value: &Value) -> String {
    serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string())
}

/// A one-line summary of a health response.
#[must_use]
pub fn health(value: &Value) -> String {
    let data = value.get("data");
    let version = data
        .and_then(|d| d.get("appVersion"))
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let api = data
        .and_then(|d| d.get("version"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    format!("KnegisWhisper {version} is running (local API v{api})")
}

/// A table of notes.
#[must_use]
pub fn notes(value: &Value) -> String {
    rows(value, "notes", &["id", "title", "noteType"], |row| {
        vec![number(row, "id"), text(row, "title"), text(row, "noteType")]
    })
}

/// A table of search hits.
#[must_use]
pub fn search_hits(value: &Value) -> String {
    rows(
        value,
        "matching notes",
        &["id", "title", "snippet"],
        |row| vec![number(row, "id"), text(row, "title"), text(row, "snippet")],
    )
}

/// A table of folders.
#[must_use]
pub fn folders(value: &Value) -> String {
    rows(value, "folders", &["id", "name"], |row| {
        vec![number(row, "id"), text(row, "name")]
    })
}

/// A table of dictionary words.
#[must_use]
pub fn dictionary(value: &Value) -> String {
    rows(value, "dictionary words", &["word", "source"], |row| {
        vec![text(row, "word"), text(row, "source")]
    })
}

/// A table of dictation history.
#[must_use]
pub fn history(value: &Value) -> String {
    rows(
        value,
        "transcriptions",
        &["id", "when", "provider", "text"],
        |row| {
            vec![
                number(row, "id"),
                text(row, "createdAt"),
                text(row, "provider"),
                text(row, "text"),
            ]
        },
    )
}

/// One note in full.
#[must_use]
pub fn note(value: &Value) -> String {
    let Some(data) = value.get("data") else {
        return "no note in the reply".to_owned();
    };

    let mut out = String::new();
    out.push_str(&format!(
        "#{} {}\n",
        number(data, "id"),
        text(data, "title")
    ));
    if let Some(folder) = data.get("folderId").and_then(Value::as_i64) {
        out.push_str(&format!("folder: {folder}\n"));
    }
    out.push_str(&format!("updated: {}\n\n", text(data, "updatedAt")));
    out.push_str(data.get("content").and_then(Value::as_str).unwrap_or(""));
    out
}

/// What a dictionary update did.
#[must_use]
pub fn dictionary_change(value: &Value) -> String {
    let data = value.get("data");
    let count = |field: &str| {
        data.and_then(|d| d.get(field))
            .and_then(Value::as_u64)
            .unwrap_or(0)
    };

    let added = count("added");
    let removed = count("removed");
    let already = count("alreadyPresent");
    let absent = count("notPresent");

    let mut parts = Vec::new();
    if added > 0 {
        parts.push(format!("added {added}"));
    }
    if removed > 0 {
        parts.push(format!("removed {removed}"));
    }
    // Reported rather than folded into "added": a caller retrying a command
    // should be able to see that nothing new happened.
    if already > 0 {
        parts.push(format!("{already} already there"));
    }
    if absent > 0 {
        parts.push(format!("{absent} not there"));
    }

    if parts.is_empty() {
        "nothing changed".to_owned()
    } else {
        parts.join(", ")
    }
}

/// Render a list response as an aligned table.
fn rows<F>(value: &Value, noun: &str, headers: &[&str], row_of: F) -> String
where
    F: Fn(&Value) -> Vec<String>,
{
    let Some(data) = value.get("data").and_then(Value::as_array) else {
        return format!("no {noun} in the reply");
    };

    if data.is_empty() {
        return format!("no {noun}");
    }

    let mut table: Vec<Vec<String>> = vec![headers.iter().map(|h| (*h).to_owned()).collect()];
    table.extend(data.iter().map(&row_of));

    let columns = headers.len();
    let mut widths = vec![0_usize; columns];
    for row in &table {
        for (index, cell) in row.iter().enumerate() {
            if let Some(width) = widths.get_mut(index) {
                *width = (*width).max(cell.chars().count());
            }
        }
    }

    let mut out = String::new();
    for row in &table {
        let mut line = String::new();
        for (index, cell) in row.iter().enumerate() {
            let width = widths.get(index).copied().unwrap_or(0);
            if index + 1 == columns {
                // No padding on the last column, so a copied line has no
                // trailing whitespace.
                line.push_str(cell);
            } else {
                line.push_str(cell);
                for _ in cell.chars().count()..width {
                    line.push(' ');
                }
                line.push_str("  ");
            }
        }
        out.push_str(line.trim_end());
        out.push('\n');
    }

    if value.get("has_more").and_then(Value::as_bool) == Some(true) {
        // Said plainly, because a truncated list that looks complete is worse
        // than no list.
        out.push_str("\nthere are more; ask for a higher --limit\n");
    }

    out
}

fn number(row: &Value, field: &str) -> String {
    row.get(field)
        .and_then(Value::as_i64)
        .map_or_else(|| "-".to_owned(), |value| value.to_string())
}

/// A field as a single line, shortened if long.
fn text(row: &Value, field: &str) -> String {
    let raw = row.get(field).and_then(Value::as_str).unwrap_or("-");
    // Newlines would break the table; a transcript is one logical line here.
    let flat: String = raw
        .chars()
        .map(|c| {
            if c == '\n' || c == '\r' || c == '\t' {
                ' '
            } else {
                c
            }
        })
        .collect();
    let flat = flat.trim();

    if flat.is_empty() {
        return "-".to_owned();
    }
    if flat.chars().count() <= COLUMN {
        return flat.to_owned();
    }

    let cut: String = flat.chars().take(COLUMN - 1).collect();
    format!("{cut}…")
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
    use serde_json::json;

    #[test]
    fn json_output_is_the_servers_answer_unchanged() {
        // A script must never depend on this module's formatting choices.
        let value = json!({"data": [{"id": 1, "title": "x"}], "has_more": false});
        let rendered = as_json(&value);
        let round_tripped: Value = serde_json::from_str(&rendered).unwrap();
        assert_eq!(round_tripped, value);
    }

    #[test]
    fn health_says_what_is_running() {
        let rendered = health(&json!({"data": {"ok": true, "version": 1, "appVersion": "0.1.0"}}));
        assert!(rendered.contains("0.1.0"));
        assert!(rendered.contains("v1"));
    }

    #[test]
    fn a_table_is_aligned_and_has_no_trailing_whitespace() {
        // Trailing spaces make a copied line awkward to paste.
        let rendered = notes(&json!({
            "data": [
                {"id": 1, "title": "Short", "noteType": "personal"},
                {"id": 1234, "title": "A much longer title", "noteType": "meeting"}
            ],
            "has_more": false
        }));

        for line in rendered.lines() {
            assert_eq!(line, line.trim_end(), "{line:?} has trailing whitespace");
        }
        // The id column is padded to the widest value.
        assert!(rendered.contains("1     "), "{rendered}");
    }

    #[test]
    fn an_empty_list_says_so_rather_than_printing_headers() {
        assert_eq!(notes(&json!({"data": [], "has_more": false})), "no notes");
        assert_eq!(
            dictionary(&json!({"data": [], "has_more": false})),
            "no dictionary words"
        );
    }

    #[test]
    fn a_truncated_list_says_there_is_more() {
        // A truncated list that looks complete is worse than no list.
        let rendered = notes(&json!({
            "data": [{"id": 1, "title": "x", "noteType": "personal"}],
            "has_more": true
        }));
        assert!(rendered.contains("there are more"));
    }

    #[test]
    fn a_complete_list_does_not_mention_more() {
        let rendered = notes(&json!({
            "data": [{"id": 1, "title": "x", "noteType": "personal"}],
            "has_more": false
        }));
        assert!(!rendered.contains("there are more"));
    }

    #[test]
    fn a_long_field_is_shortened_on_a_character_boundary() {
        let long = "å".repeat(200);
        let rendered = history(&json!({
            "data": [{"id": 1, "createdAt": "now", "provider": "local", "text": long}],
            "has_more": false
        }));
        assert!(rendered.contains('…'));
        // Every line stays within a sensible width.
        for line in rendered.lines() {
            assert!(line.chars().count() < 120, "{}", line.chars().count());
        }
    }

    #[test]
    fn newlines_in_a_transcript_do_not_break_the_table() {
        let rendered = history(&json!({
            "data": [
                {"id": 1, "createdAt": "now", "provider": "local", "text": "one\ntwo\nthree"},
                {"id": 2, "createdAt": "now", "provider": "local", "text": "four"}
            ],
            "has_more": false
        }));
        // Header plus two rows, and nothing else.
        assert_eq!(rendered.lines().count(), 3, "{rendered}");
    }

    #[test]
    fn a_missing_field_shows_a_dash_rather_than_null() {
        let rendered = history(&json!({
            "data": [{"id": 1, "text": "hello"}],
            "has_more": false
        }));
        assert!(rendered.contains('-'));
        assert!(!rendered.contains("null"));
    }

    #[test]
    fn a_reply_without_data_says_so_rather_than_pretending_it_is_empty() {
        // "No notes" and "the reply was not what we expected" are different
        // facts, and the second one is a bug worth seeing.
        assert!(notes(&json!({"error": "x"})).contains("in the reply"));
        assert!(note(&json!({})).contains("in the reply"));
    }

    #[test]
    fn one_note_shows_its_content_in_full() {
        // Truncation is for tables; asking for one note means wanting all of it.
        let content = "line one\nline two";
        let rendered = note(&json!({
            "data": {
                "id": 7, "title": "Plan", "content": content,
                "folderId": 2, "updatedAt": "2026-08-25"
            }
        }));
        assert!(rendered.contains("#7 Plan"));
        assert!(rendered.contains("folder: 2"));
        assert!(rendered.contains(content), "content must not be shortened");
    }

    #[test]
    fn a_dictionary_change_reports_every_outcome() {
        assert_eq!(
            dictionary_change(
                &json!({"data": {"added": 2, "alreadyPresent": 0, "removed": 0, "notPresent": 0}})
            ),
            "added 2"
        );
        assert_eq!(
            dictionary_change(
                &json!({"data": {"added": 0, "alreadyPresent": 1, "removed": 0, "notPresent": 0}})
            ),
            "1 already there"
        );
        assert_eq!(
            dictionary_change(
                &json!({"data": {"added": 1, "alreadyPresent": 1, "removed": 2, "notPresent": 3}})
            ),
            "added 1, removed 2, 1 already there, 3 not there"
        );
    }

    #[test]
    fn a_change_that_did_nothing_says_nothing_changed() {
        assert_eq!(
            dictionary_change(
                &json!({"data": {"added": 0, "alreadyPresent": 0, "removed": 0, "notPresent": 0}})
            ),
            "nothing changed"
        );
    }
}
