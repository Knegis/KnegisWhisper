import type { LevelSession, LevelSource } from "../lib/ipc";
import { segmentsLit } from "../lib/levelMeter";
import { useAudioLevel } from "../lib/useAudioLevel";

/**
 * A capture's live sound level, as a row of segments (KNEG-371).
 *
 * Decoration, so `aria-hidden` and not `role="meter"` (ruling, 2026-10-07):
 * ten changes a second cannot be announced usefully, and the state beside
 * every meter -- "Listening…", "Recording" -- is already a live region that
 * says a capture is running. What the meter adds is for the eye: that the
 * microphone hears you.
 *
 * Mount it only while its capture runs. `data-level` and `data-lit` carry
 * what it shows, for a check to read.
 */
export function LevelMeter({
  session,
  source,
  segments = 12,
  className,
}: {
  session: LevelSession;
  source: LevelSource;
  segments?: number;
  className?: string;
}) {
  const level = useAudioLevel(session, source);
  const lit = segmentsLit(level, segments);
  return (
    <span
      className={className === undefined ? "level-meter" : `level-meter ${className}`}
      aria-hidden="true"
      data-level={level.toFixed(2)}
      data-lit={lit}
    >
      {Array.from({ length: segments }, (_, index) => (
        <span key={index} className={index < lit ? "level-segment level-segment-lit" : "level-segment"} />
      ))}
    </span>
  );
}
