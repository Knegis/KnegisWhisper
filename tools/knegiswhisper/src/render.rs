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

/// How wide a continuation note may get.
///
/// More generous than [`COLUMN`] because a note has the line to itself, and a
/// provider's error is the one string here where the useful half is usually at
/// the end -- an HTTP status, a rate-limit reason. Cutting it at 48 would keep
/// the boilerplate and drop the fact. Still bounded, and still under the 120
/// characters `a_long_field_is_shortened_on_a_character_boundary` holds every
/// line to.
const DETAIL: usize = 100;

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
///
/// **`status` is a column of its own, and the reason a failure gets a line of
/// its own.** Until 2026-08-31 this rendered `id, when, provider, text`, and a
/// failed dictation has neither a provider nor text -- so it printed two dashes
/// and said nothing:
///
/// ```text
/// id  when                 provider  text
/// 19  2026-08-26 09:51:11  -         -
/// ```
///
/// The API had the answer the whole time: `/v1/transcriptions/list` returns
/// `status` and `errorMessage`, and the dashboard showed both. This renderer
/// discarded them (KNEG-36).
///
/// **Why a column and not a prefix on the text.** The obvious cheap fix -- put
/// the error where the transcript would go -- is wrong, and the case that
/// proves it is a dictation that transcribed correctly and then failed to
/// paste. `Dictation::fail` records that one with `status: "failed"`, a
/// provider, *and* the recovered text, because the words are the valuable half
/// and losing them is the worse failure. Writing the error into that cell would
/// destroy exactly what the recovery exists to keep. So the status is a column,
/// the transcript stays in its own, and the reason goes on a continuation line
/// where it never has to compete with either.
///
/// `cancelled` falls out of the same change without costing anything: it also
/// had no provider and no text, and was equally indistinguishable from a
/// successful dictation of nothing.
#[must_use]
pub fn history(value: &Value) -> String {
    rows_noted(
        value,
        "transcriptions",
        &["id", "when", "status", "provider", "text"],
        |row| {
            vec![
                number(row, "id"),
                text(row, "createdAt"),
                text(row, "status"),
                text(row, "provider"),
                text(row, "text"),
            ]
        },
        // Keyed on the message being there rather than on the status being
        // "failed": if a completed row ever carries one, that is worth seeing
        // too, and this way the renderer never has to agree with the writer
        // about which statuses may explain themselves.
        |row| detail(row, "errorMessage"),
    )
}

/// One stored dictation, for `history get`.
///
/// **Prints the transcript alone when there is one**, so
/// `knegiswhisper history get 3 > note.txt` still writes a file with nothing in
/// it but the words. That is the command's whole job and the reason it does not
/// print a report.
///
/// A failed dictation has no transcript, and this used to print the empty
/// string -- a command that appears to succeed and emits a blank line, which is
/// worse than the two dashes `history list` printed, because there is not even
/// a row to notice. It now says what happened instead.
///
/// **A known limit, stated rather than left to be discovered:** a dictation
/// that transcribed and then failed to paste has both a transcript and a
/// reason, and this prints only the transcript. Saying more would put prose
/// into a redirected file. `history list` shows the status and the reason for
/// that row, and `--json` carries every field.
#[must_use]
pub fn transcript(value: &Value) -> String {
    let Some(data) = value.get("data") else {
        return "no transcription in the reply".to_owned();
    };

    let words = data.get("text").and_then(Value::as_str).unwrap_or("");
    if !words.trim().is_empty() {
        return words.to_owned();
    }

    let status = data
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    match data.get("errorMessage").and_then(Value::as_str) {
        Some(why) if !why.trim().is_empty() => {
            let why = terminal_safe(why);
            format!("{status}: {}", why.trim())
        }
        // No message is not the same as no failure, and saying "failed" with
        // nothing after it is the honest version of that.
        _ => format!("{status}, and no transcript was stored"),
    }
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
    rows_noted(value, noun, headers, row_of, |_| None)
}

