import { SummaryPane, type SummaryPaneHandle } from "../components/SummaryPane";
import { getCurrentWindow } from "@tauri-apps/api/window";
/**
 * Notes > Export.
 *
 * Two states that look alike and are not: `hits === null` means nobody has
 * searched, and `hits === []` means somebody searched and nothing matched.
 * Collapsing them would show the full note list as though it were the result.
 */

import {
  forwardRef,
  useCallback,
  useEffect,
  useImperativeHandle,
  useLayoutEffect,
  useRef,
  useState,
} from "react";

import { createPortal } from "react-dom";

import { NoteEditor } from "../components/NoteEditor";
import { formatClock } from "../components/RecordingPlayer";
import { Row } from "../components/Row";
import { TranscriptPanel } from "../components/TranscriptPanel";
import {
  createFolder,
  createNote,
  deleteFolder,
  deleteNote,
  exportNoteToFile,
  EXPORT_FORMATS,
  getFolders,
  getNoteRecording,
  attachNoteRecording,
  getNoteTranscriptRows,
  getNotes,
  getNote,
  searchNotes,
  updateNote,
  type ExportFormat,
  type Note,
  type NoteUpdate,
  type SearchHit,
} from "../lib/ipc";
import { useAutosave, type AutosaveStatus } from "../lib/useAutosave";
import { message, useRefresh } from "../lib/useRefresh";

type NoteWorkspaceHandle = {
  flush: () => Promise<void>;
};

type EditorPatch = {
  title?: string;
  content?: string;
};

function SaveStatus({
  status,
  error,
  retry,
}: {
  status: AutosaveStatus;
  error: string | null;
  retry: () => Promise<void>;
}) {
  if (status !== "failed") return null;
  if (status === "failed") {
    return (
      <span className="save-status error" role="status">
        Couldn&apos;t save — <button type="button" onClick={() => { void retry(); }}>Retry</button>
        {error ? <span className="save-error"> {error}</span> : null}
      </span>
    );
  }
  const text = status === "dirty" ? "Unsaved changes" : status === "saving" ? "Saving…" : "Saved";
  return <span className="save-status" role="status">{text}</span>;
}

const NoteWorkspace = forwardRef<
  NoteWorkspaceHandle,
  { note: Note; reload: () => Promise<void> }
