import assert from "node:assert/strict";
import test from "node:test";

import { selectProvider } from "../src/lib/meetingProvider.js";
import { dictationReadiness, localProviderProblem, meetingReadiness } from "../src/lib/readiness.js";
import type {
  HotkeyReport,
  MeetingProviderSummary,
  RouteReport,
} from "../src/generated/ipc-types.js";

const report = (over: Partial<HotkeyReport> = {}): HotkeyReport => ({
  registrations: [
    { action: "dictation", results: [{ state: "registered", accelerator: "Control+Shift+D" }] },
  ],
  portalSupported: false,
  portalEnabled: false,
  bindings: [
    {
      action: "dictation",
      accelerators: [{ kind: "standard", accelerator: "Control+Shift+D" }],
      mode: "toggle",
    },
  ],
  conflicts: [],
  unhandled: [],
  cautions: [],
  ...over,
});

const route = (over: Partial<RouteReport> = {}): RouteReport => ({
  destination: "local model",
  isLocal: true,
  leavesDevice: false,
  problem: null,
  cloudLanguage: null,
  ...over,
});

/* -------------------------------------------------------------- dictation */

test("a route problem is needs-setup, fixed by opening Transcription, with the problem as detail", () => {
  const r = dictationReadiness({
    route: route({ problem: "No transcription provider is configured." }),
    hotkeys: report(),
    canInsert: true,
  });
  assert.equal(r.state, "needs-setup");
  if (r.state !== "needs-setup") return;
  assert.equal(r.problem, "Choose where your speech is turned into text.");
  assert.equal(r.detail, "No transcription provider is configured.");
  assert.deepEqual(r.fix, { view: "transcription", label: "Open Transcription" });
});

test("a cloud service with no key is needs-setup, not ready (KNEG-329)", () => {
  // A new profile: the route resolves to OpenAI with no problem, and no key is saved.
  const providers = [{ id: "openai", name: "OpenAI", configured: false }];
  const cloud = route({ isLocal: false, destination: "openai" });
  const result = dictationReadiness({ route: cloud, hotkeys: report(), canInsert: true, providers });
  assert.equal(result.state, "needs-setup");
  if (result.state !== "needs-setup") return;
  assert.equal(result.problem, "OpenAI needs a key before it can transcribe.");
  assert.match(result.detail ?? "", /no key is saved/);
  assert.deepEqual(result.fix, { view: "transcription", label: "Open Transcription" });

  // With its key it is ready, and a local route never asks about keys.
  assert.equal(dictationReadiness({ route: cloud, hotkeys: report(), canInsert: true, providers: [{ id: "openai", name: "OpenAI", configured: true }] }).state, "ready");
  assert.equal(dictationReadiness({ route: route({ isLocal: true, destination: "openai" }), hotkeys: report(), canInsert: true, providers }).state, "ready");
  // A destination that is no listed service, and no list at all, answer as before.
  assert.equal(dictationReadiness({ route: route({ isLocal: false, destination: "a server of mine" }), hotkeys: report(), canInsert: true, providers }).state, "ready");
  assert.equal(dictationReadiness({ route: cloud, hotkeys: report(), canInsert: true }).state, "ready");
});

test("a dictation binding with no accelerators is needs-setup, fixed by setting a shortcut", () => {
  const r = dictationReadiness({
    route: route(),
    hotkeys: report({
      bindings: [{ action: "dictation", accelerators: [], mode: "toggle" }],
    }),
    canInsert: true,
  });
  assert.equal(r.state, "needs-setup");
  if (r.state !== "needs-setup") return;
  assert.equal(r.problem, "No dictation shortcut is set.");
  assert.equal(r.detail, null);
  assert.deepEqual(r.fix, { view: "shortcuts", label: "Set a shortcut" });
});

test("no dictation binding at all reads the same as an empty one", () => {
  const r = dictationReadiness({
    route: route(),
    hotkeys: report({ bindings: [] }),
    canInsert: true,
  });
  assert.equal(r.state, "needs-setup");
  if (r.state !== "needs-setup") return;
  assert.equal(r.problem, "No dictation shortcut is set.");
  assert.deepEqual(r.fix, { view: "shortcuts", label: "Set a shortcut" });
});

