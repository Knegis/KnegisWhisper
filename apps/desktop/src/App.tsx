import { useCallback, useEffect, useState } from "react";
import {
  clearHistory,
  getDiagnostics,
  getHistory,
  getHotkeys,
  getProviders,
  isCaptureExclusionConfirmed,
  isProviderConnected,
  onHotkey,
  probeProviders,
  type Accelerator,
  type CaptureExclusion,
  type Diagnostics,
  type HotkeyReport,
  type NonActivating,
  type ProbeReport,
  type ProviderSummary,
  type Transcription,
} from "./lib/ipc";
import { Overlay } from "./Overlay";

/**
 * Render a capture-exclusion state without overclaiming.
 *
 * `unknown` deliberately gets the same neutral treatment as `unsupported`
 * rather than a checkmark: the call succeeded but nothing verified it, and
 * telling the user they are hidden on that basis is exactly the failure mode
 * documented in docs/UPSTREAM_AUDIT.md.
 */
function captureExclusionLabel(c: CaptureExclusion): {
  text: string;
  detail: string;
} {
  switch (c.state) {
    case "active":
      return {
        text: "Hidden from screen capture",
        detail: `verified via ${c.verified_by.api} = ${c.verified_by.value}`,
      };
    case "unsupported":
      return { text: "Not supported on this system", detail: c.reason };
    case "failed":
      return { text: "Could not be applied", detail: c.error };
    case "unknown":
      return { text: "Applied, but cannot be verified", detail: c.reason };
  }
}

function nonActivatingLabel(n: NonActivating): { text: string; detail: string } {
  switch (n.state) {
    case "active":
      return { text: "Will not take focus", detail: `verified via ${n.verified_by}` };
    case "unsupported":
      return { text: "Not supported on this system", detail: n.reason };
    case "failed":
      return { text: "Could not be applied", detail: n.error };
  }
}

function acceleratorLabel(a: Accelerator): { text: string; problem: string | null } {
  switch (a.kind) {
    case "standard":
      return { text: a.accelerator, problem: null };
    case "modifier-only":
      return {
        text: a.accelerator,
        problem:
          "Uses only modifier keys. That needs a low-level keyboard hook, which is not implemented yet, so this shortcut does nothing.",
      };
    case "invalid":
      return { text: a.input, problem: a.reason };
  }
}

function Row({
  label,
  value,
  detail,
}: {
  label: string;
  value: string;
  detail?: string;
}) {
  return (
    <div className="row">
      <span className="row-label">{label}</span>
      <span className="row-value">
        {value}
        {detail ? <em className="row-detail">{detail}</em> : null}
      </span>
    </div>
  );
}

