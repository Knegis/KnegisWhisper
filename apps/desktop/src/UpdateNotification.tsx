/**
 * The update notification window's contents.
 *
 * 392×92, content-protected, bottom-right — the same size and the same rule as
 * `MeetingNotification.tsx`: "content-protected" means out of screen shares
 * and never hidden from the person at the keyboard, the rule at the top of
 * `Overlay.tsx`.
 *
 * Says one thing: that a newer release of this project has been found. It
 * never checks on its own — the check is an explicit action on
 * Settings > Diagnostics, and this window only renders what that check found,
 * the same way the overlay renders Rust's dictation state rather than
 * guessing at it.
 *
 * There is no download button here, and there will not be one from this
 * window. Notifying that an update exists is what this surface is for;
 * fetching and installing one is a different, larger claim this project does
 * not make yet.
 */

import { useEffect, useState } from "react";

import {
  dismissUpdateNotification,
  getLastUpdateCheck,
  onUpdateCheckResult,
  type UpdateCheckResult,
} from "./lib/ipc";
import { updateCheckLabel } from "./lib/labels";

export function UpdateNotification() {
  const [result, setResult] = useState<UpdateCheckResult | null>(null);

  useEffect(() => {
    // Read once as well as subscribing: this window is created hidden at
    // startup, so by the time it is shown the event that caused it may
    // already have been delivered to a component that had not mounted yet.
    void getLastUpdateCheck().then(setResult);

    let unlisten: (() => void) | undefined;
    void onUpdateCheckResult(setResult).then((fn) => {
      unlisten = fn;
    });
    return () => unlisten?.();
  }, []);

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
        <span className="notify-detail">{label.detail}</span>
      </span>
      <button
        type="button"
        onClick={() => {
          void dismissUpdateNotification();
        }}
      >
        Dismiss
      </button>
    </div>
  );
}
