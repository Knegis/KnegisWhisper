import { test } from "node:test";
import assert from "node:assert/strict";
import { routeMenu, type RouteChoice } from "../src/lib/dictationRoute.js";
import type { LocalStatus, ProviderSummary, RouteReport, SherpaStatus } from "../src/generated/ipc-types.js";
import { closeLabel, guideShown, routeStep, serviceOffer, stepsDone } from "../src/lib/setupGuide.js";

const providers = [
  { id: "openai", name: "OpenAI", configured: false },
  { id: "mistral", name: "Mistral", configured: true },
];

test("the first step is done only when the route would work (KNEG-329)", () => {
  // A new profile: OpenAI, no key. The page must not read as ready, and neither does the guide.
  assert.deepEqual(routeStep({ isLocal: false, destination: "openai", problem: null }, providers),
    { done: false, line: "OpenAI is chosen, and it has no key yet." });
  assert.deepEqual(routeStep({ isLocal: false, destination: "mistral", problem: null }, providers),
    { done: true, line: "At Mistral. Your audio is sent to Mistral." });
  assert.deepEqual(routeStep({ isLocal: true, destination: "local model", problem: null }, providers),
    { done: true, line: "On this computer. Your audio stays here." });
  assert.deepEqual(routeStep({ isLocal: true, destination: "local model", problem: "no model is installed" }, providers),
    { done: false, line: "Nothing is chosen yet." });
  assert.equal(routeStep({ isLocal: false, destination: "my own server", problem: null }, providers).done, true);
});

test("the guide says where audio goes whenever it leaves this computer", () => {
  const cloud = routeStep({ isLocal: false, destination: "mistral", problem: null }, providers);
  assert.match(cloud.line, /is sent to Mistral/);
  const local = routeStep({ isLocal: true, destination: "local model", problem: null }, providers);
  assert.match(local.line, /stays here/);
});

test("it counts what is done, and only calls itself done at three", () => {
  const ready = { done: true, line: "" };
  const not = { done: false, line: "" };
  assert.equal(stepsDone(not, false, false), 0);
  assert.equal(stepsDone(ready, true, false), 2);
  assert.equal(stepsDone(ready, true, true), 3);
  assert.equal(closeLabel(0), "Skip setup");
  assert.equal(closeLabel(2), "Skip setup");
  assert.equal(closeLabel(3), "Done");
});

test("the guide is shown when pending and never shown before", () => {
  assert.equal(guideShown(true, false, false), true);
});

test("the first dictation wrote a history row: the guide stays shown", () => {
  assert.equal(guideShown(false, true, false), true);
});

test("a closed guide is hidden in every combination", () => {
  for (const pending of [false, true]) {
    for (const wasShown of [false, true]) {
      assert.equal(guideShown(pending, wasShown, true), false);
    }
  }
});

test("the guide stays hidden when never pending and never shown", () => {
  assert.equal(guideShown(false, false, false), false);
});

test("a service is offered only when its cloud route can be chosen", () => {
  const service = { id: "mistral", name: "Mistral", configured: true };
  const choices = new Map<string, RouteChoice>([
    ["cloud:mistral", { kind: "cloud", provider: "mistral", model: "voxtral", label: "Mistral" }],
    ["whisper:small", { kind: "whisper", model: "small", label: "Small" }],
  ]);
  assert.equal(serviceOffer({ ...service, configured: false }, null, choices), null);
  assert.deepEqual(serviceOffer(service, null, choices), { kind: "use" });
  // A matching choice comes before the in-use fallback.
  assert.deepEqual(serviceOffer(service, "mistral", choices), { kind: "use" });
  const unavailable = serviceOffer(service, null, new Map());
  assert.equal(unavailable?.kind, "settings");
  assert.ok(unavailable?.kind === "settings");
  assert.match(unavailable.line, /Mistral/);
  assert.match(unavailable.line, /Settings › Transcription/);
  assert.equal(serviceOffer(service, "mistral", new Map()), null);
  const otherChoices = new Map<string, RouteChoice>([
    ["cloud:mistral", { kind: "cloud", provider: "openai", model: "whisper-1", label: "OpenAI" }],
    ["whisper:mistral", { kind: "whisper", model: "mistral", label: "Mistral" }],
  ]);
  assert.deepEqual(serviceOffer(service, null, otherChoices), unavailable);
});

function provider(id: string, name: string): ProviderSummary {
  const models = [`${id}-1`];
  return {
    id, name, credentialSlot: id, configured: true, modelCount: models.length, models,
    dispatch: id === "corti"
      ? { state: "unresolvable", reason: "Corti needs an environment and a tenant" }
      : { state: "ready" },
  };
}

function local(): LocalStatus {
  return {
    runtimeMessage: "",
    availability: { state: "installable" },
    serverInstalled: true,
    serverFreshness: { state: "current" },
    models: [],
    port: null,
    runningModel: null,
    installDir: "",
    installDirFellBack: false,
    runtime: { state: "stopped" },
  };
}

function sherpa(): SherpaStatus {
  return {
    availability: { state: "installable" },
    cliInstalled: true,
    cliFreshness: { state: "current" },
    models: [],
    selected: null,
    selectedStream: null,
    streamBindsLoopback: null,
    modelsDir: "",
  };
}

test("the real route menu offers Mistral but sends Corti to Transcription settings", () => {
  const corti = provider("corti", "Corti");
  const mistral = provider("mistral", "Mistral");
  const route: RouteReport = {
    destination: "openai", isLocal: false, leavesDevice: true, problem: null, cloudLanguage: null,
  };
  const menu = routeMenu([corti, mistral], local(), sherpa(), route);
  assert.deepEqual(serviceOffer(mistral, route.destination, menu.choices), { kind: "use" });
  const offer = serviceOffer(corti, route.destination, menu.choices);
  assert.equal(offer?.kind, "settings");
  assert.ok(offer?.kind === "settings");
  assert.match(offer.line, /Corti/);
  assert.match(offer.line, /Settings › Transcription/);
});
