/**
 * Integrations > Calendar — one-click Google Calendar and Outlook, plus Apple's
 * unimplemented EventKit state. A build may carry KnegisWhisper's OAuth clients;
 * a custom client under Advanced overrides them. Without a built-in client,
 * the person must supply one. Google secrets stay in the credential store:
 * this screen reads only whether a custom secret is saved, never its value.
 *
 * Honesty rules this screen follows, matching the brief:
 *
 * - A provider is never shown as connected on the strength of a client ID
 *   being configured, or a credential merely existing — see
 *   `tokenStatusLabel` and `TokenStatus::Missing` in `calendar.rs`.
 * - An empty calendar list is never presented the same way as a fetch that
 *   failed. `lastSync` names which one happened, in words, every time —
 *   `syncOutcomeLabel` never lets a `completed` outcome of zero events read
 *   as silence about a `failed` one.
 * - Apple's row shows the reason EventKit is unimplemented, not an empty
 *   calendar list and not nothing.
 *
 * In plain words (KNEG-343): what to do stays in view; client details are under
 * Advanced, and token mechanics and provider errors under Learn more. Times
 * are shown in your locale, not as stored. While a connect waits for the
 * browser, a single delayed local read discovers its sign-in link; no network
 * request is made on a timer. Cancel remains available during that wait.
 */
import { useCallback, useEffect, useRef, useState } from "react";

import { FailureLine } from "../components/FailureLine";
import { LearnMore } from "../components/LearnMore";
import { Row } from "../components/Row";
import { useSettingsDraft } from "../components/SettingsDrafts";
import {
  cancelCalendarConnect,
  connectCalendar,
  disconnectCalendar,
  getCalendarStatus,
  removeLeftoverCalendar,
  retryCalendarCleanup,
  setCustomCalendarClient,
  setCalendarSelected,
  syncCalendar,
  type CalendarProblem,
  type CalendarProvider,
  type ProviderStatus,
  type SecretChange,
} from "../lib/ipc";
import {
  calendarCleanupLabel,
  calendarClientLabel,
  calendarLeftoverLabel,
  calendarTroubleshootingLabel,
} from "../lib/calendarLabels";
import {
  appleCalendarSupportLabel,
  connectOutcomeLabel,
  syncOutcomeLabel,
  tokenStatusLabel,
  type Plain,
} from "../lib/labels";
import { useRefresh } from "../lib/useRefresh";

const PROVIDERS: { id: CalendarProvider; label: string }[] = [
  { id: "google", label: "Google Calendar" },
  { id: "microsoft", label: "Outlook" },
];

/** A stored time as this computer's locale shows it, or as stored if it is not one. */
function when(stored: string): string {
  const date = new Date(stored);
  return Number.isNaN(date.getTime())
    ? stored
    : date.toLocaleString(undefined, { dateStyle: "medium", timeStyle: "short" });
}

/** A line and, under it, what is behind it. */
function Said({ line, className = "muted" }: { line: Plain; className?: string }) {
  return (
    <>
      <p className={className}>{line.text}</p>
      {line.more ? <LearnMore summary="Details"><p>{line.more}</p></LearnMore> : null}
    </>
  );
}

