import { type Note, type TrashedNote as TrashEntry } from "../lib/ipc";
import { trashedMeta } from "../lib/trash";

/**
 * A note in Trash, read-only (KNEG-401): "This note is in Trash." with
 * Restore and Delete forever, its title, when it was deleted, and its typed
 * notes as plain text.
 *
 * Not a `NoteWorkspace`, on purpose. A workspace autosaves, so a note opened
 * in one -- from the Trash list, or by Back to where it was open before it was
 * deleted -- would be written to while it is meant to be put away, and Rust
 * would refuse the write with an error on screen. Nothing here can change the
 * note: what it says is only enough to tell which note it is before restoring
 * it or deleting it for good. Restore brings it back with everything,
 * transcript and recording included.
 *
 * `entry` is the note's line in Trash, for when it went and how long its
 * recording runs; while Trash is being read again it may not be there yet, and
 * the note is shown without that line.
 */
export function TrashedNote({ note, entry, onRestore, onDeleteForever }: {
  note: Note;
  entry: TrashEntry | null;
  onRestore: () => void;
  onDeleteForever: () => void;
}) {
  const content = note.content.trim();
  return (
    <article className="trashed-note" aria-label={note.title}>
      <div className="note-page">
        <div className="trashed-note-banner">
          <p>This note is in Trash.</p>
          <div className="trashed-note-actions">
            <button type="button" className="primary" onClick={onRestore}>Restore</button>
            <button type="button" onClick={onDeleteForever}>Delete forever</button>
          </div>
        </div>
        <header className="trashed-note-header">
          <h2 className="trashed-note-title">{note.title}</h2>
          {entry === null ? null : <p className="note-meta">{trashedMeta(entry, new Date())}</p>}
        </header>
        <h3 className="note-section-heading">Your notes</h3>
        {content === "" ? <p className="muted">No notes were typed.</p> : <div className="trashed-note-text">{note.content}</div>}
      </div>
    </article>
  );
}
