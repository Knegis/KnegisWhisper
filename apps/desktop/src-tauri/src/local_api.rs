//! Starting and stopping the local HTTP API.
//!
//! **Off by default.** It is an unauthenticated-by-transport surface exposing
//! every note and transcript to anything running as this user, so it does not
//! listen unless someone has asked it to. The setting is read at startup and
//! nowhere else decides.
//!
//! The one thing this module is careful about beyond starting a server is
//! telling the truth about the discovery file. `kw_local_api::discovery::write`
//! reads the permissions back rather than assuming them, and whatever it reports
//! is passed through to the settings screen word for word — including the
//! Windows case, where there is no `chmod` and the honest answer is "your user
//! account and administrators".

use std::sync::Arc;

use kw_local_api::{discovery, serve, ApiState, Permissions, Served};
use kw_security::LocalApiToken;
use kw_storage::Db;
use serde::Serialize;
use tauri::State;
use ts_rs::TS;

/// The settings key that switches the API on.
pub const ENABLED_KEY: &str = "localApi.enabled";

/// Whether the API listens unless told otherwise. It does not.
pub const ENABLED_BY_DEFAULT: bool = false;

/// How many times to re-allocate when something takes the port between the
/// probe and the bind. Small on purpose: more than a couple of consecutive
/// losses means the range is genuinely contended, and "every port is in use" is
/// the honest report rather than a longer spin.
const BIND_ATTEMPTS: u32 = 5;

/// The running server, if there is one.
pub struct LocalApiRuntime {
    db: Db,
    served: tokio::sync::Mutex<Option<Served>>,
    /// The permissions the discovery file actually ended up with.
    permissions: std::sync::RwLock<Option<Permissions>>,
    /// Where the discovery file goes.
    ///
    /// A field rather than a call to `discovery::default_path` at each use, so
    /// tests can point it at a temporary directory. Without that they would
    /// write to the developer's real `~/.knegiswhisper/local-api.json` and could
    /// clobber the file of an application actually running on this machine.
    discovery_path: std::path::PathBuf,
}

impl std::fmt::Debug for LocalApiRuntime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LocalApiRuntime").finish_non_exhaustive()
    }
}

impl LocalApiRuntime {
    /// Use the real discovery path under the user's home directory.
    #[must_use]
    pub fn new(db: Db) -> Self {
        let discovery_path = discovery::default_path().unwrap_or_else(|err| {
            tracing::warn!(error = %err, "no home directory; the local API cannot be discovered");
            std::path::PathBuf::from(discovery::FILE_NAME)
        });
        Self::with_discovery_path(db, discovery_path)
    }

    /// Use a specific discovery path.
    #[must_use]
    pub fn with_discovery_path(db: Db, discovery_path: std::path::PathBuf) -> Self {
        Self {
            db,
            served: tokio::sync::Mutex::new(None),
            permissions: std::sync::RwLock::new(None),
            discovery_path,
        }
    }

    /// Start it if the user has switched it on.
    ///
    /// Called once at startup. Silent when the setting is off, which is the
    /// default — a log line every launch saying a disabled feature is disabled
    /// is noise.
    pub async fn start_if_enabled(&self) {
        let enabled = kw_storage::settings::get_or(self.db.pool(), ENABLED_KEY, ENABLED_BY_DEFAULT)
            .await
            .unwrap_or(ENABLED_BY_DEFAULT);

        if !enabled {
            return;
        }

        if let Err(err) = self.start().await {
            // Not fatal. The application works; the API does not, and the
            // settings screen will say why.
            tracing::error!(error = %err, "could not start the local API");
        }
    }

