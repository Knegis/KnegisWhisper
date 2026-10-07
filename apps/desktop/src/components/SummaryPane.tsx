import { forwardRef, useCallback, useEffect, useImperativeHandle, useRef, useState } from "react";
import { NoteEditor } from "./NoteEditor";
import { getNoteSummary, generateNoteSummary, cancelNoteSummary, updateNoteSummary, exportNoteSummary, onNoteSummaryChanged, type SummaryState } from "../lib/ipc";
import { useAutosave, type AutosaveStatus } from "../lib/useAutosave";

export type SummaryPaneHandle = { flush: () => Promise<void> };
/**
 * `onSaveStatus` hears this pane's autosave status as it changes, so the note
 * toolbar's "Saved" can cover the summary too (KNEG-398). Pass a stable
 * function, such as a state setter.
 */
export const SummaryPane = forwardRef<SummaryPaneHandle, { noteId: number; visible: boolean; onSource: (row: number) => void; onSaveStatus?: (status: AutosaveStatus) => void }>(function SummaryPane({ noteId, visible, onSource, onSaveStatus }, ref) {
  const [state, setState] = useState<SummaryState | null>(null);
  const current = useRef<SummaryState | null>(null);
  const dirty = useRef(false);
  const editVersion = useRef(0);
  const saveFailure = useRef<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [editorKey, setEditorKey] = useState(0);
  const [pending, setPending] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);
  const save = useCallback(async (patch: Partial<{ content: string }>) => {
    if (patch.content === undefined || current.current === null) return;
    const version = editVersion.current;
    try {
      const next = await updateNoteSummary(noteId, current.current.revision, patch.content);
      current.current = next; setState(next); dirty.current = editVersion.current !== version; saveFailure.current = null;
    } catch (cause) { saveFailure.current = String(cause); throw cause; }
  }, [noteId]);
  const autosave = useAutosave<{ content: string }>({ save });
  useEffect(() => { onSaveStatus?.(autosave.status); }, [autosave.status, onSaveStatus]);
  const flush = useCallback(async () => { await autosave.flush(); if (saveFailure.current) throw new Error(saveFailure.current); }, [autosave.flush]);
  useImperativeHandle(ref, () => ({ flush }), [flush]);
  useEffect(() => {
    let active = true;
    const load = async () => {
      try {
        const next = await getNoteSummary(noteId);
        if (!active || dirty.current) return;
        if (current.current?.content !== next.content) setEditorKey(key => key + 1);
        current.current = next; setState(next);
      } catch (cause) { if (active) setError(String(cause)); }
    };
    void load();
    const subscription = onNoteSummaryChanged(id => { if (id === noteId) void load(); });
    // State is durable; polling also recovers events missed while views were hidden.
    const timer = setInterval(() => { void load(); }, 2000);
    return () => { active = false; clearInterval(timer); void subscription.then(stop => stop()).catch(() => undefined); };
  }, [noteId]);
  const run = async (action: () => Promise<void>) => {
    setPending(true); setError(null); setNotice(null);
    try { await flush(); await action(); } catch (cause) { setError(String(cause)); } finally { setPending(false); }
  };
  const generate = () => run(async () => {
    const latest = await getNoteSummary(noteId);
    const replace = latest.edited || latest.legacy;
    if (replace && !window.confirm("Replace your edited summary? The current version will stay until the new summary succeeds.")) return;
    const retry = latest.status === "failed" || latest.status === "interrupted";
    const next = await generateNoteSummary(noteId, latest.revision, replace, retry);
    current.current = next; setState(next);
  });
  const busy = state?.status === "queued" || state?.status === "processing";
  return <section className="summary-pane" hidden={!visible} aria-label="AI summary" onClickCapture={event => {
    const link = (event.target as HTMLElement).closest<HTMLAnchorElement>('a[href^="#summary-source-"]');
    if (!link) return;
    event.preventDefault(); event.stopPropagation();
    if (state?.outdated) return;
    const source = state?.references.find(item => `#summary-source-${item.id}` === link.getAttribute("href"));
    if (source) onSource(source.rowIndex);
  }}>
    <div className="summary-toolbar controls">
      <button type="button" disabled={pending || busy || !state} onClick={() => { void generate(); }}>{state?.status === "failed" || state?.status === "interrupted" ? "Retry summary" : state?.content ? "Regenerate summary" : "Generate summary"}</button>
      {busy && <button type="button" disabled={pending} onClick={() => { void run(async () => { await cancelNoteSummary(noteId); const next = await getNoteSummary(noteId); current.current = next; setState(next); }); }}>Cancel</button>}
      {state?.content !== null && state && <>
        <button type="button" disabled={pending} onClick={() => { void run(async () => { const latest = await getNoteSummary(noteId); await navigator.clipboard.writeText(latest.content ?? ""); setNotice("Summary copied"); }); }}>Copy</button>
        <button type="button" disabled={pending} onClick={() => { void run(async () => { const path = await exportNoteSummary(noteId, "md"); if (path) setNotice(`Exported to ${path}`); }); }}>Export Markdown</button>
        <button type="button" disabled={pending} onClick={() => { void run(async () => { const path = await exportNoteSummary(noteId, "txt"); if (path) setNotice(`Exported to ${path}`); }); }}>Export text</button>
      </>}
    </div>
    <p role="status" className="muted">{!state ? "Loading summary…" : busy ? state.status === "queued" ? "Summary queued" : `Generating summary · ${state.completed} steps completed` : state.edited ? "Edited summary · source links are navigation aids" : state.legacy ? "Imported summary · source references unavailable" : state.content ? `AI summary · ${state.language ?? "language unknown"}` : "Summaries use your saved transcript. Configure the model in Settings > Cleanup & summaries."}</p>
    {state?.outdated && <p className="warn">The transcript changed. Regenerate this summary to refresh its source links.</p>}
    {(error || state?.error) && <p className="error" role="alert">{error ?? state?.error}</p>}
    {notice && <p role="status">{notice}</p>}
    {autosave.status === "failed" && <p className="error" role="alert">Couldn’t save summary: {autosave.error} <button type="button" onClick={() => { void autosave.retry().catch(() => undefined); }}>Retry save</button></p>}
    {state?.content != null && <NoteEditor key={editorKey} noteId={noteId} summary initialContent={state.content} onChange={content => {
      if (content === current.current?.content) return;
      editVersion.current += 1; dirty.current = true; autosave.schedule({ content });
    }} />}
  </section>;
});
