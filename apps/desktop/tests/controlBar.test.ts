import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { menuGroups } from "../src/lib/chipMenu.js";
import { cleanupLabel, destinationName, languageLabel, microphoneMenu, privacyLines } from "../src/lib/controlBar.js";
import type { CleanupRoute } from "../src/generated/ipc-types.js";

const local = { isLocal: true, leavesDevice: false, destination: "local model" };
const cloud = { isLocal: false, leavesDevice: true, destination: "openai" };
const ready = (destination: string, leavesDevice: boolean, caveat: string | null = null): CleanupRoute => ({
  state: "ready",
  destination,
  model: "some-model",
  leavesDevice,
  caveat,
});
const off: CleanupRoute = { state: "off" };
const problem: CleanupRoute = { state: "problem", reason: "no provider is chosen" };

test("a local route says the audio stays on this computer", () => {
  assert.deepEqual(privacyLines(local, off), ["Audio stays on this computer"]);
});

test("a cloud route names where the audio goes", () => {
  assert.deepEqual(privacyLines(cloud, off), ["Audio is sent to OpenAI"]);
});

test("a self-hosted route on a private address says the audio is sent to a server on your network", () => {
  // It leaves this computer, so it says so (ruling R15): only "stays on this
  // computer" or "sent to X", never a third "stays" that is not this computer.
  const lan = { isLocal: false, leavesDevice: false, destination: "self-hosted" };
  assert.deepEqual(privacyLines(lan, off), ["Audio is sent to a server on your network"]);
});

test("a route with a problem says the audio stays, since nothing can be sent", () => {
  const broken = { isLocal: false, leavesDevice: false, destination: "nowhere", problem: "no provider" };
  assert.deepEqual(privacyLines(broken, off), ["Audio stays on this computer"]);
});

test("cleanup that is ready and leaves the device adds where the text goes", () => {
  assert.deepEqual(privacyLines(local, ready("anthropic", true)), [
    "Audio stays on this computer",
    "Text is sent to Anthropic to tidy it up",
  ]);
});

test("cleanup that stays on the device, is off, has a problem or is absent adds nothing", () => {
  for (const cleanup of [ready("local model", false), off, problem, null]) {
    assert.deepEqual(privacyLines(cloud, cleanup), ["Audio is sent to OpenAI"], JSON.stringify(cleanup));
  }
});

test("cleanup that resolves but cannot run (a caveat) adds nothing, because nothing is sent", () => {
  const noKey = ready("anthropic", true, "no key is stored for anthropic");
  assert.deepEqual(privacyLines(local, noKey), ["Audio stays on this computer"]);
});

test("languageLabel names the chosen languages, or says it detects", () => {
  assert.equal(languageLabel([]), "Detect language");
  assert.equal(languageLabel(["sv"]), "Swedish");
  assert.equal(languageLabel(["sv", "en"]), "Swedish, English");
  // A code this build has no name for is shown as itself rather than dropped.
  assert.equal(languageLabel(["xx"]), "xx");
});

test("cleanupLabel covers every CleanupRoute state", () => {
  assert.equal(cleanupLabel(off), "Tidy up: off");
  assert.equal(cleanupLabel(ready("openai", true)), "Tidy up: on");
  assert.equal(cleanupLabel(ready("local model", false)), "Tidy up: on");
  assert.equal(cleanupLabel(problem), "Tidy up: needs setup");
  assert.equal(cleanupLabel(ready("openai", true, "no key is stored for openai")), "Tidy up: needs setup");
  assert.equal(cleanupLabel(null), "Tidy up: off");
});

test("destinationName falls back to the destination itself", () => {
  assert.equal(destinationName("openai"), "OpenAI");
  assert.equal(destinationName("self-hosted at http://10.0.0.5:8080"), "self-hosted at http://10.0.0.5:8080");
});

