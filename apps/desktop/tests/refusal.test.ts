import { test } from "node:test";
import assert from "node:assert/strict";

import { refusalText } from "../src/lib/refusal.js";

// Rust's refusals of a change, as `local_model.rs` and `local_llm.rs` word
// them. Their own tests pin that every one starts with "Not now: ".
const MEETING = "Not now: a meeting is being transcribed on this computer. Try again when it's finished.";
const GENERATING = "Not now: a local text model is answering a request. Try again when it's done.";

test("a refusal is the sentence to show, as it is", () => {
  assert.equal(refusalText(MEETING), MEETING);
  assert.equal(refusalText(GENERATING), GENERATING);
});

test("anything else is not a refusal, and stays under Details", () => {
  assert.equal(refusalText("\"model-x\" is not a model this build knows about"), null);
  assert.equal(refusalText("could not bind 127.0.0.1:8178: address in use"), null);
  // The words alone, not at the start, are not the marker.
  assert.equal(refusalText("the server said: Not now: busy"), null);
  assert.equal(refusalText(""), null);
  assert.equal(refusalText(null), null);
  assert.equal(refusalText(undefined), null);
});
