import { useSettingsDraft } from "./SettingsDrafts";
import { useEffect, useRef, useState } from "react";
import { FailureLine } from "./FailureLine";
import { LANGUAGES } from "../lib/controlBar";
import { getLocalDictationLanguages, setLocalDictationLanguages, onLocalDictationLanguages } from "../lib/ipc";

/**
 * `initial` is the value a page has already read, so the picker opens usable
 * rather than disabled until its own read answers -- in the Dictation chip's
 * panel that put focus on the panel instead of the first control.
 *
 * Choices are saved in order while the controls keep focus. Disabling the
 * fieldset that has focus drops focus to the page,
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
  const committed = useRef<string[] | null>(initial ? [...initial] : null);
  const saveGeneration = useRef(0);
  const saving = useRef(false);
  const wanted = useRef(selected);
  const alive = useRef(true);
  const [error, setError] = useState<{ text: string; detail: string } | null>(null);
  useEffect(() => {
    let disposed = false;
    alive.current = true;
    let stop: (() => void) | undefined;
    let revision = 0;
    void onLocalDictationLanguages(value => {
      revision++;
      if (!disposed && !saving.current && JSON.stringify(wanted.current) === JSON.stringify(committed.current)) { committed.current = value; wanted.current = value; setSelected(value); }
    }).then(async unlisten => {
      if (disposed) { unlisten(); return; }
      stop = unlisten;
      const before = revision;
      const value = await getLocalDictationLanguages();
      if (!disposed && before === revision && !saving.current) { committed.current = value; wanted.current = value; setSelected(value); }
    }).catch(e => { if (!disposed) setError({ text: "Couldn't read the dictation languages.", detail: String(e) }); });
    return () => { disposed = true; alive.current = false; stop?.(); };
  }, []);
  const persist = async (languages: string[]) => {
    await setLocalDictationLanguages(languages);
    committed.current = languages;
  };
  const languageDraft = useSettingsDraft({ label: "Dictation languages", dirty: JSON.stringify(selected) !== JSON.stringify(committed.current),
    save: async () => { if (wanted.current !== null) await persist(wanted.current); },
    discard: () => { wanted.current = committed.current; setSelected(committed.current); setError(null); } });
  const save = async (languages: string[]) => {
    const generation = ++saveGeneration.current;
    saving.current = true; wanted.current = languages; setSelected(languages);
    setBusy(true); setError(null);
    try { await languageDraft.run(() => persist(languages)); }
    catch (e) { if (alive.current && generation === saveGeneration.current) setError({ text: "Couldn't save the dictation languages.", detail: String(e) }); }
    finally { if (alive.current && generation === saveGeneration.current) { setBusy(false); saving.current = false; } }
  };

  return <fieldset className="local-language-picker" disabled={disabled || selected === null} aria-busy={busy || undefined}>
    <legend>Dictation languages</legend>
    <div className="local-language-options">
      <button type="button" className="language-pill" aria-pressed={selected?.length === 0} onClick={() => void save([])}>Detect automatically</button>
      {LANGUAGES.map(([code, label]) => {
        const on = selected?.includes(code) ?? false;
        return <button key={code} type="button" className="language-pill" aria-pressed={on}
          disabled={!on && selected?.length === 2}
          onClick={() => void save(on ? (wanted.current ?? []).filter(value => value !== code) : [...(wanted.current ?? []), code])}>{label}</button>;
      })}
    </div>
    <p className="muted">{note}</p>
    {languageDraft.feedback}
    {error && <FailureLine text={error.text} detail={error.detail} />}
  </fieldset>;
}
