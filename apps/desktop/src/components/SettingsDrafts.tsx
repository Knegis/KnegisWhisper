import { createContext, useCallback, useContext, useEffect, useId, useLayoutEffect, useRef, useState, type ReactNode } from "react";
import { SettingsDraftRegistry } from "../lib/settingsDrafts";

type DraftContext = { registry: SettingsDraftRegistry; request: (action: () => void) => Promise<boolean> };
const Context = createContext<DraftContext | null>(null);
export function useSettingsNavigation() {
  const context = useContext(Context);
  return context?.request ?? (async (action: () => void) => { action(); return true; });
}
export function SettingsDraftsProvider({ children }: { children: ReactNode }) {
  const [registry] = useState(() => new SettingsDraftRegistry());
  const [, refreshDrafts] = useState(0);
  useEffect(() => registry.subscribe(() => refreshDrafts(value => value + 1)), [registry]);
  const [pending, setPending] = useState<{ action: () => void; resolve: (ok: boolean) => void; origin: HTMLElement | null } | null>(null);
  const held = useRef(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const dialog = useRef<HTMLDialogElement>(null);
  const stay = useRef<HTMLButtonElement>(null);
  const request = useCallback(async (action: () => void) => {
    if (held.current) return false;
    held.current = true;
    const origin = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    await registry.settle();
    if (!registry.dirty().length) { held.current = false; action(); return true; }
    setError(null);
    return new Promise<boolean>(resolve => setPending({ action, resolve, origin }));
  }, [registry]);
  useEffect(() => {
    if (pending && dialog.current && !dialog.current.open) {
      dialog.current.showModal();
      // showModal runs after React's autoFocus and otherwise focuses the first action.
      stay.current?.focus();
    }
  }, [pending]);
  const finish = (leave: boolean) => {
    const current = pending;
    dialog.current?.close(); setPending(null); held.current = false;
    if (!current) return;
    if (leave) current.action();
    else if (current.origin?.isConnected) current.origin.focus({ preventScroll: true });
    current.resolve(leave);
  };
  const decide = async (save: boolean) => {
    setBusy(true); setError(null);
    try { if (save) await registry.save(); else await registry.discard(); finish(true); }
    catch (cause) { setError(cause instanceof Error ? cause.message : String(cause)); }
    finally { setBusy(false); }
  };
  return <Context.Provider value={{ registry, request }}>{children}
    {pending && <dialog ref={dialog} className="settings-drafts-dialog" aria-labelledby="settings-drafts-title" onCancel={event => { event.preventDefault(); if (!busy) finish(false); }}>
      <h2 id="settings-drafts-title" className="confirm-dialog-title">Unsaved settings</h2>
      <p className="confirm-dialog-detail">Save your changes before leaving, or discard the changes that haven't been saved.</p>
      <ul>{registry.dirty().map(entry => <li key={entry.label}>{entry.label}</li>)}</ul>
      {error && <p role="alert" className="error">{error}</p>}
      <div className="confirm-dialog-actions">
        <button type="button" className="primary" disabled={busy} onClick={() => void decide(true)}>{busy ? "Saving…" : "Save changes"}</button>
        <button type="button" disabled={busy} onClick={() => void decide(false)}>Discard changes</button>
        <button type="button" ref={stay} disabled={busy} autoFocus onClick={() => finish(false)}>Stay</button>
      </div>
    </dialog>}
  </Context.Provider>;
}

/** Serialize writes, expose their failures and register the current form operations. */
export function useSettingsDraft({ label, order = 0, dirty, save, discard }: { label: string; order?: number; dirty: boolean; save: () => Promise<void>; discard: () => void }) {
  const context = useContext(Context);
  const id = useId();
  const [state, setState] = useState<"idle" | "saving" | "saved" | "failed">("idle");
  const [error, setError] = useState<string | null>(null);
  const latest = useRef({ label, order, dirty, save, discard }); latest.current = { label, order, dirty, save, discard };
  const writes = useRef(Promise.resolve());
  const alive = useRef(true);
  const sequence = useRef(0);
  const intents = useRef(new Map<string, number>());
  const failedWrites = useRef(new Map<string, { work: () => Promise<void>; error: string }>());
  const run = useCallback((work: () => Promise<void>, intent = "draft") => {
    const revision = ++sequence.current;
    intents.current.set(intent, revision);
    failedWrites.current.delete(intent);
    setState("saving"); setError(null);
    const write = writes.current.then(work);
    writes.current = write.then(() => undefined, () => undefined);
    return write.then(() => {
      if (alive.current && revision === sequence.current) {
        const failed = failedWrites.current.values().next().value;
        setState(failed ? "failed" : "saved"); setError(failed?.error ?? null);
      }
    }, cause => {
      const detail = cause instanceof Error ? cause.message : String(cause);
      if (intents.current.get(intent) === revision) failedWrites.current.set(intent, { work, error: detail });
      if (alive.current && revision === sequence.current) { setState("failed"); setError(detail); }
      throw cause;
    });
  }, []);
  useLayoutEffect(() => {
    alive.current = true;
    const unregister = context?.registry.register(id, {
      get label() { return latest.current.label; }, get order() { return latest.current.order; }, dirty: () => latest.current.dirty || failedWrites.current.size > 0,
      settle: async () => { for (;;) { const writing = writes.current; await writing; if (writing === writes.current) break; } },
      save: async () => {
        if (latest.current.dirty) await run(() => latest.current.save());
        for (const [intent, failed] of [...failedWrites.current]) await run(failed.work, intent);
      },
      discard: () => { failedWrites.current.clear(); latest.current.discard(); setState("idle"); setError(null); },
    });
    return () => { alive.current = false; unregister?.(); };
  }, [context?.registry, id, run]);
  const wasDirty = useRef(dirty);
  useLayoutEffect(() => {
    if (wasDirty.current && !dirty && failedWrites.current.delete("draft")) {
      const failed = failedWrites.current.values().next().value;
      setState(failed ? "failed" : "idle"); setError(failed?.error ?? null);
    }
    wasDirty.current = dirty;
  }, [dirty]);
  // Refresh the dialog after a child form acknowledges a write or changes its draft.
  useLayoutEffect(() => { context?.registry.notify(); }, [context?.registry, dirty, state, label]);
  const status = state === "saving" ? "Saving…" : state === "failed" ? "Couldn't save" : dirty ? "Unsaved changes" : state === "saved" ? "Saved" : null;
  const feedback = status ? <span className={state === "failed" ? "settings-draft-status error" : "settings-draft-status muted"} role="status" aria-label={`${label}: ${status}`}>{status}{error ? <span className="settings-draft-error">{error}</span> : null}</span> : null;
  return { run, feedback };
}
