import assert from "node:assert/strict";
import test from "node:test";

import { saveStatusText, workspaceSaveText } from "../src/lib/saveStatus.js";
import type { AutosaveStatus } from "../src/lib/useAutosave.js";

const ALL: readonly AutosaveStatus[] = ["idle", "dirty", "saving", "saved", "failed"];

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

/* -------------------------------------------- the note and its summary (KNEG-398) */

test("\"Saved\" never shows while either editor has an edit pending or failed", () => {
  // The defect: the line followed the note alone, so it said "Saved" while an
  // AI summary edit was still waiting, or had failed.
  for (const note of ALL) {
    for (const summary of ALL) {
      if (workspaceSaveText(note, summary) !== "Saved") continue;
      for (const side of [note, summary]) {
        assert.ok(!["dirty", "saving", "failed"].includes(side), `note ${note}, summary ${summary}`);
      }
    }
  }
});

test("a summary that failed to save says so at the toolbar, whatever the note is doing", () => {
  // Its Retry is in the summary pane, which may be scrolled out of view.
  for (const note of ALL) assert.equal(workspaceSaveText(note, "failed"), "Summary not saved", note);
});

test("a note that failed to save leaves the line empty: its own banner says so", () => {
  for (const summary of ["idle", "dirty", "saving", "saved"] as const) {
    assert.equal(workspaceSaveText("failed", summary), null, summary);
  }
});

test("an edit pending in either editor reads as saving", () => {
  for (const pending of ["dirty", "saving"] as const) {
    for (const other of ["idle", "dirty", "saving", "saved"] as const) {
      assert.equal(workspaceSaveText(pending, other), "Saving…", `note ${pending}, summary ${other}`);
      assert.equal(workspaceSaveText(other, pending), "Saving…", `note ${other}, summary ${pending}`);
    }
  }
});

test("a save that landed in either editor, with nothing pending, reads as saved", () => {
  // A summary-only edit reads "Saving…" then "Saved", not "Saving…" then nothing.
  assert.equal(workspaceSaveText("idle", "saved"), "Saved");
  assert.equal(workspaceSaveText("saved", "idle"), "Saved");
  assert.equal(workspaceSaveText("saved", "saved"), "Saved");
});

test("nothing edited in either says nothing", () => {
  assert.equal(workspaceSaveText("idle", "idle"), null);
});
