/**
 * `wordAt` and `wordsMatchText`: which word of a playing row is being said,
 * and whether a row's words can be shown in place of its text (KNEG-408).
 */
import assert from "node:assert/strict";
import test from "node:test";

import type { Word } from "../src/generated/ipc-types.js";
import { wordAt, wordsMatchText } from "../src/lib/wordTiming.js";

const words: Word[] = [
  { text: "Hej,", start: 1.0, end: 1.4 },
  { text: "hur", start: 1.6, end: 1.8 },
  { text: "mår", start: 1.8, end: 2.1 },
  { text: "du?", start: 2.5, end: 2.9 },
];

test("the word being said is the last one that has started", () => {
  assert.equal(wordAt(words, 1.0), 0);
  assert.equal(wordAt(words, 1.2), 0);
  assert.equal(wordAt(words, 1.6), 1);
  assert.equal(wordAt(words, 1.79), 1);
  assert.equal(wordAt(words, 1.8), 2, "a word that starts as another ends is the new one");
  assert.equal(wordAt(words, 2.6), 3);
});

test("a word stays the current one until the next one starts", () => {
  // 1.4 to 1.6 is a pause after "Hej,", which stays lit through it.
  assert.equal(wordAt(words, 1.5), 0);
  assert.equal(wordAt(words, 2.3), 2);
  // After the last word, until the row itself ends.
  assert.equal(wordAt(words, 9), 3);
});

test("before the first word, or with no words, no word is current", () => {
  assert.equal(wordAt(words, 0.99), null);
  assert.equal(wordAt(words, 0), null);
  assert.equal(wordAt([], 1.2), null);
});

test("a time that is not a number is no word", () => {
  assert.equal(wordAt(words, Number.NaN), null);
  assert.equal(wordAt(words, Number.POSITIVE_INFINITY), null);
});

test("words that start together: the later one is current", () => {
  const together: Word[] = [
    { text: "a", start: 1, end: 1 },
    { text: "b", start: 1, end: 1.5 },
    { text: "c", start: 2, end: 2.5 },
  ];
  assert.equal(wordAt(together, 1), 1);
  assert.equal(wordAt(together, 1.9), 1);
});

test("one word, and many: the search finds the same word as a scan", () => {
  assert.equal(wordAt([{ text: "Ja.", start: 3, end: 3.4 }], 3.2), 0);
  const many: Word[] = Array.from({ length: 1001 }, (_, index) => ({ text: `w${index}`, start: index * 0.4, end: index * 0.4 + 0.3 }));
  for (const time of [0, 0.39, 0.4, 123.45, 399.99, 400, 400.1, 1e6]) {
    const scanned = many.reduce<number | null>((found, word, index) => (word.start <= time ? index : found), null);
    assert.equal(wordAt(many, time), scanned, `at ${time}`);
  }
});

test("words match a row's text with whitespace aside", () => {
  assert.equal(wordsMatchText(words, "Hej, hur mår du?"), true);
  assert.equal(wordsMatchText(words, "  Hej,  hur mår\ndu? "), true);
  assert.equal(wordsMatchText(words, "Hej, hur mår ni?"), false, "edited text");
  assert.equal(wordsMatchText(words, "Hej, hur mår du? Bra."), false, "longer text");
  assert.equal(wordsMatchText([], "Hej"), false, "no words to show");
  assert.equal(wordsMatchText([], ""), false, "no words to show");
});
