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
  return <section>
    <h2>Known voices</h2>
    <p>Recognize remembered participants after a meeting. Voice samples stay on this computer. Choose “Remember this voice” when naming a speaker.</p>
    {error ? <p className="error" role="alert">{error}</p> : null}
    {!data ? <p>Loading known voices…</p> : <>
      <label className="switch-row"><span>Recognize returning participants</span><input type="checkbox" role="switch" checked={data.enabled} disabled={busy || (!data.ready && !data.enabled)} onChange={e => { void run(() => setKnownVoicesEnabled(e.target.checked)); }} /></label>
      {data.reason ? <p>{data.reason}</p> : null}
      {data.reason?.startsWith("Repair") ? <button disabled={busy} onClick={() => { void run(repairKnownVoices); }}>Repair speaker runtime</button> : null}
      {data.voices.map(voice => <form key={`${voice.id}:${voice.displayName}`} onSubmit={e => {
        e.preventDefault(); const name = String(new FormData(e.currentTarget).get("name") ?? "");
        void run(() => renameKnownVoice(voice.id, name));
      }}>
        <label>Voice name <input name="name" defaultValue={voice.displayName} required maxLength={100} disabled={busy} /></label>
        {!voice.compatible ? <span>Needs a new voice sample</span> : null}
        <button disabled={busy} type="submit">Rename</button>
        <button disabled={busy} type="button" onClick={() => { void run(() => deleteKnownVoice(voice.id)); }}>Forget voice</button>
      </form>)}
      <p>Turning this off keeps remembered voices. Forgetting a voice removes its recognition data; names already saved in notes remain.</p>
    </>}
  </section>;
}
