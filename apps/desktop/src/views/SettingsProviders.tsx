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
 *    each and are rendered in their own section below, not counted here.)
 * 2. **A stored key is never called "connected".** Storing a secret proves only
 *    that the credential store worked. Only `probe_providers` — a real request
 *    with the user's own key, run when they press the button — can say more, and
 *    it refuses to call a provider connected on a 2xx alone.
 * 3. **The field writes and is cleared. Nothing reads a secret back.** There is
 *    no command that returns a credential value, so this screen could not
 *    display one if it tried.
 */

import { useCallback, useState } from "react";

import { Row } from "../components/Row";
import {
  getCredentialSlots,
  isProviderConnected,
  probeProviders,
  setProviderCredential,
  type CredentialSlotStatus,
  type ProbeReport,
} from "../lib/ipc";
import { useRefresh } from "../lib/useRefresh";

/**
 * What can be said about a slot before any request is made.
 *
 * Deliberately never "connected": that word is reserved for a passing probe.
 * A custom endpoint gets a different answer again, because a key on its own
 * genuinely does not configure it — the base URL lives with the feature it
 * belongs to, and that screen does not exist yet.
 */
function storedLabel(slot: CredentialSlotStatus, probe: ProbeReport | null) {
  if (!slot.stored) return { value: "No key stored", detail: undefined };

  if (slot.kind === "custom-endpoint") {
    return {
      value: "Key stored",
      detail:
        "a custom endpoint also needs its base URL, which is set with the feature that uses it — not here, and not yet",
    };
  }

  if (slot.usedFor.length === 0) {
    return {
      value: "Key stored",
      detail:
        "no provider in the registry uses this slot yet, so there is nothing to test it against",
    };
  }

  if (!probe) {
    return { value: "Key stored, untested", detail: undefined };
  }

  // The probe is keyed by provider id, and for every slot that reaches here the
  // provider id and the slot name are the same string.
  return isProviderConnected(probe, slot.slot)
    ? { value: "Connected", detail: "a real request with this key succeeded" }
    : { value: "Test failed", detail: "see the results below" };
}