test("the only result being not-registrable surfaces its reason, fixed by changing the shortcut", () => {
  const r = dictationReadiness({
    route: route(),
    hotkeys: report({
      registrations: [
        {
          action: "dictation",
          results: [{ state: "not-registrable", accelerator: "Control+Super", reason: "r" }],
        },
      ],
    }),
    canInsert: true,
  });
  assert.equal(r.state, "needs-setup");
  if (r.state !== "needs-setup") return;
  assert.equal(r.problem, "The dictation shortcut doesn't work on this computer.");
  assert.equal(r.detail, "r");
  assert.deepEqual(r.fix, { view: "shortcuts", label: "Change the shortcut" });
});

test("a hooked result counts as ready", () => {
  const r = dictationReadiness({
    route: route(),
    hotkeys: report({
      registrations: [
        { action: "dictation", results: [{ state: "hooked", accelerator: "Control+Super" }] },
      ],
    }),
    canInsert: true,
  });
  assert.equal(r.state, "ready");
});

test("a portal result counts as ready", () => {
  const r = dictationReadiness({
    route: route(),
    hotkeys: report({
      registrations: [
        {
          action: "dictation",
          results: [{ state: "portal", accelerator: "Control+Shift+D", granted: null }],
        },
      ],
    }),
    canInsert: true,
  });
  assert.equal(r.state, "ready");
});

test("a registrations list with no dictation entry is needs-setup rather than a crash", () => {
  const r = dictationReadiness({
    route: route(),
    hotkeys: report({ registrations: [] }),
    canInsert: true,
  });
  assert.equal(r.state, "needs-setup");
  if (r.state !== "needs-setup") return;
  assert.equal(r.problem, "The dictation shortcut doesn't work on this computer.");
  assert.equal(r.detail, null);
});

