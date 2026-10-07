/**
 * Settings > Providers — where an API key gets stored.
 *
 * This is the screen the application did not have. `provider_credential_set`
 * has existed in Rust and `setProviderCredential` has been exported from
 * `ipc.ts` since phase 1, and nothing called either, so every cloud provider in
 * the registry was unreachable no matter how complete its backend was.
 *
 * Three rules it holds to, each of them a thing the upstream audit found done
 * the other way:
 *
 * 1. **The slot list comes from Rust.** `credential_slots` reads
 *    `config/provider-registry.json`, so no provider id or slot name is written
 *    here. Seventeen slots, twelve providers and five per-feature custom
 *    endpoints — up from fifteen and ten once AssemblyAI and Deepgram gained
 *    their own BYOK slots for meeting streaming. (Three further enterprise
 *    slots — Bedrock, Azure, Vertex — hold a whole connector configuration
 *    each and are listed under Company accounts below, not counted here.)
 * 2. **A stored key is never called "connected".** Storing a secret proves only
 *    that the credential store worked. Only `probe_providers` — a real request
 *    with the user's own key, run when they press the button — can say more, and
 *    it refuses to call a provider connected on a 2xx alone.
 * 3. **The field writes and is cleared. Nothing reads a secret back.** There is
 *    no command that returns a credential value, so this screen could not
 *    display one if it tried.
 *
 * # The page (KNEG-363)
 *
 * One row per slot, from `lib/providerRows.ts`: In use (a key is stored),
 * Available, Custom endpoints and Company accounts, after a first row for this
 * computer, which needs no key and has no form. What a row says about itself
 * comes from the slot and nothing else -- its tile is the first letter of its
 * name, its line is its `usedFor` in words plus "Live meetings" when a meeting
 * client reads the slot (`getMeetingProviders().credentialSlot`, KNEG-373), and
 * "Not supported yet" is `getProviders().dispatch`. The filter tabs and the search field narrow the
 * same rows; neither hides a slot the registry declares from All with the
 * field empty, so every one of them can still be reached here.
 *
 * A row's key form opens under the row from its + or its Change, one row at a
 * time: opening another closes the first without saving, and what was typed
 * in it goes with it. Save is enabled once something is typed, and the typed
 * value is dropped the moment it is stored. The slot's id, its uses and its
 * feature are under Technical details in that form, or on their own for a row
 * that has none; and which keys a Test connections press sends is said in
 * the open, above the lists.
 */

import { useCallback, useEffect, useState, type KeyboardEvent, type ReactNode } from "react";

import { FailureLine } from "../components/FailureLine";
import { Icon } from "../components/Icon";
import { LearnMore } from "../components/LearnMore";
import { PageHeader } from "../components/PageHeader";
import {
  getCredentialSlots,
  getMeetingProviders,
  getProviders,
  isProviderConnected,
  probeProviders,
  setProviderCredential,
  type CredentialSlotStatus,
  type ProbeKind,
  type ProbeReport,
} from "../lib/ipc";
import type { ViewId } from "../lib/navigation";
import { PAGES } from "../lib/pages";
import { filterRows, keyRowShown, providerRows, SPEECH, TEXT, type ProviderFilter, type ProviderRow } from "../lib/providerRows";
import { message, useRefresh } from "../lib/useRefresh";

type KeyState = { label: string; tone: "saved" | "connected" | "failed"; title?: string; detail?: string; more?: string };

/**
 * What can be said about a stored key.
 *
 * Before any request, only "Key saved": that word is not "connected", which is
 * reserved for a passing probe. A custom endpoint says no more even after a
 * test, because a key on its own genuinely does not configure it -- its
 * section says so. And a key the probe ran nothing for is still "Key saved",
 * with "Not tested" under it, not "Test failed": `probe_configured` tests the
 * registry's transcription providers and nothing else (`crates/provider-probe`),
 * so Anthropic, Deepgram and AssemblyAI are never tested, and a skipped test
 * proved nothing either way. (This said "Test failed" for any key without a
 * passing test, and that no provider used an empty-`usedFor` slot "yet" --
 * which stopped being true of Deepgram when its meeting client landed.)
 */
