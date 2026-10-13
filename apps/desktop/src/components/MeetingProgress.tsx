import type { FinalisationProgress } from "../lib/ipc";
import { FINALISATION_LABELS, processingClock } from "../lib/meetingProgress";
export function MeetingProgress({ progress }: { progress: FinalisationProgress | null }) {
  if (!progress) return <span>Processing your meeting…</span>;
  return <span className="meeting-progress"><span role="status">{FINALISATION_LABELS[progress.stage]}</span><span className="muted">{processingClock(progress.elapsedSeconds)}</span></span>;
}
