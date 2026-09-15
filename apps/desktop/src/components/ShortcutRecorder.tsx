import { useEffect, useRef, useState } from "react";
import { startHotkeyCapture, endHotkeyCapture } from "../lib/ipc";
import { ShortcutCapture } from "../lib/shortcutCapture";

export function ShortcutRecorder({ onCaptured, onError, onBusy, disabled }: {
  onCaptured: (value: string) => void;
  onError: (message: string) => void;
  onBusy: (busy: boolean) => void;
  disabled: boolean;
}) {
  const [active, setActive] = useState(false);
  const [sided, setSided] = useState(false);
  const [ready, setReady] = useState(false);
  const [preview, setPreview] = useState("");
  const [hint, setHint] = useState("");
  const area = useRef<HTMLDivElement>(null);
  const callbacks = useRef({ onCaptured, onError, onBusy });
  callbacks.current = { onCaptured, onError, onBusy };

  useEffect(() => {
    if (!active) return;
    const chord = new ShortcutCapture(sided);
    let token: number | undefined;
    let disposed = false;
    let finishing = false;
    let timer: ReturnType<typeof setTimeout> | undefined;
    callbacks.current.onBusy(true);
    setReady(false); setPreview(""); setHint("");
    const finish = async (value?: string) => {
      if (finishing) return;
      finishing = true;
      setReady(false);
      try {
        if (token !== undefined) { const current = token; token = undefined; await endHotkeyCapture(current); }
        if (!disposed && value) callbacks.current.onCaptured(value);
      } catch (error) { callbacks.current.onError(String(error)); }
      finally { if (!disposed) setActive(false); }
    };
    const down = (event: KeyboardEvent) => {
      event.preventDefault(); event.stopImmediatePropagation();
      if (event.key === "Escape") { void finish(); return; }
      if (token === undefined || finishing || event.repeat || event.isComposing) return;
      const result = chord.down(event.code);
      setPreview(result.preview); setHint(result.error ?? "Release all keys to use this shortcut.");
    };
    const up = (event: KeyboardEvent) => {
      event.preventDefault(); event.stopImmediatePropagation();
      if (token === undefined || finishing) return;
      const value = chord.up(event.code);
      if (value) void finish(value);
    };
    const pointer = (event: PointerEvent) => {
      if (!area.current?.contains(event.target as Node)) { void finish(); return; }
      if (event.button === 3 || event.button === 4) {
        event.preventDefault(); event.stopImmediatePropagation();
        if (token === undefined || finishing) return;
        const code = `Mouse${event.button + 1}`;
        if (event.type === "pointerdown") { const result = chord.down(code); setPreview(result.preview); setHint(result.error ?? "Release to use this shortcut."); }
        else { const value = chord.up(code); if (value) void finish(value); }
      }
    };
    const blur = () => { void finish(); };
    window.addEventListener("keydown", down, true);
    window.addEventListener("keyup", up, true);
    window.addEventListener("pointerdown", pointer, true);
    window.addEventListener("pointerup", pointer, true);
    window.addEventListener("blur", blur);
    void startHotkeyCapture().then(async value => {
      if (disposed || finishing) { await endHotkeyCapture(value); return; }
      token = value;
      setReady(true);
      area.current?.focus();
      timer = setTimeout(() => { setHint("Recording timed out. Try again."); void finish(); }, 20000);
    }).catch(error => { callbacks.current.onError(String(error)); if (!disposed) setActive(false); });
    return () => {
      disposed = true;
      clearTimeout(timer);
      window.removeEventListener("keydown", down, true);
      window.removeEventListener("keyup", up, true);
      window.removeEventListener("pointerdown", pointer, true);
      window.removeEventListener("pointerup", pointer, true);
      window.removeEventListener("blur", blur);
      if (token !== undefined) void endHotkeyCapture(token).catch(error => callbacks.current.onError(String(error)));
      callbacks.current.onBusy(false);
    };
  }, [active, sided]);

  return <div className="shortcut-recorder" ref={area} tabIndex={active ? 0 : -1}>
    <button type="button" disabled={disabled && !active} onClick={() => setActive(value => !value)}>{active ? "Cancel" : "Record shortcut"}</button>
    <label><input type="checkbox" checked={sided} disabled={active || disabled} onChange={event => setSided(event.target.checked)} />Distinguish left/right modifiers</label>
    {active && <p role="status">{ready ? preview || "Press your shortcut…" : "Preparing…"} {ready && (hint || "Release to finish. Escape cancels.")}</p>}
  </div>;
}
