import { test } from "node:test";
import assert from "node:assert/strict";
import { localEngine, routeBusyLabel, routeHeld, routeMenu } from "../src/lib/dictationRoute.js";
import type {
  Dispatch,
  LocalStatus,
  ModelStatus,
  ProviderSummary,
  RouteReport,
  SherpaModelStatus,
  SherpaStatus,
} from "../src/generated/ipc-types.js";

function provider(id: string, name: string, configured: boolean, dispatch: Dispatch, models = [`${id}-1`, `${id}-2`]): ProviderSummary {
  return { id, name, credentialSlot: id, configured, modelCount: models.length, models, dispatch };
}

function whisper(id: string, name: string, installed: boolean): ModelStatus {
  return { id, name, sizeMb: null, recommended: false, installed, verifiedBy: "digest" };
}

function parakeet(id: string, name: string, installed: boolean, usedFor: "dictation" | "meetings"): SherpaModelStatus {
  return { id, name, sizeMb: null, language: null, installed, usedFor };
}

function local(runningModel: string | null, models: ModelStatus[] = WHISPER): LocalStatus {
  return {
    runtimeMessage: "",
    availability: { state: "installable" },
    serverInstalled: true,
    serverFreshness: { state: "current" },
    models,
    port: runningModel === null ? null : 8178,
    runningModel,
    installDir: "",
    installDirFellBack: false,
    runtime: runningModel === null ? { state: "stopped" } : { state: "ready", model: runningModel, port: 8178 },
  };
}

function sherpa(selected: string | null, models: SherpaModelStatus[] = PARAKEET): SherpaStatus {
  return {
    availability: { state: "installable" },
    cliInstalled: true,
    cliFreshness: { state: "current" },
    models,
    selected,
    selectedStream: null,
    modelsDir: "",
  };
}

const cloudRoute = (destination: string): RouteReport => ({ destination, isLocal: false, leavesDevice: true, problem: null });
const LOCAL_ROUTE: RouteReport = { destination: "local model", isLocal: true, leavesDevice: false, problem: null };
const BROKEN_ROUTE: RouteReport = { destination: "nowhere", isLocal: false, leavesDevice: false, problem: "no provider" };

const WHISPER = [
  whisper("large", "Whisper Large", true),
  whisper("medium", "Whisper Medium", false),
  whisper("small", "Whisper Small", true),
];
const PARAKEET = [
  parakeet("unified", "Parakeet Unified", true, "dictation"),
  parakeet("tdt", "Parakeet TDT", false, "dictation"),
  parakeet("nemotron", "Nemotron Streaming", true, "meetings"),
];
const READY: Dispatch = { state: "ready" };
const PROVIDERS = [
  provider("openai", "OpenAI", true, READY, ["whisper-1", "gpt-4o-transcribe"]),
  provider("groq", "Groq", true, { state: "needs-client", reason: "no Groq client is built yet" }),
  provider("berget", "Berget AI", false, READY),
  provider("corti", "Corti", true, { state: "unresolvable", reason: "Corti needs an environment and a tenant" }),
];

const rows = (menu: ReturnType<typeof routeMenu>) =>
  menu.options.map((o) => [o.id, o.label, o.group, o.current, o.disabled, o.reason]);

test("the local models come first, under On this computer, then the cloud providers, each in the order reported", () => {
  const menu = routeMenu(PROVIDERS, local(null), sherpa(null), cloudRoute("openai"));
  assert.deepEqual(
    menu.options.map((o) => [o.id, o.group]),
    [
      ["whisper:large", "On this computer"],
      ["whisper:small", "On this computer"],
      ["parakeet:unified", "On this computer"],
      ["cloud:openai", "Cloud"],
      ["cloud:berget", "Cloud"],
      ["cloud:corti", "Cloud"],
    ],
  );
});

test("only installed models are offered: an uninstalled one has nothing to switch to", () => {
  const ids = routeMenu(PROVIDERS, local(null), sherpa(null), cloudRoute("openai")).options.map((o) => o.id);
  assert.ok(ids.length > 0);
  assert.ok(!ids.includes("whisper:medium"));
  assert.ok(!ids.includes("parakeet:tdt"));
});

