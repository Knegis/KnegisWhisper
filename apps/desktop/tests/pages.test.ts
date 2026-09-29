import test from "node:test";
import assert from "node:assert/strict";
import { PAGES } from "../src/lib/pages.js";

// Dictation and Notes draw no page header: Dictation's task hero says what the
// page is for, and Notes is the writing space itself.
const HEADERLESS = new Set(["dictation", "notes"]);

test("every screen but Dictation and Notes has a title and a one-sentence intro", () => {
  for (const [id, page] of Object.entries(PAGES)) {
    if (HEADERLESS.has(id)) { assert.equal(page, null, id); continue; }
    assert.ok(page && page.title.length > 0, id);
    assert.ok(page.intro.endsWith(".") && page.intro.length <= 120, id);
  }
});
