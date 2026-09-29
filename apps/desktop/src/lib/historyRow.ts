/**
 * What one dictation history row says: its title, the line under it, and the
 * tooltip that carries the rest.
 *
 * Pure, and with no React: `tests/historyRow.test.ts` imports this directly,
 * and `components/DictationHistoryPanel.tsx` only draws it.
 *
 * # Failed is not the same as empty
 *
 * Rust stores `status` as `completed` or `failed` and nothing else
 * (`record`, `src-tauri/src/dictation.rs`). A failed row can still carry the
 * cleaned text: when only the paste failed, `fail` writes the transcript on
 * purpose "so the user can still copy it out of the history rather than
 * losing what they said" (`crates/app-core/src/dictation.rs`, the `insert`
 * error arm). So the title is the text whenever there is text, and "Didn't
 * work" only when there is none; the failure's reason goes in the meta line
 * either way, where it stays visible.
 *
 * # What moved into the tooltip
 *
 * The list this panel replaced showed each row's provider and, when kept, the
 * text before cleanup. Both are history and mechanism, so they sit in the
 * row's tooltip rather than on it -- `components/Tooltip.tsx` draws it on
 * hover and straight away on keyboard focus -- and a row that has a
 * pre-cleanup text says " · Cleaned up" in its meta, so which rows carry one
 * is visible without pointing at each.
 */

import { destinationName } from "./controlBar.js";
import { parseStoredTime, timeOfDay } from "./dayGroups.js";
import type { Transcription } from "./ipc.js";

export type HistoryRow = {
  title: string;
  /** Muted when the row has no text to show. */
  muted: boolean;
  /** `HH:MM`, then the failure's reason, then "Cleaned up" -- each when present. */
  meta: string;
  /** The row's full text and what moved off the row, or `null` when there is nothing. */
  tooltip: string | null;
  /** What pressing the row puts on the clipboard: the text, or failing that the reason. */
  copyText: string;
  /** Whether the text was typed where the cursor was. `false` for a failed row. */
  typed: boolean;
};

export function historyRow(h: Transcription): HistoryRow {
  const at = parseStoredTime(h.createdAt);
  const failed = h.status === "failed";
  const empty = h.text === "";
  const cleaned = h.rawText !== null && h.rawText !== "";
  const meta = [
    at === null ? null : timeOfDay(at),
    failed ? h.errorMessage : null,
    cleaned ? "Cleaned up" : null,
  ]
    .filter((part): part is string => part !== null && part !== "")
    .join(" · ");
  const tooltip = [
    empty ? null : h.text,
    cleaned ? `Before cleanup: ${h.rawText}` : null,
    h.provider ? `Transcribed by ${destinationName(h.provider)}${h.model ? ` (${h.model})` : ""}` : null,
  ]
    .filter((line): line is string => line !== null)
    .join("\n");
  return {
    title: empty ? "Didn't work" : h.text,
    muted: empty,
    meta,
    tooltip: tooltip === "" ? null : tooltip,
    copyText: h.text || h.errorMessage || "",
    typed: !failed,
  };
}
