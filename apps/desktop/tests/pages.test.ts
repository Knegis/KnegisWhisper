import test from "node:test";
import assert from "node:assert/strict";
import { PAGES } from "../src/lib/pages.js";

test("every screen but Notes has a title and a one-sentence intro", () => {
  for (const [id, page] of Object.entries(PAGES)) {
    if (id === "notes") { assert.equal(page, null); continue; }
    assert.ok(page && page.title.length > 0, id);
    assert.ok(page.intro.endsWith(".") && page.intro.length <= 120, id);
  }
});
