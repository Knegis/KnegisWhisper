import { useSettingsDraft } from "./SettingsDrafts";
import { useEffect, useRef, useState } from "react";
import { getSummarySettings, getSummaryReadiness, setSummarySettings, getReasoningStatus, getLocalLlmStatus, type SummarySettings, type SummaryReadiness, type ReasoningProviderSummary, type LocalLlmModelStatus } from "../lib/ipc";
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
export function SummarySettingsPanel({ focusRequest, onGoToProviders, onGoToLocalModels }: {
  focusRequest?: number | undefined; onGoToProviders: () => void; onGoToLocalModels: () => void;
}) {
  const sectionRef = useRef<HTMLElement>(null);
  const [readiness, setReadiness] = useState<SummaryReadiness | null>(null);
  const [readinessError, setReadinessError] = useState<string | null>(null);
  const committed = useRef<SummarySettings | null>(null);
  const currentSettings = useRef<SummarySettings | null>(null);
  const [settings, setSettings] = useState<SummarySettings | null>(null);
  const [providers, setProviders] = useState<ReasoningProviderSummary[]>([]);
  const [localModels, setLocalModels] = useState<LocalLlmModelStatus[]>([]);
  const [error, setError] = useState<{ text: string; detail: string } | null>(null);
  const [, setSaved] = useState(false);
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    let active = true;
    void Promise.all([getSummarySettings(), getReasoningStatus(), getLocalLlmStatus()]).then(([config, reasoning, local]) => {
      if (active) { committed.current = config; setSettings(config); setProviders(reasoning.providers); setLocalModels(local.models); }
    }).catch(cause => { if (active) setError({ text: "Couldn't load the summary settings.", detail: String(cause) }); });
    void getSummaryReadiness().then(ready => { if (active) setReadiness(ready); })
      .catch(cause => { if (active) setReadinessError(String(cause)); });
    return () => { active = false; };
  }, []);
  const loaded = settings !== null;
  useEffect(() => {
    if (focusRequest === undefined || !loaded) return;
    sectionRef.current?.scrollIntoView({ block: "start" });
    sectionRef.current?.querySelector<HTMLSelectElement>("#summary-provider")?.focus({ preventScroll: true });
  }, [focusRequest, loaded]);
  currentSettings.current = settings;
  const persist = async () => {
    const snapshot = currentSettings.current;
    if (!snapshot) return;
    await setSummarySettings(snapshot);
    committed.current = snapshot;
    setSaved(true);
  };
  const draftGuard = useSettingsDraft({ label: "Meeting summaries", dirty: settings !== null && JSON.stringify(settings) !== JSON.stringify(committed.current),
    save: persist, discard: () => { setSettings(committed.current); setSaved(false); setReadiness(null); } });
  if (!settings) return <section><h2>Meeting summaries</h2>{error ? <FailureLine text={error.text} detail={error.detail} /> : <p>Loading summary settings…</p>}</section>;
  const change = (patch: Partial<SummarySettings>) => { setSettings({ ...settings, ...patch }); setSaved(false); setReadiness(null); };
  const selected = providers.find(p => p.id === settings.provider);
  const local = settings.provider === "local" && !settings.selfHostedUrl;
  const selfHosted = settings.selfHostedUrl !== null;
  const models = local ? localModels : selected?.models ?? [];
  const save = async () => {
    setBusy(true); setError(null);
    try { await draftGuard.run(persist); } catch (cause) { setError({ text: "Couldn't save the summary settings.", detail: String(cause) }); setBusy(false); return; }
    try { setReadiness(await getSummaryReadiness()); setReadinessError(null); } catch (cause) { setReadinessError(String(cause)); }
    finally { setBusy(false); }
  };
  const goesTo = local
    ? "Transcripts stay on this computer."
    : selfHosted
      ? "Transcripts are sent to your server."
      : `Transcripts are sent to ${selected?.name ?? "the provider you choose"}, which may charge for it.`;
  return <section ref={sectionRef} data-surface="settings.intelligence.summaries">
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
    {readiness?.state === "needs-setup" && <div className="summary-setup-help"><p className="muted">{readiness.reason}</p>
      {readiness.target === "providers" && <button type="button" onClick={onGoToProviders}>Open Providers</button>}
      {readiness.target === "local-text-models" && <button type="button" onClick={onGoToLocalModels}>Open Local text models</button>}
      {readiness.target === "company-accounts" && <button type="button" onClick={() => {
        const section = document.querySelector<HTMLElement>('[data-surface="settings.intelligence.enterprise"]');
        section?.scrollIntoView({ block: "start" });
        const heading = section?.querySelector<HTMLHeadingElement>("h2");
        if (heading) { heading.tabIndex = -1; heading.focus({ preventScroll: true }); }
      }}>Open Company accounts</button>}
    </div>}
    <div className="controls"><button type="button" onClick={() => { void save(); }} disabled={busy}>{busy ? "Saving…" : "Save summary settings"}</button>{draftGuard.feedback}</div>
    {readinessError && <div><FailureLine text="Couldn't check summary setup." detail={readinessError} /><button type="button" onClick={() => {
      void getSummaryReadiness().then(ready => { setReadiness(ready); setReadinessError(null); }).catch(cause => setReadinessError(String(cause)));
    }}>Retry setup check</button></div>}
    {error && <FailureLine text={error.text} detail={error.detail} />}
  </section>;
}
