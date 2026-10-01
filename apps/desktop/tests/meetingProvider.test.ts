import { test } from "node:test";
import assert from "node:assert/strict";
import { providerMenu, selectProvider } from "../src/lib/meetingProvider.js";
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
