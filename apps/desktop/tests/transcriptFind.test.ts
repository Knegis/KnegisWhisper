import test from "node:test";
import assert from "node:assert/strict";
import {
  findMatches,
  findStatus,
  matchesByRow,
  normaliseQuery,
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
