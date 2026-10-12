/**
 * Find in a transcript (KNEG-448): which rows contain the query, where, and
 * how to step between the hits.
 *
 * It behaves like the app's database search (KNEG-406, FTS5 `unicode61
 * remove_diacritics 0`) where it matters to a Swedish reader: case is
 * ignored, accents are not -- "har" never finds "här", so a search for one
 * word does not drag in another. Unlike FTS it is a plain substring match
 * ("dag" finds "tisdag"), because a person looking at one open transcript
 * expects what Ctrl+F gives them.
 *
 * Case folding can change a string's length ("İ" lowercases to two code
 * units), so matching runs on the folded text and each folded position is
 * mapped back to the code point it came from. Ranges returned are always
 * offsets into the ORIGINAL text, on code point boundaries, so they can be
 * used to slice what is on screen without ever splitting a character.
 */

export type FindMatch = {
  /** The row's `index` field, not its position in the array searched. */
  row: number;
  /** UTF-16 offset into the row's original text. */
  start: number;
  /** Exclusive UTF-16 offset into the row's original text. */
  end: number;
  /** 0-based position of this match across every row searched. */
  ordinal: number;
};

/** NFC, trimmed, every whitespace run collapsed to one space. */
export function normaliseQuery(q: string): string {
  return q.normalize("NFC").trim().replace(/\s+/gu, " ");
}

type Folded = {
  text: string;
  /** For each folded code unit: original offset where its code point starts. */
  origStart: number[];
  /** ... and where it ends (exclusive). */
  origEnd: number[];
  /** ... and the folded offset just past its code point's folded form. */
  foldEnd: number[];
};

function fold(text: string): Folded {
  let out = "";
  const origStart: number[] = [];
  const origEnd: number[] = [];
  const foldEnd: number[] = [];
  let pos = 0;
  for (const ch of text) {
    const lower = ch.toLowerCase();
    const end = pos + ch.length;
    const foldedEnd = out.length + lower.length;
    for (let i = 0; i < lower.length; i++) {
      origStart.push(pos);
      origEnd.push(end);
      foldEnd.push(foldedEnd);
    }
    out += lower;
    pos = end;
  }
  return { text: out, origStart, origEnd, foldEnd };
}

/**
 * Every non-overlapping, case-insensitive occurrence of `query` in `rows`,
 * row by row in array order, left to right within a row.
 */
export function findMatches(
  rows: readonly { index: number; text: string }[],
  query: string,
): FindMatch[] {
  const needle = fold(normaliseQuery(query)).text;
  if (needle.length === 0) return [];
  const matches: FindMatch[] = [];
  for (const row of rows) {
    const hay = fold(row.text);
    let from = 0;
    while (from <= hay.text.length - needle.length) {
      const at = hay.text.indexOf(needle, from);
      if (at < 0) break;
      const last = at + needle.length - 1;
      matches.push({
        row: row.index,
        start: hay.origStart[at]!,
        end: hay.origEnd[last]!,
        ordinal: matches.length,
      });
      // Resume after the whole code point the match ended in, so two
      // matches can never share a character of the original text.
      from = hay.foldEnd[last]!;
    }
  }
  return matches;
}

/** One row's matches, in order, keyed by the row's index. */
export function matchesByRow(
  matches: readonly FindMatch[],
): ReadonlyMap<number, readonly FindMatch[]> {
  const by = new Map<number, FindMatch[]>();
  for (const m of matches) {
    const list = by.get(m.row);
    if (list) list.push(m);
    else by.set(m.row, [m]);
  }
  return by;
}

/** Next or previous match, wrapping; `null` when there is nothing to step to. */
export function stepMatch(current: number | null, count: number, step: 1 | -1): number | null {
  if (count <= 0) return null;
  if (current === null) return step === 1 ? 0 : count - 1;
  return (((current + step) % count) + count) % count;
}

/**
 * Split one row's text into plain and matched parts. `matches` are that row's
 * matches, sorted by `start`. No part is empty and the parts joined give the
 * text back exactly.
 */
export function textParts(
  text: string,
  matches: readonly FindMatch[],
): { text: string; ordinal: number | null }[] {
  const parts: { text: string; ordinal: number | null }[] = [];
  let at = 0;
  for (const m of matches) {
    if (m.start > at) parts.push({ text: text.slice(at, m.start), ordinal: null });
    parts.push({ text: text.slice(m.start, m.end), ordinal: m.ordinal });
    at = m.end;
  }
  if (at < text.length) parts.push({ text: text.slice(at), ordinal: null });
  return parts;
}

/** The counter the find bar shows, and what a screen reader is told. */
export function findStatus(
  current: number | null,
  count: number,
): { shown: string; spoken: string } {
  if (count === 0) return { shown: "0/0", spoken: "No matches" };
  if (current === null) {
    return { shown: `0/${count}`, spoken: count === 1 ? "1 match" : `${count} matches` };
  }
  return { shown: `${current + 1}/${count}`, spoken: `Match ${current + 1} of ${count}` };
}
