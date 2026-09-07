/** Syntax-checks every generated renderer expression before a live probe runs. */
import test from "node:test";
import assert from "node:assert/strict";
import * as snippets from "./snippets.mjs";

test("every renderer snippet is syntactically valid JavaScript", () => {
  const values = [
    snippets.removeFixtureBanner(),
    snippets.clickText("button", "Settings"),
    snippets.railGeometry(),
    snippets.groupState(),
    snippets.railCurrent(),
    snippets.contentMeasure(),
    snippets.notesGrid(),
    snippets.sidewaysState(),
    snippets.focusState(),
    snippets.meetingSnapshot(),
    snippets.invoke("meeting_status"),
    snippets.selectMeetingProvider("local streaming", true),
    snippets.meetingProviderValue("local streaming"),
    snippets.clickStop(),
  ];
  for (const value of values) {
    assert.doesNotThrow(() => new Function(value), value);
  }
});
