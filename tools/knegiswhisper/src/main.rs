//! `knegiswhisper` — the command-line client.
//!
//! Kept thin on purpose. Parsing lives in `clap`, the request and its
//! interpretation live in [`kw_cli::Client`], and the formatting lives in
//! [`kw_cli::render`] — all of which are tested. What is left here is the
//! dispatch table and the exit code, so there is very little to get wrong in a
//! place that has no test.

use clap::{Parser, Subcommand};
use kw_cli::{render, CliError, Client, Exit};
use reqwest::Method;
use serde_json::{json, Value};

#[derive(Debug, Parser)]
#[command(
    name = "knegiswhisper",
    about = "Talk to a running KnegisWhisper",
    long_about = "Reads ~/.knegiswhisper/local-api.json to find the running application, then \
                  talks to it over loopback. Exit codes: 0 success, 1 failed, 2 usage, 3 not \
                  found, 4 not running.",
    version
)]
struct Cli {
    /// Print the server's answer as JSON instead of a table.
    #[arg(long, global = true)]
    json: bool,

    /// Read the port and token from somewhere other than the default.
    #[arg(long, global = true, value_name = "PATH")]
    discovery: Option<std::path::PathBuf>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Check that KnegisWhisper is running.
    Health,

    /// Notes.
    #[command(subcommand)]
    Notes(Notes),

    /// Note folders.
    #[command(subcommand)]
    Folders(Folders),

    /// The custom dictionary that biases recognition.
    #[command(subcommand)]
    Dictionary(Dictionary),

    /// Dictation history.
    #[command(subcommand)]
    History(History),
}

#[derive(Debug, Subcommand)]
enum Notes {
    /// List notes, most recently edited first.
    List {
        #[arg(long, default_value_t = 20)]
        limit: u32,
        #[arg(long, value_name = "ID")]
        folder: Option<i64>,
        #[arg(long = "type", value_name = "TYPE")]
        note_type: Option<String>,
    },
    /// Show one note in full.
    Get { id: i64 },
    /// Full-text search.
    Search {
        query: Vec<String>,
        #[arg(long, default_value_t = 20)]
        limit: u32,
    },
    /// Change a note's title, content, or folder.
    Set {
        id: i64,
        #[arg(long)]
        title: Option<String>,
        #[arg(long)]
        content: Option<String>,
        #[arg(long, value_name = "ID")]
        folder: Option<i64>,
    },
    /// Delete a note.
    Delete { id: i64 },
}

#[derive(Debug, Subcommand)]
enum Folders {
    /// List folders.
    List,
}

#[derive(Debug, Subcommand)]
enum Dictionary {
    /// List dictionary words.
    List,
    /// Add words.
    Add { words: Vec<String> },
    /// Remove words.
    Remove { words: Vec<String> },
}

#[derive(Debug, Subcommand)]
enum History {
    /// List dictations, newest first.
    List {
        #[arg(long, default_value_t = 20)]
        limit: u32,
        /// Continue after this id, from a previous page.
        #[arg(long, value_name = "ID")]
        after: Option<i64>,
    },
    /// Show one dictation.
    Get { id: i64 },
    /// Delete a dictation and its transcript.
    Delete { id: i64 },
    /// Delete only the kept recording, leaving the transcript.
    ForgetAudio { id: i64 },
}

fn main() -> std::process::ExitCode {
    let cli = Cli::parse();

    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(err) => {
            eprintln!("knegiswhisper: could not start: {err}");
            return std::process::ExitCode::from(Exit::Failed.code() as u8);
        }
    };

    match runtime.block_on(run(&cli)) {
        Ok(output) => {
            if !output.is_empty() {
                println!("{output}");
            }
            std::process::ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("knegiswhisper: {err}");
            std::process::ExitCode::from(err.exit().code() as u8)
        }
    }
}

async fn run(cli: &Cli) -> Result<String, CliError> {
    let path = match &cli.discovery {
        Some(path) => path.clone(),
        None => kw_cli::default_discovery_path()?,
    };
    let client = Client::connect(&path)?;

    let (value, human) = dispatch(&client, &cli.command).await?;

    Ok(if cli.json {
        render::as_json(&value)
    } else {
        human
    })
}

