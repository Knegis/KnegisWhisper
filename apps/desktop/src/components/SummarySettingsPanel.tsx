import { useEffect, useState } from "react";
import { getSummarySettings, setSummarySettings, getReasoningStatus, getLocalLlmStatus, type SummarySettings, type ReasoningProviderSummary, type LocalLlmModelStatus } from "../lib/ipc";
import { FailureLine } from "./FailureLine";
import { LearnMore } from "./LearnMore";

/**
 * Settings > Cleanup & summaries: meeting summaries.
 *
 * In plain words (KNEG-343): where the transcript goes is said for every
 * choice; "this computer" and "your own server" replace "Local model" and
 * "Self-hosted endpoint"; the local model's token ceiling is under Learn more.
 * It said a self-hosted server "uses the configured custom-provider
 * credential" when no screen could save one. Providers › Your own server
 * saves it now (KNEG-415), and the line says where.
 */
export function SummarySettingsPanel() {
  const [settings, setSettings] = useState<SummarySettings | null>(null);
  const [providers, setProviders] = useState<ReasoningProviderSummary[]>([]);
  const [localModels, setLocalModels] = useState<LocalLlmModelStatus[]>([]);
  const [error, setError] = useState<{ text: string; detail: string } | null>(null);
  const [saved, setSaved] = useState(false);
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    let active = true;
    void Promise.all([getSummarySettings(), getReasoningStatus(), getLocalLlmStatus()]).then(([config, reasoning, local]) => {
      if (active) { setSettings(config); setProviders(reasoning.providers); setLocalModels(local.models); }
    }).catch(cause => { if (active) setError({ text: "Couldn't load the summary settings.", detail: String(cause) }); });
    return () => { active = false; };
  }, []);
  if (!settings) return <section><h2>Meeting summaries</h2>{error ? <FailureLine text={error.text} detail={error.detail} /> : <p>Loading summary settings…</p>}</section>;
  const change = (patch: Partial<SummarySettings>) => { setSettings({ ...settings, ...patch }); setSaved(false); };
  const selected = providers.find(p => p.id === settings.provider);
  const local = settings.provider === "local" && !settings.selfHostedUrl;
  const selfHosted = settings.selfHostedUrl !== null;
  const models = local ? localModels : selected?.models ?? [];
  const save = async () => {
    setBusy(true); setError(null);
    try { await setSummarySettings(settings); setSaved(true); } catch (cause) { setError({ text: "Couldn't save the summary settings.", detail: String(cause) }); }
    finally { setBusy(false); }
  };
  const goesTo = local
    ? "Transcripts stay on this computer."
    : selfHosted
      ? "Transcripts are sent to your server."
      : `Transcripts are sent to ${selected?.name ?? "the provider you choose"}, which may charge for it.`;
  return <section data-surface="settings.intelligence.summaries">
    <h2>Meeting summaries</h2>
    <p className="muted">Summarise saved meetings and imported recordings: decisions, action items and links back to the transcript. Your own notes stay separate.</p>
    <div className="row"><span className="row-label">Automatic summaries</span><label className="row-value controls"><input type="checkbox" checked={settings.enabled} onChange={e => change({ enabled: e.target.checked })} />After new recordings and imports</label></div>
    <div className="row"><label className="row-label" htmlFor="summary-provider">Provider</label><span className="row-value controls"><select id="summary-provider" value={selfHosted ? "self-hosted" : settings.provider} onChange={e => {
      const provider = e.target.value;
      change({ provider, model: "", selfHostedUrl: provider === "self-hosted" ? "" : null, enterprise: providers.find(p => p.id === provider)?.enterprise ?? false });
    }}><option value="">Choose a provider</option><option value="local">This computer</option><option value="self-hosted">Your own server</option>{providers.filter(p => p.id !== "local").map(p => <option key={p.id} value={p.id}>{p.name}</option>)}</select></span></div>
    {selfHosted && <div className="row"><label className="row-label" htmlFor="summary-endpoint">Server address</label><span className="row-value"><input id="summary-endpoint" type="url" value={settings.selfHostedUrl ?? ""} placeholder="http://localhost:8080/v1" onChange={e => change({ selfHostedUrl: e.target.value })} /><p className="muted">Requests go without a key unless you save one under Providers › Your own server.</p></span></div>}
    <div className="row"><label className="row-label" htmlFor="summary-model">Model</label><span className="row-value"><input id="summary-model" list="summary-models" value={settings.model} placeholder={local ? "Choose an installed model" : "Model name, or deployment for Azure"} onChange={e => change({ model: e.target.value })} /><datalist id="summary-models">{models.map(m => <option key={m.id} value={m.id}>{m.name}</option>)}</datalist>{local && <p className="muted">Install a model under Local text models first. A summary never downloads one by itself.</p>}</span></div>
    <div className="row"><label className="row-label" htmlFor="summary-language">Summary language</label><span className="row-value"><select id="summary-language" value={settings.language} onChange={e => change({ language: e.target.value })}><option value="match">Match meeting</option><option value="sv">Swedish</option><option value="en">English</option></select></span></div>
    <div className="row"><label className="row-label" htmlFor="summary-context">How much the model reads at once</label><span className="row-value"><input id="summary-context" type="number" min={4096} max={262144} step={1024} value={settings.contextBudget} onChange={e => change({ contextBudget: Number(e.target.value) })} /><p className="muted">Set this to what your model supports, in tokens. Longer transcripts are summarised in parts.</p><LearnMore><p>A model on this computer reads at most 8,192 tokens at once, whatever this says.</p></LearnMore></span></div>
    <p className="muted">{goesTo} Dictation cleanup is set separately, below.</p>
    <div className="controls"><button type="button" onClick={() => { void save(); }} disabled={busy}>{busy ? "Saving…" : "Save summary settings"}</button>{saved && <span role="status">Saved</span>}</div>
    {error && <FailureLine text={error.text} detail={error.detail} />}
  </section>;
}
