/**
 * What a meeting note says about its speaker labels, over its transcript.
 *
 * Labels are added after the note is saved (`kw_notes_core::speaker_labels`),
 * so the note is open while they are still being worked out, and stays whole
 * when they fail. Every line here says first that nothing was lost, because
 * until this existed a failure during this step cost the whole meeting.
 *
 * Pure -- no React -- so it is tested (`tests/speakerLabels.test.ts`).
 */
import type { SpeakerLabelState } from "./ipc.js";

export interface SpeakerLabelLine {
  /** `working` while labels are being added, `failed` when they were not, `offer` when the note went on without them. */
  kind: "working" | "failed" | "offer";
  text: string;
  /** What the software said, for Details. */
  detail: string | null;
  /** The button that starts labelling again, or `null` when it can't: the recording is gone. */
  retry: string | null;
  /** The button that stops waiting for labels, or `null` when nothing is waiting. */
  dismiss: string | null;
}

const DISMISS = "Continue without speaker labels";

export function speakerLabelLine(state: SpeakerLabelState | null): SpeakerLabelLine | null {
  switch (state?.status) {
    case "queued":
    case "processing":
      return {
        kind: "working",
        text: "Adding speaker labels. The note and transcript are saved; the labels appear here when they are ready.",
        detail: null,
        retry: null,
        dismiss: DISMISS,
      };
    case "failed":
    case "interrupted":
      return {
        kind: "failed",
        text: state.status === "interrupted"
          ? "Speaker labels were interrupted before they finished. The note and transcript are saved."
          : "Speaker labels couldn't be added. The note and transcript are saved.",
        detail: state.error,
        retry: state.canRetry ? "Retry speaker labels" : null,
        dismiss: DISMISS,
      };
    case "dismissed":
      // Nothing to say once the recording is gone: there is no way to add them.
      return state.canRetry
        ? { kind: "offer", text: "This note was saved without speaker labels.", detail: null, retry: "Add speaker labels", dismiss: null }
        : null;
    default:
      return null;
  }
}
