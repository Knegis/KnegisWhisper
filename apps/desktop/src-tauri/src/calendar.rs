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
//! # Honesty
//!
//! [`calendar_status`] never claims a provider is connected on the strength
//! of a credential merely being present. [`token_status`] checks the
//! credential store directly rather than trusting the database row alone --
//! see its doc comment -- and it makes no network call, matching every other
//! status command in this application (`meeting_status`, `credential_slots`;
//! see `docs/NETWORK_BOUNDARY.md`). The two commands that do reach the
//! network, [`calendar_connect`] and [`calendar_sync`], are both explicit
//! clicks. A sync's outcome -- including a failure -- is persisted to
//! settings and folded back into the next `calendar_status` read, so a
//! failed sync is never silently indistinguishable from an empty, successful
//! one, and an expired token that could not be refreshed says so rather than
//! showing stale rows as current.

use std::sync::Arc;
use std::time::Duration;

use kw_calendar_core::{
    apple, begin_authorization, calendars_repo, complete_authorization, credentials,
    ensure_fresh_access_token, events_repo, google, microsoft, store_tokens, tokens_repo,
    CalendarError, CalendarProviderKind, LoopbackReceiver, NetworkPolicySender, ProviderEndpoints,
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

/* -------------------------------------------------------------- the runtime */

/// What connecting an account, or syncing one, needs: the database,
/// credential store, and network boundary the rest of the application
/// already built, plus a guard against two calendar operations running at
/// once.
pub struct CalendarRuntime {
    db: Db,
    credentials: Arc<dyn CredentialStore>,
    policy: NetworkPolicy,
    /// One calendar network operation (a connect or a sync) at a time.
    /// `try_lock` rather than `lock`, because a second click while one is
    /// already running should be refused immediately, not queued -- a
    /// queued second `connect` would open a second browser tab for the same
    /// flow.
    busy: tokio::sync::Mutex<()>,
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
            busy: tokio::sync::Mutex::new(()),
        }
    }

    async fn client_id(&self, provider: CalendarProvider) -> CmdResult<String> {
        let raw = settings::get::<String>(self.db.pool(), client_id_key(provider))
            .await
            .map_err(|e| fail("could not read the client ID", e))?
            .unwrap_or_default();
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Err(format!(
                "no client ID is set for {}. Register your own OAuth client and paste its ID \
                 in on this screen first.",
                provider.name()
            ));
        }
        Ok(trimmed.to_owned())
    }

    /// The account currently on file as connected for `provider`, if any.
    ///
    /// A read failure is propagated as `Err`, never folded into `Ok(None)`.
    /// `connect` uses this to decide whether to disconnect a previously
    /// connected account before storing a new one; treating a failed read
    /// the same as "nothing was connected" would skip that disconnect
    /// silently, leaving a different account's refresh token live in the OS
    /// credential store with no way for `calendar_status` to ever find it
    /// again once `account_key` is overwritten.
    async fn read_previous_account(&self, provider: CalendarProvider) -> CmdResult<Option<String>> {
        settings::get::<String>(self.db.pool(), account_key(provider))
            .await
            .map_err(|e| fail("could not read the previously connected account", e))
    }

    /// Run the loopback OAuth flow, discover which account it belongs to
    /// from the provider's own calendar list, and store the result --
    /// replacing whichever account was previously connected for this
    /// provider, if it was a different one.
    pub async fn connect(&self, provider: CalendarProvider) -> CmdResult<ConnectOutcome> {
        let Ok(_guard) = self.busy.try_lock() else {
            return Err("a calendar connection or sync is already in progress".to_owned());
        };

        let client_id = self.client_id(provider).await?;
        let endpoints = provider.endpoints();

        let receiver = LoopbackReceiver::bind()
            .await
            .map_err(|e| format!("could not open a local port to receive the redirect: {e}"))?;
        let redirect_uri = receiver.redirect_uri(CALLBACK_PATH);

        let pending = begin_authorization(&endpoints, &client_id, &redirect_uri)
            .map_err(|e| e.to_string())?;

        if let Err(err) = open_in_browser(&pending.authorize_url) {
            return Err(format!(
                "could not open your browser automatically ({err}). Open this address \
                 yourself to finish connecting {}: {}",
                provider.name(),
                pending.authorize_url
            ));
        }

        let callback = receiver
            .receive(REDIRECT_WAIT)
            .await
            .map_err(|e| format!("waiting for the browser to come back: {e}"))?;

        let sender = NetworkPolicySender::new(&self.policy);
        let tokens = match complete_authorization(
            &pending, &callback, &endpoints, &client_id, &sender,
        )
        .await
        {
            Ok(tokens) => tokens,
            Err(CalendarError::AuthorizationDenied(reason)) => {
                return Ok(ConnectOutcome::Denied { reason });
            }
            Err(err) => return Err(err.to_string()),
        };

        let (account_email, calendar_count) = match provider {
            CalendarProvider::Google => self.finish_connect_google(&tokens.access_token).await?,
            CalendarProvider::Microsoft => {
                self.finish_connect_microsoft(&tokens.access_token).await?
            }
        };

        // A read failure here must not be treated as "nothing was
        // previously connected" -- that would silently skip disconnecting a
        // different account, leaving its refresh token live in the OS
        // credential store with no row anywhere pointing at it once
        // `account_key` is overwritten below. See `read_previous_account`.
        if let Some(previous) = self.read_previous_account(provider).await? {
            if previous != account_email {
                self.disconnect(provider, &previous).await?;
            }
        }

        store_tokens(
            self.db.pool(),
            self.credentials.as_ref(),
            provider.kind(),
            &account_email,
            &tokens,
        )
        .await
        .map_err(|e| e.to_string())?;

        settings::set(self.db.pool(), account_key(provider), &account_email)
            .await
            .map_err(|e| fail("could not remember the connected account", e))?;

        Ok(ConnectOutcome::Connected {
            account_email,
            calendars: calendar_count,
        })
    }

    async fn finish_connect_google(&self, access_token: &Secret) -> CmdResult<(String, u32)> {
        let calendars = google::fetch_calendar_list(&self.policy, access_token)
            .await
            .map_err(|e| {
                format!("the token exchange succeeded, but the calendar list could not be read to identify the account: {e}")
            })?;
        let email = google::primary_email(&calendars)
            .ok_or_else(|| {
                "connected, but no primary calendar was found to identify the account".to_owned()
            })?
            .to_owned();

        for entry in &calendars {
            calendars_repo::upsert_google_calendar(self.db.pool(), &email, entry)
                .await
                .map_err(|e| fail("could not save the calendar list", e))?;
        }
        let keep_ids: Vec<String> = calendars.iter().map(|c| c.id.clone()).collect();
        calendars_repo::prune_calendars(
            self.db.pool(),
            CalendarProviderKind::Google,
            &email,
            &keep_ids,
        )
        .await
        .map_err(|e| fail("could not save the calendar list", e))?;

        Ok((email, u32::try_from(calendars.len()).unwrap_or(u32::MAX)))
    }

    async fn finish_connect_microsoft(&self, access_token: &Secret) -> CmdResult<(String, u32)> {
        let calendars = microsoft::fetch_calendars(&self.policy, access_token)
            .await
            .map_err(|e| {
                format!("the token exchange succeeded, but the calendar list could not be read to identify the account: {e}")
            })?;
        let email = microsoft::account_owner_email(&calendars)
            .ok_or_else(|| {
                "connected, but no default calendar was found to identify the account".to_owned()
            })?
            .to_owned();

        for entry in &calendars {
            calendars_repo::upsert_microsoft_calendar(self.db.pool(), &email, entry)
                .await
                .map_err(|e| fail("could not save the calendar list", e))?;
        }
        let keep_ids: Vec<String> = calendars.iter().map(|c| c.id.clone()).collect();
        calendars_repo::prune_calendars(
            self.db.pool(),
            CalendarProviderKind::Microsoft,
            &email,
            &keep_ids,
        )
        .await
        .map_err(|e| fail("could not save the calendar list", e))?;

        Ok((email, u32::try_from(calendars.len()).unwrap_or(u32::MAX)))
    }

    /// Remove everything local to one connected account: the OS credential
    /// (best-effort -- a store that has already lost it is not a reason to
    /// refuse the rest of this), its calendars, and their events. The
    /// `google_calendar_tokens` / `microsoft_calendar_tokens` row itself is
    /// left in place; `kw_calendar_core::tokens_repo` has no delete
    /// function, deliberately not added for this task -- see the report.
    /// [`token_status`] reports it honestly regardless, because it checks
    /// the credential store rather than trusting that row alone.
    pub async fn disconnect(&self, provider: CalendarProvider, email: &str) -> CmdResult<()> {
        if let Some(key_ref) = credentials::token_ref_for(provider.kind(), email) {
            if let Err(err) = self.credentials.delete(&key_ref) {
                tracing::warn!(error = %err, "could not remove the stored calendar credential");
            }
        }

        let calendars = calendars_repo::list_calendars(self.db.pool(), provider.kind(), email)
            .await
            .map_err(|e| fail("could not read the calendar list", e))?;
        for calendar in &calendars {
            events_repo::delete_all_for_calendar(self.db.pool(), provider.wire_str(), &calendar.id)
                .await
                .map_err(|e| fail("could not remove the calendar's events", e))?;
        }
        calendars_repo::prune_calendars(self.db.pool(), provider.kind(), email, &[])
            .await
            .map_err(|e| fail("could not remove the calendars", e))?;

        settings::remove(self.db.pool(), account_key(provider))
            .await
            .map_err(|e| fail("could not clear the connected account", e))?;
        settings::remove(self.db.pool(), last_sync_key(provider))
            .await
            .map_err(|e| fail("could not clear the sync record", e))?;
        Ok(())
    }

    /// Refresh the calendar list and every selected calendar's events for
    /// whichever account is connected for `provider`.
    pub async fn sync(&self, provider: CalendarProvider) -> CmdResult<SyncOutcome> {
        let Ok(_guard) = self.busy.try_lock() else {
            return Err("a calendar connection or sync is already in progress".to_owned());
        };

        let Some(email) = settings::get::<String>(self.db.pool(), account_key(provider))
            .await
            .map_err(|e| fail("could not read the connected account", e))?
        else {
            return Err(format!("no {} account is connected", provider.name()));
        };
        let client_id = self.client_id(provider).await?;

        let endpoints = provider.endpoints();
        let sender = NetworkPolicySender::new(&self.policy);

        let access_token = match ensure_fresh_access_token(
            self.db.pool(),
            self.credentials.as_ref(),
            &sender,
            &endpoints,
            &client_id,
            provider.kind(),
            &email,
        )
        .await
        {
            Ok(token) => token,
            Err(err) => {
                let outcome = SyncOutcome::TokenUnavailable {
                    message: err.to_string(),
                };
                self.record_sync(provider, outcome.clone()).await;
                return Ok(outcome);
            }
        };

        let outcome = match provider {
            CalendarProvider::Google => self.sync_google(&email, &access_token).await,
            CalendarProvider::Microsoft => self.sync_microsoft(&email, &access_token).await,
        };
        self.record_sync(provider, outcome.clone()).await;
        Ok(outcome)
    }

    async fn sync_google(&self, email: &str, access_token: &Secret) -> SyncOutcome {
        let calendars = match google::fetch_calendar_list(&self.policy, access_token).await {
            Ok(list) => list,
            Err(err) => {
                return SyncOutcome::Failed {
                    message: err.to_string(),
                }
            }
        };
        for entry in &calendars {
            if let Err(err) =
                calendars_repo::upsert_google_calendar(self.db.pool(), email, entry).await
            {
                return SyncOutcome::Failed {
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
            // Matches `finish_connect_google`: a prune failure must not let
            // this report `Completed` while a calendar the provider no
            // longer lists is still sitting here locally.
            return SyncOutcome::Failed {
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
                    message: fail("could not read the calendar list", err),
                }
            }
        };

        let mut upserted = 0u32;
        let mut removed = 0u32;
        let mut errors = Vec::new();
        for row in rows.iter().filter(|r| r.is_selected) {
            match self
                .sync_google_events(&row.id, row.sync_token.as_deref(), access_token)
                .await
            {
                Ok((u, r)) => {
                    upserted += u;
                    removed += r;
                }
                Err(message) => errors.push(format!("{}: {message}", row.summary)),
            }
        }

        SyncOutcome::Completed {
            events_upserted: upserted,
            events_removed: removed,
            calendar_errors: errors,
        }
    }

    async fn sync_google_events(
        &self,
        calendar_id: &str,
        sync_token: Option<&str>,
        access_token: &Secret,
    ) -> Result<(u32, u32), String> {
        let fetch = match google::fetch_events(&self.policy, access_token, calendar_id, sync_token)
            .await
        {
            Ok(fetch) => fetch,
            Err(kw_calendar_core::SyncError::SyncTokenExpired) => {
                events_repo::delete_all_for_calendar(self.db.pool(), "google", calendar_id)
                    .await
                    .map_err(|e| fail("could not clear the calendar before a full resync", e))?;
                calendars_repo::set_sync_token(
                    self.db.pool(),
                    CalendarProviderKind::Google,
                    calendar_id,
                    None,
                )
                .await
                .map_err(|e| fail("could not clear the sync cursor", e))?;
                google::fetch_events(&self.policy, access_token, calendar_id, None)
                    .await
                    .map_err(|e| e.to_string())?
            }
            Err(err) => return Err(err.to_string()),
        };

        let mut upserted = 0u32;
        let mut removed = 0u32;
        for item in fetch.events {
            match item {
                google::SyncedEvent::Upsert(evt) => {
                    let row = events_repo::EventRow {
                        id: events_repo::row_id("google", calendar_id, &evt.id),
                        calendar_id: calendar_id.to_owned(),
                        provider: "google",
                        summary: evt.summary,
                        start_time: evt.start_time,
                        end_time: evt.end_time,
                        is_all_day: evt.is_all_day,
                        status: evt.status,
                        hangout_link: evt.hangout_link,
                        conference_data: evt.conference_data,
                        organizer_email: evt.organizer_email,
                        attendees: evt.attendees,
                    };
                    events_repo::upsert_event(self.db.pool(), &row)
                        .await
                        .map_err(|e| fail("could not save an event", e))?;
                    upserted += 1;
                }
                google::SyncedEvent::Removed { id } => {
                    events_repo::delete_event(
                        self.db.pool(),
                        &events_repo::row_id("google", calendar_id, &id),
                    )
                    .await
                    .map_err(|e| fail("could not remove a cancelled event", e))?;
                    removed += 1;
                }
            }
        }

        if let Some(token) = &fetch.next_sync_token {
            calendars_repo::set_sync_token(
                self.db.pool(),
                CalendarProviderKind::Google,
                calendar_id,
                Some(token),
            )
            .await
            .map_err(|e| fail("could not save the sync cursor", e))?;
        }

        Ok((upserted, removed))
    }

    async fn sync_microsoft(&self, email: &str, access_token: &Secret) -> SyncOutcome {
        let calendars = match microsoft::fetch_calendars(&self.policy, access_token).await {
            Ok(list) => list,
            Err(err) => {
                return SyncOutcome::Failed {
                    message: err.to_string(),
                }
            }
        };
        for entry in &calendars {
            if let Err(err) =
                calendars_repo::upsert_microsoft_calendar(self.db.pool(), email, entry).await
            {
                return SyncOutcome::Failed {
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
            // Matches `finish_connect_microsoft`: a prune failure must not
            // let this report `Completed` while a calendar the provider no
            // longer lists is still sitting here locally.
            return SyncOutcome::Failed {
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
                    message: fail("could not read the calendar list", err),
                }
            }
        };

        let mut upserted = 0u32;
        let mut removed = 0u32;
        let mut errors = Vec::new();
        for row in rows.iter().filter(|r| r.is_selected) {
            match self
                .sync_microsoft_events(&row.id, row.sync_token.as_deref(), access_token)
                .await
            {
                Ok((u, r)) => {
                    upserted += u;
                    removed += r;
                }
                Err(message) => errors.push(format!("{}: {message}", row.summary)),
            }
        }

        SyncOutcome::Completed {
            events_upserted: upserted,
            events_removed: removed,
            calendar_errors: errors,
        }
    }

    async fn sync_microsoft_events(
        &self,
        calendar_id: &str,
        delta_cursor: Option<&str>,
        access_token: &Secret,
    ) -> Result<(u32, u32), String> {
        let fetch = match microsoft::fetch_events_delta(
            &self.policy,
            access_token,
            calendar_id,
            delta_cursor,
        )
        .await
        {
            Ok(fetch) => fetch,
            Err(kw_calendar_core::SyncError::SyncTokenExpired) => {
                events_repo::delete_all_for_calendar(self.db.pool(), "microsoft", calendar_id)
                    .await
                    .map_err(|e| fail("could not clear the calendar before a full resync", e))?;
                calendars_repo::set_sync_token(
                    self.db.pool(),
                    CalendarProviderKind::Microsoft,
                    calendar_id,
                    None,
                )
                .await
                .map_err(|e| fail("could not clear the sync cursor", e))?;
                microsoft::fetch_events_delta(&self.policy, access_token, calendar_id, None)
                    .await
                    .map_err(|e| e.to_string())?
            }
            Err(err) => return Err(err.to_string()),
        };

        let mut upserted = 0u32;
        let mut removed = 0u32;
        for item in fetch.events {
            match item {
                microsoft::SyncedEvent::Upsert(evt) => {
                    let row = events_repo::EventRow {
                        id: events_repo::row_id("microsoft", calendar_id, &evt.id),
                        calendar_id: calendar_id.to_owned(),
                        provider: "microsoft",
                        summary: evt.summary,
                        start_time: evt.start_time,
                        end_time: evt.end_time,
                        is_all_day: evt.is_all_day,
                        status: evt.status,
                        hangout_link: evt.hangout_link,
                        conference_data: None,
                        organizer_email: evt.organizer_email,
                        attendees: evt.attendees,
                    };
                    events_repo::upsert_event(self.db.pool(), &row)
                        .await
                        .map_err(|e| fail("could not save an event", e))?;
                    upserted += 1;
                }
                microsoft::SyncedEvent::Removed { id } => {
                    events_repo::delete_event(
                        self.db.pool(),
                        &events_repo::row_id("microsoft", calendar_id, &id),
                    )
                    .await
                    .map_err(|e| fail("could not remove a deleted event", e))?;
                    removed += 1;
                }
            }
        }

        if let Some(link) = &fetch.delta_link {
            calendars_repo::set_sync_token(
                self.db.pool(),
                CalendarProviderKind::Microsoft,
                calendar_id,
                Some(link),
            )
            .await
            .map_err(|e| fail("could not save the sync cursor", e))?;
        }

        Ok((upserted, removed))
    }

    async fn record_sync(&self, provider: CalendarProvider, outcome: SyncOutcome) {
        let record = SyncRecord {
            at: chrono::Utc::now().to_rfc3339(),
            outcome,
        };
        if let Err(err) = settings::set(self.db.pool(), last_sync_key(provider), &record).await {
            tracing::warn!(error = %err, "could not record the calendar sync result");
        }
    }
}

/* --------------------------------------------------------------- the wire */

/// What happened when someone clicked "Connect".
///
/// Only the two outcomes that are not simply a refusal: a plain error (no
/// client ID configured, the loopback receiver could not bind, the token
/// exchange itself failed) crosses as `Err` the same as every other command,
/// because there is nothing more for the frontend to do with those than show
/// them. Being turned down at the consent screen is different -- an ordinary
/// outcome, not a bug -- so it gets its own state rather than the same
/// generic error text.
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
}

