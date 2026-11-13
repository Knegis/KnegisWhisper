//! `Integrations > Calendar`: the Tauri command layer over `kw_calendar_core`
//! for Google/Microsoft OAuth plus event sync, and Apple's described-but-
//! unimplemented EventKit stub.
//!
//! # The one sender this file may construct
//!
//! `kw_calendar_core::TokenSender` is an unsealed trait -- Task 7's review
//! cleared that as a legitimate test double, on the understanding that
//! nothing in production would ever build a different implementation.
//! Nothing here does: [`kw_calendar_core::NetworkPolicySender`] is
//! constructed in exactly one place, [`CalendarRuntime::connect`] and
//! [`CalendarRuntime::sync`] each build their own short-lived one over the
//! same [`kw_network_policy::NetworkPolicy`] this application already owns,
//! and no other type in this file implements `TokenSender` at all. Every
//! calendar-list and event fetch in [`kw_calendar_core::google`] and
//! [`kw_calendar_core::microsoft`] goes through that same `NetworkPolicy`
//! directly, under the identical `RequestPurpose::UserConfiguredCalendarProvider`
//! -- no eighth purpose is introduced anywhere in this module.
//!
//! OAuth clients resolve from an Advanced custom override, then the app-owned
//! compile-time client. Google's optional desktop secret stays in Rust and is
//! used only at the token endpoint; custom secrets live in the credential store.
//!
//! # Honesty
//!
//! [`calendar_status`] never claims a provider is connected on the strength
//! of a credential merely being present. [`token_status`] checks the
//! credential store directly rather than trusting the database row alone --
//! see its doc comment -- and it makes no network call, matching every other
//! status command in this application (`meeting_status`, `credential_slots`;
//! see `docs/NETWORK_BOUNDARY.md`). The commands that do reach the
//! network are [`calendar_connect`], [`calendar_sync`] and the throttled
//! [`calendar_sync_if_stale`] invoked when Meetings opens. A sync's outcome,
//! including a failure, is persisted to
//! settings and folded back into the next `calendar_status` read, so a
//! failed sync is never silently indistinguishable from an empty, successful
//! one, and an expired token that could not be refreshed says so rather than
//! showing stale rows as current.

use crate::calendar_clients::{self, ClientChoice, ClientSource};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};

use kw_calendar_core::{
    apple, begin_authorization, calendars_repo, complete_authorization, credentials,
    ensure_fresh_access_token, events_repo, google, meetings, microsoft, tokens_repo,
    CalendarError, CalendarProviderKind, LoopbackReceiver, NetworkPolicySender, OAuthClientConfig,
    ProviderEndpoints,
};
use kw_network_policy::NetworkPolicy;
use kw_security::{CredentialStore, KeyRef, Secret};
use kw_storage::{settings, Db};
use serde::{Deserialize, Serialize};
use tauri::State;
use ts_rs::TS;

/// A command failure, as the frontend sees it. Mirrors `commands::CmdResult`.
type CmdResult<T> = Result<T, String>;

fn fail(context: &str, err: impl std::fmt::Display) -> String {
    format!("{context}: {err}")
}

/// How long to wait for the browser to complete the consent screen and come
/// back to the loopback receiver. Minutes, not seconds: unlike every other
/// timeout in this application this one is waiting on a person, not a
/// server.
const REDIRECT_WAIT: Duration = Duration::from_secs(300);

/// Consumers reread local data; the notification deliberately carries no data.
pub const CHANGED_EVENT: &str = "calendar-changed";

const PROVIDERS: [CalendarProvider; 2] = [CalendarProvider::Google, CalendarProvider::Microsoft];

const CALLBACK_PATH: &str = "/calendar/callback";

/* -------------------------------------------------------------- provider id */

/// Which calendar provider a command is about, as it crosses the wire.
/// Mirrors `kw_calendar_core::CalendarProviderKind`, which has no `Serialize`
/// of its own -- that crate has no reason to know about JSON at all, so the
/// wire vocabulary lives here instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum CalendarProvider {
    Google,
    Microsoft,
}

impl CalendarProvider {
    const fn kind(self) -> CalendarProviderKind {
        match self {
            Self::Google => CalendarProviderKind::Google,
            Self::Microsoft => CalendarProviderKind::Microsoft,
        }
    }

    /// The value `calendar_events.provider` and `events_repo::row_id` use.
    const fn wire_str(self) -> &'static str {
        match self {
            Self::Google => "google",
            Self::Microsoft => "microsoft",
        }
    }

    const fn name(self) -> &'static str {
        match self {
            Self::Google => "Google Calendar",
            Self::Microsoft => "Microsoft Calendar",
        }
    }

    fn endpoints(self) -> ProviderEndpoints {
        match self {
            Self::Google => ProviderEndpoints::google(),
            Self::Microsoft => ProviderEndpoints::microsoft(),
        }
    }
}

/// The settings key holding the client ID the user pasted in for `provider`.
/// Not a secret -- see the module doc on `kw_calendar_core` -- so it lives in
/// `kw_storage::settings` rather than the credential store.
fn client_id_key(provider: CalendarProvider) -> &'static str {
    match provider {
        CalendarProvider::Google => "calendar.google.clientId",
        CalendarProvider::Microsoft => "calendar.microsoft.clientId",
    }
}

/// The settings key holding the email of the one account connected for
/// `provider`. This build supports at most one connected account per
/// provider at a time; connecting a different account replaces it -- see
/// [`CalendarRuntime::connect`].
fn account_key(provider: CalendarProvider) -> &'static str {
    match provider {
        CalendarProvider::Google => "calendar.google.account",
        CalendarProvider::Microsoft => "calendar.microsoft.account",
    }
}

/// The settings key holding the most recent [`SyncOutcome`], so a status read
/// never needs the network to report what the last sync attempt found.
fn last_sync_key(provider: CalendarProvider) -> &'static str {
    match provider {
        CalendarProvider::Google => "calendar.google.lastSync",
        CalendarProvider::Microsoft => "calendar.microsoft.lastSync",
    }
}

fn now_epoch() -> i64 {
    chrono::Utc::now().timestamp()
}

/// Secret edits are explicit: an omitted edit must never erase a saved value.
/// Debug intentionally omits Set's payload when callers inspect a pending edit.
#[derive(Deserialize, TS)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum SecretChange {
    Keep,
    Set { value: String },
    Clear,
}

impl std::fmt::Debug for SecretChange {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Keep => "Keep",
            Self::Set { .. } => "Set { value: <redacted> }",
            Self::Clear => "Clear",
        })
    }
}

fn custom_secret_slot() -> CmdResult<KeyRef> {
    KeyRef::new("calendar-google-custom-client")
        .ok_or_else(|| "invalid calendar custom client slot".to_owned())
}

fn read_custom_secret(
    credentials: &dyn CredentialStore,
    provider: CalendarProvider,
) -> CmdResult<Option<Secret>> {
    if provider == CalendarProvider::Microsoft {
        return Ok(None);
    }
    match credentials.get(&custom_secret_slot()?) {
        Ok(secret) => Ok(Some(secret)),
        Err(kw_security::CredentialError::NotFound(_)) => Ok(None),
        Err(_) => Err("could not read the custom Google client secret".to_owned()),
    }
}

/// SQLite and the OS store cannot share a transaction. Read the old pair first,
/// change the credential first, then compensate if the atomic settings write
/// fails. Credential errors are static so a backend cannot echo a secret to IPC.
/// The caller supplies the final write so each command declares its settings
/// ownership in its body, where the settings audit can attribute it.
async fn persist_custom_client<Fut>(
    db: &Db,
    credentials: &dyn CredentialStore,
    provider: CalendarProvider,
    client_id: Option<&str>,
    change: SecretChange,
    write_id: impl FnOnce(Option<String>) -> Fut,
) -> CmdResult<()>
where
    Fut: std::future::Future<Output = Result<(), kw_storage::StorageError>>,
{
    if provider == CalendarProvider::Microsoft && !matches!(change, SecretChange::Keep) {
        return Err("Outlook clients do not accept a Google client secret".to_owned());
    }
    let id = client_id.map(str::trim).filter(|id| !id.is_empty());
    if matches!(change, SecretChange::Set { .. }) && id.is_none() {
        return Err("a custom client ID is required to set a Google client secret".to_owned());
    }
    let previous_id = settings::get::<String>(db.pool(), client_id_key(provider))
        .await
        .map_err(|e| fail("could not read the client ID", e))?;
    let previous_secret = read_custom_secret(credentials, provider)?;
    let changed = previous_id
        .as_deref()
        .map(str::trim)
        .filter(|id| !id.is_empty())
        != id;
    let credential_changed = provider == CalendarProvider::Google
        && (changed || id.is_none() || !matches!(change, SecretChange::Keep));
    let slot = custom_secret_slot()?;
    if credential_changed {
        match change {
            SecretChange::Set { value } => credentials.set(&slot, &Secret::new(value)),
            SecretChange::Clear | SecretChange::Keep => credentials.delete(&slot),
        }
        .map_err(|_| "could not save the custom Google client secret".to_owned())?;
    }
    let result = write_id(id.map(str::to_owned)).await;
    if let Err(error) = result {
        if credential_changed {
            let restore = match previous_secret {
                Some(secret) => credentials.set(&slot, &secret),
                None => credentials.delete(&slot),
            };
            if restore.is_err() {
                return Err(
                    "could not save the client ID or restore the previous custom client secret"
                        .to_owned(),
                );
            }
        }
        return Err(fail("could not save the client ID", error));
    }
    Ok(())
}

/// Stable, allowlisted failure vocabulary. Provider prose and bodies never cross IPC.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum CalendarProblem {
    Cancelled,
    TimedOut,
    AccessDenied,
    AdminConsentRequired,
    AppBlocked,
    ClientMisconfigured,
    ClientRejected,
    ReconnectRequired,
    Network,
    ProviderUnavailable,
    Storage,
    InvalidResponse,
}

/// Classification has no I/O and examines only known codes, never arbitrary prose.
fn classify(error: &CalendarError, provider: CalendarProvider, refresh: bool) -> CalendarProblem {
    use kw_calendar_core::ExchangeError;
    use CalendarProblem as P;
    match error {
        CalendarError::AuthorizationDenied { error, description } => {
            if error == "access_denied" {
                return P::AccessDenied;
            }
            if provider == CalendarProvider::Google
                && matches!(error.as_str(), "admin_policy_enforced" | "org_internal")
            {
                return P::AppBlocked;
            }
            if provider == CalendarProvider::Microsoft {
                if let Some(problem) = aad_problem(description.as_deref()) {
                    return problem;
                }
            }
            if error == "redirect_uri_mismatch" {
                P::ClientMisconfigured
            } else {
                P::InvalidResponse
            }
        }
        CalendarError::Exchange(ExchangeError::Rejected { error, description }) => {
            if provider == CalendarProvider::Microsoft {
                if let Some(problem) = aad_problem(description.as_deref()) {
                    return problem;
                }
            }
            match error.as_str() {
                "invalid_client" | "unauthorized_client" => P::ClientRejected,
                "invalid_grant" if refresh => P::ReconnectRequired,
                "redirect_uri_mismatch" => P::ClientMisconfigured,
                _ => P::InvalidResponse,
            }
        }
        CalendarError::Exchange(ExchangeError::HttpStatus { status: 500..=599 }) => {
            P::ProviderUnavailable
        }
        CalendarError::Exchange(ExchangeError::HttpStatus { .. }) => P::InvalidResponse,
        CalendarError::Exchange(ExchangeError::Http(error)) => {
            if error.is_decode() {
                P::InvalidResponse
            } else {
                P::Network
            }
        }
        CalendarError::Exchange(ExchangeError::Policy(_)) => P::Network,
        CalendarError::ReconnectRequired | CalendarError::NotConnected { .. } => {
            P::ReconnectRequired
        }
        CalendarError::Credential(_) | CalendarError::Storage(_) => P::Storage,
        _ => P::InvalidResponse,
    }
}

fn problem_message(problem: CalendarProblem) -> String {
    match problem {
        CalendarProblem::Cancelled => "Connection cancelled.",
        CalendarProblem::TimedOut => "Sign-in timed out. Try again.",
        CalendarProblem::AccessDenied => "Calendar access was declined.",
        CalendarProblem::AdminConsentRequired => "Your administrator must approve calendar access.",
        CalendarProblem::AppBlocked => "Your organisation has blocked this app.",
        CalendarProblem::ClientMisconfigured => "The calendar client needs configuration.",
        CalendarProblem::ClientRejected => "The provider rejected this calendar client.",
        CalendarProblem::ReconnectRequired => "Reconnect this account to keep syncing.",
        CalendarProblem::Network => "Could not reach the calendar provider. Try again.",
        CalendarProblem::ProviderUnavailable => "The calendar provider is unavailable. Try again.",
        CalendarProblem::Storage => "Could not save or read calendar data.",
        CalendarProblem::InvalidResponse => "The calendar provider returned an invalid response.",
    }
    .to_owned()
}

fn sync_failure(problem: CalendarProblem) -> SyncOutcome {
    let message = problem_message(problem);
    if problem == CalendarProblem::ReconnectRequired {
        SyncOutcome::TokenUnavailable {
            message,
            problem: Some(problem),
        }
    } else {
        SyncOutcome::Failed {
            message,
            problem: Some(problem),
        }
    }
}

fn aad_problem(description: Option<&str>) -> Option<CalendarProblem> {
    // Match a complete AADSTS code prefix, not a numeric substring in prose.
    description?.split_whitespace().find_map(|word| {
        let code = word.split_once(':')?.0;
        match code {
            "AADSTS65001" | "AADSTS90094" | "AADSTS90095" => {
                Some(CalendarProblem::AdminConsentRequired)
            }
            "AADSTS50011" | "AADSTS700016" => Some(CalendarProblem::ClientMisconfigured),
            _ => None,
        }
    })
}

fn classify_sync(error: &kw_calendar_core::SyncError) -> CalendarProblem {
    use kw_calendar_core::SyncError;
    match error {
        SyncError::HttpStatus { status: 500..=599 } => CalendarProblem::ProviderUnavailable,
        SyncError::HttpStatus { status: 401 | 403 } => CalendarProblem::ClientMisconfigured,
        SyncError::Http(_) | SyncError::Policy(_) => CalendarProblem::Network,
        SyncError::Storage(_) => CalendarProblem::Storage,
        _ => CalendarProblem::InvalidResponse,
    }
}

fn classify_redirect(error: &kw_calendar_core::RedirectError) -> CalendarProblem {
    use kw_calendar_core::RedirectError;
    match error {
        RedirectError::TimedOut => CalendarProblem::TimedOut,
        RedirectError::MalformedRequest => CalendarProblem::InvalidResponse,
        _ => CalendarProblem::Network,
    }
}

#[derive(Debug, Clone, Copy, Default, Serialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum CalendarOperation {
    #[default]
    Idle,
    Connecting,
    Syncing,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct PendingSignIn {
    pub authorize_url: String,
    pub browser_opened: bool,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum LeftoverOrigin {
    Openwhispr,
    Earlier,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct LeftoverConnection {
    pub email: String,
    pub origin: LeftoverOrigin,
}

#[derive(Default)]
struct OperationState {
    operation: CalendarOperation,
    pending_sign_in: Option<PendingSignIn>,
    cancel: Option<tokio::sync::oneshot::Sender<()>>,
}

#[derive(Default)]
struct ProviderControl {
    guard: tokio::sync::Mutex<()>,
    state: Mutex<OperationState>,
    last_attempt: Mutex<Option<Instant>>,
}

impl ProviderControl {
    fn state(&self) -> std::sync::MutexGuard<'_, OperationState> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
    fn try_acquire(&self, operation: CalendarOperation) -> Option<OperationGuard<'_>> {
        let guard = self.guard.try_lock().ok()?;
        self.state().operation = operation;
        Some(OperationGuard {
            control: self,
            _guard: guard,
        })
    }

    fn acquire(&self, operation: CalendarOperation) -> CmdResult<OperationGuard<'_>> {
        self.try_acquire(operation)
            .ok_or_else(|| "a calendar connection or sync is already in progress".to_owned())
    }

    fn attempt_clock(&self) -> std::sync::MutexGuard<'_, Option<Instant>> {
        self.last_attempt
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

/// Clear status even if the command future is dropped, before releasing its guard.
struct OperationGuard<'a> {
    control: &'a ProviderControl,
    _guard: tokio::sync::MutexGuard<'a, ()>,
}
impl Drop for OperationGuard<'_> {
    fn drop(&mut self) {
        *self.control.state() = OperationState::default();
    }
}

fn cleanup_key(provider: CalendarProvider) -> &'static str {
    match provider {
        CalendarProvider::Google => "calendar.google.cleanupRefs",
        CalendarProvider::Microsoft => "calendar.microsoft.cleanupRefs",
    }
}

/// Already-fetched calendar metadata; installing it cannot reach the network.
enum GrantCalendars {
    Google(Vec<google::CalendarListEntry>),
    Microsoft(Vec<microsoft::CalendarListEntry>),
}

/// Drop after operation guards, including error exits, so listeners see the
/// completed local state. A failed operation can still have changed cleanup data.
struct NotifyOnCompletion<'a>(&'a (dyn Fn() + Send + Sync));
impl Drop for NotifyOnCompletion<'_> {
    fn drop(&mut self) {
        (self.0)();
    }
}

/* -------------------------------------------------------------- the runtime */

/// What connecting an account, or syncing one, needs: the database,
/// credential store, and network boundary the rest of the application
/// already built, plus independent provider guards. Operations for the same
/// provider refuse overlapping clicks; Google and Outlook can run together.
pub struct CalendarRuntime {
    db: Db,
    credentials: Arc<dyn CredentialStore>,
    policy: NetworkPolicy,
    google: ProviderControl,
    microsoft: ProviderControl,
    browser_opener: fn(&str) -> std::io::Result<()>,
    on_changed: Arc<dyn Fn() + Send + Sync>,
}

impl std::fmt::Debug for CalendarRuntime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CalendarRuntime").finish_non_exhaustive()
    }
}

impl CalendarRuntime {
    #[must_use]
    pub fn new(db: Db, credentials: Arc<dyn CredentialStore>, policy: NetworkPolicy) -> Self {
        Self {
            db,
            credentials,
            policy,
            google: ProviderControl::default(),
            microsoft: ProviderControl::default(),
            browser_opener: open_in_browser,
            on_changed: Arc::new(|| {}),
        }
    }

    /// The app supplies an emitter; tests can count completions without Tauri.
    #[must_use]
    pub fn with_notifier(mut self, on_changed: Arc<dyn Fn() + Send + Sync>) -> Self {
        self.on_changed = on_changed;
        self
    }

