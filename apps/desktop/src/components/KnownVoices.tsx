import { useCallback, useState } from "react";
import { deleteKnownVoice, getKnownVoicesStatus, renameKnownVoice, repairKnownVoices, setKnownVoicesEnabled } from "../lib/ipc";
import { useRefresh } from "../lib/useRefresh";

export function KnownVoices({ refreshKey }: { refreshKey: string }) {
  const load = useCallback(() => getKnownVoicesStatus(), [refreshKey]);
  const { data, error, guard } = useRefresh(load);
  const [busy, setBusy] = useState(false);
  const run = async (action: () => Promise<unknown>) => {
    if (busy) return;
    setBusy(true);
    try { await guard(action); }
    finally { setBusy(false); }
  };
  // Not drawn until it can be used (KNEG-571). In this version nothing can turn
  // it on: the switch was shown disabled beside "awaiting validation on labeled
  // recordings", which is a status report, not a setting. It is drawn as soon
  // as it is ready, wherever it is already on, wherever a voice is remembered
  // (so one can still be forgotten), and when its runtime is what is missing.
  if (!error && (!data || (!data.ready && !data.enabled && data.voices.length === 0 && !data.reason?.startsWith("Install")))) return null;
  return <section>
    <h2>Known voices</h2>
    <p>Recognize remembered participants after a meeting. Only voices you explicitly remember are saved on this computer. Keep the meeting recording, then choose Remember this voice when naming a speaker.</p>
    {error ? <p className="error" role="alert">{error}</p> : null}
    {!data ? <p>Loading known voices…</p> : <>
      <label className="switch-row"><span>Recognize returning participants</span><input type="checkbox" role="switch" checked={data.enabled} disabled={busy || (!data.ready && !data.enabled)} onChange={e => { void run(() => setKnownVoicesEnabled(e.target.checked)); }} /></label>
      {data.reason ? <p>{data.reason}</p> : null}
      {data.ready || data.reason?.startsWith("Install") ? <button disabled={busy} onClick={() => { void run(repairKnownVoices); }}>{data.ready ? "Repair Known voices runtime" : "Install Known voices runtime"}</button> : null}
      {/* One row per voice: the name is the field, so its label is for screen
          readers only; repeating "Voice name" before every field said nothing
          the field doesn't. The sample note sits under the row, not inside it. */}
      {data.voices.map(voice => <form key={`${voice.id}:${voice.displayName}`} className="known-voice" onSubmit={e => {
        e.preventDefault(); const name = String(new FormData(e.currentTarget).get("name") ?? "");
        void run(() => renameKnownVoice(voice.id, name));
      }}>
        <label className="known-voice-name"><span className="sr-only">Voice name</span><input name="name" defaultValue={voice.displayName} required maxLength={100} disabled={busy} /></label>
        <button disabled={busy} type="submit">Rename</button>
        <button disabled={busy} type="button" onClick={() => { void run(() => deleteKnownVoice(voice.id)); }}>Forget voice</button>
        {!voice.compatible ? <p className="known-voice-note">Needs a new voice sample</p> : null}
      </form>)}
      <p>Turning this off keeps remembered voices. Forgetting a voice removes its recognition data; names already saved in notes remain.</p>
    </>}
  </section>;
}
