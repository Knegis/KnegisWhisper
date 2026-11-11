let latestImportSnapshot: import("../generated/ipc-types").ImportSnapshot | null = null;
/**
 * Fixture data for running the UI in a plain browser, with no Rust behind it.
 *
 * `Shell` gates every screen on `getDiagnostics()` resolving, and `invoke` is
 * `window.__TAURI_INTERNALS__.invoke` — an object that does not exist outside a
 * Tauri webview. So `npm run dev` on its own renders the `<h1>` and an error,
 * and nothing else. This module fills that gap so the nine views, the overlay
 * and the meeting notification can be worked on with Vite's HMR instead of a
 * full Rust rebuild.
 *
 * # Why this file and not `lib/ipc.ts`
 *
 * The wrappers stay untouched. A dev branch inside `ipc.ts` would put a second
 * code path in the one file whose job is to be the single greppable list of
 * what the frontend can reach, and the stub would be a place for a real command
 * to hide. Here the interception happens *below* the wrappers, at Tauri's own
 * mock seam, so `ipc.ts` is exercised exactly as it is in production.
 *
 * # This is not a second source of truth
 *
 * Every fixture is annotated with its generated type, so `tsc` fails when a
 * Rust struct changes shape and a fixture does not. That is deliberate: the
 * defect this codebase already paid for once was a TypeScript value quietly
 * disagreeing with the Rust it claimed to describe. A fixture is allowed to be
 * *wrong about the data* — it is invented — but it is not allowed to be wrong
 * about the shape.
 *
 * Loaded only when `import.meta.env.DEV` is set and no Tauri host is present,
 * via a dynamic import in `main.tsx`, so none of it reaches a bundled build.
 */

import { emit } from "@tauri-apps/api/event";
import { mockIPC, mockWindows, mockConvertFileSrc } from "@tauri-apps/api/mocks";

import type {
  Accelerator,
  AudioLevel,
  Binding,
  CalendarStatus,
  CleanupRoute,
  CleanupSettings,
  CliStatus,
  ConfigReview,
  ConnectOutcome,
  CredentialSlotStatus,
  McpStatus,
  MediaImportResult,
  SherpaStatus,
  SidecarStatus,
  SyncOutcome,
  Diagnostics,
  DiarizationStatus,
  ExportedTranscript,
  Folder,
  Format,
  HotkeyAction,
  HotkeyChange,
  HotkeyMode,
  HotkeyReport,
  ImportReport,
  InputDevice,
  InstallProgress,
  NoteRecordingState,
  LevelSession,
  LevelSource,
  Lists,
  LocalApiStatus,
  LocalLlmProgress,
  LocalLlmStatus,
  LocalStatus,
  MeetingOutcome,
  MeetingProviderSummary,
  MeetingState,
  MeetingStatus,
  MicrophoneCheck,
  NewNoteRequest,
  Note,
  NoteRecording,
  NoteUpdateRequest,
  Offer,
  Outcome,
  ParticipantRequest,
  Person,
  ProbeReport,
  ProviderSummary,
  ReasoningStatus,
  RecoverableRecording,
  RouteReport,
  SearchHit,
  SessionState,
  Sources,
  SummarySettings,
  SummaryState,
  TranscriptEvent,
  TranscriptRow,
  Transcription,
  TrashSummary,
  UpdateCheckResult,
} from "../generated/ipc-types";
// `OverlayPreferences` is hand-written in `ipc.ts`, not generated: Rust's
// `persistent_overlay::Snapshot` has no `ts_rs` derive. A type-only import, so
// this stays below the wrappers as the module doc above requires.
import type { OverlayPreferences } from "./ipc";
import { FIXTURE_RECORDING_SECONDS, toneWav } from "./fixtureWav";
import {
  LONG_TRANSCRIPT_SECONDS,
  LONG_TRANSCRIPT_SEGMENTS,
  longTranscriptRows,
  wordedSegment,
  type StoredSegment,
} from "./fixtureTranscript";
import { emailKey, HAS_SPACE, readParticipants, trimSpace } from "./people";
import { summaryConfigured } from "./summaryConfigured";

/* ------------------------------------------------------------- capabilities */

const DIAGNOSTICS: Diagnostics = {
  appVersion: "0.1.0-dev",
  os: "windows",
  arch: "x86_64",
  webviewVersion: "121.0.2277.128",
  credentialStore: {
    state: "available",
    backend: "Windows Credential Manager",
  },
  overlay: {
    created: true,
    captureExclusion: {
      state: "active",
      verifiedBy: {
        kind: "read-back",
        api: "GetWindowDisplayAffinity",
        value: "WDA_EXCLUDEFROMCAPTURE",
      },
    },
    nonActivating: { state: "active", verifiedBy: "WS_EX_NOACTIVATE is set" },
    recordingIndicatorVisible: false,
    displayServer: "Windows DWM",
  },
  allowedEgressPurposes: [
    "transcription",
    "reasoning",
    "provider-catalog",
    "model-download",
  ],
  modelCache: {
    path: "C:\\ProgramData\\KnegisWhisper\\models",
    // Varied on purpose: this is the interesting branch, and the sentence the
    // screen has for it is otherwise never seen in dev.
    fellBack: true,
  },
  modifierOnlyShortcuts: {
    state: "available",
    mechanism: "WH_KEYBOARD_LL low-level hook",
  },
  systemAudio: {
    state: "available",
    mechanism: "WASAPI loopback",
    device: "Speakers (Realtek(R) Audio)",
    unverified: false,
  },
};

/* ----------------------------------------------------------------- hotkeys */

const standard = (accelerator: string): Accelerator => ({
  kind: "standard",
  accelerator,
});

const BINDINGS: Binding[] = [
  {
    action: "dictation",
    accelerators: [standard("Control+Shift+D")],
    mode: "toggle",
  },
  {
    action: "chat-agent",
    accelerators: [
      {
        kind: "modifier-only",
        accelerator: "Control+Super",
        modifiers: ["Control", "Super"],
      },
    ],
    mode: "toggle",
  },
  {
    // `Control+Alt` rather than another tidy chord, because this is the one
    // that renders the caution below -- and a fixture that never shows a
    // warning is a fixture nobody can check the warning against. On a Nordic
    // or German layout this really does start dictation every time you type
    // `@` (KNEG-37).
    action: "voice-agent",
    accelerators: [
      {
        kind: "modifier-only",
        accelerator: "Control+Alt",
        modifiers: ["Control", "Alt"],
      },
    ],
    // The one fixture that is held rather than toggled, so the mode control and
    // the sentence under it are visible in dev mode without anyone changing a
    // setting. A fixture where every row reads the same is a fixture that
    // cannot show a difference.
    mode: "hold",
  },
  {
    action: "translation",
    accelerators: [
      { kind: "invalid", input: "Ctrl+Shft+T", reason: "unknown key `Shft`" },
    ],
    mode: "toggle",
  },
  {
    action: "meeting",
    accelerators: [
      standard("Control+Shift+V"),
      // A mouse binding, for the same reason the `Control+Alt` row above is a
      // caution: the third classification has to be visible somewhere without
      // anybody owning a five-button mouse.
      { kind: "mouse-button", accelerator: "Mouse4", alsoDoes: "Back" },
    ],
    mode: "toggle",
  },
];

const HOTKEYS: HotkeyReport = {
  // One report, for dictation, because it is the one a screen reads a verdict
  // from: `dictationReadiness` asks whether any dictation result is live. This
  // was empty, and an empty list reads as "Needs setup" -- which the real
  // backend never sends for a registrable binding, since `register_all`
  // reports every binding in every mode (see `lib/readiness.ts`). The other
  // four are left out rather than guessed at; nothing renders them unless the
  // portal is on, and it is off here.
  registrations: [
    {
      action: "dictation",
      results: [{ state: "registered", accelerator: "Control+Shift+D" }],
    },
  ],
  portalSupported: false,
  portalEnabled: false,
  bindings: BINDINGS,
  conflicts: [
    { accelerator: "Control+Shift+V", actions: ["voice-agent", "meeting"] },
  ],
  unhandled: [
    {
      action: "voice-agent",
      reason: "no voice-agent client is built into this release",
    },
  ],
  // The sentence `hotkey::cautions` builds when the layout was read and found
  // to have AltGr -- the Present arm, naming the character, because that is
  // what a Nordic or German user gets and the one worth being able to see
  // while styling this.
  cautions: [
    {
      action: "voice-agent",
      accelerator: "Control+Alt",
      reason:
        "Control+Alt also fires when you press AltGr. This layout types '@' with Control and Alt together, and Windows sends left Control and right Alt for AltGr -- which completes this chord, with nothing downstream able to tell that apart from you pressing it deliberately.",
    },
  ],
};

/* --------------------------------------------------------------- providers */

/*
 * Four of the registry's seven transcription providers, which is all
 * `providers` lists: Deepgram and AssemblyAI were here, and are meeting
 * streaming providers with no entry in `transcription` -- so Rust never lists
 * them here, and `probe_configured`, which walks the same list, never tests
 * them. Mistral took their place in KNEG-363 as a stored key whose test fails.
 */
const PROVIDERS: ProviderSummary[] = [
  {
    id: "openai",
    name: "OpenAI",
    credentialSlot: "openai",
    configured: true,
    modelCount: 3,
    models: ["whisper-1", "gpt-4o-transcribe", "gpt-4o-mini-transcribe"],
    dispatch: { state: "ready" },
  },
  {
    id: "mistral",
    name: "Mistral",
    credentialSlot: "mistral",
    configured: true,
    modelCount: 1,
    models: ["voxtral-mini-latest"],
    dispatch: { state: "ready" },
  },
  {
    id: "groq",
    name: "Groq",
    credentialSlot: "groq",
    configured: false,
    modelCount: 1,
    models: ["whisper-large-v3-turbo"],
    dispatch: {
      state: "needs-client",
      reason: "no Groq transcription client is built yet",
    },
  },
  {
    id: "corti",
    name: "Corti",
    credentialSlot: "corti",
    configured: false,
    modelCount: 1,
    models: ["premier"],
    dispatch: {
      state: "unresolvable",
      reason: "Corti needs an environment and a tenant before it can resolve",
    },
  },
];

/*
 * Thirteen of the registry's twenty-one slots, in its order, with its slot ids,
 * names, kinds, `usedFor` and `feature` (`config/provider-registry.json`,
 * `credentials`). The ids used to be `provider:openai` and the like, which no
 * registry slot is: `PROVIDERS[].credentialSlot` above and the probe's
 * provider ids below are matched against these, so the three change together
 * or a row cannot find its dispatch or its test (KNEG-363).
 *
 * `stored` is invented, and `provider_credential_set` below moves it, so a
 * key saved on Providers flips its row. One `usedFor` is invented too: Groq
 * is `["transcription", "reasoning"]` in the registry and is speech alone
 * here, because `PROVIDERS` above invents a missing Groq client and a slot
 * with another use keeps its + (`lib/providerRows.ts`) -- this is the fixture
 * that shows "Not supported yet".
 */
const CREDENTIAL_SLOTS: CredentialSlotStatus[] = [
  { slot: "openai", name: "OpenAI", kind: "provider", usedFor: ["transcription", "reasoning"], feature: null, stored: true },
  { slot: "anthropic", name: "Anthropic", kind: "provider", usedFor: ["reasoning"], feature: null, stored: false },
  { slot: "groq", name: "Groq", kind: "provider", usedFor: ["transcription"], feature: null, stored: false },
  { slot: "mistral", name: "Mistral", kind: "provider", usedFor: ["transcription"], feature: null, stored: true },
  { slot: "corti", name: "Corti", kind: "provider", usedFor: ["transcription", "reasoning"], feature: null, stored: false },
  { slot: "note-formatting-custom", name: "Note formatting", kind: "custom-endpoint", usedFor: [], feature: "note-formatting", stored: false },
  { slot: "translation-custom", name: "Translation", kind: "custom-endpoint", usedFor: [], feature: "translation", stored: true },
  { slot: "bedrock", name: "AWS Bedrock", kind: "enterprise", usedFor: [], feature: "bedrock", stored: true },
  { slot: "azure", name: "Azure OpenAI", kind: "enterprise", usedFor: [], feature: "azure", stored: false },
  { slot: "vertex", name: "GCP Vertex AI", kind: "enterprise", usedFor: [], feature: "vertex", stored: true },
  { slot: "assemblyai", name: "AssemblyAI", kind: "provider", usedFor: [], feature: null, stored: false },
  { slot: "deepgram", name: "Deepgram", kind: "provider", usedFor: [], feature: null, stored: true },
  { slot: "custom", name: "Your own server", kind: "custom-endpoint", usedFor: ["reasoning"], feature: null, stored: false },
];

/*
 * Only providers `PROVIDERS` lists, because `probe_configured` walks the same
 * registry list: the Deepgram and AssemblyAI results that were here are ones
 * Rust cannot send. One pass (OpenAI), one failure (Mistral), and the two
 * kinds of skip.
 */
const PROBE_REPORT: ProbeReport = {
  results: [
    {
      provider: "openai",
      kind: "catalog",
      outcome: { outcome: "pass", detail: "48 models listed in 240 ms" },
    },
    {
      provider: "openai",
      kind: "voice",
      outcome: {
        outcome: "pass",
        detail: "transcribed the tone clip as `one two three`",
      },
    },
    {
      provider: "mistral",
      kind: "voice",
      outcome: {
        outcome: "fail",
        detail: "401 Unauthorized — the stored key was rejected",
      },
    },
    {
      provider: "corti",
      kind: "catalog",
      outcome: { outcome: "skipped", reason: "no-credential" },
    },
    {
      provider: "groq",
      kind: "voice",
      outcome: {
        outcome: "skipped",
        reason: "not-supported",
        detail: "no Groq transcription client is built yet",
      },
    },
  ],
  audio: "tone",
};

/* --------------------------------------------------------------- reasoning */

const CLEANUP_SETTINGS: CleanupSettings = {
  enabled: true,
  provider: "openai",
  model: "gpt-4o-mini",
  selfHostedUrl: null,
  enterprise: false,
};

const CLEANUP_ROUTE: CleanupRoute = {
  state: "ready",
  destination: "openai",
  model: "gpt-4o-mini",
  leavesDevice: true,
  caveat: null,
};

