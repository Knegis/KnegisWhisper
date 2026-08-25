//! The `knegiswhisper` command-line client.
//!
//! Upstream's CLI is a separate npm package that is not in the repository, so
//! this is new work rather than a port. What was available to port is its
//! *documented surface* — the noun-verb command shape and the exit codes — and
//! keeping those means a script written against the old tool has a chance of
//! working against this one.
//!
//! ## Exit codes
//!
//! A script branches on these, so they are part of the interface and each has a
//! test:
//!
//! | Code | Meaning |
//! | --- | --- |
//! | 0 | It worked |
//! | 1 | It failed for a reason the server explained |
//! | 2 | The command line was wrong |
//! | 3 | The thing asked for does not exist |
//! | 4 | KnegisWhisper is not running |
//!
//! 4 is separated from 1 deliberately. "Not running" is the most common failure
//! by far and the only one with an obvious fix, so a script that retries — or a
//! person reading the message — should not have to parse prose to tell it apart
//! from a real error.

use std::path::PathBuf;

use kw_local_api::discovery;
use kw_security::LocalApiToken;

pub mod render;

/// What the process should exit with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Exit {
    Ok = 0,
    Failed = 1,
    Usage = 2,
    NotFound = 3,
    NotRunning = 4,
}

impl Exit {
    #[must_use]
    pub const fn code(self) -> i32 {
        self as i32
    }
}

/// Why a command did not succeed.
#[derive(Debug, thiserror::Error)]
pub enum CliError {
    #[error(
        "KnegisWhisper does not appear to be running. Start it, and make sure the local API is \
         enabled in settings."
    )]
    NotRunning,

    #[error(
        "found {path} but could not use it: {detail}. If KnegisWhisper is running, restarting it \
         will rewrite this file."
    )]
    UnusableDiscovery { path: PathBuf, detail: String },

    #[error(
        "KnegisWhisper rejected the token in the discovery file. Restart it to get a new one."
    )]
    Unauthorized,

    #[error("{what} was not found")]
    NotFound { what: String },

    #[error("{0}")]
    Refused(String),

    #[error("could not reach KnegisWhisper: {detail}")]
    Transport { detail: String },

    #[error("{0}")]
    Usage(String),
}

impl CliError {
    /// The exit code for this failure.
    #[must_use]
    pub const fn exit(&self) -> Exit {
        match self {
            Self::NotRunning | Self::UnusableDiscovery { .. } => Exit::NotRunning,
            Self::NotFound { .. } => Exit::NotFound,
            Self::Usage(_) => Exit::Usage,
            Self::Unauthorized | Self::Refused(_) | Self::Transport { .. } => Exit::Failed,
        }
    }
}

/// A connection to a running KnegisWhisper.
#[derive(Debug, Clone)]
pub struct Client {
    base: String,
    token: LocalApiToken,
    http: reqwest::Client,
}

impl Client {
    /// Find the running application through its discovery file.
    ///
    /// A missing file and an unreadable one are told apart: the first means "not
    /// running", the second means "running, but something is wrong with the
    /// file", and the fix differs.
    pub fn connect(discovery_path: &std::path::Path) -> Result<Self, CliError> {
        if !discovery_path.exists() {
            return Err(CliError::NotRunning);
        }

        let (port, token) =
            discovery::read(discovery_path).map_err(|err| CliError::UnusableDiscovery {
                path: discovery_path.to_path_buf(),
                detail: err.to_string(),
            })?;

        Ok(Self::new(port, token))
    }

    /// Connect to a known port with a known token. Used by tests.
    #[must_use]
    pub fn new(port: u16, token: LocalApiToken) -> Self {
        Self {
            base: format!("http://127.0.0.1:{port}"),
            token,
            http: reqwest::Client::new(),
        }
    }

    /// Send a request and interpret the answer.
    pub async fn request(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<serde_json::Value>,
    ) -> Result<serde_json::Value, CliError> {
        let mut request = self
            .http
            .request(method, format!("{}{path}", self.base))
            .header("Authorization", self.token.header_value());

        if let Some(body) = body {
            request = request.json(&body);
        }

        let response = request.send().await.map_err(|err| {
            // A refused connection means the application stopped between
            // reading the file and sending, which is "not running" rather than
            // a transport fault the user can act on.
            if err.is_connect() {
                CliError::NotRunning
            } else {
                CliError::Transport {
                    detail: err.to_string(),
                }
            }
        })?;

        let status = response.status();
        let text = response.text().await.unwrap_or_default();

        if status.is_success() {
            if text.is_empty() {
                // 204, which the delete routes answer.
                return Ok(serde_json::Value::Null);
            }
            return serde_json::from_str(&text).map_err(|err| CliError::Transport {
                detail: format!("could not read the reply: {err}"),
            });
        }

        Err(interpret_failure(status.as_u16(), &text))
    }

    pub async fn get(&self, path: &str) -> Result<serde_json::Value, CliError> {
        self.request(reqwest::Method::GET, path, None).await
    }
}

