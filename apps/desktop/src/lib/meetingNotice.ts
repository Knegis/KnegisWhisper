/**
 * What Meetings' status notice and its picture tile say, in every state.
 *
 * Pure, so `tests/meetingNotice.test.ts` can hold it to what each state may
 * claim. **"Ready" is the meeting's word, not the microphone's**: as on
 * Dictation (`lib/dictationNotice.ts`), the design's "Microphone · Ready" would
 * report a test nothing ran, so the microphone is only named. And **stalled never
 * reads as recording**: nothing is being captured then, and it takes the hollow
 * ring rather than the signal dot.
 */

import type { MeetingState } from "./ipc.js";

export type MeetingTone = "ready" | "recording" | "busy" | "setup";

export type MeetingStatusView = {
  tone: MeetingTone;
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

export function meetingStatus({
  state,
  saved,
  ready,
  microphone,
  microphoneMissing,
  elapsed,
  destination,
}: {
  state: MeetingState;
  /** The outcome's sentence while a saved meeting's result is on screen, otherwise `null`. */
  saved: string | null;
  /** `meetingReadiness(...).state === "ready"`. */
  ready: boolean;
  /** The microphone chip's label. */
  microphone: string;
  /** The chosen microphone is unplugged, so a meeting uses the system default until it is back. */
  microphoneMissing: boolean;
  /** The running meeting's clock, or `null` where this page has no start to count from. */
  elapsed: string | null;
  /** Where its audio goes, as a phrase: "Sent to Deepgram", "Transcribed on this computer". */
  destination: string;
}): MeetingStatusView {
  const clock = elapsed === null ? {} : { extra: elapsed };
  switch (state) {
    case "recording":
      return { tone: "recording", notice: { label: "Recording", value: destination, ...clock }, tile: "recording" };
    case "recording-without-transcript":
      return {
        tone: "recording",
        notice: { label: "Recording", value: "the live transcript has stopped", ...clock },
        tile: "recording",
      };
    case "stalled":
      return {
        tone: "setup",
        notice: { label: "Stopped unexpectedly", value: "nothing is being recorded", ...clock },
        tile: "stopped",
      };
    case "finishing":
      return {
        tone: "busy",
        notice: { label: "Finishing", value: destination, ...clock },
        tile: "finishing the meeting…",
      };
    case "idle":
      break;
    default: {
      const unhandled: never = state;
      return unhandled;
    }
  }
  if (saved !== null) {
    return { tone: "ready", notice: { label: "Saved", value: saved }, tile: "saved" };
  }
  if (ready) {
    const inEffect = microphoneMissing ? "System default" : microphone;
    return { tone: "ready", notice: { label: "Meeting", value: `Ready · ${inEffect}` }, tile: "ready" };
  }
  return { tone: "setup", notice: { label: "Meeting", value: "Needs setup" }, tile: "needs setup" };
}
