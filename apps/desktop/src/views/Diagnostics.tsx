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

import { useEffect, useState } from "react";

import { LearnMore } from "../components/LearnMore";
import { LocalServerHealth } from "../components/LocalServerHealth";
import { Row } from "../components/Row";
import {
  checkForUpdate,
  getLastUpdateCheck,
  isCaptureExclusionConfirmed,
  type Diagnostics,
  type SystemAudioSupport,
  type UpdateCheckResult,
} from "../lib/ipc";
import { systemAudioValue } from "../lib/labels";
import {
  captureExclusionLabel,
  nonActivatingLabel,
  updateCheckLabel,
} from "../lib/labels";

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

  const [updateResult, setUpdateResult] = useState<UpdateCheckResult | null>(
    null,
  );
  const [checkingForUpdate, setCheckingForUpdate] = useState(false);

  useEffect(() => {
    // Reads what a previous check already found. This itself never triggers
    // one — "opt-in, and never a check at startup" holds for this screen too;
    // the only thing that starts a check is the button below.
    void getLastUpdateCheck().then(setUpdateResult);
  }, []);

  return (
    // No `data-surface` marker: config/ui-surfaces.json declares surfaces that
    // config/integration-manifest.json entries name, and no entry names this
    // screen. A marker for an undeclared surface would be a claim about nothing.
    <div className="view">
      <LocalServerHealth />
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
              : "Unavailable"
          }
          {...(diagnostics.credentialStore.state === "unavailable"
            ? { detail: "keys can't be saved or read until it works" }
            : {})}
        />
        {/* The failure in view, the system's reason under Details (KNEG-343). */}
        {diagnostics.credentialStore.state === "unavailable" ? (
          <LearnMore summary="Details">
            <p>{diagnostics.credentialStore.reason}</p>
          </LearnMore>
        ) : null}
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
        <h2>Software update</h2>
        <p className="muted">
          Checked only when the button below is pressed. Nothing here runs at
          startup, on a timer, or without this click.
        </p>
        <button
          type="button"
          disabled={checkingForUpdate}
          onClick={() => {
            setCheckingForUpdate(true);
            void checkForUpdate()
              .then(setUpdateResult)
              .finally(() => setCheckingForUpdate(false));
          }}
        >
          {checkingForUpdate ? "Checking…" : "Check for updates"}
        </button>
        {updateResult ? (
          <Row
            label="Last check"
            value={updateCheckLabel(updateResult).text}
            detail={updateCheckLabel(updateResult).detail}
          />
        ) : (
          <p className="muted">Never checked.</p>
        )}
      </section>

      <section>
        <h2>Network boundary</h2>
        {/*
          It said "no screen reaches the network any other way" than a
          connection test and an update check, under a list naming calendar,
          transcription, model downloads and media import. And "nothing is
          sent at startup" missed one case: a summary still queued when the
          app closed is claimed by the worker as it starts again
          (`summaries::start`, which marks only in-flight jobs interrupted).
          Read from code (KNEG-343).
        */}
        <p className="muted">
          Outbound requests are permitted only for these purposes, and each one
          follows something you set up or pressed: dictation, a meeting, cleanup
          or a summary sent to a service you chose; a model or tool you chose to
          install; a calendar you connected; a connection test on the Providers
          page; an update check here. Nothing is sent just because the app
          started, except a summary that was still waiting when the app last
          closed.
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
