import { test } from "node:test";
import assert from "node:assert/strict";
import { providerChoice, providerMenu, selectProvider } from "../src/lib/meetingProvider.js";
import type { ClientAvailability, MeetingProviderSummary } from "../src/generated/ipc-types.js";

const usable = ["openai-realtime", "local"];

test("nothing chosen and nothing saved falls back to the first usable provider", () => {
  assert.deepEqual(selectProvider(null, null, usable), {
    selected: "openai-realtime",
    savedUnusable: null,
  });
});

test("the saved provider wins over the first usable one", () => {
  assert.deepEqual(selectProvider(null, "local", usable), {
    selected: "local",
    savedUnusable: null,
  });
});

test("a choice made on screen wins over the saved provider until it is saved", () => {
  assert.deepEqual(selectProvider("openai-realtime", "local", usable), {
    selected: "openai-realtime",
    savedUnusable: null,
  });
});

test("a saved provider that cannot be used right now is named, and the first usable one is selected", () => {
  assert.deepEqual(selectProvider(null, "local", ["openai-realtime"]), {
    selected: "openai-realtime",
    savedUnusable: "local",
  });
});

test("a choice that stopped being usable falls through to the saved provider", () => {
  assert.deepEqual(selectProvider("openai-realtime", "local", ["local"]), {
    selected: "local",
    savedUnusable: null,
  });
});

test("no usable provider selects nothing but still names the saved one", () => {
  assert.deepEqual(selectProvider(null, "local", []), {
    selected: null,
    savedUnusable: "local",
  });
});

function provider(
  id: string,
  name: string,
  audioLeavesDevice: boolean,
  availability: ClientAvailability,
): MeetingProviderSummary {
  return {
    id,
    name,
    audioLeavesDevice,
    keyterms: { state: "unsupported" },
    sessionLimitSeconds: null,
    tokenRotation: false,
    sampleRates: [16000],
    speakerLabels: false,
    partials: false,
    languageDetection: false,
    reconnect: false,
    batchFallback: false,
    availability,
    credentialSlot: null,
    summary: "",
  };
}

const DEEPGRAM = provider("meeting:deepgram", "Deepgram", true, { state: "available" });
const ASSEMBLY = provider("meeting:assemblyai", "AssemblyAI", true, { state: "needs-credential", slot: "assemblyai" });
const LOCAL = provider("meeting:local", "Local model", false, { state: "available" });
const LOCAL_DOWN = provider("meeting:local", "Local model", false, { state: "declined", reason: "the server is not running" });

test("providerMenu groups providers by where the audio goes, this computer first, and marks the selected one", () => {
  const menu = providerMenu([DEEPGRAM, LOCAL], "meeting:local", "meeting:local");
  assert.deepEqual(
    menu.map((o) => [o.id, o.group, o.current, o.disabled]),
    [
      ["meeting:local", "On this computer", true, false],
      ["meeting:deepgram", "Cloud", false, false],
    ],
  );
});

test("providerMenu greys a provider that needs a key, with its reason, and omits a declined one", () => {
  const menu = providerMenu([DEEPGRAM, ASSEMBLY, LOCAL_DOWN], null, "meeting:deepgram");
  assert.deepEqual(menu.map((o) => o.id), ["meeting:deepgram", "meeting:assemblyai"]);
  const grey = menu[1];
  assert.equal(grey?.disabled, true);
  assert.equal(grey?.current, false);
  assert.equal(grey?.reason, "no key stored for assemblyai. Add one under Settings > Providers.");
  assert.equal(menu[0]?.reason, null);
});

test("providerMenu shows an unusable saved provider greyed with its reason, and the fallback as current", () => {
  const menu = providerMenu([DEEPGRAM, LOCAL_DOWN], "meeting:local", "meeting:deepgram");
  assert.deepEqual(
    menu.map((o) => [o.id, o.group, o.current, o.disabled, o.reason]),
    [
      ["meeting:local", "On this computer", false, true, "refused: the server is not running"],
      ["meeting:deepgram", "Cloud", true, false, null],
    ],
  );
});

test("providerMenu lists rows of one group together whatever the order it was given", () => {
  const menu = providerMenu([DEEPGRAM, LOCAL, ASSEMBLY], null, null);
  assert.deepEqual(menu.map((o) => o.group), ["On this computer", "Cloud", "Cloud"]);
});

test("providerMenu has no current row when nothing is selected", () => {
  assert.equal(providerMenu([ASSEMBLY], null, null).some((o) => o.current), false);
});

test("providerMenu labels a provider by name", () => {
  assert.equal(providerMenu([DEEPGRAM], null, null)[0]?.label, "Deepgram");
});

