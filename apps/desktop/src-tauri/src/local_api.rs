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
    runtime: Option<Arc<dyn kw_local_api::runtime::Runtime>>,
    uploads: std::path::PathBuf,
    operation: tokio::sync::Mutex<()>,
    db: Db,
    guard: std::sync::Mutex<Option<Arc<discovery::AccessGuard>>>,
    blocked_reason: std::sync::Mutex<Option<String>>,
    served: tokio::sync::Mutex<Option<Served>>,
    /// Whether there is a discovery file on disk for this runtime.
    ///
    /// A flag and not the permissions themselves, which is the whole of
    /// KNEG-110. This used to hold the `Permissions` that `discovery::write`
    /// returned, and every screen rendered that value for as long as the
    /// application ran -- so an access list widened after start was reported as
    /// whatever it had been at start. One field carried two facts, "there is a
    /// file" and "here is its state", and only the first of them stays true on
    /// its own.
    ///
    /// The state is read live by [`Self::permissions_now`]. This says only
    /// whether there is anything to read.
    discovery_written: std::sync::atomic::AtomicBool,
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
    pub fn with_runtime(
        mut self,
        runtime: Arc<dyn kw_local_api::runtime::Runtime>,
        uploads: std::path::PathBuf,
    ) -> Self {
        self.runtime = Some(runtime);
        self.uploads = uploads;
        self
    }
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
            runtime: None,
            uploads: std::path::PathBuf::new(),
            operation: tokio::sync::Mutex::new(()),
            db,
            guard: std::sync::Mutex::new(None),
            blocked_reason: std::sync::Mutex::new(None),
            served: tokio::sync::Mutex::new(None),
            discovery_written: std::sync::atomic::AtomicBool::new(false),
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
        let _operation = self.operation.lock().await;
        let result = self.start_inner().await;
        *self
            .blocked_reason
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = result.as_ref().err().cloned();
        result
    }

    async fn start_inner(&self) -> Result<u16, String> {
        let mut held = self.served.lock().await;
        if self.is_running() {
            if let Some(existing) = held.as_ref() {
                return Ok(existing.port());
            }
        }
        if let Some(stale) = held.take() {
            stale.shutdown().await;
        }
        self.discovery_written
            .store(false, std::sync::atomic::Ordering::Release);

        *self
            .guard
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = None;

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

        let path = self.discovery_path.clone();
        let write_token = token.clone();
        let permissions = match tokio::task::spawn_blocking(move || {
            discovery::write(&path, port, &write_token)
        })
        .await
        .map_err(|e| e.to_string())?
        {
            Ok(verdict) => verdict,
            Err(discovery::DiscoveryError::Permissions { verdict }) => {
                let error = verdict.explain();
                *self
                    .guard
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(Arc::new(
                    discovery::AccessGuard::new(self.discovery_path.clone(), verdict),
                ));
                return Err(format!("Local API permissions could not be secured: {error}. Correct the permissions and retry."));
            }
            Err(error) => return Err(error.to_string()),
        };
        if !DiscoveryPermissions::from(&permissions).is_confirmed() {
            return Err("Local API permission verification failed.".to_owned());
        }
        let guard = Arc::new(discovery::AccessGuard::new(
            self.discovery_path.clone(),
            permissions,
        ));
        let served = match serve(listener, {
            let state =
                ApiState::new(self.db.pool().clone(), token).with_access_guard(Arc::clone(&guard));
            match &self.runtime {
                Some(runtime) => state.with_runtime(Arc::clone(runtime), self.uploads.clone()),
                None => state,
            }
        }) {
            Ok(served) => served,
            Err(error) => {
                let _ = discovery::remove(&self.discovery_path);
                return Err(error.to_string());
            }
        };
        *self
            .guard
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(guard);

        self.discovery_written
            .store(true, std::sync::atomic::Ordering::Release);
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

    /// Read live permissions while active; retain the failed verdict after
    /// revocation so screens can explain a stopped API. Explicit Stop clears it.
    pub(crate) fn permissions_now(&self) -> Option<Permissions> {
        let guard = self
            .guard
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        guard.as_ref().map(|guard| {
            if guard.is_active() {
                discovery::inspect(&self.discovery_path)
            } else {
                guard.verdict()
            }
        })
    }

    /// A published discovery file and a token that has not been revoked.
    pub(crate) fn is_running(&self) -> bool {
        self.discovery_written
            .load(std::sync::atomic::Ordering::Acquire)
            && self
                .guard
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .as_ref()
                .is_some_and(|guard| guard.is_active())
    }

    pub(crate) fn blocked_reason(&self) -> Option<String> {
        if let Some(guard) = self
            .guard
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_ref()
        {
            if !guard.is_active() {
                return Some(format!("Local API access was revoked: {} Retry to create a new token after permissions are secured.", guard.verdict().explain()));
            }
        }
        self.blocked_reason
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    /// Stop the server and remove the discovery file.
    ///
    /// The file goes with the server: a leftover sends the next client at a port
    /// nothing is listening on, with a token nothing accepts.
    pub async fn stop(&self) {
        let _operation = self.operation.lock().await;
        let mut held = self.served.lock().await;
        if let Some(served) = held.take() {
            served.shutdown().await;
        }

        if let Err(err) = discovery::remove(&self.discovery_path) {
            tracing::warn!(error = %err, "could not remove the discovery file");
        }

        self.discovery_written
            .store(false, std::sync::atomic::Ordering::Release);
        *self
            .guard
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
        *self
            .blocked_reason
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
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
/// `Permissions::is_as_expected()` returned `true` for `InheritedFromProfile`
/// — correct as "the best this build currently achieves on this platform",
/// read by a screen that only had the boolean as "verified restricted", which
/// it was not: nothing was checked, it was simply what a file under
/// `%USERPROFILE%` gets by default, and two other trustees turned up in that
/// default on a real machine. See `docs/BENCH_CHECK.md`, 2026-08-26 finding 2.
/// That helper has since been deleted; this type and its
/// [`Self::is_confirmed`] are what replaced it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(
    tag = "state",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum DiscoveryPermissions {
    /// Unix. Verified by reading the file's mode back: owner read/write only.
    OwnerOnly,
    /// Windows. Verified by reading the file's access list back: this
    /// application's user, `NT AUTHORITY\SYSTEM`, `BUILTIN\Administrators`,
    /// nobody else, and nothing inherited. An administrator can still read it
    /// — they can take ownership of anything — and the label says so.
    OwnerAndAdministrators,
    /// Nothing was applied and nothing was read back, because the platform has
    /// neither mechanism. Reachable only on a target that is neither Unix nor
    /// Windows; on Windows this used to be the answer and no longer is.
    InheritedFromProfile,
    /// Unix. The mode was set and read back wider than intended.
    Wider {
        /// Octal, e.g. `"644"`, matching how a person would type it to `chmod`.
        mode: String,
    },
    /// Windows. The access list grants accounts or groups beyond the three
    /// that were asked for.
    ExtraTrustees {
        /// How many, not which. A trustee name is one of this machine's
        /// account or group names, and this value is rendered on a screen that
        /// ends up in screenshots and bug reports. The names go to the
        /// application log instead.
        count: u32,
    },
    /// Windows. The right three, but still inherited from the directory above
    /// — so what the file grants is whatever that directory grants, and it can
    /// change without anything here being asked.
    StillInheriting,
    /// Windows. The access list grants a group meaning "everyone on this
    /// machine", or there is no access list at all — which grants everything
    /// to everybody rather than nothing to anybody.
    GrantsEveryone,
    /// Windows. Nothing unexpected in the list, but what was asked for is not
    /// all there.
    Incomplete,
    /// The file's own permissions are right and the directory holding it is
    /// not restricted, so it can be deleted and replaced by anything that can
    /// write there. For a file whose contents *are* the access control, that
    /// is worse than it being readable.
    DirectoryNotRestricted,
    /// The permissions could not be applied, or could not be read back.
    Unknown { reason: String },
}

impl From<&Permissions> for DiscoveryPermissions {
    fn from(permissions: &Permissions) -> Self {
        match permissions {
            Permissions::OwnerOnly => Self::OwnerOnly,
            Permissions::OwnerAndAdministrators => Self::OwnerAndAdministrators,
            Permissions::InheritedFromProfile => Self::InheritedFromProfile,
            Permissions::Wider { mode } => Self::Wider {
                mode: format!("{mode:o}"),
            },
            // `u32` rather than `usize`: `ts-rs` renders both as `number`, and
            // a count of trustees that needs 64 bits is not a thing.
            Permissions::ExtraTrustees { count } => Self::ExtraTrustees {
                count: u32::try_from(*count).unwrap_or(u32::MAX),
            },
            Permissions::StillInheriting => Self::StillInheriting,
            Permissions::GrantsEveryone => Self::GrantsEveryone,
            Permissions::Incomplete => Self::Incomplete,
            Permissions::DirectoryNotRestricted => Self::DirectoryNotRestricted,
            Permissions::Unknown { reason } => Self::Unknown {
                reason: reason.clone(),
            },
        }
    }
}

impl DiscoveryPermissions {
    /// Whether this state was **checked**, not merely asked for.
    ///
    /// The line is checked versus unchecked — not strict versus lax.
    /// [`Self::OwnerAndAdministrators`] is on the confirmed side even though
    /// administrators can read the file, because that grant is deliberate
    /// (an administrator can take ownership of anything, so excluding them
    /// buys nothing) and because the list was read back entry by entry.
    /// [`Self::InheritedFromProfile`] is on the other side even though it may
    /// well be fine, because *nothing looked* — which is the whole reason this
    /// method exists separately from the boolean it replaced.
    ///
    /// Both the user-facing report (`local_api_status`) and the startup
    /// warning (`LocalApiRuntime::start`) go through this one decision, so
    /// there is exactly one place that draws the line rather than two that
    /// could drift. See docs/BENCH_CHECK.md, 2026-08-26 finding 2.
    #[must_use]
    pub const fn is_confirmed(&self) -> bool {
        matches!(self, Self::OwnerOnly | Self::OwnerAndAdministrators)
    }
}

/// What the settings screen shows about the local API.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct LocalApiStatus {
    pub blocked_reason: Option<String>,
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
    /// boolean was `true` for `InheritedFromProfile`, so the settings screen
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

    let port = api
        .served
        .lock()
        .await
        .as_ref()
        .filter(|_| api.is_running())
        .map(Served::port);
    let permissions = api.permissions_now();

    Ok(LocalApiStatus {
        enabled,
        running: api.is_running(),
        blocked_reason: api.blocked_reason(),
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
    async fn permission_monitor_revokes_without_waiting_for_a_client_request() {
        let (api, _db, _dir) = runtime().await;
        api.start().await.unwrap();
        discovery::remove(&api.discovery_path).unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(8), async {
            while api.is_running() {
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            }
        })
        .await
        .unwrap();
        assert!(api.blocked_reason().is_some());
        api.stop().await;
    }

    #[tokio::test]
    async fn revoked_token_cannot_be_reused_after_explicit_retry() {
        let (api, _db, _dir) = runtime().await;
        let port = api.start().await.unwrap();
        let (_, old_token) = discovery::read(&api.discovery_path).unwrap();
        discovery::remove(&api.discovery_path).unwrap();
        let client = reqwest::Client::new();
        let response = client
            .get(format!("http://127.0.0.1:{port}/v1/health"))
            .bearer_auth(old_token.expose())
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), reqwest::StatusCode::UNAUTHORIZED);
        assert!(!api.is_running());
        assert!(api.blocked_reason().is_some());
        let port = api.start().await.unwrap();
        let (_, new_token) = discovery::read(&api.discovery_path).unwrap();
        assert_ne!(old_token.expose(), new_token.expose());
        let url = format!("http://127.0.0.1:{port}/v1/health");
        assert_eq!(
            client
                .get(&url)
                .bearer_auth(old_token.expose())
                .send()
                .await
                .unwrap()
                .status(),
            reqwest::StatusCode::UNAUTHORIZED
        );
        assert!(client
            .get(&url)
            .bearer_auth(new_token.expose())
            .send()
            .await
            .unwrap()
            .status()
            .is_success());
        api.stop().await;
    }

    #[tokio::test]
    async fn failed_start_has_a_visible_reason_and_no_listener() {
        let (api, _db, _dir) = runtime().await;
        std::fs::create_dir(&api.discovery_path).unwrap();
        assert!(api.start().await.is_err());
        assert!(!api.is_running());
        assert!(api.served.lock().await.is_none());
        assert!(api.blocked_reason().is_some());
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

        let permissions = api.permissions_now().unwrap();
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
    async fn there_is_nothing_to_report_until_a_file_exists() {
        // The `Option` means "is there a discovery file", and since KNEG-110
        // that is *all* it means -- the value inside it is read fresh.
        let (api, _db, _dir) = runtime().await;
        assert_eq!(api.permissions_now(), None, "nothing has run yet");
        assert!(!api.is_running());

        api.start().await.unwrap();
        assert!(
            api.permissions_now().is_some(),
            "a real file was just written"
        );
        assert!(api.is_running());

        api.stop().await;
        assert_eq!(
            api.permissions_now(),
            None,
            "the file is gone, so there is nothing left to report"
        );
        assert!(!api.is_running());
    }

    /// The regression KNEG-110 was opened for: a verdict stored at start and
    /// shown forever.
    ///
    /// The file is removed behind the runtime's back, which is what anything
    /// with write access to `~/.knegiswhisper` could do at any moment. Nothing
    /// tells `LocalApiRuntime` it happened. A stored snapshot would go on
    /// reporting the restriction it verified at start; a re-read cannot.
    ///
    /// Deleting rather than widening because that is the one way to change what
    /// the access list says that works identically on every platform. The Unix
    /// test below drives the case the issue actually described -- a file that
    /// still exists and now grants more -- and runs on CI's Linux box, which is
    /// the only place the whole of this file is compiled.
    #[tokio::test]
    async fn the_verdict_is_re_read_rather_than_remembered() {
        let (api, _db, _dir) = runtime().await;
        api.start().await.unwrap();

        let at_start = api.permissions_now().expect("it is running");
        std::fs::remove_file(&api.discovery_path).unwrap();
        let now = api
            .permissions_now()
            .expect("still running as far as it knows");

        assert_ne!(
            at_start, now,
            "the verdict is a re-read; a stored one could not have changed"
        );
        assert!(
            !DiscoveryPermissions::from(&now).is_confirmed(),
            "a file that is not there is not a verified restriction: {}",
            now.explain()
        );

        api.stop().await;
    }

    /// The case the issue described, on the platform where it can be staged:
    /// the file is still there and now grants more than it did.
    #[cfg(unix)]
    #[tokio::test]
    async fn a_mode_widened_after_start_is_reported() {
        use std::os::unix::fs::PermissionsExt as _;

        let (api, _db, _dir) = runtime().await;
        api.start().await.unwrap();
        assert!(matches!(
            api.permissions_now(),
            Some(Permissions::OwnerOnly)
        ));

        std::fs::set_permissions(&api.discovery_path, std::fs::Permissions::from_mode(0o644))
            .unwrap();

        match api.permissions_now() {
            Some(Permissions::Wider { mode }) => assert_eq!(mode, 0o644),
            other => panic!("a world-readable token file should be reported: {other:?}"),
        }

        api.stop().await;
    }

    /// Every `Permissions` variant, paired with what it must convert to.
    ///
    /// A `const` list rather than a loop over the enum, because Rust has no
    /// way to enumerate one: adding a variant to `kw_security::Permissions`
    /// makes the `From` impl fail to compile, and this table is where the
    /// author is then reminded to say what it means here too.
    fn every_variant() -> Vec<(Permissions, DiscoveryPermissions)> {
        vec![
            (Permissions::OwnerOnly, DiscoveryPermissions::OwnerOnly),
            (
                Permissions::OwnerAndAdministrators,
                DiscoveryPermissions::OwnerAndAdministrators,
            ),
            (
                Permissions::InheritedFromProfile,
                DiscoveryPermissions::InheritedFromProfile,
            ),
            (
                Permissions::Wider { mode: 0o644 },
                DiscoveryPermissions::Wider {
                    mode: "644".to_owned(),
                },
            ),
            (
                Permissions::ExtraTrustees { count: 2 },
                DiscoveryPermissions::ExtraTrustees { count: 2 },
            ),
            (
                Permissions::StillInheriting,
                DiscoveryPermissions::StillInheriting,
            ),
            (
                Permissions::GrantsEveryone,
                DiscoveryPermissions::GrantsEveryone,
            ),
            (Permissions::Incomplete, DiscoveryPermissions::Incomplete),
            (
                Permissions::DirectoryNotRestricted,
                DiscoveryPermissions::DirectoryNotRestricted,
            ),
            (
                Permissions::Unknown {
                    reason: "no chmod on this filesystem".to_owned(),
                },
                DiscoveryPermissions::Unknown {
                    reason: "no chmod on this filesystem".to_owned(),
                },
            ),
        ]
    }

    #[test]
    fn discovery_permissions_carries_every_variant_without_collapsing_it() {
        // The property this type exists for: every state converts to its own
        // tag. Collapsing two into one is exactly the boolean-shaped defect
        // this replaced.
        for (core, expected) in every_variant() {
            assert_eq!(
                DiscoveryPermissions::from(&core),
                expected,
                "{core:?} must keep its own tag"
            );
        }

        assert_ne!(
            DiscoveryPermissions::from(&Permissions::InheritedFromProfile),
            DiscoveryPermissions::OwnerOnly,
            "an unchecked inherited ACL must never present as a verified restriction"
        );
        assert_ne!(
            DiscoveryPermissions::from(&Permissions::OwnerAndAdministrators),
            DiscoveryPermissions::OwnerOnly,
            "Windows grants administrators on purpose, and must not read as owner-only"
        );
    }

    #[test]
    fn a_confirmed_restriction_is_one_that_was_checked_not_one_that_is_strict() {
        // Renamed from `only_owner_only_is_a_confirmed_restriction`, which
        // argued a line this no longer draws. `OwnerAndAdministrators` lets
        // administrators read the file and is still confirmed, because the
        // list was read back entry by entry and an administrator can take
        // ownership of anything regardless. `InheritedFromProfile` may well be
        // perfectly restrictive and is still not confirmed, because nothing
        // looked. Strictness is not the axis; evidence is.
        assert!(DiscoveryPermissions::OwnerOnly.is_confirmed());
        assert!(DiscoveryPermissions::OwnerAndAdministrators.is_confirmed());

        assert!(!DiscoveryPermissions::InheritedFromProfile.is_confirmed());
        assert!(!DiscoveryPermissions::Wider {
            mode: "644".to_owned()
        }
        .is_confirmed());
        assert!(!DiscoveryPermissions::ExtraTrustees { count: 2 }.is_confirmed());
        assert!(!DiscoveryPermissions::StillInheriting.is_confirmed());
        assert!(!DiscoveryPermissions::GrantsEveryone.is_confirmed());
        assert!(!DiscoveryPermissions::Incomplete.is_confirmed());
        assert!(!DiscoveryPermissions::DirectoryNotRestricted.is_confirmed());
        assert!(!DiscoveryPermissions::Unknown {
            reason: "no chmod on this filesystem".to_owned()
        }
        .is_confirmed());
    }

    #[test]
    fn the_two_confirmed_decisions_cannot_drift_apart() {
        // `kw_security::Permissions::is_confirmed` and this type's are two
        // definitions of one line, which is the shape of the original defect.
        // They are kept honest by assertion rather than by hope.
        for (core, converted) in every_variant() {
            assert_eq!(
                core.is_confirmed(),
                converted.is_confirmed(),
                "{core:?} is judged differently on the two sides of the boundary"
            );
        }
    }
}
