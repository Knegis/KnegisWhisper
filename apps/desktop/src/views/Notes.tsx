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
  useRef,
  useState,
} from "react";

import { NoteEditor } from "../components/NoteEditor";
import { formatClock } from "../components/RecordingPlayer";
import { Row } from "../components/Row";
import { TranscriptPanel } from "../components/TranscriptPanel";
import {
  createFolder,
  deleteFolder,
  deleteNote,
  exportNote,
  exportNoteToFile,
  EXPORT_FORMATS,
  getFolders,
  getNoteRecording,
  getNoteTranscriptRows,
  getNotes,
  searchNotes,
  updateNote,
  type ExportedTranscript,
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
  if (status === "idle") return null;
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
  { note: Note; reload: () => Promise<void>; onClose: () => void }
>(function NoteWorkspace({ note, reload, onClose }, ref) {
  const initialTitleRef = useRef(note.title);
  const savedRef = useRef({ title: note.title, content: note.content });
  const [title, setTitle] = useState(initialTitleRef.current);
  const [transcript, setTranscript] = useState<{
    rows: Awaited<ReturnType<typeof getNoteTranscriptRows>>;
    recording: Awaited<ReturnType<typeof getNoteRecording>>;
  } | null>(null);
  const [transcriptError, setTranscriptError] = useState<string | null>(null);

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

  useImperativeHandle(ref, () => ({ flush: autosave.flush }), [autosave.flush]);

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
    <article className="note-detail" aria-labelledby={`note-title-${note.id}`}>
      <header className="note-detail-header">
        <input
          id={`note-title-${note.id}`}
          aria-label="Title"
          value={title}
          onChange={(event) => {
            const next = event.target.value;
            setTitle(next);
            if (next.trim() !== "") autosave.schedule({ title: next });
          }}
        />
        <SaveStatus status={autosave.status} error={autosave.error} retry={autosave.retry} />
        <button
          type="button"
          onClick={() => {
            void autosave.flush().then(onClose);
          }}
          aria-label={`Close note ${note.title}`}
        >
          Close
        </button>
      </header>
      <div id={`note-body-${note.id}`} className="note-workspace">
        <NoteEditor
          key={note.id}
          noteId={note.id}
          initialContent={note.content}
          onChange={(content) => autosave.schedule({ content })}
        />
        {transcript?.rows.length ? (
          <TranscriptPanel
            rows={transcript.rows}
            recording={transcript.recording}
            noteId={note.id}
          />
        ) : transcriptError ? (
          <p className="error">{transcriptError}</p>
        ) : null}
      </div>
    </article>
  );
});