>(function NoteWorkspace({ note, reload }, ref) {
  const savedRef = useRef({ title: note.title, content: note.content });
  const summaryRef = useRef<SummaryPaneHandle>(null);
  const [tab, setTab] = useState<"notes" | "summary">(note.enhancedContent !== null ? "summary" : "notes");
  const [sourceRequest, setSourceRequest] = useState<{ index: number; nonce: number } | null>(null);
  const [tabError, setTabError] = useState<string | null>(null);
  const [transcript, setTranscript] = useState<{
    rows: Awaited<ReturnType<typeof getNoteTranscriptRows>>;
    recording: Awaited<ReturnType<typeof getNoteRecording>>;
  } | null>(null);
  const [transcriptError, setTranscriptError] = useState<string | null>(null);
  const [attaching, setAttaching] = useState(false);
  const [attachmentError, setAttachmentError] = useState<string | null>(null);
  const attachmentGeneration = useRef(0);
  const attachmentPending = useRef(false);

  useEffect(() => {
    attachmentGeneration.current += 1;
    attachmentPending.current = false;
    setAttaching(false);
    setAttachmentError(null);
    return () => { attachmentGeneration.current += 1; };
  }, [note.id]);

  const attachRecording = async () => {
    if (attachmentPending.current) return;
    attachmentPending.current = true;
    const generation = attachmentGeneration.current;
    setAttaching(true);
    setAttachmentError(null);
    try {
      const recording = await attachNoteRecording(note.id);
      if (generation !== attachmentGeneration.current || recording === null) return;
      setTranscript((previous) => previous === null ? previous : { ...previous, recording });
    } catch (cause: unknown) {
      if (generation === attachmentGeneration.current) setAttachmentError(message(cause));
    } finally {
      if (generation === attachmentGeneration.current) {
        attachmentPending.current = false;
        setAttaching(false);
      }
    }
  };

  const save = useCallback(
    async (patch: Partial<EditorPatch>) => {
      const request: NoteUpdate = {};
      if (patch.content !== undefined && patch.content !== savedRef.current.content) {
        request.content = patch.content;
      }
      if (
        patch.title !== undefined &&
        patch.title.trim() !== "" &&
        patch.title !== savedRef.current.title
      ) {
        request.title = patch.title;
      }
      if (Object.keys(request).length === 0) return;
      const updated = await updateNote(note.id, request);
      if (!updated) throw new Error("This note no longer exists.");
      if (typeof request.content === "string") savedRef.current.content = request.content;
      if (typeof request.title === "string") savedRef.current.title = request.title;
      await reload();
    },
    [note.id, reload],
  );
  const autosave = useAutosave<EditorPatch>({ save });

  const flushBoth = useCallback(async () => { await autosave.flush(); await summaryRef.current?.flush(); }, [autosave.flush]);
  useImperativeHandle(ref, () => ({ flush: flushBoth }), [flushBoth]);
  useEffect(() => {
    if (!("__TAURI_INTERNALS__" in window)) return;
    let closing = false;
    const registered = getCurrentWindow().onCloseRequested(async event => {
      if (closing) return;
      event.preventDefault();
      try { await flushBoth(); closing = true; await getCurrentWindow().close(); }
      catch (cause) { setTabError(String(cause)); }
    });
    return () => { void registered.then(stop => stop()); };
  }, [flushBoth]);

  useEffect(() => {
    let current = true;
    setTranscript(null);
    setTranscriptError(null);
    void Promise.all([getNoteTranscriptRows(note.id), getNoteRecording(note.id)]).then(
      ([rows, recording]) => {
        if (current) setTranscript({ rows, recording });
      },
      (cause: unknown) => {
        if (current) setTranscriptError(message(cause));
      },
    );
    return () => { current = false; };
  }, [note.id]);

  return (
    <article id={`note-detail-${note.id}`} className="note-detail" aria-label={note.title}>
      <SaveStatus status={autosave.status} error={autosave.error} retry={autosave.retry} />
      <div className="summary-tabs" role="tablist" aria-label="Note view">
        {(["notes", "summary"] as const).map(value => <button key={value} type="button" role="tab" aria-selected={tab === value} tabIndex={tab === value ? 0 : -1} onKeyDown={event => {
          if (!["ArrowLeft", "ArrowRight", "Home", "End"].includes(event.key)) return;
          event.preventDefault();
          const next = event.key === "Home" ? "notes" : event.key === "End" ? "summary" : value === "notes" ? "summary" : "notes";
          const target = event.currentTarget.parentElement?.querySelectorAll<HTMLButtonElement>("button")[next === "notes" ? 0 : 1];
          void flushBoth().then(() => { setTab(next); target?.focus(); }).catch(cause => setTabError(String(cause)));
        }} onClick={() => {
          void flushBoth().then(() => setTab(value)).catch(cause => setTabError(String(cause)));
        }}>{value === "notes" ? "Notes" : "AI summary"}</button>)}
      </div>
      {tabError && <p className="error" role="alert">{tabError}</p>}
      <div id={`note-body-${note.id}`} className="note-workspace">
        <div className="note-prose" hidden={tab !== "notes"}>
        <NoteEditor
          key={note.id}
          noteId={note.id}
          initialContent={note.content}
          initialTitle={note.title}
          onChange={(content, title) => autosave.schedule({ content, title })}
        />
        </div>
        <SummaryPane ref={summaryRef} noteId={note.id} visible={tab === "summary"} onSource={index => setSourceRequest({ index, nonce: Date.now() })} />
        {transcript?.rows.length ? (
          <TranscriptPanel
            sourceRequest={sourceRequest}
            rows={transcript.rows}
            recording={transcript.recording}
            noteId={note.id}
            onAttach={() => { void attachRecording(); }}
            attaching={attaching}
            attachmentError={attachmentError}
          />
        ) : transcriptError ? (
          <p className="error">{transcriptError}</p>
        ) : null}
      </div>
    </article>
  );
});

