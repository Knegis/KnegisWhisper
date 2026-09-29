import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { cleanupLabel, destinationName, languageLabel, privacyLines } from "../src/lib/controlBar.js";
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

test("a self-hosted route on a private address says it stays on your network", () => {
  const lan = { isLocal: false, leavesDevice: false, destination: "self-hosted" };
  assert.deepEqual(privacyLines(lan, off), ["Audio stays on your network"]);
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
