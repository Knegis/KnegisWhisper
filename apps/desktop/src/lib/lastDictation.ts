/**
 * Which dictation Dictation's control bar shows as "Last dictation".
 *
 * Pure, and with no React: `tests/lastDictation.test.ts` imports this
 * directly, and `views/Dictation.tsx` only draws what it returns.
 *
 * Two sources, because either can be missing. This session's outcome
 * events say what was just inserted, whether or not history is on -- with it
 * off nothing is written, so the event is the only record there is. The
 * history rows say what was saved, including a dictation whose paste failed:
 * `fail` in `crates/app-core/src/dictation.rs` stores the cleaned text on the
 * row so it can be copied out rather than lost, but the `failed` event itself
 * carries no text.
 *
 * The insert is kept apart from the last outcome on purpose. A Discard or a
 * failure is an outcome too, and when "Last dictation" was read off the last
 * outcome, one of those after an insert with history off left the bar saying
 * "Nothing dictated yet" -- or, with rows saved before history went off,
 * showing one of those instead.
 */

import type { Transcription } from "../generated/ipc-types.js";
import { parseStoredTime } from "./dayGroups.js";

/** This session's last `inserted` outcome. */
export type InsertedDictation = {
  text: string;
  /** The row it wrote, or `null` when history was off. */
  historyId: number | null;
  /** When this page heard the event. */
  at: Date;
};

/** The last outcome, when it was `failed`. */
export type FailedDictation = { historyId: number | null; at: Date };

/** The dictation the control bar shows, wherever it came from. */
export type LastDictation = {
  text: string;
  at: Date | null;
  /** The text before cleanup, when the row kept one. */
  raw: string | null;
  /** Whether it was typed where the cursor was; `false` when the paste failed. */
  typed: boolean;
};

type Row = Pick<Transcription, "id" | "text" | "rawText" | "status" | "createdAt">;

/**
 * In this order:
 *
 *   - the last outcome, when it failed and its row kept the text -- nothing
 *     is newer than the last outcome;
 *   - otherwise the newer of this session's insert and the newest row with
 *     text. Row ids only grow, so a row after the insert's own row is newer;
 *     an insert that wrote no row is compared by time instead, and wins when
 *     the row's time cannot be read, since the event is known to be recent;
 *   - otherwise whichever of the two exists.
 *
 * `failed` is `null` unless the last outcome was a failure. A row, not
 * `history[0]`: a dictation that failed before anything came back writes a
 * row with no text, and "Nothing dictated yet" over an earlier one would be
 * false.
 */
export function lastDictation(
  history: readonly Row[],
  inserted: InsertedDictation | null,
  failed: FailedDictation | null,
): LastDictation | null {
  const byId = (id: number | null) => (id === null ? null : (history.find((h) => h.id === id) ?? null));
  const failedRow = failed === null ? null : byId(failed.historyId);
  if (failed !== null && failedRow !== null && failedRow.text !== "") {
    return { text: failedRow.text, at: failed.at, raw: failedRow.rawText, typed: false };
  }
  const newest = history.find((h) => h.text !== "") ?? null;
  const fromRow = (h: Row): LastDictation => ({
    text: h.text,
    at: parseStoredTime(h.createdAt),
    raw: h.rawText,
    typed: h.status !== "failed",
  });
  if (inserted === null) return newest === null ? null : fromRow(newest);
  const fromInsert: LastDictation = {
    text: inserted.text,
    at: inserted.at,
    raw: byId(inserted.historyId)?.rawText ?? null,
    typed: true,
  };
  if (newest === null || newest.id === inserted.historyId) return fromInsert;
  const rowIsNewer =
    inserted.historyId !== null
      ? newest.id > inserted.historyId
      : (parseStoredTime(newest.createdAt)?.getTime() ?? -Infinity) > inserted.at.getTime();
  return rowIsNewer ? fromRow(newest) : fromInsert;
}
