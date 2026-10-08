//! People in notes (KNEG-362): who a note was with, found by their email.
//!
//! `notes.participants` stays what it always was -- a JSON array of
//! `{displayName, email}`, kept raw and read leniently -- because imports
//! wrote it before anything here could, and an export's header reads it as it
//! stands (`transcript::participants`). This module adds the app's one writer
//! of it and the two readers the Notes panel needs: everyone named across the
//! notes, and the notes one person is in.
//!
//! **A person is their email.** Two entries that differ only in case or in
//! surrounding spaces are one person, and an entry with a name and no email is
//! nobody this module can count or filter by: the People list leaves it out
//! (the owner's ruling of 2026-10-07). Names drift -- a calendar sends one
//! spelling, somebody types another -- so the name a person is listed by is
//! the one on the most recently updated note that gives one.
//!
//! Reading is guarded as `0008_transcript_rows.sql` guards `json_each`: a
//! `participants` that is not a JSON array is no people, never an error, since
//! `json_each` raises on malformed JSON and one bad note would otherwise fail
//! the whole list.

use kw_storage::StorageError;
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, SqlitePool};
use ts_rs::TS;

use crate::notes::{Note, COLUMNS};

/// The most people one note keeps.
pub const MAX_PARTICIPANTS: usize = 200;

/// The longest email address kept, in characters: RFC 5321's limit on a
/// path, less its two angle brackets.
const MAX_EMAIL_CHARS: usize = 254;

/// One of a note's people, as `notes.participants` stores them: an email, a
/// name, or both. An absent field is left out of the JSON rather than written
/// as `null`, as imports write it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Participant {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
}

/// Someone named by email on at least one note: a row of the Notes panel's
/// People list.
#[derive(Debug, Clone, PartialEq, Eq, FromRow, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Person {
    /// Trimmed and lower-case, which is how a person is told apart, and what
    /// [`list_by_person`] is given to find their notes.
    pub email: String,
    /// The display name on the most recently updated note that gives one.
    /// `None` when no note names them.
    pub name: Option<String>,
    /// How many notes they are on.
    pub note_count: i64,
}

