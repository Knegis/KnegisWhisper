import { useSettingsDraft } from "./SettingsDrafts";
import { useEffect, useState } from "react";
import { FailureLine } from "./FailureLine";
import { getOverlayPreferences, setOverlayPreferences, resetOverlayPosition, onOverlayPreferences, type OverlayPreferences } from "../lib/ipc";
export function MicrophoneSettings() {
  const [prefs, setPrefs] = useState<OverlayPreferences | null>(null);
  // A failure is a sentence, with the error under Details (KNEG-343).
  const [error, setError] = useState<{ text: string; detail: string } | null>(null);
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    let disposed = false;
    let stop: (() => void) | undefined;
    let revision = 0;
    void onOverlayPreferences(value => { revision++; if (!disposed)
      setPrefs(value); }).then(async (fn) => {
      if (disposed) {
        fn();
        return;
      }
      stop = fn;
      const before = revision;
      const value = await getOverlayPreferences();
      if (!disposed && revision === before)
        setPrefs(value);
    }).catch(e => { if (!disposed)
      setError({ text: "Couldn't read the dictation microphone setting.", detail: String(e) }); });
    return () => { disposed = true; stop?.(); };
  }, []);
  const preferenceDraft = useSettingsDraft({ label: "Dictation microphone", dirty: false, save: async () => {}, discard: () => {} });
  const change = async (action: () => Promise<unknown>, intent: string) => {
    setBusy(true);
    setError(null);
    try {
      await preferenceDraft.run(async () => { await action(); }, intent);
      try { setPrefs(await getOverlayPreferences()); }
      catch (e) { setError({ text: "The setting changed, but couldn't be refreshed.", detail: String(e) }); }
    }
    catch (e) {
      setError({ text: "Couldn't change the dictation microphone.", detail: String(e) });
    }
    finally {
      setBusy(false);
    }
  };
  return <section data-surface="settings.microphone" aria-labelledby="floating-microphone-heading">
  <h2 id="floating-microphone-heading">Dictation microphone</h2>
  <label className="microphone-setting"><input type="checkbox" checked={prefs?.enabled ?? false} disabled={!prefs?.supported || busy} onChange={e => { const enabled = e.currentTarget.checked; void change(() => setOverlayPreferences(enabled), "enabled"); }}/> Always show dictation microphone</label>
  <p className="muted">Replaces the recording box with a draggable microphone. Click to start dictation, then click again to finish.</p>
  {prefs && !prefs.supported && <p className="muted">Available on Windows.</p>}
  <button disabled={!prefs?.supported || busy} onClick={() => { void change(resetOverlayPosition, "position"); }}>Reset position</button>
  {preferenceDraft.feedback}
  {error && <FailureLine text={error.text} detail={error.detail} />}
 </section>;
}
