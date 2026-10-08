import type { NoteListItem } from "./ipc.js";
import { readParticipants, trimSpace } from "./people.js";

export function noteHasPerson(note: NoteListItem, email: string): boolean {
  return readParticipants(note.participants).some(person => person.email !== null && trimSpace(person.email).replace(/[A-Z]/g, letter => letter.toLowerCase()) === email);
}

/** Replace committed metadata and keep the backend's newest-first cap. */
export function acknowledgeNote(notes: readonly NoteListItem[], saved: NoteListItem | null, id: number, limit: number): NoteListItem[] {
  const result = notes.filter(note => note.id !== id);
  if (saved && saved.trashedAt === null) result.push(saved);
  return result.sort((left, right) => (right.updatedAt ?? "").localeCompare(left.updatedAt ?? "") || right.id - left.id).slice(0, limit);
}