export function Notes() {
  const [folderFilter, setFolderFilter] = useState<number | null>(null);

  const load = useCallback(async () => {
    const [notes, folders] = await Promise.all([
      getNotes(folderFilter === null ? {} : { folderId: folderFilter }),
      getFolders(),
    ]);
    return { notes, folders };
  }, [folderFilter]);

  const { data, error, guard, reload } = useRefresh(load);

  const [query, setQuery] = useState("");
  const [hits, setHits] = useState<SearchHit[] | null>(null);
  const [exportFormat, setExportFormat] = useState<ExportFormat>("txt");
  const [preview, setPreview] = useState<ExportedTranscript | null>(null);
  const [exportedTo, setExportedTo] = useState<string | null>(null);
  const [openNoteId, setOpenNoteId] = useState<number | null>(null);
  const workspaceRef = useRef<NoteWorkspaceHandle>(null);

  if (!data) {
    return (
      <div className="view">
        {error ? <p className="error">{error}</p> : <p className="muted">Loading…</p>}
      </div>
    );
  }

  const { notes, folders } = data;
  const openNote = notes.find((note) => note.id === openNoteId) ?? null;

  return (
    <div className="view">
      {error ? <p className="error">{error}</p> : null}

      <section data-surface="notes.export">
        <h2>Notes</h2>

        <div className="row">
          <span className="row-label">Search</span>
          <span className="row-value controls">
            <input
              type="search"
              value={query}
              placeholder="Words to look for"
              onChange={(e) => {
                const next = e.target.value;
                setQuery(next);
                // An empty box is not a search for nothing — it is not
                // searching. Rust refuses an empty query, so never send one.
                if (next.trim() === "") {
                  setHits(null);
                  return;
                }
                void guard(async () => {
                  setHits(await searchNotes(next, 20));
                });
              }}
            />
          </span>
        </div>

        <div className="row">
          <span className="row-label">Folder</span>
          <span className="row-value controls">
            <select
              value={folderFilter ?? ""}
              onChange={(e) => {
                setFolderFilter(
                  e.target.value === "" ? null : Number(e.target.value),
                );
              }}
            >
              <option value="">All folders</option>
              {folders.map((f) => (
                <option key={f.id} value={f.id}>
                  {f.name}
                </option>
              ))}
            </select>
            <button
              type="button"
              onClick={() => {
                const name = window.prompt("Folder name");
                if (name === null || name.trim() === "") return;
                void guard(() => createFolder(name));
              }}
            >
              New folder
            </button>
            {folderFilter === null ? null : (
              <button
                type="button"
                onClick={() => {
                  void guard(async () => {
                    await deleteFolder(folderFilter);
                    // The notes survive with no folder, so the filter that
                    // pointed at it no longer selects anything real.
                    setFolderFilter(null);
                  });
                }}
              >
                Delete folder
              </button>
            )}
          </span>
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
                  <strong>{hit.title}</strong>
                  <span className="muted"> {hit.snippet}</span>
                </li>
              ))}
            </ul>
          </>
        )}

        {notes.length === 0 ? (
          <p className="muted">
            No notes yet. Meetings and imported recordings write them; dictation
            writes history instead.
          </p>
        ) : (
          <ul className="list">
            {notes.map((note) => (
              <li key={note.id}>
                <strong>{note.title}</strong>
                <span className="muted">
                  {" "}
                  {note.noteType}
                  {note.transcript ? " · has a transcript" : ""}
                  {note.updatedAt ? ` · ${note.updatedAt}` : ""}
                </span>
                <button
                  type="button"
                  aria-expanded={openNoteId === note.id}
                  aria-controls={`note-body-${note.id}`}
                  onClick={() => {
                    void (async () => {
                      await workspaceRef.current?.flush();
                      setOpenNoteId((current) =>
                        current === note.id ? null : note.id,
                      );
                    })();
                  }}
                >
                  {openNoteId === note.id ? "Close note" : "Open note"}
                </button>
                <button
                  type="button"
                  onClick={() => {
                    void guard(async () => {
                      setExportedTo(null);
                      setPreview(await exportNote(note.id, exportFormat));
                    });
                  }}
                >
                  Preview {exportFormat}
                </button>
                <button
                  type="button"
                  onClick={() => {
                    void guard(async () => {
                      const path = await exportNoteToFile(note.id, exportFormat);
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
                    void getNoteRecording(note.id)
                      .then(async (recording) => {
                        const recordingMessage = recording
                          ? ` It has a recording (${
                              recording.durationSeconds === null
                                ? "duration unknown"
                                : formatClock(recording.durationSeconds)
                            }, ${recording.storageKey}) and deleting the note deletes it too.`
                          : " It has no recording.";
                        if (!window.confirm(`Delete “${note.title}”?${recordingMessage}`)) return;
                        await workspaceRef.current?.flush();
                        await guard(async () => {
                          await deleteNote(note.id);
                          setOpenNoteId((current) =>
                            current === note.id ? null : current,
                          );
                          setPreview(null);
                        });
                      })
                      .catch((cause: unknown) => {
                        void guard(async () => { throw cause; });
                      });
                  }}
                >
                  Delete
                </button>
              </li>
            ))}
          </ul>
        )}

        {openNote ? (
          <NoteWorkspace
            key={openNote.id}
            ref={workspaceRef}
            note={openNote}
            reload={reload}
            onClose={() => setOpenNoteId(null)}
          />
        ) : null}

        <div className="row">
          <span className="row-label">Export format</span>
          <span className="row-value controls">
            <select
              value={exportFormat}
              onChange={(e) => {
                setExportFormat(e.target.value as ExportFormat);
                setPreview(null);
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

        {exportedTo ? <Row label="Written to" value={exportedTo} /> : null}

        {preview ? (
          <>
            <Row
              label={`Preview (${preview.filename})`}
              value={
                preview.segmentCount === 0
                  ? "No transcript segments"
                  : `${preview.segmentCount} segment${preview.segmentCount === 1 ? "" : "s"}`
              }
              detail={
                preview.segmentCount === 0
                  ? "this note was typed, not recorded"
                  : undefined
              }
            />
            <p className="muted">
              This is the exported transcript, not the note body shown above.
            </p>
            <pre className="preview">{preview.content}</pre>
          </>
        ) : null}
      </section>
    </div>
  );
}
