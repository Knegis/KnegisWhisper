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
 *
 * In plain words (KNEG-343): what to do and where your sign-in goes stay in
 * view; OAuth, tokens, the providers' own errors and why Apple is missing are
 * under Learn more. Times are shown in your locale, not as stored.
 */

import { useCallback, useState } from "react";

import { FailureLine } from "../components/FailureLine";
import { LearnMore } from "../components/LearnMore";
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
  type Plain,
} from "../lib/labels";
import { useRefresh } from "../lib/useRefresh";

const PROVIDERS: { id: CalendarProvider; label: string }[] = [
  { id: "google", label: "Google Calendar" },
  { id: "microsoft", label: "Microsoft Calendar" },
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
      {line.more ? (
        <LearnMore summary="Details">
          <p>{line.more}</p>
        </LearnMore>
      ) : null}
    </>
  );
}

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
  const [note, setNote] = useState<Plain | null>(null);

  const clientId = draft ?? status.clientId ?? "";
  const company = provider === "google" ? "Google" : "Microsoft";
  const portal = provider === "google" ? "Google Cloud Console" : "the Azure portal";

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
  const token = account ? tokenStatusLabel(account.token) : null;
  const lastSync = account?.lastSync ? syncOutcomeLabel(account.lastSync.outcome) : null;

  return (
    <section>
      <h2>{label}</h2>
      {/* The kind of client is a step, not background: a web client will not
          connect. So it is in view (branch review). */}
      <p className="caveat">
        To connect, register your own app in {portal} as a desktop app (not a
        web app) and paste its client ID below. Your sign-in goes only to{" "}
        {company}.
      </p>
      <LearnMore summary="Why">
        <p>
          There is no {label} app built into KnegisWhisper: a client secret
          shipped inside an open-source app would not stay secret, so a desktop
          (installed) app client, which needs none, is the kind that works. Your
          account's credentials go only to {company}'s own servers, under the
          access you grant.
        </p>
      </LearnMore>

      <div className="row">
        <span className="row-label">Client ID</span>
        <span className="row-value controls">
          <input
            type="text"
            aria-label={`${label} client ID`}
            placeholder="paste your own client ID here"
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
          No client ID yet, so {label} can't be connected.
        </p>
      ) : null}

      {account && token ? (
        <>
          <Row label="Account" value={account.email} />
          <Row label="Connection" value={token.text} detail={token.detail || undefined} />
          {token.more ? (
            <LearnMore summary="Details">
              <p>{token.more}</p>
            </LearnMore>
          ) : null}
          <Row
            label="Last sync"
            value={lastSync ? lastSync.text : "Never synced"}
            detail={account.lastSync ? when(account.lastSync.at) : undefined}
          />
          {lastSync?.more ? (
            <LearnMore summary="Details">
              <p>{lastSync.more}</p>
            </LearnMore>
          ) : null}

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
      {error ? <FailureLine text="That didn't work." detail={error} /> : null}
      {note ? <Said line={note} className="caveat" /> : null}
    </section>
  );
}

export function IntegrationsCalendar() {
  const load = useCallback(() => getCalendarStatus(), []);
  const { data: status, error, reload } = useRefresh(load);

  if (!status) {
    return (
      <div className="view" data-surface="integrations.calendar">
        {error ? <FailureLine text="This page couldn't load." detail={error} /> : <p className="muted">Loading…</p>}
      </div>
    );
  }

  const apple = appleCalendarSupportLabel(status.apple);

  return (
    <div className="view" data-surface="integrations.calendar">
      {error ? <FailureLine text="That didn't work." detail={error} /> : null}

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
        <Row label="State" value={apple.text} />
        <LearnMore summary="Why">
          <p>{apple.detail}</p>
        </LearnMore>
      </section>
    </div>
  );
}
