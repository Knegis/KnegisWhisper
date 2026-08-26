/**
 * What this machine can actually do, and how we know.
 *
 * Every line is a read of real state rather than a prediction about the
 * platform: the overlay row describes the window that exists, and the recording
 * indicator is a live reading of whether it is on screen. It used to be a
 * hardcoded `true`, which made the guarantee it expressed vacuous.
 *
 * Takes `diagnostics` as a prop rather than fetching it, because the shell
 * already reads it for the version line and two calls would be two answers.
 */

import { Row } from "../components/Row";
import {
  isCaptureExclusionConfirmed,
  type Diagnostics,
  type SystemAudioSupport,
} from "../lib/ipc";
import { captureExclusionLabel, nonActivatingLabel } from "../lib/labels";

/** What can be said about system audio, without overclaiming. */
function systemAudioValue(s: SystemAudioSupport): string {
  switch (s.state) {
    case "available":
      // The unverified case is deliberately not "Available": the mechanism
      // exists and nobody has heard it produce sound.
      return s.unverified ? "Available, never tested" : "Available";
    case "no-device":
      return "No device offers it";
    case "unsupported":
      return "Not supported here";
  }
}

function systemAudioDetail(s: SystemAudioSupport): string {
  switch (s.state) {
    case "available":
      return `${s.mechanism} — ${s.device}`;
    case "no-device":
      return s.reason;
    case "unsupported":
      return s.reason;
  }
}

export function DiagnosticsView({ diagnostics }: { diagnostics: Diagnostics }) {
  const capture = captureExclusionLabel(diagnostics.overlay.captureExclusion);
  const captureConfirmed = isCaptureExclusionConfirmed(
    diagnostics.overlay.captureExclusion,
  );
  const focus = nonActivatingLabel(diagnostics.overlay.nonActivating);

  return (
    // No `data-surface` marker: config/ui-surfaces.json declares surfaces that
    // config/integration-manifest.json entries name, and no entry names this
    // screen. A marker for an undeclared surface would be a claim about nothing.
    <div className="view">
      <section>
        <h2>Runtime</h2>
        <Row label="Version" value={diagnostics.appVersion} />
        <Row
          label="Platform"
          value={`${diagnostics.os} ${diagnostics.arch}`}
          detail={
            diagnostics.webviewVersion
              ? `webview ${diagnostics.webviewVersion}`
              : "the webview version could not be determined"
          }
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
        <Row
          label="Window"
          value={diagnostics.overlay.created ? "created" : "not created"}
        />
        <Row
          label="Capture exclusion"
          value={capture.text}
          detail={capture.detail}
        />
        <Row label="Focus" value={focus.text} detail={focus.detail} />
        {/*
          A live reading, not a setting — and it used to read like one. "Recording
          indicator: hidden" alongside "the overlay is visible whenever the
          microphone is live" says two opposite things, and the alarming one is
          the wrong one: nothing is recording, which is why nothing is showing.
        */}
        <Row
          label="Recording indicator"
          value={
            diagnostics.overlay.recordingIndicatorVisible
              ? "showing now"
              : "not showing"
          }
          detail={
            diagnostics.overlay.recordingIndicatorVisible
              ? "the microphone is live"
              : "nothing is recording; it appears whenever the microphone is live"
          }
        />
        {!captureConfirmed ? (
          <p className="caveat">
            This system cannot confirm the overlay is excluded from screen
            capture. Treat it as visible to recording software until verified.
            Display server: {diagnostics.overlay.displayServer}.
          </p>
        ) : (
          <p className="caveat">
            The exclusion flag is set and reads back correctly. That confirms the
            flag, not that a particular screen recorder honours it — no API can
            report that from inside this process.
          </p>
        )}
      </section>

      <section>
        <h2>Input and audio</h2>
        <Row
          label="Modifier-only shortcuts"
          value={
            diagnostics.modifierOnlyShortcuts.state === "available"
              ? "Supported"
              : "Not supported here"
          }
          detail={
            diagnostics.modifierOnlyShortcuts.state === "available"
              ? `${diagnostics.modifierOnlyShortcuts.mechanism}; the keys still reach other applications`
              : diagnostics.modifierOnlyShortcuts.reason
          }
        />
        <Row
          label="System audio"
          value={systemAudioValue(diagnostics.systemAudio)}
          detail={systemAudioDetail(diagnostics.systemAudio)}
        />
        {diagnostics.systemAudio.state === "available" &&
        diagnostics.systemAudio.unverified ? (
          <p className="caveat">
            The mechanism for this platform is written and nobody has run it on
            real hardware. Treat a recording made with it as unproven until
            somebody has listened to one.
          </p>
        ) : null}
      </section>

      <section>
        <h2>Network boundary</h2>
        <p className="muted">
          Outbound requests are permitted only for these purposes. Nothing is
          sent at startup — a connection test on the Providers screen is
          explicitly invoked, and no other screen reaches the network at all.
        </p>
        <ul className="purposes">
          {diagnostics.allowedEgressPurposes.map((p) => (
            <li key={p}>
              <code>{p}</code>
            </li>
          ))}
        </ul>
      </section>
    </div>
  );
}
