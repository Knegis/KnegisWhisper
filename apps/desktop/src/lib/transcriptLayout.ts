import type { TranscriptRow } from "../generated/ipc-types.js";
import { formatClock } from "./clock.js";

/**
 * How a note's transcript is laid out in the column (KNEG-448): rows grouped
 * into time sections, and within a section into speaker turns. Pure and
 * node-testable so the component only renders what this returns. Rows are
 * never reordered: `index` is the row's place in the stored transcript, and
 * every key below derives from it (or from the block start), never from a
 * speaker's display name, so renaming a speaker does not reset what the
 * reader has collapsed.
 */

/** A pause this long between rows starts a new turn even for the same speaker. */
export const TURN_GAP_SECONDS = 60;

export type TranscriptTurn = {
  /** `t${firstRow.index}`: stable across a speaker rename. */
  key: string;
  speaker: string | null;
  speakerId: string | null;
  /** The first timed row's start in the turn; null when none is timed. */
  startSeconds: number | null;
  rows: readonly TranscriptRow[];
};

export type TranscriptSection = {
  /** `s${blockStart}` in seconds. */
  key: string;
  startSeconds: number | null;
  endSeconds: number | null;
  /** "5:00 – 10:00"; empty for the single section of an untimed transcript. */
  title: string;
  /** Every row in the section, untimed ones included. */
  lineCount: number;
  turns: readonly TranscriptTurn[];
};

/** How far the transcript reaches: the largest `endSeconds ?? startSeconds ?? 0`. */
export function transcriptExtent(rows: readonly TranscriptRow[]): number {
  let extent = 0;
  for (const row of rows) {
    extent = Math.max(extent, row.endSeconds ?? row.startSeconds ?? 0);
  }
  return extent;
}

/** Section length by extent, so a long recording does not become dozens of headers. */
export function blockSecondsFor(extent: number): number {
  if (extent <= 3600) return 300;
  if (extent <= 10800) return 600;
  return 900;
}

/** The speaker key two rows must share to be one turn; null never groups. */
function turnKey(row: TranscriptRow): string | null {
  // Ids and names are different namespaces: a name that happens to equal an
  // id is not the same speaker.
  if (row.speakerId !== null) return `id:${row.speakerId}`;
  if (row.speaker !== null) return `name:${row.speaker}`;
  return null;
}

type TurnDraft = { key: string | null; first: TranscriptRow; rows: TranscriptRow[] };

function toTurn(draft: TurnDraft): TranscriptTurn {
  const firstTimed = draft.rows.find((row) => row.startSeconds !== null);
  return {
    key: `t${draft.first.index}`,
    speaker: draft.first.speaker,
    speakerId: draft.first.speakerId,
    startSeconds: firstTimed?.startSeconds ?? null,
    rows: draft.rows,
  };
}

/** One section's rows split into speaker turns. */
function turnsOf(rows: readonly TranscriptRow[]): TranscriptTurn[] {
  const turns: TurnDraft[] = [];
  let current: TurnDraft | null = null;
  // Where the last timed row left off, for the pause check.
  let lastEnd: number | null = null;
  for (const row of rows) {
    const key = turnKey(row);
    let joins = false;
    if (current !== null) {
      if (row.startSeconds === null) {
        // Nothing to split an untimed row on: it stays with the row before.
        joins = true;
      } else {
        const paused = lastEnd !== null && row.startSeconds - lastEnd >= TURN_GAP_SECONDS;
        joins = key !== null && key === current.key && !paused;
      }
    }
    if (joins && current !== null) {
      current.rows.push(row);
    } else {
      current = { key, first: row, rows: [row] };
      turns.push(current);
    }
    if (row.startSeconds !== null) lastEnd = row.endSeconds ?? row.startSeconds;
  }
  return turns.map(toTurn);
}

/**
 * Group rows into time sections of `blockSecondsFor(extent)` seconds, each
 * split into speaker turns. A row's block is `floor(start / block)` but never
 * earlier than the block already reached, so an out-of-order row stays where
 * it is rather than reopening an earlier section; blocks with no rows make no
 * section. Untimed rows follow the row before them (leading ones join the
 * first section). An all-untimed transcript is one titleless section.
 */