    fn notify_on_completion(&self) -> NotifyOnCompletion<'_> {
        NotifyOnCompletion(self.on_changed.as_ref())
    }

    fn control(&self, provider: CalendarProvider) -> &ProviderControl {
        match provider {
            CalendarProvider::Google => &self.google,
            CalendarProvider::Microsoft => &self.microsoft,
        }
    }

    async fn client(
        &self,
        provider: CalendarProvider,
    ) -> Result<OAuthClientConfig, CalendarProblem> {
        let custom_id = settings::get::<String>(self.db.pool(), client_id_key(provider))
            .await
            .map_err(|_| CalendarProblem::Storage)?;
        let secret = read_custom_secret(self.credentials.as_ref(), provider)
            .map_err(|_| CalendarProblem::Storage)?;
        match calendar_clients::resolve(provider.kind(), custom_id.as_deref(), secret) {
            ClientChoice::Custom(client) | ClientChoice::AppOwned(client) => Ok(client),
            ClientChoice::Missing => Err(CalendarProblem::ClientMisconfigured),
        }
    }

    /// The account currently on file as connected for `provider`, if any.
    ///
    /// A read failure is propagated as `Err`, never folded into `Ok(None)`.
    /// Grant installation reads it before writing a credential so its one
    /// transaction can remove the previous account and queue the actual old
    /// slot. A read failure must never silently skip that replacement work.
    async fn read_previous_account(&self, provider: CalendarProvider) -> CmdResult<Option<String>> {
        settings::get::<String>(self.db.pool(), account_key(provider))
            .await
            .map_err(|e| fail("could not read the previously connected account", e))
    }

    /// Consent and discovery perform no local grant writes. Installation has one
    /// SQLite commit point and compensates the preceding credential write.
    pub async fn connect(&self, provider: CalendarProvider) -> CmdResult<ConnectOutcome> {
        let _notify = self.notify_on_completion();
        let _guard = self
            .control(provider)
            .acquire(CalendarOperation::Connecting)?;
        let outcome = self.connect_inner(provider).await;
        Ok(match outcome {
            Ok(connected) => connected,
            Err(CalendarProblem::AccessDenied) => ConnectOutcome::Denied {
                reason: "access_denied".to_owned(),
            },
            Err(problem) => ConnectOutcome::Failed { problem },
        })
    }

    async fn connect_inner(
        &self,
        provider: CalendarProvider,
    ) -> Result<ConnectOutcome, CalendarProblem> {
        self.process_cleanup(provider).await?;
        let client = self.client(provider).await?;
        let endpoints = provider.endpoints();
        let receiver = LoopbackReceiver::bind()
            .await
            .map_err(|e| classify_redirect(&e))?;
        let pending =
            begin_authorization(&endpoints, &client, &receiver.redirect_uri(CALLBACK_PATH))
                .map_err(|e| classify(&e, provider, false))?;
        let (send, mut cancel) = tokio::sync::oneshot::channel();
        {
            let mut state = self.control(provider).state();
            state.cancel = Some(send);
            state.pending_sign_in = Some(PendingSignIn {
                authorize_url: pending.authorize_url.clone(),
                browser_opened: true,
            });
        }
        let browser_opened = (self.browser_opener)(&pending.authorize_url).is_ok();
        if let Some(pending) = &mut self.control(provider).state().pending_sign_in {
            pending.browser_opened = browser_opened;
        }
        if !browser_opened {
            tracing::warn!(
                "could not open the calendar sign-in browser; waiting for manual sign-in"
            );
        }
        let callback = tokio::select! {
            biased;
            _ = &mut cancel => return Err(CalendarProblem::Cancelled),
            callback = receiver.receive(REDIRECT_WAIT) => callback.map_err(|e| classify_redirect(&e))?,
        };
        self.control(provider).state().pending_sign_in = None;
        let sender = NetworkPolicySender::new(&self.policy);
        let tokens = tokio::select! {
            biased;
            _ = &mut cancel => return Err(CalendarProblem::Cancelled),
            tokens = complete_authorization(&pending, &callback, &endpoints, &client, &sender) =>
                tokens.map_err(|e| classify(&e, provider, false))?,
        };
        let discovery = async {
            match provider {
                CalendarProvider::Google => {
                    let list = google::fetch_calendar_list(&self.policy, &tokens.access_token)
                        .await
                        .map_err(|e| classify_sync(&e))?;
                    let email = google::primary_email(&list)
                        .ok_or(CalendarProblem::InvalidResponse)?
                        .to_owned();
                    Ok((email, GrantCalendars::Google(list)))
                }
                CalendarProvider::Microsoft => {
                    let list = microsoft::fetch_calendars(&self.policy, &tokens.access_token)
                        .await
                        .map_err(|e| classify_sync(&e))?;
                    let email = microsoft::account_owner_email(&list)
                        .ok_or(CalendarProblem::InvalidResponse)?
                        .to_owned();
                    Ok((email, GrantCalendars::Microsoft(list)))
                }
            }
        };
        let (email, calendars) = tokio::select! {
            biased;
            _ = &mut cancel => return Err(CalendarProblem::Cancelled),
            result = discovery => result?,
        };
        {
            // Close cancellation atomically with checking for an already accepted
            // request. From here the local grant commit must finish its undo work.
            let mut state = self.control(provider).state();
            state.cancel.take();
            if cancel.try_recv().is_ok() {
                return Err(CalendarProblem::Cancelled);
            }
        }
        self.install_grant(provider, &email, &tokens, client.client_id(), &calendars)
            .await
    }

    fn cancel_connect(&self, provider: CalendarProvider) {
        if let Some(cancel) = self.control(provider).state().cancel.take() {
            let _ = cancel.send(());
        }
    }

    async fn cleanup_refs(
        &self,
        provider: CalendarProvider,
    ) -> Result<Vec<String>, CalendarProblem> {
        settings::get_or(self.db.pool(), cleanup_key(provider), Vec::<String>::new())
            .await
            .map_err(|_| CalendarProblem::Storage)
    }

    /// Retain failed deletions durably. A live row always wins over a stale cleanup
    /// entry, including a reconnect reusing a slot whose earlier deletion failed.
    /// Callers hold the provider guard throughout the read/delete/write sequence.
    async fn process_cleanup(&self, provider: CalendarProvider) -> Result<(), CalendarProblem> {
        let refs = self.cleanup_refs(provider).await?;
        if refs.is_empty() {
            return Ok(());
        }
        let rows = tokens_repo::list(self.db.pool(), provider.kind())
            .await
            .map_err(|_| CalendarProblem::Storage)?;
        let remaining: Vec<String> = refs
            .into_iter()
            .filter(|name| {
                if rows.iter().any(|(_, row)| row.token_ref == *name) {
                    return false;
                }
                let Some(key) = KeyRef::new(name) else {
                    return true;
                };
                !matches!(
                    self.credentials.delete(&key),
                    Ok(()) | Err(kw_security::CredentialError::NotFound(_))
                )
            })
            .collect();
        settings::set(self.db.pool(), cleanup_key(provider), &remaining)
            .await
            .map_err(|_| CalendarProblem::Storage)?;
        if !remaining.is_empty() {
            tracing::warn!(
                provider = provider.wire_str(),
                count = remaining.len(),
                "calendar credential cleanup remains pending"
            );
        }
        Ok(())
    }

    /// The sole grant commit. All metadata and cleanup intentions become visible
    /// together; credential-store undo never touches the old account's slot.
    async fn install_grant(
        &self,
        provider: CalendarProvider,
        email: &str,
        tokens: &kw_calendar_core::TokenSet,
        client_id: &str,
        calendars: &GrantCalendars,
    ) -> Result<ConnectOutcome, CalendarProblem> {
        let previous = self
            .read_previous_account(provider)
            .await
            .map_err(|_| CalendarProblem::Storage)?;
        let new_row = tokens_repo::read(self.db.pool(), provider.kind(), email)
            .await
            .map_err(|_| CalendarProblem::Storage)?;
        let old_row = if let Some(previous) = &previous {
            tokens_repo::read(self.db.pool(), provider.kind(), previous)
                .await
                .map_err(|_| CalendarProblem::Storage)?
        } else {
            None
        };
        let slot =
            credentials::token_ref_for(provider.kind(), email).ok_or(CalendarProblem::Storage)?;
        let same_slot = new_row.as_ref().is_some_and(|r| r.token_ref == slot.slot())
            || old_row.as_ref().is_some_and(|r| r.token_ref == slot.slot());
        let restore = if same_slot {
            match self.credentials.get(&slot) {
                Ok(secret) => Some(secret),
                Err(kw_security::CredentialError::NotFound(_)) => None,
                Err(_) => return Err(CalendarProblem::Storage),
            }
        } else {
            None
        };
        let mut cleanup = self.cleanup_refs(provider).await?;
        for row in new_row.iter().chain(old_row.iter()) {
            if row.token_ref != slot.slot() && !cleanup.contains(&row.token_ref) {
                cleanup.push(row.token_ref.clone());
            }
        }
        cleanup.retain(|name| name != slot.slot());
        let secret = credentials::grant_secret(tokens, client_id)
            .map_err(|e| classify(&e, provider, false))?;
        self.credentials
            .set(&slot, &secret)
            .map_err(|_| CalendarProblem::Storage)?;
        let commit = async {
            let mut tx = self
                .db
                .pool()
                .begin()
                .await
                .map_err(|_| CalendarProblem::Storage)?;
            tokens_repo::upsert_on(
                &mut *tx,
                provider.kind(),
                email,
                slot.slot(),
                tokens.expires_at,
                &tokens.scope,
            )
            .await
            .map_err(|_| CalendarProblem::Storage)?;
            // Purge before an overlapping calendar id can move to the new owner.
            if let Some(previous) = previous.as_deref().filter(|old| *old != email) {
                events_repo::purge_account_on(&mut *tx, provider.kind(), previous)
                    .await
                    .map_err(|_| CalendarProblem::Storage)?;
                calendars_repo::prune_calendars_on(&mut *tx, provider.kind(), previous, &[])
                    .await
                    .map_err(|_| CalendarProblem::Storage)?;
                tokens_repo::delete_on(&mut *tx, provider.kind(), previous)
                    .await
                    .map_err(|_| CalendarProblem::Storage)?;
            }
            let mut keep = Vec::new();
            match calendars {
                GrantCalendars::Google(list) => {
                    for entry in list {
                        calendars_repo::upsert_google_calendar_on(&mut *tx, email, entry)
                            .await
                            .map_err(|_| CalendarProblem::Storage)?;
                        keep.push(entry.id.clone());
                    }
                }
                GrantCalendars::Microsoft(list) => {
                    for entry in list {
                        calendars_repo::upsert_microsoft_calendar_on(&mut *tx, email, entry)
                            .await
                            .map_err(|_| CalendarProblem::Storage)?;
                        keep.push(entry.id.clone());
                    }
                }
            }
            calendars_repo::prune_calendars_on(&mut *tx, provider.kind(), email, &keep)
                .await
                .map_err(|_| CalendarProblem::Storage)?;
            events_repo::purge_account_on(&mut *tx, provider.kind(), email)
                .await
                .map_err(|_| CalendarProblem::Storage)?;
            settings::set_on(&mut *tx, account_key(provider), &email)
                .await
                .map_err(|_| CalendarProblem::Storage)?;
            settings::remove_on(&mut *tx, last_sync_key(provider))
                .await
                .map_err(|_| CalendarProblem::Storage)?;
            settings::set_on(&mut *tx, cleanup_key(provider), &cleanup)
                .await
                .map_err(|_| CalendarProblem::Storage)?;
            tx.commit().await.map_err(|_| CalendarProblem::Storage)?;
            Ok::<_, CalendarProblem>(keep.len())
        }
        .await;
        let count = match commit {
            Ok(count) => count,
            Err(problem) => {
                let restored = same_slot
                    && restore
                        .as_ref()
                        .is_some_and(|old| self.credentials.set(&slot, old).is_ok());
                if !restored {
                    let deleted = matches!(
                        self.credentials.delete(&slot),
                        Ok(()) | Err(kw_security::CredentialError::NotFound(_))
                    );
                    if !deleted {
                        // The transaction rolled back: reread its original list rather
                        // than publishing any of the failed replacement's deletions.
                        let queued = async {
                            let mut refs = self.cleanup_refs(provider).await?;
                            if !refs.iter().any(|name| name == slot.slot()) {
                                refs.push(slot.slot().to_owned());
                            }
                            settings::set(self.db.pool(), cleanup_key(provider), &refs)
                                .await
                                .map_err(|_| CalendarProblem::Storage)
                        }
                        .await;
                        if queued.is_err() {
                            tracing::warn!(
                                slot = slot.slot(),
                                "could not delete or queue the uncommitted calendar credential"
                            );
                        }
                    }
                    if same_slot && previous.as_deref() == Some(email) {
                        tracing::warn!("the previous calendar credential could not be restored; reconnection is required");
                        return Err(CalendarProblem::ReconnectRequired);
                    }
                }
                return Err(problem);
            }
        };
        self.process_cleanup(provider).await?;
        Ok(ConnectOutcome::Connected {
            account_email: email.to_owned(),
            calendars: u32::try_from(count).unwrap_or(u32::MAX),
        })
    }

    /// Disconnect only the account named by the setting, never leftover rows.
    /// Queue the row's actual slot in the same commit that stops referencing it.
    pub async fn disconnect(&self, provider: CalendarProvider) -> CmdResult<()> {
        let _notify = self.notify_on_completion();
        let _guard = self.control(provider).acquire(CalendarOperation::Idle)?;
        let Some(email) = self.read_previous_account(provider).await? else {
            return Ok(());
        };
        self.process_cleanup(provider)
            .await
            .map_err(problem_message)?;
        self.remove_account(provider, &email, true)
            .await
            .map_err(problem_message)
    }

    async fn remove_leftover(&self, provider: CalendarProvider, email: &str) -> CmdResult<()> {
        let _notify = self.notify_on_completion();
        let _guard = self.control(provider).acquire(CalendarOperation::Idle)?;
        if self.read_previous_account(provider).await?.as_deref() == Some(email) {
            return Err("Use Disconnect to remove the connected account.".to_owned());
        }
        self.process_cleanup(provider)
            .await
            .map_err(problem_message)?;
        self.remove_account(provider, email, false)
            .await
            .map_err(problem_message)
    }

    async fn remove_account(
        &self,
        provider: CalendarProvider,
        email: &str,
        connected: bool,
    ) -> Result<(), CalendarProblem> {
        let row = tokens_repo::read(self.db.pool(), provider.kind(), email)
            .await
            .map_err(|_| CalendarProblem::Storage)?;
        if row.is_none() && !connected {
            return Ok(());
        }
        let mut cleanup = self.cleanup_refs(provider).await?;
        if let Some(row) = row {
            if !cleanup.contains(&row.token_ref) {
                cleanup.push(row.token_ref);
            }
        }
        if !connected {
            if let Some(active) = self
                .read_previous_account(provider)
                .await
                .map_err(|_| CalendarProblem::Storage)?
            {
                if let Some(row) = tokens_repo::read(self.db.pool(), provider.kind(), &active)
                    .await
                    .map_err(|_| CalendarProblem::Storage)?
                {
                    // Old aliases can share a native slot. Removing an alias does
                    // not stop the active row referencing it, so never queue it.
                    cleanup.retain(|name| name != &row.token_ref);
                }
            }
        }
        let mut tx = self
            .db
            .pool()
            .begin()
            .await
            .map_err(|_| CalendarProblem::Storage)?;
        tokens_repo::delete_on(&mut *tx, provider.kind(), email)
            .await
            .map_err(|_| CalendarProblem::Storage)?;
        events_repo::purge_account_on(&mut *tx, provider.kind(), email)
            .await
            .map_err(|_| CalendarProblem::Storage)?;
        calendars_repo::prune_calendars_on(&mut *tx, provider.kind(), email, &[])
            .await
            .map_err(|_| CalendarProblem::Storage)?;
        if connected {
            settings::remove_on(&mut *tx, account_key(provider))
                .await
                .map_err(|_| CalendarProblem::Storage)?;
            settings::remove_on(&mut *tx, last_sync_key(provider))
                .await
                .map_err(|_| CalendarProblem::Storage)?;
        }
        settings::set_on(&mut *tx, cleanup_key(provider), &cleanup)
            .await
            .map_err(|_| CalendarProblem::Storage)?;
        tx.commit().await.map_err(|_| CalendarProblem::Storage)?;
        self.process_cleanup(provider).await
    }

    /// Refresh the calendar list and every selected calendar's events for
    /// whichever account is connected for `provider`.
    pub async fn sync(&self, provider: CalendarProvider) -> CmdResult<SyncOutcome> {
        let _notify = self.notify_on_completion();
        let guard = self.control(provider).acquire(CalendarOperation::Syncing)?;
        self.sync_guarded(provider, guard).await
    }

    /// Both entry points hold the same non-waiting guard. Record the monotonic
    /// attempt before any fallible work, including cleanup or token lookup.
    async fn sync_guarded(
        &self,
        provider: CalendarProvider,
        _guard: OperationGuard<'_>,
    ) -> CmdResult<SyncOutcome> {
        *self.control(provider).attempt_clock() = Some(Instant::now());
        if let Err(problem) = self.process_cleanup(provider).await {
            return Ok(sync_failure(problem));
        }
        let email = match self.read_previous_account(provider).await {
            Ok(Some(email)) => email,
            Ok(None) => return Err(format!("no {} account is connected", provider.name())),
            Err(_) => return Ok(sync_failure(CalendarProblem::Storage)),
        };
        let result = async {
            let client = self.client(provider).await?;
            let sender = NetworkPolicySender::new(&self.policy);
            let access_token = ensure_fresh_access_token(
                self.db.pool(),
                self.credentials.as_ref(),
                &sender,
                &provider.endpoints(),
                &client,
                provider.kind(),
                &email,
            )
            .await
            .map_err(|e| classify(&e, provider, true))?;
            Ok::<_, CalendarProblem>(match provider {
                CalendarProvider::Google => self.sync_google(&email, &access_token).await,
                CalendarProvider::Microsoft => self.sync_microsoft(&email, &access_token).await,
            })
        }
        .await;
        let outcome = result.unwrap_or_else(sync_failure);
        Ok(self.record_sync(provider, &email, outcome).await)
    }

    /// Meetings opening is the only automatic trigger. Each provider makes its
    /// decision independently; holding the guard through the decision prevents
    /// two concurrent opens from both starting a request.
    async fn sync_if_stale(&self) -> CmdResult<Vec<ProviderSync>> {
        let mut results = Vec::new();
        for provider in PROVIDERS {
            let (freshness, _) = provider_freshness(&self.db, provider).await?;
            let skipped = if !freshness.connected {
                Some(SyncSkip::NotConnected)
            } else if freshness.selected_calendars == 0 {
                Some(SyncSkip::NoSelectedCalendars)
            } else if let Some(guard) = self
                .control(provider)
                .try_acquire(CalendarOperation::Syncing)
            {
                let elapsed = self
                    .control(provider)
                    .attempt_clock()
                    .as_ref()
                    .map(Instant::elapsed);
                let persisted = freshness.last_attempt_at.as_deref().and_then(parse_instant);
                if is_fresh(elapsed, persisted, Utc::now()) {
                    Some(SyncSkip::Fresh)
                } else {
                    let _notify = self.notify_on_completion();
                    let outcome = self.sync_guarded(provider, guard).await?;
                    results.push(ProviderSync {
                        provider,
                        outcome: Some(outcome),
                        skipped: None,
                    });
                    continue;
                }
            } else {
                Some(SyncSkip::Busy)
            };
            results.push(ProviderSync {
                provider,
                outcome: None,
                skipped,
            });
        }
        Ok(results)
    }

    async fn sync_google(&self, email: &str, access_token: &Secret) -> SyncOutcome {
        if let Err(message) = self.prepare_snapshot(CalendarProvider::Google, email).await {
            return SyncOutcome::Failed {
                message,
                problem: Some(CalendarProblem::Storage),
            };
        }
        let (start, end) = snapshot_window();
        let calendars = match google::fetch_calendar_list(&self.policy, access_token).await {
            Ok(list) => list,
            Err(err) => {
                return SyncOutcome::Failed {
                    problem: Some(classify_sync(&err)),
                    message: problem_message(classify_sync(&err)),
                }
            }
        };
        for entry in &calendars {
            if let Err(err) =
                calendars_repo::upsert_google_calendar(self.db.pool(), email, entry).await
            {
                return SyncOutcome::Failed {
                    problem: Some(CalendarProblem::Storage),
                    message: fail("could not save the calendar list", err),
                };
            }
        }
        let keep_ids: Vec<String> = calendars.iter().map(|c| c.id.clone()).collect();
        if let Err(err) = calendars_repo::prune_calendars(
            self.db.pool(),
            CalendarProviderKind::Google,
            email,
            &keep_ids,
        )
        .await
        {
            // A prune failure must not report Completed while a calendar the
            // provider no longer lists is still sitting here locally.
            return SyncOutcome::Failed {
                problem: Some(CalendarProblem::Storage),
                message: fail("could not remove calendars no longer listed", err),
            };
        }

        let rows = match calendars_repo::list_calendars(
            self.db.pool(),
            CalendarProviderKind::Google,
            email,
        )
        .await
        {
            Ok(rows) => rows,
            Err(err) => {
                return SyncOutcome::Failed {
                    problem: Some(CalendarProblem::Storage),
                    message: fail("could not read the calendar list", err),
                }
            }
        };

        sync_selected_calendars(rows, |calendar_id| async move {
            self.sync_google_events(&calendar_id, access_token, &start, &end)
                .await
        })
        .await
    }

    async fn sync_google_events(
        &self,
        calendar_id: &str,
        access_token: &Secret,
        start: &chrono::DateTime<chrono::Utc>,
        end: &chrono::DateTime<chrono::Utc>,
    ) -> Result<(u32, u32, usize), CalendarProblem> {
        let snapshot = google::fetch_window(&self.policy, access_token, calendar_id, start, end)
            .await
            .map_err(|e| classify_sync(&e))?;
        self.commit_snapshot(CalendarProvider::Google, calendar_id, start, end, snapshot)
            .await
    }

    async fn sync_microsoft(&self, email: &str, access_token: &Secret) -> SyncOutcome {
        if let Err(message) = self
            .prepare_snapshot(CalendarProvider::Microsoft, email)
            .await
        {
            return SyncOutcome::Failed {
                message,
                problem: Some(CalendarProblem::Storage),
            };
        }
        let (start, end) = snapshot_window();
        let calendars = match microsoft::fetch_calendars(&self.policy, access_token).await {
            Ok(list) => list,
            Err(err) => {
                return SyncOutcome::Failed {
                    problem: Some(classify_sync(&err)),
                    message: problem_message(classify_sync(&err)),
                }
            }
        };
        for entry in &calendars {
            if let Err(err) =
                calendars_repo::upsert_microsoft_calendar(self.db.pool(), email, entry).await
            {
                return SyncOutcome::Failed {
                    problem: Some(CalendarProblem::Storage),
                    message: fail("could not save the calendar list", err),
                };
            }
        }
        let keep_ids: Vec<String> = calendars.iter().map(|c| c.id.clone()).collect();
        if let Err(err) = calendars_repo::prune_calendars(
            self.db.pool(),
            CalendarProviderKind::Microsoft,
            email,
            &keep_ids,
        )
        .await
        {
            // A prune failure must not report Completed while a calendar the
            // provider no longer lists is still sitting here locally.
            return SyncOutcome::Failed {
                problem: Some(CalendarProblem::Storage),
                message: fail("could not remove calendars no longer listed", err),
            };
        }

        let rows = match calendars_repo::list_calendars(
            self.db.pool(),
            CalendarProviderKind::Microsoft,
            email,
        )
        .await
        {
            Ok(rows) => rows,
            Err(err) => {
                return SyncOutcome::Failed {
                    problem: Some(CalendarProblem::Storage),
                    message: fail("could not read the calendar list", err),
                }
            }
        };

        sync_selected_calendars(rows, |calendar_id| async move {
            self.sync_microsoft_events(&calendar_id, access_token, &start, &end)
                .await
        })
        .await
    }

    async fn sync_microsoft_events(
        &self,
        calendar_id: &str,
        access_token: &Secret,
        start: &chrono::DateTime<chrono::Utc>,
        end: &chrono::DateTime<chrono::Utc>,
    ) -> Result<(u32, u32, usize), CalendarProblem> {
        let snapshot = microsoft::fetch_window(&self.policy, access_token, calendar_id, start, end)
            .await
            .map_err(|e| classify_sync(&e))?;
        self.commit_snapshot(
            CalendarProvider::Microsoft,
            calendar_id,
            start,
            end,
            snapshot,
        )
        .await
    }

    /// Apply the version gate before even fetching the calendar list. Purging
    /// includes unselected calendars and events outside the new window: those
    /// rows were written before UTC and decline filtering were guaranteed.
    /// A database read failure is treated as missing history so the gate can
    /// repair the bookkeeping record; only this account's history survives a
    /// version-zero upgrade. Undecodable settings already read as `Ok(None)`.
    async fn prepare_snapshot(&self, provider: CalendarProvider, email: &str) -> CmdResult<()> {
        let key = last_sync_key(provider);
        let previous = match settings::get::<SyncRecord>(self.db.pool(), key).await {
            Ok(record) => record,
            Err(_) => {
                tracing::warn!(key, "could not read the previous calendar sync result");
                None
            }
        };
        if previous
            .as_ref()
            .is_some_and(|r| r.snapshot_version == 1 && r.account.as_deref() == Some(email))
        {
            return Ok(());
        }
        let mut record = previous
            .filter(|r| r.account.as_deref() == Some(email))
            .unwrap_or_else(|| SyncRecord {
                at: chrono::Utc::now().to_rfc3339(),
                outcome: SyncOutcome::Failed {
                    problem: None,
                    message: "The first sync after an update did not finish.".to_owned(),
                },
                account: None,
                snapshot_version: 0,
                last_success_at: None,
            });
        record.account = Some(email.to_owned());
        record.snapshot_version = 1;
        let mut tx = self
            .db
            .pool()
            .begin()
            .await
            .map_err(|e| fail("could not start cache purge", e))?;
        events_repo::purge_account_on(&mut *tx, provider.kind(), email)
            .await
            .map_err(|e| fail("could not purge the old calendar cache", e))?;
        settings::set_on(&mut *tx, last_sync_key(provider), &record)
            .await
            .map_err(|e| fail("could not record the snapshot version", e))?;
        tx.commit()
            .await
            .map_err(|e| fail("could not commit cache purge", e))?;
        Ok(())
    }

    /// Fetching is complete before entering this method. Each calendar commits
    /// independently so another calendar's network failure cannot undo it.
    async fn commit_snapshot(
        &self,
        provider: CalendarProvider,
        calendar_id: &str,
        start: &chrono::DateTime<chrono::Utc>,
        end: &chrono::DateTime<chrono::Utc>,
        snapshot: kw_calendar_core::sync_support::WindowSnapshot,
    ) -> Result<(u32, u32, usize), CalendarProblem> {
        let removed = events_repo::replace_window(
            self.db.pool(),
            provider.wire_str(),
            calendar_id,
            start,
            end,
            &snapshot.rows,
            &snapshot.keep_ids,
        )
        .await
        .map_err(|_| CalendarProblem::Storage)?;
        calendars_repo::set_sync_token(self.db.pool(), provider.kind(), calendar_id, None)
            .await
            .map_err(|_| CalendarProblem::Storage)?;
        Ok((
            u32::try_from(snapshot.rows.len()).unwrap_or(u32::MAX),
            u32::try_from(removed).unwrap_or(u32::MAX),
            snapshot.keep_ids.len(),
        ))
    }

    async fn record_sync(
        &self,
        provider: CalendarProvider,
        email: &str,
        outcome: SyncOutcome,
    ) -> SyncOutcome {
        let key = last_sync_key(provider);
        let previous = match settings::get::<SyncRecord>(self.db.pool(), key).await {
            Ok(record) => record,
            Err(_) => {
                // A database read failure is missing bookkeeping history; the
                // new sync result is still worth writing without it.
                tracing::warn!(key, "could not read the previous calendar sync result");
                None
            }
        }
        .filter(|r| r.account.as_deref() == Some(email));
        let now = chrono::Utc::now().to_rfc3339();
        let full_success = matches!(&outcome, SyncOutcome::Completed { calendar_errors, .. } if calendar_errors.is_empty());
        // A failed token attempt for a new account must not relabel another
        // account's version-one record and accidentally bypass the purge, or
        // inherit that account's last successful sync.
        let version = previous.as_ref().map_or(0, |r| r.snapshot_version);
        let record = SyncRecord {
            at: now.clone(),
            outcome: outcome.clone(),
            account: Some(email.to_owned()),
            snapshot_version: version,
            last_success_at: if full_success {
                Some(now)
            } else {
                previous.and_then(|r| r.last_success_at)
            },
        };
        if let Err(err) = settings::set(self.db.pool(), last_sync_key(provider), &record).await {
            tracing::warn!(error = %err, "could not record the calendar sync result");
            return sync_failure(CalendarProblem::Storage);
        }
        outcome
    }
}