const REASONING: ReasoningStatus = {
  settings: CLEANUP_SETTINGS,
  route: CLEANUP_ROUTE,
  providers: [
    {
      id: "openai",
      name: "OpenAI",
      credentialSlot: "openai",
      configured: true,
      enterprise: false,
      dispatch: { state: "ready" },
      models: [
        {
          id: "gpt-4o-mini",
          name: "GPT-4o mini",
          tokenParam: "max_completion_tokens",
          supportsThinking: false,
          supportsVision: true,
        },
        {
          id: "gpt-4o",
          name: "GPT-4o",
          tokenParam: "max_completion_tokens",
          supportsThinking: false,
          supportsVision: true,
        },
      ],
    },
    {
      id: "anthropic",
      name: "Anthropic",
      credentialSlot: "anthropic",
      configured: false,
      enterprise: false,
      dispatch: {
        state: "needs-config",
        reason: "no API key is stored for Anthropic",
      },
      models: [
        {
          id: "claude-sonnet-5",
          name: "Claude Sonnet 5",
          tokenParam: "max_tokens",
          supportsThinking: true,
          supportsVision: true,
        },
      ],
    },
    {
      id: "local",
      name: "This computer",
      credentialSlot: null,
      // As Rust reports it: `configured` is whether a model is running, the
      // same fact `needs-sidecar` says is not true.
      configured: false,
      enterprise: false,
      dispatch: {
        state: "needs-sidecar",
        reason: "no local model is running. Start one under Settings > Local text models.",
      },
      models: [
        {
          id: "qwen2.5-7b-instruct",
          name: "Qwen2.5 7B Instruct",
          tokenParam: "max_tokens",
          supportsThinking: false,
          supportsVision: false,
        },
      ],
    },
    {
      id: "bedrock",
      name: "AWS Bedrock",
      credentialSlot: null,
      configured: true,
      enterprise: true,
      dispatch: { state: "ready" },
      models: [
        {
          id: "anthropic.claude-3-5-sonnet-20241022-v2:0",
          name: "Claude 3.5 Sonnet (Bedrock)",
          tokenParam: "max_tokens",
          supportsThinking: false,
          supportsVision: true,
        },
      ],
    },
  ],
  enterprise: [
    {
      provider: "bedrock",
      name: "AWS Bedrock",
      configured: true,
      fields: [
        { name: "region", filled: true },
        { name: "accessKeyId", filled: true },
        { name: "secretAccessKey", filled: true },
        { name: "sessionToken", filled: false },
      ],
      problem: null,
    },
    {
      provider: "azure",
      name: "Azure OpenAI",
      configured: false,
      fields: [],
      problem: null,
    },
    {
      provider: "vertex",
      name: "Google Vertex AI",
      configured: true,
      fields: [
        { name: "projectId", filled: true },
        { name: "region", filled: true },
        { name: "serviceAccountJson", filled: false },
      ],
      problem: "the stored configuration is missing its service-account key",
    },
  ],
};

/* ---------------------------------------------------------------- meetings */

const MEETING_PROVIDERS: MeetingProviderSummary[] = [
  {
    id: "meeting:deepgram",
    name: "Deepgram",
    audioLeavesDevice: true,
    keyterms: { state: "supported", max: 100, wire: "repeated-query-param" },
    sessionLimitSeconds: null,
    tokenRotation: false,
    sampleRates: [8000, 16000, 24000, 48000],
    speakerLabels: true,
    partials: true,
    languageDetection: true,
    reconnect: true,
    batchFallback: true,
    availability: { state: "available" },
    // As `credential_slot(client(id))` gives it in Rust; the ids here are
    // fixture ids, so the slot is written out rather than derived (KNEG-373).
    credentialSlot: "deepgram",
    summary:
      "Streams audio to Deepgram. Speaker labels and live partials, no session limit.",
  },
  {
    id: "meeting:openai-realtime",
    name: "OpenAI Realtime",
    audioLeavesDevice: true,
    keyterms: { state: "supported", max: null, wire: "session-config" },
    sessionLimitSeconds: 1800,
    tokenRotation: true,
    sampleRates: [24000],
    speakerLabels: false,
    partials: true,
    languageDetection: false,
    reconnect: true,
    batchFallback: true,
    availability: { state: "available" },
    credentialSlot: "openai",
    summary:
      "Streams audio to OpenAI. No speaker labels, and the session is capped at 30 minutes.",
  },
  {
    id: "meeting:assemblyai",
    name: "AssemblyAI",
    audioLeavesDevice: true,
    keyterms: { state: "supported", max: 1000, wire: "json-query-param" },
    sessionLimitSeconds: null,
    tokenRotation: true,
    sampleRates: [16000],
    speakerLabels: true,
    partials: true,
    languageDetection: false,
    reconnect: false,
    batchFallback: false,
    // Was `no-client` with "not built yet", which stopped being true when the
    // client landed in chunk E. Now the fixture for the state added after the
    // 2026-08-27 bench run: a client exists and no key is stored.
    availability: { state: "needs-credential", slot: "assemblyai" },
    credentialSlot: "assemblyai",
    summary: "Streams audio to AssemblyAI. Speaker labels and live partials.",
  },
  {
    id: "meeting:local",
    name: "Local model",
    audioLeavesDevice: false,
    keyterms: { state: "unsupported" },
    sessionLimitSeconds: null,
    tokenRotation: false,
    sampleRates: [16000],
    speakerLabels: false,
    partials: false,
    languageDetection: false,
    reconnect: true,
    batchFallback: true,
    // `no-client`, not `declined`: the Meetings page's provider menu leaves a
    // refused provider out (it is a policy, not something to act on), and the
    // fixture wants the "On this computer" group visible with a greyed row, the
    // saved-but-unusable case the page has to explain.
    availability: {
      state: "no-client",
      reason: "No streaming client for this provider has been written yet.",
    },
    credentialSlot: null,
    summary: "Audio never leaves this machine. No partials, no speaker labels.",
  },
];

const DICTATION_LANGUAGE_CODES = ["sv", "en", "da", "no", "fi", "de", "fr", "es"];

let meetingStatus: MeetingStatus = {
  finalisation: { revision: 0, progress: null },
  state: "idle",
  transcriptFailure: null,
  provider: null,
  plan: null,
  elapsedSeconds: 0,
  sources: null,
  systemAudio: DIAGNOSTICS.systemAudio,
  // What this machine reported on 2026-09-01, so the harness shows the state a
  // Windows user actually has rather than an invented one.
  microphone: {
    state: "available",
    mechanism: "IAudioSessionManager2 over every active capture endpoint",
    endpoints: 1,
  },
  detected: [
    // Teams open and idle: the case that was indistinguishable from a call
    // until the microphone signal, and is now the one that must NOT offer.
    {
      provider: "meeting-detect:teams",
      name: "Microsoft Teams",
      evidence: "ms-teams.exe",
      inCall: false,
      signal: "none",
    },
    {
      provider: "meeting-detect:zoom",
      name: "Zoom",
      evidence: "CptHost.exe",
      inCall: true,
      signal: "call-process",
    },
  ],
  offer: null,
  promptOnDetect: true,
  notificationExcluded: {
    state: "unsupported",
    reason: "this platform cannot exclude a window from screen capture",
  },
  notificationActivatable: null,
};

let meetingLanguage = "en";
let meetingLocalModel: string | null = null;
let meetingProvider: string | null = null;
/** Settings › Meetings' "Delete each recording once its transcript is saved", off as in Rust (KNEG-409). */
let meetingDeleteRecordings = false;
/** When the fixture meeting started, which names its note when it has no name. */
let meetingStartedAt: Date | null = null;

/** A name typed before Start, tidied as Rust's `clean_title` tidies it (`meeting.rs`). */
function fixtureMeetingTitle(given: string | null | undefined): string | null {
  const line = (given ?? "").split(/[\r\n]/).map((part) => part.trim()).filter((part) => part !== "").join(" ");
  const title = [...line].slice(0, 200).join("").trimEnd();
  return title === "" ? null : title;
}

/** Rust's `note_title`: the name, or "Meeting — " and when it started, local time. */
function fixtureNoteTitle(given: string | null, started: Date): string {
  const pad = (n: number) => String(n).padStart(2, "0");
  const when = `${started.getFullYear()}-${pad(started.getMonth() + 1)}-${pad(started.getDate())} ${pad(started.getHours())}:${pad(started.getMinutes())}`;
  return given ?? `Meeting — ${when}`;
}

/** Local structured disposition, separate from a note's exportable prose. */
const recordingStates = new Map<number, NoteRecordingState>();

/**
 * The fixture meeting stopping: idle again, and what Rust's `keep` would
 * have made of it -- a new meeting note under the plan's name or when it
 * started, in the plan's folder, or in none with `folderMissing` when that
 * folder was deleted while it ran (KNEG-365). It has two transcript rows and
 * the recording attached, as a whole-recording pass leaves it, unless the
 * plan says to delete the recording once saved: then it has no recording,
 * its details say why, and the outcome says `deleted` (KNEG-409). That pass
 * runs on this computer, and a provider whose audio leaves it is named for
 * the live captions, as Rust's `transcribed_by` names it (KNEG-368).
 */
function fixtureStop(): MeetingOutcome {
  const provider = meetingStatus.provider;
  const plan = meetingStatus.plan ?? { title: null, folderId: null, deleteRecording: false, liveProviderId: null };
  const startedAt = meetingStartedAt ?? new Date();
  meetingStartedAt = null;
  meetingStatus = {
    ...meetingStatus,
    state: "idle",
    provider: null,
    plan: null,
    elapsedSeconds: 0,
    sources: null,
    offer: null,
  };
  // OpenAI is the provider whose capped sessions reconnect. Its fixture uses
  // the quiet outcome so the second public shape carrying `reconnects` can be
  // inspected in the browser; every other provider keeps the saved-note path.
  if (provider === "meeting:openai-realtime") {
    return {
      outcome: "nothing-transcribed",
      recordingPath:
        "C:\\Users\\dev\\AppData\\Roaming\\KnegisWhisper\\recordings\\2026-08-26T09-00.wav",
      mic: "silence",
      system: "nothing",
      reconnects: 1,
    };
  }
  const folderId = folders.some((f) => f.id === plan.folderId) ? plan.folderId : null;
  const noteId = nextNoteId++;
  const segments = [
    { text: "Let's start with the release.", timestamp: 4, endTimestamp: 9 },
    { text: "The installer is signed now.", timestamp: 10, endTimestamp: 15 },
  ];
  const live = MEETING_PROVIDERS.find((summary) => summary.id === provider);
  notes = [
    fixtureNote({
      id: noteId,
      title: fixtureNoteTitle(plan.title, startedAt),
      noteType: "meeting",
      recordingDetails: null,
      folderId,
      transcript: JSON.stringify(segments),
      audioDurationSeconds: 1847,
      transcribedBy: fixtureTranscribedBy("local", live?.audioLeavesDevice ? (live.id === "meeting:deepgram" ? "Deepgram Realtime" : live.name) : null),
      createdAt: nowIso(),
      updatedAt: nowIso(),
    }),
    ...notes,
  ];
  noteTranscriptRows.set(noteId, segments.map((segment, index) => ({
    index, text: segment.text, speaker: null, speakerId: null, startSeconds: segment.timestamp, endSeconds: segment.endTimestamp, words: [],
  })));
  if (plan.deleteRecording) recordingStates.set(noteId, { status: "deleted", error: null, canRetry: false });
  if (!plan.deleteRecording) {
    noteRecordings.set(noteId, { noteId, storageKey: `fixture-meeting-${noteId}.wav`, durationSeconds: FIXTURE_RECORDING_SECONDS, originalFilename: null, createdAt: nowIso() });
  }
  return {
    outcome: "saved",
    noteId,
    durationSeconds: 1847,
    recordingPath:
      "C:\\Users\\dev\\AppData\\Roaming\\KnegisWhisper\\recordings\\2026-08-26T09-00.wav",
    gaps: 0,
    reconnects: 1,
    transcriptIsPartials: false,
    recordingAttached: true,
    folderMissing: plan.folderId !== null && folderId === null,
    recordingRemoval: plan.deleteRecording ? "deleted" : null,
    recordingRemovalError: null,
  };
}

/*
 * Empty, as a fresh profile has none; `kw.interrupted()` adds one, so the
 * Meetings panel's Interrupted section and its Recover button can be seen.
 */
let recoverable: RecoverableRecording[] = [];

/*
 * The Windows-only floating microphone, off until Settings > Dictation turns
 * it on -- `overlay_preferences`'s Rust default. `#overlay` is its own page
 * load in a plain browser, so a toggle flipped from the main window's tab
 * would not reach it; `kw.overlayFloating()` flips it from inside that tab
 * instead, over the same `overlay-preferences` event Rust emits.
 */
let overlayPreferences: OverlayPreferences = {
  supported: true,
  enabled: false,
  vertical: false,
};

/** The meeting panel's typed-notes draft, read back after a webview reload. */
let meetingNotes = "";

/*
 * Whether the meeting window answers when Stop or Discard asks it to save its
 * notes (`flushMeetingPanelNotes`). There is no meeting window behind this tab
 * -- `/#meeting` is a page load of its own -- so the `plugin:event|emit_to`
 * handler answers for it, unless `kw.meetingWindowSilent()` has made it
 * silent (KNEG-418).
 */
let meetingWindowSilent = false;

/*
 * Fake sound levels (KNEG-371), sent as Rust's publishers send them
 * (`src-tauri/src/audio_level.rs`): one `audio-level` per capture every
 * 100 ms while it runs, and a last 0 per capture when it stops. A meeting's
 * start here, as in Rust; `kw.levels` for dictation, whose recording the
 * fixtures only ever announce (`kw.dictationState`). The microphone moves
 * around 0.62 and the others around 0.3, never meeting, so the Meetings
 * page's two meters always show different values.
 */
const fakeLevels = new Map<LevelSession, { timer: ReturnType<typeof setInterval>; sources: LevelSource[] }>();

function fakeLevel(source: LevelSource, tick: number): number {
  return source === "microphone" ? 0.62 + 0.08 * Math.sin(tick / 2) : 0.3 + 0.08 * Math.sin(tick / 3 + 1);
}

function startFakeLevels(session: LevelSession, sources: LevelSource[]): void {
  stopFakeLevels(session);
  let tick = 0;
  const timer = setInterval(() => {
    tick += 1;
    for (const source of sources) {
      void emit("audio-level", { session, source, level: fakeLevel(source, tick) } satisfies AudioLevel);
    }
  }, 100);
  fakeLevels.set(session, { timer, sources });
}

function stopFakeLevels(session: LevelSession): void {
  const running = fakeLevels.get(session);
  if (running === undefined) return;
  clearInterval(running.timer);
  fakeLevels.delete(session);
  for (const source of running.sources) {
    void emit("audio-level", { session, source, level: 0 } satisfies AudioLevel);
  }
}

/*
 * The Meetings page reads this in its first load, for the control bar's line
 * about where an automatic summary sends the transcript, and Cleanup &
 * summaries reads and writes it. Rust's `SummarySettings::default()`
 * (`crates/notes-core/src/summary.rs`): off, no provider, no model -- so the
 * line is absent until Cleanup & summaries turns summaries on.
 */
let summarySettings: SummarySettings = {
  enabled: false,
  provider: "",
  model: "",
  selfHostedUrl: null,
  enterprise: false,
  language: "match",
  contextBudget: 8192,
};

/*
 * Notes' summary pane, one record per note (KNEG-348). Rust's `state`
 * (`crates/notes-core/src/summary.rs`) reads the text from the note's
 * `enhanced_content` and the rest from two tables; a note with text and no
 * record yet is a legacy, edited summary there, and is here too. Generating
 * finishes at once with placeholder text instead of queueing a job: the pane's
 * queued and processing lines are not what dev mode is for, and a job loop
 * would be a second implementation to keep in step.
 */
const summaries = new Map<number, SummaryState>();
function summaryOf(id: number): SummaryState {
  const known = summaries.get(id);
  if (known) return known;
  const note = notes.find((n) => n.id === id);
  if (!note) throw new Error("This note no longer exists.");
  const legacy = note.enhancedContent !== null;
  return {
    noteId: id, content: note.enhancedContent, revision: 0, edited: legacy, outdated: false, legacy,
    status: null, completed: 0, total: 0, error: null, language: null, generatedAt: null, model: null, references: [],
  };
}

let meetingKeyterms = ["Nordbank", "KnegisWhisper", "WASAPI", "Realtek"];