/* ------------------------------------- the local windowed provider's models */

const WINDOWED = { ...LOCAL, id: "local", name: "Local windowed model" };
const STREAMING = { ...LOCAL, id: "local-streaming", name: "Local streaming model" };
const MODELS = [
  { id: "base", name: "Base", sizeMb: 142, installed: true },
  { id: "medium", name: "Medium", sizeMb: 1500, installed: false },
  { id: "tiny", name: "Tiny", sizeMb: 75, installed: true },
];
const rows = (menu: ReturnType<typeof providerMenu>) =>
  menu.map((o) => [o.id, o.label, o.group, o.current, o.disabled, o.reason]);

test("the models follow this computer's providers under their own heading, cloud last", () => {
  const menu = providerMenu([DEEPGRAM, WINDOWED, STREAMING], "local", "local", { models: MODELS, chosen: null });
  assert.deepEqual(rows(menu), [
    ["local", "Local windowed model", "On this computer", true, false, null],
    ["local-streaming", "Local streaming model", "On this computer", false, false, null],
    ["local:tiny", "Tiny · 75 MB", "Local windowed models", false, false, null],
    ["local:base", "Base · 142 MB", "Local windowed models", false, false, null],
    ["meeting:deepgram", DEEPGRAM.name, "Cloud", false, false, null],
  ]);
  assert.equal(menu[0]?.detail, "Automatic model");
});

test("a chosen model is the current row, and the provider's own row, which is Automatic, is not", () => {
  const menu = providerMenu([WINDOWED], "local", "local", { models: MODELS, chosen: "base" });
  assert.deepEqual(menu.filter((o) => o.current).map((o) => o.id), ["local:base"]);
});

test("no model is current while another provider is selected, though the choice is still listed", () => {
  const menu = providerMenu([DEEPGRAM, WINDOWED], "meeting:deepgram", "meeting:deepgram", { models: MODELS, chosen: "base" });
  assert.deepEqual(menu.filter((o) => o.current).map((o) => o.id), ["meeting:deepgram"]);
  assert.ok(menu.some((o) => o.id === "local:base"));
});

test("a chosen model that has gone is listed greyed and current, with its reason", () => {
  const menu = providerMenu([WINDOWED], "local", "local", { models: MODELS, chosen: "medium" });
  const gone = menu.find((o) => o.id === "local:medium");
  assert.deepEqual([gone?.label, gone?.current, gone?.disabled, gone?.reason], ["Medium", true, true, "not installed"]);
  assert.equal(menu.find((o) => o.id === "local")?.current, false);
});

test("no models are listed while they aren't known, or when the provider can't be used", () => {
  const unknown = providerMenu([WINDOWED], "local", "local", null);
  assert.deepEqual(unknown.map((o) => o.id), ["local"]);
  assert.equal(unknown[0]?.detail, undefined);
  assert.equal(unknown[0]?.current, true);

  const down = { ...WINDOWED, availability: LOCAL_DOWN.availability };
  const menu = providerMenu([DEEPGRAM, down], "local", "meeting:deepgram", { models: MODELS, chosen: "base" });
  assert.deepEqual(menu.map((o) => o.id), ["local", "meeting:deepgram"]);
});

test("a model row sets the model and the provider that uses it, and writes only what differs", () => {
  assert.deepEqual(providerChoice("local:base", "meeting:deepgram", null), { provider: "local", model: "base" });
  assert.deepEqual(providerChoice("local:base", "local", "tiny"), { model: "base" });
  assert.deepEqual(providerChoice("local:base", "meeting:deepgram", "base"), { provider: "local" });
  assert.equal(providerChoice("local:base", "local", "base"), null);
});

test("the provider's own row is Automatic, so it clears a model", () => {
  assert.deepEqual(providerChoice("local", "local", "base"), { model: null });
  assert.deepEqual(providerChoice("local", "meeting:deepgram", "base"), { provider: "local", model: null });
  assert.equal(providerChoice("local", "local", null), null);
});

test("another provider leaves the model alone, so going to the cloud and back keeps it", () => {
  assert.deepEqual(providerChoice("meeting:deepgram", "local", "base"), { provider: "meeting:deepgram" });
  assert.deepEqual(providerChoice("local-streaming", "local", "base"), { provider: "local-streaming" });
  assert.equal(providerChoice("meeting:deepgram", "meeting:deepgram", "base"), null);
});

test("while the model isn't known, the provider's row claims nothing about it", () => {
  assert.deepEqual(providerChoice("local", "meeting:deepgram", undefined), { provider: "local" });
  assert.equal(providerChoice("local", "local", undefined), null);
});
