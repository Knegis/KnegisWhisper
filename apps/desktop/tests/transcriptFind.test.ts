import test from "node:test";
import assert from "node:assert/strict";
import {
  findMatches,
  findStatus,
  firstMatchWhere,
  matchFrom,
  matchesByRow,
  normaliseQuery,
  regroupMatches,
  rowOrder,
  sameMatches,
  sameMatchesOver,
  stepMatch,
  textParts,
} from "../src/lib/transcriptFind.js";

const rows = (...texts: string[]) => texts.map((text, index) => ({ index, text }));

test("normaliseQuery composes, trims and collapses whitespace", () => {
  assert.equal(normaliseQuery("  a \t\n b  "), "a b");
  assert.equal(normaliseQuery("ä"), "ä");
  assert.equal(normaliseQuery("   "), "");
});

test("case is folded: HÄR finds här", () => {
  const m = findMatches(rows("Jag är här nu"), "HÄR");
  assert.deepEqual(m, [{ row: 0, start: 7, end: 10, ordinal: 0 }]);
});

test("accents are significant: har does not find här, cafe does not find café", () => {
  assert.deepEqual(findMatches(rows("här"), "har"), []);
  assert.deepEqual(findMatches(rows("café"), "cafe"), []);
  assert.equal(findMatches(rows("café"), "CAFÉ").length, 1);
});

test("plain substring: dag finds tisdag", () => {
  assert.deepEqual(findMatches(rows("tisdag"), "dag"), [{ row: 0, start: 3, end: 6, ordinal: 0 }]);
});

test("matches do not overlap: aa in aaaa gives 2", () => {
  const m = findMatches(rows("aaaa"), "aa");
  assert.deepEqual(m.map((x) => [x.start, x.end]), [[0, 2], [2, 4]]);
});

test("a decomposed query finds composed text", () => {
  assert.equal(findMatches(rows("här"), "här").length, 1);
});

test("empty or blank query finds nothing", () => {
  assert.deepEqual(findMatches(rows("abc"), ""), []);
  assert.deepEqual(findMatches(rows("abc"), "  \t "), []);
});

test("whitespace in the query is collapsed", () => {
  assert.equal(findMatches(rows("a b"), "a   b").length, 1);
});

test("expanding case folds stay in bounds (İ)", () => {
  const text = "İstanbul";
  for (const q of ["i", "stanbul", "̇", "i̇"]) {
    for (const x of findMatches(rows(text), q)) {
      assert.ok(0 <= x.start && x.start < x.end && x.end <= text.length, q);
    }
  }
  assert.deepEqual(findMatches(rows(text), "stanbul"), [{ row: 0, start: 1, end: 8, ordinal: 0 }]);
  // The folded form of the whole İ maps back to the whole İ.
  assert.deepEqual(findMatches(rows(text), "i̇stanbul"), [{ row: 0, start: 0, end: 8, ordinal: 0 }]);
});

test("a match that begins inside an expansion widens to the whole code point", () => {
  assert.deepEqual(findMatches(rows("xİy"), "̇"), [{ row: 0, start: 1, end: 2, ordinal: 0 }]);
});

test("ß is matched as itself and stays in bounds", () => {
  assert.deepEqual(findMatches(rows("Straße"), "ß"), [{ row: 0, start: 4, end: 5, ordinal: 0 }]);
  assert.deepEqual(findMatches(rows("Straße"), "STRASSE"), []);
});

test("offsets stay on code point boundaries with astral characters", () => {
  const m = findMatches(rows("\u{1F600}ab\u{1F600}ab"), "ab");
  assert.deepEqual(m.map((x) => [x.start, x.end]), [[2, 4], [6, 8]]);
  assert.deepEqual(findMatches(rows("\u{10400}"), "\u{10428}"), [{ row: 0, start: 0, end: 2, ordinal: 0 }]);
});

test("ordinals run across rows in array order and row is the row's index", () => {
  const r = [
    { index: 7, text: "ja nej ja" },
    { index: 3, text: "inget" },
    { index: 12, text: "JA" },
  ];
  assert.deepEqual(findMatches(r, "ja"), [
    { row: 7, start: 0, end: 2, ordinal: 0 },
    { row: 7, start: 7, end: 9, ordinal: 1 },
    { row: 12, start: 0, end: 2, ordinal: 2 },
  ]);
});

