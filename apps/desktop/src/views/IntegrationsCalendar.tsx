/**
 * Integrations > Calendar — Google Calendar and Microsoft Graph, plus Apple's
 * unimplemented EventKit state.
 *
 * The one thing this screen exists to make visible: there is no embedded
 * OAuth client here, for either provider. Upstream shipped a Google client
 * secret in the built application, which in an open-source build is not a
 * secret at all. The trade this project makes instead — the user registers
 * their own OAuth client and pastes the client ID in below — costs setup
 * friction, and that cost is invisible unless this screen says so. It says
 * so before either provider's section, not once in a footnote.
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
 */

import { useCallback, useState } from "react";

import { Row } from "../components/Row";
import {
  connectCalendar,
  disconnectCalendar,
  getCalendarStatus,
  setCalendarClientId,
  setCalendarSelected,
  syncCalendar,
  type CalendarProvider,
  type ProviderStatus,
} from "../lib/ipc";
import {
  appleCalendarSupportLabel,
  connectOutcomeLabel,
  syncOutcomeLabel,
  tokenStatusLabel,
} from "../lib/labels";
import { useRefresh } from "../lib/useRefresh";

const PROVIDERS: { id: CalendarProvider; label: string }[] = [
  { id: "google", label: "Google Calendar" },
  { id: "microsoft", label: "Microsoft Calendar" },
];

function ProviderSection({
  provider,
  label,
  status,
  reload,
}: {
  provider: CalendarProvider;
  label: string;
  status: ProviderStatus;
  reload: () => Promise<void>;
}) {
  // `null` means "not yet touched" — the saved value is shown until the
  // user types something, the same idea `SettingsMeetings.tsx` uses for its
  // draft fields.
  const [draft, setDraft] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [note, setNote] = useState<string | null>(null);

  const clientId = draft ?? status.clientId ?? "";

  const saveClientId = useCallback(async () => {
    await setCalendarClientId(provider, clientId.trim());
  }, [provider, clientId]);

  const run = useCallback(
    async (work: () => Promise<void>) => {
      setBusy(true);
      setError(null);
      try {
        await work();
      } catch (e) {
        setError(e instanceof Error ? e.message : String(e));
      } finally {
        setBusy(false);
      }
    },
    [],
  );

  const account = status.account;

  return (
    <section>
      <h2>{label}</h2>
      <p className="caveat">
        There is no {label} client built into this application. Register your
        own OAuth client in{" "}
        {provider === "google" ? "Google Cloud Console" : "the Azure portal"}{" "}
        (a desktop / installed-app client, not a web client with a secret) and
        paste its client ID in below. Nothing about your account credentials
        ever leaves this machine except to {provider === "google" ? "Google" : "Microsoft"}
        's own servers, under your own OAuth grant.
      </p>

      <div className="row">
        <span className="row-label">Client ID</span>
        <span className="row-value controls">
          <input
            type="text"
            aria-label={`${label} client ID`}
            placeholder="paste your own OAuth client ID here"
            value={clientId}
            onChange={(e) => setDraft(e.target.value)}
            onBlur={() => {
              void run(saveClientId);
            }}
          />
        </span>
      </div>

      {!status.clientIdConfigured ? (
        <p className="caveat">
          No client ID is set, so {label} cannot be connected yet.
        </p>
      ) : null}

      {account ? (
        <>
          <Row label="Account" value={account.email} />
          <Row
            label="Token"
            value={tokenStatusLabel(account.token).text}
            detail={tokenStatusLabel(account.token).detail || undefined}
          />
          <Row
            label="Last sync"
            value={account.lastSync ? syncOutcomeLabel(account.lastSync.outcome) : "Never synced"}
            detail={account.lastSync ? `at ${account.lastSync.at}` : undefined}
          />

          {account.calendars.length === 0 ? (
            <p className="muted">No calendars have been read yet.</p>
          ) : (
            account.calendars.map((cal) => (
              <div key={cal.id} className="row">
                <span className="row-label">
                  {cal.summary}
                  {cal.isPrimary ? " (primary)" : ""}
                </span>
                <span className="row-value controls">
                  <label>
                    <input
                      type="checkbox"
                      checked={cal.isSelected}
                      disabled={busy}
                      onChange={(e) => {
                        const selected = e.target.checked;
                        void run(async () => {
                          await setCalendarSelected(provider, cal.id, selected);
                          await reload();
                        });
                      }}
                    />
                    synced
                  </label>
                  <em className="row-detail">{cal.eventCount} event(s) stored</em>
                </span>
              </div>
            ))
          )}

          <div className="row">
            <span className="row-value controls">
              <button
                type="button"
                disabled={busy}
                onClick={() => {
                  void run(async () => {
                    const outcome = await syncCalendar(provider);
                    setNote(syncOutcomeLabel(outcome));
                    await reload();
                  });
                }}
              >
                Sync now
              </button>
              <button
                type="button"
                disabled={busy}
                onClick={() => {
                  void run(async () => {
                    await disconnectCalendar(provider);
                    setNote(null);
                    await reload();
                  });
                }}
              >
                Disconnect
              </button>
            </span>
          </div>
        </>
      ) : (
        <div className="row">
          <span className="row-value controls">
            <button
              type="button"
              disabled={busy || !status.clientIdConfigured}
              onClick={() => {
                void run(async () => {
                  await saveClientId();
                  const outcome = await connectCalendar(provider);
                  setNote(connectOutcomeLabel(outcome));
                  await reload();
                });
              }}
            >
              Connect
            </button>
          </span>
        </div>
      )}

      {busy ? (
        <p className="muted">
          Waiting for the browser — this can take a few minutes while you sign
          in and approve access.
        </p>
      ) : null}
      {error ? <p className="error">{error}</p> : null}
      {note ? <p className="caveat">{note}</p> : null}
    </section>
  );
}

export function IntegrationsCalendar() {
  const load = useCallback(() => getCalendarStatus(), []);
  const { data: status, error, reload } = useRefresh(load);

  if (!status) {
    return (
      <div className="view" data-surface="integrations.calendar">
        {error ? <p className="error">{error}</p> : <p className="muted">Loading…</p>}
      </div>
    );
  }

  return (
    <div className="view" data-surface="integrations.calendar">
      {error ? <p className="error">{error}</p> : null}

      {PROVIDERS.map(({ id, label }) => (
        <ProviderSection
          key={id}
          provider={id}
          label={label}
          status={id === "google" ? status.google : status.microsoft}
          reload={reload}
        />
      ))}

      <section>
        <h2>Apple Calendar</h2>
        <Row
          label="State"
          value={appleCalendarSupportLabel(status.apple).text}
          detail={appleCalendarSupportLabel(status.apple).detail}
        />
      </section>
    </div>
  );
}