let detectLists: Lists = {
  allow: ["webex.exe", "bluejeans.exe"],
  block: ["Teams.exe"],
};

/*
 * Settings > Meetings reads this in its first load, and with no stub that load
 * threw, so the screen rendered nothing but the error -- in dev mode it never
 * could, before or after the recording moved to its own page. A fresh
 * profile's answer: neither model and no runtime, the state that shows the
 * Install buttons and the blocker line. The two models are the registry's
 * (`generated/providers.ts`).
 */
let diarization: DiarizationStatus = {
  availability: { state: "installable" },
  cliInstalled: false,
  models: [
    { id: "sherpa-onnx-pyannote-segmentation-3-0", name: "Pyannote Segmentation 3.0", role: "segmentation", sizeMb: 6.6, installed: false },
    { id: "3dspeaker-campplus-en", name: "CAM++ Speaker Embedding (English)", role: "embedding", sizeMb: 28.2, installed: false },
  ],
  enabled: false,
  ready: false,
  modelsDir: "C:\\ProgramData\\KnegisWhisper\\models\\diarization",
};

/* -------------------------------------------------------------- dictation */

const INPUT_DEVICES: InputDevice[] = [
  { name: "Microphone Array (Realtek(R) Audio)", isDefault: true },
  { name: "Headset (Jabra Evolve2 65)", isDefault: false },
  { name: "Line In (Focusrite Scarlett 2i2)", isDefault: false },
];

/**
 * The microphone `dictation_set_device` stored, which `dictation_device` reads
 * back, so the chip's menu shows the choice it made. `null` is the system
 * default, as in Rust.
 */
let dictationDevice: string | null = null;

/**
 * The microphone a capture opens, as `Capture::start` picks it: the one
 * chosen when it is connected, the system default otherwise. What a fixture
 * meeting's `Sources` names, and what the microphone check tested (KNEG-364).
 */
function fixtureOpenedMicrophone(): string | null {
  const chosen = INPUT_DEVICES.find((device) => device.name === dictationDevice);
  return (chosen ?? INPUT_DEVICES.find((device) => device.isDefault))?.name ?? null;
}

/**
 * Whether a dictation is recording, as `kw.dictationState` last announced it
 * -- the fixtures never record one -- so the microphone check can be refused
 * as Rust refuses it (KNEG-364).
 */
let fixtureDictating = false;

/** Whether the fixture microphone check is listening: one at a time, as in Rust. */
let microphoneChecking = false;

/**
 * What `dictation_set_cloud_provider` and `dictation_use_local` stored, which
 * `dictation_route` resolves, so a switch made on the Dictation page's route
 * menu or on Settings > Transcription reads back (KNEG-385). Resolved as
 * `route::resolve` resolves these two settings: the local flag first, then
 * the provider. Every provider `PROVIDERS` lists is a public endpoint, so a
 * cloud route leaves the device. The model is kept, as Rust keeps it, though
 * no read reports it.
 */
const dictationRoute = { useLocal: false, provider: "openai", model: "whisper-1" };

/**
 * A cloud route's `cloudLanguage` as `route::resolve` gives it for the common
 * case (KNEG-370): one dictation language is sent, none or two are not. The
 * languages xAI and Mistral drop, and Corti's English, are not emulated -- the
 * page reads whatever arrives and repeats none of those rules.
 */
function resolvedRoute(): RouteReport {
  const [only, ...more] = localLanguages;
  return dictationRoute.useLocal
    ? { destination: "local model", isLocal: true, leavesDevice: false, problem: null, cloudLanguage: null }
    : {
        destination: dictationRoute.provider,
        isLocal: false,
        leavesDevice: true,
        problem: null,
        cloudLanguage: only !== undefined && more.length === 0 ? only : null,
      };
}

/**
 * Dictation's languages, as `dictation_set_local_language` stored them.
 * Swedish alone to start, so the language chip -- shown on every route but a
 * Parakeet model's -- reads as a choice rather than as "Detect language", and
 * the fixtures' OpenAI route is told to expect it.
 */
let localLanguages: string[] = ["sv"];

/* ------------------------------------------------------------ local models */

const LOCAL_MODELS: LocalStatus = {
  runtimeMessage: "Local model whisper-small-q5 is ready on port 8178.",
  availability: { state: "installable" },
  serverInstalled: true,
  // `stale` rather than `current`, so the branch that only appears when the
  // installed binary has fallen behind is one somebody can look at. The
  // `unknown` branch is the commoner one in the wild -- it is what every
  // install made before records existed reports -- but a fixture can only show
  // one, and this is the one nothing else in the app renders.
  serverFreshness: {
    state: "stale",
    installed: "6cdbf1cfd7bc4d0eaa6a4a2f6cfe83b2e0cb0e2c1b9dbb1f5f0e1a4d3c2b1a09",
    expected: "c2a4b60edb11f7e11a9191ffb50929535527d4d91c9903dbe3e554583bbbc63d",
  },
  models: [
    {
      id: "whisper-large-v3-turbo-q5",
      name: "Whisper Large v3 Turbo (Q5)",
      sizeMb: 574,
      recommended: true,
      installed: true,
      verifiedBy: "digest",
    },
    {
      id: "whisper-medium-q5",
      name: "Whisper Medium (Q5)",
      sizeMb: 1530,
      recommended: false,
      installed: false,
      verifiedBy: "digest",
    },
    {
      id: "whisper-small-q5",
      name: "Whisper Small (Q5)",
      sizeMb: 488,
      recommended: false,
      installed: true,
      verifiedBy: "size",
    },
    // The registry's, so the list shows a KB model with its "slower" label
    // (KNEG-407).
    {
      id: "kb-whisper-large",
      name: "KB Whisper Large (Swedish, Q5)",
      sizeMb: 1081,
      recommended: false,
      installed: false,
      verifiedBy: "digest",
    },
    {
      id: "whisper-base-en",
      name: "Whisper Base (English)",
      sizeMb: null,
      recommended: false,
      installed: false,
      verifiedBy: "nothing",
    },
  ],
  port: 8178,
  // Deliberately disagreeing with the route, which starts on openai: the local
  // server is up with a model loaded and dictation is not using it. That is the
  // state the marked button must *not* claim, so the fixtures show it rather
  // than the easy case. `local_server_start` and `local_server_stop` move
  // this and `port`.
  runningModel: "whisper-small-q5",
  installDir: "C:\\ProgramData\\KnegisWhisper\\whisper-server",
  installDirFellBack: true,
  // Moved with `runtimeMessage` by `local_server_start` and `_stop`, as Rust
  // derives the one from the other.
  runtime: { state: "ready", model: "whisper-small-q5", port: 8178 },
};

const SHERPA: SherpaStatus = {
  availability: { state: "installable" },
  cliInstalled: true,
  // `unknown` rather than `stale`, and deliberately the opposite choice from
  // the whisper block above: this is the commoner state in the wild -- what
  // every install made before records existed reports -- and the sherpa row is
  // the one with no Update button beside it, so the sentence has to carry the
  // whole weight on its own.
  cliFreshness: {
    state: "unknown",
    why: "it was installed before this application started recording what it installs",
  },
  // One dictation model installed and selected, one installable, and two
  // meetings models installed, one of them chosen -- so the screen is worked
  // on against every state its rows have rather than the easy one. Languages
  // as the registry lists them (KNEG-330): the list's language filter reads
  // them, and Parakeet TDT's is the one with Swedish.
  models: [
    {
      id: "parakeet-unified-en-0.6b",
      name: "Parakeet Unified EN 0.6B",
      sizeMb: 630,
      language: "en",
      supportedLanguages: ["en"],
      installed: true,
      usedFor: "dictation",
    },
    {
      id: "parakeet-tdt-0.6b-v3",
      name: "Parakeet TDT 0.6B v3",
      sizeMb: 660,
      language: "multilingual",
      supportedLanguages: [
        "bg", "hr", "cs", "da", "nl", "en", "et", "fi", "fr", "de", "el", "hu", "it",
        "lv", "lt", "mt", "pl", "pt", "ro", "sk", "sl", "es", "sv", "ru", "uk",
      ],
      installed: false,
      usedFor: "dictation",
    },
    {
      // The measured one. Installed and chosen for meetings in the harness,
      // because "a streaming model that cannot be used" was the state this
      // screen showed until 2026-09-01 and a fixture still showing it would
      // keep that dead branch alive in the only place anyone looks at it.
      id: "nemotron-speech-streaming-en-0.6b",
      name: "Nemotron Speech Streaming EN 0.6B",
      sizeMb: 442,
      language: "en",
      supportedLanguages: ["en"],
      installed: true,
      usedFor: "meetings",
    },
    {
      // Installed and not chosen, so "Use for meetings" can be pressed: it
      // moves meetings and leaves dictation where it was.
      id: "nemotron-3.5-asr-streaming-0.6b",
      name: "Nemotron 3.5 ASR Streaming 0.6B",
      sizeMb: 650,
      language: "multilingual",
      supportedLanguages: ["en", "es", "fr", "it", "pt", "nl", "de", "tr", "ru", "ar", "hi", "ja", "ko", "vi", "uk"],
      installed: true,
      usedFor: "meetings",
    },
  ],
  // Selected, while the route is not local: a selection only matters once the
  // route is. `sherpa_use_model` moves it, and `local_server_start` clears it,
  // as Rust does.
  selected: "parakeet-unified-en-0.6b",
  selectedStream: "nemotron-speech-streaming-en-0.6b",
  // A server that can be kept to this computer, so the fixtures' local streaming stays startable.
  streamBindsLoopback: true,
  modelsDir: "C:\\ProgramData\\KnegisWhisper\\models\\sherpa",
};

const LOCAL_LLM: LocalLlmStatus = {
  availability: {
    state: "not-published",
    reason: "no llama.cpp build is published for windows-aarch64",
    tracking: "https://github.com/ggml-org/llama.cpp/releases",
  },
  serverInstalled: false,
  // Nothing is installed on this fixture's platform, so there is nothing to be
  // out of date. The `stale` and `unknown` branches are rendered by the two
  // blocks above; a third copy here would be a third thing to keep in step.
  serverFreshness: { state: "not-installed" },
  models: [
    {
      id: "qwen2.5-7b-instruct-q4",
      name: "Qwen2.5 7B Instruct (Q4_K_M)",
      provider: "qwen",
      sizeBytes: 4_680_000_000,
      quantization: "Q4_K_M",
      contextLength: 32768,
      recommended: true,
      supportsThinking: false,
      // Installed and still agreeing with the record written when it was
      // downloaded. The screen must not call this "verified": two integers
      // agreeing is not a hash, and the caveat on this state is what says so.
      integrity: { state: "unchanged-since-install", verifiedBy: "stated-digest" },
      // The ordinary case: thirty-two of the thirty-nine published objects are
      // checked against the publisher's stated digest, not one measured here.
      verifiedBy: "stated-digest",
      draft: {
        fileName: "qwen2.5-0.5b-instruct-q4.gguf",
        sizeBytes: 352_000_000,
        // A drafter can rot on its own, so it carries its own state. This one
        // has been written to since it was installed and its length has not
        // moved, which is the case the screen has to describe without calling
        // the file bad.
        integrity: { state: "changed", recordedBytes: 352_000_000, bytes: 352_000_000 },
        // Deliberately stronger than its model's, so the screen's "the drafter
        // differs" branch is rendered by a fixture rather than only in theory.
        verifiedBy: "digest",
      },
    },
    {
      id: "llama-3.1-8b-instruct-q4",
      name: "Llama 3.1 8B Instruct (Q4_K_M)",
      provider: "llama",
      sizeBytes: 4_920_000_000,
      quantization: "Q4_K_M",
      contextLength: 131072,
      recommended: false,
      supportsThinking: false,
      // The state every GGUF on a real machine is in the day this ships: the
      // file is there and nothing beside it says what was checked. It is
      // rendered as neither good news nor bad, which is the whole point.
      integrity: { state: "unrecorded" },
      verifiedBy: "digest",
      draft: null,
    },
    {
      id: "qwen2.5-14b-instruct-q4",
      name: "Qwen2.5 14B Instruct (Q4_K_M)",
      provider: "qwen",
      sizeBytes: 8_990_000_000,
      quantization: "Q4_K_M",
      contextLength: 32768,
      recommended: false,
      supportsThinking: false,
      // The one state here that is an accusation, and the one that was
      // impossible to render before KNEG-130: a re-hash read the whole file and
      // refused it. Until then the verdict was thrown away with the record and
      // the row fell back to `unrecorded` -- the *nothing is known* answer --
      // so the worst finding this application can make looked exactly like the
      // ordinary state of every model installed before records existed.
      integrity: {
        state: "failed-recheck",
        expected: "9f2c1d4a6b8e0357c1a9d2f4b6e8071932a5c7d9e1f3054627b8d0a2c4e6f819",
        actual: "0a1b2c3d4e5f60718293a4b5c6d7e8f90112233445566778899aabbccddeeff0",
      },
      verifiedBy: "digest",
      draft: null,
    },
    {
      id: "phi-4-mini-q8",
      name: "Phi-4 Mini (Q8_0)",
      provider: "phi",
      sizeBytes: null,
      quantization: "Q8_0",
      contextLength: null,
      recommended: false,
      supportsThinking: true,
      integrity: { state: "not-installed" },
      // The degraded case: a model added to the registry before anybody measured
      // or pinned it. Nothing ships in this state, and the screen still has to
      // say so plainly rather than leaving the row silent.
      verifiedBy: "size",
      draft: null,
    },
  ],
  port: null,
  runningModel: null,
  installDir: "C:\\ProgramData\\KnegisWhisper\\llama-server",
  idleStopMinutes: 10,
};

const LOCAL_API: LocalApiStatus = {
  blockedReason: null,
  enabled: true,
  running: true,
  port: 8749,
  discoveryPath:
    "C:\\Users\\dev\\AppData\\Roaming\\KnegisWhisper\\local-api.json",
  /* Windows paths, so the Windows state. This used to say "owner-only", which
   * only a Unix machine can produce and which both screens render with a
   * literal `0600` that could never appear beside a `C:\` path. */
  discoveryPermissions: { state: "owner-and-administrators" },
};

/* ------------------------------------------------------ cli, mcp and import */

const CLI: CliStatus = {
  blockedReason: null,
  binaryName: "knegiswhisper.exe",
  binaryLocation: {
    state: "sibling",
    path: "C:\\Program Files\\KnegisWhisper\\knegiswhisper.exe",
  },
  discoveryPath:
    "C:\\Users\\dev\\AppData\\Roaming\\KnegisWhisper\\local-api.json",
  localApiRunning: true,
  /* Same file, same machine, so the same state as `LOCAL_API` above. The two
   * screens read one file; a fixture where they disagreed would hide exactly
   * the drift `discoveryPermissionsLabel` exists to prevent. */
  discoveryPermissions: { state: "owner-and-administrators" },
};

const MCP: McpStatus = {
  blockedReason: null,
  binaryName: "knegiswhisper-mcp.exe",
  /* Not found on purpose: the screen has a sentence for a binary it cannot
   * locate, and a fixture that always finds one never shows it. */
  binaryLocation: { state: "not-found" },
  command: "knegiswhisper-mcp --stdio",
  discoveryPath:
    "C:\\Users\\dev\\AppData\\Roaming\\KnegisWhisper\\local-api.json",
  localApiRunning: true,
  // It is the same file the CLI reads, so this must stay byte-for-byte the
  // same fixture state and exercise the shared rendering block on both pages.
  discoveryPermissions: { state: "owner-and-administrators" },
};

