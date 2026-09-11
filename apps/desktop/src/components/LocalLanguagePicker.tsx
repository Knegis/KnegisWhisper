import { useEffect, useState } from "react";
import { getLocalDictationLanguages, setLocalDictationLanguages, onLocalDictationLanguages } from "../lib/ipc";

const LANGUAGES = [["sv", "Swedish"], ["en", "English"], ["da", "Danish"], ["no", "Norwegian"], ["fi", "Finnish"], ["de", "German"], ["fr", "French"], ["es", "Spanish"]] as const;

export function LocalLanguagePicker({ disabled = false }: { disabled?: boolean }) {
  const [selected, setSelected] = useState<string[] | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
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
    }).catch(e => { if (!disposed) setError(String(e)); });
    return () => { disposed = true; stop?.(); };
  }, []);
  const save = async (languages: string[]) => {
    setBusy(true); setError(null);
    try { await setLocalDictationLanguages(languages); setSelected(languages); }
    catch (e) { setError(String(e)); }
    finally { setBusy(false); }
  };
  return <fieldset className="local-language-picker" disabled={disabled || busy || selected === null}>
    <legend>Whisper dictation languages</legend>
    <button type="button" aria-pressed={selected?.length === 0} className={selected?.length === 0 ? "marked" : undefined} onClick={() => void save([])}>Detect automatically</button>
    <div className="local-language-options">
      {LANGUAGES.map(([code, label]) => <label key={code}>
        <input type="checkbox" checked={selected?.includes(code) ?? false}
          disabled={!selected?.includes(code) && selected?.length === 2}
          onChange={e => void save(e.target.checked ? [...(selected ?? []), code] : (selected ?? []).filter(value => value !== code))} />{label}
      </label>)}
    </div>
    <p className="muted">Choose up to two. With two, Whisper detects which one to use for each dictation.</p>
    {error && <p className="error" role="alert">{error}</p>}
  </fieldset>;
}
