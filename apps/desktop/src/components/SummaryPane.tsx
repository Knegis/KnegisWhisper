import { forwardRef, useCallback, useEffect, useImperativeHandle, useRef, useState } from "react";
import { NoteEditor } from "./NoteEditor";
import { askConfirm } from "./ConfirmDialog";
import { getNoteSummary, getSummaryReadiness, generateNoteSummary, cancelNoteSummary, updateNoteSummary, exportNoteSummary, onNoteSummaryChanged, type SummaryState, type SummaryReadiness, type SummarySetupTarget } from "../lib/ipc";
import { summaryProgressText } from "../lib/summaryProgress";
import { summaryModelName } from "../lib/meetingView";
import { useAutosave, type AutosaveStatus } from "../lib/useAutosave";

export type SummaryPaneHandle = { flush: () => Promise<void>; draft: () => string; discardDraft: () => void };
/**
 * `onSaveStatus` hears this pane's autosave status as it changes, so the note
 * toolbar's "Saved" can cover the summary too (KNEG-398). Pass a stable
 * function, such as a state setter.
 */
export const SummaryPane = forwardRef<SummaryPaneHandle, { noteId: number; visible: boolean; hasTranscript: boolean | null; focusOnReady: boolean; onSetup: (target: SummarySetupTarget) => Promise<void>; onSource: (row: number) => void; onSaveStatus?: (status: AutosaveStatus) => void; readOnly?: boolean }>(function SummaryPane({ noteId, visible, hasTranscript, focusOnReady, onSetup, onSource, onSaveStatus, readOnly = false }, ref) {
  const actionRef = useRef<HTMLButtonElement>(null);
  const focusedReturn = useRef(false);
  const [state, setState] = useState<SummaryState | null>(null);
  const current = useRef<SummaryState | null>(null);
  const dirty = useRef(false);
  const summaryDraft = useRef("");
  const editVersion = useRef(0);
  const saveFailure = useRef<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [editorKey, setEditorKey] = useState(0);
  const [pending, setPending] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);
  const [readiness, setReadiness] = useState<SummaryReadiness | null>(null);
  const [readinessError, setReadinessError] = useState<string | null>(null);
  const readinessGeneration = useRef(0);
  useEffect(() => {
    if (focusOnReady && visible && state !== null && (readiness !== null || readinessError !== null) && hasTranscript !== null && !focusedReturn.current) {
      focusedReturn.current = true;
      actionRef.current?.focus();
    }
  }, [focusOnReady, visible, state, readiness, readinessError, hasTranscript]);
  const refreshReadiness = useCallback(async () => {
    const generation = ++readinessGeneration.current;
    try {
      const next = await getSummaryReadiness();
      if (generation === readinessGeneration.current) { setReadiness(next); setReadinessError(null); }
      return next;
    } catch (cause) {
      if (generation === readinessGeneration.current) { setReadiness(null); setReadinessError(String(cause)); }
      throw cause;
    }
  }, []);
  useEffect(() => {
    if (!visible) return;
    void refreshReadiness().catch(() => undefined);
    const timer = setInterval(() => { void refreshReadiness().catch(() => undefined); }, 2000);
    return () => { ++readinessGeneration.current; clearInterval(timer); };
  }, [visible, refreshReadiness]);
  const save = useCallback(async (patch: Partial<{ content: string }>) => {
    if (patch.content === undefined || current.current === null) return;
    const version = editVersion.current;
    try {
      const next = await updateNoteSummary(noteId, current.current.revision, patch.content);
      current.current = next; setState(next); dirty.current = editVersion.current !== version; saveFailure.current = null;
    } catch (cause) { saveFailure.current = String(cause); throw cause; }
  }, [noteId]);
  const autosave = useAutosave<{ content: string }>({ save, paused: readOnly });
  useEffect(() => { onSaveStatus?.(autosave.status); }, [autosave.status, onSaveStatus]);
  const flush = useCallback(async () => { await autosave.flush(); if (saveFailure.current) throw new Error(saveFailure.current); }, [autosave.flush]);
  useImperativeHandle(ref, () => ({ flush, draft: () => summaryDraft.current, discardDraft: () => { autosave.discard(); dirty.current = false; saveFailure.current = null; } }), [flush, autosave.discard]);
  useEffect(() => {
    let active = true;
    const load = async () => {
      try {
        const next = await getNoteSummary(noteId);
        if (!active || dirty.current) return;
        if (current.current?.content !== next.content) setEditorKey(key => key + 1);
        current.current = next; summaryDraft.current = next.content ?? ""; setState(next);
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
    const ready = await refreshReadiness();
    if (ready.state === "needs-setup") { await onSetup(ready.target); return; }
    const latest = await getNoteSummary(noteId);
    const replace = latest.edited || latest.legacy;
    if (replace && !(await askConfirm("Replace your edited summary? The current version will stay until the new summary succeeds.", { confirm: "Replace summary" }))) return;
    const retry = latest.status === "failed" || latest.status === "interrupted";
    const next = await generateNoteSummary(noteId, latest.revision, replace, retry);
    if (current.current?.content !== next.content) setEditorKey(key => key + 1);
    current.current = next; summaryDraft.current = next.content ?? ""; setState(next);
  });
  const busy = state?.status === "queued" || state?.status === "processing";
  const needsSetup = !busy && readiness?.state === "needs-setup";
  return <section className="summary-pane" hidden={!visible} aria-label="AI summary" onClickCapture={event => {
    const link = (event.target as HTMLElement).closest<HTMLAnchorElement>('a[href^="#summary-source-"]');
    if (!link) return;
    event.preventDefault(); event.stopPropagation();
    if (state?.outdated) return;
    const source = state?.references.find(item => `#summary-source-${item.id}` === link.getAttribute("href"));
    if (source) onSource(source.rowIndex);
  }}>
    <div className="summary-toolbar controls">
      <button ref={actionRef} type="button" data-summary-action disabled={readOnly || pending || busy || !state || hasTranscript !== true || (readiness === null && readinessError === null)} onClick={() => {
        if (readinessError) void refreshReadiness().catch(() => undefined);
        else void generate();
      }}>{readinessError ? "Retry setup check" : needsSetup ? "Set up summaries" : state?.status === "failed" || state?.status === "interrupted" ? "Retry summary" : state?.content ? "Regenerate summary" : "Generate summary"}</button>
      {busy && <button type="button" disabled={readOnly || pending} onClick={() => { void run(async () => { await cancelNoteSummary(noteId); const next = await getNoteSummary(noteId); current.current = next; setState(next); }); }}>Cancel</button>}
      {state?.content !== null && state && <>
        <button type="button" disabled={pending} onClick={() => { void run(async () => { const latest = await getNoteSummary(noteId); await navigator.clipboard.writeText(latest.content ?? ""); setNotice("Summary copied"); }); }}>Copy</button>
        <button type="button" disabled={pending} onClick={() => { void run(async () => { const path = await exportNoteSummary(noteId, "md"); if (path) setNotice(`Exported to ${path}`); }); }}>Export Markdown</button>
        <button type="button" disabled={pending} onClick={() => { void run(async () => { const path = await exportNoteSummary(noteId, "txt"); if (path) setNotice(`Exported to ${path}`); }); }}>Export text</button>
      </>}
    </div>
    <p role="status" className="muted">{!state ? "Loading summary…" : summaryProgressText(state) ?? "Summaries use your saved transcript."}</p>
    {!busy && state?.content && <p className="muted">{state.edited ? "Edited summary · source links are navigation aids" : state.legacy ? "Imported summary · source references unavailable" : `AI summary · ${state.language ?? "language unknown"}${state.model ? ` · by ${summaryModelName(state.model)}` : ""}`}</p>}
    {hasTranscript === null ? <p className="muted">Loading saved transcript…</p> : !hasTranscript ? <p className="muted">This note needs a saved transcript with speech before a summary can be generated.</p> : needsSetup && readiness?.state === "needs-setup" ? <p className="muted">{readiness.reason}</p> : null}
    {readinessError && <p className="error" role="alert">Couldn’t check summary setup: {readinessError}</p>}
    {/* The people are part of what a summary was made from, and since KNEG-362 they can change after it. */}
    {state?.outdated && <p className="warn">The transcript or the people changed since this summary was made. Regenerate it to update it and its source links.</p>}
    {(error || state?.error) && <p className="error" role="alert">{error ?? state?.error}</p>}
    {notice && <p role="status">{notice}</p>}
    {autosave.status === "failed" && <p className="error" role="alert">Couldn’t save summary: {autosave.error} <button type="button" disabled={readOnly} onClick={() => { void autosave.retry().catch(() => undefined); }}>Retry save</button></p>}
    {state?.content != null && <NoteEditor readOnly={readOnly} key={editorKey} noteId={noteId} summary initialContent={state.content} onChange={content => {
      if (content === current.current?.content) return;
      summaryDraft.current = content; editVersion.current += 1; dirty.current = true; autosave.schedule({ content });
    }} />}
  </section>;
});