function Dashboard() {
  const [diagnostics, setDiagnostics] = useState<Diagnostics | null>(null);
  const [hotkeys, setHotkeys] = useState<HotkeyReport | null>(null);
  const [providers, setProviders] = useState<ProviderSummary[]>([]);
  const [probe, setProbe] = useState<ProbeReport | null>(null);
  const [probing, setProbing] = useState(false);
  const [history, setHistory] = useState<Transcription[]>([]);
  const [lastHotkey, setLastHotkey] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const message = (e: unknown) => (e instanceof Error ? e.message : String(e));

  const refresh = useCallback(async () => {
    try {
      const [d, h, p, rows] = await Promise.all([
        getDiagnostics(),
        getHotkeys(),
        getProviders(),
        getHistory(20),
      ]);
      setDiagnostics(d);
      setHotkeys(h);
      setProviders(p);
      setHistory(rows);
    } catch (e) {
      setError(message(e));
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  // Proves the hotkey path end to end: the shortcut fires in Rust and the event
  // arrives here. Until the dictation pipeline lands, this is the only visible
  // effect, and it is better than nothing happening at all.
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    void onHotkey((fired) => {
      setLastHotkey(`${fired.action} (${fired.accelerator}) at ${new Date().toLocaleTimeString()}`);
    }).then((fn) => {
      unlisten = fn;
    });
    return () => unlisten?.();
  }, []);

  const runProbe = async () => {
    setProbing(true);
    setError(null);
    try {
      setProbe(await probeProviders());
    } catch (e) {
      setError(message(e));
    } finally {
      setProbing(false);
    }
  };

  if (!diagnostics) {
    return (
      <main className="shell">
        <h1>KnegisWhisper</h1>
        {error ? <p className="error">{error}</p> : <p className="muted">Loading…</p>}
      </main>
    );
  }

  const capture = captureExclusionLabel(diagnostics.overlay.captureExclusion);
  const captureConfirmed = isCaptureExclusionConfirmed(diagnostics.overlay.captureExclusion);
  const focus = nonActivatingLabel(diagnostics.overlay.nonActivating);

  return (
    <main className="shell">
      <header>
        <h1>KnegisWhisper</h1>
        <p className="muted">
          v{diagnostics.appVersion} · {diagnostics.os} {diagnostics.arch}
          {diagnostics.webviewVersion ? ` · webview ${diagnostics.webviewVersion}` : null}
        </p>
      </header>

      {error ? <p className="error">{error}</p> : null}

      <section>
        <h2>Runtime</h2>
        <Row
          label="Credential store"
          value={
            diagnostics.credentialStore.state === "available"
              ? diagnostics.credentialStore.backend
              : "unavailable"
          }
          {...(diagnostics.credentialStore.state === "unavailable"
            ? { detail: diagnostics.credentialStore.reason }
            : {})}
        />
        <Row
          label="Model cache"
          value={diagnostics.modelCache.path}
          {...(diagnostics.modelCache.fellBack
            ? {
                detail:
                  "the per-user location contains a space or a non-ASCII character, which the native speech binaries cannot read, so a machine-wide location is used instead",
              }
            : {})}
        />
      </section>

      <section>
        <h2>Privacy overlay</h2>
        <Row label="Window" value={diagnostics.overlay.created ? "created" : "not created"} />
        <Row label="Capture exclusion" value={capture.text} detail={capture.detail} />
        <Row label="Focus" value={focus.text} detail={focus.detail} />
        <Row
          label="Recording indicator"
          value={diagnostics.overlay.recordingIndicatorVisible ? "always shown" : "hidden"}
          detail="the overlay is visible whenever the microphone is live"
        />
        {!captureConfirmed ? (
          <p className="caveat">
            This system cannot confirm the overlay is excluded from screen capture. Treat it as
            visible to recording software until verified. Display server:{" "}
            {diagnostics.overlay.displayServer}.
          </p>
        ) : (
          <p className="caveat">
            The exclusion flag is set and reads back correctly. That confirms the flag, not that a
            particular screen recorder honours it — no API can report that from inside this process.
          </p>
        )}
      </section>

      <section>
        <h2>Shortcuts</h2>
        {hotkeys ? (
          <>
            {hotkeys.bindings.map((b) => (
              <div key={b.action} className="row">
                <span className="row-label">{b.action}</span>
                <span className="row-value">
                  {b.accelerators.length === 0 ? (
                    <em className="row-detail">none configured</em>
                  ) : (
                    b.accelerators.map((a, i) => {
                      const label = acceleratorLabel(a);
                      return (
                        <span key={`${b.action}-${i}`}>
                          <code>{label.text}</code>
                          {label.problem ? (
                            <em className="row-detail warn">{label.problem}</em>
                          ) : null}
                        </span>
                      );
                    })
                  )}
                </span>
              </div>
            ))}
            {hotkeys.conflicts.length > 0 ? (
              <p className="caveat">
                {hotkeys.conflicts
                  .map((c) => `${c.accelerator} is claimed by ${c.actions.join(" and ")}`)
                  .join("; ")}
                .
              </p>
            ) : null}
            {lastHotkey ? <Row label="Last fired" value={lastHotkey} /> : null}
          </>
        ) : (
          <p className="muted">Loading…</p>
        )}
      </section>

      <section>
        <h2>Transcription providers</h2>
        <p className="muted">
          A stored key is not a working provider. Only a connection test can establish that.
        </p>
        {providers.map((p) => {
          const connected = probe ? isProviderConnected(probe, p.id) : false;
          return (
            <Row
              key={p.id}
              label={p.name}
              value={
                !p.configured
                  ? "no key stored"
                  : connected
                    ? "connected"
                    : probe
                      ? "test failed"
                      : "key stored, untested"
              }
              detail={`${p.modelCount} model${p.modelCount === 1 ? "" : "s"} · slot ${p.credentialSlot}`}
            />
          );
        })}
        <button type="button" onClick={() => void runProbe()} disabled={probing}>
          {probing ? "Testing…" : "Test connections"}
        </button>
        {probe ? (
          <>
            {probe.audio === "tone" ? (
              <p className="caveat">
                The voice test used a generated tone, not speech. A pass proves the endpoint,
                credential and model id were accepted; it cannot prove words are recognised.
              </p>
            ) : null}
            <ul className="purposes">
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

      <section>
        <h2>History</h2>
        {history.length === 0 ? (
          <p className="muted">Nothing dictated yet.</p>
        ) : (
          <>
            <ul className="purposes">
              {history.map((h) => (
                <li key={h.id}>
                  <code>{h.status}</code> {h.text || h.errorMessage || "(empty)"}
                  {h.provider ? <em className="row-detail">{h.provider}</em> : null}
                </li>
              ))}
            </ul>
            <button
              type="button"
              onClick={() => {
                void clearHistory().then(() => refresh());
              }}
            >
              Clear history
            </button>
          </>
        )}
      </section>

      <section>
        <h2>Network boundary</h2>
        <p className="muted">
          Outbound requests are permitted only for these purposes. Nothing is sent at startup —
          the connection test above is the only thing on this screen that reaches the network, and
          only when you press it.
        </p>
        <ul className="purposes">
          {diagnostics.allowedEgressPurposes.map((p) => (
            <li key={p}>
              <code>{p}</code>
            </li>
          ))}
        </ul>
      </section>
    </main>
  );
}

/**
 * Both windows are served by one bundle, routed on the URL fragment. One bundle
 * means one CSP and one build.
 */
export default function App() {
  const isOverlay = window.location.hash === "#overlay";
  return isOverlay ? <Overlay /> : <Dashboard />;
}
