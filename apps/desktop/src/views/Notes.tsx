/**
 * Notes: the library in the list panel, and the open note beside it.
 *
 * The library is the panel `SplitView` draws on every page: New note and
 * Import, search, the folders with their notes indented under them, and the
 * notes in no folder. The title bar's list toggle shows and hides it, as it
 * does every other page's panel. While the library was a column on the right
 * of this page it had a toggle of its own, portalled into the title bar, and
 * a peek that opened it when the pointer reached the window's right edge;
 * both went with the column.
 *
 * Notes > Export (the `notes.export` surface) is reached two ways -- a
 * note's right-click menu in the library, and the open note's actions --
 * and both run `exportNote`, so there is one Export… and one "Written to".
 * That line and any failure sit at the top of the note rather than in the
 * panel, where a collapsed panel would hide them; "Written to" goes when
 * another note opens, so it never sits above a note it is not about.
 *
 * The panel's "Notes" is the page's `h1`, and `SplitView`'s `heading`
 * stands in for it while the panel is collapsed.
 *
 * Coming back to Notes from another page reopens the note that was open when
 * it was left (`lastOpenNoteId`); the first visit, and a visit after that note
 * has gone, opens the most recently edited one.
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
  type DragEvent as ReactDragEvent,
} from "react";

import { createPortal } from "react-dom";

import { Icon } from "../components/Icon";
import { NoteWorkspace, type NoteWorkspaceHandle } from "../components/NoteWorkspace";
import { PanelAction, PanelHeader, PanelItem, PanelSection } from "../components/Panel";
import { formatClock } from "../components/RecordingPlayer";
import { Row } from "../components/Row";
import { SplitView } from "../components/SplitView";
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
import { parseStoredTime } from "../lib/dayGroups";
import { recentMeta } from "../lib/meetingView";
import { groupNotesByFolder } from "../lib/noteGroups";
import { carriesNote, draggedNoteId, folderChange, NOTE_DRAG_TYPE } from "../lib/noteDrag";
import { useRefresh } from "../lib/useRefresh";

function noteDate(value: string | null) {
  if (!value) return "Date unavailable";
  const date = parseStoredTime(value);
  return date === null ? value : new Intl.DateTimeFormat(undefined, { dateStyle: "medium", timeStyle: "short" }).format(date);
}

/**
 * The note open when Notes was last left, for as long as the app is open. Held
 * here rather than in the component because the page unmounts whenever another
 * one opens, as the transcript column's choice is held in `NoteWorkspace.tsx`.
 * Never cleared: a note deleted since -- here or through the API -- is simply
 * not found on the way back, and the most recently edited one opens instead.
 */
let lastOpenNoteId: number | null = null;

/** The right-click menu's target: a note, or a folder heading. */
type ContextMenu = { x: number; y: number; anchor: HTMLButtonElement } & ({ kind: "note"; note: Note } | { kind: "folder"; folder: Folder });