test("matchesByRow groups in order", () => {
  const m = findMatches([{ index: 5, text: "aa" }, { index: 9, text: "a" }], "a");
  const by = matchesByRow(m);
  assert.deepEqual([...by.keys()], [5, 9]);
  assert.deepEqual(by.get(5)?.map((x) => x.ordinal), [0, 1]);
  assert.deepEqual(by.get(9)?.map((x) => x.ordinal), [2]);
  assert.equal(by.get(1), undefined);
});

test("stepMatch wraps both ways and handles empty and unset", () => {
  assert.equal(stepMatch(null, 0, 1), null);
  assert.equal(stepMatch(0, 0, -1), null);
  assert.equal(stepMatch(null, 3, 1), 0);
  assert.equal(stepMatch(null, 3, -1), 2);
  assert.equal(stepMatch(2, 3, 1), 0);
  assert.equal(stepMatch(0, 3, -1), 2);
  assert.equal(stepMatch(1, 3, 1), 2);
  assert.equal(stepMatch(1, 3, -1), 0);
});

test("textParts splits around matches and joins back to the text", () => {
  const text = "ja nej ja!";
  const parts = textParts(text, findMatches(rows(text), "ja"));
  assert.deepEqual(parts, [
    { text: "ja", ordinal: 0 },
    { text: " nej ", ordinal: null },
    { text: "ja", ordinal: 1 },
    { text: "!", ordinal: null },
  ]);
  assert.equal(parts.map((p) => p.text).join(""), text);
});

test("textParts has no empty parts and handles no matches and whole-text matches", () => {
  assert.deepEqual(textParts("abc", []), [{ text: "abc", ordinal: null }]);
  assert.deepEqual(textParts("", []), []);
  assert.deepEqual(textParts("abab", findMatches(rows("abab"), "ab")), [
    { text: "ab", ordinal: 0 },
    { text: "ab", ordinal: 1 },
  ]);
  const text = "İx";
  const parts = textParts(text, findMatches(rows(text), "i̇"));
  assert.equal(parts.map((p) => p.text).join(""), text);
  assert.ok(parts.every((p) => p.text.length > 0));
});

test("findStatus", () => {
  assert.deepEqual(findStatus(null, 0), { shown: "0/0", spoken: "No matches" });
  assert.deepEqual(findStatus(0, 0), { shown: "0/0", spoken: "No matches" });
  assert.deepEqual(findStatus(null, 1), { shown: "0/1", spoken: "1 match" });
  assert.deepEqual(findStatus(null, 5), { shown: "0/5", spoken: "5 matches" });
  assert.deepEqual(findStatus(1, 5), { shown: "2/5", spoken: "Match 2 of 5" });
});

test("sameMatches compares two lists match by match", () => {
  const text = [{ index: 0, text: "möte och mötet" }, { index: 1, text: "nästa möte" }];
  const a = findMatches(text, "möte");
  assert.equal(sameMatches(a, a), true);
  assert.equal(sameMatches(a, findMatches(text, " MÖTE ")), true);
  assert.equal(sameMatches(a, findMatches(text, "möt")), false);
  assert.equal(sameMatches(a, a.slice(1)), false);
  assert.equal(sameMatches(a, findMatches([text[0]!, { index: 2, text: "nästa möte" }], "möte")), false);
  assert.equal(sameMatches([], []), true);
});

test("regroupMatches keeps a row's array when its matches are unchanged, and the map when every row's is", () => {
  const text = [{ index: 0, text: "möte och mötet" }, { index: 1, text: "inget" }, { index: 2, text: "nästa möte" }];
  const before = regroupMatches(new Map(), findMatches(text, "möte"));
  assert.deepEqual([...before.keys()], [0, 2]);
  assert.deepEqual(before.get(2), [{ row: 2, start: 6, end: 10, ordinal: 2 }]);
  // The same matches from a query that folds to the same text: the very same map, so nothing redraws.
  assert.equal(regroupMatches(before, findMatches(text, "MÖTE")), before);
  assert.equal(regroupMatches(before, findMatches(text, " möte ")), before);
  // Row 2's text changed under the same query: row 0 keeps its array, row 2 gets a new one.
  const changed = regroupMatches(before, findMatches([text[0]!, text[1]!, { index: 2, text: "ett möte till" }], "möte"));
  assert.notEqual(changed, before);
  assert.equal(changed.get(0), before.get(0));
  assert.notEqual(changed.get(2), before.get(2));
  assert.deepEqual(changed.get(2), [{ row: 2, start: 4, end: 8, ordinal: 2 }]);
});

