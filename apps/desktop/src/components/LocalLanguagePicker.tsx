import { useEffect, useRef, useState } from "react";
import { FailureLine } from "./FailureLine";
import { LANGUAGES } from "../lib/controlBar";
import { getLocalDictationLanguages, setLocalDictationLanguages, onLocalDictationLanguages } from "../lib/ipc";

/**
 * `initial` is the value a page has already read, so the picker opens usable
 * rather than disabled until its own read answers -- in the Dictation chip's
 * panel that put focus on the panel instead of the first control.
 *
 * While a choice is saved, further choices are ignored rather than the
 * fieldset disabled: disabling the box that has focus drops focus to the page,
 * which left the Dictation panel with nothing for Escape to close (KNEG-385).
 *
 * In plain words (KNEG-343): its legend names no engine, and a failure is a
 * sentence with the error under Details. The languages are Dictation's, for
 * local Whisper and cloud routes alike (KNEG-370), so the line under the
 * choices is the page's to give: `note`, from `languageNote`, which says what
 * the route in use does with them.
 */
export function LocalLanguagePicker({
  disabled = false,
  initial,
  note,
}: {
  disabled?: boolean;
  initial?: readonly string[];
  note: string;
}) {
  const [selected, setSelected] = useState<string[] | null>(initial ? [...initial] : null);
  const [busy, setBusy] = useState(false);
  const saving = useRef(false);
  const [error, setError] = useState<{ text: string; detail: string } | null>(null);
  useEffect(() => {
    let disposed = false;
    let stop: (() => void) | undefined;
    let revision = 0;
    void onLocalDictationLanguages(value => {
      revision++;
      if (!disposed) setSelected(value);
    }).then(async unlisten => {
      if (disposed) { unlisten(); return; }
      stop = unlisten;
      const before = revision;
      const value = await getLocalDictationLanguages();
      if (!disposed && before === revision) setSelected(value);
    }).catch(e => { if (!disposed) setError({ text: "Couldn't read the dictation languages.", detail: String(e) }); });
    return () => { disposed = true; stop?.(); };
  }, []);
  const save = async (languages: string[]) => {
    if (saving.current) return;
    saving.current = true;
    setBusy(true); setError(null);
    try { await setLocalDictationLanguages(languages); setSelected(languages); }
    catch (e) { setError({ text: "Couldn't save the dictation languages.", detail: String(e) }); }
    finally { saving.current = false; setBusy(false); }
  };
  return <fieldset className="local-language-picker" disabled={disabled || selected === null} aria-busy={busy || undefined}>
    <legend>Dictation languages</legend>
    <button type="button" aria-pressed={selected?.length === 0} className={selected?.length === 0 ? "marked" : undefined} onClick={() => void save([])}>Detect automatically</button>
    <div className="local-language-options">
      {LANGUAGES.map(([code, label]) => <label key={code}>
        <input type="checkbox" checked={selected?.includes(code) ?? false}
          disabled={!selected?.includes(code) && selected?.length === 2}
          onChange={e => void save(e.target.checked ? [...(selected ?? []), code] : (selected ?? []).filter(value => value !== code))} />{label}
      </label>)}
    </div>
    <p className="muted">{note}</p>
    {error && <FailureLine text={error.text} detail={error.detail} />}
  </fieldset>;
}
