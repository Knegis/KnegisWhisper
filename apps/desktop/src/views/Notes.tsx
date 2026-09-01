/**
 * Notes > Export.
 *
 * Two states that look alike and are not: `hits === null` means nobody has
 * searched, and `hits === []` means somebody searched and nothing matched.
 * Collapsing them would show the full note list as though it were the result.
 */

import { useCallback, useState } from "react";

import { Row } from "../components/Row";
import {
  createFolder,
  deleteFolder,
  deleteNote,
  exportNote,
  exportNoteToFile,
  EXPORT_FORMATS,
  getFolders,
  getNotes,
  searchNotes,
  type ExportedTranscript,
  type ExportFormat,
  type SearchHit,
} from "../lib/ipc";
import { useRefresh } from "../lib/useRefresh";

export function Notes() {
  const [folderFilter, setFolderFilter] = useState<number | null>(null);

  const load = useCallback(async () => {
    const [notes, folders] = await Promise.all([
      getNotes(folderFilter === null ? {} : { folderId: folderFilter }),
      getFolders(),
    ]);
    return { notes, folders };
  }, [folderFilter]);

  const { data, error, guard } = useRefresh(load);

  const [query, setQuery] = useState("");
  const [hits, setHits] = useState<SearchHit[] | null>(null);
  const [exportFormat, setExportFormat] = useState<ExportFormat>("txt");
  const [preview, setPreview] = useState<ExportedTranscript | null>(null);
  const [exportedTo, setExportedTo] = useState<string | null>(null);
  const [openNoteId, setOpenNoteId] = useState<number | null>(null);

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
                    setOpenNoteId((current) =>
                      current === note.id ? null : note.id,
                    );
                  }}
                >
                  {openNoteId === note.id ? "Close note" : "Read note"}
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
                    void guard(async () => {
                      await deleteNote(note.id);
                      setOpenNoteId((current) =>
                        current === note.id ? null : current,
                      );
                      setPreview(null);
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
          <article
            className="note-detail"
            aria-labelledby={`note-title-${openNote.id}`}
          >
            <header className="note-detail-header">
              <h3 id={`note-title-${openNote.id}`}>{openNote.title}</h3>
              <button
                type="button"
                onClick={() => {
                  setOpenNoteId(null);
                }}
                aria-label={`Close note ${openNote.title}`}
              >
                Close
              </button>
            </header>
            <div id={`note-body-${openNote.id}`} className="note-body">
              {openNote.content.trim() === "" ? (
                <span className="muted">This note has no body text.</span>
              ) : (
                openNote.content
              )}
            </div>
          </article>
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
