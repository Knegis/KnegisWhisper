import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { LocalLanguagePicker } from "./LocalLanguagePicker";
import { LANGUAGE_NOTE_LEAD, languageNote } from "../lib/controlBar";
import { languagesApply, localEngine } from "../lib/dictationRoute";
import {
  getDictationRoute,
  getLocalDictationLanguages,
  getSherpaStatus,
  onLocalDictationLanguages,
  setOverlayLanguageMenu,
} from "../lib/ipc";

/**
 * The floating microphone's language menu, in its own `dictation-languages`
 * window. No picker while a Parakeet model is the local engine, as Settings ›
 * Transcription's is off then: a Parakeet model uses its own languages
 * (KNEG-386). On every other route -- local Whisper or a cloud provider --
 * the picker, with the line saying what that route does with the languages
 * (`languageNote`, KNEG-370). The window is built once and shown and hidden,
 * so the route and the engine are read again each time the menu opens, and
 * again when the languages are announced, since the note follows them.
 */
export function LanguagePopup() {
  // `undefined` until the first read answers. A failed read leaves the picker
  // usable: locking it would hide the languages over an IPC error.
  const [shown, setShown] = useState<{ applies: boolean; note: string } | undefined>(undefined);
  useEffect(() => {
    let disposed = false;
    // Reads can overlap -- the menu opening as a choice is announced -- and
    // only the last one asked for is shown.
    let latest = 0;
    const read = () => {
      const mine = ++latest;
      void Promise.all([getDictationRoute(), getSherpaStatus(), getLocalDictationLanguages()])
        .then(([route, sherpa, languages]) => {
          if (disposed || mine !== latest) return;
          const engine = localEngine(route, sherpa);
          setShown({ applies: languagesApply(engine), note: languageNote(engine, route, languages) });
        })
        .catch(() => { if (!disposed && mine === latest) setShown({ applies: true, note: LANGUAGE_NOTE_LEAD }); });
    };
    read();
    const stopMenu = listen<boolean>("overlay-language-menu", event => { if (event.payload) read(); });
    const stopLanguages = onLocalDictationLanguages(() => read());
    return () => {
      disposed = true;
      void stopMenu.then(unlisten => unlisten());
      void stopLanguages.then(unlisten => unlisten());
    };
  }, []);
  const close = () => { void setOverlayLanguageMenu(false); };
  useEffect(() => {
    const key = (event: KeyboardEvent) => { if (event.key === "Escape") close(); };
    document.addEventListener("keydown", key);
    return () => document.removeEventListener("keydown", key);
  }, []);
  return <div className="language-popup" onPointerDown={event => { if (event.target === event.currentTarget) close(); }}>
    <div className="microphone-language-menu" role="dialog" aria-label="Dictation languages">
      {/* Replaced rather than shown disabled: the window is 272px tall, and the
          picker plus a line saying why it is off pushed Done out of it. */}
      {shown !== undefined && !shown.applies
        ? <p className="muted">Parakeet is the local engine, and it uses its model's own languages, so there is nothing to choose here.</p>
        : <LocalLanguagePicker disabled={shown === undefined} note={shown?.note ?? LANGUAGE_NOTE_LEAD} />}
      <button type="button" className="language-done" onClick={close}>Done</button>
    </div>
  </div>;
}
