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

        // Deliberately not `!permissions.is_as_expected()`: that helper also
        // accepts `InheritedFromProfile`, which is merely this platform's
        // profile-directory default and was never verified as a restriction.
        // A log line that stays silent on it is the same defect this file's
        // user-facing report exists to avoid -- see `DiscoveryPermissions`'s
        // doc comment and docs/BENCH_CHECK.md, 2026-08-26 finding 2.
        if !DiscoveryPermissions::from(&permissions).is_confirmed() {
            // Worth a warning: the token is the whole access control, so a file
            // that is not a verified restriction is a real weakening rather
            // than a cosmetic detail.
            tracing::warn!(
                permissions = %permissions.explain(),
                "the local API discovery file's permissions are not a verified restriction"
            );
        }

        if let Ok(mut held) = self.permissions.write() {
            *held = Some(permissions);
        }
        *held = Some(served);

        tracing::info!(port, "the local API is listening on loopback");
        Ok(port)
    }

    /// Where the discovery file lives. Deterministic and independent of
    /// whether the API is running right now, so a screen can show it either
    /// way — see `integrations.rs`.
    pub(crate) fn discovery_path(&self) -> &std::path::Path {
        &self.discovery_path
    }

    /// The discovery file's permissions as of the last successful write.
    /// `None` once the API has stopped: the file is removed with it, so
    /// there is nothing left to describe.
    pub(crate) fn permissions_snapshot(&self) -> Option<Permissions> {
        self.permissions.read().ok().and_then(|held| held.clone())
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

/// The discovery file's permissions, shown as a state and its evidence — not
/// reduced to a boolean.
///
/// Mirrors `kw_local_api::Permissions` variant for variant, converted here
/// rather than derived there: `kw-local-api` has no reason to depend on
/// `ts-rs`, the same reasoning `diagnostics.rs`'s `ShortcutHookStatus` and
/// `local_model.rs`'s `VerifiedBy` already follow for their own core types.
///
/// This type exists because of one specific defect.
/// `Permissions::is_as_expected()` returns `true` for `InheritedFromProfile`
/// — correct as "the best this build currently achieves on this platform",
/// read by a screen that only has the boolean as "verified restricted", which
/// it is not: nothing was checked, it is simply what a file under
/// `%USERPROFILE%` gets by default, and two other trustees turned up in that
/// default on a real machine. See `docs/BENCH_CHECK.md`, 2026-08-26 finding 2.
/// `Integrations > CLI` switches on `state` and gives `InheritedFromProfile`
/// its own honest sentence rather than a green tick that reuses the boolean.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(
    tag = "state",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum DiscoveryPermissions {
    /// Verified by reading the file's mode back: owner read/write only.
    OwnerOnly,
    /// Windows. Inherits the profile directory's ACL. **Not** a verified
    /// restriction — nothing here checked what that ACL actually grants,
    /// only that no narrower one was applied.
    InheritedFromProfile,
    /// The mode was set and read back wider than intended.
    Wider {
        /// Octal, e.g. `"644"`, matching how a person would type it to `chmod`.
        mode: String,
    },
    /// The mode could not be set or could not be read back.
    Unknown { reason: String },
}

impl From<&Permissions> for DiscoveryPermissions {
    fn from(permissions: &Permissions) -> Self {
        match permissions {
            Permissions::OwnerOnly => Self::OwnerOnly,
            Permissions::InheritedFromProfile => Self::InheritedFromProfile,
            Permissions::Wider { mode } => Self::Wider {
                mode: format!("{mode:o}"),
            },
            Permissions::Unknown { reason } => Self::Unknown {
                reason: reason.clone(),
            },
        }
    }
}

impl DiscoveryPermissions {
    /// Whether this is a *verified* restriction, not merely "the best this
    /// platform's default gives you". Only [`Self::OwnerOnly`] qualifies:
    /// [`Self::InheritedFromProfile`] is deliberately excluded even though
    /// `kw_local_api::Permissions::is_as_expected()` accepts it, because
    /// nothing here confirmed what that inherited ACL actually grants. Both
    /// the user-facing report (`local_api_status`) and the startup warning
    /// (`LocalApiRuntime::start`) go through this one decision, so there is
    /// exactly one place that draws the line rather than two that could
    /// drift apart. See docs/BENCH_CHECK.md, 2026-08-26 finding 2.
    #[must_use]
    pub const fn is_confirmed(&self) -> bool {
        matches!(self, Self::OwnerOnly)
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
    /// The discovery file's real permission state, evidence and all -- the
    /// same [`DiscoveryPermissions`] `Integrations > CLI` renders, so the two
    /// screens agree on one vocabulary rather than each describing this file
    /// in its own words. `None` when it is not running, because there is no
    /// file to describe.
    ///
    /// This used to carry a second field, `discovery_permissions_as_expected:
    /// Option<bool>`, derived from `Permissions::is_as_expected()`. That
    /// boolean is `true` for `InheritedFromProfile`, so the settings screen
    /// warned only when it was `false` and stayed silent on an inherited ACL
    /// -- reassuring where nothing had been verified. Removed rather than
    /// redefined: a screen that reads the real state has no use for a second,
    /// looser summary of the same fact. See docs/BENCH_CHECK.md, 2026-08-26
    /// finding 2.
    pub discovery_permissions: Option<DiscoveryPermissions>,
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
        discovery_permissions: permissions.as_ref().map(DiscoveryPermissions::from),
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

    #[tokio::test]
    async fn the_discovery_path_accessor_is_stable_whether_or_not_it_is_running() {
        // `integrations.rs` shows this path unconditionally -- a person
        // configuring the CLI wants to know where to look even before they
        // have switched the local API on.
        let (api, _db, _dir) = runtime().await;
        let before = api.discovery_path().to_path_buf();
        assert_eq!(before, api.discovery_path);

        api.start().await.unwrap();
        assert_eq!(api.discovery_path(), before);
        api.stop().await;
        assert_eq!(api.discovery_path(), before);
    }

    #[tokio::test]
    async fn the_permissions_snapshot_tracks_running_not_a_cached_start() {
        let (api, _db, _dir) = runtime().await;
        assert_eq!(api.permissions_snapshot(), None, "nothing has run yet");

        api.start().await.unwrap();
        assert!(
            api.permissions_snapshot().is_some(),
            "a real file was just written"
        );

        api.stop().await;
        assert_eq!(
            api.permissions_snapshot(),
            None,
            "the file is gone, so there is nothing left to report"
        );
    }

    #[test]
    fn discovery_permissions_carries_every_variant_without_collapsing_it() {
        // The property this type exists for: `InheritedFromProfile` must
        // convert to its own tag, never to `OwnerOnly` -- collapsing the two
        // is exactly the boolean-shaped defect this replaces.
        assert_eq!(
            DiscoveryPermissions::from(&Permissions::OwnerOnly),
            DiscoveryPermissions::OwnerOnly
        );
        assert_eq!(
            DiscoveryPermissions::from(&Permissions::InheritedFromProfile),
            DiscoveryPermissions::InheritedFromProfile
        );
        assert_ne!(
            DiscoveryPermissions::from(&Permissions::InheritedFromProfile),
            DiscoveryPermissions::OwnerOnly,
            "an inherited ACL must never present as a verified restriction"
        );
        assert_eq!(
            DiscoveryPermissions::from(&Permissions::Wider { mode: 0o644 }),
            DiscoveryPermissions::Wider {
                mode: "644".to_owned()
            }
        );
        assert_eq!(
            DiscoveryPermissions::from(&Permissions::Unknown {
                reason: "no chmod on this filesystem".to_owned()
            }),
            DiscoveryPermissions::Unknown {
                reason: "no chmod on this filesystem".to_owned()
            }
        );
    }

    #[test]
    fn only_owner_only_is_a_confirmed_restriction() {
        // The decision both the startup warning and the user-facing status
        // now share. `InheritedFromProfile` must land on the "not confirmed"
        // side even though `Permissions::is_as_expected()` treats it as
        // acceptable -- that is the whole point of this method existing
        // separately from that helper.
        assert!(DiscoveryPermissions::OwnerOnly.is_confirmed());
        assert!(!DiscoveryPermissions::InheritedFromProfile.is_confirmed());
        assert!(!DiscoveryPermissions::Wider {
            mode: "644".to_owned()
        }
        .is_confirmed());
        assert!(!DiscoveryPermissions::Unknown {
            reason: "no chmod on this filesystem".to_owned()
        }
        .is_confirmed());
    }
}
