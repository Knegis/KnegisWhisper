import test from "node:test";
import assert from "node:assert/strict";
import { PAGES } from "../src/lib/pages.js";

// Dictation, Meetings and Notes draw no page header: the task heroes of
// Dictation and Meetings say what each page is for, and Notes is the writing
// space itself.
const HEADERLESS = new Set(["dictation", "meetings", "notes"]);

test("every screen but Dictation, Meetings and Notes has a title and a one-sentence intro", () => {
  for (const [id, page] of Object.entries(PAGES)) {
    if (HEADERLESS.has(id)) { assert.equal(page, null, id); continue; }
    assert.ok(page && page.title.length > 0, id);
    assert.ok(page.intro.endsWith(".") && page.intro.length <= 120, id);
  }
});
