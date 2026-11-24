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
import { Row, type RowTone } from "../components/Row";
import {
  checkForUpdate,
  getLastUpdateCheck,
  getUpdatePreferences,
  onUpdateCheckResult,
  openUpdateReleasePage,
  setAutomaticUpdateCheck,
  getMeetingStatus,
  isCaptureExclusionConfirmed,
  type Diagnostics,
  type MeetingStatus,
  type SystemAudioSupport,
  type UpdateCheckResult,
  type UpdatePreferences,
} from "../lib/ipc";
import { systemAudioValue } from "../lib/labels";
import {
  captureExclusionLabel,
  detectedLabel,
  microphoneDetail,
  microphoneValue,
  nonActivatingLabel,
  updateCheckLabel,
} from "../lib/labels";

/** No dot on a mechanism nobody has listened to: it is neither fine nor off. */
function systemAudioTone(s: SystemAudioSupport): RowTone | undefined {
  if (s.state !== "available") return "off";
  return s.unverified ? undefined : "ok";
}

function updateTone(result: UpdateCheckResult): RowTone | undefined {
  if (result.state === "up-to-date") return "ok";
  return result.state === "check-failed" || result.state === "current-version-unparseable" ? "bad" : undefined;
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

/**
 * How a meeting is detected, and what proves the recording window is hidden
 * from screen sharing (KNEG-574).
 *
 * These lines were on Settings > Meetings, among the settings: the Win32
 * interface the microphone is watched through, the affinity value read back
 * from the notification window, and each running application with the
 * evidence it was matched on. None of them can be changed. They are read once
 * when this screen opens, like the rest of it; Settings > Meetings keeps the
 * switch and the two lists, and a folded list of what is running.
 */
function MeetingDetection() {
  const [status, setStatus] = useState<MeetingStatus | null>(null);
  const [problem, setProblem] = useState<string | null>(null);
  useEffect(() => {
    let active = true;
    getMeetingStatus()
      .then((next) => { if (active) setStatus(next); })
      .catch((cause) => { if (active) setProblem(String(cause)); });
    return () => { active = false; };
  }, []);
  if (problem) {
    return (
      <section>
        <h2>meeting detection</h2>
        <Row label="State" tone="bad" value="Couldn't read it" detail={problem} />
      </section>
    );
  }
  if (!status) return null;
  const watching = status.microphone?.state === "available";
  const window = status.notificationExcluded ? captureExclusionLabel(status.notificationExcluded) : null;
  return (
    <section>
      <h2>meeting detection</h2>
      <Row
        label="Microphone activity"
        tone={watching ? "ok" : "off"}
        value={microphoneValue(status.microphone)}
        detail={microphoneDetail(status.microphone)}
      />
      {window ? (
        <Row
          label="Meeting notification window"
          tone={status.notificationExcluded && isCaptureExclusionConfirmed(status.notificationExcluded) ? "ok" : "bad"}
          value={window.text}
          detail={window.detail}
        />
      ) : null}
      {status.detected.length === 0 ? (
        <Row label="Meeting applications" value="None running" />
      ) : (
        status.detected.map((d) => (
          <Row key={`${d.provider}-${d.evidence}`} label={d.name} value={d.inCall ? "In a call" : "Open"} detail={detectedLabel(d)} />
        ))
      )}
    </section>
  );
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

  const [preferences, setPreferences] = useState<UpdatePreferences | null>(null);
  const [savingAutomatic, setSavingAutomatic] = useState(false);
  const [updateProblem, setUpdateProblem] = useState<string | null>(null);
  const [openProblem, setOpenProblem] = useState<string | null>(null);

  useEffect(() => {
    // These are local reads. Rust owns the automatic schedule and reports
    // its results through the same event as the manual check.
    let disposed = false;
    let unlisten: (() => void) | undefined;
    const problem = (err: unknown) => { if (!disposed) setUpdateProblem(String(err)); };
    const reloadPreferences = () => getUpdatePreferences().then((next) => {
      if (!disposed) setPreferences(next);
    }).catch(problem);
    void onUpdateCheckResult((next) => {
      if (disposed) return;
      setUpdateResult(next);
      setOpenProblem(null);
      void reloadPreferences();
    }).then((fn) => {
      if (disposed) fn();
      else unlisten = fn;
    }).catch(problem);
    void getLastUpdateCheck().then((next) => {
      if (!disposed) setUpdateResult(next);
    }).catch(problem);
    void reloadPreferences();
    return () => { disposed = true; unlisten?.(); };
  }, []);

  return (
    <div className="view" data-surface="settings.diagnostics">
      <LocalServerHealth />
      <section>
        <h2>runtime</h2>
        <Row label="Version" value={<span className="row-pixel">{diagnostics.appVersion}</span>} />
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
            ? { detail: "keys can't be saved or read until it works", tone: "bad" as const }
            : {})}
        >
          {/* The failure in view, the system's reason under Details (KNEG-343). */}
          {diagnostics.credentialStore.state === "unavailable" ? (
            <LearnMore summary="Details">
              <p>{diagnostics.credentialStore.reason}</p>
            </LearnMore>
          ) : null}
        </Row>
        <Row
          label="Model cache"
          code
          value={diagnostics.modelCache.path}
          {...(diagnostics.modelCache.fellBack
            ? {
                detail:
                  "the per-user location contains a space or a non-ASCII character, which the native speech binaries cannot read, so a machine-wide location is used instead",
              }
            : {})}
        />
        <Row
          label="Log file"
          code
          value={diagnostics.logFile}
          detail="kept on this computer; attach it when reporting a crash"
        />
      </section>

      <section>
        <h2>privacy overlay</h2>
        <Row
          label="Window"
          tone={diagnostics.overlay.created ? "ok" : "bad"}
          value={diagnostics.overlay.created ? "created" : "not created"}
        />
        <Row
          label="Capture exclusion"
          tone={captureConfirmed ? "ok" : "bad"}
          value={capture.text}
          detail={capture.detail}
        />
        <Row
          label="Focus"
          tone={diagnostics.overlay.nonActivating.state === "active" ? "ok" : "bad"}
          value={focus.text}
          detail={focus.detail}
        />
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
          <p className="caveat warn">
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
        <h2>input and audio</h2>
        <Row
          label="Modifier-only shortcuts"
          tone={diagnostics.modifierOnlyShortcuts.state === "available" ? "ok" : "off"}
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
          tone={systemAudioTone(diagnostics.systemAudio)}
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

      <MeetingDetection />

      <section>
        <h2>software update</h2>
        <label className="switch-row">
          <span>Check for updates automatically</span>
          <input
            type="checkbox"
            role="switch"
            checked={preferences?.automatic ?? true}
            disabled={preferences === null || savingAutomatic}
            onChange={(event) => {
              if (!preferences) return;
              const previous = preferences;
              const enabled = event.currentTarget.checked;
              setPreferences({ ...previous, automatic: enabled });
              setSavingAutomatic(true);
              setUpdateProblem(null);
              void setAutomaticUpdateCheck(enabled)
                .catch((err: unknown) => {
                  setPreferences(previous);
                  setUpdateProblem(String(err));
                })
                .finally(() => setSavingAutomatic(false));
            }}
          />
        </label>
        <p className="muted">
          Once a day, KnegisWhisper asks GitHub whether a newer version has been published. GitHub sees your network address and which version you have. Nothing is downloaded or installed. With this off, it checks only when you press the button.
        </p>
        <Row
          label="Last automatic check"
          value={preferences?.lastAutomaticCheckAt ? new Date(preferences.lastAutomaticCheckAt).toLocaleString() : "Never"}
        />
        {updateProblem ? <p className="caveat warn" role="alert">{updateProblem}</p> : null}
        <button
          type="button"
          className="action-iris"
          disabled={checkingForUpdate}
          onClick={() => {
            setCheckingForUpdate(true);
            setUpdateProblem(null);
            setOpenProblem(null);
            void checkForUpdate()
              .then(setUpdateResult)
              .catch((err: unknown) => setUpdateProblem(String(err)))
              .finally(() => setCheckingForUpdate(false));
          }}
        >
          {checkingForUpdate ? <span className="action-spinner" aria-hidden="true" /> : null}
          {checkingForUpdate ? "Checking…" : "Check for updates"}
        </button>
        {updateResult ? (
          <Row
            label="Last check"
            tone={updateTone(updateResult)}
            value={updateCheckLabel(updateResult).text}
            detail={updateCheckLabel(updateResult).detail}
          >
            {updateResult.state === "update-available" ? (
              <div className="diagnostics-update-actions">
                <button type="button" onClick={() => {
                  setOpenProblem(null);
                  void openUpdateReleasePage().catch((err: unknown) => setOpenProblem(String(err)));
                }}>Open release page</button>
                {openProblem ? <span className="caveat warn" role="alert">{openProblem}</span> : null}
              </div>
            ) : null}
          </Row>
        ) : (
          <p className="muted">Never checked.</p>
        )}
      </section>

      <section>
        <h2>network boundary</h2>
        {/*
          It said "no screen reaches the network any other way" than a
          connection test and an update check, under a list naming calendar,
          transcription, model downloads and media import. And "nothing is
          sent at startup" missed one case: a summary still queued when the
          app closed is claimed by the worker as it starts again
          (`summaries::start`, which marks only in-flight jobs interrupted).
          Read from code (KNEG-343).
          The once-a-day update check, evaluated after 15 seconds with each
          attempt persisted before sending (including failures), is another
          exception that the switch above turns off (KNEG-558).
        */}
        <p>
          Outbound requests are permitted only for these purposes, and each one
          follows something you set up or pressed: dictation, a meeting, cleanup
          or a summary sent to a service you chose; a model or tool you chose to
          install; a calendar you connected; a connection test on the Providers
          page; an update check here. Nothing is sent just because the app
          started, except a summary that was still waiting when the app last
          closed, and the once-a-day update check unless you switch it off above.
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
