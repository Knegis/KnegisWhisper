import { useEffect, useRef, useState } from "react";
import { getMeetingStatus, onMeetingProgress, onMeetingState, type FinalisationUpdate } from "./ipc";
import { newerProgress } from "./meetingProgress";

/** The runtime clock survives navigation; only the display ticks in the renderer. */
export function useFinalisationProgress() {
  const [snapshot, setSnapshot] = useState<{ update: FinalisationUpdate; received: number } | null>(null);
  const latest = useRef<FinalisationUpdate | null>(null);
  const [now, setNow] = useState(() => performance.now());
  useEffect(() => {
    let disposed = false;
    const stops: Array<() => void> = [];
    const accept = (update: FinalisationUpdate) => {
      if (disposed || !update || !newerProgress(latest.current, update)) return;
      latest.current = update;
      const received = performance.now(); setNow(received); setSnapshot({ update, received });
    };
    const read = () => { void getMeetingStatus().then(status => accept(status.finalisation)).catch(() => { /* Keep the last authoritative stage. */ }); };
    const keep = (stop: () => void) => { if (disposed) stop(); else stops.push(stop); };
    void onMeetingProgress(accept).then(stop => { keep(stop); if (!disposed) read(); }).catch(() => { if (!disposed) read(); });
    void onMeetingState(read).then(keep).catch(() => { if (!disposed) read(); });
    return () => { disposed = true; stops.forEach(stop => stop()); };
  }, []);
  const progress = snapshot?.update.progress ?? null;
  useEffect(() => {
    if (!progress) return;
    const timer = window.setInterval(() => setNow(performance.now()), 1000);
    return () => window.clearInterval(timer);
  }, [progress?.operationId]);
  return progress && snapshot ? { ...progress, elapsedSeconds: progress.elapsedSeconds + Math.max(0, now - snapshot.received) / 1000 } : null;
}
