import { useEffect } from "react";
import { LocalLanguagePicker } from "./LocalLanguagePicker";
import { setOverlayLanguageMenu } from "../lib/ipc";
export function LanguagePopup() {
  const close = () => { void setOverlayLanguageMenu(false); };
  useEffect(() => {
    const key = (event: KeyboardEvent) => { if (event.key === "Escape") close(); };
    document.addEventListener("keydown", key);
    return () => document.removeEventListener("keydown", key);
  }, []);
  return <div className="language-popup" onPointerDown={event => { if (event.target === event.currentTarget) close(); }}>
    <div className="microphone-language-menu" role="dialog" aria-label="Dictation languages">
      <LocalLanguagePicker /><button type="button" onClick={close}>Done</button>
    </div>
  </div>;
}
