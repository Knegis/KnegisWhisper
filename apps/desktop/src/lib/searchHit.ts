import type { SearchHit } from "../generated/ipc-types.js";
import { formatClock } from "./clock.js";

/**
 * What a search hit says under its title: the snippet, and before it when
 * the words were said when the hit is in a meeting transcript's row
 * ("12:40 · …"), on the player's clock (KNEG-406). A row with no time, or no
 * row, is the snippet alone; 0:00 would be a time nobody said it at.
 */
export function hitMeta(hit: SearchHit): string {
  const start = hit.transcriptRow?.startSeconds;
  return start === null || start === undefined ? hit.snippet : `${formatClock(start)} · ${hit.snippet}`;
}
