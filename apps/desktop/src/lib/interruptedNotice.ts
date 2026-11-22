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
export function interruptedMeta(recording: RecoverableRecording): string {
  return `Not a note yet · recording kept${recording.hasNotes ? " · typed notes kept" : ""}`;
}