const SIDECARS: SidecarStatus[] = [
  {
    sidecar: "ffmpeg",
    availability: { state: "installable" },
    installed: true,
  },
  {
    sidecar: "yt-dlp",
    availability: {
      state: "not-published",
      reason: "no yt-dlp build is published for windows-aarch64",
      tracking: "https://github.com/yt-dlp/yt-dlp/releases",
    },
    installed: false,
  },
];

/* ---------------------------------------------------------------- calendars */

const CALENDAR: CalendarStatus = {
  google: {
    clientIdConfigured: true,
    customClientId: "918273645-abcdef.apps.googleusercontent.com",
    clientSource: "custom",
    customGoogleSecretConfigured: false,
    appClientAvailable: false,
    account: {
      email: "axel@example.com",
      token: { state: "active" },
      calendars: [
        {
          id: "primary",
          summary: "axel@example.com",
          isPrimary: true,
          isSelected: true,
          eventCount: 42,
        },
        {
          id: "team",
          summary: "Platform team",
          isPrimary: false,
          isSelected: true,
          eventCount: 17,
        },
        {
          id: "holidays",
          summary: "Holidays in Sweden",
          isPrimary: false,
          isSelected: false,
          eventCount: 0,
        },
      ],
      lastSync: {
        account: "axel@example.com",
        snapshotVersion: 1,
        lastSuccessAt: null,
        at: "2026-08-27T07:12:00Z",
        outcome: {
          state: "completed",
          eventsUpserted: 59,
          eventsRemoved: 3,
          /* One calendar failing while the sync as a whole "completed" is the
           * case the screen has to be able to say out loud. */
          calendarErrors: ["Platform team: 403 from the Calendar API"],
        },
      },
    },
  },
  microsoft: {
    clientIdConfigured: false,
    customClientId: null,
    clientSource: "missing",
    customGoogleSecretConfigured: false,
    appClientAvailable: false,
    account: null,
  },
  apple: {
    state: "unsupported",
    reason: "the Apple calendar connector is a stub; EventKit is not wired up",
  },
};

/* ------------------------------------------------------- history and notes */

/**
 * The two history switches, held rather than returned as constants.
 *
 * The Dictation screen reloads after writing either one, so a constant would
 * make the checkbox snap back and hide the thing worth looking at here: with
 * `historyEnabled` off, "Keep the text before cleanup" is disabled and says why.
 *
 * `historyEnabled` starts on and `keepRawText` starts on. Only the first matches
 * the real default -- keeping the raw text is off in a real install, and it is on
 * here so the invented history below can show a `rawText` row.
 */
let historyEnabled = true;
let keepRawText = true;

/*
 * The shapes Rust writes, not approximations of them. `status` is only ever
 * `completed` or `failed` (`record`, `src-tauri/src/dictation.rs`); this list
 * said `ok` and `nothing-to-transcribe`, and a screen written against those
 * called every real dictation a failure. A recording that heard nothing writes
 * no row at all (`NothingToTranscribe` returns before `write_history` in
 * `crates/app-core/src/dictation.rs`), so there is no row for it here. The two
 * failed rows are the two ways `fail` writes one: a request that failed before
 * any text came back (no text, no provider), and a paste that failed after it
 * did, which keeps the cleaned text so it can be copied out of history.
 */
const HISTORY: Transcription[] = [
  {
    id: 41,
    text: "Let's move the release to Thursday so the localisation pass has time to land.",
    rawText:
      "lets move the release to thursday so the localization pass has time to land",
    createdAt: "2026-08-26T14:12:03Z",
    hasAudio: true,
    audioDurationMs: 5400,
    provider: "openai",
    model: "gpt-4o-transcribe",
    status: "completed",
    errorMessage: null,
    errorCode: null,
    routeKind: "cloud",
  },
  {
    id: 40,
    text: "Remember to revoke the old Deepgram key.",
    rawText: "remember to revoke the old deepgram key",
    createdAt: "2026-08-26T11:48:55Z",
    hasAudio: false,
    audioDurationMs: 2100,
    provider: "local",
    model: "whisper-large-v3-turbo-q5",
    status: "completed",
    errorMessage: null,
    errorCode: null,
    routeKind: "local",
  },
  {
    id: 39,
    text: "",
    rawText: null,
    createdAt: "2026-08-26T09:30:12Z",
    hasAudio: true,
    audioDurationMs: 800,
    // `fail` records no provider when nothing came back (`recovered: None`).
    provider: null,
    model: null,
    status: "failed",
    errorMessage: "401 Unauthorized — the stored key was rejected",
    errorCode: "auth",
    routeKind: "cloud",
  },
  {
    // Only the paste failed: the text and its provider are kept.
    id: 38,
    text: "Send the Nordbank notes to Jonas before Friday.",
    rawText: "send the nordbank notes to jonas before friday",
    createdAt: "2026-08-25T17:02:44Z",
    hasAudio: false,
    audioDurationMs: 3100,
    provider: "openai",
    model: "gpt-4o-transcribe",
    status: "failed",
    errorMessage: "could not deliver the paste keystroke: the focused window refused synthetic input",
    errorCode: null,
    routeKind: "cloud",
  },
];

let folders: Folder[] = [
  { id: 1, name: "Inbox", isDefault: true, sortOrder: 0 },
  { id: 2, name: "Customer calls", isDefault: false, sortOrder: 1 },
  { id: 3, name: "Internal", isDefault: false, sortOrder: 2 },
];

/**
 * Where a note's transcript was made, as Rust's `TranscribedBy` stores it
 * (KNEG-368), for the two routes the fixtures transcribe by. Rust names every
 * route (`src-tauri/src/transcribed_by.rs`); `liveName` is a meeting's
 * live-caption provider when those captions came from off this computer.
 */
const FIXTURE_ROUTES: Record<string, { provider: string; name: string; onThisComputer: boolean }> = {
  local: { provider: "local", name: "this computer", onThisComputer: true },
  openai: { provider: "openai", name: "OpenAI", onThisComputer: false },
};

function fixtureTranscribedBy(provider: string, liveName: string | null = null): string | null {
  const route = FIXTURE_ROUTES[provider];
  if (route === undefined) return null;
  return JSON.stringify(liveName === null ? route : { ...route, liveName });
}

/**
 * A fixture note: the columns it is given, and every other one empty, as
 * Rust reads a note that has none of them. The one place a column added to
 * `Note` gets its empty value for every fixture note. Its times are given,
 * not defaulted: `nowIso` is declared below the fixture notes that use this.
 */
function fixtureNote(given: Pick<Note, "id" | "title" | "noteType" | "createdAt" | "updatedAt"> & Partial<Note>): Note {
  return {
    content: "",
    recordingDetails: null,
    enhancedContent: null,
    folderId: null,
    transcript: null,
    participants: null,
    audioDurationSeconds: null,
    transcribedBy: null,
    trashedAt: null,
    ...given,
  };
}

function noteListItem(note: Note) {
  const { id, title, noteType, folderId, participants, audioDurationSeconds, createdAt, updatedAt, trashedAt } = note;
  return { id, title, noteType, folderId, participants, audioDurationSeconds, createdAt, updatedAt, trashedAt, hasTranscript: Boolean(note.transcript?.trim()) };
}

/**
 * The notes not in Trash (KNEG-401): every list, search and count reads these, as Rust's queries
 * ask for `trashed_at IS NULL`. Only `notes_get` and the Trash read the rest.
 */
function liveNotes(): Note[] {
  return notes.filter((note) => note.trashedAt === null);
}

/**
 * As Rust's commands: a note in Trash refuses an edit in plain words (`commands::IN_TRASH`), and a
 * note that isn't there is `false`. Returns whether the note is there to edit.
 */
function editable(id: number | undefined): boolean {
  const note = notes.find((n) => n.id === id);
  if (note?.trashedAt != null) throw new Error("This note is in Trash. Restore it to edit it.");
  return note !== undefined;
}

/** A fixture recording's size on disk: 16 kHz mono 16-bit PCM and its 44-byte header, as `fixtureImport` counts it. */
function fixtureRecordingBytes(recording: NoteRecording): number {
  return recording.durationSeconds === null ? 0 : Math.round(recording.durationSeconds * 32000) + 44;
}

/** As `trash::list`: the notes in Trash, most recently trashed first, with what their recordings take. */
function fixtureTrash(): TrashSummary {
  const trashed = notes
    .filter((note) => note.trashedAt !== null)
    .sort((x, y) => (y.trashedAt ?? "").localeCompare(x.trashedAt ?? "") || y.id - x.id)
    .map((note) => {
      const recording = noteRecordings.get(note.id) ?? null;
      return {
        id: note.id,
        title: note.title,
        trashedAt: note.trashedAt ?? "",
        hasRecording: recording !== null,
        recordingSeconds: recording?.durationSeconds ?? null,
        recordingBytes: recording === null ? 0 : fixtureRecordingBytes(recording),
        originalFilename: recording?.originalFilename ?? null,
      };
    });
  return { notes: trashed, recordingBytes: trashed.reduce((sum, note) => sum + note.recordingBytes, 0) };
}

/** As `recordings::empty_trash` and `delete_forever`: the note, its recording and its rows go. */
function deleteFixtureNote(id: number): void {
  notes = notes.filter((n) => n.id !== id);
  noteRecordings.delete(id);
  noteTranscriptRows.delete(id);
  speakerNames.delete(id);
}

/**
 * People on the fixture notes (KNEG-362), answered as Rust's `notes_core::people` answers. An email
 * is kept as `normalise_email` keeps it -- trimmed, ASCII lower-case, one `@` with something on
 * both sides, no whitespace, at most 254 characters -- or `null` when it isn't one.
 */
function fixtureEmail(input: string): string | null {
  const email = emailKey(input);
  if ([...email].length > 254 || HAS_SPACE.test(email)) return null;
  const [local, domain, ...rest] = email.split("@");
  return local && domain && rest.length === 0 ? email : null;
}

/**
 * As `people::tidy`, throwing its plain-word refusals: an entry sent back as it was read is
 * written back as stored, an address already on the note passes unchecked, a new one is checked,
 * and stored entries the screen can't show stay, after the people.
 */
function fixtureTidyParticipants(current: string | null, list: readonly ParticipantRequest[]): string | null {
  let stored: unknown[] = [];
  try {
    const parsed: unknown = current === null ? [] : JSON.parse(current);
    if (Array.isArray(parsed)) stored = parsed;
  } catch { /* not a list: nothing to keep */ }
  const field = (entry: unknown, key: string): string | null => {
    if (typeof entry !== "object" || entry === null || Array.isArray(entry)) return null;
    const value = (entry as Record<string, unknown>)[key];
    return typeof value === "string" && trimSpace(value) !== "" ? trimSpace(value) : null;
  };
  const readable = (entry: unknown) => field(entry, "email") !== null || field(entry, "displayName") !== null;
  const keyOf = (entry: unknown) => { const email = field(entry, "email"); return email === null ? null : emailKey(email); };
  const storedEmails = new Set(stored.map(keyOf).filter((key) => key !== null));
  const unmatched = stored.filter(readable);
  const emails = new Set<string>();
  const kept: unknown[] = [];
  let changed = false;
  for (const entry of list) {
    const name = trimSpace(entry.displayName ?? "") || null;
    const raw = trimSpace(entry.email ?? "") || null;
    if (name === null && raw === null) throw new Error("Each person needs an email address or a name.");
    const key = raw === null ? null : emailKey(raw);
    if (key !== null && emails.has(key)) continue;
    const at = unmatched.findIndex((was) => keyOf(was) === key && field(was, "displayName") === name);
    let value: unknown;
    if (at !== -1) {
      value = unmatched.splice(at, 1)[0];
    } else {
      changed = true;
      const email = raw === null ? null : (fixtureEmail(raw) ?? (key !== null && storedEmails.has(key) ? raw : undefined));
      if (email === undefined) throw new Error(`'${raw}' isn't an email address.`);
      value = { ...(name === null ? {} : { displayName: name }), ...(email === null ? {} : { email }) };
    }
    if (kept.length === 200 && list.length >= stored.filter(readable).length) throw new Error("A note can have at most 200 people.");
    if (key !== null) emails.add(key);
    kept.push(value);
  }
  if (kept.length > 200 && changed) throw new Error("A note can have at most 200 people.");
  kept.push(...stored.filter((entry) => !readable(entry)));
  return kept.length === 0 ? null : JSON.stringify(kept);
}

/** As `people::people`: by email, the name from the newest note that has one, most notes first. */
function fixturePeople(): Person[] {
  const found = new Map<string, { name: string | null; notes: Set<number> }>();
  const newestFirst = [...liveNotes()].sort((x, y) => (y.updatedAt ?? "").localeCompare(x.updatedAt ?? "") || y.id - x.id);
  for (const note of newestFirst) {
    for (const participant of readParticipants(note.participants)) {
      if (participant.email === null) continue;
      const email = emailKey(participant.email);
      const person = found.get(email) ?? { name: null, notes: new Set<number>() };
      person.name ??= participant.displayName;
      person.notes.add(note.id);
      found.set(email, person);
    }
  }
  const people = [...found].map(([email, { name, notes: on }]) => ({ email, name, noteCount: on.size }));
  // SQLite's order: `lower()` folds ASCII alone, and text compares code point by code point.
  const order = (x: string, y: string) => (x < y ? -1 : x > y ? 1 : 0);
  const label = (person: Person) => emailKey(person.name ?? person.email);
  return people.sort((x, y) => y.noteCount - x.noteCount || order(label(x), label(y)) || order(x.email, y.email));
}

/** As `people::list_by_person`: the notes whose people include `email`, in any case. */
function fixtureOnNote(note: Note, email: string): boolean {
  const wanted = emailKey(email);
  return readParticipants(note.participants).some((participant) => participant.email !== null && emailKey(participant.email) === wanted);
}

/**
 * Fixture note 105's transcript: two rows timed word by word and one, as every note saved before
 * KNEG-408 is, without words, all inside the fixture recording's first 20 s.
 * `tools/visual-check/word-highlight.mjs` plays it.
 */
const WORD_TIMING_SEGMENTS: StoredSegment[] = [
  wordedSegment(0.5, 0.65, "Hej och välkommen till genomgången av lanseringen, som flyttas till torsdag i nästa vecka."),
  wordedSegment(10.2, 0.5, "Installationsprogrammet är signerat, så vi kan släppa när lokaliseringen är klar."),
  { text: "Den här raden har inga ord, bara en tid.", timestamp: 16.4, endTimestamp: 19.6 },
];