test("regroupMatches gives a row new matches when an ordinal moves, and drops rows with none", () => {
  const rowsOf = (first: string) => [{ index: 0, text: first }, { index: 1, text: "ja" }];
  const before = regroupMatches(new Map(), findMatches(rowsOf("nej"), "ja"));
  assert.deepEqual(before.get(1), [{ row: 1, start: 0, end: 2, ordinal: 0 }]);
  // Row 0 gains a match, so row 1's match is the second: its ordinal moved, and it is redrawn.
  const after = regroupMatches(before, findMatches(rowsOf("ja"), "ja"));
  assert.notEqual(after.get(1), before.get(1));
  assert.deepEqual(after.get(1), [{ row: 1, start: 0, end: 2, ordinal: 1 }]);
  // A query matching nothing leaves no rows; from no rows to none again, the map is kept.
  const none = regroupMatches(after, []);
  assert.equal(none.size, 0);
  assert.equal(regroupMatches(none, []), none);
});

test("sameMatchesOver compares only the rows given", () => {
  const a = regroupMatches(new Map(), findMatches([{ index: 0, text: "ja" }, { index: 1, text: "ja ja" }], "ja"));
  const b = regroupMatches(a, findMatches([{ index: 0, text: "nej" }, { index: 1, text: "ja ja" }], "ja"));
  assert.equal(sameMatchesOver([{ index: 0 }, { index: 1 }], a, a), true);
  // Row 0 lost its match, and row 1's ordinals moved from 1, 2 to 0, 1.
  assert.equal(sameMatchesOver([{ index: 0 }], a, b), false);
  assert.equal(sameMatchesOver([{ index: 1 }], a, b), false);
  // A row neither map has is the same in both.
  assert.equal(sameMatchesOver([{ index: 9 }], a, b), true);
  // Row 1 gaining a match after row 0's leaves row 0 the same.
  const c = regroupMatches(new Map(), findMatches([{ index: 0, text: "ja" }, { index: 1, text: "nej" }], "ja"));
  const d = regroupMatches(c, findMatches([{ index: 0, text: "ja" }, { index: 1, text: "nej ja" }], "ja"));
  assert.equal(sameMatchesOver([{ index: 0 }], c, d), true);
  assert.equal(sameMatchesOver([{ index: 1 }], c, d), false);
});

test("firstMatchWhere finds the first match a rising test passes, asking it about log n times", () => {
  const matches = findMatches(Array.from({ length: 100 }, (_, index) => ({ index, text: "a" })), "a");
  let asked = 0;
  const from = (row: number) => (match: { row: number }) => { asked += 1; return match.row >= row; };
  assert.equal(firstMatchWhere(matches, from(37)), 37);
  assert.ok(asked <= 8, `asked ${asked} times`);
  assert.equal(firstMatchWhere(matches, from(0)), 0);
  assert.equal(firstMatchWhere(matches, from(99)), 99);
  assert.equal(firstMatchWhere(matches, from(100)), null);
  assert.equal(firstMatchWhere([], from(0)), null);
});

test("matchFrom goes to the first match at or after a place, in the rows' order, wrapping to the first", () => {
  // Stored order is not index order: a place compares by its row's position in `rows`.
  const stored = [{ index: 7, text: "ja nej ja" }, { index: 3, text: "ja" }, { index: 12, text: "nej" }, { index: 1, text: "ja" }];
  const order = rowOrder(stored);
  assert.deepEqual([...order.entries()], [[7, 0], [3, 1], [12, 2], [1, 3]]);
  const matches = findMatches(stored, "ja");
  assert.deepEqual(matches.map((match) => [match.row, match.start]), [[7, 0], [7, 7], [3, 0], [1, 0]]);
  // At a match: that match.
  assert.equal(matchFrom(matches, order, { row: 7, start: 7 }), 1);
  // Between two matches in a row, and past a row's last match.
  assert.equal(matchFrom(matches, order, { row: 7, start: 3 }), 1);
  assert.equal(matchFrom(matches, order, { row: 7, start: 8 }), 2);
  // In a row with none (12): the next row's.
  assert.equal(matchFrom(matches, order, { row: 12, start: 0 }), 3);
  // Past the last match: round to the first.
  assert.equal(matchFrom(matches, order, { row: 1, start: 1 }), 0);
  // A row the order does not know, and no matches at all.
  assert.equal(matchFrom(matches, order, { row: 99, start: 0 }), 0);
  assert.equal(matchFrom([], order, { row: 7, start: 0 }), null);
});