function keyState(slot: CredentialSlotStatus, probe: ProbeReport | null): KeyState | null {
  if (!slot.stored) return null;
  if (slot.kind === "custom-endpoint" || !probe) return { label: "Key saved", tone: "saved" };

  // The probe is keyed by provider id, and for every slot that reaches here the
  // provider id and the slot name are the same string.
  const mine = probe.results.filter((r) => r.provider === slot.slot);
  if (!mine.some((r) => r.outcome.outcome !== "skipped")) {
    // The probe's own reason for skipping is the mechanism, so it goes under
    // the row's Why (KNEG-343).
    const why = mine.map((r) => ("detail" in r.outcome ? r.outcome.detail : "")).find(Boolean);
    return why
      ? { label: "Key saved", tone: "saved", detail: "Not tested.", more: why }
      : { label: "Key saved", tone: "saved", detail: "Not tested: Test connections doesn't cover this key." };
  }
  return isProviderConnected(probe, slot.slot)
    ? { label: "Connected", tone: "connected", title: "A real request with this key succeeded." }
    : { label: "Test failed", tone: "failed" };
}

const TABS: readonly { id: ProviderFilter; label: string }[] = [
  { id: "all", label: "All" },
  { id: "speech", label: "Speech to text" },
  { id: "text", label: "Text models" },
];

/** Words for a probe's kind, for the sentence a failure gets. */
const KIND: Record<ProbeKind, string> = { catalog: "model list", chat: "text", voice: "speech" };

/** This computer, filtered with the rest: it matches both tabs, and a search for its name. */
const LOCAL: ProviderRow = { slot: "", name: "This computer", letter: "", purposes: [SPEECH, TEXT], stored: true, unsupported: false };

/**
 * Focus the + or Change that opens `formId` -- after a form closes, so focus is not dropped on the
 * page. Found by `data-toggle-for`, not `aria-controls`: that names the form only while it is open,
 * since an id that points at nothing is an error to anything reading it, and by the time this runs
 * the form has closed.
 */
const focusToggle = (formId: string) => document.querySelector<HTMLElement>(`[data-toggle-for="${formId}"]`)?.focus();

