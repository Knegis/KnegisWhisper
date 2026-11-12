# Calendar provider setup

Owner runbook for app-owned one-click Connect in Settings > Calendar, dated 2026-11-11.
No client has been registered, no real account has connected and no build has carried these
values. Evidence so far is unit tests and dev-fixture screenshots. Registration and a real
consent round trip remain owner work; do not mark R1/R2 verified before a bundled build
completes real consent.

## Portal instructions and evidence

The portal steps below were taken from the providers' documentation read during planning on
2026-11-11. The orchestrator confirmed that all 13 linked pages resolved that day. None of these
steps has been exercised against a real portal or account. The owner should re-check the linked
pages before registering.

## Google Cloud

1. Create or select the project's Google Cloud project. Enable the **Google Calendar API**.
2. In Google Auth Platform, configure **Branding**: app name, support contact, homepage,
   privacy-policy URL and authorized domains. Use an owner-controlled verified domain for
   the homepage and privacy policy; complete domain ownership verification before requesting
   OAuth verification. The public homepage must describe the app and link to its privacy policy.
3. Set **Audience** to **External**. Publish to **In production** for rollout. External apps
   left in Testing have refresh grants expire after 7 days for these calendar scopes; adding
   test users is not a durable release setup.
4. Configure Data Access for exactly these read-only scopes:
   - `https://www.googleapis.com/auth/calendar.calendarlist.readonly`
   - `https://www.googleapis.com/auth/calendar.events.readonly`
5. Create an OAuth client with application type **Desktop app**, not Web application.
   Retain the client ID and desktop client secret for the build configuration below. The app
   uses PKCE with an ephemeral-port IPv4 loopback callback at `/calendar/callback`.
6. Until verification, expect the unverified-app warning and the 100-user cap for unapproved
   sensitive scopes. In production does not itself mean verified or remove that cap. Submit
   for verification with the verified-domain homepage, privacy policy, branding and scope
   justification. Record Google's actual result before announcing broad availability.

The desktop secret is a public build value: it is extractable from a distributed binary and
provides no installation authentication or protection for user data. Rust sends it only on token
and refresh grants. Do not put it into a frontend variable, a log, settings or this document.

Official references for owner re-check:

