import assert from "node:assert/strict";
import test from "node:test";
import { normaliseSpeakerName, renameHeading, suggestionsFor } from "../src/lib/speakerNames.js";

test("speaker names trim and collapse inner whitespace", () => {
  assert.equal(normaliseSpeakerName("  Anna \t Lind  \n"), "Anna Lind");
  assert.equal(normaliseSpeakerName(" \t\n "), "");
});

test("the heading names the current speaker", () => {
  assert.equal(renameHeading("Speaker 1"), "Name for Speaker 1");
});

test("suggestions exclude only the current name without reordering", () => {
  assert.deepEqual(suggestionsFor("Anna Lind", ["Erik Berg", "ANNA LIND", "Ravi", "Anna", "anna lind"]), ["Erik Berg", "Ravi", "Anna"]);
});
