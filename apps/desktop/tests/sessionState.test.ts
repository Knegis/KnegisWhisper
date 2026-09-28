import assert from "node:assert/strict";
import test from "node:test";

import { sessionStateLabel } from "../src/lib/labels.js";
import type { SessionState } from "../src/generated/ipc-types.js";

const states: SessionState[] = ["idle", "recording", "transcribing", "inserting"];

test("every dictation session state gets its own non-empty label", () => {
  for (const state of states) {
    const label = sessionStateLabel(state);
    assert.notEqual(label.trim(), "", `${state} rendered an empty label`);
  }
});

test("the four labels are distinct from one another", () => {
  const labels = states.map(sessionStateLabel);
  assert.equal(new Set(labels).size, labels.length, labels.join(", "));
});
