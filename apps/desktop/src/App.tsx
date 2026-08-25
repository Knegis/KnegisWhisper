import { useEffect, useState } from "react";
import {
  getDiagnostics,
  isCaptureExclusionConfirmed,
  type CaptureExclusion,
  type Diagnostics,
} from "./lib/ipc";

/**
 * Render a capture-exclusion state without overclaiming.
 *
 * `unknown` deliberately gets the same neutral treatment as `unsupported`
 * rather than a checkmark: the call succeeded but nothing verified it, and
 * telling the user they are hidden on that basis is exactly the failure mode
 * documented in docs/UPSTREAM_AUDIT.md.
 */
function captureExclusionLabel(c: CaptureExclusion): { text: string; detail: string } {
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

function Row({ label, value, detail }: { label: string; value: string; detail?: string }) {
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

export default function App() {
  const [diagnostics, setDiagnostics] = useState<Diagnostics | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    getDiagnostics()
      .then((d) => {
        if (!cancelled) setDiagnostics(d);
      })
      .catch((e: unknown) => {
        if (!cancelled) setError(e instanceof Error ? e.message : String(e));
      });
    return () => {
      cancelled = true;
    };
  }, []);

  if (error) {
    return (
      <main className="shell">
        <h1>KnegisWhisper</h1>
        <p className="error">Could not read diagnostics: {error}</p>
      </main>
    );
  }

  if (!diagnostics) {
    return (
      <main className="shell">
        <h1>KnegisWhisper</h1>
        <p className="muted">Reading diagnostics…</p>
      </main>
    );
  }

  const capture = captureExclusionLabel(diagnostics.captureExclusion);
  const confirmed = isCaptureExclusionConfirmed(diagnostics.captureExclusion);

  return (
    <main className="shell">
      <header>
        <h1>KnegisWhisper</h1>
        <p className="muted">
          v{diagnostics.appVersion} · {diagnostics.os} {diagnostics.arch}
        </p>
      </header>

      <section>
        <h2>Runtime</h2>
        <Row
          label="Electron"
          value={diagnostics.electronPresent ? "present" : "not present"}
        />
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
      </section>

      <section>
        <h2>Privacy overlay</h2>
        <Row
          label="Capture exclusion"
          value={capture.text}
          detail={capture.detail}
        />
        {!confirmed ? (
          <p className="caveat">
            This system cannot confirm the overlay is excluded from screen
            capture. Treat it as visible to recording software until verified.
          </p>
        ) : null}
      </section>

      <section>
        <h2>Network boundary</h2>
        <p className="muted">
          Outbound requests are permitted only for these purposes. Nothing is
          sent at startup.
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