export function SettingsProviders() {
  const load = useCallback(() => getCredentialSlots(), []);
  const { data: slots, error, guard, fail } = useRefresh(load);

  /** Per-slot draft. Never seeded from a stored value; there is no way to. */
  const [drafts, setDrafts] = useState<Record<string, string>>({});
  /** Per-slot outcome of the last press, so no control is ever silent. */
  const [results, setResults] = useState<Record<string, string>>({});
  const [probe, setProbe] = useState<ProbeReport | null>(null);
  const [probing, setProbing] = useState(false);

  const say = (slot: string, what: string) =>
    setResults((prev) => ({ ...prev, [slot]: what }));

  const save = (slot: CredentialSlotStatus) => {
    const typed = (drafts[slot.slot] ?? "").trim();
    if (!typed) {
      say(slot.slot, "Type a key first. Use Clear to remove the stored one.");
      return;
    }
    void guard(async () => {
      await setProviderCredential(slot.slot, typed);
      // Cleared immediately, and this is the only copy that ever existed in the
      // renderer. A field that keeps the key on screen after saving is a field
      // that leaks it to the next person at the machine.
      setDrafts((prev) => {
        const next = { ...prev };
        delete next[slot.slot];
        return next;
      });
      say(
        slot.slot,
        slot.kind === "custom-endpoint"
          ? "Stored. This does not test anything — a custom endpoint needs its base URL too."
          : "Stored. Press Test connections to find out whether it works.",
      );
      // Any earlier verdict is now about a different key.
      setProbe(null);
    });
  };

  const clear = (slot: CredentialSlotStatus) => {
    void guard(async () => {
      // An empty value is how `provider_credential_set` deletes. Explicit here
      // so the intent is not hidden in a falsy check.
      await setProviderCredential(slot.slot, "");
      say(slot.slot, "Removed from the credential store.");
      setProbe(null);
    });
  };

  const runProbe = async () => {
    setProbing(true);
    try {
      setProbe(await probeProviders());
    } catch (e) {
      fail(e);
    } finally {
      setProbing(false);
    }
  };

  if (!slots) {
    return (
      <div className="view">
        {error ? <p className="error">{error}</p> : <p className="muted">Loading…</p>}
      </div>
    );
  }

  const providers = slots.filter((s) => s.kind === "provider");
  const custom = slots.filter((s) => s.kind === "custom-endpoint");
  // Enterprise slots hold a whole multi-field configuration as one JSON
  // document, so a single-secret field cannot fill one. They are named here
  // rather than silently absent — a slot that exists and appears nowhere is
  // indistinguishable from a slot nobody implemented.
  const enterprise = slots.filter((s) => s.kind === "enterprise");
  const testable = providers.filter((s) => s.stored && s.usedFor.length > 0);

  const field = (slot: CredentialSlotStatus) => {
    const state = storedLabel(slot, probe);
    return (
      <div key={slot.slot} className="row">
        <span className="row-label">{slot.name}</span>
        <span className="row-value">
          {state.value}
          {state.detail ? <em className="row-detail">{state.detail}</em> : null}
        </span>
        {/*
          A direct child of the row, not of the value, so the grid gives it a
          row of its own spanning both columns. The field then starts level with
          the label because it genuinely begins there -- pulling it left with a
          margin instead put it on top of any label long enough to wrap, which
          "Self-hosted cleanup endpoint key" is.
        */}
        <span className="controls">
            <input
              type="password"
              autoComplete="off"
              aria-label={`API key for ${slot.name}`}
              placeholder={slot.stored ? "Replace the stored key" : "Paste a key"}
              value={drafts[slot.slot] ?? ""}
              onChange={(e) =>
                setDrafts((prev) => ({ ...prev, [slot.slot]: e.target.value }))
              }
              onKeyDown={(e) => {
                if (e.key === "Enter") save(slot);
              }}
            />
            {/* Never disabled: a control that silently does nothing is the defect. */}
            <button type="button" onClick={() => save(slot)}>
              Save
            </button>
            {slot.stored ? (
              <button type="button" onClick={() => clear(slot)}>
                Clear
              </button>
            ) : null}
        </span>
        <em className="row-detail">
          slot <code>{slot.slot}</code>
          {slot.usedFor.length > 0 ? ` · used for ${slot.usedFor.join(" and ")}` : ""}
          {slot.feature ? ` · ${slot.feature}` : ""}
        </em>
        {results[slot.slot] ? (
          <em className="row-detail">{results[slot.slot]}</em>
        ) : null}
      </div>
    );
  };

  return (
    <div className="view" data-surface="settings.providers">
      {error ? <p className="error">{error}</p> : null}

      <section>
        <p className="muted">
          Keys go straight into this machine's credential store —{" "}
          <strong>never</strong> into the database, a settings row, or a log
          line. Nothing reads one back out, so this screen cannot show you a key
          you have already stored; it can only replace or remove it.
        </p>
        {providers.map(field)}
      </section>

      <section>
        <h2>Connection tests</h2>
        <p className="muted">
          A stored key is not a working provider. This is the only thing on this
          screen that reaches the network, and only when you press it — it sends
          a real request with your own credentials.
        </p>
        <Row
          label="Ready to test"
          value={
            testable.length === 0
              ? "Nothing yet"
              : testable.map((s) => s.name).join(", ")
          }
          detail={
            testable.length === 0
              ? "store a key for a provider above first"
              : undefined
          }
        />
        <button
          type="button"
          onClick={() => void runProbe()}
          disabled={probing || testable.length === 0}
        >
          {probing ? "Testing…" : "Test connections"}
        </button>
        {probe ? (
          <>
            {probe.audio === "tone" ? (
              <p className="caveat">
                The voice test used a generated tone, not speech. A pass proves
                the endpoint, credential and model id were accepted; it cannot
                prove words are recognised.
              </p>
            ) : null}
            {probe.results.every((r) => r.outcome.outcome === "skipped") ? (
              <p className="caveat">
                Every test was skipped, so nothing was proven either way.
              </p>
            ) : null}
            {/* `list`, not `purposes`: each result is a pill, a verdict and a
                sentence, which is a row rather than a pill. */}
            <ul className="list">
              {probe.results
                .filter((r) => r.outcome.outcome !== "skipped")
                .map((r, i) => (
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
          </>
        ) : null}
      </section>

      {enterprise.length > 0 ? (
        <section>
          <h2>Enterprise connectors</h2>
          <p className="muted">
            {enterprise.map((s) => s.name).join(", ")} use organisation
            credentials — a region and a key pair, or an endpoint and a
            deployment — rather than a single API key, so they are configured on
            the Intelligence screen where those fields are.
          </p>
          <Row
            label="Stored here"
            value={
              enterprise.some((s) => s.stored)
                ? enterprise
                    .filter((s) => s.stored)
                    .map((s) => s.name)
                    .join(", ")
                : "None yet"
            }
          />
        </section>
      ) : null}

      <section>
        <h2>Custom endpoints</h2>
        <p className="muted">
          Five features can be pointed at an OpenAI-compatible endpoint of your
          own. The key can be stored here; the base URL belongs with the feature
          that uses it, and those screens are not written yet — so a key stored
          here configures nothing on its own, and this page says so rather than
          looking finished.
        </p>
        {custom.map(field)}
      </section>
    </div>
  );
}
