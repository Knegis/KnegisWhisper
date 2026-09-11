import assert from "node:assert/strict";
import test from "node:test";
import { microphoneState, isMicrophoneDrag } from "../src/lib/microphone.js";
test("recording is distinct from processing and unknown state", () => {
    assert.equal(microphoneState("recording", false).recording, true);
    for (const state of ["transcribing", "inserting"]) {
        assert.equal(microphoneState(state, false).recording, false);
        assert.equal(microphoneState(state, false).disabled, true);
        assert.equal(microphoneState(state, false).busy, true);
    }
    assert.equal(microphoneState(null, true).label, "Status unavailable");
    assert.equal(microphoneState(null, false).disabled, true);
    assert.equal(microphoneState("idle", false).label, "Start dictation");
});
test("drag threshold is six pixels including diagonal motion", () => {
    assert.equal(isMicrophoneDrag(5, 0, 0, 0), false);
    assert.equal(isMicrophoneDrag(6, 0, 0, 0), true);
    assert.equal(isMicrophoneDrag(5, 5, 0, 0), true);
});
