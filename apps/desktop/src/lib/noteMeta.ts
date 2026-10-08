/**
 * What a note's header says about it besides its title: the folder its
 * breadcrumb names, and the date, length and where it was transcribed in its
 * meta row. The people beside them are `NotePeople`'s, read by `lib/people.ts`
 * (KNEG-362).
 *
 * Pure, and with no React: `tests/noteMeta.test.ts` imports this directly.
 */
import type { Folder, Note } from "../generated/ipc-types.js";
import { parseStoredTime } from "./dayGroups.js";

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

/**
 * The meta row's line for where a note's transcript was made, from its
 * stored `transcribedBy` -- Rust's `TranscribedBy` as JSON (KNEG-368): "on
 * this computer", naming a meeting's live-caption provider beside it when the
 * captions came from elsewhere, or "by" whoever made it.
 *
 * Read leniently, as `lib/people.ts` reads participants: a record that is
 * missing -- every note saved before it was kept -- or that will not read
 * says nothing, rather than a guess or an error in the header.
 */
export function transcribedLine(json: string | null): string | null {
  if (json === null) return null;
  let parsed: unknown;
  try {
    parsed = JSON.parse(json);
  } catch {
    return null;
  }
  if (typeof parsed !== "object" || parsed === null || Array.isArray(parsed)) return null;
  const { name, onThisComputer, liveName } = parsed as { name?: unknown; onThisComputer?: unknown; liveName?: unknown };
  if (typeof name !== "string" || name.trim() === "" || typeof onThisComputer !== "boolean") return null;
  if (!onThisComputer) return `Transcribed by ${name}`;
  return typeof liveName === "string" && liveName.trim() !== ""
    ? `Transcribed on this computer · live captions by ${liveName}`
    : "Transcribed on this computer";
}

/**
 * The header's facts about `note`. Each is `null` when the note does not have
 * it, and the header then leaves that part out rather than printing a
 * placeholder.
 *
 * - `folder` is the name of the folder `folderId` points at. A folder that is
 *   not in `folders` -- deleted since the list was read -- is `null`, the
 *   same answer `groupNotesByFolder` gives by listing the note as unfiled.
 * - `date` is the creation time, read by `parseStoredTime` as the list's
 *   tooltip reads it.
 * - `minutes` is the recording's length rounded to whole minutes.
 * - `transcribed` is `transcribedLine`'s line for where it was transcribed.
 */
export function noteMeta(
  note: Note,
  folders: readonly Folder[],
): { folder: string | null; date: Date | null; minutes: number | null; transcribed: string | null } {
  const folder = note.folderId === null ? null : (folders.find((item) => item.id === note.folderId)?.name ?? null);
  return {
    folder,
    date: parseStoredTime(note.createdAt),
    minutes: note.audioDurationSeconds === null ? null : Math.round(note.audioDurationSeconds / 60),
    transcribed: transcribedLine(note.transcribedBy),
  };
}
