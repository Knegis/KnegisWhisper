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
    snippets.transcriptToggle(),
    snippets.pressTranscriptToggle(),
    snippets.notesTabsContained(),
    snippets.notesEntry("A title with \"quotes\" and a 'single' one"),
    snippets.focusState(),
    snippets.meetingSnapshot(),
    snippets.invoke("meeting_status"),
    snippets.selectMeetingProvider("local streaming"),
    snippets.startMeetingRecording(true),
    snippets.recordThisCall(false),
    snippets.meetingProviderValue("local streaming"),
    snippets.clickStop(),
    snippets.installInvokeStub(),
    snippets.stubInvoke({
      delay: { local_server_start: 1500 },
      reject: { reasoning_set_cleanup: "a \"refusal\" and a 'quote'" },
      merge: { reasoning_status: { route: { state: "off" } } },
    }),
    snippets.stubInvoke(),
    snippets.stubCalls("meeting_set_provider"),
    snippets.clearStubCalls(),
    snippets.invokeFixture("meeting_set_provider", { provider: "meeting:local" }),
    snippets.emitDictationState("recording"),
    snippets.chipState("Transcription: "),
    snippets.chipCenter("Tidy up"),
    snippets.centerOfText(".task-hero button.primary", "Choose a provider"),
    snippets.centerOfText(".task-hero button", null),
    snippets.optionCenter("Whisper \"Small\" (Q5)"),
    snippets.menuVisible(),
    snippets.menuState(),
    snippets.taskPageState(),
    snippets.focusChip("Microphone: "),
    snippets.themeState(),
    snippets.playerState(),
    snippets.startPlaybackSampler(),
    snippets.playbackSamples(),
    snippets.choosePlayerSpeed(".note-side-player", "1.5"),
  ];
  for (const value of values) {
    assert.doesNotThrow(() => new Function(value), value);
  }
});