let notes: Note[] = [
  fixtureNote({
    id: 101,
    title: "Weekly sync — platform",
    content:
      "Release moves to Thursday. Localisation pass is the blocker; Anna owns it.\n\nOpen: who signs off the Windows installer?",
    noteType: "meeting",
    enhancedContent:
      "## Decisions\n\n- Release moves to **Thursday**.\n\n## Owners\n\n- Localisation pass — Anna\n\n## Open questions\n\n- Windows installer sign-off is unassigned.",
    folderId: 3,
    transcript: JSON.stringify([
      { speaker: "Anna", text: "The localisation pass needs two more days." },
      { speaker: "Ravi", text: "Then Thursday is the earliest honest date." },
    ]),
    participants: JSON.stringify([
      { displayName: "Anna Lindqvist", email: "anna@example.com" },
      { displayName: "Ravi Menon", email: "ravi@example.com" },
    ]),
    audioDurationSeconds: 1847,
    // A meeting transcribed on this computer (KNEG-368).
    transcribedBy: fixtureTranscribedBy("local"),
    createdAt: "2026-08-26T09:00:00Z",
    updatedAt: "2026-08-26T09:31:00Z",
  }),
  fixtureNote({
    id: 102,
    title: "Customer call — Nordbank",
    content:
      "They want on-prem transcription only. Cloud providers are a non-starter for the compliance team.",
    noteType: "meeting",
    folderId: 2,
    participants: JSON.stringify([
      { displayName: "Jonas Berg", email: "jonas@nordbank.example" },
    ]),
    audioDurationSeconds: 2612,
    createdAt: "2026-08-25T13:15:00Z",
    updatedAt: "2026-08-25T14:02:00Z",
  }),
  fixtureNote({
    id: 103,
    title: "Scratch",
    content: "check whether WASAPI loopback survives a device change mid-call",
    noteType: "note",
    folderId: 1,
    createdAt: "2026-08-24T08:20:00Z",
    updatedAt: "2026-08-24T08:20:00Z",
  }),
  fixtureNote({
    id: 104,
    title:
      "A deliberately long title that exists so the notes list can be judged when someone types far more than the column was designed for",
    content:
      "Long-form body text, here so wrapping and vertical rhythm can be seen honestly rather than guessed at.\n\n" +
      "The overlay, the list panel and this pane all share `styles.css`, so a change made against three short notes tends to look fine and then fall apart on a real one. " +
      "This note is the real one.\n\nIt has several paragraphs, a list:\n\n- one\n- two\n- three\n\nand a trailing line.",
    // An import transcribed in the cloud, so a note says "Transcribed by OpenAI" (KNEG-368).
    noteType: "import",
    // A transcript of its own, so `tools/visual-check/shell-title-bar.mjs` has a second note
    // with a transcript column: one with no transcript has no column (KNEG-377). 102 and 103 are
    // the notes without.
    transcript: JSON.stringify([
      { speaker: "Ravi", text: "This is the note with the long title, read aloud." },
    ]),
    transcribedBy: fixtureTranscribedBy("openai"),
    createdAt: "2026-08-20T19:45:00Z",
    updatedAt: "2026-08-23T07:10:00Z",
  }),
  fixtureNote({
    id: 105,
    // An import transcribed by local Whisper, whose rows highlight word by word (KNEG-408). The
    // oldest note, so it is listed last and every check that opens the newest still opens 101.
    title: "Word timings",
    noteType: "import",
    transcript: JSON.stringify(WORD_TIMING_SEGMENTS),
    audioDurationSeconds: FIXTURE_RECORDING_SECONDS,
    transcribedBy: fixtureTranscribedBy("local"),
    createdAt: "2026-08-19T10:00:00Z",
    updatedAt: "2026-08-19T10:12:00Z",
  }),
  fixtureNote({
    id: 106,
    // A long meeting for the transcript column (KNEG-448): three 5-minute sections, turns, word
    // timings, lines with no speaker or no time (`fixtureTranscript.ts`). Dated just after 105 so
    // 101 stays the newest note and 105 the oldest.
    title: "Roadmap review — long transcript",
    noteType: "meeting",
    transcript: JSON.stringify(LONG_TRANSCRIPT_SEGMENTS),
    audioDurationSeconds: LONG_TRANSCRIPT_SECONDS,
    transcribedBy: fixtureTranscribedBy("local"),
    createdAt: "2026-08-19T12:00:00Z",
    updatedAt: "2026-08-19T12:30:00Z",
  }),
];

// Every note here is served a playable WAV as long as its recording's `durationSeconds`
// (`servePlayableRecordings`): 120 s for 101 and 105, so 101's recording is shorter than the
// meeting, whose own `audioDurationSeconds` still says it ran 1847 s, and 690 s for 106.
const noteRecordings = new Map<number, NoteRecording>([
  [
    101,
    {
      noteId: 101,
      storageKey: "20260901-101500.wav",
      durationSeconds: FIXTURE_RECORDING_SECONDS,
      originalFilename: null,
      createdAt: "2026-08-26T09:00:00Z",
    },
  ],
  [
    105,
    {
      noteId: 105,
      storageKey: "20260819-100000.wav",
      durationSeconds: FIXTURE_RECORDING_SECONDS,
      originalFilename: "genomgang.m4a",
      createdAt: "2026-08-19T10:00:00Z",
    },
  ],
  [
    106,
    {
      noteId: 106,
      storageKey: "20260819-120000.wav",
      durationSeconds: LONG_TRANSCRIPT_SECONDS,
      originalFilename: null,
      createdAt: "2026-08-19T12:00:00Z",
    },
  ],
]);

const noteTranscriptRows = new Map<number, TranscriptRow[]>([
  [
    101,
    [
      {
        index: 0,
        startSeconds: 72,
        endSeconds: 86,
        speaker: "Anna",
        speakerId: "speaker_0",
        text: "The localisation pass needs two more days.",
        // Saved before KNEG-408, as most notes were: highlighted as a row.
        words: [],
      },
      {
        index: 1,
        startSeconds: 88,
        endSeconds: 101,
        speaker: "Ravi",
        speakerId: "speaker_1",
        text: "Then Thursday is the earliest honest date.",
        words: [],
      },
    ],
  ],
  [105, WORD_TIMING_SEGMENTS.map((segment, index) => ({
    index,
    startSeconds: segment.timestamp,
    endSeconds: segment.endTimestamp,
    speaker: null,
    speakerId: null,
    text: segment.text,
    words: segment.words ?? [],
  }))],
  [106, longTranscriptRows()],
]);
const speakerNames = new Map<number, Map<string, string>>();

let nextNoteId = 107;
let nextFolderId = 4;

function fixtureImport(result: Omit<MediaImportResult, "noteId" | "recording" | "retainedBytes">): MediaImportResult {
  const noteId = nextNoteId++;
  const durationSeconds = result.durationMs / 1000;
  const recording: NoteRecording = { noteId, storageKey: `fixture-${noteId}.wav`, durationSeconds, originalFilename: "fixture.wav", createdAt: nowIso() };
  const segments = result.segments.length ? result.segments : [{ text: result.text, timestamp: 0, endTimestamp: durationSeconds }];
  notes.unshift(fixtureNote({ id: noteId, title: "Imported fixture", noteType: "import", transcript: JSON.stringify(segments), audioDurationSeconds: durationSeconds, transcribedBy: fixtureTranscribedBy(result.provider), createdAt: nowIso(), updatedAt: nowIso() }));
  noteRecordings.set(noteId, recording);
  noteTranscriptRows.set(noteId, segments.map((segment, index) => ({ index, text: segment.text, speaker: null, speakerId: null, startSeconds: segment.timestamp, endSeconds: segment.endTimestamp, words: [] })));
  const complete = { ...result, noteId, recording, retainedBytes: durationSeconds * 32000 + 44 };
  latestImportSnapshot = { operationId: `fixture-${noteId}`, stage: "succeeded", elapsedSeconds: 1, provider: result.provider, model: result.model, result: complete, error: null };
  return complete;
}


const SETTINGS = new Map<string, unknown>([
  ["dictation.device", "Microphone Array (Realtek(R) Audio)"],
  ["dictation.useLocal", false],
  ["ui.theme", "system"],
  ["meetings.promptOnDetect", true],
]);

/* ----------------------------------------------------------------- helpers */

/** Read one argument without pretending to know more about it than we do. */
function arg<T>(args: Record<string, unknown>, key: string): T | undefined {
  return args[key] as T | undefined;
}

const nowIso = (): string => new Date().toISOString();
/** Now as SQLite's `CURRENT_TIMESTAMP` writes it, UTC with no zone: how Rust stamps `trashed_at`. */
const sqliteNow = (): string => new Date().toISOString().replace("T", " ").slice(0, 19);

/* ---------------------------------------------------------------- handlers */

