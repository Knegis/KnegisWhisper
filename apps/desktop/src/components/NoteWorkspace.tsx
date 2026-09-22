import { forwardRef, useCallback, useEffect, useImperativeHandle, useRef, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { attachNoteRecording, getNoteRecording, getNoteTranscriptRows, updateNote, type Note, type NoteUpdate } from "../lib/ipc";
import { useAutosave } from "../lib/useAutosave";
import { transcriptToMarkdown } from "../lib/noteBody";
import { message } from "../lib/useRefresh";
import { NoteEditor } from "./NoteEditor";
import { SummaryPane, type SummaryPaneHandle } from "./SummaryPane";
import { RecordingPlayer, type RecordingPlayerHandle } from "./RecordingPlayer";
import { TranscriptPanel } from "./TranscriptPanel";

export type NoteWorkspaceHandle = { flush: () => Promise<void> };
const TABS = ["notes", "transcript", "replay"] as const;
type Tab = typeof TABS[number];
type Patch = { title: string; content: string };

export const NoteWorkspace = forwardRef<NoteWorkspaceHandle, { note: Note; reload: () => Promise<void> }>(function NoteWorkspace({ note, reload }, ref) {
  const saved = useRef({ title: note.title, content: note.content });
  const draft = useRef({ title: note.title, content: note.content });
  const [title, setTitle] = useState(note.title);
  const [empty, setEmpty] = useState(false);
  const [ready, setReady] = useState(false);
  const [seed, setSeed] = useState({ content: note.content, generation: 0 });
  const summaryRef = useRef<SummaryPaneHandle>(null);
  const playerRef = useRef<RecordingPlayerHandle>(null);
  const [tab, setTab] = useState<Tab>("notes");
  const [sourceRequest, setSourceRequest] = useState<{ index: number; nonce: number } | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [currentTime, setCurrentTime] = useState(0);
  const [playing, setPlaying] = useState(false);
  const [playbackStarted, setPlaybackStarted] = useState(false);
  const [transcript, setTranscript] = useState<{
    rows: Awaited<ReturnType<typeof getNoteTranscriptRows>>;
    recording: Awaited<ReturnType<typeof getNoteRecording>>;
  } | null>(null);
  const [transcriptError, setTranscriptError] = useState<string | null>(null);
  const [attaching, setAttaching] = useState(false);
  const [attachmentError, setAttachmentError] = useState<string | null>(null);
  const alive = useRef(true);
  const attachmentPending = useRef(false);
  useEffect(() => { alive.current = true; return () => { alive.current = false; }; }, []);

  const save = useCallback(async (patch: Partial<Patch>) => {
    const request: NoteUpdate = {};
    if (patch.content !== undefined && patch.content !== saved.current.content) request.content = patch.content;
    if (patch.title !== undefined && patch.title !== saved.current.title) request.title = patch.title;
    if (!Object.keys(request).length) return;
    if (!await updateNote(note.id, request)) throw new Error("This note no longer exists.");
    if (typeof request.content === "string") saved.current.content = request.content;
    if (typeof request.title === "string") saved.current.title = request.title;
    await reload();
  }, [note.id, reload]);
  const autosave = useAutosave<Patch>({ save });
  const normalizeTitle = useCallback(() => {
    const normalized = draft.current.title.trim() || "Untitled note";
    if (normalized !== draft.current.title) {
      draft.current.title = normalized; setTitle(normalized);
      autosave.schedule({ title: normalized, content: draft.current.content });
    }
  }, [autosave.schedule]);
  const flush = useCallback(async () => {
    normalizeTitle(); await autosave.flush(); await summaryRef.current?.flush();
  }, [normalizeTitle, autosave.flush]);
  useImperativeHandle(ref, () => ({ flush }), [flush]);
  useEffect(() => {
    if (!("__TAURI_INTERNALS__" in window)) return;
    let closing = false;
    const registered = getCurrentWindow().onCloseRequested(async event => {
      if (closing) return;
      event.preventDefault();
      try { await flush(); closing = true; await getCurrentWindow().close(); }
      catch (cause) { setError(message(cause)); }
    });
    return () => { void registered.then(stop => stop()); };
  }, [flush]);
  const loadTranscript = useCallback(async () => {
    setTranscriptError(null);
    try {
      const [rows, recording] = await Promise.all([getNoteTranscriptRows(note.id), getNoteRecording(note.id)]);
      if (alive.current) setTranscript({ rows, recording });
    } catch (cause) { if (alive.current) setTranscriptError(message(cause)); }
  }, [note.id]);
  useEffect(() => { void loadTranscript(); }, [loadTranscript]);
  const attachRecording = async () => {
    if (attachmentPending.current) return;
    attachmentPending.current = true; setAttaching(true); setAttachmentError(null);
    try {
      const recording = await attachNoteRecording(note.id);
      if (alive.current && recording) setTranscript(previous => previous ? { ...previous, recording } : previous);
    } catch (cause) { if (alive.current) setAttachmentError(message(cause)); }
    finally { if (alive.current) { attachmentPending.current = false; setAttaching(false); } }
  };
  const switchTab = async (next: Tab, focus = false) => {
    try {
      await flush();
      if (!alive.current) return;
      setError(null); setTab(next);
      if (focus) document.getElementById(`note-${note.id}-tab-${next}`)?.focus();
    } catch (cause) { setError(message(cause)); }
  };
  const playInterval = useCallback((start: number, end: number | null) => {
    setPlaybackStarted(true); playerRef.current?.playInterval(start, end);
  }, []);
  const onPlayingChange = useCallback((value: boolean) => { setPlaying(value); if (value) setPlaybackStarted(true); }, []);
  const showSource = async (index: number) => {
    try {
      await flush();
      if (!alive.current) return;
      setError(null); setTab("transcript"); setSourceRequest({ index, nonce: Date.now() });
      const row = transcript?.rows.find(item => item.index === index);
      if (row?.startSeconds != null && transcript?.recording) playInterval(row.startSeconds, row.endSeconds);
    } catch (cause) { setError(message(cause)); }
  };
  const startFromTranscript = () => {
    if (!ready || draft.current.content.trim() || !transcript?.rows.length) return;
    const content = transcriptToMarkdown(transcript.rows);
    if (!content.trim()) return;
    draft.current.content = content; setEmpty(false); setReady(false);
    setSeed(previous => ({ content, generation: previous.generation + 1 }));
    autosave.schedule({ content });
  };
  return <article id={`note-detail-${note.id}`} className="note-detail" aria-label={title}>
    <header className="note-detail-header">
      <input className="note-title" aria-label="Title" value={title} disabled={!ready} onChange={event => {
        const value = event.target.value; draft.current.title = value; setTitle(value);
        autosave.schedule({ title: value.trim() || "Untitled note", content: draft.current.content });
      }} onBlur={normalizeTitle} />
      {autosave.status === "failed" && <p className="save-status error" role="alert">Couldn't save: {autosave.error} <button type="button" onClick={() => { void autosave.retry().catch(() => undefined); }}>Retry save</button></p>}
    </header>
    <div className="summary-tabs note-tabs" role="tablist" aria-label="Note view">
      {TABS.map((value, index) => <button key={value} id={`note-${note.id}-tab-${value}`} type="button" role="tab"
        aria-controls={`note-${note.id}-panel-${value}`} aria-selected={tab === value} tabIndex={tab === value ? 0 : -1}
        onClick={() => { void switchTab(value); }} onKeyDown={event => {
          if (!["ArrowLeft", "ArrowRight", "Home", "End"].includes(event.key)) return;
          event.preventDefault();
          const next = event.key === "Home" ? 0 : event.key === "End" ? 2 : (index + (event.key === "ArrowRight" ? 1 : 2)) % 3;
          void switchTab(TABS[next]!, true);
        }}>{value === "notes" ? "Notes" : value === "transcript" ? "Transcript" : "Replay"}</button>)}
    </div>
    {error && <p className="error" role="alert">{error}</p>}
    <div className="note-workspace">
      <section id={`note-${note.id}-panel-notes`} role="tabpanel" aria-labelledby={`note-${note.id}-tab-notes`} hidden={tab !== "notes"}>
        <div className="note-prose">
          <h2 className="note-section-heading">Your notes</h2>
          {ready && empty && transcript?.rows.some(row => row.text.trim()) && <button type="button" className="start-from-transcript" onClick={startFromTranscript}>Start from transcript</button>}
          <NoteEditor key={seed.generation} noteId={note.id} initialContent={seed.content}
            {...(seed.generation === 0 ? { legacyTitle: note.title } : {})}
            onReady={content => { draft.current.content = content; setEmpty(!content.trim()); setReady(true); }}
            onChange={content => { draft.current.content = content; setEmpty(!content.trim()); autosave.schedule({ content }); }} />
        </div>
        <h2 className="note-section-heading">AI summary</h2>
        <SummaryPane ref={summaryRef} noteId={note.id} visible={tab === "notes"} onSource={index => { void showSource(index); }} />
      </section>
      {transcript?.recording && <div className={tab === "replay" ? "note-full-player" : "note-mini-player"} hidden={tab !== "replay" && !playbackStarted}>
        <RecordingPlayer ref={playerRef} noteId={note.id} onCurrentTime={setCurrentTime} onPlayingChange={onPlayingChange} compact={tab !== "replay"} />
      </div>}
      {(["transcript", "replay"] as const).map(value => <section key={value} id={`note-${note.id}-panel-${value}`} role="tabpanel"
        aria-labelledby={`note-${note.id}-tab-${value}`} hidden={tab !== value}>
        {note.recordingDetails && <details className="recording-details" open><summary>Recording details</summary><p>{note.recordingDetails}</p></details>}
        {transcriptError ? <p className="error" role="alert">{transcriptError} <button type="button" onClick={() => { void loadTranscript(); }}>Retry</button></p> : !transcript ? <p role="status">Loading transcript and recording…</p> : <>
          {!transcript.recording && value === "replay" && <div className="recording-empty">
            <p>This note has no recording attached. Select its original recording to replay it. A copy is saved; your original stays in place.</p>
            <button type="button" disabled={attaching} onClick={() => { void attachRecording(); }}>{attaching ? "Attaching…" : "Attach recording…"}</button>
            <p className="muted">16-bit PCM WAV, mono or stereo.</p>
            {attachmentError && <p className="error" role="alert">{attachmentError}</p>}
          </div>}
          {transcript.rows.length ? <TranscriptPanel sourceRequest={value === "transcript" ? sourceRequest : null} rows={transcript.rows}
            recording={transcript.recording} currentTime={currentTime} playing={playing} visible={tab === value} playInterval={playInterval} />
            : <p className="muted">This note has no transcript.</p>}
        </>}
      </section>)}
    </div>
  </article>;
});