/// Why a note's people weren't saved. The first three are the person's
/// mistake and say so in plain words; the screen shows them as they are.
#[derive(Debug, thiserror::Error)]
pub enum PeopleError {
    #[error("'{0}' isn't an email address.")]
    NotAnEmail(String),
    #[error("Each person needs an email address or a name.")]
    Unnamed,
    #[error("A note can have at most {MAX_PARTICIPANTS} people.")]
    TooMany,
    #[error("could not write the people: {0}")]
    Encode(#[from] serde_json::Error),
    #[error(transparent)]
    Storage(#[from] StorageError),
}

/// `input` as an email address is kept: trimmed and ASCII lower-case, or
/// `None` when it isn't one.
///
/// Deliberately loose. An address needs exactly one `@` with something on
/// both sides, no whitespace, and at most 254 characters; anything stricter
/// refuses real addresses (a `+` tag, an internationalised domain) for no
/// gain, since nothing here sends mail to it. Only ASCII is folded, as
/// SQLite's `lower()` folds it, so the people list and its filter agree.
#[must_use]
pub fn normalise_email(input: &str) -> Option<String> {
    let email = input.trim().to_ascii_lowercase();
    if email.chars().count() > MAX_EMAIL_CHARS || email.chars().any(char::is_whitespace) {
        return None;
    }
    let (local, domain) = email.split_once('@')?;
    if local.is_empty() || domain.is_empty() || domain.contains('@') {
        return None;
    }
    Some(email)
}

/// `list` as it is stored: names and emails trimmed, emails normalised, and a
/// second entry for an email already listed dropped. Refused when an email
/// isn't one, when an entry has neither an email nor a name, or as soon as
/// more than [`MAX_PARTICIPANTS`] people are kept -- so however long a list
/// is sent, each entry is compared with at most that many.
fn tidy(list: &[Participant]) -> Result<Vec<Participant>, PeopleError> {
    let mut kept: Vec<Participant> = Vec::with_capacity(list.len().min(MAX_PARTICIPANTS));
    for entry in list {
        let display_name = present(entry.display_name.as_deref()).map(str::to_owned);
        let email = match present(entry.email.as_deref()) {
            None => None,
            Some(raw) => {
                Some(normalise_email(raw).ok_or_else(|| PeopleError::NotAnEmail(raw.to_owned()))?)
            }
        };
        if display_name.is_none() && email.is_none() {
            return Err(PeopleError::Unnamed);
        }
        let listed = email
            .as_ref()
            .is_some_and(|email| kept.iter().any(|p| p.email.as_ref() == Some(email)));
        if listed {
            continue;
        }
        if kept.len() == MAX_PARTICIPANTS {
            return Err(PeopleError::TooMany);
        }
        kept.push(Participant {
            display_name,
            email,
        });
    }
    Ok(kept)
}

/// `value` trimmed, or `None` when nothing is left.
fn present(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

/// Replace note `id`'s people with `list`, tidied as `tidy` tidies it, and
/// say whether the note exists. An empty list stores NULL, as a note that
/// never had any.
///
/// `updated_at` is not touched, as `notes::set_folder` leaves it: saying who
/// was there is not working on the note, and the library orders by it.
pub async fn set_participants(
    pool: &SqlitePool,
    id: i64,
    list: &[Participant],
) -> Result<bool, PeopleError> {
    let kept = tidy(list)?;
    let stored = if kept.is_empty() {
        None
    } else {
        Some(serde_json::to_string(&kept)?)
    };
    let result = sqlx::query("UPDATE notes SET participants = ? WHERE id = ?")
        .bind(stored)
        .bind(id)
        .execute(pool)
        .await
        .map_err(StorageError::from)?;
    Ok(result.rows_affected() > 0)
}

/// Everyone named by email on a note, the most notes first, then by the name
/// they are listed by (their email where no note names them).
///
/// One statement: every entry with a text email, grouped by that email
/// trimmed and lower-cased, its name the first non-blank one in the order
/// newest note first.
pub async fn people(pool: &SqlitePool) -> Result<Vec<Person>, StorageError> {
    Ok(sqlx::query_as::<_, Person>(
        "WITH entries AS (
             SELECT notes.id AS note_id,
                    notes.updated_at AS updated_at,
                    lower(trim(json_extract(p.value, '$.email'))) AS email,
                    CASE WHEN json_type(p.value, '$.displayName') = 'text'
                         THEN nullif(trim(json_extract(p.value, '$.displayName')), '') END AS name
             FROM notes,
                  json_each(CASE WHEN json_valid(notes.participants) THEN CASE WHEN json_type(notes.participants) = 'array' THEN notes.participants END END) AS p
             WHERE p.type = 'object' AND json_type(p.value, '$.email') = 'text'
         ),
         named AS (
             SELECT email, note_id,
                    first_value(name) OVER (PARTITION BY email ORDER BY name IS NULL, updated_at DESC, note_id DESC) AS name
             FROM entries
             WHERE email <> ''
         )
         SELECT email, max(name) AS name, count(DISTINCT note_id) AS note_count
         FROM named
         GROUP BY email
         ORDER BY note_count DESC, lower(coalesce(max(name), email)), email",
    )
    .fetch_all(pool)
    .await?)
}

/// The notes `email` is on, newest first, as `notes::list` orders them.
/// Matched as the People list groups them: trimmed and ASCII lower-case, so
/// any spelling of a listed email finds the same notes. `limit` is clamped as
/// `notes::list` clamps it.
pub async fn list_by_person(
    pool: &SqlitePool,
    email: &str,
    limit: i64,
) -> Result<Vec<Note>, StorageError> {
    let limit = limit.clamp(1, 500);
    let sql = format!(
        "SELECT {COLUMNS} FROM notes
         WHERE EXISTS (
             SELECT 1
             FROM json_each(CASE WHEN json_valid(notes.participants) THEN CASE WHEN json_type(notes.participants) = 'array' THEN notes.participants END END) AS p
             WHERE p.type = 'object' AND json_type(p.value, '$.email') = 'text'
               AND lower(trim(json_extract(p.value, '$.email'))) = ?
         )
         ORDER BY updated_at DESC LIMIT ?"
    );
    Ok(sqlx::query_as::<_, Note>(&sql)
        .bind(email.trim().to_ascii_lowercase())
        .bind(limit)
        .fetch_all(pool)
        .await?)
}

/// The display names in a stored `participants`, for the summary prompt's
/// spelling hint: names only, never an address (KNEG-362). A name holding an
/// `@` is taken for an address -- imports put the email in the name field
/// when there was no name -- and is left out with it. Lenient as every reader
/// of `participants` is: anything that doesn't read is no names.
#[must_use]
pub fn display_names(participants: Option<&str>) -> Vec<String> {
    let Some(raw) = participants else {
        return Vec::new();
    };
    let Ok(entries) = serde_json::from_str::<Vec<serde_json::Value>>(raw) else {
        return Vec::new();
    };
    entries
        .iter()
        .filter_map(|entry| present(entry.get("displayName")?.as_str()))
        .filter(|name| !name.contains('@'))
        .map(str::to_owned)
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
    use crate::notes::{self, NewNote};
    use kw_storage::Db;

    async fn db() -> Db {
        Db::open_in_memory().await.unwrap()
    }

    fn person(name: Option<&str>, email: Option<&str>) -> Participant {
        Participant {
            display_name: name.map(str::to_owned),
            email: email.map(str::to_owned),
        }
    }

    /// A note with `participants` stored exactly as given, as an import
    /// writes it.
    async fn note_with(pool: &SqlitePool, title: &str, participants: Option<&str>) -> i64 {
        notes::create(
            pool,
            &NewNote {
                title: title.to_owned(),
                participants: participants.map(str::to_owned),
                ..Default::default()
            },
        )
        .await
        .unwrap()
    }

    async fn stored(pool: &SqlitePool, id: i64) -> Option<String> {
        notes::get(pool, id).await.unwrap().unwrap().participants
    }

    /// Notes are created within one second of each other in these tests, so
    /// which is newer is set rather than left to `CURRENT_TIMESTAMP`.
    async fn updated(pool: &SqlitePool, id: i64, at: &str) {
        sqlx::query("UPDATE notes SET updated_at = ? WHERE id = ?")
            .bind(at)
            .bind(id)
            .execute(pool)
            .await
            .unwrap();
    }

    /* ------------------------------------------------------------- emails */

    #[test]
    fn an_email_is_trimmed_and_lower_cased() {
        assert_eq!(
            normalise_email("  Anna.Lind@Example.COM ").as_deref(),
            Some("anna.lind@example.com")
        );
        assert_eq!(
            normalise_email("ravi+notes@example.com").as_deref(),
            Some("ravi+notes@example.com")
        );
        // Only ASCII is folded, as SQLite's lower() folds it: Å stays Å.
        assert_eq!(
            normalise_email("ÅSA@Example.se").as_deref(),
            Some("Åsa@example.se")
        );
    }

    #[test]
    fn what_is_not_an_email_is_refused() {
        for input in [
            "",
            "   ",
            "anna",
            "anna@",
            "@example.com",
            "anna@@example.com",
            "anna@ex@ample.com",
            "anna lind@example.com",
            "anna@example .com",
            "anna\t@example.com",
        ] {
            assert_eq!(normalise_email(input), None, "{input:?}");
        }
    }

    #[test]
    fn an_email_is_at_most_254_characters() {
        let at_limit = format!("{}@example.com", "a".repeat(254 - "@example.com".len()));
        assert_eq!(at_limit.chars().count(), 254);
        assert_eq!(
            normalise_email(&at_limit).as_deref(),
            Some(at_limit.as_str())
        );
        let over = format!("a{at_limit}");
        assert_eq!(normalise_email(&over), None);
    }

    /* ------------------------------------------------------------- saving */

    #[tokio::test]
    async fn people_are_saved_tidied_and_read_back() {
        let db = db().await;
        let p = db.pool();
        let id = note_with(p, "Kickoff", None).await;

        assert!(set_participants(
            p,
            id,
            &[
                person(Some("  Anna Lindqvist "), Some(" Anna@Example.com ")),
                person(None, Some("ravi@example.com")),
                person(Some("Jonas Berg"), None),
            ],
        )
        .await
        .unwrap());
        assert_eq!(
            stored(p, id).await.as_deref(),
            Some(
                r#"[{"displayName":"Anna Lindqvist","email":"anna@example.com"},{"email":"ravi@example.com"},{"displayName":"Jonas Berg"}]"#
            )
        );
    }

    #[tokio::test]
    async fn a_second_entry_for_an_email_is_dropped_whatever_its_case() {
        let db = db().await;
        let p = db.pool();
        let id = note_with(p, "Kickoff", None).await;
        set_participants(
            p,
            id,
            &[
                person(Some("Anna"), Some("anna@example.com")),
                person(Some("Anna Lindqvist"), Some("ANNA@example.com ")),
            ],
        )
        .await
        .unwrap();
        assert_eq!(
            stored(p, id).await.as_deref(),
            Some(r#"[{"displayName":"Anna","email":"anna@example.com"}]"#)
        );
    }

    #[tokio::test]
    async fn an_empty_list_stores_null() {
        let db = db().await;
        let p = db.pool();
        let id = note_with(p, "Kickoff", Some(r#"[{"email":"anna@example.com"}]"#)).await;
        assert!(set_participants(p, id, &[]).await.unwrap());
        assert_eq!(stored(p, id).await, None);
    }

    #[tokio::test]
    async fn a_refused_list_leaves_the_note_as_it_was() {
        let db = db().await;
        let p = db.pool();
        let before = r#"[{"email":"anna@example.com"}]"#;
        let id = note_with(p, "Kickoff", Some(before)).await;

        let refused = set_participants(p, id, &[person(Some("Anna"), Some("anna@"))])
            .await
            .unwrap_err();
        assert_eq!(refused.to_string(), "'anna@' isn't an email address.");

        let refused = set_participants(p, id, &[person(Some("  "), Some(" "))])
            .await
            .unwrap_err();
        assert_eq!(
            refused.to_string(),
            "Each person needs an email address or a name."
        );

        let crowd: Vec<Participant> = (0..=MAX_PARTICIPANTS)
            .map(|i| person(None, Some(&format!("p{i}@example.com"))))
            .collect();
        let refused = set_participants(p, id, &crowd).await.unwrap_err();
        assert_eq!(refused.to_string(), "A note can have at most 200 people.");

        assert_eq!(stored(p, id).await.as_deref(), Some(before));
    }

    #[tokio::test]
    async fn two_hundred_people_once_duplicates_are_dropped_are_kept() {
        let db = db().await;
        let p = db.pool();
        let id = note_with(p, "All hands", None).await;
        let mut crowd: Vec<Participant> = (0..MAX_PARTICIPANTS)
            .map(|i| person(None, Some(&format!("p{i}@example.com"))))
            .collect();
        crowd.push(person(None, Some("P0@example.com")));
        assert!(set_participants(p, id, &crowd).await.unwrap());
        assert_eq!(people(p).await.unwrap().len(), MAX_PARTICIPANTS);
    }

    #[tokio::test]
    async fn saving_people_is_not_editing_the_note() {
        let db = db().await;
        let p = db.pool();
        let id = note_with(p, "Kickoff", None).await;
        updated(p, id, "2026-01-01 09:00:00").await;
        set_participants(p, id, &[person(None, Some("anna@example.com"))])
            .await
            .unwrap();
        assert_eq!(
            notes::get(p, id)
                .await
                .unwrap()
                .unwrap()
                .updated_at
                .as_deref(),
            Some("2026-01-01 09:00:00")
        );
    }

    #[tokio::test]
    async fn a_missing_note_is_reported_not_created() {
        let db = db().await;
        assert!(
            !set_participants(db.pool(), 404, &[person(None, Some("anna@example.com"))])
                .await
                .unwrap()
        );
    }

    /* ------------------------------------------------------------- reading */

    #[tokio::test]
    async fn people_are_counted_by_email_most_notes_first() {
        let db = db().await;
        let p = db.pool();
        note_with(
            p,
            "Weekly",
            Some(r#"[{"displayName":"Anna Lindqvist","email":"anna@example.com"},{"displayName":"Ravi Menon","email":"ravi@example.com"}]"#),
        )
        .await;
        note_with(
            p,
            "Planning",
            Some(r#"[{"displayName":"Anna Lindqvist","email":" ANNA@example.com"}]"#),
        )
        .await;
        note_with(
            p,
            "Customer",
            Some(r#"[{"email":"jonas@nordbank.example"}]"#),
        )
        .await;

        assert_eq!(
            people(p).await.unwrap(),
            [
                Person {
                    email: "anna@example.com".into(),
                    name: Some("Anna Lindqvist".into()),
                    note_count: 2,
                },
                // A tie is ordered by the name shown: the email where there is no name.
                Person {
                    email: "jonas@nordbank.example".into(),
                    name: None,
                    note_count: 1,
                },
                Person {
                    email: "ravi@example.com".into(),
                    name: Some("Ravi Menon".into()),
                    note_count: 1,
                },
            ]
        );
    }

    #[tokio::test]
    async fn a_person_twice_on_one_note_counts_once() {
        let db = db().await;
        let p = db.pool();
        note_with(
            p,
            "Imported",
            Some(r#"[{"email":"anna@example.com"},{"email":"Anna@Example.com"}]"#),
        )
        .await;
        assert_eq!(people(p).await.unwrap()[0].note_count, 1);
    }

    #[tokio::test]
    async fn a_name_comes_from_the_newest_note_that_has_one() {
        let db = db().await;
        let p = db.pool();
        let old = note_with(
            p,
            "Old",
            Some(r#"[{"displayName":"Anna L","email":"anna@example.com"}]"#),
        )
        .await;
        let newer = note_with(
            p,
            "Newer",
            Some(r#"[{"displayName":"Anna Lindqvist","email":"anna@example.com"}]"#),
        )
        .await;
        // The newest of all gives no name, so the one before it is used.
        let newest = note_with(
            p,
            "Newest",
            Some(r#"[{"displayName":" ","email":"anna@example.com"}]"#),
        )
        .await;
        updated(p, old, "2026-01-01 09:00:00").await;
        updated(p, newer, "2026-02-01 09:00:00").await;
        updated(p, newest, "2026-03-01 09:00:00").await;

        let listed = people(p).await.unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].name.as_deref(), Some("Anna Lindqvist"));
        assert_eq!(listed[0].note_count, 3);

        updated(p, old, "2026-04-01 09:00:00").await;
        assert_eq!(people(p).await.unwrap()[0].name.as_deref(), Some("Anna L"));
    }

    #[tokio::test]
    async fn people_without_an_email_are_left_out() {
        // The owner's ruling: the email is the identity.
        let db = db().await;
        let p = db.pool();
        note_with(
            p,
            "Imported",
            Some(r#"[{"displayName":"Jonas Berg"},{"displayName":"Erik","email":""},{"displayName":"Lisa","email":null},{"email":"anna@example.com"}]"#),
        )
        .await;
        let listed = people(p).await.unwrap();
        assert_eq!(
            listed
                .iter()
                .map(|person| person.email.as_str())
                .collect::<Vec<_>>(),
            ["anna@example.com"]
        );
    }

    #[tokio::test]
    async fn participants_that_do_not_read_are_no_people_rather_than_an_error() {
        let db = db().await;
        let p = db.pool();
        for raw in [
            "not json",
            "{\"email\":\"loose@example.com\"}",
            "[\"Axel\"]",
            "[null, 3, [\"x\"]]",
            "[{\"email\": 42}]",
        ] {
            note_with(p, "Broken", Some(raw)).await;
        }
        note_with(p, "Fine", Some(r#"[{"email":"anna@example.com"}]"#)).await;

        assert_eq!(
            people(p).await.unwrap(),
            [Person {
                email: "anna@example.com".into(),
                name: None,
                note_count: 1,
            }]
        );
        assert_eq!(
            list_by_person(p, "anna@example.com", 10)
                .await
                .unwrap()
                .len(),
            1
        );
    }

    #[tokio::test]
    async fn a_persons_notes_are_found_whatever_the_case_newest_first() {
        let db = db().await;
        let p = db.pool();
        let older = note_with(p, "Older", Some(r#"[{"email":"Anna@Example.com"}]"#)).await;
        let newer = note_with(
            p,
            "Newer",
            Some(r#"[{"displayName":"Anna","email":" anna@example.com "}]"#),
        )
        .await;
        note_with(p, "Someone else", Some(r#"[{"email":"ravi@example.com"}]"#)).await;
        note_with(p, "Nobody", None).await;
        updated(p, older, "2026-01-01 09:00:00").await;
        updated(p, newer, "2026-02-01 09:00:00").await;

        for asked in [
            "anna@example.com",
            "ANNA@EXAMPLE.COM",
            "  Anna@example.com ",
        ] {
            let found: Vec<i64> = list_by_person(p, asked, 10)
                .await
                .unwrap()
                .into_iter()
                .map(|note| note.id)
                .collect();
            assert_eq!(found, [newer, older], "{asked:?}");
        }
        assert!(list_by_person(p, "nobody@example.com", 10)
            .await
            .unwrap()
            .is_empty());
        assert_eq!(
            list_by_person(p, "anna@example.com", 1)
                .await
                .unwrap()
                .len(),
            1
        );
    }

    /* ------------------------------------------------- the summary's hint */

    #[test]
    fn the_names_a_summary_is_given_are_names_and_never_addresses() {
        let names = display_names(Some(
            r#"[{"displayName":"Anna Lindqvist","email":"anna@example.com"},
                {"email":"jonas@nordbank.example"},
                {"displayName":"ravi@example.com","email":"ravi@example.com"},
                {"displayName":"  Åsa Berg "},
                {"displayName":"  "},
                "Axel"]"#,
        ));
        assert_eq!(names, ["Anna Lindqvist", "Åsa Berg"]);
        assert!(display_names(None).is_empty());
        assert!(display_names(Some("not json")).is_empty());
        assert!(display_names(Some(r#"{"displayName":"Anna"}"#)).is_empty());
    }
}