/// Keep partial success accounting identical for both providers. The callback
/// returns only after a whole calendar snapshot committed, so an incomplete
/// fetch contributes an error and cannot inflate either event counter.
async fn sync_selected_calendars<F, Fut>(
    rows: Vec<calendars_repo::CalendarRow>,
    mut sync: F,
) -> SyncOutcome
where
    F: FnMut(String) -> Fut,
    Fut: std::future::Future<Output = Result<(u32, u32, usize), CalendarProblem>>,
{
    let mut upserted = 0;
    let mut removed = 0;
    let mut errors = Vec::new();
    for row in rows.into_iter().filter(|r| r.is_selected) {
        match sync(row.id).await {
            Ok((u, r, unreadable)) => {
                upserted += u;
                removed += r;
                if unreadable > 0 {
                    errors.push(time_error(&row.summary, unreadable));
                }
            }
            Err(problem) => errors.push(format!("{}: {}", row.summary, problem_message(problem))),
        }
    }
    SyncOutcome::Completed {
        events_upserted: upserted,
        events_removed: removed,
        calendar_errors: errors,
    }
}

/// Freeze bounds once per sync, avoiding moving edges between calendars/pages.
fn snapshot_window() -> (chrono::DateTime<chrono::Utc>, chrono::DateTime<chrono::Utc>) {
    let now = chrono::Utc::now();
    (
        now - chrono::Duration::hours(24),
        now + chrono::Duration::days(30),
    )
}

fn time_error(calendar: &str, count: usize) -> String {
    let noun = if count == 1 { "event" } else { "events" };
    format!("{count} {noun} in {calendar} had a time that could not be read")
}

/* --------------------------------------------------------------- the wire */

/// What happened when someone clicked "Connect".
///
/// A held operation guard is the only command-level refusal. Every consent,
/// transport, provider and persistence failure has a stable classified outcome.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(
    tag = "state",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum ConnectOutcome {
    Connected {
        account_email: String,
        calendars: u32,
    },
    Denied {
        reason: String,
    },
    Failed {
        problem: CalendarProblem,
    },
}

/// What one sync attempt found. Both the command's own return value and
/// what [`CalendarRuntime::record_sync`] persists to settings use this shape.
/// If recording fails, the command reports Storage and the previous durable
/// record remains available; a successful fetch must not hide that failure.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(
    tag = "state",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum SyncOutcome {
    Completed {
        events_upserted: u32,
        events_removed: u32,
        /// Per-calendar failures within an otherwise-completed sync -- a
        /// calendar list refresh can succeed while one calendar's events
        /// fail, and reporting only the aggregate would hide which one.
        calendar_errors: Vec<String>,
    },
    /// The grant needs reconnection, including a rejected refresh grant or a
    /// changed client binding. A transport failure belongs to Failed instead:
    /// retrying the network is sufficient and need not discard the connection.
    TokenUnavailable {
        #[serde(default)]
        problem: Option<CalendarProblem>,
        message: String,
    },
    Failed {
        #[serde(default)]
        problem: Option<CalendarProblem>,
        message: String,
    },
}

/// A sync outcome together with when it happened, persisted to settings so
/// [`calendar_status`] can report it without a network call.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SyncRecord {
    /// RFC 3339.
    pub at: String,
    pub outcome: SyncOutcome,
    #[serde(default)]
    pub account: Option<String>,
    #[serde(default)]
    pub snapshot_version: u32,
    /// Most recent complete success for this account; partial attempts for the
    /// same account preserve this timestamp.
    #[serde(default)]
    pub last_success_at: Option<String>,
}

/// Whether a token is on file and, if so, whether it is still within its
/// recorded expiry.
///
/// A local read, not a network probe -- reaching the network on every status
/// poll is exactly what `docs/NETWORK_BOUNDARY.md` rules out for a passive
/// status command. `Missing` covers both "never connected" and "the
/// credential is gone even though the database row is still there", which is
/// also possible after a credential-store reset. Disconnect removes its row.
/// Checking the credential
/// store directly, rather than trusting the row's mere existence, is what
/// keeps that decision from ever reading back as "connected".
///
/// `RefreshFailed` exists because `expires_at` alone cannot distinguish "past
/// its expiry, never tried since" from "past its expiry, and the most recent
/// attempt to refresh it already failed" -- see [`combine_token_status`].
/// Reporting the second case as plain `NeedsRefresh`, the way this type used
/// to before a token could carry a known, recorded failure, is what let
/// `calendar_status` say "Connected -- the next sync refreshes it
/// automatically" in the same payload as a `SyncOutcome::TokenUnavailable`
/// telling the user to reconnect.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(
    tag = "state",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum TokenStatus {
    Active,
    NeedsRefresh,
    Missing,
    RefreshFailed { message: String },
    ReconnectRequired,
}

async fn token_status(
    db: &Db,
    credentials: &dyn CredentialStore,
    kind: CalendarProviderKind,
    email: &str,
) -> CmdResult<TokenStatus> {
    let Some(row) = tokens_repo::read(db.pool(), kind, email)
        .await
        .map_err(|e| fail("could not read the stored token", e))?
    else {
        return Ok(TokenStatus::Missing);
    };
    let Some(key_ref) = KeyRef::new(&row.token_ref) else {
        return Ok(TokenStatus::Missing);
    };
    if !credentials.contains(&key_ref) {
        return Ok(TokenStatus::Missing);
    }
    let provider = match kind {
        CalendarProviderKind::Google => CalendarProvider::Google,
        CalendarProviderKind::Microsoft => CalendarProvider::Microsoft,
    };
    let custom = settings::get::<String>(db.pool(), client_id_key(provider))
        .await
        .map_err(|_| problem_message(CalendarProblem::Storage))?;
    let choice = calendar_clients::resolve(kind, custom.as_deref(), None);
    let expected = match &choice {
        ClientChoice::AppOwned(client) | ClientChoice::Custom(client) => Some(client.client_id()),
        ClientChoice::Missing => None,
    };
    let binding = match credentials::bundle_client_id(credentials, &key_ref) {
        Ok(binding) => binding,
        Err(CalendarError::Credential(kw_security::CredentialError::NotFound(_))) => {
            return Ok(TokenStatus::Missing)
        }
        Err(CalendarError::Bundle(_)) => return Ok(TokenStatus::ReconnectRequired),
        Err(_) => return Err(problem_message(CalendarProblem::Storage)),
    };
    if binding.is_none() || binding.as_deref() != expected {
        return Ok(TokenStatus::ReconnectRequired);
    }
    Ok(if row.expires_at > now_epoch() {
        TokenStatus::Active
    } else {
        TokenStatus::NeedsRefresh
    })
}