export function SettingsProviders({ onGoTo }: { onGoTo: (id: ViewId) => void }) {
  const load = useCallback(async () => {
    // The meeting providers only add "Live meetings" to the rows whose keys
    // they read. A failed read loses that word, not the page.
    const [slots, transcription, meetings] = await Promise.all([
      getCredentialSlots(),
      getProviders(),
      getMeetingProviders().catch(() => []),
    ]);
    return { slots, transcription, meetings };
  }, []);
  const { data, error, guard, fail } = useRefresh(load);

  /** The one row whose form is open, and what is typed in it. Never seeded from a stored value; there is no way to. */
  const [open, setOpen] = useState<string | null>(null);
  const [draft, setDraft] = useState("");
  /** Per-slot outcome of the last press, so no control is ever silent. */
  const [results, setResults] = useState<Record<string, string>>({});
  /**
   * Per-slot failure of the last Save or Clear, drawn at its row as an alert.
   * It was the page's error line at the top, out of sight of the row and not
   * announced (KNEG-377).
   */
  const [rowErrors, setRowErrors] = useState<Record<string, { text: string; detail: string }>>({});
  const [probe, setProbe] = useState<ProbeReport | null>(null);
  const [probing, setProbing] = useState(false);
  const [testNote, setTestNote] = useState<string | null>(null);
  const [filter, setFilter] = useState<ProviderFilter>("all");
  const [query, setQuery] = useState("");
  /*
   * Where focus goes once a Save or Clear has been re-read: the row's new +
   * or Change. An effect, not a call after `guard`, because the row moves
   * between lists -- its button is a new element -- and the commit that draws
   * it may land after `guard` resolves; focusing then found the old button,
   * which was about to go, and left focus on the page.
   */
  const [refocus, setRefocus] = useState<{ formId: string } | null>(null);
  useEffect(() => {
    if (refocus) focusToggle(refocus.formId);
  }, [refocus]);

  const say = (slot: string, what: string) =>
    setResults((prev) => ({ ...prev, [slot]: what }));
  /** A row's failure: the sentence, and the error itself for its Details (KNEG-343). */
  const rowError = (slot: string, what: { text: string; detail: string } | null) =>
    setRowErrors((prev) => {
      const next = { ...prev };
      if (what === null) delete next[slot];
      else next[slot] = what;
      return next;
    });

  /** Open this row's form, closing any other -- and dropping whatever was typed there. */
  const toggle = (slot: string) => {
    setDraft("");
    setOpen((current) => (current === slot ? null : slot));
  };

  const save = (slot: CredentialSlotStatus) => {
    const typed = draft.trim();
    if (!typed) {
      say(slot.slot, "Type a key first. Use Clear to remove the stored one.");
      return;
    }
    let done = false;
    rowError(slot.slot, null);
    void guard(async () => {
      try {
        await setProviderCredential(slot.slot, typed);
      } catch (cause) {
        rowError(slot.slot, { text: "Couldn't save the key.", detail: message(cause) });
        return;
      }
      // Cleared immediately, and this is the only copy that ever existed in the
      // renderer. A field that keeps the key on screen after saving is a field
      // that leaks it to the next person at the machine.
      setDraft("");
      setOpen(null);
      say(
        slot.slot,
        slot.kind === "custom-endpoint"
          ? "Saved. It does nothing on its own yet: the feature that uses it needs its server's address too."
          : "Saved. Press Test connections to check that it works.",
      );
      // Any earlier verdict is now about a different key, and "nothing to
      // test" may no longer be true.
      setProbe(null);
      setTestNote(null);
      done = true;
    }).then(() => { if (done) setRefocus({ formId: `provider-form-${slot.slot}` }); });
  };

  const clear = (slot: CredentialSlotStatus) => {
    let done = false;
    rowError(slot.slot, null);
    void guard(async () => {
      // An empty value is how `provider_credential_set` deletes. Explicit here
      // so the intent is not hidden in a falsy check.
      try {
        await setProviderCredential(slot.slot, "");
      } catch (cause) {
        rowError(slot.slot, { text: "Couldn't remove the key.", detail: message(cause) });
        return;
      }
      setDraft("");
      setOpen(null);
      say(slot.slot, "Removed from this computer.");
      setProbe(null);
      setTestNote(null);
      done = true;
    }).then(() => { if (done) setRefocus({ formId: `provider-form-${slot.slot}` }); });
  };

  const header = (actions?: ReactNode) =>
    actions === undefined ? <PageHeader page={PAGES.providers} /> : <PageHeader page={PAGES.providers} actions={actions} />;

  if (!data) {
    return (
      <div className="view">
        {header()}
        {error ? <FailureLine text="This page couldn't load." detail={error} /> : <p className="muted">Loading…</p>}
      </div>
    );
  }

  const { slots, transcription, meetings } = data;
  const bySlot = new Map(slots.map((s) => [s.slot, s]));
  const lists = providerRows(slots, transcription, meetings);
  // What `probe_configured` will test: a transcription provider with a key.
  const testable = slots.filter((s) => s.stored && transcription.some((p) => p.credentialSlot === s.slot));

  const runProbe = async () => {
    if (testable.length === 0) {
      setTestNote("Nothing to test yet: Test connections checks speech-to-text providers, and none has a saved key.");
      return;
    }
    setTestNote(null);
    setProbing(true);
    try {
      setProbe(await probeProviders());
    } catch (e) {
      fail(e);
    } finally {
      setProbing(false);
    }
  };

  // A filter or a search that hides the open row closes its form and drops
  // what was typed, as closing it does: a key must not wait, unseen, in a row
  // that is out of view (KNEG-377).
  const narrow = (nextFilter: ProviderFilter, nextQuery: string) => {
    setFilter(nextFilter);
    setQuery(nextQuery);
    if (open !== null && !keyRowShown(lists, open, nextFilter, nextQuery)) {
      setOpen(null);
      setDraft("");
    }
  };

  const onTabKey = (event: KeyboardEvent<HTMLDivElement>) => {
    const at = TABS.findIndex((t) => t.id === filter);
    const to = event.key === "ArrowRight" ? (at + 1) % TABS.length
      : event.key === "ArrowLeft" ? (at + TABS.length - 1) % TABS.length
      : event.key === "Home" ? 0
      : event.key === "End" ? TABS.length - 1
      : -1;
    const next = TABS[to];
    if (!next) return;
    event.preventDefault();
    narrow(next.id, query);
    document.getElementById(`providers-tab-${next.id}`)?.focus();
  };

  const nameOf = (id: string) => bySlot.get(id)?.name ?? id;
  const reasonOf = (id: string) => {
    const dispatch = transcription.find((p) => p.credentialSlot === id)?.dispatch;
    return dispatch && dispatch.state !== "ready" ? dispatch.reason : undefined;
  };

  const tile = (row: ProviderRow) => <span className="provider-tile" aria-hidden="true">{row.letter}</span>;

  /** A slot that takes one secret: a provider or a custom endpoint. */
  const keyRow = (row: ProviderRow) => {
    const slot = bySlot.get(row.slot);
    if (!slot) return null;
    const state = keyState(slot, probe);
    const formId = `provider-form-${row.slot}`;
    const isOpen = open === row.slot;
    const reason = reasonOf(row.slot);
    // The slot's own ids, from the data: behind a disclosure, because ids move
    // behind Learn more rather than being deleted. In the form, or on its own
    // for a row that has no form.
    const technical = (
      <div className="provider-tech">
        <LearnMore summary="Technical details">
          <p>
            slot <code>{slot.slot}</code>
            {slot.usedFor.length > 0 ? ` · used for ${slot.usedFor.join(" and ")}` : ""}
            {slot.feature ? ` · ${slot.feature}` : ""}
          </p>
          {row.unsupported && reason ? <p>{reason}</p> : null}
        </LearnMore>
      </div>
    );
    return (
      <li key={row.slot} className={`provider-row${row.unsupported && !row.stored ? " provider-row-unsupported" : ""}`}>
        {tile(row)}
        <span className="provider-text">
          <span className="provider-name">{row.name}</span>
          {row.purposes.length > 0 ? (
            <span className="provider-purposes">
              {/* One span per purpose, so a narrow row wraps between them, not inside "Live meetings". */}
              {row.purposes.map((purpose, i) => (
                <span key={purpose} className="provider-purpose">{i > 0 ? ` · ${purpose}` : purpose}</span>
              ))}
            </span>
          ) : null}
        </span>
        <span className="provider-actions">
          {state ? (
            <span className={`provider-state provider-state-${state.tone}`} {...(state.title ? { title: state.title } : {})}>
              {state.tone === "failed" ? null : <Icon name="check" size={14} />}
              {state.label}
            </span>
          ) : null}
          {row.stored ? (
            // Named for its row, as the + beside an empty one is: a list of
            // "Change, Change, Change" says nothing about which key each one
            // changes. "Change" leads, so the visible word is the name's first.
            <button type="button" className="secondary provider-change" aria-label={`Change the key for ${row.name}`} aria-expanded={isOpen} aria-controls={isOpen ? formId : undefined} data-toggle-for={formId} onClick={() => toggle(row.slot)}>
              Change
            </button>
          ) : row.unsupported ? (
            <span className="provider-unsupported" {...(reason ? { title: reason } : {})}>Not supported yet</span>
          ) : (
            <button type="button" className="provider-add" aria-label={`Add a key for ${row.name}`} aria-expanded={isOpen} aria-controls={isOpen ? formId : undefined} data-toggle-for={formId} onClick={() => toggle(row.slot)}>
              <Icon name="plus" size={20} />
            </button>
          )}
        </span>
        {state?.detail ? <em className="row-detail provider-detail">{state.detail}</em> : null}
        {state?.more ? (
          <div className="provider-detail">
            <LearnMore summary="Why">
              <p>{state.more}</p>
            </LearnMore>
          </div>
        ) : null}
        {results[row.slot] ? <em className="row-detail provider-detail" role="status">{results[row.slot]}</em> : null}
        {rowErrors[row.slot] ? (
          <div className="provider-detail">
            <FailureLine text={rowErrors[row.slot]!.text} detail={rowErrors[row.slot]!.detail} />
          </div>
        ) : null}
        {isOpen ? (
          <span className="controls provider-form" id={formId}>
            <input
              type="password"
              autoComplete="off"
              autoFocus
              aria-label={`API key for ${row.name}`}
              placeholder={row.stored ? "Replace the stored key" : "Paste a key"}
              value={draft}
              onChange={(e) => setDraft(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter" && draft.trim() !== "") save(slot);
                if (e.key === "Escape") {
                  toggle(row.slot);
                  focusToggle(formId);
                }
              }}
            />
            <button type="button" className="primary" disabled={draft.trim() === ""} onClick={() => save(slot)}>
              Save
            </button>
            {row.stored ? (
              <button type="button" className="secondary" onClick={() => clear(slot)}>
                Clear
              </button>
            ) : null}
          </span>
        ) : null}
        {isOpen || (row.unsupported && !row.stored) ? technical : null}
      </li>
    );
  };

  // Enterprise slots hold a whole multi-field configuration as one JSON
  // document, so a single-secret field cannot fill one. They are listed here
  // rather than silently absent — a slot that exists and appears nowhere is
  // indistinguishable from a slot nobody implemented.
  const companyRow = (row: ProviderRow) => (
    <li key={row.slot} className="provider-row">
      {tile(row)}
      <span className="provider-text">
        <span className="provider-name">{row.name}</span>
        <button type="button" className="provider-link" onClick={() => onGoTo("intelligence")}>
          Set up under Cleanup &amp; summaries
        </button>
      </span>
      <span className="provider-actions">
        {row.stored ? <span className="provider-state provider-state-saved"><Icon name="check" size={14} />Saved</span> : null}
      </span>
    </li>
  );

  const group = (id: string, heading: string, items: readonly ReactNode[], note?: string) =>
    items.length === 0 ? null : (
      <section className="providers-group" aria-labelledby={`providers-${id}`}>
        <h2 id={`providers-${id}`}>{heading}</h2>
        {note ? <p className="muted providers-note">{note}</p> : null}
        <ul className="provider-list">{items}</ul>
      </section>
    );

  const showLocal = filterRows([LOCAL], filter, query).length > 0;
  const inUse = filterRows(lists.inUse, filter, query).map(keyRow);
  const available = filterRows(lists.available, filter, query).map(keyRow);
  const custom = filterRows(lists.custom, filter, query).map(keyRow);
  const company = filterRows(lists.enterprise, filter, query).map(companyRow);
  const local = (
    <li key="this-computer" className="provider-row provider-row-local">
      <span className="provider-tile provider-tile-local" aria-hidden="true"><Icon name="computer" size={22} /></span>
      <span className="provider-text">
        <span className="provider-name">This computer</span>
        <span className="provider-purposes">Speech to text and text models. No key needed.</span>
      </span>
    </li>
  );
  const nothing = !showLocal && inUse.length + available.length + custom.length + company.length === 0;

  const ran = probe?.results.filter((r) => r.outcome.outcome !== "skipped") ?? [];
  const failures = ran.flatMap((r) => (r.outcome.outcome === "fail" ? [{ ...r, detail: r.outcome.detail }] : []));

  return (
    <div className="view providers" data-surface="settings.providers">
      {header(
        <>
          <label className="providers-search">
            <Icon name="search" size={16} />
            <input
              type="search"
              aria-label="Search providers"
              placeholder="Search providers"
              value={query}
              onChange={(e) => narrow(filter, e.target.value)}
            />
          </label>
          <button type="button" className="secondary providers-test" onClick={() => void runProbe()} disabled={probing}>
            {probing ? "Testing…" : "Test connections"}
          </button>
        </>,
      )}

      {error ? <FailureLine text="That didn't work." detail={error} /> : null}

      <div className="providers-tabs" role="tablist" aria-label="Filter providers" onKeyDown={onTabKey}>
        {TABS.map((t) => (
          <button
            key={t.id}
            id={`providers-tab-${t.id}`}
            type="button"
            role="tab"
            aria-selected={filter === t.id}
            aria-controls="providers-lists"
            tabIndex={filter === t.id ? 0 : -1}
            onClick={() => narrow(t.id, query)}
          >
            {t.label}
          </button>
        ))}
      </div>

      <p className="providers-lock">
        <Icon name="lock" size={14} />
        A saved key sends nothing by itself. Audio or text goes to a provider only when you choose it under Transcription, Meetings or Cleanup &amp; summaries.
      </p>
      <p className="providers-line">
        Test connections is the only thing on this page that reaches the network, and only when you press it: it sends a real request with each saved speech-to-text key.
      </p>
      {/* Who a press would send to, named -- it is data leaving this computer, so it is never folded away. */}
      <p className="providers-line providers-ready">
        {testable.length === 0
          ? "Ready to test: nothing yet. Save a key for a speech-to-text provider first."
          : `Ready to test: ${testable.map((s) => s.name).join(", ")}.`}
      </p>

      {testNote ? <p className="providers-line" role="status">{testNote}</p> : null}
      {probe ? (
        <div className="providers-tests" role="status">
          {probe.audio === "tone" ? (
            <p className="caveat">
              The test sent a tone, not speech. A pass shows the provider
              accepted the key and the model; it can't show that words are
              recognised.
            </p>
          ) : null}
          {probe.results.every((r) => r.outcome.outcome === "skipped") ? (
            <p className="caveat">
              Every test was skipped, so nothing was proven either way.
            </p>
          ) : null}
          {failures.map((r, i) => (
            // What the provider answered is under Test details below (KNEG-343).
            <p key={`${r.provider}-${r.kind}-${i}`} className="error providers-failure">
              {nameOf(r.provider)} failed its {KIND[r.kind]} test. See Test details.
            </p>
          ))}
          {ran.length > 0 ? (
            <LearnMore summary="Test details">
              {/* `list`, not `purposes`: each result is a pill, a verdict and a
                  sentence, which is a row rather than a pill. */}
              <ul className="list">
                {ran.map((r, i) => (
                  <li key={`${r.provider}-${r.kind}-${i}`}>
                    <code>
                      {r.provider}/{r.kind}
                    </code>{" "}
                    {r.outcome.outcome}
                    {"detail" in r.outcome && r.outcome.detail ? (
                      <em className="row-detail">{r.outcome.detail}</em>
                    ) : null}
                  </li>
                ))}
              </ul>
            </LearnMore>
          ) : null}
        </div>
      ) : null}

      <div id="providers-lists" role="tabpanel" aria-labelledby={`providers-tab-${filter}`}>
        {group("in-use", "In use", [...(showLocal ? [local] : []), ...inUse])}
        {group("available", "Available", available)}
        {group(
          "custom",
          "Custom endpoints",
          custom,
          "For features that can use a server of your own. A key saved here does nothing on its own yet: the screens where you enter each server's address aren't built.",
        )}
        {group(
          "company",
          "Company accounts",
          company,
          "These use your organisation's settings rather than one key, so they are set up under Cleanup & summaries.",
        )}
        {nothing ? <p className="muted providers-empty">Nothing here matches “{query.trim()}”.</p> : null}
      </div>

      <LearnMore summary="How keys are stored">
        <p>
          Keys go straight into this computer's credential store — never into
          the database, a settings row, or a log line. Nothing reads one back
          out, so this page cannot show you a key you have already stored; it
          can only replace or remove it.
        </p>
        <p>
          A stored key is not a working provider. Only Test connections can
          say more, with a real request using your own key.
        </p>
      </LearnMore>
    </div>
  );
}
