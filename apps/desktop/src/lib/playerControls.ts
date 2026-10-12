/**
 * Arithmetic behind the recording player's skip buttons and the section ticks
 * on its scrubber (KNEG-448). Pure so it can be tested without an audio
 * element: the player passes in what the element reports and gets back a
 * time or a position, never touching `currentTime` here.
 */

/** How far the -15 / +15 buttons jump. */
export const SKIP_SECONDS = 15;

const isUsableDuration = (d: number): boolean => Number.isFinite(d) && d > 0;

/**
 * Where a skip of `delta` seconds from `current` lands. Clamped to the
 * recording when its length is known; while the element has not reported a
 * length yet (NaN, Infinity or 0) only the start is a limit, so a skip
 * forward is not silently swallowed. A non-finite `current` counts as 0.
 */
export function skipTarget(current: number, delta: number, duration: number): number {
  const from = Number.isFinite(current) ? current : 0;
  const target = from + delta;
  const upper = isUsableDuration(duration) ? duration : Infinity;
  return Math.min(Math.max(target, 0), upper);
}

/**
 * Positions along the scrubber, as fractions of the recording, for marks given
 * in seconds (where transcript sections start). Marks at the very start or
 * end, outside the recording, or not a number carry no information and are
 * dropped; the rest come back ascending without duplicates. Without a usable
 * duration there is nothing to place, so the result is empty.
 */
export function markFractions(marks: readonly number[], duration: number): number[] {
  if (!isUsableDuration(duration)) return [];
  const fractions = new Set<number>();
  for (const mark of marks) {
    if (mark > 0 && mark < duration) fractions.add(mark / duration);
  }
  return [...fractions].sort((a, b) => a - b);
}
