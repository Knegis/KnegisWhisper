/**
 * What Trash says (KNEG-401): how much space its recordings take, a trashed
 * note's line in the list, and the two questions asked before anything is
 * deleted for good.
 *
 * Delete moves a note to Trash and asks nothing -- Undo is beside the line
 * that says so -- so the only questions are Delete forever and Empty trash.
 * They name the note and its recording by what a person knows them by: the
 * title, how long the recording runs, how much space they take. Never a
 * stored file name: that is the app's own name for a copy, and means nothing
 * to anyone else. A recording made from an imported file was always a copy,
 * so the question says the original isn't touched.
 *
 * Pure, with no React: `tests/trash.test.ts` imports this directly.
 */
import type { TrashedNote, TrashSummary } from "../generated/ipc-types.js";
import { recentMeta } from "./meetingView.js";

/**
 * Bytes as a person reads them, in the decimal units a disk is sold in:
 * "412 MB", "3.8 MB", "1.3 GB", "640 KB". Anything at all is at least 1 KB.
 */
export function trashSize(bytes: number): string {
  if (bytes >= 1_000_000_000) return `${(bytes / 1_000_000_000).toFixed(1)} GB`;
  if (bytes >= 10_000_000) return `${Math.round(bytes / 1_000_000)} MB`;
  if (bytes >= 1_000_000) return `${(bytes / 1_000_000).toFixed(1)} MB`;
  return `${bytes === 0 ? 0 : Math.max(1, Math.round(bytes / 1000))} KB`;
}

/** A recording's length as the library's rows give it: whole minutes, at least one. */
export function recordingLength(seconds: number): string {
  return recentMeta(null, seconds, new Date());
}

/**
 * A trashed note's line in the Trash list: "Deleted 3 Oct · 31 min", as the
 * library's rows say "26 Aug · 31 min". The length is the recording's, which is
 * what goes when Trash is emptied; a note with none says only when it went.
 */
export function trashedMeta(note: TrashedNote, now: Date): string {
  const date = recentMeta(note.trashedAt, null, now);
  return [
    date === "" ? "Deleted" : `Deleted ${date}`,
    note.recordingSeconds === null ? null : recordingLength(note.recordingSeconds),
  ]
    .filter((part) => part !== null)
    .join(" · ");
}

/** The line above Empty trash: what the recordings in Trash take on this computer. */
export function trashSpaceLine(summary: TrashSummary): string {
  if (summary.notes.length === 0) return "Trash is empty.";
  if (!summary.notes.some((note) => note.hasRecording)) return "Nothing in Trash has a recording.";
  return `Recordings in Trash take ${trashSize(summary.recordingBytes)} on this computer.`;
}

const CANNOT_UNDO = "This can't be undone.";

/** Delete forever's question, for one note in Trash. */
export function deleteForeverQuestion(note: TrashedNote): string {
  const parts = [`Delete “${note.title}” forever?`];
  if (note.hasRecording) {
    parts.push(note.recordingSeconds === null
      ? "Its recording is deleted with it."
      : `Its recording (${recordingLength(note.recordingSeconds)}) is deleted with it.`);
    if (note.originalFilename !== null) parts.push("Your original file isn't touched.");
  }
  parts.push(CANNOT_UNDO);
  return parts.join(" ");
}

/** Empty trash's question: how many notes, and what their recordings take. */
export function emptyTrashQuestion(summary: TrashSummary): string {
  const count = summary.notes.length;
  const recorded = summary.notes.filter((note) => note.hasRecording);
  const parts = [`Delete ${count} ${count === 1 ? "note" : "notes"} forever?`];
  if (recorded.length > 0) {
    // What the files measured; a recording whose file is already gone takes nothing.
    const size = summary.recordingBytes > 0 ? ` (${trashSize(summary.recordingBytes)})` : "";
    parts.push(recorded.length === 1
      ? `Its recording${size} is deleted with it.`
      : `Their recordings${size} are deleted with them.`);
    if (recorded.some((note) => note.originalFilename !== null)) {
      parts.push(recorded.length === 1 ? "Your original file isn't touched." : "Your original files aren't touched.");
    }
  }
  parts.push(CANNOT_UNDO);
  return parts.join(" ");
}

/** What Delete says, beside its Undo. */
export function movedToTrash(title: string): string {
  return `Moved “${title}” to Trash.`;
}