/// Fold the most recent recorded sync's own attempt to resolve this token
/// into what [`token_status`] derived from the row and the credential store
/// alone.
///
/// Pure, taking already-computed inputs, so the decision is testable without
/// a database or a credential store -- `token_status`'s own async I/O is a
/// separate concern from this one.
///
/// Only `NeedsRefresh` is ever downgraded. `Active` means the cached access
/// token is not due to expire yet, which can only be true because a refresh
/// succeeded (or was never needed) more recently than any recorded failure
/// -- `ensure_fresh_access_token` never advances `expires_at` on a failed
/// attempt, so `Active` and a *current* recorded `TokenUnavailable` cannot
/// both be true at once. `Missing` already says everything there is to say.
///
/// A `NeedsRefresh` paired with a recorded `TokenUnavailable` is not merely
/// "expired, untested" -- it is definitively broken *right now*, because the
/// only way the row's `expires_at` could have moved back into the future
/// since that recorded failure is a refresh that succeeded, which would have
/// produced `Active` instead. Any other recorded outcome (`Completed`,
/// `Failed` for a non-token reason, or none at all) leaves `NeedsRefresh`
/// exactly as `token_status` reported it: expired, but not known to be
/// broken.
fn combine_token_status(row_status: TokenStatus, last_sync: Option<&SyncRecord>) -> TokenStatus {
    if row_status != TokenStatus::NeedsRefresh {
        return row_status;
    }
    match last_sync.map(|record| &record.outcome) {
        Some(SyncOutcome::TokenUnavailable { message, .. }) => TokenStatus::RefreshFailed {
            message: message.clone(),
        },
        _ => TokenStatus::NeedsRefresh,
    }
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct CalendarSummary {
    pub id: String,
    pub summary: String,
    pub is_primary: bool,
    pub is_selected: bool,
    /// What sync actually wrote for this calendar, not the provider's own
    /// total -- see `kw_calendar_core::events_repo::count_events`.
    pub event_count: i64,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AccountStatus {
    pub email: String,
    pub token: TokenStatus,
    pub calendars: Vec<CalendarSummary>,
    pub last_sync: Option<SyncRecord>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ProviderStatus {
    /// Whether an effective client exists, from either a custom ID or this build.
    pub client_id_configured: bool,
    /// The saved custom client ID, shown back because a client ID is not a secret.
    pub custom_client_id: Option<String>,
    /// Whether the effective client comes from a custom override, this app, or is missing.
    pub client_source: ClientSource,
    /// Whether the credential store contains a custom Google secret; false for Microsoft.
    pub custom_google_secret_configured: bool,
    /// Whether this build provides an app-owned client for the provider.
    pub app_client_available: bool,
    pub account: Option<AccountStatus>,
    pub operation: CalendarOperation,
    pub pending_sign_in: Option<PendingSignIn>,
    pub cleanup_pending: u32,
    pub leftover: Vec<LeftoverConnection>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct CalendarStatus {
    pub google: ProviderStatus,
    pub microsoft: ProviderStatus,
    pub apple: kw_calendar_core::AppleCalendarSupport,
}

async fn provider_status(
    db: &Db,
    credentials: &dyn CredentialStore,
    provider: CalendarProvider,
) -> CmdResult<ProviderStatus> {
    let custom_client_id = settings::get::<String>(db.pool(), client_id_key(provider))
        .await
        .map_err(|e| fail("could not read the client ID", e))?;
    let custom_google_secret_configured =
        provider == CalendarProvider::Google && credentials.contains(&custom_secret_slot()?);
    let client_source =
        calendar_clients::resolve(provider.kind(), custom_client_id.as_deref(), None).source();
    let app_client_available =
        calendar_clients::resolve(provider.kind(), None, None).source() == ClientSource::App;
    let client_id_configured = client_source != ClientSource::Missing;

    let account_email = settings::get::<String>(db.pool(), account_key(provider))
        .await
        .map_err(|e| fail("could not read the connected account", e))?;

    let leftover = tokens_repo::list(db.pool(), provider.kind())
        .await
        .map_err(|_| problem_message(CalendarProblem::Storage))?
        .into_iter()
        .filter(|(email, _)| account_email.as_ref() != Some(email))
        .map(|(email, row)| {
            let positional = row
                .token_ref
                .strip_prefix(&format!("calendar-{}-", provider.wire_str()))
                .is_some_and(|suffix| {
                    !suffix.is_empty() && suffix.bytes().all(|b| b.is_ascii_digit())
                });
            LeftoverConnection {
                email,
                origin: if positional {
                    LeftoverOrigin::Openwhispr
                } else {
                    LeftoverOrigin::Earlier
                },
            }
        })
        .collect();
    let cleanup: Vec<String> = settings::get_or(db.pool(), cleanup_key(provider), Vec::new())
        .await
        .map_err(|_| problem_message(CalendarProblem::Storage))?;
    let account = match account_email {
        None => None,
        Some(email) => {
            let token = token_status(db, credentials, provider.kind(), &email).await?;
            let rows = calendars_repo::list_calendars(db.pool(), provider.kind(), &email)
                .await
                .map_err(|e| fail("could not read the calendar list", e))?;

            let mut calendars = Vec::with_capacity(rows.len());
            for row in rows {
                let event_count =
                    events_repo::count_events(db.pool(), provider.wire_str(), &row.id)
                        .await
                        .map_err(|e| fail("could not count events", e))?;
                calendars.push(CalendarSummary {
                    id: row.id,
                    summary: row.summary,
                    is_primary: row.is_primary,
                    is_selected: row.is_selected,
                    event_count,
                });
            }

            let last_sync = settings::get::<SyncRecord>(db.pool(), last_sync_key(provider))
                .await
                .map_err(|e| fail("could not read the last sync record", e))?;
            // Never let a bare `NeedsRefresh` read as "will refresh
            // automatically" once a sync has already tried that and
            // recorded that it failed -- see `combine_token_status`.
            let token = combine_token_status(token, last_sync.as_ref());

            Some(AccountStatus {
                email,
                token,
                calendars,
                last_sync,
            })
        }
    };

    Ok(ProviderStatus {
        operation: CalendarOperation::Idle,
        pending_sign_in: None,
        cleanup_pending: u32::try_from(cleanup.len()).unwrap_or(u32::MAX),
        leftover,
        client_id_configured,
        custom_client_id,
        client_source,
        custom_google_secret_configured,
        app_client_available,
        account,
    })
}

/* --------------------------------------------------------- opening a browser */

/// The program and arguments that open `url` in the system's default
/// browser, chosen per platform.
///
/// A hand-rolled mapping rather than a `shell`/`opener` Tauri plugin, for the
/// same reason `crates/audio-core::system_audio` reimplements loopback
/// capture on top of `cpal` instead of a new dependency: the actual need is
/// a handful of lines. On Windows this deliberately calls `rundll32
/// url.dll,FileProtocolHandler` rather than `cmd /c start` -- `cmd.exe`
/// re-parses its whole command line for shell metacharacters (`&`, `|`, ...)
/// regardless of how the arguments were quoted on the way in, and an OAuth
/// authorize URL is exactly the kind of string that contains `&` between its
/// own query parameters. `rundll32` calls the DLL export directly with the
/// URL as a literal argument, so there is no shell in the loop to
/// reinterpret it.
fn browser_command(url: &str) -> (&'static str, Vec<String>) {
    #[cfg(target_os = "windows")]
    {
        (
            "rundll32",
            vec!["url.dll,FileProtocolHandler".to_owned(), url.to_owned()],
        )
    }
    #[cfg(target_os = "macos")]
    {
        ("open", vec![url.to_owned()])
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        ("xdg-open", vec![url.to_owned()])
    }
}

pub(crate) fn open_in_browser(url: &str) -> std::io::Result<()> {
    let (program, args) = browser_command(url);
    std::process::Command::new(program).args(args).spawn()?;
    Ok(())
}

/// A small, local projection for the Meetings page. Times are copied exactly
/// from the normalised snapshot; presenting them in local time belongs to UI.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct UpcomingEvent {
    pub id: String,
    pub provider: CalendarProvider,
    pub calendar_id: String,
    pub title: Option<String>,
    pub start: String,
    pub end: String,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ProviderFreshness {
    pub provider: CalendarProvider,
    pub connected: bool,
    pub selected_calendars: u32,
    pub last_attempt_at: Option<String>,
    pub last_success_at: Option<String>,
    pub last_outcome: Option<SyncOutcome>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct UpcomingMeetings {
    pub events: Vec<UpcomingEvent>,
    pub has_more: bool,
    pub providers: Vec<ProviderFreshness>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct CandidateEvent {
    pub id: String,
    pub provider: CalendarProvider,
    pub calendar_id: String,
    pub title: Option<String>,
    pub start: String,
    pub end: String,
    pub meeting_like: bool,
}

#[derive(Debug, Clone, Serialize, TS)]
pub struct CalendarCandidates {
    pub events: Vec<CandidateEvent>,
    pub auto: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum SyncSkip {
    NotConnected,
    Fresh,
    Busy,
    NoSelectedCalendars,
}

#[derive(Debug, Clone, Serialize, TS)]
pub struct ProviderSync {
    pub provider: CalendarProvider,
    pub outcome: Option<SyncOutcome>,
    pub skipped: Option<SyncSkip>,
}

/// Internal recording-start input retains the whole event for participant and
/// link resolution. Explicit choices may include solo blocks; Auto may not.
#[derive(Debug, Clone)]
pub(crate) struct CalendarEventAt {
    pub provider: CalendarProvider,
    pub row: events_repo::EventRow,
    pub meeting_like: bool,
}

fn parse_instant(value: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|at| at.with_timezone(&Utc))
}

/// Take the later valid attempt from the monotonic and wall clocks. Ignoring a
/// future wall clock prevents an OS clock correction from suppressing syncs.
fn is_fresh(
    since_memory: Option<Duration>,
    persisted_at: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
) -> bool {
    since_memory.is_some_and(|elapsed| elapsed < Duration::from_secs(300))
        || persisted_at.is_some_and(|at| {
            at <= now && now.signed_duration_since(at) < chrono::Duration::seconds(300)
        })
}

/// The sole snapshot gate for every reader. Freshness still describes an
/// ungated account, but legacy/unbound or another account's rows never escape.
/// No credential lookup, runtime guard or network policy is needed here.
async fn provider_freshness(
    db: &Db,
    provider: CalendarProvider,
) -> CmdResult<(ProviderFreshness, Option<String>)> {
    let account = settings::get::<String>(db.pool(), account_key(provider))
        .await
        .map_err(|e| fail("could not read the calendar account", e))?;
    let record = settings::get::<SyncRecord>(db.pool(), last_sync_key(provider))
        .await
        .map_err(|e| fail("could not read calendar freshness", e))?
        .filter(|r| account.is_some() && r.account == account);
    let selected = if let Some(email) = account.as_deref() {
        calendars_repo::list_calendars(db.pool(), provider.kind(), email)
            .await
            .map_err(|e| fail("could not read calendar selection", e))?
            .iter()
            .filter(|c| c.is_selected)
            .count()
    } else {
        0
    };
    let gated_account = account
        .clone()
        .filter(|_| record.as_ref().is_some_and(|r| r.snapshot_version == 1));
    Ok((
        ProviderFreshness {
            provider,
            connected: account.is_some(),
            selected_calendars: u32::try_from(selected).unwrap_or(u32::MAX),
            last_attempt_at: record.as_ref().map(|r| r.at.clone()),
            last_success_at: record.as_ref().and_then(|r| r.last_success_at.clone()),
            last_outcome: record.map(|r| r.outcome),
        },
        gated_account,
    ))
}

/// Both projections share the same account/version gate and selected-calendar
/// repository read. Unreadable rows cannot participate in pure selection rules.
async fn calendar_window(
    db: &Db,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
) -> CmdResult<(Vec<events_repo::EventRow>, Vec<ProviderFreshness>)> {
    let mut rows = Vec::new();
    let mut providers = Vec::new();
    for provider in PROVIDERS {
        let (freshness, gated_account) = provider_freshness(db, provider).await?;
        if let Some(email) = gated_account {
            let (events, _) =
                events_repo::list_overlapping(db.pool(), provider.kind(), &email, from, to)
                    .await
                    .map_err(|e| fail("could not read calendar events", e))?;
            rows.extend(events);
        }
        providers.push(freshness);
    }
    Ok((rows, providers))
}

fn row_provider(row: &events_repo::EventRow) -> CalendarProvider {
    match row.provider {
        "microsoft" => CalendarProvider::Microsoft,
        _ => CalendarProvider::Google,
    }
}

impl From<events_repo::EventRow> for UpcomingEvent {
    fn from(row: events_repo::EventRow) -> Self {
        Self {
            provider: row_provider(&row),
            id: row.id,
            calendar_id: row.calendar_id,
            title: row.summary,
            start: row.start_time,
            end: row.end_time,
        }
    }
}

async fn upcoming_at(db: &Db, now: DateTime<Utc>, limit: u32) -> CmdResult<UpcomingMeetings> {
    if !(1..=20).contains(&limit) {
        return Err("calendar upcoming limit must be between 1 and 20".to_owned());
    }
    let horizon = now
        .checked_add_signed(chrono::Duration::days(7))
        .unwrap_or(DateTime::<Utc>::MAX_UTC);
    let (rows, providers) = calendar_window(db, now, horizon).await?;
    let (events, has_more) = meetings::upcoming(now, &rows, limit as usize);
    Ok(UpcomingMeetings {
        events: events.into_iter().map(UpcomingEvent::from).collect(),
        has_more,
        providers,
    })
}

/// Read the snapshot at the recording's actual start instant. The extra second
/// in the half-open repository window includes the inclusive ten-minute lead.
/// This function takes only a database and time: it cannot refresh a token or
/// wait for a provider operation, even while a network sync is in progress.
pub(crate) async fn calendar_events_at(
    db: &Db,
    at: DateTime<Utc>,
) -> CmdResult<Vec<CalendarEventAt>> {
    let end = at
        .checked_add_signed(chrono::Duration::seconds(601))
        .unwrap_or(DateTime::<Utc>::MAX_UTC);
    let (rows, _) = calendar_window(db, at, end).await?;
    Ok(meetings::time_matches(at, &rows)
        .into_iter()
        .map(|row| CalendarEventAt {
            provider: row_provider(&row),
            meeting_like: meetings::is_meeting_like(&row),
            row,
        })
        .collect())
}

async fn candidates_at(db: &Db, at: DateTime<Utc>) -> CmdResult<CalendarCandidates> {
    let matches = calendar_events_at(db, at).await?;
    let mut meeting_like = matches.iter().filter(|event| event.meeting_like);
    let auto = meeting_like
        .next()
        .filter(|_| meeting_like.next().is_none())
        .map(|event| event.row.id.clone());
    let events = matches
        .into_iter()
        .map(|event| CandidateEvent {
            provider: event.provider,
            id: event.row.id,
            calendar_id: event.row.calendar_id,
            title: event.row.summary,
            start: event.row.start_time,
            end: event.row.end_time,
            meeting_like: event.meeting_like,
        })
        .collect();
    Ok(CalendarCandidates { events, auto })
}

/* ------------------------------------------------------------------ commands */

/// A local-only Meetings read; opening the page chooses separately whether to sync.
#[tauri::command]
pub async fn calendar_upcoming(
    state: State<'_, crate::state::AppState>,
    limit: u32,
) -> CmdResult<UpcomingMeetings> {
    upcoming_at(state.db(), Utc::now(), limit).await
}

/// Recording Start offers cached choices without waiting for a network operation.
#[tauri::command]
pub async fn meeting_calendar_candidates(
    state: State<'_, crate::state::AppState>,
) -> CmdResult<CalendarCandidates> {
    candidates_at(state.db(), Utc::now()).await
}

#[tauri::command]
pub async fn calendar_sync_if_stale(
    runtime: State<'_, Arc<CalendarRuntime>>,
) -> CmdResult<Vec<ProviderSync>> {
    runtime.sync_if_stale().await
}

#[tauri::command]
pub async fn calendar_status(
    state: State<'_, crate::state::AppState>,
    runtime: State<'_, Arc<CalendarRuntime>>,
) -> CmdResult<CalendarStatus> {
    let mut google =
        provider_status(state.db(), state.credentials(), CalendarProvider::Google).await?;
    let mut microsoft =
        provider_status(state.db(), state.credentials(), CalendarProvider::Microsoft).await?;
    for (provider, status) in [
        (CalendarProvider::Google, &mut google),
        (CalendarProvider::Microsoft, &mut microsoft),
    ] {
        let state = runtime.control(provider).state();
        status.operation = state.operation;
        status.pending_sign_in = state.pending_sign_in.clone();
    }
    Ok(CalendarStatus {
        google,
        microsoft,
        apple: apple::support(),
    })
}

/// Keep the legacy command's argument names while applying the same safe
/// pair update and secret-clearing rule as the Advanced custom-client command.
#[tauri::command]
pub async fn calendar_set_client_id(
    runtime: State<'_, Arc<CalendarRuntime>>,
    provider: CalendarProvider,
    client_id: String,
) -> CmdResult<()> {
    let _notify = runtime.notify_on_completion();
    // Serialize client edits with connect and sync so a grant cannot be issued
    // while its client configuration is being replaced.
    let _guard = runtime.control(provider).acquire(CalendarOperation::Idle)?;
    let pool = runtime.db.pool();
    persist_custom_client(
        &runtime.db,
        runtime.credentials.as_ref(),
        provider,
        Some(client_id.as_str()),
        SecretChange::Keep,
        |id| async move {
            if let Some(id) = id {
                settings::set(pool, client_id_key(provider), &id).await
            } else {
                settings::remove(pool, client_id_key(provider)).await
            }
        },
    )
    .await
}

#[tauri::command]
pub async fn calendar_set_custom_client(
    runtime: State<'_, Arc<CalendarRuntime>>,
    provider: CalendarProvider,
    client_id: Option<String>,
    google_secret: SecretChange,
) -> CmdResult<()> {
    let _notify = runtime.notify_on_completion();
    // Serialize client edits with connect and sync so a grant cannot be issued
    // while its client configuration is being replaced.
    let _guard = runtime.control(provider).acquire(CalendarOperation::Idle)?;
    let pool = runtime.db.pool();
    persist_custom_client(
        &runtime.db,
        runtime.credentials.as_ref(),
        provider,
        client_id.as_deref(),
        google_secret,
        |id| async move {
            if let Some(id) = id {
                settings::set(pool, client_id_key(provider), &id).await
            } else {
                settings::remove(pool, client_id_key(provider)).await
            }
        },
    )
    .await
}

#[tauri::command]
pub async fn calendar_connect(
    runtime: State<'_, Arc<CalendarRuntime>>,
    provider: CalendarProvider,
) -> CmdResult<ConnectOutcome> {
    runtime.connect(provider).await
}

/// Disconnect whichever account is connected for `provider`. A no-op,
/// successfully, if none is.
#[tauri::command]
pub async fn calendar_disconnect(
    runtime: State<'_, Arc<CalendarRuntime>>,
    provider: CalendarProvider,
) -> CmdResult<()> {
    runtime.disconnect(provider).await
}

/// Cancellation never waits for the operation guard it is meant to release.
#[tauri::command]
pub async fn calendar_cancel_connect(
    runtime: State<'_, Arc<CalendarRuntime>>,
    provider: CalendarProvider,
) -> CmdResult<()> {
    runtime.cancel_connect(provider);
    Ok(())
}

#[tauri::command]
pub async fn calendar_remove_leftover(
    runtime: State<'_, Arc<CalendarRuntime>>,
    provider: CalendarProvider,
    email: String,
) -> CmdResult<()> {
    runtime.remove_leftover(provider, &email).await
}

#[tauri::command]
pub async fn calendar_retry_cleanup(
    runtime: State<'_, Arc<CalendarRuntime>>,
    provider: CalendarProvider,
) -> CmdResult<()> {
    let _notify = runtime.notify_on_completion();
    let _guard = runtime.control(provider).acquire(CalendarOperation::Idle)?;
    runtime
        .process_cleanup(provider)
        .await
        .map_err(problem_message)
}

#[tauri::command]
pub async fn calendar_sync(
    runtime: State<'_, Arc<CalendarRuntime>>,
    provider: CalendarProvider,
) -> CmdResult<SyncOutcome> {
    runtime.sync(provider).await
}

/// Toggle whether one calendar is synced and visible to local readers.
/// Selecting it permits its cached snapshot immediately; the next sync refreshes it.
#[tauri::command]
pub async fn calendar_set_selected(
    runtime: State<'_, Arc<CalendarRuntime>>,
    provider: CalendarProvider,
    calendar_id: String,
    selected: bool,
) -> CmdResult<()> {
    let _notify = runtime.notify_on_completion();
    let _guard = runtime.control(provider).acquire(CalendarOperation::Idle)?;
    calendars_repo::set_selected(runtime.db.pool(), provider.kind(), &calendar_id, selected)
        .await
        .map_err(|e| fail("could not save the calendar selection", e))
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

    fn at() -> DateTime<Utc> {
        parse_instant("2026-11-11T12:00:00Z").unwrap()
    }

    async fn reader_event(
        db: &Db,
        provider: CalendarProvider,
        calendar: &str,
        id: &str,
        start: DateTime<Utc>,
        solo: bool,
    ) -> String {
        let id = events_repo::row_id(provider.wire_str(), calendar, id);
        events_repo::upsert_event(
            db.pool(),
            &events_repo::EventRow {
                id: id.clone(),
                calendar_id: calendar.into(),
                provider: provider.wire_str(),
                summary: Some(id.clone()),
                start_time: start.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
                end_time: (start + chrono::Duration::hours(1))
                    .to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
                is_all_day: false,
                status: "confirmed".into(),
                hangout_link: (!solo).then(|| "https://example.test/meeting".into()),
                conference_data: None,
                organizer_email: None,
                attendees: vec![],
            },
        )
        .await
        .unwrap();
        id
    }

    async fn reader_account(runtime: &CalendarRuntime, provider: CalendarProvider, calendar: &str) {
        settings::set(
            runtime.db.pool(),
            account_key(provider),
            &"reader@example.com",
        )
        .await
        .unwrap();
        seed_snapshot_calendar(&runtime.db, provider, calendar, "reader@example.com").await;
        runtime
            .prepare_snapshot(provider, "reader@example.com")
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn calendar_upcoming_merges_providers_caps_and_reports_both_freshness_records() {
        let db = Db::open_in_memory().await.unwrap();
        let runtime = snapshot_runtime(&db);
        let empty = upcoming_at(&db, at(), 5).await.unwrap();
        assert_eq!(empty.providers.len(), 2);
        assert!(empty
            .providers
            .iter()
            .all(|p| !p.connected && p.selected_calendars == 0 && p.last_attempt_at.is_none()));
        for provider in PROVIDERS {
            reader_account(&runtime, provider, "primary").await;
        }
        let earlier = reader_event(
            &db,
            CalendarProvider::Microsoft,
            "primary",
            "running",
            at() - chrono::Duration::minutes(10),
            false,
        )
        .await;
        let later = reader_event(
            &db,
            CalendarProvider::Google,
            "primary",
            "later",
            at() + chrono::Duration::hours(2),
            false,
        )
        .await;
        reader_event(&db, CalendarProvider::Google, "primary", "solo", at(), true).await;
        let upcoming = upcoming_at(&db, at(), 1).await.unwrap();
        assert_eq!(upcoming.events.len(), 1);
        assert_eq!(upcoming.events[0].id, earlier);
        assert_eq!(upcoming.events[0].provider, CalendarProvider::Microsoft);
        assert_eq!(upcoming.events[0].start, "2026-11-11T11:50:00Z");
        assert!(upcoming.has_more);
        let all = upcoming_at(&db, at(), 20).await.unwrap();
        assert_eq!(
            all.events.iter().map(|e| e.id.as_str()).collect::<Vec<_>>(),
            [earlier, later]
        );
        assert!(!all.has_more);
        assert!(all
            .providers
            .iter()
            .all(|p| p.connected && p.selected_calendars == 1 && p.last_attempt_at.is_some()));
        for limit in [0, 21] {
            assert!(upcoming_at(&db, at(), limit).await.is_err());
        }
        runtime
            .disconnect(CalendarProvider::Microsoft)
            .await
            .unwrap();
        let one_connected = upcoming_at(&db, at(), 5).await.unwrap();
        assert!(one_connected.providers[0].connected);
        assert!(!one_connected.providers[1].connected);
    }

    #[tokio::test]
    async fn calendar_candidates_preserve_solo_choices_and_never_break_a_meeting_tie() {
        let db = Db::open_in_memory().await.unwrap();
        let runtime = snapshot_runtime(&db);
        reader_account(&runtime, CalendarProvider::Google, "g").await;
        reader_account(&runtime, CalendarProvider::Microsoft, "m").await;
        let solo = reader_event(&db, CalendarProvider::Google, "g", "solo", at(), true).await;
        let candidates = candidates_at(&db, at()).await.unwrap();
        assert_eq!(candidates.events[0].id, solo);
        assert!(!candidates.events[0].meeting_like);
        assert!(candidates.auto.is_none());
        let meeting = reader_event(
            &db,
            CalendarProvider::Google,
            "g",
            "meeting",
            at() + chrono::Duration::minutes(10),
            false,
        )
        .await;
        assert_eq!(candidates_at(&db, at()).await.unwrap().auto, Some(meeting));
        reader_event(
            &db,
            CalendarProvider::Microsoft,
            "m",
            "overlap",
            at(),
            false,
        )
        .await;
        let candidates = candidates_at(&db, at()).await.unwrap();
        assert_eq!(candidates.events.len(), 3);
        assert!(candidates.auto.is_none());
        reader_event(
            &db,
            CalendarProvider::Google,
            "g",
            "too-late",
            at() + chrono::Duration::seconds(601),
            false,
        )
        .await;
        assert_eq!(calendar_events_at(&db, at()).await.unwrap().len(), 3);
    }

    #[tokio::test]
    async fn calendar_readers_gate_legacy_unbound_accounts_and_hide_other_calendars() {
        let db = Db::open_in_memory().await.unwrap();
        let runtime = snapshot_runtime(&db);
        for provider in PROVIDERS {
            // Legacy imported credentials can be present without a client binding.
            settings::set(db.pool(), account_key(provider), &"reader@example.com")
                .await
                .unwrap();
            let key = credentials::token_ref_for(provider.kind(), "reader@example.com").unwrap();
            let unbound = Secret::new(
                serde_json::json!({
                    "access_token": "fixture-access", "refresh_token": "fixture-refresh"
                })
                .to_string(),
            );
            runtime.credentials.set(&key, &unbound).unwrap();
            assert!(
                credentials::bundle_client_id(runtime.credentials.as_ref(), &key)
                    .unwrap()
                    .is_none()
            );
            tokens_repo::upsert(
                db.pool(),
                provider.kind(),
                "reader@example.com",
                key.slot(),
                now_epoch() + 3600,
                "scope",
            )
            .await
            .unwrap();
            seed_snapshot_calendar(&db, provider, "selected", "reader@example.com").await;
            reader_event(&db, provider, "selected", "legacy", at(), false).await;
            let legacy = SyncRecord {
                at: at().to_rfc3339(),
                account: None,
                snapshot_version: 0,
                last_success_at: None,
                outcome: sync_failure(CalendarProblem::Network),
            };
            settings::set(db.pool(), last_sync_key(provider), &legacy)
                .await
                .unwrap();
        }
        assert!(upcoming_at(&db, at(), 5).await.unwrap().events.is_empty());
        assert!(candidates_at(&db, at()).await.unwrap().events.is_empty());
        for provider in PROVIDERS {
            runtime
                .prepare_snapshot(provider, "reader@example.com")
                .await
                .unwrap();
            reader_event(&db, provider, "selected", "new", at(), false).await;
            seed_snapshot_calendar(&db, provider, "deselected", "reader@example.com").await;
            reader_event(&db, provider, "deselected", "hidden", at(), false).await;
            calendars_repo::set_selected(db.pool(), provider.kind(), "deselected", false)
                .await
                .unwrap();
            seed_snapshot_calendar(&db, provider, "other", "other@example.com").await;
            reader_event(&db, provider, "other", "foreign", at(), false).await;
        }
        let events = calendar_events_at(&db, at()).await.unwrap();
        assert_eq!(events.len(), 2);
        assert!(events.iter().all(|e| e.row.id.ends_with(":new")));
        assert_eq!(upcoming_at(&db, at(), 5).await.unwrap().events.len(), 2);
        // A version-one marker bound to another account still cannot open the gate
        // or disclose that account's success/outcome in freshness.
        settings::set(
            db.pool(),
            account_key(CalendarProvider::Google),
            &"other@example.com",
        )
        .await
        .unwrap();
        let upcoming = upcoming_at(&db, at(), 5).await.unwrap();
        assert_eq!(upcoming.events.len(), 1);
        assert!(upcoming.providers[0].last_attempt_at.is_none());
        assert!(upcoming.providers[0].last_success_at.is_none());
        assert!(upcoming.providers[0].last_outcome.is_none());
    }

    #[test]
    fn calendar_freshness_clock_table() {
        for (memory, persisted, expected) in [
            (None, None, false),
            (Some(299), None, true),
            (Some(300), None, false),
            (None, Some(-299), true),
            (None, Some(-300), false),
            (Some(301), Some(-10), true),
            (Some(10), Some(-301), true),
            (None, Some(1), false),
            (Some(10), Some(1), true),
            (Some(300), Some(-300), false),
        ] {
            assert_eq!(
                is_fresh(
                    memory.map(Duration::from_secs),
                    persisted.map(|s| at() + chrono::Duration::seconds(s)),
                    at()
                ),
                expected
            );
        }
        assert!(!is_fresh(None, parse_instant("unparseable"), at()));
    }

    #[tokio::test]
    async fn calendar_throttle_skips_are_local_and_manual_attempts_ignore_freshness() {
        let db = Db::open_in_memory().await.unwrap();
        let count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let observed = count.clone();
        let runtime = snapshot_runtime(&db).with_notifier(Arc::new(move || {
            observed.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }));
        let result = runtime.sync_if_stale().await.unwrap();
        assert!(result
            .iter()
            .all(|p| p.skipped == Some(SyncSkip::NotConnected)));
        settings::set(
            db.pool(),
            account_key(CalendarProvider::Google),
            &"reader@example.com",
        )
        .await
        .unwrap();
        assert_eq!(
            runtime.sync_if_stale().await.unwrap()[0].skipped,
            Some(SyncSkip::NoSelectedCalendars)
        );
        reader_account(&runtime, CalendarProvider::Google, "primary").await;
        let result = runtime.sync_if_stale().await.unwrap();
        assert_eq!(result[0].skipped, Some(SyncSkip::Fresh));
        let guard = runtime
            .control(CalendarProvider::Google)
            .acquire(CalendarOperation::Connecting)
            .unwrap();
        assert_eq!(
            runtime.sync_if_stale().await.unwrap()[0].skipped,
            Some(SyncSkip::Busy)
        );
        drop(guard);
        assert_eq!(count.load(std::sync::atomic::Ordering::SeqCst), 0);
        assert!(runtime
            .control(CalendarProvider::Google)
            .attempt_clock()
            .is_none());
        // No token row exists: this attempt terminates locally, yet must start
        // the monotonic throttle and emit exactly once despite persisted freshness.
        runtime.sync(CalendarProvider::Google).await.unwrap();
        assert!(runtime
            .control(CalendarProvider::Google)
            .attempt_clock()
            .is_some());
        assert_eq!(count.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert_eq!(
            runtime.sync_if_stale().await.unwrap()[0].skipped,
            Some(SyncSkip::Fresh)
        );
        // Force a stale retry using injected clocks, still with no usable token.
        *runtime.control(CalendarProvider::Google).attempt_clock() = None;
        settings::remove(db.pool(), last_sync_key(CalendarProvider::Google))
            .await
            .unwrap();
        let result = runtime.sync_if_stale().await.unwrap();
        assert!(result[0].outcome.is_some());
        assert!(result[0].skipped.is_none());
        assert_eq!(count.load(std::sync::atomic::Ordering::SeqCst), 2);
        // Readers remain usable while the provider operation guard is held and
        // never notify: neither they nor freshness reads are sync triggers.
        let _guard = runtime
            .control(CalendarProvider::Google)
            .acquire(CalendarOperation::Syncing)
            .unwrap();
        upcoming_at(&db, at(), 5).await.unwrap();
        candidates_at(&db, at()).await.unwrap();
        calendar_events_at(&db, at()).await.unwrap();
        assert_eq!(count.load(std::sync::atomic::Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn calendar_notifier_counts_runtime_operations_on_success_and_failure() {
        let db = Db::open_in_memory().await.unwrap();
        let count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let observed = count.clone();
        let runtime = snapshot_runtime(&db).with_notifier(Arc::new(move || {
            observed.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }));
        let provider = CalendarProvider::Google;
        runtime.disconnect(provider).await.unwrap();
        assert_eq!(count.load(std::sync::atomic::Ordering::SeqCst), 1);
        // Invalid cleanup data fails before browser/network construction.
        settings::set(db.pool(), cleanup_key(provider), &vec!["pending"])
            .await
            .unwrap();
        sqlx::query("DROP TABLE google_calendar_tokens")
            .execute(db.pool())
            .await
            .unwrap();
        assert!(matches!(
            runtime.connect(provider).await.unwrap(),
            ConnectOutcome::Failed { .. }
        ));
        assert_eq!(count.load(std::sync::atomic::Ordering::SeqCst), 2);
        assert!(runtime
            .remove_leftover(provider, "leftover@example.com")
            .await
            .is_err());
        assert_eq!(count.load(std::sync::atomic::Ordering::SeqCst), 3);
        runtime.sync(provider).await.unwrap();
        assert_eq!(count.load(std::sync::atomic::Ordering::SeqCst), 4);
        assert!(runtime.control(provider).attempt_clock().is_some());
    }

    fn grant_tokens(access: &str) -> kw_calendar_core::TokenSet {
        kw_calendar_core::TokenSet {
            access_token: Secret::new(access.to_owned()),
            refresh_token: Some(Secret::new("refresh-fixture".into())),
            expires_at: now_epoch() + 3600,
            scope: "scope".into(),
        }
    }

    #[derive(Debug, Default)]
    struct ControlledCredentials {
        memory: kw_security::MemoryCredentialStore,
        fail_delete: std::sync::atomic::AtomicBool,
        fail_set_at: std::sync::atomic::AtomicUsize,
        sets: std::sync::atomic::AtomicUsize,
    }

    impl CredentialStore for ControlledCredentials {
        fn get(&self, key: &KeyRef) -> Result<Secret, kw_security::CredentialError> {
            self.memory.get(key)
        }
        fn set(&self, key: &KeyRef, secret: &Secret) -> Result<(), kw_security::CredentialError> {
            let count = self.sets.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
            if self.fail_set_at.load(std::sync::atomic::Ordering::SeqCst) == count {
                return Err(kw_security::CredentialError::Backend(
                    "private-store-error".into(),
                ));
            }
            self.memory.set(key, secret)
        }
        fn delete(&self, key: &KeyRef) -> Result<(), kw_security::CredentialError> {
            if self.fail_delete.load(std::sync::atomic::Ordering::SeqCst) {
                return Err(kw_security::CredentialError::Backend(
                    "private-store-error".into(),
                ));
            }
            self.memory.delete(key)
        }
    }

    fn grant_calendars(provider: CalendarProvider, email: &str) -> GrantCalendars {
        match provider {
            CalendarProvider::Google => GrantCalendars::Google(vec![google::CalendarListEntry {
                id: email.into(),
                summary: email.into(),
                description: None,
                background_color: None,
                is_primary: true,
                selected: true,
            }]),
            CalendarProvider::Microsoft => {
                GrantCalendars::Microsoft(vec![microsoft::CalendarListEntry {
                    id: email.into(),
                    summary: email.into(),
                    background_color: None,
                    is_primary: true,
                    owner_email: Some(email.into()),
                }])
            }
        }
    }

    async fn lifecycle_runtime() -> (CalendarRuntime, Arc<ControlledCredentials>) {
        let db = Db::open_in_memory().await.unwrap();
        let creds = Arc::new(ControlledCredentials::default());
        for provider in [CalendarProvider::Google, CalendarProvider::Microsoft] {
            write_client_id(&db, provider, Some("client".into()))
                .await
                .unwrap();
        }
        (
            CalendarRuntime::new(db, creds.clone(), NetworkPolicy::new().unwrap()),
            creds,
        )
    }

    async fn install(
        runtime: &CalendarRuntime,
        provider: CalendarProvider,
        email: &str,
        access: &str,
    ) -> Result<ConnectOutcome, CalendarProblem> {
        runtime
            .install_grant(
                provider,
                email,
                &grant_tokens(access),
                "client",
                &grant_calendars(provider, email),
            )
            .await
    }

    async fn seed_import(
        runtime: &CalendarRuntime,
        provider: CalendarProvider,
        email: &str,
        name: &str,
    ) {
        let slot = KeyRef::new(name).unwrap();
        runtime
            .credentials
            .set(
                &slot,
                &credentials::grant_secret(&grant_tokens("imported-access"), "client").unwrap(),
            )
            .unwrap();
        tokens_repo::upsert(
            runtime.db.pool(),
            provider.kind(),
            email,
            name,
            now_epoch() + 3600,
            "scope",
        )
        .await
        .unwrap();
        seed_snapshot_calendar(&runtime.db, provider, email, email).await;
    }

    #[tokio::test]
    async fn calendar_provider_guards_are_independent_in_both_directions() {
        let (runtime, _) = lifecycle_runtime().await;
        for (held, other) in [
            (CalendarProvider::Google, CalendarProvider::Microsoft),
            (CalendarProvider::Microsoft, CalendarProvider::Google),
        ] {
            let _guard = runtime
                .control(held)
                .acquire(CalendarOperation::Syncing)
                .unwrap();
            assert!(runtime
                .sync(held)
                .await
                .unwrap_err()
                .contains("already in progress"));
            assert!(runtime.disconnect(held).await.is_err());
            assert!(runtime.remove_leftover(held, "none").await.is_err());
            assert_eq!(
                runtime.sync(other).await.unwrap_err(),
                format!("no {} account is connected", other.name())
            );
            assert!(
                settings::get::<SyncRecord>(runtime.db.pool(), last_sync_key(other))
                    .await
                    .unwrap()
                    .is_none()
            );
            runtime.disconnect(other).await.unwrap();
        }
    }

    #[tokio::test]
    async fn calendar_cancel_and_failed_browser_keep_the_previous_connection() {
        for browser_ok in [true, false] {
            let (mut runtime, creds) = lifecycle_runtime().await;
            let provider = CalendarProvider::Google;
            install(&runtime, provider, "old@example.com", "old-access")
                .await
                .unwrap();
            let slot = credentials::token_ref_for(provider.kind(), "old@example.com").unwrap();
            let before = creds.get(&slot).unwrap();
            runtime.browser_opener = if browser_ok {
                |_| Ok(())
            } else {
                |_| Err(std::io::Error::other("browser unavailable"))
            };
            let task = async {
                let (outcome, ()) = tokio::join!(runtime.connect(provider), async {
                    loop {
                        if let Some(pending) =
                            runtime.control(provider).state().pending_sign_in.clone()
                        {
                            assert_eq!(pending.browser_opened, browser_ok);
                            assert!(pending
                                .authorize_url
                                .starts_with("https://accounts.google.com/"));
                            break;
                        }
                        tokio::task::yield_now().await;
                    }
                    assert!(matches!(
                        runtime.control(provider).state().operation,
                        CalendarOperation::Connecting
                    ));
                    assert!(runtime.connect(provider).await.is_err());
                    runtime.cancel_connect(provider);
                });
                outcome.unwrap()
            };
            let outcome = tokio::time::timeout(Duration::from_secs(2), task)
                .await
                .unwrap();
            assert!(matches!(
                outcome,
                ConnectOutcome::Failed {
                    problem: CalendarProblem::Cancelled
                }
            ));
            assert_eq!(
                runtime
                    .read_previous_account(provider)
                    .await
                    .unwrap()
                    .as_deref(),
                Some("old@example.com")
            );
            assert_eq!(creds.get(&slot).unwrap().expose(), before.expose());
            assert_eq!(
                tokens_repo::list(runtime.db.pool(), provider.kind())
                    .await
                    .unwrap()
                    .len(),
                1
            );
            assert_eq!(
                calendars_repo::list_calendars(
                    runtime.db.pool(),
                    provider.kind(),
                    "old@example.com"
                )
                .await
                .unwrap()
                .len(),
                1
            );
            assert!(runtime.control(provider).state().pending_sign_in.is_none());
            assert!(matches!(
                runtime.control(provider).state().operation,
                CalendarOperation::Idle
            ));
            runtime.cancel_connect(provider); // idempotent without an active flow
        }
    }

    #[tokio::test]
    async fn calendar_replacement_commit_failure_keeps_all_previous_state() {
        for provider in [CalendarProvider::Google, CalendarProvider::Microsoft] {
            let (runtime, creds) = lifecycle_runtime().await;
            install(&runtime, provider, "old@example.com", "old-access")
                .await
                .unwrap();
            seed_snapshot_calendar(&runtime.db, provider, "with-event", "old@example.com").await;
            let old_slot = credentials::token_ref_for(provider.kind(), "old@example.com").unwrap();
            let before = creds.get(&old_slot).unwrap();
            settings::set(runtime.db.pool(), last_sync_key(provider), &"old-record")
                .await
                .unwrap();
            sqlx::query("CREATE TRIGGER refuse_account BEFORE INSERT ON settings WHEN NEW.key LIKE 'calendar.%.account' BEGIN SELECT RAISE(ABORT, 'fixture'); END").execute(runtime.db.pool()).await.unwrap();
            assert_eq!(
                install(&runtime, provider, "new@example.com", "new-access")
                    .await
                    .unwrap_err(),
                CalendarProblem::Storage
            );
            assert_eq!(
                runtime
                    .read_previous_account(provider)
                    .await
                    .unwrap()
                    .as_deref(),
                Some("old@example.com")
            );
            assert_eq!(creds.get(&old_slot).unwrap().expose(), before.expose());
            assert!(!creds.contains(
                &credentials::token_ref_for(provider.kind(), "new@example.com").unwrap()
            ));
            assert!(
                tokens_repo::read(runtime.db.pool(), provider.kind(), "new@example.com")
                    .await
                    .unwrap()
                    .is_none()
            );
            assert_eq!(
                calendars_repo::list_calendars(
                    runtime.db.pool(),
                    provider.kind(),
                    "old@example.com"
                )
                .await
                .unwrap()
                .len(),
                2
            );
            assert_eq!(
                events_repo::count_events(runtime.db.pool(), provider.wire_str(), "with-event")
                    .await
                    .unwrap(),
                1
            );
            assert_eq!(
                settings::get::<String>(runtime.db.pool(), last_sync_key(provider))
                    .await
                    .unwrap()
                    .as_deref(),
                Some("old-record")
            );
            assert!(runtime.cleanup_refs(provider).await.unwrap().is_empty());
        }
    }

    #[tokio::test]
    async fn calendar_same_account_restore_and_restore_failure_have_defined_states() {
        for restore_fails in [false, true] {
            let (runtime, creds) = lifecycle_runtime().await;
            let provider = CalendarProvider::Google;
            install(&runtime, provider, "same@example.com", "old-access")
                .await
                .unwrap();
            let slot = credentials::token_ref_for(provider.kind(), "same@example.com").unwrap();
            let old = creds.get(&slot).unwrap();
            if restore_fails {
                creds.fail_set_at.store(
                    creds.sets.load(std::sync::atomic::Ordering::SeqCst) + 2,
                    std::sync::atomic::Ordering::SeqCst,
                );
            }
            sqlx::query("CREATE TRIGGER refuse_account BEFORE INSERT ON settings WHEN NEW.key LIKE 'calendar.%.account' BEGIN SELECT RAISE(ABORT, 'fixture'); END").execute(runtime.db.pool()).await.unwrap();
            let problem = install(&runtime, provider, "same@example.com", "new-access")
                .await
                .unwrap_err();
            if restore_fails {
                assert_eq!(problem, CalendarProblem::ReconnectRequired);
                assert!(!creds.contains(&slot));
                assert_eq!(
                    token_status(
                        &runtime.db,
                        creds.as_ref(),
                        provider.kind(),
                        "same@example.com"
                    )
                    .await
                    .unwrap(),
                    TokenStatus::Missing
                );
            } else {
                assert_eq!(problem, CalendarProblem::Storage);
                assert_eq!(creds.get(&slot).unwrap().expose(), old.expose());
            }
            assert_eq!(
                runtime
                    .read_previous_account(provider)
                    .await
                    .unwrap()
                    .as_deref(),
                Some("same@example.com")
            );
        }
    }

    #[tokio::test]
    async fn calendar_leftover_restore_failure_does_not_require_active_account_reconnect() {
        for provider in [CalendarProvider::Google, CalendarProvider::Microsoft] {
            for (bundle_present, restore_fails, delete_fails) in [
                (true, false, false),
                (true, true, false),
                (true, true, true),
                (false, false, false),
                (false, false, true),
            ] {
                let (runtime, creds) = lifecycle_runtime().await;
                install(&runtime, provider, "active@example.com", "active-access")
                    .await
                    .unwrap();
                let active_slot =
                    credentials::token_ref_for(provider.kind(), "active@example.com").unwrap();
                let active = creds.get(&active_slot).unwrap();
                let slot =
                    credentials::token_ref_for(provider.kind(), "leftover@example.com").unwrap();
                seed_import(&runtime, provider, "leftover@example.com", slot.slot()).await;
                let leftover = creds.get(&slot).unwrap();
                if !bundle_present {
                    creds.delete(&slot).unwrap();
                }
                if restore_fails {
                    creds.fail_set_at.store(
                        creds.sets.load(std::sync::atomic::Ordering::SeqCst) + 2,
                        std::sync::atomic::Ordering::SeqCst,
                    );
                }
                creds
                    .fail_delete
                    .store(delete_fails, std::sync::atomic::Ordering::SeqCst);
                sqlx::query("CREATE TRIGGER refuse_account BEFORE INSERT ON settings WHEN NEW.key LIKE 'calendar.%.account' BEGIN SELECT RAISE(ABORT, 'fixture'); END").execute(runtime.db.pool()).await.unwrap();
                assert_eq!(
                    install(
                        &runtime,
                        provider,
                        "leftover@example.com",
                        "replacement-access"
                    )
                    .await
                    .unwrap_err(),
                    CalendarProblem::Storage
                );
                assert_eq!(
                    runtime
                        .read_previous_account(provider)
                        .await
                        .unwrap()
                        .as_deref(),
                    Some("active@example.com")
                );
                assert_eq!(creds.get(&active_slot).unwrap().expose(), active.expose());
                assert!(tokens_repo::read(
                    runtime.db.pool(),
                    provider.kind(),
                    "leftover@example.com"
                )
                .await
                .unwrap()
                .is_some());
                if bundle_present && !restore_fails {
                    assert_eq!(creds.get(&slot).unwrap().expose(), leftover.expose());
                } else {
                    assert_eq!(creds.contains(&slot), delete_fails);
                }
                assert_eq!(
                    runtime.cleanup_refs(provider).await.unwrap(),
                    if delete_fails {
                        vec![slot.slot().to_owned()]
                    } else {
                        vec![]
                    }
                );
                // Even a queued failed undo cannot delete a still-referenced slot.
                creds
                    .fail_delete
                    .store(false, std::sync::atomic::Ordering::SeqCst);
                runtime.process_cleanup(provider).await.unwrap();
                if delete_fails {
                    assert!(creds.contains(&slot));
                }
            }
        }
    }

    #[tokio::test]
    async fn calendar_cleanup_survives_restart_and_reused_slot_is_never_deleted() {
        let (runtime, creds) = lifecycle_runtime().await;
        let provider = CalendarProvider::Google;
        install(&runtime, provider, "same@example.com", "old-access")
            .await
            .unwrap();
        let slot = credentials::token_ref_for(provider.kind(), "same@example.com").unwrap();
        creds
            .fail_delete
            .store(true, std::sync::atomic::Ordering::SeqCst);
        runtime.disconnect(provider).await.unwrap();
        assert_eq!(
            runtime.cleanup_refs(provider).await.unwrap(),
            vec![slot.slot()]
        );
        assert_eq!(
            provider_status(&runtime.db, creds.as_ref(), provider)
                .await
                .unwrap()
                .cleanup_pending,
            1
        );
        let restarted = CalendarRuntime::new(
            runtime.db.clone(),
            creds.clone(),
            NetworkPolicy::new().unwrap(),
        );
        restarted.process_cleanup(provider).await.unwrap(); // start-of-connect retry still fails
        assert_eq!(
            restarted.cleanup_refs(provider).await.unwrap(),
            vec![slot.slot()]
        );
        install(&restarted, provider, "same@example.com", "new-access")
            .await
            .unwrap();
        assert!(restarted.cleanup_refs(provider).await.unwrap().is_empty());
        let new = creds.get(&slot).unwrap();
        creds
            .fail_delete
            .store(false, std::sync::atomic::Ordering::SeqCst);
        for _ in 0..3 {
            restarted.process_cleanup(provider).await.unwrap();
        }
        assert_eq!(creds.get(&slot).unwrap().expose(), new.expose());
        // A stale list from an earlier interrupted cleanup must also preserve a live row.
        settings::set(runtime.db.pool(), cleanup_key(provider), &vec![slot.slot()])
            .await
            .unwrap();
        restarted.process_cleanup(provider).await.unwrap();
        assert!(restarted.cleanup_refs(provider).await.unwrap().is_empty());
        assert_eq!(creds.get(&slot).unwrap().expose(), new.expose());
        creds
            .fail_delete
            .store(true, std::sync::atomic::Ordering::SeqCst);
        install(&restarted, provider, "other@example.com", "other-access")
            .await
            .unwrap();
        assert_eq!(
            restarted.cleanup_refs(provider).await.unwrap(),
            vec![slot.slot()]
        );
        let restarted = CalendarRuntime::new(
            runtime.db.clone(),
            creds.clone(),
            NetworkPolicy::new().unwrap(),
        );
        creds
            .fail_delete
            .store(false, std::sync::atomic::Ordering::SeqCst);
        restarted.process_cleanup(provider).await.unwrap();
        assert!(restarted.cleanup_refs(provider).await.unwrap().is_empty());
        assert!(!creds.contains(&slot));
    }

    #[tokio::test]
    async fn calendar_imported_and_native_leftovers_are_listed_and_removed_independently() {
        for provider in [CalendarProvider::Google, CalendarProvider::Microsoft] {
            let (runtime, creds) = lifecycle_runtime().await;
            let positional = format!("calendar-{}-0", provider.wire_str());
            seed_import(&runtime, provider, "import@example.com", &positional).await;
            let earlier =
                credentials::token_ref_for(provider.kind(), "earlier@example.com").unwrap();
            seed_import(&runtime, provider, "earlier@example.com", earlier.slot()).await;
            runtime.disconnect(provider).await.unwrap();
            assert_eq!(
                tokens_repo::list(runtime.db.pool(), provider.kind())
                    .await
                    .unwrap()
                    .len(),
                2
            );
            install(&runtime, provider, "active@example.com", "active-access")
                .await
                .unwrap();
            let status = provider_status(&runtime.db, creds.as_ref(), provider)
                .await
                .unwrap();
            assert_eq!(status.leftover.len(), 2);
            assert!(matches!(status.leftover[0].origin, LeftoverOrigin::Earlier));
            assert!(matches!(
                status.leftover[1].origin,
                LeftoverOrigin::Openwhispr
            ));
            let json = serde_json::to_string(&status).unwrap();
            for private in [
                "active-access",
                "refresh-fixture",
                "imported-access",
                earlier.slot(),
                &positional,
            ] {
                assert!(!json.contains(private));
            }
            assert!(runtime
                .remove_leftover(provider, "active@example.com")
                .await
                .unwrap_err()
                .contains("Disconnect"));
            runtime
                .remove_leftover(provider, "import@example.com")
                .await
                .unwrap();
            runtime
                .remove_leftover(provider, "import@example.com")
                .await
                .unwrap();
            assert!(!creds.contains(&KeyRef::new(&positional).unwrap()));
            assert_eq!(
                events_repo::count_events(
                    runtime.db.pool(),
                    provider.wire_str(),
                    "import@example.com"
                )
                .await
                .unwrap(),
                0
            );
            runtime.disconnect(provider).await.unwrap();
            assert!(creds.contains(&earlier));
            assert!(
                tokens_repo::read(runtime.db.pool(), provider.kind(), "earlier@example.com")
                    .await
                    .unwrap()
                    .is_some()
            );
            assert!(runtime
                .read_previous_account(provider)
                .await
                .unwrap()
                .is_none());
            // An imported positional row can be the connected account as well.
            settings::set(
                runtime.db.pool(),
                account_key(provider),
                &"earlier@example.com",
            )
            .await
            .unwrap();
            runtime.disconnect(provider).await.unwrap();
            seed_import(&runtime, provider, "import@example.com", &positional).await;
            settings::set(
                runtime.db.pool(),
                account_key(provider),
                &"import@example.com",
            )
            .await
            .unwrap();
            runtime.disconnect(provider).await.unwrap();
            assert!(!creds.contains(&KeyRef::new(&positional).unwrap()));
            assert!(tokens_repo::list(runtime.db.pool(), provider.kind())
                .await
                .unwrap()
                .is_empty());
        }
    }

    #[tokio::test]
    async fn calendar_removing_a_leftover_alias_never_queues_the_active_slot() {
        let (runtime, creds) = lifecycle_runtime().await;
        let provider = CalendarProvider::Google;
        install(&runtime, provider, "active@example.com", "active-access")
            .await
            .unwrap();
        let slot = credentials::token_ref_for(provider.kind(), "active@example.com").unwrap();
        tokens_repo::upsert(
            runtime.db.pool(),
            provider.kind(),
            "alias@example.com",
            slot.slot(),
            now_epoch() + 3600,
            "scope",
        )
        .await
        .unwrap();
        runtime
            .remove_leftover(provider, "alias@example.com")
            .await
            .unwrap();
        assert!(runtime.cleanup_refs(provider).await.unwrap().is_empty());
        assert!(creds.contains(&slot));
    }

    #[tokio::test]
    async fn calendar_reconnect_uses_native_slot_and_clears_cache_and_sync_record() {
        let (runtime, creds) = lifecycle_runtime().await;
        let provider = CalendarProvider::Google;
        seed_import(
            &runtime,
            provider,
            "import@example.com",
            "calendar-google-12",
        )
        .await;
        settings::set(runtime.db.pool(), last_sync_key(provider), &"stale-record")
            .await
            .unwrap();
        for access in ["first-access", "second-access"] {
            install(&runtime, provider, "import@example.com", access)
                .await
                .unwrap();
            let native = credentials::token_ref_for(provider.kind(), "import@example.com").unwrap();
            assert_eq!(
                tokens_repo::read(runtime.db.pool(), provider.kind(), "import@example.com")
                    .await
                    .unwrap()
                    .unwrap()
                    .token_ref,
                native.slot()
            );
            assert!(creds.contains(&native));
            assert!(!creds.contains(&KeyRef::new("calendar-google-12").unwrap()));
            assert_eq!(
                tokens_repo::list(runtime.db.pool(), provider.kind())
                    .await
                    .unwrap()
                    .len(),
                1
            );
            assert_eq!(
                events_repo::count_events(runtime.db.pool(), "google", "import@example.com")
                    .await
                    .unwrap(),
                0
            );
            assert!(
                settings::get::<serde_json::Value>(runtime.db.pool(), last_sync_key(provider))
                    .await
                    .unwrap()
                    .is_none()
            );
        }
    }

    #[tokio::test]
    async fn calendar_failed_new_grant_deletion_is_queued_and_write_failure_keeps_old_grant() {
        let (runtime, creds) = lifecycle_runtime().await;
        let provider = CalendarProvider::Google;
        install(&runtime, provider, "old@example.com", "old-access")
            .await
            .unwrap();
        creds.fail_set_at.store(
            creds.sets.load(std::sync::atomic::Ordering::SeqCst) + 1,
            std::sync::atomic::Ordering::SeqCst,
        );
        assert_eq!(
            install(&runtime, provider, "new@example.com", "new-access")
                .await
                .unwrap_err(),
            CalendarProblem::Storage
        );
        assert_eq!(
            runtime
                .read_previous_account(provider)
                .await
                .unwrap()
                .as_deref(),
            Some("old@example.com")
        );
        sqlx::query("CREATE TRIGGER refuse_account BEFORE INSERT ON settings WHEN NEW.key LIKE 'calendar.%.account' BEGIN SELECT RAISE(ABORT, 'fixture'); END").execute(runtime.db.pool()).await.unwrap();
        creds
            .fail_delete
            .store(true, std::sync::atomic::Ordering::SeqCst);
        assert_eq!(
            install(&runtime, provider, "new@example.com", "new-access")
                .await
                .unwrap_err(),
            CalendarProblem::Storage
        );
        let slot = credentials::token_ref_for(provider.kind(), "new@example.com").unwrap();
        assert_eq!(
            runtime.cleanup_refs(provider).await.unwrap(),
            vec![slot.slot()]
        );
        creds
            .fail_delete
            .store(false, std::sync::atomic::Ordering::SeqCst);
        runtime.process_cleanup(provider).await.unwrap();
        assert!(!creds.contains(&slot));
    }

    #[tokio::test]
    async fn calendar_binding_status_requires_reconnect_without_network() {
        let (runtime, creds) = lifecycle_runtime().await;
        let provider = CalendarProvider::Google;
        install(&runtime, provider, "user@example.com", "access")
            .await
            .unwrap();
        let slot = credentials::token_ref_for(provider.kind(), "user@example.com").unwrap();
        for binding in [None, Some("another-client")] {
            let bundle = serde_json::json!({ "access_token": "private-access", "refresh_token": "private-refresh", "client_id": binding });
            creds.set(&slot, &Secret::new(bundle.to_string())).unwrap();
            assert_eq!(
                provider_status(&runtime.db, creds.as_ref(), provider)
                    .await
                    .unwrap()
                    .account
                    .unwrap()
                    .token,
                TokenStatus::ReconnectRequired
            );
            assert!(matches!(
                runtime.sync(provider).await.unwrap(),
                SyncOutcome::TokenUnavailable {
                    problem: Some(CalendarProblem::ReconnectRequired),
                    ..
                }
            ));
        }
    }

    #[tokio::test]
    async fn calendar_purge_marker_failure_rolls_back_the_cache_purge() {
        let (runtime, _) = lifecycle_runtime().await;
        let provider = CalendarProvider::Google;
        seed_snapshot_calendar(&runtime.db, provider, "cached", "user@example.com").await;
        sqlx::query("CREATE TRIGGER refuse_marker BEFORE INSERT ON settings WHEN NEW.key = 'calendar.google.lastSync' BEGIN SELECT RAISE(ABORT, 'fixture'); END").execute(runtime.db.pool()).await.unwrap();
        assert!(runtime
            .prepare_snapshot(provider, "user@example.com")
            .await
            .is_err());
        assert_eq!(
            events_repo::count_events(runtime.db.pool(), "google", "cached")
                .await
                .unwrap(),
            1
        );
        assert!(
            settings::get::<SyncRecord>(runtime.db.pool(), last_sync_key(provider))
                .await
                .unwrap()
                .is_none()
        );
    }

    #[tokio::test]
    async fn calendar_missing_restore_bundle_and_disconnect_transaction_failure() {
        let (runtime, creds) = lifecycle_runtime().await;
        let provider = CalendarProvider::Google;
        install(&runtime, provider, "user@example.com", "old-access")
            .await
            .unwrap();
        let slot = credentials::token_ref_for(provider.kind(), "user@example.com").unwrap();
        creds.delete(&slot).unwrap();
        sqlx::query("CREATE TRIGGER refuse_account BEFORE INSERT ON settings WHEN NEW.key = 'calendar.google.account' BEGIN SELECT RAISE(ABORT, 'fixture'); END").execute(runtime.db.pool()).await.unwrap();
        assert_eq!(
            install(&runtime, provider, "user@example.com", "new-access")
                .await
                .unwrap_err(),
            CalendarProblem::ReconnectRequired
        );
        assert!(!creds.contains(&slot));
        sqlx::query("DROP TRIGGER refuse_account")
            .execute(runtime.db.pool())
            .await
            .unwrap();
        install(&runtime, provider, "user@example.com", "working-access")
            .await
            .unwrap();
        seed_snapshot_calendar(&runtime.db, provider, "cached", "user@example.com").await;
        sqlx::query("CREATE TRIGGER refuse_disconnect BEFORE DELETE ON settings WHEN OLD.key = 'calendar.google.account' BEGIN SELECT RAISE(ABORT, 'fixture'); END").execute(runtime.db.pool()).await.unwrap();
        assert!(runtime.disconnect(provider).await.is_err());
        assert!(creds.contains(&slot));
        assert!(
            tokens_repo::read(runtime.db.pool(), provider.kind(), "user@example.com")
                .await
                .unwrap()
                .is_some()
        );
        assert_eq!(
            events_repo::count_events(runtime.db.pool(), "google", "cached")
                .await
                .unwrap(),
            1
        );
        assert_eq!(
            runtime
                .read_previous_account(provider)
                .await
                .unwrap()
                .as_deref(),
            Some("user@example.com")
        );
        assert!(runtime.cleanup_refs(provider).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn calendar_connect_classifies_pre_consent_storage_failures() {
        let (runtime, _) = lifecycle_runtime().await;
        runtime.db.pool().close().await;
        assert!(matches!(
            runtime.connect(CalendarProvider::Google).await.unwrap(),
            ConnectOutcome::Failed {
                problem: CalendarProblem::Storage
            }
        ));
        assert!(matches!(
            runtime.control(CalendarProvider::Google).state().operation,
            CalendarOperation::Idle
        ));
    }

    #[tokio::test]
    async fn calendar_sync_record_write_failure_is_classified() {
        let (runtime, _) = lifecycle_runtime().await;
        sqlx::query("CREATE TRIGGER refuse_marker BEFORE INSERT ON settings WHEN NEW.key = 'calendar.google.lastSync' BEGIN SELECT RAISE(ABORT, 'fixture'); END").execute(runtime.db.pool()).await.unwrap();
        let outcome = runtime
            .record_sync(
                CalendarProvider::Google,
                "user@example.com",
                SyncOutcome::Completed {
                    events_upserted: 0,
                    events_removed: 0,
                    calendar_errors: vec![],
                },
            )
            .await;
        assert!(matches!(
            outcome,
            SyncOutcome::Failed {
                problem: Some(CalendarProblem::Storage),
                ..
            }
        ));
    }

    #[test]
    fn calendar_problem_classification_is_allowlisted_and_contextual() {
        use kw_calendar_core::ExchangeError as E;
        use CalendarProblem as P;
        let google = CalendarProvider::Google;
        let microsoft = CalendarProvider::Microsoft;
        for (provider, code, expected) in [
            (google, "access_denied", P::AccessDenied),
            (microsoft, "access_denied", P::AccessDenied),
            (google, "admin_policy_enforced", P::AppBlocked),
            (google, "org_internal", P::AppBlocked),
            (google, "redirect_uri_mismatch", P::ClientMisconfigured),
            (google, "private-unknown-error", P::InvalidResponse),
            (microsoft, "org_internal", P::InvalidResponse),
        ] {
            let error = CalendarError::AuthorizationDenied {
                error: code.into(),
                description: Some("private-description".into()),
            };
            assert_eq!(classify(&error, provider, false), expected);
            assert!(
                !serde_json::to_string(&ConnectOutcome::Failed { problem: expected })
                    .unwrap()
                    .contains("private")
            );
        }
        for (code, expected) in [
            ("65001", P::AdminConsentRequired),
            ("90094", P::AdminConsentRequired),
            ("90095", P::AdminConsentRequired),
            ("50011", P::ClientMisconfigured),
            ("700016", P::ClientMisconfigured),
            ("650010", P::InvalidResponse),
        ] {
            let description = Some(format!("AADSTS{code}: private-description"));
            for error in [
                CalendarError::AuthorizationDenied {
                    error: "unknown".into(),
                    description: description.clone(),
                },
                CalendarError::Exchange(E::Rejected {
                    error: "unknown".into(),
                    description,
                }),
            ] {
                assert_eq!(classify(&error, microsoft, false), expected);
                assert_eq!(classify(&error, google, false), P::InvalidResponse);
            }
        }
        for (code, refresh, expected) in [
            ("invalid_client", false, P::ClientRejected),
            ("unauthorized_client", false, P::ClientRejected),
            ("invalid_grant", false, P::InvalidResponse),
            ("invalid_grant", true, P::ReconnectRequired),
        ] {
            assert_eq!(
                classify(
                    &CalendarError::Exchange(E::Rejected {
                        error: code.into(),
                        description: None
                    }),
                    google,
                    refresh
                ),
                expected
            );
        }
        for (error, expected) in [
            (CalendarError::ReconnectRequired, P::ReconnectRequired),
            (CalendarError::StateMismatch, P::InvalidResponse),
            (CalendarError::MissingCallbackParams, P::InvalidResponse),
            (CalendarError::NoRefreshToken, P::InvalidResponse),
            (
                CalendarError::Exchange(E::MalformedResponse),
                P::InvalidResponse,
            ),
            (
                CalendarError::Credential(kw_security::CredentialError::Backend("private".into())),
                P::Storage,
            ),
            (CalendarError::Storage(sqlx::Error::PoolClosed), P::Storage),
        ] {
            assert_eq!(classify(&error, google, false), expected);
        }
        for status in [401, 403, 500, 503] {
            assert_eq!(
                classify_sync(&kw_calendar_core::SyncError::HttpStatus { status }),
                if status >= 500 {
                    P::ProviderUnavailable
                } else {
                    P::ClientMisconfigured
                }
            );
        }
        for (status, expected) in [
            (400, P::InvalidResponse),
            (499, P::InvalidResponse),
            (500, P::ProviderUnavailable),
            (503, P::ProviderUnavailable),
            (599, P::ProviderUnavailable),
            (600, P::InvalidResponse),
        ] {
            for provider in [google, microsoft] {
                for refresh in [false, true] {
                    assert_eq!(
                        classify(
                            &CalendarError::Exchange(E::HttpStatus { status }),
                            provider,
                            refresh
                        ),
                        expected
                    );
                }
            }
        }
        assert_eq!(
            classify_redirect(&kw_calendar_core::RedirectError::TimedOut),
            P::TimedOut
        );
        assert_eq!(
            classify_redirect(&kw_calendar_core::RedirectError::Bind(
                std::io::Error::other("fixture")
            )),
            P::Network
        );
        assert_eq!(
            classify_redirect(&kw_calendar_core::RedirectError::MalformedRequest),
            P::InvalidResponse
        );
        // Construct a reqwest error without performing a request.
        let transport = reqwest::Client::new()
            .get("invalid-url")
            .build()
            .unwrap_err();
        let problem = classify(&CalendarError::Exchange(E::Http(transport)), google, true);
        assert!(matches!(
            sync_failure(problem),
            SyncOutcome::Failed {
                problem: Some(P::Network),
                ..
            }
        ));
        let old: SyncOutcome =
            serde_json::from_str(r#"{"state":"failed","message":"legacy"}"#).unwrap();
        assert!(matches!(old, SyncOutcome::Failed { problem: None, .. }));
    }

    async fn write_client_id(
        db: &Db,
        provider: CalendarProvider,
        id: Option<String>,
    ) -> Result<(), kw_storage::StorageError> {
        if let Some(id) = id {
            settings::set(db.pool(), client_id_key(provider), &id).await
        } else {
            settings::remove(db.pool(), client_id_key(provider)).await
        }
    }

    fn snapshot_runtime(db: &Db) -> CalendarRuntime {
        CalendarRuntime::new(
            db.clone(),
            Arc::new(kw_security::MemoryCredentialStore::new()),
            NetworkPolicy::new().unwrap(),
        )
    }

    #[tokio::test]
    async fn calendar_failures_and_item_errors_report_partial_success_without_losing_counters() {
        let db = Db::open_in_memory().await.unwrap();
        let runtime = snapshot_runtime(&db);
        for id in ["bad", "good", "unreadable"] {
            seed_snapshot_calendar(&db, CalendarProvider::Google, id, "user@example.com").await;
        }
        let calendars = calendars_repo::list_calendars(
            db.pool(),
            CalendarProviderKind::Google,
            "user@example.com",
        )
        .await
        .unwrap();
        let start = events_repo::parse_row_instant("2019-01-01").unwrap();
        let end = events_repo::parse_row_instant("2021-01-01").unwrap();
        let outcome = sync_selected_calendars(calendars, |id| {
            let runtime = &runtime;
            async move {
                if id == "bad" {
                    return Err(CalendarProblem::Network);
                }
                let keep_ids = if id == "unreadable" {
                    vec![events_repo::row_id("google", &id, "old")]
                } else {
                    vec![]
                };
                runtime
                    .commit_snapshot(
                        CalendarProvider::Google,
                        &id,
                        &start,
                        &end,
                        kw_calendar_core::sync_support::WindowSnapshot {
                            rows: vec![],
                            keep_ids,
                        },
                    )
                    .await
            }
        })
        .await;
        let SyncOutcome::Completed {
            events_upserted,
            events_removed,
            calendar_errors,
        } = outcome
        else {
            panic!("expected partial completion");
        };
        assert_eq!((events_upserted, events_removed), (0, 1));
        assert_eq!(
            calendar_errors,
            [
                "bad: Could not reach the calendar provider. Try again.",
                "1 event in unreadable had a time that could not be read"
            ]
        );
        assert_eq!(
            events_repo::count_events(db.pool(), "google", "bad")
                .await
                .unwrap(),
            1
        );
        assert_eq!(
            events_repo::count_events(db.pool(), "google", "good")
                .await
                .unwrap(),
            0
        );
        assert_eq!(
            events_repo::count_events(db.pool(), "google", "unreadable")
                .await
                .unwrap(),
            1
        );
    }

    async fn seed_snapshot_calendar(db: &Db, provider: CalendarProvider, id: &str, email: &str) {
        let table = match provider {
            CalendarProvider::Google => "google_calendars",
            CalendarProvider::Microsoft => "microsoft_calendars",
        };
        sqlx::query(&format!(
            "INSERT INTO {table} (id, summary, account_email) VALUES (?, ?, ?)"
        ))
        .bind(id)
        .bind(id)
        .bind(email)
        .execute(db.pool())
        .await
        .unwrap();
        sqlx::query("INSERT INTO calendar_events (id, provider, calendar_id, start_time, end_time) VALUES (?, ?, ?, '2020-01-01', '2020-01-02')")
            .bind(events_repo::row_id(provider.wire_str(), id, "old")).bind(provider.wire_str()).bind(id).execute(db.pool()).await.unwrap();
    }

    #[test]
    fn old_sync_records_default_to_an_ungated_snapshot() {
        let record: SyncRecord = serde_json::from_str(
            r#"{"at":"2026-01-01T00:00:00Z","outcome":{"state":"failed","message":"old"}}"#,
        )
        .unwrap();
        assert_eq!(record.snapshot_version, 0);
        assert!(record.account.is_none());
        assert!(record.last_success_at.is_none());
    }

    #[tokio::test]
    async fn snapshot_gate_purges_only_this_accounts_provider_before_any_new_rows() {
        for provider in [CalendarProvider::Google, CalendarProvider::Microsoft] {
            for mode in ["missing", "version-zero", "other-account"] {
                let db = Db::open_in_memory().await.unwrap();
                let runtime = snapshot_runtime(&db);
                seed_snapshot_calendar(&db, provider, "current", "user@example.com").await;
                seed_snapshot_calendar(&db, provider, "other", "other@example.com").await;
                let other_provider = match provider {
                    CalendarProvider::Google => CalendarProvider::Microsoft,
                    CalendarProvider::Microsoft => CalendarProvider::Google,
                };
                seed_snapshot_calendar(&db, other_provider, "current", "user@example.com").await;
                if mode != "missing" {
                    let record = SyncRecord {
                        at: "2026-01-01T00:00:00Z".to_owned(),
                        outcome: SyncOutcome::Failed {
                            problem: Some(CalendarProblem::Storage),
                            message: "previous".to_owned(),
                        },
                        account: Some(
                            if mode == "other-account" {
                                "other@example.com"
                            } else {
                                "user@example.com"
                            }
                            .to_owned(),
                        ),
                        snapshot_version: if mode == "other-account" { 1 } else { 0 },
                        last_success_at: Some("2025-12-01T00:00:00Z".to_owned()),
                    };
                    settings::set(db.pool(), last_sync_key(provider), &record)
                        .await
                        .unwrap();
                }
                runtime
                    .prepare_snapshot(provider, "user@example.com")
                    .await
                    .unwrap();
                assert_eq!(
                    events_repo::count_events(db.pool(), provider.wire_str(), "current")
                        .await
                        .unwrap(),
                    0
                );
                assert_eq!(
                    events_repo::count_events(db.pool(), provider.wire_str(), "other")
                        .await
                        .unwrap(),
                    1
                );
                assert_eq!(
                    events_repo::count_events(db.pool(), other_provider.wire_str(), "current")
                        .await
                        .unwrap(),
                    1
                );
                let gate = settings::get::<SyncRecord>(db.pool(), last_sync_key(provider))
                    .await
                    .unwrap()
                    .unwrap();
                assert_eq!(gate.snapshot_version, 1);
                assert_eq!(gate.account.as_deref(), Some("user@example.com"));
                if mode != "version-zero" {
                    assert_ne!(gate.at, "2026-01-01T00:00:00Z");
                    assert!(gate.last_success_at.is_none());
                    assert!(
                        matches!(gate.outcome, SyncOutcome::Failed { message, problem: None } if message == "The first sync after an update did not finish.")
                    );
                } else {
                    assert_eq!(gate.at, "2026-01-01T00:00:00Z");
                    assert_eq!(
                        gate.last_success_at.as_deref(),
                        Some("2025-12-01T00:00:00Z")
                    );
                    assert!(
                        matches!(gate.outcome, SyncOutcome::Failed { message, problem: Some(CalendarProblem::Storage) } if message == "previous")
                    );
                }
                // Version one for this account is idempotent, even with new
                // rows outside the snapshot window left by a later sync.
                sqlx::query("INSERT INTO calendar_events (id, provider, calendar_id, start_time, end_time) VALUES ('new', ?, 'current', '2030-01-01', '2030-01-02')")
                    .bind(provider.wire_str()).execute(db.pool()).await.unwrap();
                runtime
                    .prepare_snapshot(provider, "user@example.com")
                    .await
                    .unwrap();
                assert_eq!(
                    events_repo::count_events(db.pool(), provider.wire_str(), "current")
                        .await
                        .unwrap(),
                    1
                );
            }
        }
    }

    #[tokio::test]
    async fn undecodable_sync_record_is_replaced_by_snapshot_gate_and_sync_result() {
        let db = Db::open_in_memory().await.unwrap();
        let runtime = snapshot_runtime(&db);
        let provider = CalendarProvider::Google;
        let key = last_sync_key(provider);
        seed_snapshot_calendar(&db, provider, "current", "user@example.com").await;
        settings::set(db.pool(), key, &"undecodable sync record")
            .await
            .unwrap();
        assert!(matches!(
            settings::get::<SyncRecord>(db.pool(), key).await,
            Ok(None)
        ));

        runtime
            .prepare_snapshot(provider, "user@example.com")
            .await
            .unwrap();
        assert_eq!(
            events_repo::count_events(db.pool(), "google", "current")
                .await
                .unwrap(),
            0
        );
        let gate = settings::get::<SyncRecord>(db.pool(), key)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(gate.account.as_deref(), Some("user@example.com"));
        assert_eq!(gate.snapshot_version, 1);
        assert!(gate.last_success_at.is_none());
        assert!(
            matches!(gate.outcome, SyncOutcome::Failed { message, problem: None } if message == "The first sync after an update did not finish.")
        );

        // A normal result preserves the repaired gate. Also exercise recording
        // directly over corrupt history, as a token failure can precede gating.
        for corrupt_again in [false, true] {
            if corrupt_again {
                settings::set(db.pool(), key, &42).await.unwrap();
                assert!(matches!(
                    settings::get::<SyncRecord>(db.pool(), key).await,
                    Ok(None)
                ));
            }
            runtime
                .record_sync(
                    provider,
                    "user@example.com",
                    SyncOutcome::TokenUnavailable {
                        problem: Some(CalendarProblem::ReconnectRequired),
                        message: "unavailable".to_owned(),
                    },
                )
                .await;
            let record = settings::get::<SyncRecord>(db.pool(), key)
                .await
                .unwrap()
                .unwrap();
            assert_eq!(record.account.as_deref(), Some("user@example.com"));
            assert_eq!(record.snapshot_version, if corrupt_again { 0 } else { 1 });
            assert!(record.last_success_at.is_none());
            assert!(
                matches!(record.outcome, SyncOutcome::TokenUnavailable { message, .. } if message == "unavailable")
            );
        }
    }

    #[tokio::test]
    async fn last_success_changes_only_after_full_success_and_token_failure_cannot_skip_gate() {
        let db = Db::open_in_memory().await.unwrap();
        let runtime = snapshot_runtime(&db);
        let provider = CalendarProvider::Google;
        let old = SyncRecord {
            at: "2026-01-01T00:00:00Z".to_owned(),
            outcome: SyncOutcome::Failed {
                problem: Some(CalendarProblem::Storage),
                message: "previous".to_owned(),
            },
            account: Some("other@example.com".to_owned()),
            snapshot_version: 1,
            last_success_at: Some("2025-12-01T00:00:00Z".to_owned()),
        };
        settings::set(db.pool(), last_sync_key(provider), &old)
            .await
            .unwrap();
        runtime
            .record_sync(
                provider,
                "user@example.com",
                SyncOutcome::TokenUnavailable {
                    problem: Some(CalendarProblem::ReconnectRequired),
                    message: "unavailable".to_owned(),
                },
            )
            .await;
        let token_failure = settings::get::<SyncRecord>(db.pool(), last_sync_key(provider))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(token_failure.snapshot_version, 0);
        assert!(token_failure.last_success_at.is_none());
        runtime
            .prepare_snapshot(provider, "user@example.com")
            .await
            .unwrap();
        for outcome in [
            SyncOutcome::Failed {
                problem: Some(CalendarProblem::Storage),
                message: "failed".to_owned(),
            },
            SyncOutcome::TokenUnavailable {
                problem: Some(CalendarProblem::ReconnectRequired),
                message: "unavailable".to_owned(),
            },
            SyncOutcome::Completed {
                events_upserted: 1,
                events_removed: 0,
                calendar_errors: vec!["partial".to_owned()],
            },
        ] {
            runtime
                .record_sync(provider, "user@example.com", outcome)
                .await;
            let record = settings::get::<SyncRecord>(db.pool(), last_sync_key(provider))
                .await
                .unwrap()
                .unwrap();
            assert!(record.last_success_at.is_none());
            assert_eq!(record.snapshot_version, 1);
        }
        runtime
            .record_sync(
                provider,
                "user@example.com",
                SyncOutcome::Completed {
                    events_upserted: 0,
                    events_removed: 0,
                    calendar_errors: vec![],
                },
            )
            .await;
        let record = settings::get::<SyncRecord>(db.pool(), last_sync_key(provider))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(record.last_success_at.as_deref(), Some(record.at.as_str()));
        assert_ne!(record.last_success_at, old.last_success_at);
        assert_eq!(record.account.as_deref(), Some("user@example.com"));
        assert_eq!(record.snapshot_version, 1);
        runtime
            .record_sync(
                provider,
                "user@example.com",
                SyncOutcome::Completed {
                    events_upserted: 1,
                    events_removed: 0,
                    calendar_errors: vec!["partial".to_owned()],
                },
            )
            .await;
        let partial = settings::get::<SyncRecord>(db.pool(), last_sync_key(provider))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(partial.last_success_at, record.last_success_at);
    }

    #[tokio::test]
    async fn unreadable_item_preserves_its_row_and_counts_as_a_calendar_error() {
        let db = Db::open_in_memory().await.unwrap();
        let runtime = snapshot_runtime(&db);
        seed_snapshot_calendar(&db, CalendarProvider::Google, "team", "user@example.com").await;
        let id = events_repo::row_id("google", "team", "old");
        sqlx::query("UPDATE calendar_events SET start_time = '2026-11-11T09:00:00Z', end_time = '2026-11-11T10:00:00Z' WHERE id = ?")
            .bind(&id).execute(db.pool()).await.unwrap();
        let start = events_repo::parse_row_instant("2026-11-10").unwrap();
        let end = start + chrono::Duration::days(31);
        let (upserted, removed, unreadable) = runtime
            .commit_snapshot(
                CalendarProvider::Google,
                "team",
                &start,
                &end,
                kw_calendar_core::sync_support::WindowSnapshot {
                    rows: vec![],
                    keep_ids: vec![id],
                },
            )
            .await
            .unwrap();
        assert_eq!((upserted, removed, unreadable), (0, 0, 1));
        assert_eq!(
            events_repo::count_events(db.pool(), "google", "team")
                .await
                .unwrap(),
            1
        );
        assert_eq!(
            time_error("Team calendar", unreadable),
            "1 event in Team calendar had a time that could not be read"
        );
    }

    #[test]
    fn every_provider_has_distinct_settings_keys() {
        for provider in [CalendarProvider::Google, CalendarProvider::Microsoft] {
            assert!(kw_storage::settings::check_key(client_id_key(provider)).is_ok());
            assert!(kw_storage::settings::check_key(account_key(provider)).is_ok());
            assert!(kw_storage::settings::check_key(last_sync_key(provider)).is_ok());
        }
        assert_ne!(
            client_id_key(CalendarProvider::Google),
            client_id_key(CalendarProvider::Microsoft)
        );
        assert_ne!(
            account_key(CalendarProvider::Google),
            account_key(CalendarProvider::Microsoft)
        );
    }

    #[test]
    fn the_wire_provider_maps_onto_the_calendar_core_kind() {
        assert_eq!(
            CalendarProvider::Google.kind(),
            CalendarProviderKind::Google
        );
        assert_eq!(
            CalendarProvider::Microsoft.kind(),
            CalendarProviderKind::Microsoft
        );
    }

    #[test]
    fn the_browser_command_carries_the_url_verbatim_as_one_argument() {
        // Whichever platform this was built for, the url must appear intact
        // as its own argument -- never folded into a single string that a
        // shell would have to re-split, which is exactly the injection this
        // exists to avoid. See the module doc on why `rundll32` rather than
        // `cmd /c start` on Windows.
        let url = "https://accounts.google.com/o/oauth2/v2/auth?client_id=abc&scope=x&state=y";
        let (_program, args) = browser_command(url);
        assert!(args.contains(&url.to_owned()));
    }

    #[test]
    fn connect_outcomes_serialize_with_a_discriminant() {
        let json = serde_json::to_value(ConnectOutcome::Connected {
            account_email: "user@example.com".to_owned(),
            calendars: 3,
        })
        .unwrap();
        assert_eq!(json["state"], "connected");
        assert_eq!(json["accountEmail"], "user@example.com");

        let json = serde_json::to_value(ConnectOutcome::Denied {
            reason: "access_denied".to_owned(),
        })
        .unwrap();
        assert_eq!(json["state"], "denied");
    }

    #[test]
    fn sync_outcomes_serialize_with_a_discriminant() {
        let json = serde_json::to_value(SyncOutcome::Completed {
            events_upserted: 5,
            events_removed: 1,
            calendar_errors: Vec::new(),
        })
        .unwrap();
        assert_eq!(json["state"], "completed");
        assert_eq!(json["eventsUpserted"], 5);

        let json = serde_json::to_value(SyncOutcome::TokenUnavailable {
            problem: Some(CalendarProblem::ReconnectRequired),
            message: "expired".to_owned(),
        })
        .unwrap();
        assert_eq!(json["state"], "token-unavailable");
    }

    #[tokio::test]
    async fn a_never_connected_account_reports_a_missing_token_without_a_database_row() {
        let db = kw_storage::Db::open_in_memory().await.unwrap();
        let creds = kw_security::MemoryCredentialStore::new();
        let status = token_status(
            &db,
            &creds,
            CalendarProviderKind::Google,
            "nobody@example.com",
        )
        .await
        .unwrap();
        assert_eq!(status, TokenStatus::Missing);
    }

    #[tokio::test]
    async fn a_database_row_with_no_matching_credential_reports_missing_not_active() {
        // An older build or external credential reset can leave metadata
        // behind after its credential is gone. A caller that only looked at the row's `expires_at` would
        // report this account as still connected.
        let db = kw_storage::Db::open_in_memory().await.unwrap();
        let creds = kw_security::MemoryCredentialStore::new();
        kw_calendar_core::tokens_repo::upsert(
            db.pool(),
            CalendarProviderKind::Google,
            "user@example.com",
            "calendar-google-abc123",
            now_epoch() + 3_600,
            "calendar.readonly",
        )
        .await
        .unwrap();

        let status = token_status(
            &db,
            &creds,
            CalendarProviderKind::Google,
            "user@example.com",
        )
        .await
        .unwrap();
        assert_eq!(status, TokenStatus::Missing);
    }

    #[tokio::test]
    async fn a_present_credential_within_its_expiry_reports_active() {
        let db = kw_storage::Db::open_in_memory().await.unwrap();
        let creds = kw_security::MemoryCredentialStore::new();
        write_client_id(&db, CalendarProvider::Google, Some("client".into()))
            .await
            .unwrap();
        let slot =
            credentials::token_ref_for(CalendarProviderKind::Google, "user@example.com").unwrap();
        creds
            .set(
                &slot,
                &credentials::grant_secret(&grant_tokens("access"), "client").unwrap(),
            )
            .unwrap();
        kw_calendar_core::tokens_repo::upsert(
            db.pool(),
            CalendarProviderKind::Google,
            "user@example.com",
            slot.slot(),
            now_epoch() + 3_600,
            "calendar.readonly",
        )
        .await
        .unwrap();

        let status = token_status(
            &db,
            &creds,
            CalendarProviderKind::Google,
            "user@example.com",
        )
        .await
        .unwrap();
        assert_eq!(status, TokenStatus::Active);
    }

    #[tokio::test]
    async fn a_present_credential_past_its_expiry_reports_needs_refresh() {
        let db = kw_storage::Db::open_in_memory().await.unwrap();
        let creds = kw_security::MemoryCredentialStore::new();
        write_client_id(&db, CalendarProvider::Google, Some("client".into()))
            .await
            .unwrap();
        let slot =
            credentials::token_ref_for(CalendarProviderKind::Google, "user@example.com").unwrap();
        creds
            .set(
                &slot,
                &credentials::grant_secret(&grant_tokens("access"), "client").unwrap(),
            )
            .unwrap();
        kw_calendar_core::tokens_repo::upsert(
            db.pool(),
            CalendarProviderKind::Google,
            "user@example.com",
            slot.slot(),
            now_epoch() - 10,
            "calendar.readonly",
        )
        .await
        .unwrap();

        let status = token_status(
            &db,
            &creds,
            CalendarProviderKind::Google,
            "user@example.com",
        )
        .await
        .unwrap();
        assert_eq!(status, TokenStatus::NeedsRefresh);
    }

    fn token_unavailable_record(message: &str) -> SyncRecord {
        SyncRecord {
            account: None,
            snapshot_version: 0,
            last_success_at: None,
            at: "2026-01-01T00:00:00Z".to_owned(),
            outcome: SyncOutcome::TokenUnavailable {
                problem: Some(CalendarProblem::ReconnectRequired),
                message: message.to_owned(),
            },
        }
    }

    #[test]
    fn needs_refresh_with_a_recorded_token_failure_becomes_refresh_failed() {
        let record = token_unavailable_record("invalid_grant");
        let combined = combine_token_status(TokenStatus::NeedsRefresh, Some(&record));
        assert_eq!(
            combined,
            TokenStatus::RefreshFailed {
                message: "invalid_grant".to_owned()
            }
        );
    }

    #[test]
    fn needs_refresh_with_no_recorded_sync_stays_needs_refresh() {
        // Expired, but nothing has tried and failed yet -- a real difference
        // from the case above, and the label for it must not say the same
        // thing as a known failure.
        assert_eq!(
            combine_token_status(TokenStatus::NeedsRefresh, None),
            TokenStatus::NeedsRefresh
        );
    }

    #[test]
    fn needs_refresh_with_an_unrelated_recorded_outcome_stays_needs_refresh() {
        let completed = SyncRecord {
            account: None,
            snapshot_version: 0,
            last_success_at: None,
            at: "2026-01-01T00:00:00Z".to_owned(),
            outcome: SyncOutcome::Completed {
                events_upserted: 3,
                events_removed: 0,
                calendar_errors: Vec::new(),
            },
        };
        assert_eq!(
            combine_token_status(TokenStatus::NeedsRefresh, Some(&completed)),
            TokenStatus::NeedsRefresh
        );

        let failed = SyncRecord {
            account: None,
            snapshot_version: 0,
            last_success_at: None,
            at: "2026-01-01T00:00:00Z".to_owned(),
            outcome: SyncOutcome::Failed {
                problem: Some(CalendarProblem::Storage),
                message: "the calendar list could not be read".to_owned(),
            },
        };
        assert_eq!(
            combine_token_status(TokenStatus::NeedsRefresh, Some(&failed)),
            TokenStatus::NeedsRefresh
        );
    }

    #[test]
    fn active_and_missing_are_never_downgraded_by_a_recorded_failure() {
        // `Active` cannot coexist with a *current* recorded `TokenUnavailable`
        // in practice -- see the doc comment -- but this pins that the
        // function does not even try to second-guess either state: only
        // `NeedsRefresh` is ever folded together with the sync record.
        let record = token_unavailable_record("invalid_grant");
        assert_eq!(
            combine_token_status(TokenStatus::Active, Some(&record)),
            TokenStatus::Active
        );
        assert_eq!(
            combine_token_status(TokenStatus::Missing, Some(&record)),
            TokenStatus::Missing
        );
    }

    #[tokio::test]
    async fn a_previous_account_read_failure_is_propagated_not_treated_as_absent() {
        // The regression this guards against: `connect` used to fold a
        // settings-read error into `unwrap_or(None)`, which took the
        // "nothing was previously connected" branch on a database failure
        // and silently skipped disconnecting a different account -- leaving
        // its refresh token live in the OS credential store with nothing
        // pointing at it once `account_key` was overwritten.
        let db = kw_storage::Db::open_in_memory().await.unwrap();
        let creds: Arc<dyn CredentialStore> = Arc::new(kw_security::MemoryCredentialStore::new());
        let policy = NetworkPolicy::new().unwrap();
        let runtime = CalendarRuntime::new(db.clone(), creds, policy);

        // A closed pool fails every subsequent query, which is the cheapest
        // way to force `settings::get` into `Err` without a real outage.
        db.pool().close().await;

        let err = runtime
            .read_previous_account(CalendarProvider::Google)
            .await
            .unwrap_err();
        assert!(
            err.contains("could not read"),
            "a read failure must surface as an error, not as Ok(None): {err}"
        );
    }

    #[tokio::test]
    async fn calendar_custom_client_updates_and_clears_the_pair() {
        let db = Db::open_in_memory().await.unwrap();
        let creds = kw_security::MemoryCredentialStore::new();
        let provider = CalendarProvider::Google;
        let slot = custom_secret_slot().unwrap();
        persist_custom_client(
            &db,
            &creds,
            provider,
            Some(" first "),
            SecretChange::Set {
                value: "first-secret".into(),
            },
            |id| write_client_id(&db, provider, id),
        )
        .await
        .unwrap();
        assert_eq!(
            settings::get::<String>(db.pool(), client_id_key(provider))
                .await
                .unwrap()
                .as_deref(),
            Some("first")
        );
        assert_eq!(creds.get(&slot).unwrap().expose(), "first-secret");
        persist_custom_client(
            &db,
            &creds,
            provider,
            Some("first"),
            SecretChange::Keep,
            |id| write_client_id(&db, provider, id),
        )
        .await
        .unwrap();
        assert_eq!(creds.get(&slot).unwrap().expose(), "first-secret");
        let status =
            serde_json::to_string(&provider_status(&db, &creds, provider).await.unwrap()).unwrap();
        assert!(!status.contains("first-secret"));
        assert!(status.contains("\"customGoogleSecretConfigured\":true"));
        persist_custom_client(
            &db,
            &creds,
            provider,
            Some("second"),
            SecretChange::Keep,
            |id| write_client_id(&db, provider, id),
        )
        .await
        .unwrap();
        assert!(!creds.contains(&slot));
        persist_custom_client(
            &db,
            &creds,
            provider,
            Some("third"),
            SecretChange::Set {
                value: "third-secret".into(),
            },
            |id| write_client_id(&db, provider, id),
        )
        .await
        .unwrap();
        assert_eq!(creds.get(&slot).unwrap().expose(), "third-secret");
        persist_custom_client(
            &db,
            &creds,
            provider,
            Some("third"),
            SecretChange::Clear,
            |id| write_client_id(&db, provider, id),
        )
        .await
        .unwrap();
        assert!(!creds.contains(&slot));
        for cleared in [None, Some(" ")] {
            persist_custom_client(
                &db,
                &creds,
                provider,
                Some("third"),
                SecretChange::Set {
                    value: "third-secret".into(),
                },
                |id| write_client_id(&db, provider, id),
            )
            .await
            .unwrap();
            persist_custom_client(&db, &creds, provider, cleared, SecretChange::Keep, |id| {
                write_client_id(&db, provider, id)
            })
            .await
            .unwrap();
            assert!(!creds.contains(&slot));
            assert!(settings::get::<String>(db.pool(), client_id_key(provider))
                .await
                .unwrap()
                .is_none());
        }
    }

    #[tokio::test]
    async fn calendar_custom_client_rejects_invalid_secret_edits() {
        let db = Db::open_in_memory().await.unwrap();
        let creds = kw_security::MemoryCredentialStore::new();
        for change in [
            SecretChange::Clear,
            SecretChange::Set {
                value: "private-value".into(),
            },
        ] {
            let error = persist_custom_client(
                &db,
                &creds,
                CalendarProvider::Microsoft,
                Some("id"),
                change,
                |id| write_client_id(&db, CalendarProvider::Microsoft, id),
            )
            .await
            .unwrap_err();
            assert!(!error.contains("private-value"));
        }
        for id in [None, Some(" ")] {
            assert!(persist_custom_client(
                &db,
                &creds,
                CalendarProvider::Google,
                id,
                SecretChange::Set {
                    value: "private-value".into()
                },
                |id| write_client_id(&db, CalendarProvider::Google, id)
            )
            .await
            .is_err());
        }
        assert!(settings::keys(db.pool()).await.unwrap().is_empty());
        assert!(!creds.contains(&custom_secret_slot().unwrap()));
        assert!(!format!(
            "{:?}",
            SecretChange::Set {
                value: "private-value".into()
            }
        )
        .contains("private-value"));
    }

    #[tokio::test]
    async fn calendar_custom_client_settings_failure_restores_the_previous_pair() {
        let db = Db::open_in_memory().await.unwrap();
        let creds = kw_security::MemoryCredentialStore::new();
        let provider = CalendarProvider::Google;
        let slot = custom_secret_slot().unwrap();
        // Fail only writes, so reads and credential changes succeed first.
        sqlx::query("CREATE TRIGGER reject_calendar_insert BEFORE INSERT ON settings BEGIN SELECT RAISE(FAIL, 'injected failure'); END").execute(db.pool()).await.unwrap();
        assert!(persist_custom_client(
            &db,
            &creds,
            provider,
            Some("new"),
            SecretChange::Set {
                value: "new-secret".into()
            },
            |id| write_client_id(&db, provider, id)
        )
        .await
        .is_err());
        assert!(!creds.contains(&slot));
        sqlx::query("DROP TRIGGER reject_calendar_insert")
            .execute(db.pool())
            .await
            .unwrap();
        persist_custom_client(
            &db,
            &creds,
            provider,
            Some("old"),
            SecretChange::Set {
                value: "old-secret".into(),
            },
            |id| write_client_id(&db, provider, id),
        )
        .await
        .unwrap();
        sqlx::query("CREATE TRIGGER reject_calendar_insert BEFORE INSERT ON settings BEGIN SELECT RAISE(FAIL, 'injected failure'); END").execute(db.pool()).await.unwrap();
        sqlx::query("CREATE TRIGGER reject_calendar_delete BEFORE DELETE ON settings BEGIN SELECT RAISE(FAIL, 'injected failure'); END").execute(db.pool()).await.unwrap();
        for (id, change) in [
            (
                Some("new"),
                SecretChange::Set {
                    value: "new-secret".into(),
                },
            ),
            (Some("new"), SecretChange::Keep),
            (None, SecretChange::Keep),
        ] {
            assert!(persist_custom_client(
                &db,
                &creds,
                provider,
                id,
                change,
                |id| write_client_id(&db, provider, id)
            )
            .await
            .is_err());
            assert_eq!(creds.get(&slot).unwrap().expose(), "old-secret");
            assert_eq!(
                settings::get::<String>(db.pool(), client_id_key(provider))
                    .await
                    .unwrap()
                    .as_deref(),
                Some("old")
            );
        }
    }

    #[tokio::test]
    async fn calendar_custom_client_injected_write_failure_restores_the_previous_pair() {
        let db = Db::open_in_memory().await.unwrap();
        let creds = kw_security::MemoryCredentialStore::new();
        let provider = CalendarProvider::Google;
        let slot = custom_secret_slot().unwrap();
        for previous_id in [None, Some("old")] {
            persist_custom_client(
                &db,
                &creds,
                provider,
                previous_id,
                if previous_id.is_some() {
                    SecretChange::Set {
                        value: "old-secret".into(),
                    }
                } else {
                    SecretChange::Keep
                },
                |id| write_client_id(&db, provider, id),
            )
            .await
            .unwrap();
            for (id, change) in [
                (
                    Some("new"),
                    SecretChange::Set {
                        value: "new-secret".into(),
                    },
                ),
                (Some("new"), SecretChange::Keep),
                (None, SecretChange::Keep),
            ] {
                let secret_set = matches!(&change, SecretChange::Set { .. });
                let error =
                    persist_custom_client(&db, &creds, provider, id, change, |normalized_id| {
                        assert_eq!(normalized_id.as_deref(), id);
                        // The write callback runs after the credential edit.
                        assert_eq!(creds.contains(&slot), secret_set);
                        std::future::ready(Err(kw_storage::StorageError::Sqlx(
                            sqlx::Error::PoolClosed,
                        )))
                    })
                    .await
                    .unwrap_err();
                assert!(error.contains("could not save the client ID"));
                assert_eq!(
                    settings::get::<String>(db.pool(), client_id_key(provider))
                        .await
                        .unwrap()
                        .as_deref(),
                    previous_id
                );
                assert_eq!(creds.contains(&slot), previous_id.is_some());
                if previous_id.is_some() {
                    assert_eq!(creds.get(&slot).unwrap().expose(), "old-secret");
                }
            }
        }
    }

    #[cfg(feature = "tauri-test")]
    #[tokio::test]
    async fn calendar_custom_client_refuses_an_existing_operation_guard() {
        use tauri::Manager as _;

        let db = Db::open_in_memory().await.unwrap();
        let runtime = Arc::new(CalendarRuntime::new(
            db.clone(),
            Arc::new(kw_security::MemoryCredentialStore::new()),
            NetworkPolicy::new().unwrap(),
        ));
        let app = tauri::test::mock_builder()
            .manage(runtime.clone())
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .unwrap();
        let _guard = runtime
            .control(CalendarProvider::Google)
            .guard
            .try_lock()
            .unwrap();
        let expected = "a calendar connection or sync is already in progress";
        assert_eq!(
            calendar_set_custom_client(
                app.state(),
                CalendarProvider::Google,
                Some("id".into()),
                SecretChange::Keep,
            )
            .await
            .unwrap_err(),
            expected,
        );
        assert_eq!(
            calendar_set_client_id(app.state(), CalendarProvider::Google, "id".into())
                .await
                .unwrap_err(),
            expected,
        );
        assert!(settings::keys(db.pool()).await.unwrap().is_empty());
    }
    #[cfg(feature = "tauri-test")]
    #[tokio::test]
    async fn calendar_notifier_counts_client_selection_and_cleanup_commands() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use tauri::Manager as _;

        let db = Db::open_in_memory().await.unwrap();
        let count = Arc::new(AtomicUsize::new(0));
        let observed = count.clone();
        let runtime = Arc::new(snapshot_runtime(&db).with_notifier(Arc::new(move || {
            observed.fetch_add(1, Ordering::SeqCst);
        })));
        let app = tauri::test::mock_builder()
            .manage(runtime.clone())
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .unwrap();
        let provider = CalendarProvider::Google;
        calendar_set_client_id(app.state(), provider, "custom".into())
            .await
            .unwrap();
        assert_eq!(count.load(Ordering::SeqCst), 1);
        calendar_set_custom_client(app.state(), provider, None, SecretChange::Keep)
            .await
            .unwrap();
        assert_eq!(count.load(Ordering::SeqCst), 2);
        seed_snapshot_calendar(&db, provider, "primary", "reader@example.com").await;
        calendar_set_selected(app.state(), provider, "primary".into(), false)
            .await
            .unwrap();
        assert_eq!(count.load(Ordering::SeqCst), 3);
        calendar_retry_cleanup(app.state(), provider).await.unwrap();
        assert_eq!(count.load(Ordering::SeqCst), 4);
        // Each rejected command also completes once, even before acquiring a
        // guard. The ongoing operation retains sole ownership of its state.
        let _guard = runtime
            .control(provider)
            .acquire(CalendarOperation::Connecting)
            .unwrap();
        assert!(
            calendar_set_client_id(app.state(), provider, "other".into())
                .await
                .is_err()
        );
        assert_eq!(count.load(Ordering::SeqCst), 5);
        assert!(
            calendar_set_custom_client(app.state(), provider, None, SecretChange::Keep)
                .await
                .is_err()
        );
        assert_eq!(count.load(Ordering::SeqCst), 6);
        assert!(
            calendar_set_selected(app.state(), provider, "primary".into(), true)
                .await
                .is_err()
        );
        assert_eq!(count.load(Ordering::SeqCst), 7);
        assert!(calendar_retry_cleanup(app.state(), provider).await.is_err());
        assert_eq!(count.load(Ordering::SeqCst), 8);
    }

    #[derive(Debug)]
    struct RefusingCredentialWrites(kw_security::MemoryCredentialStore);

    impl CredentialStore for RefusingCredentialWrites {
        fn get(&self, key: &KeyRef) -> Result<Secret, kw_security::CredentialError> {
            self.0.get(key)
        }
        fn set(&self, _key: &KeyRef, _secret: &Secret) -> Result<(), kw_security::CredentialError> {
            Err(kw_security::CredentialError::Backend(
                "private-backend-detail".into(),
            ))
        }
        fn delete(&self, _key: &KeyRef) -> Result<(), kw_security::CredentialError> {
            Err(kw_security::CredentialError::Backend(
                "private-backend-detail".into(),
            ))
        }
    }

    #[derive(Debug)]
    struct PresenceOnlyCredentials(bool);

    impl CredentialStore for PresenceOnlyCredentials {
        fn get(&self, _key: &KeyRef) -> Result<Secret, kw_security::CredentialError> {
            panic!("status must not read the custom secret value");
        }
        fn set(&self, _key: &KeyRef, _secret: &Secret) -> Result<(), kw_security::CredentialError> {
            panic!("status must not write credentials");
        }
        fn delete(&self, _key: &KeyRef) -> Result<(), kw_security::CredentialError> {
            panic!("status must not delete credentials");
        }
        fn contains(&self, key: &KeyRef) -> bool {
            assert_eq!(key, &custom_secret_slot().unwrap());
            self.0
        }
    }

    #[tokio::test]
    async fn calendar_status_checks_custom_secret_presence_without_reading_it() {
        let db = Db::open_in_memory().await.unwrap();
        for present in [false, true] {
            let credentials = PresenceOnlyCredentials(present);
            for provider in [CalendarProvider::Google, CalendarProvider::Microsoft] {
                let status = provider_status(&db, &credentials, provider).await.unwrap();
                assert_eq!(
                    status.custom_google_secret_configured,
                    present && provider == CalendarProvider::Google
                );
            }
        }
    }

    #[tokio::test]
    async fn calendar_credential_write_failure_keeps_the_previous_pair() {
        let db = Db::open_in_memory().await.unwrap();
        let memory = kw_security::MemoryCredentialStore::new();
        let provider = CalendarProvider::Google;
        persist_custom_client(
            &db,
            &memory,
            provider,
            Some("old"),
            SecretChange::Set {
                value: "old-secret".into(),
            },
            |id| write_client_id(&db, provider, id),
        )
        .await
        .unwrap();
        let creds = RefusingCredentialWrites(memory);
        for change in [
            SecretChange::Keep,
            SecretChange::Set {
                value: "new-secret".into(),
            },
            SecretChange::Clear,
        ] {
            let error = persist_custom_client(&db, &creds, provider, Some("new"), change, |id| {
                write_client_id(&db, provider, id)
            })
            .await
            .unwrap_err();
            assert!(!error.contains("private-backend-detail"));
            assert!(!error.contains("new-secret"));
            assert_eq!(
                creds.get(&custom_secret_slot().unwrap()).unwrap().expose(),
                "old-secret"
            );
            assert_eq!(
                settings::get::<String>(db.pool(), client_id_key(provider))
                    .await
                    .unwrap()
                    .as_deref(),
                Some("old")
            );
        }
    }

    #[tokio::test]
    async fn calendar_runtime_resolves_custom_clients_without_exposing_secrets() {
        let db = Db::open_in_memory().await.unwrap();
        let credentials = Arc::new(kw_security::MemoryCredentialStore::new());
        let runtime = CalendarRuntime::new(
            db.clone(),
            credentials.clone(),
            NetworkPolicy::new().unwrap(),
        );
        for provider in [CalendarProvider::Google, CalendarProvider::Microsoft] {
            persist_custom_client(
                &db,
                credentials.as_ref(),
                provider,
                Some("custom-id"),
                SecretChange::Keep,
                |id| write_client_id(&db, provider, id),
            )
            .await
            .unwrap();
            let client = runtime.client(provider).await.unwrap();
            assert_eq!(client.client_id(), "custom-id");
            assert_eq!(client.kind(), provider.kind());
            assert!(client.client_secret().is_none());
            let status = provider_status(&db, credentials.as_ref(), provider)
                .await
                .unwrap();
            assert_eq!(status.client_source, ClientSource::Custom);
            assert!(status.client_id_configured);
            assert_eq!(status.custom_client_id.as_deref(), Some("custom-id"));
            assert!(!status.custom_google_secret_configured);
        }
        persist_custom_client(
            &db,
            credentials.as_ref(),
            CalendarProvider::Google,
            Some("custom-id"),
            SecretChange::Set {
                value: "custom-secret".into(),
            },
            |id| write_client_id(&db, CalendarProvider::Google, id),
        )
        .await
        .unwrap();
        let client = runtime.client(CalendarProvider::Google).await.unwrap();
        assert_eq!(client.client_secret().unwrap().expose(), "custom-secret");
        for provider in [CalendarProvider::Google, CalendarProvider::Microsoft] {
            let status = provider_status(&db, credentials.as_ref(), provider)
                .await
                .unwrap();
            assert!(!serde_json::to_string(&status)
                .unwrap()
                .contains("custom-secret"));
            assert_eq!(
                status.custom_google_secret_configured,
                provider == CalendarProvider::Google
            );
        }
    }
}
