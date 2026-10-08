/**
 * One capture's live level, for a meter (KNEG-371).
 *
 * Listens to Rust's one `audio-level` event and keeps only the readings for
 * `session` and `source`, so the Dictation page, the overlay, the Meetings
 * page's two meters and its microphone check each follow their own capture.
 * The arithmetic -- staleness, the reduced-motion pace -- is
 * `lib/levelMeter.ts`'s; this is the React and the timers around it.
 *
 * Mount it only while the capture runs. It asks for nothing: Rust sends a
 * level from the moment a capture opens until it stops, and a meter mounted
 * a moment late simply starts at silence.
 */

import { useEffect, useState } from "react";

import { onAudioLevel, type LevelSession, type LevelSource } from "./ipc";
import { STALE_MS, levelAt, showDelay, type LevelSample } from "./levelMeter";

const REDUCED_MOTION = "(prefers-reduced-motion: reduce)";

/** The level to show now, 0..1. */
export function useAudioLevel(session: LevelSession, source: LevelSource): number {
  const [sample, setSample] = useState<LevelSample | null>(null);
  // When the meter was last drawn. Moved by each reading shown and once more
  // when that reading goes stale, which is what lets `levelAt` empty a meter
  // whose capture went quiet without another render to notice it.
  const [now, setNow] = useState(() => Date.now());

  useEffect(() => {
    // `disposed` because `listen` resolves after the effect returns: a meter
    // unmounted first must still unlisten, and must not set state after.
    let disposed = false;
    let unlisten: (() => void) | null = null;
    let pending: LevelSample | null = null;
    let lastShownAt: number | null = null;
    let showTimer: ReturnType<typeof setTimeout> | undefined;
    let staleTimer: ReturnType<typeof setTimeout> | undefined;

    const show = () => {
      showTimer = undefined;
      if (disposed || pending === null) return;
      const next = pending;
      pending = null;
      lastShownAt = Date.now();
      setSample(next);
      setNow(lastShownAt);
      clearTimeout(staleTimer);
      staleTimer = setTimeout(() => {
        if (!disposed) setNow(Date.now());
      }, STALE_MS);
    };

    onAudioLevel((reading) => {
      if (disposed || reading.session !== session || reading.source !== source) return;
      pending = { level: reading.level, at: Date.now() };
      // Already waiting out the reduced-motion interval: the newest reading
      // replaces the one that was waiting, and goes when the wait ends.
      if (showTimer !== undefined) return;
      const wait = showDelay(lastShownAt, Date.now(), window.matchMedia(REDUCED_MOTION).matches);
      if (wait === 0) show();
      else showTimer = setTimeout(show, wait);
    })
      .then((stop) => {
        if (disposed) stop();
        else unlisten = stop;
      })
      .catch((cause: unknown) => {
        // A meter is decoration: a page that cannot hear levels still works,
        // and shows silence. Said in the console for whoever is looking.
        if (!disposed) console.warn("Could not listen for sound levels", cause);
      });

    return () => {
      disposed = true;
      unlisten?.();
      clearTimeout(showTimer);
      clearTimeout(staleTimer);
    };
  }, [session, source]);

  return levelAt(sample, now);
}