- [Calendar API quickstart and project setup](https://developers.google.com/workspace/calendar/api/quickstart/python)
- [OAuth for desktop apps and loopback redirects](https://developers.google.com/identity/protocols/oauth2/native-app)
- [OAuth token expiration](https://developers.google.com/identity/protocols/oauth2#expiration)
- [Unverified apps and user cap](https://developers.google.com/identity/protocols/oauth2/production-readiness/brand-verification)
- [Sensitive-scope verification and application requirements](https://developers.google.com/identity/protocols/oauth2/production-readiness/sensitive-scope-verification)

## Microsoft Entra

1. Create an app registration with supported account types **Accounts in any organizational
   directory and personal Microsoft accounts**. This covers Outlook.com/Hotmail and Microsoft
   365 work/school accounts. Retain its Application (client) ID.
2. Under Authentication, configure the **Mobile and desktop applications** platform. Add
   `http://127.0.0.1/calendar/callback` through the manifest's `replyUrlsWithType`, with type
   `InstalledClient` (merge into the existing list; do not replace unrelated entries):

   ```json
   {
     "replyUrlsWithType": [
       {
         "url": "http://127.0.0.1/calendar/callback",
         "type": "InstalledClient"
       }
     ]
   }
   ```

   Loopback redirect ports are ignored for matching, so the app can bind an available ephemeral
   port. Preserve the IPv4 loopback address and callback path. If the current portal exposes a
   different manifest schema, consult the redirect-URI documentation before saving.
3. Enable **Allow public client flows**. Configure Microsoft Graph **delegated** permissions
   `Calendars.Read` and `offline_access`. Do not add application permissions or a client secret.
   The app uses the common authorization endpoint and `prompt=select_account`.
4. Complete publisher verification with the organization's eligible verified publisher account.
   Publisher verification identifies the publisher; it does not override a tenant's consent rules.
5. Check the work/school tenant's user-consent policy. A tenant may block user consent, require
   a verified publisher, or require administrator consent even for delegated permissions that
   normally permit user consent. Have an authorized tenant administrator grant consent if
   required. Record unreachable tenant-policy cases as untested; do not promise universal
   work-account access.

Official references for owner re-check:

- [Register an application and select supported account types](https://learn.microsoft.com/en-us/entra/identity-platform/quickstart-register-app)
- [Desktop app registration](https://learn.microsoft.com/en-us/entra/identity-platform/scenario-desktop-app-registration)
- [Redirect URI restrictions, loopback ports and manifest configuration](https://learn.microsoft.com/en-us/entra/identity-platform/reply-url)
- [Microsoft Graph delegated permissions](https://learn.microsoft.com/en-us/graph/permissions-reference#calendarsread)
- [Offline access](https://learn.microsoft.com/en-us/entra/identity-platform/scopes-oidc#the-offline_access-scope)
- [Publisher verification](https://learn.microsoft.com/en-us/entra/identity-platform/publisher-verification-overview)
- [Tenant user-consent settings](https://learn.microsoft.com/en-us/entra/identity/enterprise-apps/configure-user-consent)
- [Grant tenant-wide admin consent](https://learn.microsoft.com/en-us/entra/identity/enterprise-apps/grant-admin-consent)

## Build values

Only placeholders appear here. Supply real values through the shell or repository configuration,
never checked-in files. Rust reads these with `option_env!` in
`apps/desktop/src-tauri/src/calendar_clients.rs`; changing them requires rebuilding.
`build.rs` emits `cargo:rerun-if-env-changed` for each without printing values.

| Name | Local shell | GitHub repository configuration |
| --- | --- | --- |
| `KW_GOOGLE_CALENDAR_CLIENT_ID` | Google Desktop app ID | Actions variable |
| `KW_GOOGLE_CALENDAR_CLIENT_SECRET` | Google public desktop secret | Actions secret (masked in logs, public in binary) |
| `KW_MICROSOFT_CALENDAR_CLIENT_ID` | Microsoft Application (client) ID | Actions variable |

Google is available only with both non-blank values; Microsoft requires only its ID.
There are no `VITE_` equivalents; the frontend never receives the Google secret.

For local PowerShell, set values before starting dev and clean up the environment when it exits:

```powershell
$env:KW_GOOGLE_CALENDAR_CLIENT_ID = '<GOOGLE_DESKTOP_CLIENT_ID>'
$env:KW_GOOGLE_CALENDAR_CLIENT_SECRET = '<GOOGLE_PUBLIC_DESKTOP_CLIENT_SECRET>'
$env:KW_MICROSOFT_CALENDAR_CLIENT_ID = '<MICROSOFT_APPLICATION_CLIENT_ID>'
try {
    npm run tauri:dev
} finally {
    Remove-Item Env:KW_GOOGLE_CALENDAR_CLIENT_ID -ErrorAction SilentlyContinue
    Remove-Item Env:KW_GOOGLE_CALENDAR_CLIENT_SECRET -ErrorAction SilentlyContinue
    Remove-Item Env:KW_MICROSOFT_CALENDAR_CLIENT_ID -ErrorAction SilentlyContinue
}
```

On PowerShell installations that block the npm script shim, use `npm.cmd run tauri:dev`.
Clearing the shell does not remove values already compiled into a binary; rebuild to remove them.

In GitHub Settings > Secrets and variables > Actions, add the two ID variables and the Google
secret above. `.github/workflows/release.yml` passes them to `tauri:build` and experimental
bundles. `tools/release/release.py preflight` reports provider availability; its calendar-client
gate fails only when workflow input `require_calendar_clients` is true (default false).
`prepare` writes whether one-click calendars are included into release notes. Once registration
is done, the owner sets `require_calendar_clients` to true for release runs. Presence is not
proof of correct registration or real consent.

Forks and builds without values fall back to Advanced custom-client setup and its explanation.
Resolution is Custom > App-owned > Missing. A custom Google secret is stored only in credential
slot `calendar-google-custom-client`; Microsoft has no secret. Existing custom client IDs remain
Custom. Changing any issuing client ID requires reconnecting its grants.

## Rollout and compatibility

With all three values a build carries both one-click providers; without them it uses Advanced.
Partial configuration carries only the available provider. Owner registration and R12 are the
remaining gates for real-account claims. The connection lifecycle, snapshot sync, local readers
and Settings > Calendar are built and fixture-tested. Meetings Upcoming/event picker and the
recording link are also built and fixture-tested, still unverified against real accounts. R12
checks cover these features as well.

There is **no database migration**. `notes.calendar_event_id`, `notes.participants` and all
`calendar_events` columns already exist. `TokenBundle.client_id` and
`SyncRecord.last_success_at` default on read; `MeetingPlan.calendar` also defaults on
read. Grants lacking a matching client binding show reconnect-required in the UI.
Old cursors are ignored and written NULL. The snapshot version in
`calendar.<provider>.lastSync` hides pre-upgrade event rows until the first new-style sync purges
that account's old caches, or reconnect clears them.

The snapshot window is [now - 24 hours, now + 30 days). Google uses `singleEvents`, `timeMin`
and `timeMax`; Outlook uses Graph v1.0 `calendarView` with UTC preference. Declined events are
excluded, times normalized to UTC and each calendar committed atomically with overlap pruning.
A page failure keeps that calendar's cache. Sync runs on Sync now or Meetings opening (automatic
attempts at most once per 5 minutes per provider), never launch, focus or timer. Local readers
and the payload-free `calendar-changed` event do not fetch from a provider.

## Rollback

Turning off app-owned clients means removing build values and rebuilding. Without a Custom
client, resolution becomes Missing; the rest of the calendar implementation remains in place.

Reverting code or installing an older binary is a separate operation:

- The older binary opens the same database because there is no migration. Its delta code
  ignores NULL cursors and does a full fetch. Grants issued to app-owned clients require
  reconnecting with a custom client.
- **Before any downgrade, stop or recover every pending recording with the new build.** The
  recording link is built and fixture-tested, still unverified against real accounts. An older
  binary reading its sidecar drops `calendar`
  and saves without the linked `calendar_event_id` and participants. Recording and title survive.
- Test the downgrade on a copy of a profile before calling it safe. This has not been exercised.
- Changing an app-owned client ID requires reconnecting every grant it issued.

## R12: owner real-account checklist

Pending for each of **Google**, **personal Outlook.com**, and **Microsoft 365 work** accounts.
Extends KNEG-82 after provider registration. Record **build, date, account kind and result** for
each check, including failed, blocked and untested outcomes. Never record addresses, tokens,
authorization codes, PKCE verifiers or transcript text. The following blank template is not run
evidence:

| Build | Date | Account kind | Check | Result |
| --- | --- | --- | --- | --- |
| <BUILD> | <DATE> | <GOOGLE / PERSONAL OUTLOOK / MICROSOFT 365 WORK> | <CHECK NUMBER> | <RESULT OR UNTESTED> |

1. [ ] Connect with no Advanced values; confirm the returned calendar list is correct.
2. [ ] Select calendars, sync and see Upcoming, including a recurring occurrence on a
   non-default calendar. Upcoming/event-picker UI is built and fixture-tested, still unverified
   against real accounts.
3. [ ] Re-enter Meetings within and after 5 minutes: verify throttling and Sync now. Verify
   no requests at launch, on focus or while idle; there is no timer.
4. [ ] Refresh after real access-token expiry, including Microsoft refresh-token rotation.
5. [ ] Record against an event using the built, fixture-tested recording link, still unverified
   against real accounts: default title when none was typed, attendees as participants and
   `notes.calendar_event_id`. Check overlap without
   guessing, Don't link, change/remove while recording and Recover. Verify Stop,
   discard-with-notes, local API stop and Recover all save the chosen link snapshot.
6. [ ] Interrupt a sync part-way: the affected calendar keeps its cache, the report is partial
   and recording is unaffected.
7. [ ] Disconnect and connect a different account; check replacement, leftover Remove and
   cleanup Retry if reachable.
8. [ ] Deny, cancel, time out and exercise any reachable policy screen. Record an untested
   tenant-policy case explicitly as untested.
9. [ ] Confirm the Apple Calendar row still reads Unsupported.