function noteDate(value: string | null) {
  if (!value) return "Date unavailable";
  const date = new Date(value.includes("T") ? value : value.replace(" ", "T") + "Z");
  return Number.isNaN(date.getTime()) ? value : new Intl.DateTimeFormat(undefined, { dateStyle: "medium", timeStyle: "short" }).format(date);
}

export function Notes({ initialNoteId = null, openRequest = null, onOpenNote }: { initialNoteId?: number | null; openRequest?: { id: number } | null; onOpenNote?: (id: number) => void }) {
  const [context, setContext] = useState<{ note: Note; x: number; y: number; anchor: HTMLButtonElement } | null>(null);
  const contextRef = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => {
    if (!context || !contextRef.current) return;
    const menu = contextRef.current;
    const bounds = menu.getBoundingClientRect();
    menu.style.left = `${Math.max(8, Math.min(context.x, window.innerWidth - bounds.width - 8))}px`;
    menu.style.top = `${Math.max(8, Math.min(context.y, window.innerHeight - bounds.height - 8))}px`;
    menu.querySelector<HTMLButtonElement>("button")?.focus();
    const outside = (event: PointerEvent) => { if (!menu.contains(event.target as Node)) setContext(null); };
    const escape = (event: KeyboardEvent) => { if (event.key === "Escape") { event.preventDefault(); setContext(null); context.anchor.focus(); } };
    const close = () => setContext(null);
    document.addEventListener("pointerdown", outside);
    document.addEventListener("keydown", escape);
    window.addEventListener("resize", close);
    window.addEventListener("blur", close);
    return () => { document.removeEventListener("pointerdown", outside); document.removeEventListener("keydown", escape); window.removeEventListener("resize", close); window.removeEventListener("blur", close); };
  }, [context]);
  const [libraryTool, setLibraryTool] = useState<"search" | "folders" | "new-folder" | null>(null);
  const [folderName, setFolderName] = useState("");
  const searchGeneration = useRef(0);
  const toolRef = useRef<HTMLDivElement>(null);
  useEffect(() => { toolRef.current?.querySelector<HTMLInputElement | HTMLSelectElement>("input, select")?.focus(); }, [libraryTool]);
  const [libraryExpanded, setLibraryExpanded] = useState(true);
  const [libraryPeek, setLibraryPeek] = useState(false);
  const libraryVisible = libraryExpanded || libraryPeek;
  useEffect(() => {
    if (libraryExpanded) return;
    let closeTimer: ReturnType<typeof setTimeout> | undefined;
    const cancelClose = () => { clearTimeout(closeTimer); closeTimer = undefined; };
    const closeSoon = () => {
      if (!closeTimer) closeTimer = setTimeout(() => { setLibraryPeek(false); closeTimer = undefined; }, 250);
    };
    const move = (event: PointerEvent) => {
      if (event.pointerType !== "mouse") return;
      const target = event.target instanceof Element ? event.target : null;
      if (context || target?.closest('.notes-library, .notes-library-toggle, .notes-context-menu')) { cancelClose(); return; }
      if (event.clientX >= window.innerWidth - 8 && event.clientY >= 38) {
        cancelClose(); setLibraryPeek(true);
      } else {
        closeSoon();
      }
    };
    const leave = () => { if (!context) closeSoon(); };
    document.addEventListener("pointermove", move);
    document.documentElement.addEventListener("pointerleave", leave);
    return () => { cancelClose(); document.removeEventListener("pointermove", move); document.documentElement.removeEventListener("pointerleave", leave); };
  }, [libraryExpanded, context]);

  const [folderFilter, setFolderFilter] = useState<number | null>(null);

  const load = useCallback(async () => {
    const [notes, folders] = await Promise.all([
      getNotes(folderFilter === null ? {} : { folderId: folderFilter }),
      getFolders(),
    ]);
    return { notes, folders };
  }, [folderFilter]);

  const { data, error, guard, reload, fail } = useRefresh(load);

  const [query, setQuery] = useState("");
  const [hits, setHits] = useState<SearchHit[] | null>(null);
  const [exportFormat, setExportFormat] = useState<ExportFormat>("txt");
  const [exportedTo, setExportedTo] = useState<string | null>(null);
  const [openNoteId, setOpenNoteId] = useState<number | null>(initialNoteId);
  const workspaceRef = useRef<NoteWorkspaceHandle>(null);
  const [requestedNote, setRequestedNote] = useState<Note | null>(null);
  const initialSelectionDone = useRef(false);
  useEffect(() => {
    if (!data || initialSelectionDone.current) return;
    initialSelectionDone.current = true;
    if (initialNoteId !== null || openRequest) return;
    const first = data.notes.at(0);
    if (first) { setRequestedNote(first); setOpenNoteId(first.id); onOpenNote?.(first.id); }
  }, [data, initialNoteId, openRequest, onOpenNote]);
  const scrollToRequest = useRef<number | null>(null);
  useEffect(() => {
    if (!openRequest) return;
    let cancelled = false;
    void (async () => {
      await workspaceRef.current?.flush();
      const note = await getNote(openRequest.id);
      if (cancelled) return;
      if (!note) throw new Error("This note is no longer available.");
      scrollToRequest.current = note.id;
      setRequestedNote(note); setFolderFilter(null); setQuery(""); setHits(null); setOpenNoteId(note.id);
    })().catch(e => { if (!cancelled) fail(e); });
    return () => { cancelled = true; };
  }, [openRequest, fail]);
  useEffect(() => {
    if (scrollToRequest.current !== openNoteId) return;
    const element = document.getElementById(`note-detail-${openNoteId}`);
    if (element) { element.scrollIntoView({ block: "start" }); scrollToRequest.current = null; }
  }, [openNoteId, requestedNote, data]);

  const selectNote = async (id: number) => {
    await workspaceRef.current?.flush();
    const note = await getNote(id);
    if (!note) throw new Error("This note is no longer available.");
    setRequestedNote(note);
    setOpenNoteId(id);
    onOpenNote?.(id);
  };
  const newNote = () => guard(async () => {
    await workspaceRef.current?.flush();
    const id = await createNote({ title: "Untitled note", content: "" });
    await selectNote(id);
  });

  if (!data) {
    return (
      <div className="view">
        {error ? <p className="error">{error}</p> : <p className="muted">Loading…</p>}
      </div>
    );
  }

  const titlebarControls = document.getElementById("notes-titlebar-controls");
  const { notes, folders } = data;
  const openNote = notes.find((note) => note.id === openNoteId) ?? (requestedNote?.id === openNoteId ? requestedNote : null);

  return (
    <div className={`view notes-view ${libraryVisible ? "" : "notes-library-collapsed"}`}>
      {error ? <p className="error">{error}</p> : null}

      {titlebarControls && createPortal(<>
            <button type="button" className="notes-library-toggle" aria-label={libraryExpanded ? "Collapse notes sidebar" : "Expand notes sidebar"} title={libraryExpanded ? "Collapse notes sidebar" : "Expand notes sidebar"} aria-expanded={libraryVisible} aria-controls="notes-library-content" onClick={() => { setLibraryPeek(false); setLibraryExpanded(value => !value); }}>
              <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" aria-hidden="true"><path d="M3 3H21V21H3Z M14.5 3V21"/><path d={libraryExpanded ? "M17 10L19 12L17 14" : "M19 10L17 12L19 14"}/></svg>
            </button>
      </>, titlebarControls)}
      {context && createPortal(<div ref={contextRef} className="notes-context-menu" role="dialog" aria-label={`Actions for ${context.note.title}`} style={{ left: context.x, top: context.y }} onContextMenu={event => event.preventDefault()} onBlur={event => { if (!event.currentTarget.contains(event.relatedTarget)) setContext(null); }}>
                <button
                  type="button"
                  onClick={() => {
                    setContext(null);
                    void guard(async () => {
                      const path = await exportNoteToFile(context.note.id, exportFormat);
                      // `null` is a cancelled dialog, not a failure. Saying
                      // "exported" for it would be a lie about a file that does
                      // not exist.
                      setExportedTo(path);
                    });
                  }}
                >
                  Export…
                </button>
                <button
                  type="button"
                  onClick={() => {
                    setContext(null);
                    void getNoteRecording(context.note.id)
                      .then(async (recording) => {
                        const recordingMessage = recording
                          ? ` It has a recording (${
                              recording.durationSeconds === null
                                ? "duration unknown"
                                : formatClock(recording.durationSeconds)
                            }, ${recording.storageKey}) and deleting the note deletes it too.`
                          : " It has no recording.";
                        if (!window.confirm(`Delete “${context.note.title}”?${recordingMessage}`)) return;
                        await workspaceRef.current?.flush();
                        await guard(async () => {
                          await deleteNote(context.note.id);
                          setOpenNoteId((current) =>
                            current === context.note.id ? null : current,
                          );
                        });
                      })
                      .catch((cause: unknown) => {
                        void guard(async () => { throw cause; });
                      });
                  }}
                >
                  Delete
                </button>
        <div className="row">
          <span className="row-label">Export format</span>
          <span className="row-value controls">
            <select aria-label="Export format"
              value={exportFormat}
              onChange={(e) => {
                setExportFormat(e.target.value as ExportFormat);
              }}
            >
              {EXPORT_FORMATS.map((f) => (
                <option key={f} value={f}>
                  {f}
                </option>
              ))}
            </select>
          </span>
        </div>

      </div>, document.body)}
      <div className="notes-layout">
        <section className="notes-main" aria-label="Note editor">
        {openNote ? (
          <NoteWorkspace
            key={openNote.id}
            ref={workspaceRef}
            note={openNote}
            reload={reload}
          />
        ) : <div className="notes-empty"><h3>Your writing space</h3><p className="muted">Choose a note or meeting from the sidebar, or start a new note.</p><button type="button" onClick={() => { void newNote(); }}>New note</button></div>}

        </section>
        <aside className="notes-library" aria-label="Notes and meetings">
          <section id="notes-library-content" data-surface="notes.export" hidden={!libraryVisible}>

        <div className="notes-library-tools" role="toolbar" aria-label="Notes library tools">
          <button type="button" aria-label="New note" title="New note" onClick={() => { void newNote(); }}>
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true"><path d="M14 3H4v18h16V9M14 3v6h6l-6-6ZM8 15h8m-4-4v8" /></svg>
          </button>
          <button type="button" aria-label="Search notes" title="Search notes" aria-expanded={libraryTool === "search"} aria-controls="notes-library-tool-panel" onClick={() => setLibraryTool(value => value === "search" ? null : "search")}>
            <span className="notes-tool-search-icon" aria-hidden="true" />
          </button>
          <button type="button" aria-label="Folders" title="Folders" aria-expanded={libraryTool === "folders"} aria-controls="notes-library-tool-panel" data-active={folderFilter !== null} onClick={() => setLibraryTool(value => value === "folders" ? null : "folders")}>
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinejoin="round" aria-hidden="true"><path d="M3 7V4h6l3 3h9v13H3V7Zm0 3h18" /></svg>
          </button>
          <button type="button" aria-label="New folder" title="New folder" aria-expanded={libraryTool === "new-folder"} aria-controls="notes-library-tool-panel" onClick={() => setLibraryTool(value => value === "new-folder" ? null : "new-folder")}>
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true"><path d="M3 7V4h6l3 3h9v13H3V7Zm6 7h6m-3-3v6" /></svg>
          </button>
        </div>
        <div id="notes-library-tool-panel" ref={toolRef} onKeyDown={event => { if (event.key === "Escape") { setLibraryTool(null); document.querySelector<HTMLButtonElement>('.notes-library-tools button[aria-expanded="true"]')?.focus(); } }}>
          {libraryTool === "search" && <div className="notes-tool-panel">
            <input type="search" aria-label="Search notes and meetings" placeholder="Search notes and meetings" value={query} onChange={event => {
              const next = event.target.value;
              setQuery(next);
              const generation = ++searchGeneration.current;
              if (!next.trim()) { setHits(null); return; }
              void searchNotes(next, 20).then(value => { if (generation === searchGeneration.current) setHits(value); }).catch(fail);
            }} />
          </div>}
          {libraryTool === "folders" && <div className="notes-tool-panel">
            <select aria-label="Folder" value={folderFilter ?? ""} onChange={event => setFolderFilter(event.target.value === "" ? null : Number(event.target.value))}>
              <option value="">All folders</option>
              {folders.map(folder => <option key={folder.id} value={folder.id}>{folder.name}</option>)}
            </select>
            {folderFilter !== null && <button type="button" onClick={() => { void guard(async () => { await deleteFolder(folderFilter); setFolderFilter(null); }); }}>Delete folder</button>}
          </div>}
          {libraryTool === "new-folder" && <form className="notes-tool-panel" onSubmit={event => { event.preventDefault(); if (!folderName.trim()) return; void guard(async () => { await createFolder(folderName.trim()); setFolderName(""); setLibraryTool("folders"); }); }}>
            <input aria-label="Folder name" placeholder="Folder name" value={folderName} onChange={event => setFolderName(event.target.value)} />
            <button type="submit" disabled={!folderName.trim()}>Create folder</button>
          </form>}
        </div>

        {hits === null ? null : (
          <>
            <h3>
              {hits.length === 0
                ? `Nothing matches "${query}"`
                : `${hits.length} match${hits.length === 1 ? "" : "es"}`}
            </h3>
            <ul className="list">
              {hits.map((hit) => (
                <li key={hit.id}>
                  <button type="button" onClick={() => { void selectNote(hit.id).catch(fail); }}>{hit.title}</button>
                  <span className="muted"> {hit.snippet}</span>
                </li>
              ))}
            </ul>
          </>
        )}

        {hits !== null ? null : notes.length === 0 ? (
          <p className="muted">
            No notes in this folder. Create a note, record a meeting, or import a recording.
          </p>
        ) : (
          <ul className="list">
            {notes.map((note) => (
              <li key={note.id}>
                <button type="button" className={`notes-entry ${openNoteId === note.id ? "notes-entry-current" : ""}`} aria-current={openNoteId === note.id ? "page" : undefined} onClick={() => { void selectNote(note.id).catch(fail); }}
                  onContextMenu={event => { event.preventDefault(); setContext({ note, x: event.clientX, y: event.clientY, anchor: event.currentTarget }); }}
                  onKeyDown={event => { if (event.key === "ContextMenu" || (event.shiftKey && event.key === "F10")) { event.preventDefault(); const rect = event.currentTarget.getBoundingClientRect(); setContext({ note, x: rect.left, y: rect.bottom, anchor: event.currentTarget }); } }}
                  aria-haspopup="dialog">
                  <span className="notes-entry-title" title={note.title}>{note.title}</span>
                  <time className="notes-entry-date" dateTime={note.createdAt ?? undefined}>{noteDate(note.createdAt)}</time>
                </button>
              </li>
            ))}
          </ul>
        )}


        {exportedTo ? <Row label="Written to" value={exportedTo} /> : null}


          </section>
        </aside>
      </div>
    </div>
  );
}