function ProviderSection({ provider, label, status, reload }: {
  provider: CalendarProvider;
  label: string;
  status: ProviderStatus;
  reload: () => Promise<void>;
}) {
  // Untouched fields follow the saved status. A secret draft is only the
  // person's new input: a saved secret can never be filled into this form.
  const [draft, setDraft] = useState<string | null>(null);
  const [secret, setSecret] = useState<SecretChange>({ kind: "keep" });
  const [busy, setBusy] = useState(false);
  const [saving, setSaving] = useState(false);
  const [connectInFlight, setConnectInFlight] = useState(false);
  const [cancelling, setCancelling] = useState(false);
  const [copied, setCopied] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [note, setNote] = useState<Plain | null>(null);
  const [problem, setProblem] = useState<CalendarProblem | null>(null);
  const held = useRef(false);
  const cancelHeld = useRef(false);
  const clientId = draft ?? status.customClientId ?? "";
  const dirty = clientId.trim() !== (status.customClientId ?? "") || secret.kind !== "keep";
  const connecting = connectInFlight || status.operation === "connecting";
  const disabled = busy || saving || status.operation !== "idle";
  const pendingSignIn = status.pendingSignIn;

  const discardClient = useCallback(() => {
    setDraft(null);
    setSecret({ kind: "keep" });
  }, []);
  const saveClient = useCallback(async () => {
    setSaving(true);
    try {
      await setCustomCalendarClient(provider, clientId.trim() || null, secret);
      await reload();
      discardClient();
    } finally {
      setSaving(false);
    }
  }, [provider, clientId, secret, reload, discardClient]);
  const clientDraft = useSettingsDraft({ label: `${label} custom client`, dirty, save: saveClient, discard: discardClient });

  const run = useCallback(async (work: () => Promise<void>) => {
    if (held.current) return;
    held.current = true;
    setBusy(true);
    setError(null);
    try {
      await work();
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      held.current = false;
      setBusy(false);
    }
  }, []);

  // Connect waits up to five minutes. Read its browser result once after it
  // starts, and cancel this local read if the call finishes or the view leaves.
  useEffect(() => {
    if (!connectInFlight) return;
    const timer = window.setTimeout(() => { void reload(); }, 1000);
    return () => window.clearTimeout(timer);
  }, [connectInFlight, reload]);

  const startConnect = () => {
    void run(async () => {
      if (dirty) await clientDraft.run(saveClient);
      setNote(null);
      setProblem(null);
      setCopied(false);
      setConnectInFlight(true);
      try {
        const outcome = await connectCalendar(provider);
        setNote(connectOutcomeLabel(outcome));
        setProblem(outcome.state === "failed" ? outcome.problem : null);
      } finally {
        await reload();
        setConnectInFlight(false);
      }
    });
  };
  const account = status.account;
  const token = account ? tokenStatusLabel(account.token) : null;
  const lastOutcome = account?.lastSync?.outcome;
  const lastSync = lastOutcome ? syncOutcomeLabel(lastOutcome) : null;
  const lastSyncProblem = lastOutcome && (lastOutcome.state === "failed" || lastOutcome.state === "token-unavailable") ? lastOutcome.problem : null;
  const reconnect = account?.token.state === "reconnect-required"
    || problem?.kind === "reconnect-required" || lastSyncProblem?.kind === "reconnect-required";
  const needsSignIn = !account || reconnect || (account.token.state !== "active" && account.token.state !== "needs-refresh");
  const timedOut = problem?.kind === "timed-out";

  return (
    <section data-calendar-provider={provider}>
      <h2>{label.toLowerCase()}</h2>
      {provider === "microsoft" ? <p className="muted">Outlook.com, Hotmail and Microsoft 365</p> : null}
      {status.clientSource === "missing" ? <p className="muted">{calendarClientLabel(provider, status.clientSource)}</p> : null}

      {connecting ? (
        <>
          <p role="status">Finish signing in in your browser</p>
          {pendingSignIn?.browserOpened === false ? (
            <p className="caveat">Your browser didn't open. Copy the sign-in link into your browser.</p>
          ) : null}
          <p className="muted">{calendarTroubleshootingLabel(provider)}</p>
          <div className="action-row calendar-actions">
            <button type="button" disabled={cancelling} onClick={() => {
              if (cancelHeld.current) return;
              cancelHeld.current = true;
              setCancelling(true);
              setError(null);
              void cancelCalendarConnect(provider).then(reload).catch(cause => {
                setError(cause instanceof Error ? cause.message : String(cause));
              }).finally(() => { cancelHeld.current = false; setCancelling(false); });
            }}>Cancel</button>
            {pendingSignIn ? <button type="button" onClick={() => {
              void navigator.clipboard.writeText(pendingSignIn.authorizeUrl).then(() => {
                setCopied(true);
              }).catch(cause => setError(cause instanceof Error ? cause.message : String(cause)));
            }}>Copy sign-in link</button> : null}
          </div>
          {copied ? <p className="muted" role="status">Sign-in link copied.</p> : null}
        </>
      ) : (
        <>
          {reconnect ? <p className="caveat">A connection imported from OpenWhispr, or made with a different client, has to be made again here.</p> : null}
          {timedOut ? <p className="muted">{calendarTroubleshootingLabel(provider)}</p> : null}
          {needsSignIn || timedOut ? <div className="action-row calendar-actions">
            <button type="button" className="action-iris" disabled={disabled || status.clientSource === "missing"} onClick={startConnect}>
              {timedOut ? "Retry" : reconnect || account ? "Reconnect" : "Connect"}
            </button>
          </div> : null}
        </>
      )}

      {account && token ? (
        <>
          <Row label="Account" value={account.email} />
          <Row label="Connection" tone={token.text === "Connected" ? "ok" : "off"} value={token.text} detail={token.detail || undefined}>
            {token.more ? <LearnMore summary="Details"><p>{token.more}</p></LearnMore> : null}
          </Row>
          <Row label="Last sync" value={lastSync ? lastSync.text : "Never synced"}
            detail={account.lastSync ? <span className="vocabulary-count">{when(account.lastSync.at)}</span> : undefined}>
            {lastSync?.more ? <LearnMore summary="Details"><p>{lastSync.more}</p></LearnMore> : null}
          </Row>
          {account.calendars.length === 0 ? <p className="muted">No calendars have been read yet.</p> : (
            <div className="calendar-cards">
              {account.calendars.map(cal => (
                <div key={cal.id} className="calendar-card">
                  <span className="calendar-card-name">{cal.summary}{cal.isPrimary ? " (primary)" : ""}</span>
                  <span className="calendar-card-count" data-empty={cal.eventCount === 0} aria-hidden="true">{cal.eventCount}</span>
                  <em className="row-detail">{cal.eventCount} event(s) stored</em>
                  <label>
                    <input type="checkbox" checked={cal.isSelected} disabled={disabled} onChange={event => {
                      const selected = event.target.checked;
                      void run(async () => {
                        await clientDraft.run(async () => { await setCalendarSelected(provider, cal.id, selected); }, `calendar:${cal.id}`);
                        await reload();
                      });
                    }} />
                    synced
                  </label>
                </div>
              ))}
            </div>
          )}
          <div className="action-row calendar-actions">
            <button type="button" className="action-iris" disabled={disabled || needsSignIn} onClick={() => {
              void run(async () => {
                const outcome = await syncCalendar(provider);
                setNote(syncOutcomeLabel(outcome));
                setProblem(outcome.state === "failed" || outcome.state === "token-unavailable" ? outcome.problem : null);
                await reload();
              });
            }}>{status.operation === "syncing" ? "Syncing…" : "Sync now"}</button>
            <button type="button" disabled={disabled} onClick={() => {
              void run(async () => {
                await disconnectCalendar(provider);
                setNote(null);
                setProblem(null);
                await reload();
              });
            }}>Disconnect</button>
          </div>
        </>
      ) : null}

      {status.leftover.map(leftover => <div className="row" key={leftover.email}>
        <span className="row-label">{calendarLeftoverLabel(leftover.origin)}</span>
        <div className="row-value">
          <span>{leftover.email}</span>
          <button type="button" disabled={disabled} aria-label={`Remove ${leftover.email}`} onClick={() => {
            void run(async () => { await removeLeftoverCalendar(provider, leftover.email); await reload(); });
          }}>Remove</button>
        </div>
      </div>)}
      {status.cleanupPending > 0 ? <div className="action-row">
        <p className="caveat">{calendarCleanupLabel(status.cleanupPending)}</p>
        <button type="button" disabled={disabled} aria-label={`Retry ${label} credential cleanup`} onClick={() => {
          void run(async () => { await retryCalendarCleanup(provider); await reload(); });
        }}>Retry</button>
      </div> : null}
      {error ? <FailureLine text="That didn't work." detail={error} /> : null}
      {note ? <Said line={note} className="caveat" /> : null}

      <LearnMore summary="Advanced" unfold={status.clientSource === "missing"}>
        {status.clientSource !== "missing" ? <p className="muted">{calendarClientLabel(provider, status.clientSource)}</p> : null}
        <p>{status.appClientAvailable
          ? "Use your own desktop app client, or clear the Client ID to return to this build's built-in client."
          : "This build needs your own desktop app client."}</p>
        {clientDraft.feedback}
        <div className="row">
          <span className="row-label">Client ID</span>
          <div className="row-value">
            <input type="text" aria-label={`${label} client ID`} placeholder="Your own client ID" value={clientId}
              disabled={disabled} onChange={event => setDraft(event.target.value)} />
          </div>
        </div>
        {provider === "google" ? <div className="row">
          <span className="row-label">Client secret</span>
          <div className="row-value">
            <input type="password" aria-label="Google Calendar client secret" autoComplete="new-password"
              placeholder={status.customGoogleSecretConfigured ? "Saved" : "Client secret"}
              value={secret.kind === "set" ? secret.value : ""} disabled={disabled}
              onChange={event => setSecret(event.target.value ? { kind: "set", value: event.target.value } : { kind: "keep" })} />
            {status.customGoogleSecretConfigured && secret.kind === "keep" ? <em className="row-detail">Saved</em> : null}
            {secret.kind === "clear" ? <em className="row-detail">Secret will be removed when you save.</em> : null}
            <button type="button" disabled={disabled || secret.kind === "clear" || (!status.customGoogleSecretConfigured && secret.kind !== "set")}
              onClick={() => setSecret({ kind: "clear" })}>Remove secret</button>
          </div>
        </div> : null}
        <div className="action-row">
          <button type="button" disabled={disabled || !dirty} onClick={() => { void run(() => clientDraft.run(saveClient)); }}>Save changes</button>
          <button type="button" disabled={disabled || !dirty} onClick={discardClient}>Discard changes</button>
        </div>
      </LearnMore>

    </section>
  );
}

export function IntegrationsCalendar() {
  const load = useCallback(() => getCalendarStatus(), []);
  const { data: status, error, reload } = useRefresh(load);
  if (!status) {
    return <div className="view" data-surface="integrations.calendar">
      {error ? <FailureLine text="This page couldn't load." detail={error} /> : <p className="muted">Loading…</p>}
    </div>;
  }
  const apple = appleCalendarSupportLabel(status.apple);
  return (
    <div className="view" data-surface="integrations.calendar">
      {error ? <FailureLine text="That didn't work." detail={error} /> : null}
      {PROVIDERS.map(({ id, label }) => <ProviderSection key={id} provider={id} label={label}
        status={id === "google" ? status.google : status.microsoft} reload={reload} />)}
      <section>
        <h2>apple calendar</h2>
        <Row label="State" value={apple.text}>
          <LearnMore summary="Why"><p>{apple.detail}</p></LearnMore>
        </Row>
      </section>
    </div>
  );
}
