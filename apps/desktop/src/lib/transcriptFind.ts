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

/**
 * Whether two lists hold the same matches, at the same places and with the same ordinals: a new
 * query that finds what the last one found (`"MÖTE"` after `"möte"`, a space typed after a word)
 * can keep the last one's list, and with it the current match.
 */
export function sameMatches(a: readonly FindMatch[], b: readonly FindMatch[]): boolean {
  return a === b || (a.length === b.length && a.every((m, i) => {
    const n = b[i]!;
    return m.row === n.row && m.start === n.start && m.end === n.end && m.ordinal === n.ordinal;
  }));
}

/**
 * `matchesByRow(matches)` for a new query's matches, keeping from `previous` -- what the lines
 * are drawn with now -- each row's array whose matches are the same, and `previous` itself when
 * every row's are (KNEG-448). A memoised line is redrawn when its array is a new one, so a query
 * that finds what the last one found leaves its lines alone: "MÖTE" after "möte" redraws
 * nothing, and a letter more redraws only the lines whose matches it changed.
 */
export function regroupMatches(
  previous: ReadonlyMap<number, readonly FindMatch[]>,
  matches: readonly FindMatch[],
): ReadonlyMap<number, readonly FindMatch[]> {
  const next = new Map<number, readonly FindMatch[]>();
  let kept = true;
  for (const [row, list] of matchesByRow(matches)) {
    const before = previous.get(row);
    if (before !== undefined && sameMatches(before, list)) {
      next.set(row, before);
    } else {
      next.set(row, list);
      kept = false;
    }
  }
  // Every row kept and as many rows: the same rows, so the same map.
  return kept && next.size === previous.size ? previous : next;
}

/**
 * Whether `a` and `b` give each of `rows` the same matches -- the same array, as
 * `regroupMatches` keeps it -- whatever they give other rows: a turn or a section is redrawn
 * for a new query only when one of its own lines' matches changed.
 */
export function sameMatchesOver(
  rows: readonly { index: number }[],
  a: ReadonlyMap<number, readonly FindMatch[]>,
  b: ReadonlyMap<number, readonly FindMatch[]>,
): boolean {
  if (a === b) return true;
  return rows.every((row) => a.get(row.index) === b.get(row.index));
}

/**
 * The first of `matches` that `test` passes, for a test that, once it passes, passes every later
 * match too -- "drawn below the top of the view", "at or after this place". Found by halving,
 * so a test that reads the page is asked about log n times, not once per match. `null` when no
 * match passes.
 */
export function firstMatchWhere(
  matches: readonly FindMatch[],
  test: (match: FindMatch) => boolean,
): number | null {
  let low = 0;
  let high = matches.length;
  while (low < high) {
    const middle = (low + high) >> 1;
    if (test(matches[middle]!)) high = middle;
    else low = middle + 1;
  }
  return low < matches.length ? low : null;
}

/** Each row's position in `rows`, by its `index`: the order the transcript reads in. */
export function rowOrder(rows: readonly { index: number }[]): ReadonlyMap<number, number> {
  return new Map(rows.map((row, position) => [row.index, position]));
}

/**
 * Where a new query starts (KNEG-448): the first of `matches` at or after `from` -- the match
 * that was current, a row's index and an offset in its text -- in the order `order` gives the
 * rows (`rowOrder`); the first match when none is that far on, or when `from`'s row is not in
 * the order. `null` when there are no matches. Narrowing or widening a query so keeps to the
 * place the reader was at, rather than going back to an earlier line.
 */
export function matchFrom(
  matches: readonly FindMatch[],
  order: ReadonlyMap<number, number>,
  from: { row: number; start: number },
): number | null {
  if (matches.length === 0) return null;
  const at = order.get(from.row);
  if (at === undefined) return 0;
  const reached = firstMatchWhere(matches, (match) => {
    const position = order.get(match.row) ?? Number.POSITIVE_INFINITY;
    return position > at || (position === at && match.start >= from.start);
  });
  return reached ?? 0;
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
