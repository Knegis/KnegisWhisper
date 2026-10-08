import { listen } from "@tauri-apps/api/event";
/** Renders authoritative dictation state; the floating control shares the same pipeline. */
import { useEffect, useRef, useState } from "react";
import { getDictationState, onDictationState, onDictationOutcome, toggleDictation, getOverlayPreferences, onOverlayPreferences, dragOverlay, setOverlayMessage, setOverlayLanguageMenu, type SessionState, type OverlayPreferences } from "./lib/ipc";
import { microphoneState, isMicrophoneDrag } from "./lib/microphone";
import { LevelMeter } from "./components/LevelMeter";
import "./styles.css";
export function Overlay() {
  const [state, setState] = useState<SessionState | null>(null);
  const [unavailable, setUnavailable] = useState(false);
  const [prefs, setPrefs] = useState<OverlayPreferences | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [holdHover, setHoldHover] = useState(false);
  const [menuOpen, setMenuOpen] = useState(false);
  const [anchorOffset, setAnchorOffset] = useState({ x: 0, y: 0 });
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
        listen<{ x: number; y: number }>("overlay-anchor-offset", event => { if (!disposed) setAnchorOffset(event.payload); }),
        listen<boolean>("overlay-language-menu", event => { if (!disposed) setMenuOpen(event.payload); }),
        listen("overlay-outside-click", () => { if (!disposed) setMenuOpen(false); }),
        onDictationState(value => { revision++; if (!disposed) {
          setState(value);
          setUnavailable(false);
        } }),
        onOverlayPreferences(value => { prefRevision++; if (!disposed) {
          setPrefs(value);
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
  useEffect(() => { setError(null); setMenuOpen(false); }, [prefs?.enabled]);
  useEffect(() => {
    if (prefs?.enabled) void setOverlayMessage(error !== null).catch(() => { });
  }, [error, prefs?.enabled]);
  useEffect(() => {
    if (prefs?.enabled) void setOverlayLanguageMenu(menuOpen).catch(e => setError(String(e))).finally(() => setHoldHover(false));
  }, [menuOpen, prefs?.enabled]);
  useEffect(() => {
    if (!menuOpen) return;
    const closeOutside = (event: PointerEvent) => {
      if (event.target instanceof Element && !event.target.closest(".microphone-language-menu, .microphone-language-toggle")) setMenuOpen(false);
    };
    const escape = (event: KeyboardEvent) => { if (event.key === "Escape") setMenuOpen(false); };
    document.addEventListener("pointerdown", closeOutside);
    document.addEventListener("keydown", escape);
    return () => { document.removeEventListener("pointerdown", closeOutside); document.removeEventListener("keydown", escape); };
  }, [menuOpen]);
  const act = async () => {
    if (inFlight.current || state === null || (state !== "idle" && state !== "recording"))
      return;
    setMenuOpen(false);
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
    // The microphone's live level beside "Recording" (KNEG-371), mounted only while recording.
    return <div className={`overlay overlay-${state ?? "idle"}`}><span className={`indicator indicator-${state === "recording" ? "recording" : "idle"}`} aria-hidden="true"/><span className="overlay-text">{label}</span>{state === "recording" ? <LevelMeter session="dictation" source="microphone" segments={8} className="overlay-level"/> : null}</div>;
  }
  const view = microphoneState(state, unavailable);
  return <div className={`microphone-overlay ${prefs.vertical ? "is-vertical" : ""}`}>
  {!menuOpen && error && <div className="microphone-error" role="alert"><span>{error}</span>{state === null && <button onClick={() => { void getDictationState().then(value => { setState(value); setUnavailable(false); setError(null); }).catch(e => setError(String(e))); }}>Retry status</button>}<button aria-label="Dismiss message" onClick={() => setError(null)}>{"\u00d7"}</button></div>}
  <div key="microphone-controls" role="group" aria-label="Dictation controls" className={`microphone-controls ${holdHover ? "hold-hover" : ""}`} style={{ transform: `translate(${anchorOffset.x}px, ${anchorOffset.y}px)` }}>
  <button className={`floating-microphone ${view.recording ? "is-recording" : ""}`} data-expanded={state !== "idle" || pending || error !== null} aria-label={view.label} title={`${view.label}. Drag to move.`} aria-disabled={view.disabled || pending} aria-busy={view.busy || pending} onPointerDown={e => {
      if (menuOpen || e.button !== 0)
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
  } }} onClick={e => { if (menuOpen) { void act(); return; } if (pointerClick.current) {
    pointerClick.current = false;
    return;
  } if (e.detail === 0 && !view.disabled && !pending)
    void act(); }}>
   {view.busy ? <span className="microphone-spinner" aria-hidden="true"/> : <span className={`microphone-glyph ${view.recording ? "speaking" : ""}`} aria-hidden="true"/>}
   {/* A strip of the live level under the glyph while recording (KNEG-371). `speaking` stays the recording flag: the glyph is the privacy indicator, and a level-driven one would look idle through a pause. */}
   {view.recording ? <LevelMeter session="dictation" source="microphone" segments={6} className="microphone-level"/> : null}
  </button>
  <button type="button" className="microphone-language-toggle" aria-label="Choose dictation languages" aria-expanded={menuOpen} title="Dictation languages" onClick={() => { setHoldHover(true); setMenuOpen(value => !value); }}><span>{menuOpen ? "\u00d7" : ">"}</span></button>
  </div>
  <span className="microphone-status" aria-live="polite">{view.label}</span>
 </div>;
}
