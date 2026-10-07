/**
 * What a note's header says about it besides its title: the folder its
 * breadcrumb names, and the date, length and participants in its meta row.
 *
 * Pure, and with no React: `tests/noteMeta.test.ts` imports this directly.
 */
import type { Folder, Note } from "../generated/ipc-types.js";
import { parseStoredTime } from "./dayGroups.js";

/**
 * The names in a note's stored `participants`, a JSON array of
 * `{displayName, email}`: the display name where there is one, the email
 * where there is not.
 *
 * The same reading as `notes_core::transcript::participants`, which puts
 * these names in an export's header, so the note on screen and the file
 * written from it name the same people. That includes its two refusals: a
 * value that does not parse is no participants rather than an error -- it is
 * not worth failing a header over -- and a name that is blank is dropped
 * rather than shown as an empty entry.
 */
/**
 * Whether a note's stored transcript (`Note.transcript`, a JSON array of
 * segments) has a row.
 *
 * For the wide layout's transcript column, which a note without one used to
 * get anyway, saying "This note has no transcript." (KNEG-377). Read from the
 * note rather than from the rows `note_transcript_rows` loads, so the layout
 * is right on the first frame instead of changing when they arrive.
 */
export function hasStoredTranscript(json: string | null): boolean {
  if (json === null || json.trim() === "") return false;
  try {
    const parsed: unknown = JSON.parse(json);
    return Array.isArray(parsed) && parsed.length > 0;
  } catch {
    return false;
  }
}

export function parseParticipants(json: string | null): string[] {
  if (json === null) return [];
  let parsed: unknown;
  try {
    parsed = JSON.parse(json);
  } catch {
    return [];
  }
  if (!Array.isArray(parsed)) return [];
  const names: string[] = [];
  for (const entry of parsed) {
    if (typeof entry !== "object" || entry === null) continue;
    const { displayName, email } = entry as { displayName?: unknown; email?: unknown };
    const name = typeof displayName === "string" ? displayName : typeof email === "string" ? email : null;
    if (name !== null && name.trim() !== "") names.push(name);
  }
  return names;
}

/**
 * The header's facts about `note`. Each is `null` (or, for participants,
 * empty) when the note does not have it, and the header then leaves that part
 * out rather than printing a placeholder.
 *
 * - `folder` is the name of the folder `folderId` points at. A folder that is
 *   not in `folders` -- deleted since the list was read -- is `null`, the
 *   same answer `groupNotesByFolder` gives by listing the note as unfiled.
 * - `date` is the creation time, read by `parseStoredTime` as the list's
 *   tooltip reads it.
 * - `minutes` is the recording's length rounded to whole minutes.
 */
export function noteMeta(
  note: Note,
  folders: readonly Folder[],
): { folder: string | null; date: Date | null; minutes: number | null; participants: string[] } {
  const folder = note.folderId === null ? null : (folders.find((item) => item.id === note.folderId)?.name ?? null);
  return {
    folder,
    date: parseStoredTime(note.createdAt),
    minutes: note.audioDurationSeconds === null ? null : Math.round(note.audioDurationSeconds / 60),
    participants: parseParticipants(note.participants),
  };
}
