import { useEffect, useState } from "react";
import { getSummarySettings, setSummarySettings, getReasoningStatus, getLocalLlmStatus, type SummarySettings, type ReasoningProviderSummary, type LocalLlmModelStatus } from "../lib/ipc";

export function SummarySettingsPanel() {
  const [settings, setSettings] = useState<SummarySettings | null>(null);
  const [providers, setProviders] = useState<ReasoningProviderSummary[]>([]);
  const [localModels, setLocalModels] = useState<LocalLlmModelStatus[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [saved, setSaved] = useState(false);
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    let active = true;
    void Promise.all([getSummarySettings(), getReasoningStatus(), getLocalLlmStatus()]).then(([config, reasoning, local]) => {
      if (active) { setSettings(config); setProviders(reasoning.providers); setLocalModels(local.models); }
    }).catch(cause => { if (active) setError(String(cause)); });
    return () => { active = false; };
  }, []);
  if (!settings) return <section><h2>Meeting summaries</h2><p>{error ?? "Loading summary settings…"}</p></section>;
  const change = (patch: Partial<SummarySettings>) => { setSettings({ ...settings, ...patch }); setSaved(false); };
  const selected = providers.find(p => p.id === settings.provider);
  const local = settings.provider === "local" && !settings.selfHostedUrl;
  const models = local ? localModels : selected?.models ?? [];
  const save = async () => {
    setBusy(true); setError(null);
    try { await setSummarySettings(settings); setSaved(true); } catch (cause) { setError(String(cause)); }
    finally { setBusy(false); }
  };
  return <section data-surface="settings.intelligence.summaries">
    <h2>Meeting summaries</h2>
    <p className="muted">Summarize saved meetings and audio/video imports with decisions, action items and links to the transcript. Your notes stay separate.</p>
    <div className="row"><span className="row-label">Automatic summaries</span><label className="row-value controls"><input type="checkbox" checked={settings.enabled} onChange={e => change({ enabled: e.target.checked })} />After new recordings and imports</label></div>
    <div className="row"><label className="row-label" htmlFor="summary-provider">Provider</label><span className="row-value controls"><select id="summary-provider" value={settings.selfHostedUrl !== null ? "self-hosted" : settings.provider} onChange={e => {
      const provider = e.target.value;
      change({ provider, model: "", selfHostedUrl: provider === "self-hosted" ? "" : null, enterprise: providers.find(p => p.id === provider)?.enterprise ?? false });
    }}><option value="">Choose a provider</option><option value="local">Local model</option><option value="self-hosted">Self-hosted endpoint</option>{providers.filter(p => p.id !== "local").map(p => <option key={p.id} value={p.id}>{p.name}</option>)}</select></span></div>
    {settings.selfHostedUrl !== null && <div className="row"><label className="row-label" htmlFor="summary-endpoint">Endpoint</label><span className="row-value"><input id="summary-endpoint" type="url" value={settings.selfHostedUrl} placeholder="http://localhost:8080/v1" onChange={e => change({ selfHostedUrl: e.target.value })} /><p className="muted">Uses the configured custom-provider credential, if needed.</p></span></div>}
    <div className="row"><label className="row-label" htmlFor="summary-model">Model</label><span className="row-value"><input id="summary-model" list="summary-models" value={settings.model} placeholder={local ? "Choose an installed model" : "Model or deployment ID"} onChange={e => change({ model: e.target.value })} /><datalist id="summary-models">{models.map(m => <option key={m.id} value={m.id}>{m.name}</option>)}</datalist>{local && <p className="muted">Install models in Local models first. Generation never downloads a model automatically.</p>}</span></div>
    <div className="row"><label className="row-label" htmlFor="summary-language">Summary language</label><span className="row-value"><select id="summary-language" value={settings.language} onChange={e => change({ language: e.target.value })}><option value="match">Match meeting</option><option value="sv">Swedish</option><option value="en">English</option></select></span></div>
    <div className="row"><label className="row-label" htmlFor="summary-context">Model context budget</label><span className="row-value"><input id="summary-context" type="number" min={4096} max={262144} step={1024} value={settings.contextBudget} onChange={e => change({ contextBudget: Number(e.target.value) })} /><p className="muted">Set this to a context size your model supports. Long transcripts are processed in sections. Local inference uses at most 8,192 tokens.</p></span></div>
    <p className="muted">{local ? "Transcript text stays on this device." : settings.selfHostedUrl !== null ? "Transcript text is sent to your chosen endpoint." : "Transcript text is sent directly to the selected provider. Provider charges may apply."} Dictation cleanup has separate settings.</p>
    <div className="controls"><button type="button" onClick={() => { void save(); }} disabled={busy}>{busy ? "Saving…" : "Save summary settings"}</button>{saved && <span role="status">Saved</span>}</div>
    {error && <p role="alert" className="error">{error}</p>}
  </section>;
}
