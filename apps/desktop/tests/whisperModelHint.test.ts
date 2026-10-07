/**
 * `whisperModelHint` labels the big local Whisper models (KNEG-407).
 *
 * By size rather than by id: KB Whisper Medium and Large were the reason, but
 * the upstream Medium, Large and Turbo are bigger still, and a list that called
 * only the KB ones slower would imply the others are not.
 */
import assert from "node:assert/strict";
import test from "node:test";

import { whisperModelHint } from "../src/lib/labels.js";

test("a model of 500 MB or more is labelled slower and best for imports and meetings", () => {
  for (const sizeMb of [500, 540, 1081, 1500, 3000]) {
    assert.equal(whisperModelHint(sizeMb), "slower; best for imports and meetings", `${sizeMb} MB`);
  }
});

test("a smaller model, or one of unknown size, has no label", () => {
  for (const sizeMb of [30, 55, 142, 175, 466, 499]) {
    assert.equal(whisperModelHint(sizeMb), null, `${sizeMb} MB`);
  }
  assert.equal(whisperModelHint(null), null);
});
