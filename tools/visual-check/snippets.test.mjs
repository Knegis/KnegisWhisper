/** Syntax-checks every generated renderer expression before a live probe runs. */
import test from "node:test";
import assert from "node:assert/strict";
import * as snippets from "./snippets.mjs";

test("every renderer snippet is syntactically valid JavaScript", () => {
  const values = [
    snippets.removeFixtureBanner(),
    snippets.clickText("button", "Settings"),
    snippets.clickSelector(".app-titlebar button[aria-controls=\"list-panel\"]"),
    snippets.railState(),
    snippets.panelState(),
    snippets.panelToggle(),
    snippets.pageHeading(),
    snippets.overflowState(),
    snippets.searchState(),
    snippets.blurActive(),
    snippets.notesGrid(),
    snippets.notesTabsContained(),
    snippets.notesEntry("A title with \"quotes\" and a 'single' one"),
    snippets.focusState(),
    snippets.meetingSnapshot(),
    snippets.invoke("meeting_status"),
    snippets.selectMeetingProvider("local streaming"),
    snippets.startMeetingRecording(true),
    snippets.meetingProviderValue("local streaming"),
    snippets.clickStop(),
  ];
  for (const value of values) {
    assert.doesNotThrow(() => new Function(value), value);
  }
});
