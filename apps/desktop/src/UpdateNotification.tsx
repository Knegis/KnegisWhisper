/**
 * The update notification window's contents.
 *
 * 392×104, content-protected, bottom-right — the same rule as
 * `MeetingNotification.tsx`: "content-protected" means out of screen shares
 * and never hidden from the person at the keyboard, the rule at the top of
 * `Overlay.tsx`.
 *
 * Renders the result of Rust's manual or daily automatic check. Rust owns
 * the schedule and session dismissal; this component only displays its state.
 * The Open release page button asks Rust to open the URL it built. There is
 * still no download button: nothing here downloads or installs an update.
 */

import { useEffect, useState } from "react";

import {
  dismissUpdateNotification,
  getLastUpdateCheck,
  onUpdateCheckResult,
  openUpdateReleasePage,
  type UpdateCheckResult,
} from "./lib/ipc";
import { updateCheckLabel } from "./lib/labels";

export function UpdateNotification() {
  const [result, setResult] = useState<UpdateCheckResult | null>(null);
  const [stateReadProblem, setStateReadProblem] = useState<string | null>(null);

  const [openProblem, setOpenProblem] = useState<string | null>(null);

  useEffect(() => {
    // Read once as well as subscribing: this window is created hidden at
    // startup, so by the time it is shown the event that caused it may
    // already have been delivered to a component that had not mounted yet.
    void getLastUpdateCheck()
      .then(setResult)
      .catch((err: unknown) => setStateReadProblem(String(err)));

    let disposed = false;
    let unlisten: (() => void) | undefined;
    void onUpdateCheckResult((next) => {
      setResult(next);
      setOpenProblem(null);
    }).then((fn) => {
      if (disposed) fn();
      else unlisten = fn;
    }).catch((err: unknown) => setStateReadProblem(String(err)));
    return () => { disposed = true; unlisten?.(); };
  }, []);

  // Nothing was readable and no event has arrived since, so this window cannot
  // say whether a release exists. Saying so beats the empty panel that used to
  // stand in for it, which was indistinguishable from "no update" (KNEG-167).
  if (stateReadProblem && result === null) {
    return (
      <div className="notify">
        <span className="indicator" aria-hidden="true" />
        <span className="notify-body">
          <strong>Update state unknown</strong>
          <span className="notify-detail notify-alert">{stateReadProblem}</span>
        </span>
      </div>
    );
  }

  // The window is shown by Rust only for this one state; every other result
  // is for whichever screen asked. A stray render in another state shows an
  // empty panel rather than a stale one.
  if (!result || result.state !== "update-available") {
    return <div className="notify notify-empty" />;
  }

  const label = updateCheckLabel(result);

  return (
    <div className="notify">
      <span className="indicator" aria-hidden="true" />
      <span className="notify-body">
        <strong>{label.text}</strong>
        <span className={`notify-detail${openProblem ? " notify-alert" : ""}`}>{openProblem ?? label.detail}</span>
      </span>
      <div className="notify-actions">
        <button
          type="button"
          className="primary"
          onClick={() => {
            setOpenProblem(null);
            void openUpdateReleasePage().catch((err: unknown) => setOpenProblem(String(err)));
          }}
        >
          Open release page
        </button>
        <button
          type="button"
          onClick={() => {
            void dismissUpdateNotification();
          }}
        >
          Dismiss
        </button>
      </div>
    </div>
  );
}