let removedVocabulary: string | null = null;
const HANDLERS: Record<string, (args: Record<string, unknown>) => unknown> = {
  /* capabilities */
  diagnostics: () => DIAGNOSTICS,

  /* hotkeys */
  hotkeys: () => HOTKEYS,
  hotkey_capture_start: () => 1,
  hotkey_capture_end: () => undefined,
  hotkey_set: (a) => {
    const action = arg<HotkeyAction>(a, "action") ?? "dictation";
    // One comma-separated string, as `setHotkey` sends it and Rust's
    // `hotkey::parse_list` splits it -- without that function's rescue of a
    // comma key, which no fixture binding uses (KNEG-348).
    const accelerators = (arg<string>(a, "accelerators") ?? "")
      .split(",")
      .map((accelerator) => accelerator.trim())
      .filter((accelerator) => accelerator !== "");
    const change: HotkeyChange = {
      binding: {
        action,
        accelerators: accelerators.map(standard),
        mode: "toggle",
      },
      registrations: accelerators.map((accelerator) => ({
        state: "registered" as const,
        accelerator,
      })),
      live: accelerators.length > 0,
      // Computed from the accelerator rather than hardcoded, so typing
      // `Control+Alt` into the field in dev mode produces the warning the real
      // backend would. The real rule runs the chord matcher over the keys
      // AltGr sends; this is the shape of its answer, not a second copy of it.
      cautions: accelerators
        .filter((a) => /control|ctrl/i.test(a) && /(^|\+)alt/i.test(a))
        .map((accelerator) => ({
          action,
          accelerator,
          reason: `${accelerator} also fires when you press AltGr. This layout types '@' with Control and Alt together, and Windows sends left Control and right Alt for AltGr -- which completes this chord, with nothing downstream able to tell that apart from you pressing it deliberately.`,
        })),
    };
    return change;
  },
  // Echoes the mode back rather than remembering it: this file exists so a
  // screen can be looked at without a backend, and a fixture that kept state
  // would start being a second implementation to keep in step.
  hotkey_set_mode: (a) => {
    const action = arg<HotkeyAction>(a, "action") ?? "dictation";
    const mode = arg<HotkeyMode>(a, "mode") ?? "toggle";
    const change: HotkeyChange = {
      binding: {
        action,
        accelerators: [standard("Control+Shift+D")],
        mode,
      },
      registrations: [
        { state: "registered" as const, accelerator: "Control+Shift+D" },
      ],
      live: true,
      cautions: [],
    };
    return change;
  },

  /* providers */
  // Copies, as a serialised reply would be: the setter below moves these, and
  // a screen must not see its last read change underneath it.
  providers: () => PROVIDERS.map((p) => ({ ...p })),
  credential_slots: () => CREDENTIAL_SLOTS.map((s) => ({ ...s })),
  // Remembered, so a key saved on Providers flips its row to "Key saved" and
  // Clear flips it back. An empty value deletes, as it does in Rust; the value
  // itself is dropped here, since nothing can read one back anyway.
  provider_credential_set: (a) => {
    const id = arg<string>(a, "slot");
    const stored = (arg<string>(a, "value") ?? "").trim() !== "";
    const slot = CREDENTIAL_SLOTS.find((s) => s.slot === id);
    if (!slot) throw new Error(`${JSON.stringify(id)} is not a credential slot this application uses`);
    slot.stored = stored;
    for (const p of PROVIDERS) if (p.credentialSlot === id) p.configured = stored;
    return null;
  },
  provider_credential_present: (a) =>
    CREDENTIAL_SLOTS.find((s) => s.slot === arg<string>(a, "slot"))?.stored ??
    false,
  probe_providers: () => PROBE_REPORT,

  /* reasoning */
  // What was last stored and where it resolves, as Rust reports them: a write
  // below moves both, so Settings and Dictation's Tidy up chip read it back.
  reasoning_status: () => ({
    ...REASONING,
    settings: { ...REASONING.settings },
    route: REASONING.route,
  }),
  // Mirrors `reasoning_set_cleanup` (src-tauri/src/reasoning.rs): off always
  // resolves; on needs a model, and a provider or a URL; it is refused before
  // anything is stored, so a refusal leaves the status as it was. A provider
  // with no key resolves, with the caveat Rust words.
  reasoning_set_cleanup: (a) => {
    const settings = arg<CleanupSettings>(a, "settings");
    if (!settings) throw new Error("no cleanup settings were sent");
    let route: CleanupRoute;
    if (!settings.enabled) {
      route = { state: "off" };
    } else {
      if (!(settings.model ?? "").trim()) {
        throw new Error(
          "cleanup needs a model. Providers disagree about parameter names, so there is no safe default to fall back on.",
        );
      }
      const url = (settings.selfHostedUrl ?? "").trim();
      const provider = REASONING.providers.find((p) => p.id === settings.provider);
      if (!url && !provider) {
        throw new Error("cleanup needs a provider or a self-hosted URL");
      }
      route = url
        ? {
            state: "ready",
            destination: `self-hosted at ${url}`,
            model: settings.model ?? "",
            leavesDevice: !url.includes("127.0.0.1") && !url.includes("localhost"),
            caveat: null,
          }
        : {
            state: "ready",
            destination: provider!.id,
            model: settings.model ?? "",
            leavesDevice: true,
            caveat: provider!.configured
              ? null
              : `no key is stored for ${provider!.credentialSlot ?? provider!.id}. Cleanup will leave the transcript as it is until one is, on the Providers screen.`,
          };
    }
    REASONING.settings = { ...settings };
    REASONING.route = route;
    return route;
  },
  enterprise_config_set: () => null,
  enterprise_config_clear: () => null,

  /* history */
  history: (a) => {
    const limit = arg<number | null>(a, "limit");
    return typeof limit === "number" ? HISTORY.slice(0, limit) : HISTORY;
  },
  history_delete: () => true,
  history_clear: () => HISTORY.length,
  // Clearing is not switching off and switching off is not clearing, which is
  // the point the two adjacent controls have to make. `history_clear` reports a
  // count and leaves the fixture list alone; this pair moves a flag and never
  // touches a row.
  history_enabled: () => historyEnabled,
  history_set_enabled: (a) => {
    historyEnabled = arg<boolean>(a, "enabled") ?? historyEnabled;
    return null;
  },
  // On, so the invented history above can show a `rawText` row. The real
  // default is off -- see `KEEP_RAW_TEXT_KEY`.
  history_keep_raw_text: () => keepRawText,
  history_set_keep_raw_text: (a) => {
    keepRawText = arg<boolean>(a, "enabled") ?? keepRawText;
    return null;
  },

  /* notes */
  notes_list: (a) => {
    const noteType = arg<string | null>(a, "noteType");
    const folderId = arg<number | null>(a, "folderId");
    const person = arg<string | null>(a, "person");
    const limit = arg<number | null>(a, "limit");
    let out = liveNotes();
    // As Rust's `list_notes`: a filter of its own, refused beside the others (KNEG-362).
    if (typeof person === "string") {
      if (!person.trim()) throw new Error("a person filter needs an email");
      if (noteType || typeof folderId === "number") {
        throw new Error("'person' is a filter of its own: send it without 'noteType' or 'folderId'");
      }
      out = out.filter((n) => fixtureOnNote(n, person));
    }
    if (noteType) out = out.filter((n) => n.noteType === noteType);
    if (typeof folderId === "number") {
      out = out.filter((n) => n.folderId === folderId);
    }
    out = [...out].sort((x, y) =>
      (y.updatedAt ?? "").localeCompare(x.updatedAt ?? ""),
    );
    return typeof limit === "number" ? out.slice(0, limit) : out;
  },
  notes_list_summaries: (a) => {
    const limit = arg<number | null>(a, "limit") ?? 100;
    return (HANDLERS.notes_list!({ ...a, limit: null }) as Note[])
      .map(noteListItem)
      .sort((left, right) => (right.updatedAt ?? "").localeCompare(left.updatedAt ?? "") || right.id - left.id)
      .slice(0, limit);
  },
  notes_get_summary: (a) => {
    const note = notes.find(note => note.id === arg<number>(a, "id"));
    return note ? noteListItem(note) : null;
  },
  notes_search: (a) => {
    const query = (arg<string>(a, "query") ?? "").trim().toLowerCase();
    if (query === "") return [] satisfies SearchHit[];
    const marked = (text: string, at: number) =>
      `${text.slice(Math.max(0, at - 35), at)}[${text.slice(at, at + query.length)}]${text.slice(at + query.length, at + query.length + 35)}`;
    // As Rust answers it: the title or the notes first, with no row; otherwise
    // the transcript row the words are in, with its time and its own snippet
    // (KNEG-406). A plain substring here, where Rust asks FTS5 for every word. Never a note in
    // Trash (KNEG-401).
    return liveNotes().flatMap((n): SearchHit[] => {
      const at = n.content.toLowerCase().indexOf(query);
      if (at !== -1) return [{ id: n.id, title: n.title, snippet: marked(n.content, at), transcriptRow: null }];
      if (n.title.toLowerCase().includes(query)) {
        return [{ id: n.id, title: n.title, snippet: n.content.slice(0, 90), transcriptRow: null }];
      }
      const row = (noteTranscriptRows.get(n.id) ?? []).find((r) => r.text.toLowerCase().includes(query));
      if (!row) return [];
      return [{
        id: n.id,
        title: n.title,
        snippet: marked(row.text, row.text.toLowerCase().indexOf(query)),
        transcriptRow: { index: row.index, startSeconds: row.startSeconds },
      }];
    });
  },
  notes_get: (a) => notes.find((n) => n.id === arg<number>(a, "id")) ?? null,
  notes_create: (a) => {
    const r = arg<NewNoteRequest>(a, "request");
    const id = nextNoteId++;
    // As Rust's `notes_create`: where a transcript was made is not the caller's to say (KNEG-368).
    notes = [
      fixtureNote({
        id,
        title: r?.title ?? "Untitled",
        content: r?.content ?? "",
        noteType: r?.noteType ?? "note",
        folderId: r?.folderId ?? null,
        transcript: r?.transcript ?? null,
        participants: r?.participants ?? null,
        audioDurationSeconds: r?.audioDurationSeconds ?? null,
        createdAt: nowIso(),
        updatedAt: nowIso(),
      }),
      ...notes,
    ];
    return id;
  },
  notes_update: (a) => {
    const id = arg<number>(a, "id");
    const r = arg<NoteUpdateRequest>(a, "request");
    if (!editable(id)) return false;
    notes = notes.map((n) =>
      n.id === id
        ? {
            ...n,
            title: r?.title ?? n.title,
            content: r?.content ?? n.content,
            folderId: r?.folderId === undefined ? n.folderId : r.folderId,
            updatedAt: nowIso(),
          }
        : n,
    );
    return true;
  },
  // KNEG-401, as Rust's `trash` module and `recordings::delete_forever` / `empty_trash` answer.
  notes_trash: (a) => {
    const id = arg<number>(a, "id");
    if (!notes.some((n) => n.id === id && n.trashedAt === null)) return false;
    notes = notes.map((n) => (n.id === id ? { ...n, trashedAt: sqliteNow() } : n));
    return true;
  },
  notes_restore: (a) => {
    const id = arg<number>(a, "id");
    if (!notes.some((n) => n.id === id && n.trashedAt !== null)) return false;
    notes = notes.map((n) => (n.id === id ? { ...n, trashedAt: null } : n));
    return true;
  },
  notes_delete_forever: (a) => {
    const id = arg<number>(a, "id");
    if (typeof id !== "number" || !notes.some((n) => n.id === id && n.trashedAt !== null)) return false;
    deleteFixtureNote(id);
    return true;
  },
  notes_trash_list: () => fixtureTrash(),
  notes_empty_trash: (a) => {
    // Only the notes the question counted, and only while still in Trash.
    const asked = arg<number[]>(a, "ids") ?? [];
    const trashed = notes.filter((n) => n.trashedAt !== null && asked.includes(n.id)).map((n) => n.id);
    for (const id of trashed) deleteFixtureNote(id);
    return trashed.length;
  },
  // A note listed here gets a player, and the player's `recording` URL is
  // answered with the fixtures' two-minute WAV (`servePlayableRecordings`).
  // A note that is not has no player, as in the app.
  speech_installations: () => ({ revision: 0, jobs: [] }),
  note_recording_state: a => recordingStates.get(arg<number>(a, "id") ?? 0) ?? null,
  note_recording_retry_deletion: () => { throw new Error("This note has no recording deletion request."); },
  note_recording_get: (a) => {
    const id = arg<number>(a, "id");
    return typeof id === "number" ? noteRecordings.get(id) ?? null : null;
  },
  note_transcript_rows: (a) => {
    const id = arg<number>(a, "id");
    return typeof id === "number" ? noteTranscriptRows.get(id) ?? [] : [];
  },
  known_voices_status: () => ({ enrollmentReason: null, enabled: false, ready: false, reason: "Known voices is awaiting validation on labeled recordings from separate days.", voices: [] }),
  known_voices_set_enabled: () => { throw new Error("Known voices has not passed validation"); },
  note_speaker_rename: (a) => {
    const id = arg<number>(a, "id");
    const speakerId = arg<string>(a, "speakerId");
    const rawName = arg<string>(a, "name");
    if (typeof id !== "number" || !editable(id)) throw new Error("there is no note with that id");
    if (typeof speakerId !== "string" || !speakerId.trim()) throw new Error("a speaker id cannot be blank");
    const name = (rawName ?? "").trim().replace(/\s+/g, " ");
    const names = speakerNames.get(id) ?? new Map<string, string>();
    if (name) names.set(speakerId, name);
    else names.delete(speakerId);
    speakerNames.set(id, names);
    const speakerNumber = /^speaker_(\d+)$/.exec(speakerId)?.[1];
    const display = name || (speakerNumber === undefined ? speakerId : `Speaker ${Number(speakerNumber) + 1}`);
    noteTranscriptRows.set(id, (noteTranscriptRows.get(id) ?? []).map(row =>
      row.speakerId === speakerId ? { ...row, speaker: display } : row,
    ));
  },
  note_speaker_suggestions: (a) => {
    const id = arg<number>(a, "id");
    if (typeof id !== "number") return [];
    const used = new Set((noteTranscriptRows.get(id) ?? []).map(row => row.speaker?.toLocaleLowerCase()).filter(name => name !== undefined));
    // Other notes' names, never a note's in Trash (KNEG-401).
    const live = new Set(liveNotes().map((note) => note.id));
    const candidates = [
      ...(id === 101 ? ["Anna Lind", "Erik Berg"] : []),
      ...[...speakerNames].filter(([noteId]) => noteId !== id && live.has(noteId)).flatMap(([, names]) => [...names.values()]),
    ];
    const seen = new Set<string>();
    return candidates.filter(name => {
      const key = name.toLocaleLowerCase();
      if (used.has(key) || seen.has(key)) return false;
      seen.add(key);
      return true;
    }).slice(0, 20);
  },
  folders_list: () => folders,
  // KNEG-362. As Rust: checked and tidied, refused in plain words, and `updatedAt` left alone.
  people_list: () => fixturePeople(),
  people_page: (a) => fixturePeople().slice(Math.max(0, arg<number>(a, "offset") ?? 0), Math.max(0, arg<number>(a, "offset") ?? 0) + Math.min(501, Math.max(1, arg<number>(a, "limit") ?? 51))),
  notes_set_participants: (a) => {
    const id = arg<number>(a, "id");
    if (!editable(id)) return false;
    const note = notes.find((n) => n.id === id);
    if (!note) return false;
    const participants = fixtureTidyParticipants(note.participants, arg<ParticipantRequest[]>(a, "participants") ?? []);
    notes = notes.map((n) => (n.id === id ? { ...n, participants } : n));
    return true;
  },
  notes_set_folder: (a) => {
    const id = arg<number>(a, "id");
    const folderId = arg<number | null>(a, "folderId") ?? null;
    if (!editable(id)) return false;
    notes = notes.map((n) => (n.id === id ? { ...n, folderId } : n));
    return true;
  },
  folder_create: (a) => {
    const id = nextFolderId++;
    folders = [
      ...folders,
      {
        id,
        name: arg<string>(a, "name") ?? "New folder",
        isDefault: false,
        sortOrder: folders.length,
      },
    ];
    return id;
  },
  folder_delete: (a) => {
    const id = arg<number>(a, "id");
    const before = folders.length;
    folders = folders.filter((f) => f.id !== id);
    notes = notes.map((n) => (n.folderId === id ? { ...n, folderId: null } : n));
    return folders.length !== before;
  },

  /* transcript export */
  note_export: (a) => {
    const id = arg<number>(a, "id");
    const format = arg<Format>(a, "format") ?? "txt";
    const note = notes.find((n) => n.id === id);
    const segments: Array<{ speaker: string; text: string }> = note?.transcript
      ? (JSON.parse(note.transcript) as Array<{
          speaker: string;
          text: string;
        }>)
      : [];
    const slug = (note?.title ?? "note")
      .toLowerCase()
      .replace(/[^a-z0-9]+/g, "-")
      .replace(/^-+|-+$/g, "");
    const exported: ExportedTranscript = {
      format,
      filename: `${slug}.${format}`,
      content:
        segments.length > 0
          ? segments.map((s) => `${s.speaker}: ${s.text}`).join("\n")
          : (note?.content ?? ""),
      segmentCount: segments.length,
    };
    return exported;
  },
  note_export_to_file: () =>
    "C:\\Users\\dev\\Documents\\weekly-sync-platform.md",

  /* settings */
  settings_get: (a) => SETTINGS.get(arg<string>(a, "key") ?? "") ?? null,
  settings_set: (a) => {
    const key = arg<string>(a, "key");
    if (key !== undefined) SETTINGS.set(key, a["value"]);
    return null;
  },
  settings_keys: () => [...SETTINGS.keys()],

  /* import */
  import_openwhispr: () => {
    const report: ImportReport = {
      source: "C:\\Users\\dev\\AppData\\Roaming\\openwhispr\\openwhispr.db",
      tables: [
        {
          table: "notes",
          copied: 128,
          skipped: 2,
          absentColumns: ["enhanced_content"],
          droppedColumns: ["legacy_sync_state"],
          skipReason: "referenced folder 7, which was itself not imported",
        },
        {
          table: "folders",
          copied: 6,
          skipped: 0,
          absentColumns: [],
          droppedColumns: [],
          skipReason: null,
        },
        {
          table: "transcriptions",
          copied: 412,
          skipped: 0,
          absentColumns: ["route_kind"],
          droppedColumns: [],
          skipReason: null,
        },
      ],
      relocatedCredentials: [
        { sourceTable: "settings", account: "openai", slot: "openai" },
      ],
      absentTables: ["dictation_presets"],
      ftsRows: 128,
    };
    return report;
  },

  /* dictation */
  dictation_state: () => "idle" satisfies SessionState,
  dictation_toggle: () =>
    ({
      outcome: "inserted",
      text: "Dictated text from the dev fixtures.",
      clipboardRestored: true,
      historyId: 42,
    }) satisfies Outcome,
  dictation_cancel: () => ({ outcome: "cancelled" }) satisfies Outcome,
  input_devices: () => INPUT_DEVICES,
  dictation_set_device: (a) => {
    dictationDevice = arg<string | null>(a, "device") ?? null;
    return null;
  },
  dictation_device: () => dictationDevice,
  // Rust's `microphone_check` (`microphone_check.rs`): refused where Rust
  // refuses, in its words; otherwise five seconds of fake levels on the
  // check's own session, as its publisher sends them, with a last 0, and
  // then what it heard. The fixtures' microphone always hears you; which one
  // was tested follows `dictation_set_device`, the default standing in for
  // a choice that isn't connected.
  microphone_check: async () => {
    if (meetingStatus.state !== "idle" && meetingStatus.state !== "finishing") {
      throw new Error("Not now: a meeting is under way. Test the microphone once it has ended.");
    }
    if (fixtureDictating) {
      throw new Error("Not now: a dictation is recording. Test the microphone once it has finished.");
    }
    if (microphoneChecking) throw new Error("Not now: the microphone is already being tested.");
    microphoneChecking = true;
    try {
      startFakeLevels("microphone-check", ["microphone"]);
      await new Promise((resolve) => setTimeout(resolve, 5000));
      stopFakeLevels("microphone-check");
      const device = fixtureOpenedMicrophone();
      return {
        device,
        fellBack: dictationDevice !== null && dictationDevice !== device ? dictationDevice : null,
        heard: true,
        peakLevel: 0.7,
      } satisfies MicrophoneCheck;
    } finally {
      microphoneChecking = false;
    }
  },
  insertion_supported: () => true,
  dictation_route: () => resolvedRoute(),
  dictation_use_local: (a) => {
    dictationRoute.useLocal = arg<boolean>(a, "useLocal") ?? false;
    return null;
  },
  // Refused where Rust refuses -- a pair the registry does not list, and Corti
  // without its environment and tenant -- so a screen's error path can be
  // seen; otherwise stored, and the route is off local, as in Rust.
  dictation_set_cloud_provider: (a) => {
    const provider = arg<string>(a, "provider") ?? "";
    const model = arg<string>(a, "model") ?? "";
    if (!PROVIDERS.some((p) => p.id === provider && p.models.includes(model))) {
      throw new Error(`"${provider}" / "${model}" is not a transcription model the registry lists`);
    }
    if (provider === "corti" && !(arg<string | null>(a, "cortiEnvironment") && arg<string | null>(a, "cortiTenant"))) {
      throw new Error("Corti needs both an environment and a tenant.");
    }
    dictationRoute.useLocal = false;
    dictationRoute.provider = provider;
    dictationRoute.model = model;
    return null;
  },
  dictation_local_language: () => [...localLanguages],
  // `validated_local_languages`' eight codes (src-tauri/src/dictation.rs).
  // Checked as `validated_local_languages` checks, and announced as Rust
  // announces it, so the Dictation chip and the floating microphone follow.
  dictation_set_local_language: (a) => {
    const languages = arg<string[]>(a, "languages") ?? [];
    if (languages.length > 2) throw new Error("Choose at most two languages");
    if (new Set(languages).size !== languages.length || !languages.every((code) => DICTATION_LANGUAGE_CODES.includes(code))) {
      throw new Error("Choose distinct supported dictation languages");
    }
    localLanguages = [...languages];
    void emit("dictation-languages", localLanguages);
    return null;
  },

  /* overlay */
  overlay_preferences: () => overlayPreferences,
  overlay_preferences_set: (a) => {
    overlayPreferences = {
      ...overlayPreferences,
      enabled: arg<boolean>(a, "enabled") ?? overlayPreferences.enabled,
    };
    void emit("overlay-preferences", overlayPreferences);
    return overlayPreferences;
  },
  overlay_position_reset: () => null,
  // The floating microphone's drag gesture and its message/language-menu
  // visibility. Both are fire-and-forget from `Overlay.tsx`'s point of view.
  overlay_drag: () => null,
  overlay_message: () => null,

  /* local transcription */
  // Copies, as for `providers`: the writes below move these, and a screen must
  // not see its last read change underneath it.
  sherpa_status: () => ({ ...SHERPA }),
  sherpa_install_model: () => null,
  sherpa_use_model: (a) => {
    // Rust checks only that the id resolves (`resolve_id`), not what the model
    // is for; so does this. An id it does not know is refused.
    const model = SHERPA.models.find((m) => m.id === arg<string>(a, "modelId"));
    if (!model) throw new Error(`Unknown model: ${arg<string>(a, "modelId") ?? ""}`);
    SHERPA.selected = model.id;
    return null;
  },
  // Refused where Rust refuses: an offline model, or one not installed. Moves
  // only the meetings choice, as Rust writes only `meetings.streamModel`.
  sherpa_use_stream_model: (a) => {
    const model = SHERPA.models.find((m) => m.id === arg<string>(a, "modelId"));
    if (!model) throw new Error(`Unknown model: ${arg<string>(a, "modelId") ?? ""}`);
    if (model.usedFor !== "meetings") {
      throw new Error(`${model.name} is an offline model, so it is for dictation rather than meetings.`);
    }
    if (!model.installed) throw new Error(`${model.name} is not installed. Install it before choosing it for meetings.`);
    SHERPA.selectedStream = model.id;
    return null;
  },
  local_model_status: () => ({ ...LOCAL_MODELS }),
  local_model_install: () => "whisper-large-v3-turbo-q5",
  // A second and a half before it answers, as a real server takes seconds to
  // load a model, so the route chip's "Starting …" and the held Start button
  // can be seen. The old server goes first, as `start_model` stops one
  // running another model, and a Parakeet selection is released.
  local_server_start: async (a) => {
    const model = LOCAL_MODELS.models.find((m) => m.id === arg<string>(a, "modelId"));
    if (!model?.installed) throw new Error("That model is not installed.");
    LOCAL_MODELS.port = null;
    LOCAL_MODELS.runningModel = null;
    LOCAL_MODELS.runtimeMessage = `Checking or starting local model ${model.id}...`;
    LOCAL_MODELS.runtime = { state: "checking", model: model.id };
    await new Promise((resolve) => setTimeout(resolve, 1500));
    LOCAL_MODELS.port = 8178;
    LOCAL_MODELS.runningModel = model.id;
    LOCAL_MODELS.runtimeMessage = `Local model ${model.id} is ready on port 8178.`;
    LOCAL_MODELS.runtime = { state: "ready", model: model.id, port: 8178 };
    SHERPA.selected = null;
    return 8178;
  },
  local_server_stop: () => {
    LOCAL_MODELS.port = null;
    LOCAL_MODELS.runningModel = null;
    LOCAL_MODELS.runtimeMessage = "Local model server is stopped.";
    LOCAL_MODELS.runtime = { state: "stopped" };
    return null;
  },
  local_server_refresh: () => ({
    files: 37,
    restarted: "whisper-large-v3-turbo-q5",
    restartError: null,
  }),

  /* local text generation */
  local_llm_status: () => LOCAL_LLM,
  local_llm_install: () => null,
  local_llm_start: () => 8080,
  local_llm_stop: () => null,

  /* local api */
  local_api_status: () => LOCAL_API,
  local_api_start: () => 8749,
  local_api_stop: () => null,

  /* meetings */
  meeting_providers: () => MEETING_PROVIDERS,
  meeting_status: () => meetingStatus,
  meeting_notes_get: () => meetingNotes,
  meeting_notes_update: (a) => {
    meetingNotes = arg<string>(a, "markdown") ?? meetingNotes;
    return null;
  },
  meeting_notification_placement_save: () => null,
  meeting_notification_placement_reset: () => null,
  meeting_recoverable_recordings: () => recoverable,
  meeting_recover_recording: (a) => {
    const key = arg<string>(a, "storageKey");
    if (!recoverable.some((r) => r.storageKey === key)) {
      throw new Error("No interrupted recordings in the fixture profile");
    }
    recoverable = recoverable.filter((r) => r.storageKey !== key);
    return {
      outcome: "saved",
      noteId: 101,
      durationSeconds: 1847,
      recordingPath:
        "C:\\Users\\dev\\AppData\\Roaming\\KnegisWhisper\\recordings\\" + (key ?? ""),
      gaps: 0,
      reconnects: 0,
      transcriptIsPartials: false,
      recordingAttached: true,
      folderMissing: false,
      recordingRemoval: null,
      recordingRemovalError: null,
    } satisfies MeetingOutcome;
  },
  // The plan comes back on `meeting_status` while the meeting runs, tidied,
  // as Rust keeps it with the session (KNEG-365), with the delete choice
  // read from the setting at Start (KNEG-409).
  meeting_start: (a) => {
    const wantsSystemAudio = arg<boolean>(a, "systemAudio") ?? false;
    const sources: Sources = {
      microphone: true,
      // The microphone the meeting opened, kept for its whole run: a choice
      // made afterwards does not move it, as in Rust (KNEG-364).
      microphoneName: fixtureOpenedMicrophone(),
      systemAudio: wantsSystemAudio,
      systemAudioReason: wantsSystemAudio
        ? null
        : "system audio was not requested",
    };
    meetingStartedAt = new Date();
    meetingStatus = {
      ...meetingStatus,
      state: "recording",
      provider: arg<string>(a, "provider") ?? "meeting:deepgram",
      plan: {
        title: fixtureMeetingTitle(arg<string | null>(a, "title")),
        folderId: arg<number | null>(a, "folderId") ?? null,
        deleteRecording: meetingDeleteRecordings,
        liveProviderId: arg<string>(a, "provider") ?? "meeting:deepgram",
      },
      elapsedSeconds: 0,
      sources,
    };
    // The meters' levels, from the captures that opened, as `start_live` publishes them.
    startFakeLevels("meeting", wantsSystemAudio ? ["microphone", "system"] : ["microphone"]);
    return sources;
  },
  // `fixtureStop` saves the note (a folder deleted while the meeting ran has
  // no `kw` hand-hold: delete it in Notes, or through `folder_delete`). The
  // outcome is announced as well as returned, as Rust's `stop` announces it:
  // that is what makes the page read Recent again. Not the `idle` state, for
  // the reason `meeting_cancel` gives.
  meeting_stop: () => {
    stopFakeLevels("meeting");
    const outcome = fixtureStop();
    void emit("meeting-outcome", outcome);
    return outcome;
  },
  meeting_cancel: () => {
    stopFakeLevels("meeting");
    meetingStartedAt = null;
    meetingStatus = {
      ...meetingStatus,
      state: "idle",
      provider: null,
      plan: null,
      elapsedSeconds: 0,
      sources: null,
      offer: null,
    };
    // How Rust's `cancel` says a meeting was discarded (`meeting.rs`), so the
    // page can say "Discarded." here too. Rust keeps typed notes as a note and
    // names it; this tab has no meeting window to have typed them in, so
    // there is none. Not the `idle` state event Rust sends first: no other
    // meeting command here announces a state, and an `idle` heard once would
    // outrank every later status read, so the next Start would never show.
    void emit("meeting-outcome", { outcome: "cancelled", noteId: null } satisfies MeetingOutcome);
    return null;
  },
  meeting_dismiss: () => {
    meetingStatus = { ...meetingStatus, offer: null };
    return null;
  },
  meeting_set_prompt: (a) => {
    meetingStatus = {
      ...meetingStatus,
      promptOnDetect: arg<boolean>(a, "enabled") ?? false,
    };
    return null;
  },
  meeting_delete_recordings: () => meetingDeleteRecordings,
  meeting_set_delete_recordings: (a) => {
    meetingDeleteRecordings = arg<boolean>(a, "enabled") ?? false;
    return null;
  },
  summary_settings_get: () => summarySettings,
  summary_readiness_get: () => summaryConfigured(summarySettings)
    ? { state: "ready" as const }
    : { state: "needs-setup" as const, reason: "Choose a summary provider and model.", target: "summary-settings" as const },
  note_summary_get: (a) => summaryOf(arg<number>(a, "id") ?? 0),
  note_summary_generate: (a) => {
    const id = arg<number>(a, "id") ?? 0;
    const current = summaryOf(id);
    // The two refusals `generate` and `route()` make before queueing, in their words.
    if (!summaryConfigured(summarySettings)) throw new Error("Choose a summary provider and model.");
    const note = notes.find((n) => n.id === id);
    if (!note?.transcript?.trim() || note.transcript.trim() === "[]") throw new Error("This note has no transcript to summarize.");
    const next: SummaryState = {
      ...current,
      content: `## Summary

A fixture summary of "${note.title}", written without a model.

## Action items

- None.`,
      revision: current.revision + 1, edited: false, outdated: false, legacy: false,
      status: "succeeded", error: null, language: "en", generatedAt: nowIso(),
      // What Rust stores at success: the id the settings named (KNEG-367).
      model: summarySettings.model,
    };
    summaries.set(id, next);
    return next;
  },
  note_summary_cancel: () => null,
  note_summary_update: (a) => {
    const id = arg<number>(a, "id") ?? 0;
    const current = summaryOf(id);
    if (arg<number>(a, "revision") !== current.revision) throw new Error("Summary changed. Reload before saving.");
    const next: SummaryState = { ...current, content: arg<string>(a, "content") ?? "", revision: current.revision + 1, edited: true };
    summaries.set(id, next);
    return next;
  },
  note_summary_export: () => null,
  summary_settings_set: (a) => {
    summarySettings = arg<SummarySettings>(a, "settings") ?? summarySettings;
    return null;
  },
  meeting_language: () => meetingLanguage,
  meeting_set_language: (a) => {
    meetingLanguage = arg<string>(a, "language") ?? meetingLanguage;
    return null;
  },
  meeting_local_model: () => meetingLocalModel,
  meeting_set_local_model: (a) => {
    const model = arg<string | null>(a, "model") ?? null;
    if (model !== null && !LOCAL_MODELS.models.some((known) => known.id === model)) {
      throw new Error(`"${model}" is not a model this build knows about`);
    }
    meetingLocalModel = model;
    return null;
  },
  meeting_provider: () => meetingProvider,
  meeting_set_provider: (a) => {
    meetingProvider = arg<string>(a, "provider") ?? meetingProvider;
    return null;
  },
  vocabulary_delete: (a) => { const id = arg<number>(a, "id") ?? 0; removedVocabulary = meetingKeyterms[id - 1] ?? null; meetingKeyterms.splice(id - 1, 1); return null; },
  vocabulary_restore: () => { if (removedVocabulary !== null) meetingKeyterms.push(removedVocabulary); removedVocabulary = null; return null; },
  vocabulary_list: () => meetingKeyterms.map((word, index) => ({ id: index + 1, word, source: "manual", createdAt: null })),
  vocabulary_update: (a) => {
    const add = (arg<string[]>(a, "add") ?? []).map(word => word.trim().replace(/\s+/g, " "));
    const remove = arg<string[]>(a, "remove") ?? [];
    if (add.some(word => !word || [...word].length > 100)) throw new Error("Vocabulary terms must contain 1-100 characters");
    const added = add.filter(word => !meetingKeyterms.includes(word));
    const removed = meetingKeyterms.filter(word => remove.includes(word)).length;
    meetingKeyterms = [...new Set([...meetingKeyterms, ...added])].filter(word => !remove.includes(word));
    return { added: added.length, removed, alreadyPresent: add.length - added.length, notPresent: remove.length - removed };
  },
  vocabulary_edit: (a) => {
    const id = arg<number>(a, "id") ?? 0;
    const word = (arg<string>(a, "word") ?? "").trim().replace(/\s+/g, " ");
    if (!word || word.length > 100 || meetingKeyterms.some((value, index) => value === word && index !== id - 1)) throw new Error("Invalid or duplicate vocabulary entry");
    if (id < 1 || id > meetingKeyterms.length) throw new Error("Entry no longer exists");
    meetingKeyterms[id - 1] = word;
    return null;
  },
  vocabulary_review: (a) => {
    const provider = arg<string>(a, "provider");
    const supported = ["whisper", "assemblyai-realtime", "deepgram-realtime", "corti-realtime"].includes(provider ?? "");
    const included = supported ? meetingKeyterms.slice(0, 2) : [];
    return { supported, model: provider === "deepgram-realtime" ? "nova-3" : arg<string>(a, "model") ?? null, included,
      omitted: meetingKeyterms.slice(included.length).map(word => ({ word, reason: supported ? "Illustrative fixture limit" : "Not supported by this integration" })), limit: "Development fixture: at most 2 terms" };
  },
  meeting_keyterms: () => meetingKeyterms,
  meeting_set_keyterms: (a) => {
    meetingKeyterms = arg<string[]>(a, "keyterms") ?? meetingKeyterms;
    return null;
  },
  /*
   * Deliberately reports terms being dropped. Deepgram's fixture caps keyterms
   * at 100 and this asks for more, so the screen's "silently discarded" warning
   * has something to render -- the branch nobody sees if the fixture is tidy.
   */
  meeting_review_config: () =>
    ({
      keytermsRequested: meetingKeyterms.length,
      keytermsUsed: Math.min(meetingKeyterms.length, 2),
      keytermsDropped: Math.max(0, meetingKeyterms.length - 2),
      languageRequired: false,
      sessionLimitSeconds: 1800,
    }) satisfies ConfigReview,

  /* cli and mcp */
  cli_status: () => CLI,
  mcp_status: () => MCP,

  /* media import */
  media_import_status: () => SIDECARS,
  media_import_snapshot: () => latestImportSnapshot,
  media_import_install_sidecar: () => null,
  pick_import_file: () => "C:\\Users\\dev\\Music\\interview.m4a",
  media_import_audio: () =>
    fixtureImport({
      text: "So the localisation pass is the blocker, and Anna owns it.",
      provider: "openai",
      model: "gpt-4o-transcribe",
      durationMs: 184_000,
      source: { kind: "audio" },
      // A cloud provider answers with prose and reports no timings, so the
      // screen writes its one honest fallback cue. Left empty on purpose:
      // these two fixtures exist to show both halves.
      segments: [],
    }),
  media_import_video: () =>
    fixtureImport({
      text: "Recorded walkthrough of the meeting detector.",
      provider: "local",
      model: "whisper-large-v3-turbo-q5",
      durationMs: 921_000,
      source: { kind: "video" },
      // The local server does report timings, one per utterance.
      segments: [
        {
          text: "Recorded walkthrough of the meeting detector.",
          timestamp: 0,
          endTimestamp: 3.4,
        },
      ],
    }),
  /* calendars */
  calendar_status: () => CALENDAR,
  calendar_set_client_id: () => null,
  calendar_set_custom_client: () => null,
  calendar_set_selected: () => null,
  calendar_disconnect: () => null,
  calendar_connect: () =>
    ({
      state: "connected",
      accountEmail: "axel@example.com",
      calendars: 3,
    }) satisfies ConnectOutcome,
  calendar_sync: () =>
    ({
      state: "completed",
      eventsUpserted: 12,
      eventsRemoved: 1,
      calendarErrors: [],
    }) satisfies SyncOutcome,

  /* updates */
  update_check: () =>
    ({
      state: "update-available",
      current: "0.1.0-dev",
      latest: "0.2.0",
      url: "https://github.com/axelalbertkarlsson/KnegisWhisper/releases/tag/v0.2.0",
    }) satisfies UpdateCheckResult,
  update_check_last: () =>
    ({
      state: "up-to-date",
      current: "0.1.0-dev",
      latest: "0.1.0-dev",
    }) satisfies UpdateCheckResult,
  update_dismiss: () => null,

  diarization_status: () => diarization,
  diarization_set_enabled: (a) => {
    const enabled = arg<boolean>(a, "enabled") ?? false;
    // Rust refuses to switch them on with a model or the runtime missing.
    if (enabled && !diarization.ready) throw new Error("Speaker labels need both models: the segmentation model and the speaker embedding model. Install whichever is missing.");
    diarization = { ...diarization, enabled };
    return null;
  },
  meeting_detect_lists: () => detectLists,
  meeting_set_detect_lists: (a) => {
    detectLists = arg<Lists>(a, "lists") ?? detectLists;
    return null;
  },

  /* window */
  // `TitleBar` asks this on mount for every screen; unstubbed it threw on
  // load and covered the page in the "no stub" toast before anyone got to
  // look at anything else.
  "plugin:window|is_maximized": () => false,

  /* events sent to one window */
  /*
   * Stop and Discard ask the meeting window to save what was typed in it
   * (`flushMeetingPanelNotes`, `lib/ipc.ts`): `meeting-notes-flush` sent to
   * the `meeting` window, then up to 5 s for `meeting-notes-flushed` with the
   * same request id. This answers for that window, as `MeetingPanel` does once
   * its autosave has flushed, so both can finish here (KNEG-418) -- before
   * this, every Stop and Discard in the fixtures ran out the 5 s and stopped
   * there. Silent after `kw.meetingWindowSilent()`, so that wait can be seen
   * running out. Any other event is sent nowhere: there is no other window
   * here. `mockIPC` never calls this itself -- see `routeEmitTo`.
   */
  "plugin:event|emit_to": (a) => {
    const target = arg<{ kind: string; label?: string }>(a, "target");
    const payload = arg<{ requestId?: string }>(a, "payload");
    if (target?.label === "meeting" && arg<string>(a, "event") === "meeting-notes-flush" && !meetingWindowSilent) {
      void emit("meeting-notes-flushed", { requestId: payload?.requestId ?? "", error: null });
    }
    return null;
  },
};