    /// Start the server and write the discovery file.
    pub async fn start(&self) -> Result<u16, String> {
        let mut held = self.served.lock().await;
        if let Some(existing) = held.as_ref() {
            return Ok(existing.port());
        }

        let token = LocalApiToken::generate().map_err(|err| err.to_string())?;

        // The documented range, so the surface is predictable and a firewall
        // rule can name it. A client finds the port in the discovery file, not
        // by scanning.
        let (first, last) = kw_local_api::PORT_RANGE;
        let range = kw_sidecar::PortRange { first, last };

        // Allocating a port and then binding it is inherently racy: the probe
        // binds and drops, and anything on the machine may take the port in
        // between — a second copy of the application, or another test in this
        // same process. `port.rs` documents the mitigation as "the caller
        // allocates again"; this is the caller doing it. Without the retry the
        // race surfaced as a raw `os error 10048` from Start, and it was the
        // test suite that noticed, failing a different test each run.
        let mut attempt = 0;
        let (port, listener) = loop {
            attempt += 1;
            let port = kw_sidecar::port::allocate(range, &kw_sidecar::BindProbe)
                .map_err(|err| err.to_string())?;

            match tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port)).await {
                Ok(listener) => break (port, listener),
                Err(err) if attempt < BIND_ATTEMPTS => {
                    tracing::debug!(port, attempt, error = %err, "lost the race for a port, retrying");
                }
                Err(err) => {
                    return Err(format!(
                        "could not listen on 127.0.0.1:{port} after {attempt} attempts: {err}"
                    ))
                }
            }
        };

        let served = serve(
            listener,
            ApiState::new(self.db.pool().clone(), token.clone()),
        )
        .map_err(|err| err.to_string())?;

        let permissions =
            discovery::write(&self.discovery_path, port, &token).map_err(|err| err.to_string())?;

        if !permissions.is_as_expected() {
            // Worth a warning: the token is the whole access control, so a file
            // more readable than intended is a real weakening rather than a
            // cosmetic detail.
            tracing::warn!(
                permissions = %permissions.explain(),
                "the local API discovery file is not as restricted as intended"
            );
        }

        if let Ok(mut held) = self.permissions.write() {
            *held = Some(permissions);
        }
        *held = Some(served);

        tracing::info!(port, "the local API is listening on loopback");
        Ok(port)
    }

    /// Stop the server and remove the discovery file.
    ///
    /// The file goes with the server: a leftover sends the next client at a port
    /// nothing is listening on, with a token nothing accepts.
    pub async fn stop(&self) {
        let mut held = self.served.lock().await;
        if let Some(served) = held.take() {
            served.shutdown().await;
        }

        if let Err(err) = discovery::remove(&self.discovery_path) {
            tracing::warn!(error = %err, "could not remove the discovery file");
        }

        if let Ok(mut permissions) = self.permissions.write() {
            *permissions = None;
        }
    }
}

/// What the settings screen shows about the local API.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct LocalApiStatus {
    /// Whether the user has switched it on.
    pub enabled: bool,
    /// Whether it is listening right now.
    pub running: bool,
    pub port: Option<u16>,
    /// Where a client looks for the port and token.
    pub discovery_path: Option<String>,
    /// What the discovery file's permissions actually are, in plain words.
    /// `None` when it is not running, because there is no file to describe.
    pub discovery_permissions: Option<String>,
    /// True when the permissions are the best this platform offers.
    pub discovery_permissions_as_expected: Option<bool>,
}

/// Report the local API's state.
#[tauri::command]
pub async fn local_api_status(
    api: State<'_, Arc<LocalApiRuntime>>,
) -> Result<LocalApiStatus, String> {
    let enabled = kw_storage::settings::get_or(api.db.pool(), ENABLED_KEY, ENABLED_BY_DEFAULT)
        .await
        .unwrap_or(ENABLED_BY_DEFAULT);

    let port = api.served.lock().await.as_ref().map(Served::port);
    let permissions = api.permissions.read().ok().and_then(|held| held.clone());

    Ok(LocalApiStatus {
        enabled,
        running: port.is_some(),
        port,
        discovery_path: port.map(|_| api.discovery_path.display().to_string()),
        discovery_permissions: permissions.as_ref().map(Permissions::explain),
        discovery_permissions_as_expected: permissions.as_ref().map(Permissions::is_as_expected),
    })
}

/// Switch the local API on and start it.
#[tauri::command]
pub async fn local_api_start(api: State<'_, Arc<LocalApiRuntime>>) -> Result<u16, String> {
    // The setting is written first, so a restart brings it back up. Starting it
    // without recording the choice would make it forget every launch.
    kw_storage::settings::set(api.db.pool(), ENABLED_KEY, &true)
        .await
        .map_err(|err| err.to_string())?;
    api.start().await
}