test("a Parakeet model for meetings is not a dictation route, even installed", () => {
  const ids = routeMenu(PROVIDERS, local(null), sherpa(null), cloudRoute("openai")).options.map((o) => o.id);
  assert.ok(ids.includes("parakeet:unified"));
  assert.ok(!ids.includes("parakeet:nemotron"));
});

test("a provider this build has no client for is left out", () => {
  const ids = routeMenu(PROVIDERS, local(null), sherpa(null), cloudRoute("openai")).options.map((o) => o.id);
  assert.ok(ids.includes("cloud:openai"));
  assert.ok(!ids.includes("cloud:groq"));
});

test("a provider with no key stored is greyed with \"needs a key\", and choosing it writes nothing", () => {
  const menu = routeMenu(PROVIDERS, local(null), sherpa(null), cloudRoute("openai"));
  const berget = menu.options.find((o) => o.id === "cloud:berget");
  assert.deepEqual(berget && [berget.disabled, berget.reason], [true, "needs a key"]);
  assert.equal(menu.choices.get("cloud:berget"), undefined);
});

test("Corti is greyed \"set up in Settings\" unless it is the route in use", () => {
  const elsewhere = routeMenu(PROVIDERS, local(null), sherpa(null), cloudRoute("openai"));
  const greyed = elsewhere.options.find((o) => o.id === "cloud:corti");
  assert.deepEqual(greyed && [greyed.current, greyed.disabled, greyed.reason], [false, true, "set up in Settings"]);
  assert.equal(elsewhere.choices.get("cloud:corti"), undefined);

  const inUse = routeMenu(PROVIDERS, local(null), sherpa(null), cloudRoute("corti"));
  const current = inUse.options.find((o) => o.id === "cloud:corti");
  assert.deepEqual(current && [current.current, current.disabled, current.reason], [true, false, null]);
});

test("Corti with no key says it needs one first", () => {
  const providers = [provider("corti", "Corti", false, { state: "unresolvable", reason: "needs an environment" })];
  const corti = routeMenu(providers, local(null), sherpa(null), cloudRoute("openai")).options.find(
    (o) => o.id === "cloud:corti",
  );
  assert.deepEqual(corti && [corti.disabled, corti.reason], [true, "needs a key"]);
});

test("a provider whose registry entry lists no model is greyed, as Settings disables its button", () => {
  const providers = [provider("empty", "Empty", true, READY, [])];
  const empty = routeMenu(providers, local(null), sherpa(null), cloudRoute("openai")).options.find(
    (o) => o.id === "cloud:empty",
  );
  assert.deepEqual(empty && [empty.disabled, empty.reason], [true, "no model listed"]);
});

test("a cloud route: the provider it resolves to is current, and nothing else is", () => {
  const menu = routeMenu(PROVIDERS, local("small"), sherpa(null), cloudRoute("openai"));
  assert.deepEqual(
    menu.options.filter((o) => o.current).map((o) => o.id),
    ["cloud:openai"],
  );
  assert.equal(menu.current?.id, "cloud:openai");
  assert.equal(menu.current?.label, "OpenAI");
});

test("a local route with no Parakeet model selected: the model the running server loaded is current", () => {
  const menu = routeMenu(PROVIDERS, local("small"), sherpa(null), LOCAL_ROUTE);
  assert.deepEqual(
    menu.options.filter((o) => o.current).map((o) => o.id),
    ["whisper:small"],
  );
  assert.equal(menu.current?.label, "Whisper Small");
});

test("a selected Parakeet model outranks a running whisper server, so it is current and the whisper model is not", () => {
  const menu = routeMenu(PROVIDERS, local("small"), sherpa("unified"), LOCAL_ROUTE);
  assert.deepEqual(
    menu.options.filter((o) => o.current).map((o) => o.id),
    ["parakeet:unified"],
  );
});

test("a server that is up is not a route pointing at it: off the local route, no local model is current", () => {
  const menu = routeMenu(PROVIDERS, local("small"), sherpa("unified"), cloudRoute("openai"));
  assert.deepEqual(
    menu.options.filter((o) => o.current).map((o) => o.id),
    ["cloud:openai"],
  );
});

