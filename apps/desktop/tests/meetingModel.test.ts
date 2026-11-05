import { strict as assert } from "node:assert";
import { test } from "node:test";

import { AUTOMATIC_MODEL, meetingModelDetail, meetingModelOptions, transcriberLabel } from "../src/lib/meetingModel.js";

const MODELS = [
  { id: "large", name: "Large", sizeMb: 3000, installed: true },
  { id: "medium", name: "Medium", sizeMb: 1500, installed: false },
  { id: "tiny", name: "Tiny", sizeMb: 75, installed: true },
  { id: "custom", name: "Custom", sizeMb: null, installed: true },
];
const ALL_INSTALLED = MODELS.map((model) => ({ ...model, installed: true }));
const MORE = " Install more under Settings > Transcription > Local.";

test("the options are Automatic, then what's installed, smallest first and an unknown size last", () => {
  assert.deepEqual(meetingModelOptions(MODELS, null), [
    { id: AUTOMATIC_MODEL, label: "Automatic", disabled: false },
    { id: "tiny", label: "Tiny · 75 MB", disabled: false },
    { id: "large", label: "Large · 3000 MB", disabled: false },
    { id: "custom", label: "Custom", disabled: false },
  ]);
});

test("a chosen model that has gone stays listed and greyed, so the select shows what is saved", () => {
  const notInstalled = meetingModelOptions(MODELS, "medium").at(-1);
  assert.deepEqual(notInstalled, { id: "medium", label: "Medium · not installed", disabled: true });
  // One this version no longer knows has only its id to go by.
  const unknown = meetingModelOptions(MODELS, "retired").at(-1);
  assert.deepEqual(unknown, { id: "retired", label: "retired · not installed", disabled: true });
  // An installed choice isn't listed twice.
  assert.equal(meetingModelOptions(MODELS, "tiny").filter((option) => option.id === "tiny").length, 1);
});

test("the chip names the chosen model only for the provider that uses it", () => {
  const LOCAL = { id: "local", name: "Local windowed model" };
  assert.equal(transcriberLabel(LOCAL, MODELS, "tiny"), "Tiny");
  assert.equal(transcriberLabel(LOCAL, MODELS, null), "Local windowed model");
  // One this version no longer knows has only its id to go by.
  assert.equal(transcriberLabel(LOCAL, MODELS, "retired"), "retired");
  assert.equal(transcriberLabel({ id: "deepgram-realtime", name: "Deepgram" }, MODELS, "tiny"), "Deepgram");
  assert.equal(transcriberLabel(null, MODELS, "tiny"), "Not set up");
});

test("a chosen model says it is loaded at Start and that local dictation shares it meanwhile", () => {
  assert.equal(
    meetingModelDetail({ models: ALL_INSTALLED, chosen: "tiny", runningModel: "large", parakeet: null }),
    "Loaded when a meeting starts, for the live transcript and the saved note. " +
      "Until the note is saved, dictation on a local Whisper model uses this one too.",
  );
});

test("a chosen model that isn't installed says only that, with the way out", () => {
  for (const chosen of ["medium", "retired"]) {
    assert.equal(
      meetingModelDetail({ models: MODELS, chosen, runningModel: "tiny", parakeet: null }),
      "This model isn't installed. Install it under Settings > Transcription > Local, or choose another.",
    );
  }
});

test("Automatic names the model loaded now, and only when one is", () => {
  assert.equal(
    meetingModelDetail({ models: ALL_INSTALLED, chosen: null, runningModel: "tiny", parakeet: null }),
    "Uses Tiny, the Whisper model loaded now.",
  );
  assert.equal(
    meetingModelDetail({ models: ALL_INSTALLED, chosen: null, runningModel: null, parakeet: null }),
    "Uses the Whisper model last chosen for dictation, or the first one installed.",
  );
});

test("Automatic says when a Parakeet model chosen for dictation makes the saved note", () => {
  assert.equal(
    meetingModelDetail({ models: ALL_INSTALLED, chosen: null, runningModel: "tiny", parakeet: "Parakeet V3" }),
    "Uses Tiny, the Whisper model loaded now. The saved note comes from Parakeet V3, the model chosen for dictation.",
  );
  // A model chosen here isn't overruled by it, so nothing is said.
  const chosen = meetingModelDetail({ models: ALL_INSTALLED, chosen: "tiny", runningModel: null, parakeet: "Parakeet V3" });
  assert.ok(!chosen.includes("Parakeet"), chosen);
});

test("installing more is mentioned only while there are more to install", () => {
  assert.ok(meetingModelDetail({ models: MODELS, chosen: null, runningModel: null, parakeet: null }).endsWith(MORE));
  assert.ok(meetingModelDetail({ models: MODELS, chosen: "tiny", runningModel: null, parakeet: null }).endsWith(MORE));
  assert.ok(!meetingModelDetail({ models: ALL_INSTALLED, chosen: null, runningModel: null, parakeet: null }).includes("Install more"));
});
