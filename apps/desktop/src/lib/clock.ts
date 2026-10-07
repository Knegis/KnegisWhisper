/**
 * Seconds as the recording player's clock: `m:ss`, or `h:mm:ss` from an hour.
 * Here rather than in `components/RecordingPlayer.tsx`, which re-exports it,
 * so a search hit's time (`lib/searchHit.ts`) reads as the player does and a
 * node test can import it without the component.
 */
export function formatClock(seconds: number): string {
  const total = Math.max(0, Math.floor(seconds));
  const hours = Math.floor(total / 3600);
  const minutes = Math.floor((total % 3600) / 60);
  const remaining = total % 60;
  return hours > 0
    ? `${hours}:${String(minutes).padStart(2, "0")}:${String(remaining).padStart(2, "0")}`
    : `${minutes}:${String(remaining).padStart(2, "0")}`;
}