test("a local route with no server running and no Parakeet model has no current row", () => {
  const menu = routeMenu(PROVIDERS, local(null), sherpa(null), LOCAL_ROUTE);
  assert.equal(menu.options.length, 6);
  assert.equal(menu.current, null);
  assert.ok(menu.options.every((o) => !o.current));
});

test("a route with a problem, or one to a destination the menu does not list, has no current row", () => {
  for (const route of [BROKEN_ROUTE, cloudRoute("self-hosted")]) {
    const menu = routeMenu(PROVIDERS, local("small"), sherpa(null), route);
    assert.equal(menu.options.length, 6, route.destination);
    assert.equal(menu.current, null, route.destination);
    assert.ok(menu.options.every((o) => !o.current), route.destination);
  }
});

test("the route in use can still be greyed: a current provider with no key says so", () => {
  const menu = routeMenu(PROVIDERS, local(null), sherpa(null), cloudRoute("berget"));
  assert.deepEqual(
    rows(menu).find(([id]) => id === "cloud:berget"),
    ["cloud:berget", "Berget AI", "Cloud", true, true, "needs a key"],
  );
});

test("a provider not in use is switched to with its first model, as Settings does", () => {
  const menu = routeMenu(PROVIDERS, local(null), sherpa(null), LOCAL_ROUTE);
  assert.deepEqual(menu.choices.get("cloud:openai"), {
    kind: "cloud",
    provider: "openai",
    model: "whisper-1",
    label: "OpenAI",
  });
});

test("the provider in use writes nothing when chosen again, so the route keeps its own model", () => {
  // Not models[0]: the route may be on gpt-4o-transcribe, which no read
  // reports, and writing models[0] would replace it.
  const menu = routeMenu(PROVIDERS, local(null), sherpa(null), cloudRoute("openai"));
  assert.equal(menu.current?.id, "cloud:openai");
  assert.equal(menu.choices.get("cloud:openai"), undefined);
});

test("the local models are switched to by id, and the one in use writes nothing", () => {
  const menu = routeMenu(PROVIDERS, local("small"), sherpa(null), LOCAL_ROUTE);
  assert.deepEqual(menu.choices.get("whisper:large"), { kind: "whisper", model: "large", label: "Whisper Large" });
  assert.deepEqual(menu.choices.get("parakeet:unified"), {
    kind: "parakeet",
    model: "unified",
    label: "Parakeet Unified",
  });
  assert.equal(menu.choices.get("whisper:small"), undefined);
});

test("every row that can be chosen has a choice, and only those", () => {
  const menu = routeMenu(PROVIDERS, local("small"), sherpa(null), cloudRoute("openai"));
  const choosable = menu.options.filter((o) => !o.current && !o.disabled).map((o) => o.id);
  assert.ok(choosable.length > 0);
  assert.deepEqual([...menu.choices.keys()].sort(), choosable.sort());
});

test("which engine a local route runs on", () => {
  assert.equal(localEngine(cloudRoute("openai"), sherpa("unified")), null);
  assert.equal(localEngine(BROKEN_ROUTE, sherpa(null)), null);
  assert.equal(localEngine(LOCAL_ROUTE, sherpa(null)), "whisper");
  assert.equal(localEngine(LOCAL_ROUTE, sherpa("unified")), "parakeet");
});

test("the chip says Starting while a local model is being switched to, and nothing for a cloud switch", () => {
  assert.equal(routeBusyLabel({ kind: "whisper", model: "large", label: "Whisper Large" }), "Starting Whisper Large…");
  assert.equal(routeBusyLabel({ kind: "parakeet", model: "unified", label: "Parakeet Unified" }), "Starting Parakeet Unified…");
  assert.equal(routeBusyLabel({ kind: "cloud", provider: "openai", model: "whisper-1", label: "OpenAI" }), null);
  assert.equal(routeBusyLabel(null), null);
});

test("the route and the language can be changed only between dictations", () => {
  assert.equal(routeHeld("idle"), null);
  for (const state of ["recording", "transcribing", "inserting"] as const) {
    // Rust resolves both at Stop, so a change now would apply to audio
    // already captured under the old privacy line.
    assert.equal(routeHeld(state), "Can't change during a dictation", state);
  }
});
