/**
 * The privacy overlay's contents.
 *
 * Deliberately minimal, and deliberately unambiguous: while the microphone is
 * live this says so, in words, with a visible animated indicator. The local user
 * is never in doubt about whether they are being recorded.
 *
 * That is a product rule, not a style choice. "Privacy overlay" means hidden
 * from the *screen share*, never hidden from the person at the keyboard.
 */

import { useEffect, useState } from "react";
import { onHotkey } from "./lib/ipc";

type State = "idle" | "recording";

export function Overlay() {
  const [state, setState] = useState<State>("idle");

  // Until the dictation pipeline lands, the hotkey toggles the indicator. The
  // wiring is the same either way: Rust owns the recording state and the overlay
  // renders it.
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    void onHotkey((fired) => {
      if (fired.action === "dictation") {
        setState((s) => (s === "recording" ? "idle" : "recording"));
      }
    }).then((fn) => {
      unlisten = fn;
    });
    return () => unlisten?.();
  }, []);

  return (
    <div className={`overlay overlay-${state}`}>
      <span className={`indicator indicator-${state}`} aria-hidden="true" />
      <span className="overlay-text">
        {state === "recording" ? "Recording" : "Ready"}
      </span>
    </div>
  );
}