/// What one sync attempt found. Both the command's own return value and
/// what [`CalendarRuntime::record_sync`] persists to settings -- one shape
/// for both means the screen can never see a different answer from a sync
/// it just ran than from the record `calendar_status` reads back later.
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
    /// The token could not be resolved or refreshed. Distinguished from
    /// [`SyncOutcome::Failed`] because it names a specific, actionable fact
    /// -- reconnecting the account is the fix -- rather than an
    /// undifferentiated error.
    TokenUnavailable {
        message: String,
    },
    Failed {
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
}

/// Whether a token is on file and, if so, whether it is still within its
/// recorded expiry.
///
/// A local read, not a network probe -- reaching the network on every status
/// poll is exactly what `docs/NETWORK_BOUNDARY.md` rules out for a passive
/// status command. `Missing` covers both "never connected" and "the
/// credential is gone even though the database row is still there", which is
/// exactly the state [`CalendarRuntime::disconnect`] leaves behind -- see its
/// doc comment on why the row itself is not deleted. Checking the credential
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
        Some(SyncOutcome::TokenUnavailable { message }) => TokenStatus::RefreshFailed {
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
    pub client_id_configured: bool,
    /// The client ID itself, shown back rather than only "configured" or
    /// not. It is not a secret -- there is no client secret in this flow at
    /// all, see `kw_calendar_core`'s module doc -- so there is nothing to
    /// protect by hiding it, and showing it is what makes "you supplied
    /// this" a fact on screen rather than a claim in a comment.
    pub client_id: Option<String>,
    pub account: Option<AccountStatus>,
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
    let client_id = settings::get::<String>(db.pool(), client_id_key(provider))
        .await
        .map_err(|e| fail("could not read the client ID", e))?;
    let client_id_configured = client_id.as_deref().is_some_and(|s| !s.trim().is_empty());

    let account_email = settings::get::<String>(db.pool(), account_key(provider))
        .await
        .map_err(|e| fail("could not read the connected account", e))?;

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
        client_id_configured,
        client_id,
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

fn open_in_browser(url: &str) -> std::io::Result<()> {
    let (program, args) = browser_command(url);
    std::process::Command::new(program).args(args).spawn()?;
    Ok(())
}

/* ------------------------------------------------------------------ commands */

#[tauri::command]
pub async fn calendar_status(
    state: State<'_, crate::state::AppState>,
) -> CmdResult<CalendarStatus> {
    let google = provider_status(state.db(), state.credentials(), CalendarProvider::Google).await?;
    let microsoft =
        provider_status(state.db(), state.credentials(), CalendarProvider::Microsoft).await?;
    Ok(CalendarStatus {
        google,
        microsoft,
        apple: apple::support(),
    })
}

/// Save (or, given a blank value, clear) the user-supplied OAuth client ID
/// for `provider`. Never a secret -- see the module doc -- so this is a
/// plain settings write, the same shape as every other settings command.
#[tauri::command]
pub async fn calendar_set_client_id(
    state: State<'_, crate::state::AppState>,
    provider: CalendarProvider,
    client_id: String,
) -> CmdResult<()> {
    let trimmed = client_id.trim();
    if trimmed.is_empty() {
        return settings::remove(state.db().pool(), client_id_key(provider))
            .await
            .map_err(|e| fail("could not clear the client ID", e));
    }
    settings::set(state.db().pool(), client_id_key(provider), &trimmed)
        .await
        .map_err(|e| fail("could not save the client ID", e))
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
    let Some(email) = settings::get::<String>(runtime.db.pool(), account_key(provider))
        .await
        .map_err(|e| fail("could not read the connected account", e))?
    else {
        return Ok(());
    };
    runtime.disconnect(provider, &email).await
}

#[tauri::command]
pub async fn calendar_sync(
    runtime: State<'_, Arc<CalendarRuntime>>,
    provider: CalendarProvider,
) -> CmdResult<SyncOutcome> {
    runtime.sync(provider).await
}

/// Toggle whether one calendar is synced. Never touches the sync cursor --
/// see `kw_calendar_core::calendars_repo`'s module doc -- so switching a
/// calendar off and back on resumes from where it left off rather than
/// starting over.
#[tauri::command]
pub async fn calendar_set_selected(
    state: State<'_, crate::state::AppState>,
    provider: CalendarProvider,
    calendar_id: String,
    selected: bool,
) -> CmdResult<()> {
    calendars_repo::set_selected(state.db().pool(), provider.kind(), &calendar_id, selected)
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
        // The exact state `CalendarRuntime::disconnect` leaves behind: the
        // credential is gone, but the token_ref row was deliberately left in
        // place. A caller that only looked at the row's `expires_at` would
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
        let slot =
            credentials::token_ref_for(CalendarProviderKind::Google, "user@example.com").unwrap();
        creds
            .set(&slot, &kw_security::Secret::new("bundle".to_owned()))
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
        let slot =
            credentials::token_ref_for(CalendarProviderKind::Google, "user@example.com").unwrap();
        creds
            .set(&slot, &kw_security::Secret::new("bundle".to_owned()))
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
            at: "2026-01-01T00:00:00Z".to_owned(),
            outcome: SyncOutcome::TokenUnavailable {
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
            at: "2026-01-01T00:00:00Z".to_owned(),
            outcome: SyncOutcome::Failed {
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
}
