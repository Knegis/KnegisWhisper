import type { FinalisationProgress, FinalisationUpdate } from "./ipc.js";
import { formatClock } from "./clock.js";

export const FINALISATION_LABELS: Record<FinalisationProgress["stage"], string> = {
  "preparing-recording": "Preparing recording",
  "finishing-live-transcript": "Finishing live transcript",
  "transcribing-recording": "Transcribing recording",
  "analysing-speakers": "Analysing speakers",
  "saving-note": "Saving note",
};
export function newerProgress(current: FinalisationUpdate | null, incoming: FinalisationUpdate): boolean {
  if (!current || incoming.revision > current.revision) return true;
  return incoming.revision === current.revision && current.progress !== null && incoming.progress !== null
    && incoming.progress.operationId === current.progress.operationId && incoming.progress.elapsedSeconds > current.progress.elapsedSeconds;
}
export function processingClock(seconds: number): string { return `Processing ${formatClock(seconds)}`; }