/// Turn a failed response into the right error.
///
/// Pure, so every status and body shape is covered without a server. The
/// server's `code` is preferred over its status, because that is the field it
/// promises to keep stable.
#[must_use]
pub fn interpret_failure(status: u16, body: &str) -> CliError {
    #[derive(serde::Deserialize)]
    struct Envelope {
        error: Detail,
    }
    #[derive(serde::Deserialize)]
    struct Detail {
        code: String,
        message: String,
    }

    match serde_json::from_str::<Envelope>(body) {
        Ok(envelope) => match envelope.error.code.as_str() {
            "not_found" => CliError::NotFound {
                what: envelope.error.message,
            },
            "unauthorized" => CliError::Unauthorized,
            _ => CliError::Refused(envelope.error.message),
        },
        // A body we cannot parse still has a status worth reporting, and a bare
        // status is better than "could not read the reply".
        Err(_) => match status {
            401 => CliError::Unauthorized,
            404 => CliError::NotFound {
                what: "that".to_owned(),
            },
            _ => CliError::Refused(format!("KnegisWhisper answered {status}")),
        },
    }
}

/// The default discovery path, or a usage error explaining there is no home
/// directory to look in.
pub fn default_discovery_path() -> Result<PathBuf, CliError> {
    discovery::default_path().map_err(|err| CliError::Usage(err.to_string()))
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

    #[test]
    fn each_failure_maps_to_its_documented_exit_code() {
        // A script branches on these, so they are interface.
        assert_eq!(CliError::NotRunning.exit(), Exit::NotRunning);
        assert_eq!(
            CliError::UnusableDiscovery {
                path: PathBuf::from("x"),
                detail: "bad".to_owned()
            }
            .exit(),
            Exit::NotRunning
        );
        assert_eq!(
            CliError::NotFound {
                what: "note".to_owned()
            }
            .exit(),
            Exit::NotFound
        );
        assert_eq!(CliError::Usage("bad flag".to_owned()).exit(), Exit::Usage);
        assert_eq!(CliError::Unauthorized.exit(), Exit::Failed);
        assert_eq!(CliError::Refused("no".to_owned()).exit(), Exit::Failed);
    }

    #[test]
    fn the_exit_codes_are_the_documented_numbers() {
        assert_eq!(Exit::Ok.code(), 0);
        assert_eq!(Exit::Failed.code(), 1);
        assert_eq!(Exit::Usage.code(), 2);
        assert_eq!(Exit::NotFound.code(), 3);
        assert_eq!(Exit::NotRunning.code(), 4);
    }

    #[test]
    fn not_running_is_told_apart_from_a_broken_discovery_file() {
        // Same exit code, different message, because the fix differs: start the
        // application, versus restart it to rewrite the file.
        assert!(CliError::NotRunning
            .to_string()
            .contains("does not appear to be running"));
        assert!(CliError::UnusableDiscovery {
            path: PathBuf::from("~/.knegiswhisper/local-api.json"),
            detail: "not hex".to_owned(),
        }
        .to_string()
        .contains("restarting it will rewrite"));
    }

    #[test]
    fn the_servers_code_is_preferred_over_its_status() {
        // `code` is the field the server promises to keep stable.
        let body = r#"{"error":{"code":"not_found","message":"no note with that id"}}"#;
        match interpret_failure(404, body) {
            CliError::NotFound { what } => assert!(what.contains("no note")),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_validation_failure_reports_the_servers_message() {
        let body = r#"{"error":{"code":"validation_error","message":"'q' is required"}}"#;
        match interpret_failure(400, body) {
            CliError::Refused(message) => assert_eq!(message, "'q' is required"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn an_unauthorised_answer_says_to_restart() {
        let body =
            r#"{"error":{"code":"unauthorized","message":"a valid bearer token is required"}}"#;
        let err = interpret_failure(401, body);
        assert!(matches!(err, CliError::Unauthorized));
        assert!(err.to_string().contains("Restart"));
    }

    #[test]
    fn an_unparseable_body_still_reports_the_status() {
        // A proxy, or a future version answering something unexpected. A bare
        // status is more useful than "could not read the reply".
        match interpret_failure(503, "<html>Service Unavailable</html>") {
            CliError::Refused(message) => assert!(message.contains("503")),
            other => panic!("{other:?}"),
        }
        assert!(matches!(
            interpret_failure(401, "not json"),
            CliError::Unauthorized
        ));
        assert!(matches!(
            interpret_failure(404, ""),
            CliError::NotFound { .. }
        ));
    }

    #[test]
    fn connecting_with_no_discovery_file_reports_not_running() {
        let dir = tempfile::tempdir().unwrap();
        let err = Client::connect(&dir.path().join("local-api.json")).unwrap_err();
        assert_eq!(err.exit(), Exit::NotRunning);
        assert!(err.to_string().contains("Start it"));
    }

    #[test]
    fn connecting_with_a_corrupt_discovery_file_says_which_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("local-api.json");
        std::fs::write(&path, "{ not json").unwrap();

        let err = Client::connect(&path).unwrap_err();
        assert_eq!(err.exit(), Exit::NotRunning);
        assert!(err.to_string().contains("local-api.json"));
    }
}