export function transcriptSections(rows: readonly TranscriptRow[]): TranscriptSection[] {
  if (rows.length === 0) return [];
  const extent = transcriptExtent(rows);
  const block = blockSecondsFor(extent);

  const firstTimed = rows.find((row) => row.startSeconds !== null);
  if (firstTimed === undefined) {
    return [
      {
        key: "s0",
        startSeconds: null,
        endSeconds: null,
        title: "",
        lineCount: rows.length,
        turns: turnsOf(rows),
      },
    ];
  }

  // Walk once, assigning each row to a block index that never goes backwards.
  let current = Math.floor((firstTimed.startSeconds ?? 0) / block);
  const groups: { blockIndex: number; rows: TranscriptRow[] }[] = [];
  for (const row of rows) {
    if (row.startSeconds !== null) current = Math.max(current, Math.floor(row.startSeconds / block));
    const last = groups[groups.length - 1];
    if (last !== undefined && last.blockIndex === current) {
      last.rows.push(row);
    } else {
      // (Leading untimed rows land in the first section's block, `current`.)
      groups.push({ blockIndex: current, rows: [row] });
    }
  }

  return groups.map((group, i) => {
    const start = group.blockIndex * block;
    const end = i === groups.length - 1 ? extent : start + block;
    return {
      key: `s${start}`,
      startSeconds: start,
      endSeconds: end,
      title: `${formatClock(start)} – ${formatClock(end)}`,
      lineCount: group.rows.length,
      turns: turnsOf(group.rows),
    };
  });
}

/** Where the section ticks on the player's scrubber go: every start after the first. */
export function sectionMarks(sections: readonly TranscriptSection[]): number[] {
  const marks: number[] = [];
  for (const section of sections.slice(1)) {
    if (section.startSeconds !== null) marks.push(section.startSeconds);
  }
  return marks;
}

/** The row playing at `seconds`: the first that has started and not yet ended (null end = to the end). */
export function lineAt(rows: readonly TranscriptRow[], seconds: number): TranscriptRow | null {
  return (
    rows.find(
      (row) =>
        row.startSeconds !== null &&
        row.startSeconds <= seconds &&
        (row.endSeconds === null || seconds < row.endSeconds),
    ) ?? null
  );
}

/**
 * The last row that has started by `seconds`. Unlike `lineAt` it holds through
 * gaps between rows, so the player card keeps naming the last speaker.
 */
export function lastStartedLine(rows: readonly TranscriptRow[], seconds: number): TranscriptRow | null {
  let found: TranscriptRow | null = null;
  for (const row of rows) {
    if (row.startSeconds !== null && row.startSeconds <= seconds) found = row;
  }
  return found;
}

/**
 * Shift-click range: the `index` of every playable row (one with a start)
 * between the anchor and the target, inclusive, in stored order whichever
 * comes first. Unknown anchor or target selects nothing.
 */
export function rangeSelection(rows: readonly TranscriptRow[], anchor: number, target: number): number[] {
  const a = rows.findIndex((row) => row.index === anchor);
  const b = rows.findIndex((row) => row.index === target);
  if (a < 0 || b < 0) return [];
  const [from, to] = a <= b ? [a, b] : [b, a];
  return rows
    .slice(from, to + 1)
    .filter((row) => row.startSeconds !== null)
    .map((row) => row.index);
}

/**
 * The interval a selection plays: first selected playable row's start to the
 * last one's end (the recording's duration when that row runs to the end).
 * Under two playable lines there is nothing to offer, so null.
 */
export function selectionInterval(
  rows: readonly TranscriptRow[],
  selected: ReadonlySet<number>,
  duration: number | null,
): { start: number; end: number | null; label: string } | null {
  const picked = rows.filter((row) => row.startSeconds !== null && selected.has(row.index));
  const first = picked[0];
  const last = picked[picked.length - 1];
  if (picked.length < 2 || first === undefined || last === undefined || first.startSeconds === null) return null;
  const start = first.startSeconds;
  const end = last.endSeconds ?? duration;
  return { start, end, label: `${formatClock(start)}–${formatClock(end ?? start)}` };
}
