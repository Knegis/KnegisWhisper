import assert from "node:assert/strict";
import test from "node:test";

import { dictationReadiness, meetingReadiness } from "../src/lib/readiness.js";
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
  summary: "runs on this machine",
  ...over,
});

const available = { state: "available", mechanism: "WASAPI loopback", device: "Speakers", unverified: false } as const;

test("no usable provider is needs-setup, fixed by setting up transcription", () => {
  const r = meetingReadiness({
    usable: [],
    selected: null,
    languageWarning: null,
    systemAudio: available,
  });
  assert.equal(r.state, "needs-setup");
  if (r.state !== "needs-setup") return;
  assert.equal(r.problem, "Meeting transcription isn't set up yet.");
  assert.deepEqual(r.fix, { view: "transcription", label: "Set up transcription" });
});

test("nothing selected among usable providers is also needs-setup", () => {
  const r = meetingReadiness({
    usable: [provider()],
    selected: null,
    languageWarning: null,
    systemAudio: available,
  });
  assert.equal(r.state, "needs-setup");
});

test("a language warning is needs-setup, fixed by opening meeting settings, with the warning as detail", () => {
  const r = meetingReadiness({
    usable: [provider()],
    selected: "local",
    languageWarning: "This provider cannot detect the language on its own.",
    systemAudio: available,
  });
  assert.equal(r.state, "needs-setup");
  if (r.state !== "needs-setup") return;
  assert.equal(r.problem, "Choose a meeting language first.");
  assert.equal(r.detail, "This provider cannot detect the language on its own.");
  assert.deepEqual(r.fix, { view: "meeting-settings", label: "Open meeting settings" });
});

test("system audio not being available is ready, with a caution", () => {
  const r = meetingReadiness({
    usable: [provider()],
    selected: "local",
    languageWarning: null,
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
    languageWarning: null,
    systemAudio: available,
  });
  assert.deepEqual(r, { state: "ready", caution: null });
});
