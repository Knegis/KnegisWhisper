/**
 * What the window says, on any screen, about a meeting that was cut short.
 *
 * A meeting the application never saw end -- it crashed, or was closed, while
 * the meeting ran or was being saved -- has no note. Its recording and typed
 * notes are still on this computer, offered under Recent on the Meetings page,
 * and until this existed that was the only place that said so: somebody who
 * opened Notes found no meeting and took it for deleted.
 *
 * Only `unfinished` recordings are raised here. A recording left with no note
 * on purpose -- a quiet room keeps its recording -- is offered on Meetings as
 * it always was, and would be a notice at every launch if it counted.
 *
 * Pure -- no React -- so it is tested (`tests/interruptedNotice.test.ts`).
 */
import type { RecoverableRecording } from "./ipc.js";

/** The sentence for the window's notice, or `null` when no meeting was cut short. */
export function interruptedNotice(recordings: readonly RecoverableRecording[]): string | null {
  const cut = recordings.filter((recording) => recording.unfinished);
  if (cut.length === 0) return null;
  const notes = cut.some((recording) => recording.hasNotes);
  if (cut.length === 1) {
    return `A meeting was cut short before it was saved. Its recording${notes ? " and typed notes are" : " is"} kept on this computer.`;
  }
  return `${cut.length} meetings were cut short before they were saved. Their recordings${notes ? " and typed notes" : ""} are kept on this computer.`;
}

/** What an interrupted recording's row says under its name, on the Meetings page. */
/**
 * The one line Recent gives its recordings with no note (KNEG-575). They were
 * a card each, ahead of every saved meeting: twenty-four cards, on the profile
 * this was measured on, before the first meeting.
 */
export function unfinishedSummary(count: number): string {
  return count === 1 ? "1 unfinished recording" : `${count} unfinished recordings`;
}

/**
 * Whether the list under that line starts open. A few are shown at once, as
 * they were; more than that wait behind Review, so the saved meetings below
 * are not pushed off the page.
 */
export const UNFINISHED_SHOWN_OPEN = 3;

/** What deleting asks first, for one recording or for all of them. */
export function deleteUnfinishedQuestion(count: number, anyNotes: boolean): string {
  const what = count === 1 ? "this recording" : `these ${count} recordings`;
  const notes = anyNotes ? (count === 1 ? " and the notes typed during it" : " and any notes typed during them") : "";
  return `Delete ${what}${notes}? ${count === 1 ? "It was" : "They were"} never made into a note, and this can't be undone.`;
}

export function interruptedMeta(recording: RecoverableRecording): string {
  return `Not a note yet · recording kept${recording.hasNotes ? " · typed notes kept" : ""}`;
}