test("the two endpoint routes Rust names are said in words, not as their ids (KNEG-352)", () => {
  assert.equal(destinationName("custom"), "your custom endpoint");
  assert.equal(destinationName("self-hosted"), "your self-hosted server");
  const custom = { isLocal: false, leavesDevice: true, destination: "custom" };
  assert.deepEqual(privacyLines(custom, off), ["Audio is sent to your custom endpoint"]);
});

test("every provider in the registry is named as the registry names it", () => {
  // The names are copied into `lib/controlBar.ts` rather than imported, so the
  // bundle does not carry the whole registry; this is what keeps the copy honest.
  const registry = JSON.parse(
    readFileSync(new URL("../../../../config/provider-registry.json", import.meta.url), "utf8"),
  ) as Record<string, unknown>;
  for (const group of ["transcription", "reasoning", "enterprise"]) {
    const providers = registry[group] as { id: string; name: string }[];
    assert.ok(Array.isArray(providers) && providers.length > 0, group);
    for (const provider of providers) assert.equal(destinationName(provider.id), provider.name, provider.id);
  }
});

const devices = [
  { name: "Microphone Array (Realtek(R) Audio)", isDefault: true },
  { name: "Headset (Jabra Evolve2 65)", isDefault: false },
];

test("with no microphone chosen, the system default is current and is the chip's label", () => {
  const menu = microphoneMenu(devices, null);
  assert.equal(menu.label, "System default");
  assert.equal(menu.missing, null);
  assert.deepEqual(
    menu.options.map((o) => [o.id, o.label, o.current]),
    [
      ["", "System default", true],
      ["Microphone Array (Realtek(R) Audio)", "Microphone Array (Realtek(R) Audio) (default)", false],
      ["Headset (Jabra Evolve2 65)", "Headset (Jabra Evolve2 65)", false],
    ],
  );
});

test("a chosen microphone is current, and its row's words are the chip's label", () => {
  const menu = microphoneMenu(devices, "Microphone Array (Realtek(R) Audio)");
  assert.equal(menu.label, "Microphone Array (Realtek(R) Audio) (default)");
  assert.deepEqual(menu.options.filter((o) => o.current).map((o) => o.id), ["Microphone Array (Realtek(R) Audio)"]);
});

test("a chosen microphone that is unplugged is listed first as itself, not as the system default", () => {
  const menu = microphoneMenu(devices, "USB Mic");
  assert.equal(menu.missing, "USB Mic");
  assert.equal(menu.label, "USB Mic (not connected)");
  assert.deepEqual(menu.options[0], {
    id: "USB Mic",
    label: "USB Mic (not connected)",
    group: null,
    current: true,
    disabled: false,
    reason: null,
  });
  assert.equal(menu.options.filter((o) => o.current).length, 1);
});

test("given a heading, every microphone row sits under it, as one group in the order listed (KNEG-364)", () => {
  const heading = "Used by meetings and dictation";
  const menu = microphoneMenu(devices, "USB Mic", heading);
  assert.deepEqual(menu.options.map((o) => o.group), [heading, heading, heading, heading]);
  assert.deepEqual(
    menuGroups(menu.options).map((run) => [run.group, run.items.map((item) => item.option.id)]),
    [[heading, ["USB Mic", "", "Microphone Array (Realtek(R) Audio)", "Headset (Jabra Evolve2 65)"]]],
  );
  // Only the heading changes: the rows, their words and what is current are Dictation's.
  const plain = microphoneMenu(devices, "USB Mic");
  assert.deepEqual(menu.options.map((o) => ({ ...o, group: null })), plain.options);
  assert.equal(menu.label, plain.label);
  assert.equal(menu.missing, plain.missing);
});

test("every microphone row can be chosen, and none is grouped", () => {
  const { options } = microphoneMenu(devices, "USB Mic");
  // The unplugged one, the system default and the two connected ones.
  assert.equal(options.length, 4);
  for (const o of options) {
    assert.equal(o.disabled, false, o.id);
    assert.equal(o.reason, null, o.id);
    assert.equal(o.group, null, o.id);
  }
});