/* ------------------------------------------------------------ dev console */

/**
 * Hand-holds for the states that only exist while something is happening.
 *
 * The overlay and the meeting notification are driven by events, so without
 * these they can only ever be seen at rest. Exposed on `globalThis` rather than
 * bound to keys, because a key handler added in dev is a key handler that
 * eventually ships.
 */
const devConsole = {
  dictationState(state: SessionState) {
    fixtureDictating = state === "recording";
    void emit("dictation-state", state);
  },
  /**
   * `#overlay` is its own page load, so the checkbox on Settings > Dictation
   * in a different tab cannot reach it. This flips the same
   * `overlay-preferences` event Rust would, so `#overlay` can be seen in its
   * floating-microphone mode instead of only the label one.
   */
  /**
   * The local windowed provider, usable and under the id Rust gives it. The
   * fixtures otherwise list it greyed, as `meeting:local`, for the screens that
   * explain a provider that can't be used.
   */
  localMeetingProvider() {
    const at = MEETING_PROVIDERS.findIndex((provider) => provider.id === "meeting:local");
    const listed = MEETING_PROVIDERS[at];
    if (listed) {
      MEETING_PROVIDERS[at] = { ...listed, id: "local", name: "Local windowed model", availability: { state: "available" } };
    }
  },
  speakerModels(installed = true) {
    diarization = {
      ...diarization,
      cliInstalled: installed,
      ready: installed,
      models: diarization.models.map((model) => ({ ...model, installed })),
    };
  },
  overlayFloating(enabled = true, vertical = false) {
    overlayPreferences = { ...overlayPreferences, enabled, vertical };
    void emit("overlay-preferences", overlayPreferences);
  },
  /**
   * A failed dictation, so `.microphone-error` -- the floating microphone's
   * own error banner -- can be seen without a real transcription failure.
   */
  dictationFailed(message = "No response from the transcription service.") {
    void emit("dictation-outcome", { outcome: "failed", message, historyId: null } satisfies Outcome);
  },
  /** Invented progress for UI checks. `snapshot=false` sends a deliberately stale event only. */
  meetingProgress(update: MeetingStatus["finalisation"], snapshot = true) {
    if (snapshot) meetingStatus = { ...meetingStatus, finalisation: update, state: update.progress ? "finishing" : "idle" };
    void emit("meeting-progress", update);
    if (snapshot) void emit("meeting-state", meetingStatus.state);
  },
  meetingState(state: MeetingState) {
    // The failure sentence travels with the state rather than being a second
    // thing to remember to set: either failure state with an empty detail line
    // is a rendering of this screen that cannot be reviewed, because the
    // sentence is most of what it exists to show.
    //
    // The `recording-without-transcript` words are the real ones, from the
    // meeting of 2026-08-28. The `stalled` words are **not** real: no recorder
    // thread has ever been seen to panic, and this is the sentence the code
    // would produce, not one anybody has read on a screen (KNEG-116).
    meetingStatus = {
      ...meetingStatus,
      state,
      offer: state === "idle" ? null : meetingStatus.offer,
      transcriptFailure:
        state === "recording-without-transcript"
          ? "openai-realtime accepted the connection and then stopped transcribing: on the system audio stream the provider reported: The Realtime Beta API is no longer supported."
          : state === "stalled"
            ? "the recording thread stopped unexpectedly, so nothing further is being recorded or transcribed. What was captured before it stopped is still on disk, and Stop will transcribe it."
            : null,
    };
    void emit("meeting-state", state);
  },
  /** `extra` overrides the rest, e.g. `{ source: "system", speaker: null, timestamp: 641 }`. */
  transcript(text: string, isFinal = true, extra: Partial<TranscriptEvent> = {}) {
    void emit("meeting-transcript", {
      text,
      isFinal,
      timestamp: meetingStatus.elapsedSeconds,
      speaker: "Anna",
      source: "mic",
      ...extra,
    } satisfies TranscriptEvent);
  },
  /**
   * An interrupted recording, as an earlier session would leave one. Read
   * when the Meetings page next loads, so open it (or leave and come back)
   * after calling this. `title` is the name it was given before Start, as
   * its plan sidecar would carry it (KNEG-365).
   */
  interrupted(storageKey = "20260901-101500-3f2a9c1e.wav", hasNotes = true, title: string | null = null) {
    recoverable = [...recoverable.filter((r) => r.storageKey !== storageKey), { storageKey, hasNotes, title }];
  },
  offer(name = "Zoom") {
    const next: Offer = {
      provider: "meeting-detect:zoom",
      name,
      evidence: "CptHost.exe",
      signal: "call-process",
    };
    meetingStatus = { ...meetingStatus, offer: next };
    void emit("meeting-offer", next);
  },
  /** The other offer, and the one worth looking at: it appeared on the weaker
   * signal, so the window has to say so rather than claiming a meeting. */
  micOffer(name = "Microsoft Teams") {
    const next: Offer = {
      provider: "meeting-detect:teams",
      name,
      evidence: "ms-teams.exe",
      signal: "microphone",
    };
    meetingStatus = { ...meetingStatus, offer: next };
    void emit("meeting-offer", next);
  },
  clearOffer() {
    meetingStatus = { ...meetingStatus, offer: null };
    void emit("meeting-offer", null);
  },
  /**
   * The meeting window stops answering when Stop or Discard asks it to save
   * its notes, as a window that hung would: the page waits its 5 s, then
   * offers "Stop recording anyway" and "Discard anyway" (KNEG-418). Start a
   * meeting first; `kw.meetingWindowSilent(false)` answers again.
   */
  meetingWindowSilent(silent = true) {
    meetingWindowSilent = silent;
  },
  /**
   * Fake levels for `session`'s microphone, ten a second, as a running
   * capture's publisher sends them; `false` stops them with the last 0 Rust
   * sends. For dictation and the overlay (with `kw.dictationState`), whose
   * recording the fixtures never start; a meeting's run from its Start.
   */
  levels(session: LevelSession = "dictation", running = true) {
    if (running) startFakeLevels(session, ["microphone"]);
    else stopFakeLevels(session);
  },
  /** One reading, e.g. `kw.level(0.75)`, for a meter to be seen at a known value. */
  level(level: number, session: LevelSession = "dictation", source: LevelSource = "microphone") {
    void emit("audio-level", { session, source, level } satisfies AudioLevel);
  },
  /** A model download's progress, for `id` (Whisper Large v3 Turbo unless named). */
  modelProgress(written: number, total: number | null = 574_000_000, id = "whisper-large-v3-turbo-q5") {
    void emit("local-model-progress", {
      stage: "model",
      id,
      written,
      total,
    } satisfies InstallProgress);
  },
  llmProgress(written: number, total: number | null = 4_680_000_000) {
    void emit("local-llm-progress", {
      stage: "model",
      id: "qwen2.5-7b-instruct-q4",
      written,
      total,
    } satisfies LocalLlmProgress);
  },
};

