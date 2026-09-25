import { NoteWorkspace, type NoteWorkspaceHandle } from "../components/NoteWorkspace";
/**
 * Notes > Export.
 *
 * Two states that look alike and are not: `hits === null` means nobody has
 * searched, and `hits === []` means somebody searched and nothing matched.
 * Collapsing them would show the full note list as though it were the result.
 */

import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
} from "react";

import { createPortal } from "react-dom";

import { formatClock } from "../components/RecordingPlayer";
import { Row } from "../components/Row";
import {
  createFolder,
  createNote,
  deleteFolder,
  deleteNote,
  exportNoteToFile,
  EXPORT_FORMATS,
  getFolders,
  getNotes,
  getNote,
  searchNotes,
  setNoteFolder,
  getNoteRecording,
  type ExportFormat,
  type Folder,
  type Note,
  type SearchHit,
} from "../lib/ipc";
import { groupNotesByFolder } from "../lib/noteGroups";
import { useRefresh } from "../lib/useRefresh";

function noteDate(value: string | null) {
  if (!value) return "Date unavailable";
  const date = new Date(value.includes("T") ? value : value.replace(" ", "T") + "Z");
  return Number.isNaN(date.getTime()) ? value : new Intl.DateTimeFormat(undefined, { dateStyle: "medium", timeStyle: "short" }).format(date);
}

/** The right-click menu's target: a note, or a folder heading. */
type ContextMenu = { x: number; y: number; anchor: HTMLButtonElement } & ({ kind: "note"; note: Note } | { kind: "folder"; folder: Folder });

