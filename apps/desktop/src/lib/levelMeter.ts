/**
 * The arithmetic behind the live sound-level meters (KNEG-371), apart from
 * React so it can be tested on its own. `useAudioLevel` and
 * `components/LevelMeter.tsx` are the rest.
 *
 * A level is 0..1, which Rust has already placed on the meter: 0 at −60 dBFS
 * or quieter, 1 at full scale, linear in decibels (`kw_audio_core::level`).
 * Rust sends one about ten times a second while a capture runs, a steady
 * sound at least every 200 ms, and a last 0 when the capture stops
 * (`src-tauri/src/audio_level.rs`). Staleness is the backstop for that last
 * 0 not arriving -- a window that was closed, an event that was lost -- so a
 * meter never stays lit over a capture that has gone.
 */

/** A reading this old shows as silence. */
export const STALE_MS = 400;

/** Under reduced motion, the shortest time between two changes of a meter: at most four a second. */
export const REDUCED_MOTION_INTERVAL_MS = 250;

/** One reading, and when it arrived (`Date.now()`). */
export type LevelSample = { level: number; at: number };

/** A level as 0..1; NaN is silence. */
function clamp(level: number): number {
  return Number.isNaN(level) ? 0 : Math.min(1, Math.max(0, level));
}

/**
 * How many of `segments` a level of 0..1 lights: its share, rounded, so less
 * than half a segment -- the quietest twentieth or so of the range -- lights
 * nothing.
 */
export function segmentsLit(level: number, segments: number): number {
  if (!(segments > 0)) return 0;
  return Math.round(clamp(level) * segments);
}

/**
 * The level a meter shows at `now`: the last reading, until it is
 * `STALE_MS` old; silence before any reading and after that.
 */
export function levelAt(sample: LevelSample | null, now: number): number {
  if (sample === null || now - sample.at >= STALE_MS) return 0;
  return clamp(sample.level);
}

/**
 * How long to hold a new reading before showing it: none, unless reduced
 * motion is asked for and the meter changed less than
 * `REDUCED_MOTION_INTERVAL_MS` ago, in which case the rest of that interval.
 */
export function showDelay(lastShownAt: number | null, now: number, reducedMotion: boolean): number {
  if (!reducedMotion || lastShownAt === null) return 0;
  return Math.max(0, lastShownAt + REDUCED_MOTION_INTERVAL_MS - now);
}
