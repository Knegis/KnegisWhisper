import { useCallback, useEffect, useRef, useState } from "react";
import {
  folderSharePreview, folderShareExport, folderSharePickImport, folderShareStatus,
  folderShareImport, folderShareCancel, folderShareDismiss, folderShareShowExport,
  type Folder, type ShareOptions, type SharePreview, type ShareStatus,
} from "../lib/ipc";
import { sharingActive, sharingBytes, sharingStage } from "../lib/folderSharing";
import "./FolderSharing.css";

type Props = { folder: Folder | null; trigger: HTMLElement | null; importRequest: number; close: () => void; flush: () => Promise<void>; imported: (folderId: number) => Promise<void> };
const contentChoices: [keyof ShareOptions, string][] = [["personalNotes", "Personal notes"], ["summaries", "Summaries"], ["transcripts", "Transcripts"], ["recordings", "Recordings"]];

export function FolderSharing({ folder, trigger, importRequest, close, flush, imported }: Props) {
  const [preview, setPreview] = useState<SharePreview | null>(null);
  const [selected, setSelected] = useState<Set<number>>(new Set());
  const [options, setOptions] = useState<ShareOptions>({ personalNotes: true, summaries: true, transcripts: true, recordings: true });
  const [status, setStatus] = useState<ShareStatus | null>(null);
  const [name, setName] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [pending, setPending] = useState(false);
  const [minimized, setMinimized] = useState(false);
  const finished = useRef<string | null>(null);
  const importStarted = useRef(0);
  const dialog = useRef<HTMLDialogElement>(null);
  const opener = useRef<HTMLElement | null>(null);
  const callbacks = useRef({ imported, close, flush });
  callbacks.current = { imported, close, flush };
  const fail = (cause: unknown) => setError(cause instanceof Error ? cause.message : String(cause));
  const run = async (action: () => Promise<unknown>) => {
    setError(null); setPending(true);
    try { await action(); } catch (cause) { fail(cause); } finally { setPending(false); }
  };
  useEffect(() => {
    if (!folder) { setPreview(null); return; }
    let live = true;
    setError(null); setPreview(null); setMinimized(false);
    void folderSharePreview(folder.id).then(value => {
      if (!live) return;
      setPreview(value); setSelected(new Set(value.notes.map(note => note.id))); setOptions(value.options);
    }).catch(cause => { if (live) fail(cause); });
    return () => { live = false; };
  }, [folder]);
  useEffect(() => {
    if (!importRequest || importStarted.current === importRequest) return;
    importStarted.current = importRequest;
    setMinimized(false);
    void run(async () => { await folderSharePickImport(); setStatus(await folderShareStatus()); });
  }, [importRequest]);
  const poll = useCallback(async () => {
    try {
      const value = await folderShareStatus();
      setStatus(value);
      if (value?.stage === "succeeded" && value.folderId !== null && finished.current !== value.operationId) {
        finished.current = value.operationId;
        await callbacks.current.imported(value.folderId);
      }
    } catch (cause) { fail(cause); }
  }, []);
  useEffect(() => {
    let live = true;
    let timer: ReturnType<typeof setTimeout>;
    const tick = async () => { if (!live) return; await poll(); if (live) timer = setTimeout(() => { void tick(); }, 500); };
    void tick();
    return () => { live = false; clearTimeout(timer); };
  }, [poll]);
  useEffect(() => { if (status?.stage === "awaiting-confirmation") setName(status.preview?.folderName ?? "Shared folder"); }, [status?.operationId, status?.stage]);
  const visible = (folder !== null || status !== null || pending || error !== null) && !(minimized && status && sharingActive(status.stage) && status.stage !== "awaiting-confirmation");
  useEffect(() => {
    const node = dialog.current;
    if (!node) return;
    if (visible && !node.open) {
      opener.current = trigger ?? (document.activeElement instanceof HTMLElement ? document.activeElement : null);
      node.showModal();
    } else if (!visible && node.open) { node.close(); }
  }, [visible]);
  const dismiss = async () => {
    if (status && sharingActive(status.stage)) { await folderShareCancel(status.operationId); await poll(); return; }
    if (status) await folderShareDismiss(status.operationId);
    setStatus(null); setPreview(null); setError(null); callbacks.current.close();
  };
  const exportReady = folder !== null && preview !== null && !status;
  const importReady = status?.stage === "awaiting-confirmation";
  const done = status !== null && !sharingActive(status.stage);
  const selectedNotes = preview?.notes.filter(note => selected.has(note.id)) ?? [];
  const missing = options.recordings && selectedNotes.some(note => note.recordingMissing);
  return <dialog ref={dialog} className="folder-sharing" aria-labelledby="folder-sharing-title" onClose={() => {
    opener.current?.focus();
    // Resizing can hide the list panel while the dialog is open.
    if (document.activeElement !== opener.current) document.querySelector<HTMLButtonElement>('button[aria-controls="list-panel"]')?.focus();
  }} onCancel={event => { event.preventDefault(); if (!pending && status?.stage !== "saving") void run(dismiss); }}>
    <h2 id="folder-sharing-title">{status?.direction === "import" ? "Import shared folder" : folder || status?.direction === "export" ? "Share folder" : "Folder sharing"}</h2>
    <div className="folder-sharing-body">
    {folder && !status && <>
      <p>Creates a copy you can send. Later changes aren’t shared automatically.</p>
      {!preview && !error && <p role="status">Reading folder…</p>}
      {preview && <>
        <h3>{preview.folderName}</h3>
        <p>{selected.size} of {preview.notes.length} notes selected · approximately {sharingBytes(selectedNotes.reduce((sum, note) => sum + note.textBytes + (options.recordings ? note.recordingBytes : 0), 0))}</p>
        <p className="muted">Estimate includes all note text; excluding text can make the file smaller.</p>
        <fieldset disabled={pending}><legend>Include</legend>{contentChoices.map(([key, label]) => <label key={key}><input type="checkbox" checked={options[key]} onChange={event => setOptions(value => ({ ...value, [key]: event.target.checked }))} />{label}</label>)}</fieldset>
        {!options.transcripts && options.summaries && <p>Summary source links will be removed because transcripts aren’t included.</p>}
        <div className="folder-sharing-selection"><button type="button" disabled={pending} onClick={() => setSelected(new Set(preview.notes.map(note => note.id)))}>Select all</button><button type="button" disabled={pending} onClick={() => setSelected(new Set())}>Select none</button></div>
        <div className="folder-sharing-notes">{preview.notes.map(note => <label key={note.id}><input type="checkbox" disabled={pending} checked={selected.has(note.id)} onChange={event => setSelected(value => { const next = new Set(value); if (event.target.checked) next.add(note.id); else next.delete(note.id); return next; })} /><span>{note.title || "Untitled note"}{note.recordingMissing && <small>Recording unavailable</small>}</span><span>{sharingBytes(note.recordingBytes)}</span></label>)}</div>
        <p>Titles, dates and participant names are included. This file has no password protection.</p>
        {missing && <p role="alert">A selected recording is unavailable. Restore it, exclude its note, or uncheck Recordings.</p>}
      </>}
    </>}
    {importReady && status.preview && <>
      <p>Imports a separate copy. Existing folders and notes won’t change.</p>
      <label className="folder-sharing-name">Folder name<input autoFocus value={name} maxLength={1024} onChange={event => setName(event.target.value)} /></label>
      <p>{status.preview.notes.length} notes · {sharingBytes(status.preview.totalBytes)}</p>
      <p>Includes: {contentChoices.filter(([key]) => status.preview?.options[key]).map(([, label]) => label).join(", ") || "Titles, dates and participants only"}.</p>
      <div className="folder-sharing-notes">{status.preview.notes.map(note => <p key={note.id}>{note.title || "Untitled note"}</p>)}</div>
      <p>If that folder name exists, a number will be added.</p>
    </>}
    {status && !importReady && <div role="status" aria-live="polite"><p>{sharingStage(status.stage)}</p>{sharingActive(status.stage) && status.bytesProcessed > 0 && <p>{sharingBytes(status.bytesProcessed)} processed</p>}{status.stage === "succeeded" && status.direction === "import" && <p>{status.preview ? `${status.preview.notes.length} ${status.preview.notes.length === 1 ? "note" : "notes"} imported.` : "Folder imported."}</p>}{status.savedTo && <p className="folder-sharing-path">Saved to {status.savedTo}</p>}</div>}
    {(error || status?.error) && <p role="alert">{error ?? status?.error}</p>}
    </div>
    <div className="folder-sharing-actions">
      {exportReady && <button type="button" className="primary" disabled={pending || selected.size === 0 || missing} onClick={() => { void run(async () => { await callbacks.current.flush(); await folderShareExport(folder.id, [...selected], options); setStatus(await folderShareStatus()); callbacks.current.close(); }); }}>Save sharing file…</button>}
      {importReady && <button type="button" className="primary" disabled={pending || !name.trim()} onClick={() => { void run(async () => { await folderShareImport(status.operationId, name); await poll(); }); }}>Import folder</button>}
      {status?.savedTo && <button type="button" onClick={() => { void run(() => folderShareShowExport(status.operationId)); }}>Show in folder</button>}
      {status && sharingActive(status.stage) && !importReady && <button type="button" disabled={pending} onClick={() => setMinimized(true)}>Keep working</button>}
      <button type="button" disabled={pending || status?.stage === "saving" || status?.stage === "cancelling"} onClick={() => { void run(dismiss); }}>{done || (!status && !exportReady) ? "Close" : "Cancel"}</button>
    </div>
  </dialog>;
}
