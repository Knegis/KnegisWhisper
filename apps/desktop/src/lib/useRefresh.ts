/**
 * The read-then-re-read pattern every view uses, lifted out of `App.tsx`.
 *
 * It is one small hook rather than a store because of what it already got
 * right: after any mutation it *re-reads from Rust* instead of updating local
 * state optimistically. That is why the hotkey row shows what was actually
 * registered rather than what was typed, and why "Use for dictation" reflects
 * the route the backend resolved rather than the one the click intended. A view
 * that keeps its own copy of backend state is a view that can disagree with it.
 *
 * `guard` is the other half: it clears the error, runs the mutation, re-reads,
 * and puts any failure on screen. Nothing here swallows an exception — a button
 * that silently does nothing is the defect this shape exists to prevent.
 */

import { useCallback, useEffect, useState, useRef } from "react";

/** Turn anything thrown into something showable. */
export function message(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}

export type Refreshed<T> = {
  /** `null` until the first read completes. Distinct from an empty result. */
  data: T | null;
  error: string | null;
  /** Re-read. Safe to call from an event handler. */
  reload: () => Promise<void>;
  /** Run a mutation, then re-read. Failures land in `error`. */
  guard: (work: () => Promise<unknown>) => Promise<void>;
  /** Report a failure that was not a mutation, e.g. a search that threw. */
  fail: (e: unknown) => void;
  /** Clear the shown error without re-reading. */
  clearError: () => void;
};

/**
 * Read `load()` on mount and after every mutation.
 *
 * `load` must be memoised by the caller — `useCallback` with the dependencies
 * that actually change what it fetches. An inline lambda re-reads forever.
 */
export function useRefresh<T>(load: () => Promise<T>): Refreshed<T> {
  const [data, setData] = useState<T | null>(null);
  const [error, setError] = useState<string | null>(null);

  const readGeneration = useRef(0);
  const mutationGeneration = useRef(0);
  const reload = useCallback(async () => {
    const generation = ++readGeneration.current;
    const mutation = mutationGeneration.current;
    try {
      const next = await load();
      if (generation === readGeneration.current && mutation === mutationGeneration.current) setData(next);
    } catch (e) {
      if (generation === readGeneration.current && mutation === mutationGeneration.current) setError(message(e));
    }
  }, [load]);

  useEffect(() => {
    void reload();
    return () => { readGeneration.current++; mutationGeneration.current++; };
  }, [reload]);

  const guard = useCallback(
    async (work: () => Promise<unknown>) => {
      const generation = ++mutationGeneration.current;
      readGeneration.current++;
      setError(null);
      try {
        await work();
        if (generation === mutationGeneration.current) await reload();
      } catch (e) {
        if (generation === mutationGeneration.current) setError(message(e));
      }
    },
    [reload],
  );

  const fail = useCallback((e: unknown) => setError(message(e)), []);
  const clearError = useCallback(() => setError(null), []);

  return { data, error, reload, guard, fail, clearError };
}
