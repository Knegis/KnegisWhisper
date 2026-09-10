/** Renders authoritative dictation state; the floating control shares the same pipeline. */
import { useEffect, useRef, useState } from "react";
import { getDictationState, onDictationState, onDictationOutcome, toggleDictation, getOverlayPreferences, onOverlayPreferences, dragOverlay, setOverlayMessage, type SessionState, type OverlayPreferences } from "./lib/ipc";
import { microphoneState, isMicrophoneDrag } from "./lib/microphone";
import "./styles.css";
export function Overlay() {
  const [state, setState] = useState<SessionState | null>(null);
  const [unavailable, setUnavailable] = useState(false);
  const [prefs, setPrefs] = useState<OverlayPreferences | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [pending, setPending] = useState(false);
  const inFlight = useRef(false);
  const pointerClick = useRef(false);
  const gesture = useRef<{
    x: number;
    y: number;
    dragged: boolean;
  } | null>(null);
  const queue = useRef<Promise<void>>(Promise.resolve());
  const moveQueued = useRef(false);
  const enqueue = (phase: "start" | "move" | "end" | "cancel") => {
    if (phase === "move" && moveQueued.current)
      return queue.current;
    if (phase === "move")
      moveQueued.current = true;
    queue.current = queue.current.then(() => dragOverlay(phase)).catch(e => { setError(String(e)); }).finally(() => { if (phase === "move")
      moveQueued.current = false; });
    return queue.current;
  };
  useEffect(() => {
    let disposed = false;
    const stops: (() => void)[] = [];
    let revision = 0;
    let prefRevision = 0;
    void (async () => {
      await Promise.all([
        onDictationState(value => { revision++; if (!disposed) {
          setState(value);
          setUnavailable(false);
        } }),
        onOverlayPreferences(value => { prefRevision++; if (!disposed) {
          setPrefs(value);
          setError(null);
        } }),
        onDictationOutcome(value => { if (!disposed && value.outcome === "failed")
          setError(value.message); }),
      ].map(async (subscription) => {
        const stop = await subscription;
        if (disposed)
          stop();
        else
          stops.push(stop);
      }));
      const before = revision;
      const beforePrefs = prefRevision;
      await Promise.all([
        getDictationState().then(value => { if (!disposed && before === revision)
          setState(value); }),
        getOverlayPreferences().then(value => { if (!disposed && beforePrefs === prefRevision)
          setPrefs(value); }),
      ]);
    })().catch(e => { if (!disposed) {
      setUnavailable(true);
      setError(String(e));
    } });
    return () => { disposed = true; stops.forEach(stop => stop()); };
  }, []);
  useEffect(() => {
    if (prefs?.enabled)
      void setOverlayMessage(error !== null).catch(() => { });
  }, [error, prefs?.enabled]);
  const act = async () => {
    if (inFlight.current || state === null || (state !== "idle" && state !== "recording"))
      return;
    inFlight.current = true;
    setPending(true);
    setError(null);
    try {
      const outcome = await toggleDictation();
      if (outcome.outcome === "failed")
        setError(outcome.message);
    }
    catch (e) {
      setError(String(e));
    }
    finally {
      // A failed start/finish must never leave a guessed recording state.
      try {
        setState(await getDictationState());
      }
      catch {
        setState(null);
        setUnavailable(true);
      }
      inFlight.current = false;
      setPending(false);
    }
  };
  if (!prefs?.enabled) {
    const label = state === null ? unavailable ? "State unknown" : "..." : state === "recording" ? "Recording" : state === "transcribing" ? "Transcribing" : state === "inserting" ? "Inserting" : "Ready";
    return <div className={`overlay overlay-${state ?? "idle"}`}><span className={`indicator indicator-${state === "recording" ? "recording" : "idle"}`} aria-hidden="true"/><span className="overlay-text">{label}</span></div>;
  }
  const view = microphoneState(state, unavailable);
  return <div className="microphone-overlay">
  {error && <div className="microphone-error" role="alert"><span>{error}</span>{state === null && <button onClick={() => { void getDictationState().then(value => { setState(value); setUnavailable(false); setError(null); }).catch(e => setError(String(e))); }}>Retry status</button>}<button aria-label="Dismiss message" onClick={() => setError(null)}>{"\u00d7"}</button></div>}
  <button className={`floating-microphone ${view.recording ? "is-recording" : ""}`} aria-label={view.label} title={`${view.label}. Drag to move.`} aria-disabled={view.disabled || pending} aria-busy={view.busy || pending} onPointerDown={e => {
      if (e.button !== 0)
        return;
      e.preventDefault();
      pointerClick.current = true;
      e.currentTarget.setPointerCapture(e.pointerId);
      gesture.current = { x: e.screenX, y: e.screenY, dragged: false };
      void enqueue("start");
    }} onPointerMove={e => {
      const g = gesture.current;
      if (!g)
        return;
      g.dragged ||= isMicrophoneDrag(e.screenX, e.screenY, g.x, g.y);
      if (g.dragged)
        void enqueue("move");
    }} onPointerUp={e => {
      const g = gesture.current;
      gesture.current = null;
      if (!g)
        return;
      e.currentTarget.releasePointerCapture(e.pointerId);
      void enqueue(g.dragged ? "end" : "cancel").then(() => { if (!g.dragged && !view.disabled && !pending)
        void act(); });
    }} onPointerCancel={() => { gesture.current = null; void enqueue("cancel"); }} onLostPointerCapture={() => { if (gesture.current) {
    gesture.current = null;
    void enqueue("cancel");
  } }} onClick={e => { if (pointerClick.current) {
    pointerClick.current = false;
    return;
  } if (e.detail === 0 && !view.disabled && !pending)
    void act(); }}>
   {view.busy ? <span className="microphone-spinner" aria-hidden="true"/> : <span className={`microphone-glyph ${view.recording ? "speaking" : ""}`} aria-hidden="true"/>}
  </button>
  <span className="microphone-status" aria-live="polite">{view.label}</span>
 </div>;
}