/// Send the request for one command and render it both ways.
///
/// Both forms are produced regardless of `--json`, because the human form is
/// cheap and doing it here keeps the branch in one place.
async fn dispatch(client: &Client, command: &Command) -> Result<(Value, String), CliError> {
    match command {
        Command::Health => {
            let value = client.get("/v1/health").await?;
            let human = render::health(&value);
            Ok((value, human))
        }

        Command::Notes(Notes::List {
            limit,
            folder,
            note_type,
        }) => {
            let mut query = format!("/v1/notes/list?limit={limit}");
            if let Some(folder) = folder {
                query.push_str(&format!("&folder_id={folder}"));
            }
            if let Some(note_type) = note_type {
                query.push_str(&format!("&note_type={}", urlencode(note_type)));
            }
            let value = client.get(&query).await?;
            let human = render::notes(&value);
            Ok((value, human))
        }

        Command::Notes(Notes::Get { id }) => {
            let value = client.get(&format!("/v1/notes/{id}")).await?;
            let human = render::note(&value);
            Ok((value, human))
        }

        Command::Notes(Notes::Search { query, limit }) => {
            if query.is_empty() {
                return Err(CliError::Usage(
                    "give something to search for: knegiswhisper notes search <words>".to_owned(),
                ));
            }
            let joined = query.join(" ");
            let value = client
                .get(&format!(
                    "/v1/notes/search?q={}&limit={limit}",
                    urlencode(&joined)
                ))
                .await?;
            let human = render::search_hits(&value);
            Ok((value, human))
        }

        Command::Notes(Notes::Set {
            id,
            title,
            content,
            folder,
        }) => {
            let mut body = serde_json::Map::new();
            if let Some(title) = title {
                body.insert("title".to_owned(), json!(title));
            }
            if let Some(content) = content {
                body.insert("content".to_owned(), json!(content));
            }
            if let Some(folder) = folder {
                body.insert("folder_id".to_owned(), json!(folder));
            }
            if body.is_empty() {
                // Caught here rather than by the server, so the message can name
                // the flags rather than the JSON fields.
                return Err(CliError::Usage(
                    "nothing to change: pass --title, --content, or --folder".to_owned(),
                ));
            }

            let value = client
                .request(Method::PATCH, &format!("/v1/notes/{id}"), Some(body.into()))
                .await?;
            let human = render::note(&value);
            Ok((value, human))
        }

        Command::Notes(Notes::Delete { id }) => {
            client
                .request(Method::DELETE, &format!("/v1/notes/{id}"), None)
                .await?;
            Ok((Value::Null, format!("deleted note {id}")))
        }

        Command::Folders(Folders::List) => {
            let value = client.get("/v1/folders/list").await?;
            let human = render::folders(&value);
            Ok((value, human))
        }

        Command::Dictionary(Dictionary::List) => {
            let value = client.get("/v1/dictionary/list").await?;
            let human = render::dictionary(&value);
            Ok((value, human))
        }

        Command::Dictionary(Dictionary::Add { words }) => {
            dictionary_update(client, words, "add").await
        }

        Command::Dictionary(Dictionary::Remove { words }) => {
            dictionary_update(client, words, "remove").await
        }

        Command::History(History::List { limit, after }) => {
            let mut query = format!("/v1/transcriptions/list?limit={limit}");
            if let Some(after) = after {
                query.push_str(&format!("&after={after}"));
            }
            let value = client.get(&query).await?;
            let human = render::history(&value);
            Ok((value, human))
        }

        Command::History(History::Get { id }) => {
            let value = client.get(&format!("/v1/transcriptions/{id}")).await?;
            let human = value
                .get("data")
                .and_then(|d| d.get("text"))
                .and_then(Value::as_str)
                .unwrap_or("no text in the reply")
                .to_owned();
            Ok((value, human))
        }

        Command::History(History::Delete { id }) => {
            client
                .request(Method::DELETE, &format!("/v1/transcriptions/{id}"), None)
                .await?;
            Ok((Value::Null, format!("deleted dictation {id}")))
        }

        Command::History(History::ForgetAudio { id }) => {
            client
                .request(
                    Method::DELETE,
                    &format!("/v1/transcriptions/{id}/audio"),
                    None,
                )
                .await?;
            Ok((
                Value::Null,
                format!("deleted the recording for dictation {id}; the transcript is still there"),
            ))
        }
    }
}