/// [`rows`], plus a line under any record that has something more to say.
///
/// One table renderer rather than two: the four callers that have no notes pass
/// a closure that never returns one and are unchanged by this existing. A
/// second copy of the width arithmetic is how the two would drift.
///
/// A note is deliberately *not* a column. An error message is present on a
/// small minority of rows and long when it is present, so a column would be
/// mostly empty and would push the transcript off an 80-column terminal for
/// every row to accommodate a few.
fn rows_noted<F, N>(value: &Value, noun: &str, headers: &[&str], row_of: F, note_of: N) -> String
where
    F: Fn(&Value) -> Vec<String>,
    N: Fn(&Value) -> Option<String>,
{
    let Some(data) = value.get("data").and_then(Value::as_array) else {
        return format!("no {noun} in the reply");
    };

    if data.is_empty() {
        return format!("no {noun}");
    }

    let mut table: Vec<Vec<String>> = vec![headers.iter().map(|h| (*h).to_owned()).collect()];
    table.extend(data.iter().map(&row_of));

    // Aligned with `table`, so the header's `None` keeps the indices in step.
    let mut notes: Vec<Option<String>> = vec![None];
    notes.extend(data.iter().map(&note_of));

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
    for (position, row) in table.iter().enumerate() {
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

        if let Some(Some(note)) = notes.get(position) {
            // Indented under the id column and marked, so it reads as belonging
            // to the row above rather than as a row of its own.
            out.push_str(&format!("      ↳ {note}\n"));
        }
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
    match one_line(raw, COLUMN) {
        Some(flat) => flat,
        None => "-".to_owned(),
    }
}

/// A field for a continuation line, or nothing when there is none.
///
/// Named for the line it produces rather than for the field, because `note` is
/// already a note in this crate's other sense -- the thing `notes` lists.
fn detail(row: &Value, field: &str) -> Option<String> {
    one_line(row.get(field).and_then(Value::as_str)?, DETAIL)
}

/// One line, shortened if long, or `None` when there is nothing to show.
///
/// Shared by the cells and the notes so there is one truncation rule. The
/// character-boundary take matters: a byte slice would panic on the `å`
/// this is tested with.
fn one_line(raw: &str, limit: usize) -> Option<String> {
    let flat = terminal_safe(raw);
    let flat = flat.trim();

    if flat.is_empty() {
        return None;
    }
    if flat.chars().count() <= limit {
        return Some(flat.to_owned());
    }

    let cut: String = flat.chars().take(limit - 1).collect();
    Some(format!("{cut}…"))
}

/// Prevent untrusted text from being interpreted as terminal instructions.
///
/// This also makes newlines and tabs safe for the single-line table layout.
fn terminal_safe(raw: &str) -> String {
    raw.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect()
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

    /* ------------------------------------------- a dictation that did not work */

    /// The row from KNEG-36, rendered.
    ///
    /// The old output was `19  2026-08-26 09:51:11  -  -`, which is a record
    /// that a dictation happened and nothing else. Both dashes were true and
    /// neither was the fact.
    #[test]
    fn a_failed_dictation_says_so_and_says_why() {
        let rendered = history(&json!({
            "data": [{
                "id": 19,
                "createdAt": "2026-08-26 09:51:11",
                "status": "failed",
                "provider": null,
                "text": "",
                "errorMessage": "provider returned 429"
            }],
            "has_more": false
        }));
        assert!(rendered.contains("failed"), "{rendered}");
        assert!(rendered.contains("provider returned 429"), "{rendered}");
    }

    /// The case that decided the design.
    ///
    /// `Dictation::fail` records a dictation that transcribed and then failed
    /// to paste with the recovered text *and* the reason. Putting the error in
    /// the text column -- the obvious cheap fix -- would have thrown away the
    /// words, which are the half worth keeping.
    #[test]
    fn a_recovered_failure_keeps_both_its_transcript_and_its_reason() {
        let rendered = history(&json!({
            "data": [{
                "id": 20,
                "createdAt": "now",
                "status": "failed",
                "provider": "local",
                "text": "the quick brown fox",
                "errorMessage": "the paste was refused by the target window"
            }],
            "has_more": false
        }));
        assert!(rendered.contains("the quick brown fox"), "{rendered}");
        assert!(
            rendered.contains("the paste was refused by the target window"),
            "{rendered}"
        );
        assert!(
            rendered.contains("local"),
            "the provider survives too: {rendered}"
        );
    }

    /// Three ways to have no text, and they are not the same thing.
    ///
    /// Before the status column all three printed identically, so "I cancelled
    /// it", "it broke" and "the microphone heard nothing" were one row.
    #[test]
    fn cancelled_failed_and_heard_nothing_are_told_apart() {
        let rendered = history(&json!({
            "data": [
                {"id": 1, "createdAt": "now", "status": "cancelled", "text": ""},
                {"id": 2, "createdAt": "now", "status": "failed", "text": "",
                 "errorMessage": "no microphone"},
                {"id": 3, "createdAt": "now", "status": "completed", "provider": "local",
                 "text": ""}
            ],
            "has_more": false
        }));
        for status in ["cancelled", "failed", "completed"] {
            assert!(
                rendered.contains(status),
                "{status} missing from {rendered}"
            );
        }
        // And the one with a reason is the only one that gets a second line:
        // header, three rows, one note.
        assert_eq!(rendered.lines().count(), 5, "{rendered}");
    }

    /// A note must not be mistakable for a record.
    #[test]
    fn a_continuation_line_does_not_look_like_a_row() {
        let rendered = history(&json!({
            "data": [{"id": 7, "createdAt": "now", "status": "failed", "text": "",
                      "errorMessage": "provider returned 429"}],
            "has_more": false
        }));
        let note = rendered
            .lines()
            .find(|l| l.contains("429"))
            .expect("the reason is rendered");
        assert!(note.starts_with("      \u{21b3} "), "{note:?}");
        assert!(!note.trim_start().starts_with('7'), "{note:?}");
    }

    /// A provider that answers with a wall of JSON must not wreck the table.
    #[test]
    fn a_long_reason_stays_on_one_bounded_line() {
        let rendered = history(&json!({
            "data": [{"id": 1, "createdAt": "now", "status": "failed", "text": "",
                      "errorMessage": format!("{}\nsecond line", "e".repeat(400))}],
            "has_more": false
        }));
        assert_eq!(
            rendered.lines().count(),
            3,
            "header, row, one note: {rendered}"
        );
        for line in rendered.lines() {
            assert!(line.chars().count() < 120, "{}", line.chars().count());
        }
    }

    #[test]
    fn terminal_controls_in_a_history_reason_are_neutralized() {
        let rendered = history(&json!({
            "data": [{"id": 1, "createdAt": "now", "status": "failed", "text": "",
                      "errorMessage": "provider said \u{1b}]52;c;QUJD\u{7}no"}],
            "has_more": false
        }));
        assert!(!rendered.contains('\u{1b}'), "{rendered:?}");
        assert!(!rendered.contains('\u{7}'), "{rendered:?}");
        assert!(rendered.contains("provider said"), "{rendered}");
    }

    /* ------------------------------------------------------------ history get */

    /// `history get` stays pipeable.
    #[test]
    fn one_transcription_prints_its_words_and_nothing_else() {
        let rendered = transcript(&json!({
            "data": {"id": 1, "status": "completed", "text": "the quick brown fox"}
        }));
        assert_eq!(rendered, "the quick brown fox");
    }

    /// It used to print the empty string here -- a command that looks like it
    /// worked and emits a blank line.
    #[test]
    fn one_failed_transcription_says_what_happened_rather_than_nothing() {
        let rendered = transcript(&json!({
            "data": {"id": 19, "status": "failed", "text": "",
                     "errorMessage": "provider returned 429"}
        }));
        assert!(!rendered.trim().is_empty());
        assert!(rendered.contains("failed"), "{rendered}");
        assert!(rendered.contains("provider returned 429"), "{rendered}");
    }

    #[test]
    fn terminal_controls_in_one_failed_transcription_are_neutralized() {
        let rendered = transcript(&json!({
            "data": {"id": 19, "status": "failed", "text": "",
                     "errorMessage": "provider said \u{1b}]52;c;QUJD\u{7}no"}
        }));
        assert!(!rendered.contains('\u{1b}'), "{rendered:?}");
        assert!(!rendered.contains('\u{7}'), "{rendered:?}");
        assert!(rendered.contains("provider said"), "{rendered}");
    }

    /// No message is not the same as no failure.
    #[test]
    fn a_failure_with_no_message_still_says_it_failed() {
        let rendered = transcript(&json!({
            "data": {"id": 19, "status": "cancelled", "text": ""}
        }));
        assert!(rendered.contains("cancelled"), "{rendered}");
        assert!(rendered.contains("no transcript"), "{rendered}");
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