/// Switch the local API off and stop it.
#[tauri::command]
pub async fn local_api_stop(api: State<'_, Arc<LocalApiRuntime>>) -> Result<(), String> {
    kw_storage::settings::set(api.db.pool(), ENABLED_KEY, &false)
        .await
        .map_err(|err| err.to_string())?;
    api.stop().await;
    Ok(())
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

    /// A runtime whose discovery file lives in a temporary directory.
    ///
    /// The `TempDir` is returned so it outlives the test: dropping it deletes
    /// the directory, and a test that let it drop early would be checking for a
    /// file the harness had already removed.
    async fn runtime() -> (LocalApiRuntime, Db, tempfile::TempDir) {
        let db = Db::open_in_memory().await.unwrap();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("local-api.json");
        (
            LocalApiRuntime::with_discovery_path(db.clone(), path),
            db,
            dir,
        )
    }

    #[tokio::test]
    async fn it_does_not_listen_unless_switched_on() {
        // The decision that matters most in this file. A surface exposing every
        // note to any local process must not be on because nobody said no.
        let (api, _db, _dir) = runtime().await;
        api.start_if_enabled().await;
        assert!(api.served.lock().await.is_none());
    }

    #[tokio::test]
    async fn the_default_is_off() {
        // Asserted through the read path rather than against the constant: the
        // question is what a fresh database answers, and `assert!(!CONST)` is
        // compiled away.
        let (api, db, _dir) = runtime().await;
        let enabled = kw_storage::settings::get_or(db.pool(), ENABLED_KEY, ENABLED_BY_DEFAULT)
            .await
            .unwrap();
        assert!(!enabled, "a fresh database must not have it on");

        api.stop().await;
    }

    #[tokio::test]
    async fn switching_it_on_is_remembered_so_it_survives_a_restart() {
        let (api, db, _dir) = runtime().await;
        kw_storage::settings::set(db.pool(), ENABLED_KEY, &true)
            .await
            .unwrap();

        api.start_if_enabled().await;
        let running = api.served.lock().await.is_some();
        assert!(running, "it should come up when the setting says so");

        api.stop().await;
        assert!(api.served.lock().await.is_none());
    }

    #[tokio::test]
    async fn starting_twice_returns_the_same_port_rather_than_two_servers() {
        let (api, _db, _dir) = runtime().await;
        let first = api.start().await.unwrap();
        let second = api.start().await.unwrap();
        assert_eq!(first, second);
        api.stop().await;
    }

    #[tokio::test]
    async fn the_port_is_inside_the_documented_range() {
        // So a firewall rule can name it and a listener outside it is
        // recognisably not ours.
        let (api, _db, _dir) = runtime().await;
        let port = api.start().await.unwrap();
        let (first, last) = kw_local_api::PORT_RANGE;
        assert!((first..=last).contains(&port), "got {port}");
        api.stop().await;
    }

    #[tokio::test]
    async fn stopping_removes_the_discovery_file() {
        // A leftover file sends the next client at a dead port with a dead token.
        let (api, _db, _dir) = runtime().await;
        api.start().await.unwrap();

        let path = api.discovery_path.clone();
        assert!(path.exists(), "the file should exist while it is running");

        api.stop().await;
        assert!(!path.exists(), "and go when it stops");
    }

    #[tokio::test]
    async fn the_reported_permissions_are_the_ones_the_platform_gave() {
        // Never a claim: on Windows this must say administrators can read it.
        let (api, _db, _dir) = runtime().await;
        api.start().await.unwrap();

        let permissions = api.permissions.read().unwrap().clone().unwrap();
        let explanation = permissions.explain();
        if cfg!(unix) {
            assert!(explanation.contains("verified"), "{explanation}");
        } else {
            assert!(explanation.contains("administrators"), "{explanation}");
            assert!(!explanation.contains("only by your user"));
        }

        api.stop().await;
    }

    #[tokio::test]
    async fn stopping_when_nothing_runs_is_harmless() {
        let (api, _db, _dir) = runtime().await;
        api.stop().await;
        api.stop().await;
    }
}