export function Notes({ initialNoteId = null, openRequest = null, onOpenNote, onImport }: { initialNoteId?: number | null; openRequest?: { id: number } | null; onOpenNote?: (id: number) => void; onImport: () => void }) {
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
  const [libraryTool, setLibraryTool] = useState<"new-folder" | null>(null);
  const [folderName, setFolderName] = useState("");
  const searchGeneration = useRef(0);
  const toolRef = useRef<HTMLDivElement>(null);
  const newFolderButton = useRef<HTMLButtonElement>(null);
  useEffect(() => { toolRef.current?.querySelector<HTMLInputElement | HTMLSelectElement>("input, select")?.focus(); }, [libraryTool]);
  // A press anywhere but the open panel closes it, as the context menu closes. The New folder
  // button that opens it is left to its own click handler: this runs on pointerdown, before
  // click, and closing here first would make the button's toggle reopen what it was pressed to
  // close.
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

  // Which folders are closed in the library. Session state on purpose: a
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
  const alive = useRef(true);
  useEffect(() => { alive.current = true; return () => { alive.current = false; }; }, []);
  useEffect(() => {
    if (!data || initialSelectionDone.current) return;
    initialSelectionDone.current = true;
    if (initialNoteId !== null || openRequest) return;
    const open = (note: Note) => { setRequestedNote(note); reveal(note.folderId); setOpenNoteId(note.id); onOpenNote?.(note.id); };
    const newest = () => { const first = data.notes.at(0); if (first) open(first); };
    const last = lastOpenNoteId;
    const listed = data.notes.find(note => note.id === last);
    if (listed) { open(listed); return; }
    if (last === null) { newest(); return; }
    // Not among the library's newest notes -- an older one opened from search -- so fetched, as an
    // open request is. Not opened once the page has gone: `onOpenNote` would navigate back to it.
    void getNote(last).then(note => { if (!alive.current) return; if (note) open(note); else newest(); }).catch(fail);
  }, [data, initialNoteId, openRequest, onOpenNote, reveal, fail]);
  useEffect(() => { if (openNoteId !== null) lastOpenNoteId = openNoteId; }, [openNoteId]);
  const mainRef = useRef<HTMLElement>(null);
  // A note opens at its top. The pane outlives the note inside it, so on its own it keeps the
  // previous note's offset, clamped against the new note's first layout before the editor has its
  // content: measured on 2026-09-26 at 122px in a 480px window. An open request used to call
  // scrollIntoView on the article instead, which parks it flush with the pane's edge, 21px past
  // the top padding. Layout timing, so the reset lands before the first paint. In a window wide
  // enough for the transcript column the note scrolls inside its own article instead, which is
  // keyed by note and so starts at the top by itself.
  useLayoutEffect(() => { if (mainRef.current) mainRef.current.scrollTop = 0; }, [openNoteId, openRequest]);
  // "Written to" is about the export just made, and it sits above the open note: once another
  // note opens it would read as that note's file. Layout timing, so it is gone before the new
  // note's first paint rather than one frame after.
  useLayoutEffect(() => { setExportedTo(null); }, [openNoteId]);
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
  // The one Export…, for the right-click menu and the open note's actions alike. Flushed first
  // when it is the open note, as selecting, moving and deleting are: the Export button sits
  // just above the editor, and a file written from the stored text would miss whatever was typed
  // in the last moment. Only then: the workspace holds the open note's edits and no other's, so
  // flushing for another note's export gains nothing, and a save of the open note that keeps
  // failing would block exporting a note it has nothing to do with.
  const exportNote = (id: number) => {
    void guard(async () => {
      if (id === openNoteId) await workspaceRef.current?.flush();
      const path = await exportNoteToFile(id, exportFormat);
      // `null` is a cancelled dialog, not a failure. Saying
      // "exported" for it would be a lie about a file that does
      // not exist.
      setExportedTo(path);
    });
  };
  // A note's menu, at the pointer for a right-click and under the button otherwise: the
  // keyboard's menu key, Shift+F10, and the open note's More.
  const openNoteMenu = (note: Note, anchor: HTMLButtonElement, at?: { x: number; y: number }) => {
    const rect = anchor.getBoundingClientRect();
    setContext({ kind: "note", note, x: at?.x ?? rect.left, y: at?.y ?? rect.bottom, anchor });
  };

  // Filing by drag and drop, and by the right-click menu, through one move.
  // `drag` is the note in the air, whose folder decides whether the "No folder"
  // strip is offered; `dropTarget` is the group under the pointer: a folder id,
  // null for the strip, undefined for none.
  const [drag, setDrag] = useState<{ noteId: number; folderId: number | null } | null>(null);
  const [dropTarget, setDropTarget] = useState<number | null | undefined>(undefined);
  const moveNote = useCallback((note: Note, folderId: number | null) => {
    reveal(folderId);
    // Flushed first: the workspace may hold an unsaved edit of this very note,
    // and the reload after the move would show the stored text over the typed one.
    void guard(async () => { await workspaceRef.current?.flush(); await setNoteFolder(note.id, folderId); });
  }, [guard, reveal]);
  const dropHandlers = (target: number | null) => ({
    onDragOver: (event: ReactDragEvent<HTMLElement>) => {
      if (!carriesNote(event.dataTransfer.types)) return;
      event.preventDefault();
      event.dataTransfer.dropEffect = "move";
      setDropTarget(target);
    },
    onDragLeave: (event: ReactDragEvent<HTMLElement>) => {
      if (event.relatedTarget instanceof Node && event.currentTarget.contains(event.relatedTarget)) return;
      setDropTarget(current => (current === target ? undefined : current));
    },
    onDrop: (event: ReactDragEvent<HTMLElement>) => {
      const id = draggedNoteId(event.dataTransfer);
      if (id === null) return;
      event.preventDefault();
      setDropTarget(undefined);
      setDrag(null);
      const note = data?.notes.find(item => item.id === id);
      if (note && folderChange(note.folderId, target)) moveNote(note, target);
    },
  });

  if (!data) {
    return (
      <SplitView label="Notes library" heading="Notes" panel={<PanelHeader title="Notes" />}>
        <section className="notes-main" aria-label="Note editor">
          <div className="notes-notices">{error ? <p className="error">{error}</p> : <p className="muted">Loading…</p>}</div>
        </section>
      </SplitView>
    );
  }

  const { notes, folders } = data;
  const openNote = notes.find((note) => note.id === openNoteId) ?? (requestedNote?.id === openNoteId ? requestedNote : null);
  const groups = groupNotesByFolder(notes, folders);
  const now = new Date();
  const renderEntry = (note: Note) => {
    // The Meetings list's meta line, "26 Aug · 31 min", so a meeting reads the same in both panels.
    const meta = recentMeta(note.createdAt, note.audioDurationSeconds, now);
    return (
      <PanelItem key={note.id} className="notes-entry" titleClassName="notes-entry-title"
        title={note.title} {...(meta === "" ? {} : { meta })} current={openNoteId === note.id}
        // The title, which a long one loses to the ellipsis, and the full creation time the meta line shortens.
        tooltip={`${note.title}\n${noteDate(note.createdAt)}`}
        onClick={() => { void selectNote(note.id).catch(fail); }}
        draggable data-dragging={drag?.noteId === note.id || undefined}
        onDragStart={event => { event.dataTransfer.setData(NOTE_DRAG_TYPE, String(note.id)); event.dataTransfer.effectAllowed = "move"; setDrag({ noteId: note.id, folderId: note.folderId }); }}
        onDragEnd={() => { setDrag(null); setDropTarget(undefined); }}
        onContextMenu={event => { event.preventDefault(); openNoteMenu(note, event.currentTarget, { x: event.clientX, y: event.clientY }); }}
        onKeyDown={event => { if (event.key === "ContextMenu" || (event.shiftKey && event.key === "F10")) { event.preventDefault(); openNoteMenu(note, event.currentTarget); } }}
        aria-haspopup="dialog" />
    );
  };

  const panel = (
    <>
      <PanelHeader title="Notes" />
      <div className="panel-surface" data-surface="notes.export">
        <PanelSection label={null}>
          <PanelAction icon="newNote" label="New note" onClick={() => { void newNote(); }} />
          <PanelAction icon="import" label="Import audio or video" onClick={onImport} />
        </PanelSection>
        <label className="panel-search">
          <Icon name="search" size={15} />
          {/* Not "Search notes and meetings": that is the title bar's field, and two fields of one name cannot be told apart in a screen reader's list of fields. */}
          <input type="search" aria-label="Filter the library" placeholder="Filter the library" value={query} onChange={event => {
            const next = event.target.value;
            setQuery(next);
            const generation = ++searchGeneration.current;
            if (!next.trim()) { setHits(null); return; }
            void searchNotes(next, 20).then(value => { if (generation === searchGeneration.current) setHits(value); }).catch(fail);
          }} />
        </label>

        {hits === null ? null : hits.length === 0 ? (
          <p className="panel-note">{`Nothing matches "${query}"`}</p>
        ) : (
          <PanelSection label={`${hits.length} match${hits.length === 1 ? "" : "es"}`}>
            {hits.map((hit) => (
              <PanelItem key={hit.id} className="notes-hit" title={hit.title} meta={hit.snippet} tooltip={hit.title}
                current={openNoteId === hit.id} onClick={() => { void selectNote(hit.id).catch(fail); }} />
            ))}
          </PanelSection>
        )}

        {hits !== null ? null : (
          <>
            <div className="panel-section">
              <div className="notes-folders-head">
                <h2 className="panel-label">Folders</h2>
                <button ref={newFolderButton} type="button" className="panel-tool" aria-label="New folder" title="New folder" aria-expanded={libraryTool === "new-folder"} aria-controls="notes-library-tool-panel" onClick={() => setLibraryTool(value => value === "new-folder" ? null : "new-folder")}>
                  <Icon name="folderPlus" size={16} />
                </button>
                <button type="button" className="panel-tool" aria-label="Expand all folders" title="Expand all folders" disabled={folders.length === 0} onClick={() => setCollapsed(new Set())}>
                  <Icon name="expandAll" size={16} />
                </button>
                <button type="button" className="panel-tool" aria-label="Collapse all folders" title="Collapse all folders" disabled={folders.length === 0} onClick={() => setCollapsed(new Set(folders.map(folder => folder.id)))}>
                  <Icon name="collapseAll" size={16} />
                </button>
              </div>
              <div id="notes-library-tool-panel" ref={toolRef} onKeyDown={event => { if (event.key === "Escape") { setLibraryTool(null); newFolderButton.current?.focus(); } }}>
                {libraryTool === "new-folder" && <form className="notes-tool-panel" onSubmit={event => { event.preventDefault(); if (!folderName.trim()) return; void guard(async () => { await createFolder(folderName.trim()); setFolderName(""); setLibraryTool(null); }); }}>
                  <input aria-label="Folder name" placeholder="Folder name" value={folderName} onChange={event => setFolderName(event.target.value)} />
                  <button type="submit" disabled={!folderName.trim()}>Create folder</button>
                </form>}
              </div>
              {notes.length === 0 && folders.length === 0 ? (
                <p className="panel-note">
                  No notes yet. Create a note, record a meeting, or import a recording.
                </p>
              ) : (
                <div className="panel-list">
                  {groups.folders.map(({ folder, notes: filed }) => {
                    const open = !collapsed.has(folder.id);
                    return (
                      <section key={folder.id} className="notes-folder" data-drop-target={dropTarget === folder.id || undefined} {...dropHandlers(folder.id)}>
                        <button type="button" className="notes-folder-toggle" aria-expanded={open} aria-controls={`notes-folder-${folder.id}`} aria-haspopup="dialog"
                          onClick={() => setCollapsed(prev => { const next = new Set(prev); if (next.has(folder.id)) next.delete(folder.id); else next.add(folder.id); return next; })}
                          onContextMenu={event => { event.preventDefault(); setContext({ kind: "folder", folder, x: event.clientX, y: event.clientY, anchor: event.currentTarget }); }}
                          onKeyDown={event => { if (event.key === "ContextMenu" || (event.shiftKey && event.key === "F10")) { event.preventDefault(); const rect = event.currentTarget.getBoundingClientRect(); setContext({ kind: "folder", folder, x: rect.left, y: rect.bottom, anchor: event.currentTarget }); } }}>
                          <Icon name={open ? "chevronDown" : "chevronRight"} size={12} />
                          <Icon name="folder" size={16} />
                          <span className="notes-folder-name" title={folder.name}>{folder.name}</span>
                          <span className="notes-folder-count"><span className="sr-only">, </span>{filed.length}<span className="sr-only">{filed.length === 1 ? " note" : " notes"}</span></span>
                        </button>
                        <div id={`notes-folder-${folder.id}`} className="notes-folder-notes" hidden={!open}>
                          {filed.length ? filed.map(renderEntry) : <p className="notes-folder-empty">No notes in this folder.</p>}
                        </div>
                      </section>
                    );
                  })}
                  {/* The strip sits just above the unfiled notes, below the folders: a note dragged out
                      of a folder starts above it, so its appearance moves nothing under the pointer. */}
                  {drag && drag.folderId !== null && folders.length > 0 && (
                    <div className="notes-drop-unfiled" data-drop-target={dropTarget === null || undefined} {...dropHandlers(null)}>No folder</div>
                  )}
                </div>
              )}
            </div>
            {groups.unfiled.length > 0 && <PanelSection label="Not in a folder">{groups.unfiled.map(renderEntry)}</PanelSection>}
          </>
        )}
      </div>
    </>
  );

  return (
    <SplitView label="Notes library" heading="Notes" panel={panel}>
      <section className="notes-main" aria-label="Note editor" ref={mainRef}>
        {error || exportedTo ? (
          <div className="notes-notices">
            {error ? <p className="error">{error}</p> : null}
            {exportedTo ? <Row label="Written to" value={exportedTo} /> : null}
          </div>
        ) : null}
        {openNote ? (
          <NoteWorkspace
            key={openNote.id}
            ref={workspaceRef}
            note={openNote}
            reload={reload}
            onExport={() => exportNote(openNote.id)}
            onMore={anchor => openNoteMenu(openNote, anchor)}
          />
        ) : <div className="notes-empty"><h2>Your writing space</h2><p className="muted">Choose a note or meeting from the list, or start a new note.</p><button type="button" className="primary" onClick={() => { void newNote(); }}>New note</button> <button type="button" onClick={onImport}>Import audio or video</button></div>}
      </section>
      {context && createPortal(<div ref={contextRef} className="notes-context-menu" role="dialog" aria-label={context.kind === "note" ? `Actions for ${context.note.title}` : `Actions for folder ${context.folder.name}`} style={{ left: context.x, top: context.y }} onContextMenu={event => event.preventDefault()} onBlur={event => { if (!event.currentTarget.contains(event.relatedTarget)) setContext(null); }}>
        {context.kind === "note" ? <>
                <button
                  type="button"
                  onClick={() => {
                    setContext(null);
                    exportNote(context.note.id);
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
              moveNote(note, folderId);
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
    </SplitView>
  );
}
