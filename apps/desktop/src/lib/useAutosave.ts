import { useCallback, useEffect, useRef, useState } from "react";

export type AutosaveStatus = "idle" | "dirty" | "saving" | "saved" | "failed";

type AutosaveOptions<T extends object> = {
  save: (patch: Partial<T>) => Promise<void>;
  delayMs?: number | undefined;
  paused?: boolean;
};

type Autosave<T extends object> = {
  status: AutosaveStatus;
  error: string | null;
  schedule: (patch: Partial<T>) => void;
  flush: () => Promise<void>;
  retry: () => Promise<void>;
  discard: () => void;
};

function message(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

/**
 * Debounce a patch stream without letting requests race each other. A later
 * patch is held until the preceding request has completed, so an old response
 * can never write over newer typing.
 */
export function useAutosave<T extends object>({
  save,
  delayMs = 800,
  paused = false,
}: AutosaveOptions<T>): Autosave<T> {
  const saveRef = useRef(save);
  const pausedRef = useRef(paused);
  pausedRef.current = paused;
  const delayRef = useRef(delayMs);
  const pendingRef = useRef<Partial<T> | null>(null);
  const timerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const inFlightRef = useRef(false);
  const failureRef = useRef<string | null>(null);
  const rerunRef = useRef(false);
  const activeSaveRef = useRef<Promise<void> | null>(null);
  const [status, setStatus] = useState<AutosaveStatus>("idle");
  const [error, setError] = useState<string | null>(null);

  saveRef.current = save;
  delayRef.current = delayMs;

  const clearTimer = useCallback(() => {
    if (timerRef.current !== null) {
      clearTimeout(timerRef.current);
      timerRef.current = null;
    }
  }, []);

  const run = useCallback((): Promise<void> => {
    clearTimer();
    if (pausedRef.current) return Promise.resolve();
    if (inFlightRef.current) {
      rerunRef.current = true;
      return activeSaveRef.current ?? Promise.resolve();
    }

    const patch = pendingRef.current;
    if (patch === null) return Promise.resolve();
    pendingRef.current = null;
    inFlightRef.current = true;
    rerunRef.current = false;
    setStatus("saving");
    setError(null);

    const active = (async () => {
      try {
        await saveRef.current(patch);
        failureRef.current = null;
        if (pendingRef.current !== null || rerunRef.current) {
          inFlightRef.current = false;
          await run();
          return;
        }
        setStatus("saved");
      } catch (cause) {
        pendingRef.current = { ...patch, ...(pendingRef.current ?? {}) };
        failureRef.current = message(cause);
        setError(failureRef.current);
        setStatus("failed");
      } finally {
        inFlightRef.current = false;
      }
    })();
    activeSaveRef.current = active;
    return active;
  }, [clearTimer]);

  const schedule = useCallback(
    (patch: Partial<T>) => {
      pendingRef.current = { ...pendingRef.current, ...patch };
      setError(null);
      setStatus("dirty");
      clearTimer();
      timerRef.current = setTimeout(() => {
        void run();
      }, delayRef.current);
    },
    [clearTimer, run],
  );

  const flush = useCallback(async () => {
    clearTimer();
    await run();
    if (pausedRef.current && pendingRef.current !== null) throw new Error("This note is read-only. Copy or discard your unsaved draft, or restore the note and save it explicitly.");
    if (failureRef.current !== null) throw new Error(failureRef.current);
  }, [clearTimer, run]);

  const retry = useCallback(async () => {
    if (pendingRef.current === null) return;
    await flush();
  }, [flush]);

  useEffect(() => {
    return () => {
      // React cleanup cannot await this; the pending edit still starts saving.
      void flush().catch(() => undefined);
    };
  }, [flush]);

  const discard = useCallback(() => { clearTimer(); pendingRef.current = null; failureRef.current = null; setError(null); setStatus("idle"); }, [clearTimer]);
  return { status, error, schedule, flush, retry, discard };
}
