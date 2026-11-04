/**
 * What Dictation's status notice and its picture tile say.
 *
 * Pure, so `tests/dictationNotice.test.ts` can hold it to one rule: **"Ready"
 * is dictation's word, not the microphone's.** The design draws this notice as
 * "Microphone · Ready", but `lib/readiness.ts` reads the route, the shortcut
 * and the insert path and never opens a microphone, so the label says
 * "Dictation" and the microphone is only named. An interface that reports a
 * state it never read is the defect `tray.rs` describes at its head.
 *
 * Recording and busy outrank readiness: once a dictation has started,
 * "will starting work?" means nothing.
 */

export type DictationTone = "ready" | "recording" | "busy" | "setup";

export type DictationStatus = {
  tone: DictationTone;
  notice: {
    label: string;
    /** The announced words. */
    value: string;
    /** A clock that ticks every second, so it is kept out of the live region. */
    extra?: string;
  };
  /** The tile's small label, lower-case in the pixel face as the design sets it. */
  tile: string;
};

export function dictationStatus({
  recording,
  busyLabel,
  ready,
  microphone,
  microphoneMissing,
  elapsed,
}: {
  recording: boolean;
  /** `sessionStateLabel` while a dictation is being transcribed or typed, otherwise `null`. */
  busyLabel: string | null;
  /** `dictationReadiness(...).state === "ready"`. */
  ready: boolean;
  /** The microphone chip's label. */
  microphone: string;
  /** The chosen microphone is unplugged, so dictation uses the system default until it is back. */
  microphoneMissing: boolean;
  elapsed: string;
}): DictationStatus {
  if (recording) {
    return {
      tone: "recording",
      notice: { label: "Recording", value: "the overlay is showing", extra: elapsed },
      tile: `recording ${elapsed}`,
    };
  }
  if (busyLabel !== null) {
    return {
      tone: "busy",
      notice: { label: "Dictation", value: busyLabel },
      tile: busyLabel.toLowerCase(),
    };
  }
  if (ready) {
    const inEffect = microphoneMissing ? "System default" : microphone;
    return {
      tone: "ready",
      notice: { label: "Dictation", value: `Ready · ${inEffect}` },
      tile: "ready",
    };
  }
  return {
    tone: "setup",
    notice: { label: "Dictation", value: "Needs setup" },
    tile: "needs setup",
  };
}
