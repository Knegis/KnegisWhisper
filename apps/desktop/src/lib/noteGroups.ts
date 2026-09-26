/**
 * The notes sidebar's grouping: one group per folder, and the unfiled notes.
 * The view decides the order on screen (folders first, since 2026-09-26).
 *
 * Pure so the order can be tested without a screen. Both inputs keep the
 * order they arrive in — notes as the list command returns them (most
 * recently updated first), folders as the folder command returns them — and
 * a folder with nothing in it is still a group, or a folder someone has just
 * created would be invisible. A note whose folder no longer exists is
 * unfiled rather than lost; the database sets the reference to null when a
 * folder is deleted, so this is the same answer one reload later.
 */
import type { Folder, Note } from "../generated/ipc-types.js";

export type FolderGroup = { folder: Folder; notes: Note[] };
export type NoteGroups = { unfiled: Note[]; folders: FolderGroup[] };

export function groupNotesByFolder(notes: readonly Note[], folders: readonly Folder[]): NoteGroups {
  const byFolder = new Map<number, Note[]>(folders.map((folder) => [folder.id, []]));
  const unfiled: Note[] = [];
  for (const note of notes) {
    const group = note.folderId === null ? undefined : byFolder.get(note.folderId);
    if (group) group.push(note);
    else unfiled.push(note);
  }
  return {
    unfiled,
    folders: folders.map((folder) => ({ folder, notes: byFolder.get(folder.id) ?? [] })),
  };
}
