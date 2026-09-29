import assert from "node:assert/strict";
import test from "node:test";

import { saveStatusText } from "../src/lib/saveStatus.js";

test("dirty and saving both read as the same in-flight line", () => {
  assert.equal(saveStatusText("dirty"), "Saving…");
  assert.equal(saveStatusText("saving"), "Saving…");
});

test("saved reads as \"Saved\"", () => {
  assert.equal(saveStatusText("saved"), "Saved");
});

test("idle and failed show nothing next to the title -- failed already has its own alert", () => {
  assert.equal(saveStatusText("idle"), null);
  assert.equal(saveStatusText("failed"), null);
});
