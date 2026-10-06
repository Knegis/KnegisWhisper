import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { LocalLanguagePicker } from "./LocalLanguagePicker";
import { getSherpaStatus, setOverlayLanguageMenu } from "../lib/ipc";

/**
 * The floating microphone's language menu, in its own `dictation-languages`
 * window. No picker while a Parakeet model is the local engine, as Settings ›
 * Transcription's is off then: the languages apply to local Whisper, and a
 * Parakeet model uses its own (KNEG-386). The window is built once and shown
 * and hidden, so the engine is read again each time the menu opens.
 */
export function LanguagePopup() {
  // `undefined` until the first read answers. A failed read leaves the picker
  // usable: locking it would hide the Whisper languages over an IPC error.
  const [parakeet, setParakeet] = useState<boolean | undefined>(undefined);
  useEffect(() => {
    let disposed = false;
    const read = () => {
      void getSherpaStatus()
        .then(status => { if (!disposed) setParakeet(status.selected !== null); })
        .catch(() => { if (!disposed) setParakeet(false); });
    };
    read();
    const stop = listen<boolean>("overlay-language-menu", event => { if (event.payload) read(); });
    return () => { disposed = true; void stop.then(unlisten => unlisten()); };
  }, []);
  const close = () => { void setOverlayLanguageMenu(false); };
  useEffect(() => {
    const key = (event: KeyboardEvent) => { if (event.key === "Escape") close(); };
    document.addEventListener("keydown", key);
    return () => document.removeEventListener("keydown", key);
  }, []);
  return <div className="language-popup" onPointerDown={event => { if (event.target === event.currentTarget) close(); }}>
    <div className="microphone-language-menu" role="dialog" aria-label="Dictation languages">
      {/* Replaced rather than shown disabled: the window is 252px tall, and the
          picker plus a line saying why it is off pushed Done out of it. */}
      {parakeet
        ? <p className="muted">Parakeet is the local engine, and it uses its model's own languages, so there is nothing to choose here.</p>
        : <LocalLanguagePicker disabled={parakeet === undefined} />}
      <button type="button" onClick={close}>Done</button>
    </div>
  </div>;
}