async fn dictionary_update(
    client: &Client,
    words: &[String],
    field: &str,
) -> Result<(Value, String), CliError> {
    if words.is_empty() {
        return Err(CliError::Usage(format!(
            "give at least one word: knegiswhisper dictionary {field} <words>"
        )));
    }

    let value = client
        .request(
            Method::POST,
            "/v1/dictionary/update",
            Some(json!({ field: words })),
        )
        .await?;
    let human = render::dictionary_change(&value);
    Ok((value, human))
}

/// Percent-encode a query value.
///
/// Hand-rolled rather than pulled in as a dependency, because the alphabet that
/// needs escaping in a query value is short and fixed. Encoding everything
/// outside the unreserved set is conservative and cannot under-escape.
fn urlencode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(char::from(*byte));
            }
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
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
    use clap::CommandFactory as _;

    #[test]
    fn the_command_tree_is_well_formed() {
        // clap's own check for conflicting flags, duplicate names, and bad
        // defaults. Cheap, and it fails at test time rather than at run time.
        Cli::command().debug_assert();
    }

    #[test]
    fn the_documented_commands_parse() {
        for argv in [
            vec!["knegiswhisper", "health"],
            vec!["knegiswhisper", "notes", "list"],
            vec!["knegiswhisper", "notes", "list", "--limit", "5"],
            vec!["knegiswhisper", "notes", "get", "1"],
            vec!["knegiswhisper", "notes", "search", "warehouse", "plan"],
            vec!["knegiswhisper", "notes", "set", "1", "--title", "x"],
            vec!["knegiswhisper", "notes", "delete", "1"],
            vec!["knegiswhisper", "folders", "list"],
            vec!["knegiswhisper", "dictionary", "list"],
            vec!["knegiswhisper", "dictionary", "add", "Knegis"],
            vec!["knegiswhisper", "dictionary", "remove", "Knegis"],
            vec!["knegiswhisper", "history", "list", "--after", "9"],
            vec!["knegiswhisper", "history", "get", "1"],
            vec!["knegiswhisper", "history", "delete", "1"],
            vec!["knegiswhisper", "history", "forget-audio", "1"],
            vec!["knegiswhisper", "--json", "health"],
        ] {
            assert!(Cli::try_parse_from(&argv).is_ok(), "{argv:?} should parse");
        }
    }

    #[test]
    fn an_unknown_command_or_flag_is_a_parse_error() {
        // clap exits 2 for these, which is the documented usage code.
        for argv in [
            vec!["knegiswhisper", "notez", "list"],
            vec!["knegiswhisper", "notes", "list", "--limitt", "5"],
            vec!["knegiswhisper", "notes", "get", "abc"],
            vec!["knegiswhisper"],
        ] {
            assert!(Cli::try_parse_from(&argv).is_err(), "{argv:?}");
        }
    }

    #[test]
    fn search_words_are_collected_rather_than_needing_quotes() {
        // `notes search warehouse plan` should search for both words, which is
        // what someone types without thinking about quoting.
        let cli =
            Cli::try_parse_from(["knegiswhisper", "notes", "search", "warehouse", "plan"]).unwrap();
        match cli.command {
            Command::Notes(Notes::Search { query, .. }) => {
                assert_eq!(query, ["warehouse", "plan"]);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn the_json_flag_works_after_the_subcommand_too() {
        // Global, because `knegiswhisper notes list --json` is what people type.
        let cli = Cli::try_parse_from(["knegiswhisper", "notes", "list", "--json"]).unwrap();
        assert!(cli.json);
    }

    #[test]
    fn query_values_are_percent_encoded() {
        assert_eq!(urlencode("warehouse plan"), "warehouse%20plan");
        assert_eq!(urlencode("C++ (draft)"), "C%2B%2B%20%28draft%29");
        assert_eq!(urlencode("a&b=c"), "a%26b%3Dc");
        assert_eq!(urlencode("plain-word_1.0~"), "plain-word_1.0~");
    }

    #[test]
    fn encoding_cannot_be_used_to_add_a_query_parameter() {
        // The failure this protects against: a search for "x&limit=1" changing
        // the limit.
        let encoded = urlencode("x&limit=1");
        assert!(!encoded.contains('&'));
        assert!(!encoded.contains('='));
    }

    #[test]
    fn non_ascii_search_terms_are_encoded_as_utf8_bytes() {
        // Swedish is a first-class case for this application.
        assert_eq!(urlencode("Håkan"), "H%C3%A5kan");
    }
}
