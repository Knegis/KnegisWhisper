import { useCallback, useEffect, useRef, useState } from "react";
import { message } from "./useRefresh";

/** Library refreshes and acknowledged metadata updates share a generation. */
export function useNotesLibrary<T>(load: () => Promise<T>) {
  const [data, setData] = useState<T | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [revision, setRevision] = useState(0);
  const generation = useRef(0);
  const alive = useRef(true);
  const fail = useCallback((cause: unknown) => { if (alive.current) setError(message(cause)); }, []);
  const reload = useCallback(async () => {
    const token = ++generation.current;
    try {
      const fresh = await load();
      if (alive.current && token === generation.current) { setData(fresh); setRevision(value => value + 1); }
    } catch (cause) { if (token === generation.current) fail(cause); }
  }, [load, fail]);
  useEffect(() => { alive.current = true; void reload(); return () => { alive.current = false; ++generation.current; }; }, [reload]);
  const guard = useCallback(async (work: () => Promise<unknown>) => {
    setError(null);
    try { await work(); await reload(); } catch (cause) { fail(cause); }
  }, [reload, fail]);
  const clearError = useCallback(() => setError(null), []);
  const beginUpdate = useCallback(() => ++generation.current, []);
  const isCurrent = useCallback((token: number) => alive.current && token === generation.current, []);
  const apply = useCallback((token: number, change: (current: T) => T) => {
    if (alive.current && token === generation.current) setData(current => current === null ? current : change(current));
  }, []);
  return { data, error, guard, reload, fail, revision, beginUpdate, apply, isCurrent, clearError };
}