test("insertion not being supported is ready, with a caution rather than a block", () => {
  const r = dictationReadiness({ route: route(), hotkeys: report(), canInsert: false });
  assert.equal(r.state, "ready");
  if (r.state !== "ready") return;
  assert.ok(r.caution);
  assert.match(r.caution, /can't be typed into other apps/);
});

test("everything working is ready with no caution", () => {
  const r = dictationReadiness({ route: route(), hotkeys: report(), canInsert: true });
  assert.deepEqual(r, { state: "ready", caution: null });
});

/* ---------------------------------------------------------------- meetings */

const provider = (over: Partial<MeetingProviderSummary> = {}): MeetingProviderSummary => ({
  id: "local",
  name: "Local model",
  audioLeavesDevice: false,
  keyterms: { state: "unsupported" },
  sessionLimitSeconds: null,
  tokenRotation: false,
  sampleRates: [16000],
  speakerLabels: false,
  partials: true,
  languageDetection: true,
  reconnect: false,
  batchFallback: false,
  availability: { state: "available" },
  credentialSlot: null,
  summary: "runs on this machine",
  ...over,
});

const available = { state: "available", mechanism: "WASAPI loopback", device: "Speakers", unverified: false } as const;

test("no usable provider is needs-setup, naming both ways out in the visible problem", () => {
  const r = meetingReadiness({
    usable: [],
    selected: null,
    savedUnusable: null,
    languageWarning: null,
    localProblem: null,
    systemAudio: available,
  });
  assert.equal(r.state, "needs-setup");
  if (r.state !== "needs-setup") return;
  // Both paths are something to do now, so neither is folded: the fix button
  // only goes to Transcription, and the cloud path must not be the half that
  // is hidden behind Learn more.
  assert.equal(
    r.problem,
    "Meeting transcription isn't set up yet. Set up local transcription, or add an API key for a cloud service.",
  );
  // Where the key goes is the detail, not the instruction.
  assert.equal(r.detail, "API keys for cloud services are added under Settings > Providers.");
  assert.deepEqual(r.fix, { view: "transcription", label: "Set up transcription" });
});

/*
 * The case Ruling 17 exists for: somebody chose the local provider (for
 * privacy), its model server is not running, and a cloud key is stored.
 * `selectProvider` falls back to the cloud provider, so without this the page
 * said Ready over a Start that would send the meeting to the cloud.
 */
const localChosenCloudFallback = () => {
  const cloud = provider({
    id: "meeting:openai-realtime",
    name: "OpenAI Realtime",
    audioLeavesDevice: true,
  });
  const usable = [cloud];
  const { selected, savedUnusable } = selectProvider(null, "local", usable.map((p) => p.id));
  return { usable, selected, savedUnusable };
};

test("the fixture: a saved local provider that cannot be used falls back to the cloud one", () => {
  const { selected, savedUnusable } = localChosenCloudFallback();
  assert.equal(selected, "meeting:openai-realtime");
  assert.equal(savedUnusable, "local");
});

test("a saved provider that can't be used is needs-setup, even with another usable, fixed by choosing a provider", () => {
  const { usable, selected, savedUnusable } = localChosenCloudFallback();
  assert.equal(savedUnusable, "local");
  const r = meetingReadiness({
    usable,
    selected,
    savedUnusable: { name: "Local model", reason: "The local model server isn't running." },
    languageWarning: null,
    localProblem: null,
    systemAudio: available,
  });
  assert.equal(r.state, "needs-setup");
  if (r.state !== "needs-setup") return;
  assert.equal(r.problem, "Local model, the provider you chose, can't be used right now.");
  assert.equal(r.detail, "The local model server isn't running.");
  assert.deepEqual(r.fix, { view: "provider-menu", label: "Choose a provider" });
});

test("a saved provider this version doesn't offer is not named by its raw id", () => {
  // KNEG-354, item 3: "local-streaming, the provider you chose, …". The id
  // goes to the detail, behind Learn more.
  const { usable, selected } = localChosenCloudFallback();
  const r = meetingReadiness({
    usable,
    selected,
    savedUnusable: { name: null, reason: "It was saved as “local-streaming”, which this version of the app doesn't offer." },
    languageWarning: null,
    localProblem: null,
    systemAudio: available,
  });
  assert.equal(r.state, "needs-setup");
  if (r.state !== "needs-setup") return;
  assert.equal(r.problem, "The provider you chose can't be used right now.");
  assert.match(r.detail ?? "", /local-streaming/);
});

test("no usable provider outranks a saved one that can't be used, and says which one it was", () => {
  const r = meetingReadiness({
    usable: [],
    selected: null,
    savedUnusable: { name: "Local model", reason: "The local model server isn't running." },
    languageWarning: null,
    localProblem: null,
    systemAudio: available,
  });
  assert.equal(r.state, "needs-setup");
  if (r.state !== "needs-setup") return;
  assert.match(r.problem, /^Meeting transcription isn't set up yet\./);
  assert.deepEqual(r.fix, { view: "transcription", label: "Set up transcription" });
  // The saved choice is not lost from the screen: it is the first thing the
  // detail says.
  assert.equal(
    r.detail,
    "Local model, the provider you chose, can't be used right now. The local model server isn't running. API keys for cloud services are added under Settings > Providers.",
  );
});

test("a saved provider that can't be used outranks a missing language", () => {
  const { usable, selected } = localChosenCloudFallback();
  const r = meetingReadiness({
    usable,
    selected,
    savedUnusable: { name: "Local model", reason: "r" },
    languageWarning: "This provider cannot detect the language on its own.",
    localProblem: null,
    systemAudio: available,
  });
  assert.equal(r.state, "needs-setup");
  if (r.state !== "needs-setup") return;
  assert.deepEqual(r.fix, { view: "provider-menu", label: "Choose a provider" });
});

test("nothing selected among usable providers is also needs-setup", () => {
  const r = meetingReadiness({
    usable: [provider()],
    selected: null,
    savedUnusable: null,
    languageWarning: null,
    localProblem: null,
    systemAudio: available,
  });
  assert.equal(r.state, "needs-setup");
});

test("a language warning is needs-setup, fixed by choosing a language, with the warning as detail", () => {
  const r = meetingReadiness({
    usable: [provider()],
    selected: "local",
    savedUnusable: null,
    languageWarning: "This provider cannot detect the language on its own.",
    localProblem: null,
    systemAudio: available,
  });
  assert.equal(r.state, "needs-setup");
  if (r.state !== "needs-setup") return;
  assert.equal(r.problem, "Choose a meeting language first.");
  assert.equal(r.detail, "This provider cannot detect the language on its own.");
  assert.deepEqual(r.fix, { view: "language-menu", label: "Choose a language" });
});

test("system audio not being available is ready, with a caution", () => {
  const r = meetingReadiness({
    usable: [provider()],
    selected: "local",
    savedUnusable: null,
    languageWarning: null,
    localProblem: null,
    systemAudio: { state: "unsupported", reason: "no such mechanism on this platform" },
  });
  assert.equal(r.state, "ready");
  if (r.state !== "ready") return;
  assert.ok(r.caution);
  assert.match(r.caution, /Only your microphone/);
});

test("everything fine is ready with no caution", () => {
  const r = meetingReadiness({
    usable: [provider()],
    selected: "local",
    savedUnusable: null,
    languageWarning: null,
    localProblem: null,
    systemAudio: available,
  });
  assert.deepEqual(r, { state: "ready", caution: null });
});

/* ------------------------------- a local provider that can't start (KNEG-387) */

const whisper = (over: { port?: number | null; serverInstalled?: boolean; installed?: boolean[] } = {}) => ({
  port: over.port ?? null,
  serverInstalled: over.serverInstalled ?? true,
  models: (over.installed ?? [false, true]).map((installed, index) => ({ id: `model-${index}`, installed })),
});

const sherpa = (over: { cliInstalled?: boolean; selectedStream?: string | null; installed?: boolean; streamBindsLoopback?: boolean | null } = {}) => ({
  cliInstalled: over.cliInstalled ?? true,
  selectedStream: over.selectedStream === undefined ? "nemotron" : over.selectedStream,
  models: [
    { id: "parakeet", installed: true },
    { id: "nemotron", installed: over.installed ?? true },
  ],
  streamBindsLoopback: over.streamBindsLoopback === undefined ? true : over.streamBindsLoopback,
});

test("the local provider can start when its server runs, or when a model is installed for Start to load", () => {
  assert.equal(localProviderProblem("local", whisper({ port: 8178, installed: [false] }), null), null);
  assert.equal(localProviderProblem("local", whisper(), null), null);
});

test("the local provider with no model installed, or no engine, says so before Start", () => {
  for (const status of [whisper({ installed: [false, false] }), whisper({ serverInstalled: false })]) {
    assert.deepEqual(localProviderProblem("local", status, null), {
      problem: "Local transcription needs a model installed on this computer first.",
      detail: "Install one under Settings > Transcription > Local. Start then loads it.",
    });
  }
});

test("a model chosen for meetings has to be installed itself, because Start loads that one", () => {
  const notInstalled = {
    problem: "The model chosen for meetings isn't installed.",
    detail: "Install it under Settings > Transcription > Local, or choose another model.",
  };
  // Another model being installed, or running, doesn't help: Start wouldn't use it.
  assert.deepEqual(localProviderProblem("local", whisper(), null, "model-0"), notInstalled);
  assert.deepEqual(localProviderProblem("local", whisper({ port: 8178 }), null, "model-0"), notInstalled);
  assert.deepEqual(localProviderProblem("local", whisper(), null, "retired"), notInstalled);
  assert.equal(localProviderProblem("local", whisper(), null, "model-1"), null);
  // Automatic, and a provider it isn't for, ask nothing of it.
  assert.equal(localProviderProblem("local", whisper(), null, null), null);
  assert.equal(localProviderProblem("local-streaming", whisper(), sherpa(), "retired"), null);
});

test("local streaming needs a streaming model chosen, downloaded, and its runtime installed", () => {
  assert.equal(localProviderProblem("local-streaming", null, sherpa()), null);
  assert.deepEqual(localProviderProblem("local-streaming", null, sherpa({ selectedStream: null })), {
    problem: "Choose a streaming model for live meetings first.",
    detail: "It runs on this computer and is chosen under Settings > Transcription > Local.",
  });
  for (const status of [sherpa({ installed: false }), sherpa({ cliInstalled: false })]) {
    assert.deepEqual(localProviderProblem("local-streaming", null, status), {
      problem: "The streaming model for live meetings isn't downloaded yet.",
      detail: "Download it under Settings > Transcription > Local.",
    });
  }
});

test("local streaming whose server can't be kept to this computer says so before Start (KNEG-485)", () => {
  // The pinned server listens on every interface, so Start would be refused (KNEG-484).
  // Nothing in Transcription fixes it, so the way out is another provider.
  assert.deepEqual(localProviderProblem("local-streaming", null, sherpa({ streamBindsLoopback: false })), {
    problem: "The streaming server for live meetings can't be limited to this computer.",
    detail: "This version of it listens on the network, so it isn't started. Choose Local windowed model or a cloud provider for live meetings.",
    fix: "provider-menu",
  });
  // Not known -- no server installed yet, or its help could not be read -- holds nothing:
  // Start's own refusal is still there.
  assert.equal(localProviderProblem("local-streaming", null, sherpa({ streamBindsLoopback: null })), null);
  assert.equal(localProviderProblem("local-streaming", null, sherpa({ streamBindsLoopback: true })), null);
  // A model still to download is said first: that is the next thing to do.
  assert.equal(
    localProviderProblem("local-streaming", null, sherpa({ installed: false, streamBindsLoopback: false }))?.problem,
    "The streaming model for live meetings isn't downloaded yet.",
  );
});

test("what can't be known yet, and cloud providers, never hold Start", () => {
  // Before the statuses arrive, Start's own error is still there to say what went wrong.
  assert.equal(localProviderProblem("local", null, null), null);
  assert.equal(localProviderProblem("local-streaming", null, null), null);
  assert.equal(
    localProviderProblem("meeting:deepgram", whisper({ installed: [false] }), sherpa({ selectedStream: null })),
    null,
  );
  assert.equal(localProviderProblem(null, whisper({ installed: [false] }), null), null);
});

test("a local provider that can't start is needs-setup, fixed in Transcription, after a saved choice and before the language", () => {
  const localProblem = {
    problem: "Local transcription needs a model installed on this computer first.",
    detail: "Install one.",
  };
  const r = meetingReadiness({
    usable: [provider()],
    selected: "local",
    savedUnusable: null,
    languageWarning: "This provider cannot detect the language on its own.",
    localProblem,
    systemAudio: available,
  });
  assert.deepEqual(r, {
    state: "needs-setup",
    problem: localProblem.problem,
    detail: localProblem.detail,
    fix: { view: "transcription", label: "Set up local transcription" },
  });
  const saved = meetingReadiness({
    usable: [provider()],
    selected: "local",
    savedUnusable: { name: "Deepgram", reason: "Needs a key." },
    languageWarning: null,
    localProblem,
    systemAudio: available,
  });
  assert.equal(saved.state === "needs-setup" && saved.fix.view, "provider-menu");
});

test("a local provider whose way out is another provider offers the provider menu, not Transcription (KNEG-485)", () => {
  const localProblem = { problem: "The streaming server can't be limited.", detail: "Choose another.", fix: "provider-menu" as const };
  const r = meetingReadiness({
    usable: [provider()],
    selected: "local-streaming",
    savedUnusable: null,
    languageWarning: null,
    localProblem,
    systemAudio: available,
  });
  assert.deepEqual(r, {
    state: "needs-setup",
    problem: localProblem.problem,
    detail: localProblem.detail,
    fix: { view: "provider-menu", label: "Choose another provider" },
  });
});
