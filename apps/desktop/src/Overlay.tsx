/**
 * The privacy overlay's contents.
 *
 * Deliberately minimal, and deliberately unambiguous: while the microphone is
 * live this says so, in words, with a visible animated indicator. The local user
 * is never in doubt about whether they are being recorded.
 *
 * That is a product rule, not a style choice. "Privacy overlay" means hidden
 * from the *screen share*, never hidden from the person at the keyboard.
 *
 * **It renders Rust's state and keeps none of its own.** It used to keep its
 * own: it listened for the hotkey and toggled a local flag, under a comment
 * reading "until the dictation pipeline lands". That pipeline landed in phase 2
 * and this window went on guessing — so a hotkey that fired and then failed to
 * dictate left the one window whose job is answering "am I being recorded?"
 * saying yes. Same defect as the four hotkeys that registered and dispatched
 * nowhere, in the worst possible place for it.
 */

import { useEffect, useState } from "react";

import { getDictationState, onDictationState, type SessionState } from "./lib/ipc";

export function Overlay() {
  /**
   * `null` until Rust has answered.
   *
   * Not defaulted to "idle": the window is only shown while a recording is
   * live, so a default of idle would render "Ready" over an open microphone for
   * as long as the first read takes.
   */
  const [state, setState] = useState<SessionState | null>(null);
  const [stateReadFailed, setStateReadFailed] = useState(false);

  useEffect(() => {
    // Read once as well as subscribing. The window is created hidden and shown
    // when recording starts, so the state may already have changed before this
    // component mounted and the event for it is gone.
    void getDictationState()
      .then(setState)
      .catch(() => setStateReadFailed(true));

    let unlisten: (() => void) | undefined;
    void onDictationState(setState).then((fn) => {
      unlisten = fn;
    });
    return () => unlisten?.();
  }, []);

  // Every state but idle has the microphone either open or still holding audio,
  // and the honest word for each differs. Transcribing is not recording, and
  // saying "Recording" through it would be the overclaim; saying "Ready" would
  // be worse.
  // This window exists to answer whether the microphone is open. Once that
  // answer cannot be read, "Ready" would repeat the idle-state guess KNEG-167
  // fixes rather than honestly say it is unavailable.
  const label =
    state === null
      ? stateReadFailed
        ? "State unknown"
        : "…"
      : state === "recording"
        ? "Recording"
        : state === "transcribing"
          ? "Transcribing"
          : state === "inserting"
            ? "Inserting"
            : "Ready";

  return (
    <div className={`overlay overlay-${state ?? "idle"}`}>
      <span
        className={`indicator indicator-${state === "recording" ? "recording" : "idle"}`}
        aria-hidden="true"
      />
      <span className="overlay-text">{label}</span>
    </div>
  );
}
