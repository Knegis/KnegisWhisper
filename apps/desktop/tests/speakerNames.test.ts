import assert from "node:assert/strict";
import test from "node:test";
import {
  canSaveSpeakerName,
  defaultSpeakerName,
  matchingSuggestions,
  normaliseSpeakerName,
  renameHeading,
  speakerIsNamed,
  suggestionsFor,
} from "../src/lib/speakerNames.js";

test("speaker names trim and collapse inner whitespace", () => {
  assert.equal(normaliseSpeakerName("  Anna \t Lind  \n"), "Anna Lind");
  assert.equal(normaliseSpeakerName(" \t\n "), "");
});

test("the heading names the current speaker", () => {
  assert.equal(renameHeading("Speaker 1"), "Name for Speaker 1");
});

test("suggestions exclude only the current name without reordering", () => {
  assert.deepEqual(suggestionsFor("Anna Lind", ["Erik Berg", "ANNA LIND", "Ravi", "Anna", "anna lind"]), ["Erik Berg", "Ravi", "Anna"]);
});

test("a speaker nobody named shows the label Rust gives it", () => {
  // `speaker_name` in crates/notes-core/src/transcript.rs: `speaker_0` is the first speaker.
  assert.equal(defaultSpeakerName("speaker_0"), "Speaker 1");
  assert.equal(defaultSpeakerName("speaker_11"), "Speaker 12");
  assert.equal(defaultSpeakerName("A"), "A");
  assert.equal(defaultSpeakerName("speaker_x"), "speaker_x");
});

test("a speaker is named only when they show something other than that label", () => {
  assert.equal(speakerIsNamed("Speaker 1", "speaker_0"), false);
  assert.equal(speakerIsNamed("Anna Lind", "speaker_0"), true);
  assert.equal(speakerIsNamed("A", "A"), false);
  // Another speaker's number is a name somebody chose.
  assert.equal(speakerIsNamed("Speaker 2", "speaker_0"), true);
});

test("Save is on only when it would change something", () => {
  const unnamed = { current: "Speaker 1", speakerId: "speaker_0" };
  assert.equal(canSaveSpeakerName("", unnamed), false);
  assert.equal(canSaveSpeakerName("   ", unnamed), false);
  assert.equal(canSaveSpeakerName(" Speaker  1 ", unnamed), false);
  assert.equal(canSaveSpeakerName("Anna", unnamed), true);
  const named = { current: "Anna Lind", speakerId: "speaker_0" };
  assert.equal(canSaveSpeakerName("Anna  Lind ", named), false);
  assert.equal(canSaveSpeakerName("Anna", named), true);
  // Cleared, Save removes the name, as Rust takes an empty name to mean.
  assert.equal(canSaveSpeakerName("", named), true);
});

test("suggestions narrow to what is typed, in their order", () => {
  const names = ["Erik Berg", "Anna Lind", "Annika Ek", "Ravi"];
  assert.deepEqual(matchingSuggestions("Speaker 1", "", names), names);
  assert.deepEqual(matchingSuggestions("Speaker 1", "  an ", names), ["Anna Lind", "Annika Ek"]);
  assert.deepEqual(matchingSuggestions("Speaker 1", "ERIK", names), ["Erik Berg"]);
  assert.deepEqual(matchingSuggestions("Speaker 1", "zzz", names), []);
});

test("a name already in the field doesn't narrow the suggestions", () => {
  const names = ["Erik Berg", "Anna Lind", "Ravi"];
  // A named speaker's own name, as the form opens: the others are still on offer.
  assert.deepEqual(matchingSuggestions("Sara Holm", "Sara Holm", names), names);
  // A suggestion just pressed: the row stays put, and another can be pressed instead.
  assert.deepEqual(matchingSuggestions("Speaker 1", "anna lind", names), names);
});

test("suggestions leave out the current name and stop at a row's worth", () => {
  assert.deepEqual(matchingSuggestions("Anna Lind", "", ["ANNA LIND", "Ravi"]), ["Ravi"]);
  const many = Array.from({ length: 20 }, (_, index) => `Person ${index}`);
  assert.equal(matchingSuggestions("Speaker 1", "", many).length, 8);
  // Typing still reaches one past the cap: part of a name narrows to it, and the whole of one keeps
  // its pill in the row.
  assert.deepEqual(matchingSuggestions("Speaker 1", "Za", [...many, "Zara"]), ["Zara"]);
  const whole = matchingSuggestions("Speaker 1", "Person 19", many);
  assert.equal(whole.length, 8);
  assert.ok(whole.includes("Person 19"));
});