declare global {
  // eslint-disable-next-line no-var
  var kw: typeof devConsole | undefined;
}

/* ------------------------------------------------------------------ install */

/**
 * Say on the page, not just in the console, that nothing here is real.
 *
 * Installed by `installDevFixtures` itself so the two cannot drift: there is no
 * arrangement of this module that mocks the IPC without also marking the
 * screen. The console notice below is not enough on its own -- a bench run on
 * 2026-08-27 recorded two steps as *verified* from this browser tab, against
 * invented pass results that looked exactly like real ones, and nothing on
 * screen contradicted them. Written inline rather than as a class in
 * `styles.css` because it must survive a stylesheet that has not loaded, a
 * theme that inverts, and any rule a view might add later.
 *
 * The title prefix matters as much as the bar: with the Tauri window and this
 * tab both open, the taskbar is where the two get confused.
 */
function markThePageAsInvented(): void {
  document.title = `FIXTURES — ${document.title}`;

  const bar = document.createElement("div");
  bar.dataset.devFixtures = "banner";
  bar.textContent =
    "DEV FIXTURES — no Rust behind this page. Every value is invented, " +
    "including anything that reads as a pass. Use the KnegisWhisper " +
    "application window, not this browser tab.";
  bar.style.cssText = [
    "position:fixed",
    "inset:0 0 auto 0",
    "z-index:2147483647",
    "padding:0.55rem 1rem",
    "background:#7f1d1d",
    "color:#fff",
    "font:600 13px/1.45 ui-sans-serif,system-ui,sans-serif",
    "text-align:center",
    "letter-spacing:0.01em",
    "border-bottom:2px solid #fca5a5",
    "pointer-events:none",
  ].join(";");

  const show = () => {
    document.body.style.paddingTop = "2.6rem";
    document.body.appendChild(bar);
  };

  if (document.body) {
    show();
  } else {
    document.addEventListener("DOMContentLoaded", show, { once: true });
  }
}

/**
 * The URL a fixture recording of `seconds` plays from: `toneWav`'s tone, built the first time a
 * note with a recording of that length asks and then kept, one per length, so only the long note
 * pays for its 690 s. Kept because `recordingSrc` runs on every render: a new blob URL each time
 * would hand the `<audio>` a new `src` and reload it mid-playback.
 */
const fixtureRecordingUrls = new Map<number, string>();
function fixtureRecording(seconds: number): string {
  let url = fixtureRecordingUrls.get(seconds);
  if (url === undefined) {
    url = URL.createObjectURL(new Blob([toneWav({ seconds })], { type: "audio/wav" }));
    fixtureRecordingUrls.set(seconds, url);
  }
  return url;
}

/**
 * Answers the player's `recording` URL (`lib/ipc.ts`' `recordingSrc`) for
 * every fixture note that has a recording, as Rust's scheme answers it in the
 * app, so the player plays in browser dev and a headless check can measure
 * playback (KNEG-412, `tools/visual-check/note-playback.mjs`). Before this the
 * fixtures had nothing behind that URL and the player could only show its
 * error. Every other URL still goes to Tauri's mock, which nothing answers,
 * so an id with no recording fails as Rust's 404 would.
 */
function servePlayableRecordings(): void {
  const internals = (window as unknown as {
    __TAURI_INTERNALS__: { convertFileSrc: (filePath: string, protocol?: string) => string };
  }).__TAURI_INTERNALS__;
  const mocked = internals.convertFileSrc.bind(internals);
  internals.convertFileSrc = (filePath, protocol = "asset") =>
    protocol === "recording" && noteRecordings.has(Number(filePath))
      ? fixtureRecording(noteRecordings.get(Number(filePath))?.durationSeconds ?? FIXTURE_RECORDING_SECONDS)
      : mocked(filePath, protocol);
}

/**
 * Sends `plugin:event|emit_to` to its handler. With `shouldMockEvents`,
 * `mockIPC` answers every `plugin:event|` command itself, before the handler
 * table is asked, and has no case for `emit_to`: it resolves to nothing and
 * nothing hears the event. So this wraps the invoke `mockIPC` installed and
 * takes that one command out from under it; every other command still goes
 * to the mock. Measured: with the handler in the table and no wrapper, Stop
 * still ran out its 5 s (`tools/visual-check/meeting-discard.mjs`, KNEG-418).
 */
function routeEmitTo(): void {
  const internals = (window as unknown as {
    __TAURI_INTERNALS__: { invoke: (cmd: string, args?: unknown, options?: unknown) => Promise<unknown> };
  }).__TAURI_INTERNALS__;
  const mocked = internals.invoke;
  const handler = HANDLERS["plugin:event|emit_to"];
  if (handler === undefined) throw new Error("dev fixtures: no stub for \"plugin:event|emit_to\"");
  internals.invoke = (cmd, args, options) =>
    cmd === "plugin:event|emit_to"
      ? Promise.resolve().then(() => handler((args ?? {}) as Record<string, unknown>))
      : mocked(cmd, args, options);
}

export function installDevFixtures(): void {
  markThePageAsInvented();
  mockWindows("main");
  mockConvertFileSrc("windows");
  servePlayableRecordings();
  mockIPC(
    (cmd, payload) => {
      const handler = HANDLERS[cmd];
      if (handler === undefined) {
        // Loud, rather than a plausible-looking empty value. A stub that
        // invents a shape is how a screen comes to be judged against data the
        // backend would never send.
        const problem = `dev fixtures: no stub for "${cmd}"`;
        console.error(problem, payload);
        throw new Error(problem);
      }
      return handler((payload ?? {}) as Record<string, unknown>);
    },
    { shouldMockEvents: true },
  );
  routeEmitTo();

  globalThis.kw = devConsole;
  console.info(
    "%cKnegisWhisper dev fixtures active",
    "font-weight:bold",
    [
      "",
      "No Rust behind this — every value is invented.",
      "Windows:  /   ·   /#overlay   ·   /#meeting",
      "Try:      kw.dictationState('recording')",
      "          kw.offer()",
      "          kw.meetingState('recording')",
      "          kw.meetingWindowSilent()",
      "          kw.overlayFloating()",
      "          kw.speakerModels()",
      "          kw.localMeetingProvider()",
      "          kw.dictationFailed()",
      "          kw.levels()  ·  kw.level(0.75)",
    ].join("\n  "),
  );
}