export function Notes({ initialNoteId = null, openRequest = null, onOpenNote }: { initialNoteId?: number | null; openRequest?: { id: number } | null; onOpenNote?: (id: number) => void }) {
  const [context, setContext] = useState<ContextMenu | null>(null);
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
  const [libraryTool, setLibraryTool] = useState<"search" | "new-folder" | null>(null);
  const [folderName, setFolderName] = useState("");
  const searchGeneration = useRef(0);
  const toolRef = useRef<HTMLDivElement>(null);
  useEffect(() => { toolRef.current?.querySelector<HTMLInputElement | HTMLSelectElement>("input, select")?.focus(); }, [libraryTool]);
  // A press anywhere but the open panel closes it, as the context menu closes. The two buttons that
  // open the panels are left to their own click handlers: this runs on pointerdown, before click,
  // and closing here first would make the button's toggle reopen what it was pressed to close.
  useEffect(() => {
    if (!libraryTool) return;
    const outside = (event: PointerEvent) => {
      const target = event.target instanceof Element ? event.target : null;
      if (toolRef.current?.contains(target) || target?.closest('[aria-controls="notes-library-tool-panel"]')) return;
      setLibraryTool(null);
    };
    document.addEventListener("pointerdown", outside);
    return () => document.removeEventListener("pointerdown", outside);
  }, [libraryTool]);
  const [libraryExpanded, setLibraryExpanded] = useState(() => !window.matchMedia("(max-width: 900px)").matches);
  const [libraryPeek, setLibraryPeek] = useState(false);
  useEffect(() => {
    const narrow = window.matchMedia("(max-width: 900px)");
    const collapse = () => { if (narrow.matches) { setLibraryExpanded(false); setLibraryPeek(false); } };
    narrow.addEventListener("change", collapse);
    return () => narrow.removeEventListener("change", collapse);
  }, []);
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

  // Which folders are closed in the sidebar. Session state on purpose: a
  // setting would need a settings key and a screen, and a folder closed an
  // hour ago is not a preference worth that.
  const [collapsed, setCollapsed] = useState<Set<number>>(() => new Set());
  // Open the folder a note is being shown from, so a note reached by search,
  // by a link or by moving it is never selected inside a closed group.
  const reveal = useCallback((folderId: number | null) => {
    if (folderId === null) return;
    setCollapsed(prev => { if (!prev.has(folderId)) return prev; const next = new Set(prev); next.delete(folderId); return next; });
  }, []);

  const load = useCallback(async () => {
    const [notes, folders] = await Promise.all([getNotes(), getFolders()]);
    return { notes, folders };
  }, []);

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
    if (first) { setRequestedNote(first); reveal(first.folderId); setOpenNoteId(first.id); onOpenNote?.(first.id); }
  }, [data, initialNoteId, openRequest, onOpenNote, reveal]);
  const mainRef = useRef<HTMLElement>(null);
  // A note opens at its top. The pane outlives the note inside it, so on its own it keeps the
  // previous note's offset, clamped against the new note's first layout before the editor has its
  // content: measured on 2026-09-26 at 122px in a 480px window. An open request used to call
  // scrollIntoView on the article instead, which parks it flush with the pane's edge, 21px past
  // the top padding. Layout timing, so the reset lands before the first paint.
  useLayoutEffect(() => { if (mainRef.current) mainRef.current.scrollTop = 0; }, [openNoteId, openRequest]);
  useEffect(() => {
    if (!openRequest) return;
    let cancelled = false;
    void (async () => {
      await workspaceRef.current?.flush();
      const note = await getNote(openRequest.id);
      if (cancelled) return;
      if (!note) throw new Error("This note is no longer available.");
      setRequestedNote(note); reveal(note.folderId); setQuery(""); setHits(null); setOpenNoteId(note.id);
    })().catch(e => { if (!cancelled) fail(e); });
    return () => { cancelled = true; };
  }, [openRequest, fail, reveal]);

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
  const groups = groupNotesByFolder(notes, folders);
  const renderEntries = (entries: readonly Note[]) => (
    <ul className="list">
      {entries.map((note) => (
        <li key={note.id}>
          <button type="button" className={`notes-entry ${openNoteId === note.id ? "notes-entry-current" : ""}`} aria-current={openNoteId === note.id ? "page" : undefined} onClick={() => { void selectNote(note.id).catch(fail); }}
            onContextMenu={event => { event.preventDefault(); setContext({ kind: "note", note, x: event.clientX, y: event.clientY, anchor: event.currentTarget }); }}
            onKeyDown={event => { if (event.key === "ContextMenu" || (event.shiftKey && event.key === "F10")) { event.preventDefault(); const rect = event.currentTarget.getBoundingClientRect(); setContext({ kind: "note", note, x: rect.left, y: rect.bottom, anchor: event.currentTarget }); } }}
            aria-haspopup="dialog">
            <span className="notes-entry-title" title={note.title}>{note.title}</span>
            <time className="notes-entry-date" dateTime={note.createdAt ?? undefined}>{noteDate(note.createdAt)}</time>
          </button>
        </li>
      ))}
    </ul>
  );

  return (
    <div className={`view notes-view ${libraryVisible ? "" : "notes-library-collapsed"}`}>
      {error ? <p className="error">{error}</p> : null}

      {titlebarControls && createPortal(<>
            <button type="button" className="notes-library-toggle" aria-label={libraryExpanded ? "Collapse notes sidebar" : "Expand notes sidebar"} title={libraryExpanded ? "Collapse notes sidebar" : "Expand notes sidebar"} aria-expanded={libraryVisible} aria-controls="notes-library-content" onClick={() => { setLibraryPeek(false); setLibraryExpanded(value => !value); }}>
              <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" aria-hidden="true"><path d="M3 3H21V21H3Z M14.5 3V21"/><path d={libraryExpanded ? "M17 10L19 12L17 14" : "M19 10L17 12L19 14"}/></svg>
            </button>
      </>, titlebarControls)}
      {context && createPortal(<div ref={contextRef} className="notes-context-menu" role="dialog" aria-label={context.kind === "note" ? `Actions for ${context.note.title}` : `Actions for folder ${context.folder.name}`} style={{ left: context.x, top: context.y }} onContextMenu={event => event.preventDefault()} onBlur={event => { if (!event.currentTarget.contains(event.relatedTarget)) setContext(null); }}>
        {context.kind === "note" ? <>
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

        <div className="row">
          <span className="row-label">Move to folder</span>
          <span className="row-value">
            <select aria-label="Move to folder" value={context.note.folderId ?? ""} onChange={event => {
              const note = context.note;
              const folderId = event.target.value === "" ? null : Number(event.target.value);
              setContext(null);
              reveal(folderId);
              // Flushed first: the workspace may hold an unsaved edit of this
              // very note, and the reload after the move would show the stored
              // text over the typed one.
              void guard(async () => { await workspaceRef.current?.flush(); await setNoteFolder(note.id, folderId); });
            }}>
              <option value="">No folder</option>
              {folders.map(folder => <option key={folder.id} value={folder.id}>{folder.name}</option>)}
            </select>
          </span>
        </div>
        </> : <button type="button" onClick={() => {
          const { folder } = context;
          setContext(null);
          // Deleting a folder never deletes a note: the database sets the
          // note's folder to null, so the confirmation says so.
          if (!window.confirm(`Delete folder “${folder.name}”? Its notes are kept and become unfiled.`)) return;
          void guard(async () => { await deleteFolder(folder.id); });
        }}>Delete folder</button>}
      </div>, document.body)}
      <div className="notes-layout">
        <section className="notes-main" aria-label="Note editor" ref={mainRef}>
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
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true"><path d="M4 12V2.6C4 2.26863 4.26863 2 4.6 2H16.2515C16.4106 2 16.5632 2.06321 16.6757 2.17574L19.8243 5.32426C19.9368 5.43679 20 5.5894 20 5.74853V21.4C20 21.7314 19.7314 22 19.4 22H11" /><path d="M16 2V5.4C16 5.73137 16.2686 6 16.6 6H20" /><path d="M1.99219 19H4.99219M7.99219 19H4.99219M4.99219 19V16M4.99219 19V22" /></svg>
          </button>
          <button type="button" aria-label="Search notes" title="Search notes" aria-expanded={libraryTool === "search"} aria-controls="notes-library-tool-panel" onClick={() => setLibraryTool(value => value === "search" ? null : "search")}>
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true"><path d="M17 17L21 21" /><path d="M3 11C3 15.4183 6.58172 19 11 19C13.213 19 15.2161 18.1015 16.6644 16.6493C18.1077 15.2022 19 13.2053 19 11C19 6.58172 15.4183 3 11 3C6.58172 3 3 6.58172 3 11Z" /></svg>
          </button>
          <button type="button" aria-label="New folder" title="New folder" aria-expanded={libraryTool === "new-folder"} aria-controls="notes-library-tool-panel" onClick={() => setLibraryTool(value => value === "new-folder" ? null : "new-folder")}>
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true"><path d="M18 6H20M22 6H20M20 6V4M20 6V8" /><path d="M21.4 20H2.6C2.26863 20 2 19.7314 2 19.4V11H21.4C21.7314 11 22 11.2686 22 11.6V19.4C22 19.7314 21.7314 20 21.4 20Z" /><path d="M2 11V4.6C2 4.26863 2.26863 4 2.6 4H8.77805C8.92127 4 9.05977 4.05124 9.16852 4.14445L12.3315 6.85555C12.4402 6.94876 12.5787 7 12.722 7H14" /></svg>
          </button>
          {/* The two line icons touch the top and bottom of their box, so the strokes are let out of it. */}
          <button type="button" aria-label="Expand all folders" title="Expand all folders" disabled={folders.length === 0} onClick={() => setCollapsed(new Set())}>
            <svg viewBox="0 0 24 24" overflow="visible" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true"><path d="M18 2L6 2" /><path d="M18 22L6 22" /><path d="M12 14V19M12 19L15 16M12 19L9 16" /><path d="M12 10V5M12 5L15 8M12 5L9 8" /></svg>
          </button>
          <button type="button" aria-label="Collapse all folders" title="Collapse all folders" disabled={folders.length === 0} onClick={() => setCollapsed(new Set(folders.map(folder => folder.id)))}>
            <svg viewBox="0 0 24 24" overflow="visible" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true"><path d="M18 2L6 2" /><path d="M18 22L6 22" /><path d="M12 5V10M12 10L15 7M12 10L9 7" /><path d="M12 19V14M12 14L15 17M12 14L9 17" /></svg>
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
          {libraryTool === "new-folder" && <form className="notes-tool-panel" onSubmit={event => { event.preventDefault(); if (!folderName.trim()) return; void guard(async () => { await createFolder(folderName.trim()); setFolderName(""); setLibraryTool(null); }); }}>
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

        {hits !== null ? null : notes.length === 0 && folders.length === 0 ? (
          <p className="muted">
            No notes yet. Create a note, record a meeting, or import a recording.
          </p>
        ) : (
          <>
            {groups.unfiled.length > 0 && renderEntries(groups.unfiled)}
            {groups.folders.map(({ folder, notes: filed }) => {
              const open = !collapsed.has(folder.id);
              return (
                <section key={folder.id} className="notes-folder">
                  <button type="button" className="notes-folder-toggle" aria-expanded={open} aria-controls={`notes-folder-${folder.id}`} aria-haspopup="dialog"
                    onClick={() => setCollapsed(prev => { const next = new Set(prev); if (next.has(folder.id)) next.delete(folder.id); else next.add(folder.id); return next; })}
                    onContextMenu={event => { event.preventDefault(); setContext({ kind: "folder", folder, x: event.clientX, y: event.clientY, anchor: event.currentTarget }); }}
                    onKeyDown={event => { if (event.key === "ContextMenu" || (event.shiftKey && event.key === "F10")) { event.preventDefault(); const rect = event.currentTarget.getBoundingClientRect(); setContext({ kind: "folder", folder, x: rect.left, y: rect.bottom, anchor: event.currentTarget }); } }}>
                    <svg className="notes-folder-chevron" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true"><path d="M9 6l6 6-6 6" /></svg>
                    <span className="notes-folder-name" title={folder.name}>{folder.name}</span>
                    <span className="notes-folder-count muted">{filed.length}</span>
                  </button>
                  <div id={`notes-folder-${folder.id}`} hidden={!open}>
                    {filed.length ? renderEntries(filed) : <p className="muted notes-folder-empty">No notes in this folder.</p>}
                  </div>
                </section>
              );
            })}
          </>
        )}


        {exportedTo ? <Row label="Written to" value={exportedTo} /> : null}


          </section>
        </aside>
      </div>
    </div>
  );
}
