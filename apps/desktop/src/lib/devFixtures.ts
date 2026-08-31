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
import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";

import type {
  Accelerator,
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
  ExportedTranscript,
  Folder,
  Format,
  HotkeyAction,
  HotkeyChange,
  HotkeyReport,
  ImportReport,
  InputDevice,
  InstallProgress,
  Lists,
  LocalApiStatus,
  LocalLlmProgress,
  LocalLlmStatus,
  LocalStatus,
  MeetingOutcome,
  MeetingProviderSummary,
  MeetingState,
  MeetingStatus,
  NewNoteRequest,
  Note,
  NoteUpdateRequest,
  Offer,
  Outcome,
  ProbeReport,
  ProviderSummary,
  ReasoningStatus,
  RouteReport,
  SearchHit,
  SessionState,
  Sources,
  TranscriptEvent,
  Transcription,
  UpdateCheckResult,
} from "../generated/ipc-types";

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
  { action: "dictation", accelerators: [standard("Control+Shift+D")] },
  {
    action: "chat-agent",
    accelerators: [
      {
        kind: "modifier-only",
        accelerator: "Control+Super",
        modifiers: ["Control", "Super"],
      },
    ],
  },
  { action: "voice-agent", accelerators: [standard("Control+Shift+V")] },
  {
    action: "translation",
    accelerators: [
      { kind: "invalid", input: "Ctrl+Shft+T", reason: "unknown key `Shft`" },
    ],
  },
  { action: "meeting", accelerators: [standard("Control+Shift+V")] },
];

const HOTKEYS: HotkeyReport = {
  bindings: BINDINGS,
  conflicts: [
    { accelerator: "Control+Shift+V", actions: ["voice-agent", "meeting"] },
  ],
  unusable: ["translation", "voice-agent"],
  unhandled: [
    {
      action: "voice-agent",
      reason: "no voice-agent client is built into this release",
    },
  ],
};

/* --------------------------------------------------------------- providers */

const PROVIDERS: ProviderSummary[] = [
  {
    id: "openai",
    name: "OpenAI",
    credentialSlot: "provider:openai",
    configured: true,
    modelCount: 3,
    models: ["whisper-1", "gpt-4o-transcribe", "gpt-4o-mini-transcribe"],
    dispatch: { state: "ready" },
  },
  {
    id: "deepgram",
    name: "Deepgram",
    credentialSlot: "provider:deepgram",
    configured: true,
    modelCount: 2,
    models: ["nova-3", "nova-2"],
    dispatch: { state: "ready" },
  },
  {
    id: "assemblyai",
    name: "AssemblyAI",
    credentialSlot: "provider:assemblyai",
    configured: false,
    modelCount: 2,
    models: ["universal", "universal-streaming"],
    dispatch: { state: "ready" },
  },
  {
    id: "groq",
    name: "Groq",
    credentialSlot: "provider:groq",
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
    credentialSlot: "provider:corti",
    configured: false,
    modelCount: 1,
    models: ["premier"],
    dispatch: {
      state: "unresolvable",
      reason: "Corti needs an environment and a tenant before it can resolve",
    },
  },
];

const CREDENTIAL_SLOTS: CredentialSlotStatus[] = [
  {
    slot: "provider:openai",
    name: "OpenAI API key",
    kind: "provider",
    usedFor: ["transcription", "cleanup", "meeting transcription"],
    feature: null,
    stored: true,
  },
  {
    slot: "provider:deepgram",
    name: "Deepgram API key",
    kind: "provider",
    usedFor: ["transcription", "meeting transcription"],
    feature: null,
    stored: true,
  },
  {
    slot: "provider:assemblyai",
    name: "AssemblyAI API key",
    kind: "provider",
    usedFor: ["transcription"],
    feature: null,
    stored: false,
  },
  {
    slot: "provider:groq",
    name: "Groq API key",
    kind: "provider",
    usedFor: ["transcription"],
    feature: null,
    stored: false,
  },
  {
    slot: "custom:cleanup-endpoint",
    name: "Self-hosted cleanup endpoint key",
    kind: "custom-endpoint",
    usedFor: ["cleanup"],
    feature: "Intelligence",
    stored: false,
  },
  {
    slot: "enterprise:bedrock",
    name: "AWS Bedrock",
    kind: "enterprise",
    usedFor: ["cleanup"],
    feature: "Intelligence",
    stored: true,
  },
];

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
      provider: "deepgram",
      kind: "catalog",
      outcome: { outcome: "pass", detail: "12 models listed in 180 ms" },
    },
    {
      provider: "deepgram",
      kind: "voice",
      outcome: {
        outcome: "fail",
        detail: "401 Unauthorized — the stored key was rejected",
      },
    },
    {
      provider: "assemblyai",
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
      credentialSlot: "provider:openai",
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
      credentialSlot: "provider:anthropic",
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
      name: "Local model",
      credentialSlot: null,
      configured: true,
      enterprise: false,
      dispatch: {
        state: "needs-sidecar",
        reason: "the llama.cpp sidecar is not installed",
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
    availability: {
      state: "declined",
      reason: "the local server is installed but not running",
    },
    summary: "Audio never leaves this machine. No partials, no speaker labels.",
  },
];

let meetingStatus: MeetingStatus = {
  state: "idle",
  provider: null,
  elapsedSeconds: 0,
  sources: null,
  systemAudio: DIAGNOSTICS.systemAudio,
  detected: [
    {
      provider: "meeting-detect:teams",
      name: "Microsoft Teams",
      evidence: "ms-teams.exe",
      inCall: false,
    },
    {
      provider: "meeting-detect:zoom",
      name: "Zoom",
      evidence: "CptHost.exe",
      inCall: true,
    },
  ],
  promptOnDetect: true,
  notificationExcluded: {
    state: "unsupported",
    reason: "this platform cannot exclude a window from screen capture",
  },
};

let meetingLanguage = "en";
let meetingKeyterms = ["Nordbank", "KnegisWhisper", "WASAPI", "Realtek"];

let detectLists: Lists = {
  allow: ["webex.exe", "bluejeans.exe"],
  block: ["Teams.exe"],
};

/* -------------------------------------------------------------- dictation */

const INPUT_DEVICES: InputDevice[] = [
  { name: "Microphone Array (Realtek(R) Audio)", isDefault: true },
  { name: "Headset (Jabra Evolve2 65)", isDefault: false },
  { name: "Line In (Focusrite Scarlett 2i2)", isDefault: false },
];

const ROUTE: RouteReport = {
  destination: "openai",
  isLocal: false,
  leavesDevice: true,
  problem: null,
};

/* ------------------------------------------------------------ local models */

const LOCAL_MODELS: LocalStatus = {
  availability: { state: "installable" },
  serverInstalled: true,
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
  // Deliberately disagreeing with ROUTE, which resolves to openai: the local
  // server is up with a model loaded and dictation is not using it. That is the
  // state the marked button must *not* claim, so the fixtures show it rather
  // than the easy case.
  runningModel: "whisper-small-q5",
  installDir: "C:\\ProgramData\\KnegisWhisper\\whisper-server",
  installDirFellBack: true,
};

const SHERPA: SherpaStatus = {
  availability: { state: "installable" },
  cliInstalled: true,
  // One installed and selected, one installable, and the two streaming models
  // carrying the reason they cannot be picked -- the three states this list has,
  // so the screen is worked on against all of them rather than the easy one.
  models: [
    {
      id: "parakeet-unified-en-0.6b",
      name: "Parakeet Unified EN 0.6B",
      sizeMb: 630,
      language: "en",
      installed: true,
      notOffered: null,
    },
    {
      id: "parakeet-tdt-0.6b-v3",
      name: "Parakeet TDT 0.6B v3",
      sizeMb: 660,
      language: null,
      installed: false,
      notOffered: null,
    },
    {
      id: "nemotron-streaming-en",
      name: "Nemotron Streaming EN",
      sizeMb: 480,
      language: "en",
      installed: false,
      notOffered:
        "streaming model — it is served over sherpa-onnx's own WebSocket protocol, and no client for that protocol has been written",
    },
  ],
  selected: "parakeet-unified-en-0.6b",
  modelsDir: "C:\\ProgramData\\KnegisWhisper\\models\\sherpa",
};

const LOCAL_LLM: LocalLlmStatus = {
  availability: {
    state: "not-published",
    reason: "no llama.cpp build is published for windows-aarch64",
    tracking: "https://github.com/ggml-org/llama.cpp/releases",
  },
  serverInstalled: false,
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
      installed: false,
      // The ordinary case: thirty-two of the thirty-nine published objects are
      // checked against the publisher's stated digest, not one measured here.
      verifiedBy: "stated-digest",
      draft: {
        fileName: "qwen2.5-0.5b-instruct-q4.gguf",
        sizeBytes: 352_000_000,
        installed: false,
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
      installed: false,
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
      installed: false,
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
  binaryName: "knegiswhisper-mcp.exe",
  /* Not found on purpose: the screen has a sentence for a binary it cannot
   * locate, and a fixture that always finds one never shows it. */
  binaryLocation: { state: "not-found" },
  command: "knegiswhisper-mcp --stdio",
  localApiRunning: true,
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
    clientId: "918273645-abcdef.apps.googleusercontent.com",
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
    clientId: null,
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
    status: "ok",
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
    status: "ok",
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
    provider: "deepgram",
    model: "nova-3",
    status: "failed",
    errorMessage: "401 Unauthorized — the stored key was rejected",
    errorCode: "auth",
    routeKind: "cloud",
  },
  {
    id: 38,
    text: "",
    rawText: null,
    createdAt: "2026-08-25T17:02:44Z",
    hasAudio: false,
    audioDurationMs: 300,
    provider: null,
    model: null,
    status: "nothing-to-transcribe",
    errorMessage: "only silence was captured",
    errorCode: "silence",
    routeKind: null,
  },
];

let folders: Folder[] = [
  { id: 1, name: "Inbox", isDefault: true, sortOrder: 0 },
  { id: 2, name: "Customer calls", isDefault: false, sortOrder: 1 },
  { id: 3, name: "Internal", isDefault: false, sortOrder: 2 },
];

let notes: Note[] = [
  {
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
    createdAt: "2026-08-26T09:00:00Z",
    updatedAt: "2026-08-26T09:31:00Z",
  },
  {
    id: 102,
    title: "Customer call — Nordbank",
    content:
      "They want on-prem transcription only. Cloud providers are a non-starter for the compliance team.",
    noteType: "meeting",
    enhancedContent: null,
    folderId: 2,
    transcript: null,
    participants: JSON.stringify([
      { displayName: "Jonas Berg", email: "jonas@nordbank.example" },
    ]),
    audioDurationSeconds: 2612,
    createdAt: "2026-08-25T13:15:00Z",
    updatedAt: "2026-08-25T14:02:00Z",
  },
  {
    id: 103,
    title: "Scratch",
    content: "check whether WASAPI loopback survives a device change mid-call",
    noteType: "note",
    enhancedContent: null,
    folderId: 1,
    transcript: null,
    participants: null,
    audioDurationSeconds: null,
    createdAt: "2026-08-24T08:20:00Z",
    updatedAt: "2026-08-24T08:20:00Z",
  },
  {
    id: 104,
    title:
      "A deliberately long title that exists so the notes list can be judged when someone types far more than the column was designed for",
    content:
      "Long-form body text, here so wrapping and vertical rhythm can be seen honestly rather than guessed at.\n\n" +
      "The overlay, the sidebar and this pane all share `styles.css`, so a change made against three short notes tends to look fine and then fall apart on a real one. " +
      "This note is the real one.\n\nIt has several paragraphs, a list:\n\n- one\n- two\n- three\n\nand a trailing line.",
    noteType: "note",
    enhancedContent: null,
    folderId: null,
    transcript: null,
    participants: null,
    audioDurationSeconds: null,
    createdAt: "2026-08-20T19:45:00Z",
    updatedAt: "2026-08-23T07:10:00Z",
  },
];

let nextNoteId = 105;
let nextFolderId = 4;

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

/* ---------------------------------------------------------------- handlers */

const HANDLERS: Record<string, (args: Record<string, unknown>) => unknown> = {
  /* capabilities */
  diagnostics: () => DIAGNOSTICS,

  /* hotkeys */
  hotkeys: () => HOTKEYS,
  hotkey_set: (a) => {
    const action = arg<HotkeyAction>(a, "action") ?? "dictation";
    const accelerators = arg<string[]>(a, "accelerators") ?? [];
    const change: HotkeyChange = {
      binding: { action, accelerators: accelerators.map(standard) },
      registrations: accelerators.map((accelerator) => ({
        state: "registered" as const,
        accelerator,
      })),
      live: accelerators.length > 0,
    };
    return change;
  },

  /* providers */
  providers: () => PROVIDERS,
  credential_slots: () => CREDENTIAL_SLOTS,
  provider_credential_set: () => null,
  provider_credential_present: (a) =>
    CREDENTIAL_SLOTS.find((s) => s.slot === arg<string>(a, "slot"))?.stored ??
    false,
  probe_providers: () => PROBE_REPORT,

  /* reasoning */
  reasoning_status: () => REASONING,
  reasoning_set_cleanup: (a) => {
    const settings = arg<CleanupSettings>(a, "settings");
    if (!settings?.enabled) return { state: "off" } satisfies CleanupRoute;
    const route: CleanupRoute = {
      state: "ready",
      destination: settings.selfHostedUrl
        ? "self-hosted"
        : (settings.provider ?? "openai"),
      model: settings.model ?? "gpt-4o-mini",
      leavesDevice: !settings.selfHostedUrl,
      caveat: null,
    };
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
    const limit = arg<number | null>(a, "limit");
    let out = notes;
    if (noteType) out = out.filter((n) => n.noteType === noteType);
    if (typeof folderId === "number") {
      out = out.filter((n) => n.folderId === folderId);
    }
    out = [...out].sort((x, y) =>
      (y.updatedAt ?? "").localeCompare(x.updatedAt ?? ""),
    );
    return typeof limit === "number" ? out.slice(0, limit) : out;
  },
  notes_search: (a) => {
    const query = (arg<string>(a, "query") ?? "").trim().toLowerCase();
    if (query === "") return [] satisfies SearchHit[];
    return notes
      .filter(
        (n) =>
          n.title.toLowerCase().includes(query) ||
          n.content.toLowerCase().includes(query),
      )
      .map((n): SearchHit => {
        const at = n.content.toLowerCase().indexOf(query);
        if (at === -1) {
          return { id: n.id, title: n.title, snippet: n.content.slice(0, 90) };
        }
        const before = n.content.slice(Math.max(0, at - 35), at);
        const hit = n.content.slice(at, at + query.length);
        const after = n.content.slice(at + query.length, at + query.length + 35);
        return { id: n.id, title: n.title, snippet: `${before}[${hit}]${after}` };
      });
  },
  notes_get: (a) => notes.find((n) => n.id === arg<number>(a, "id")) ?? null,
  notes_create: (a) => {
    const r = arg<NewNoteRequest>(a, "request");
    const id = nextNoteId++;
    notes = [
      {
        id,
        title: r?.title ?? "Untitled",
        content: r?.content ?? "",
        noteType: r?.noteType ?? "note",
        enhancedContent: null,
        folderId: r?.folderId ?? null,
        transcript: r?.transcript ?? null,
        participants: r?.participants ?? null,
        audioDurationSeconds: r?.audioDurationSeconds ?? null,
        createdAt: nowIso(),
        updatedAt: nowIso(),
      },
      ...notes,
    ];
    return id;
  },
  notes_update: (a) => {
    const id = arg<number>(a, "id");
    const r = arg<NoteUpdateRequest>(a, "request");
    if (!notes.some((n) => n.id === id)) return false;
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
  notes_delete: (a) => {
    const id = arg<number>(a, "id");
    const before = notes.length;
    notes = notes.filter((n) => n.id !== id);
    return notes.length !== before;
  },
  folders_list: () => folders,
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
        { sourceTable: "settings", account: "openai", slot: "provider:openai" },
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
  dictation_set_device: () => null,
  insertion_supported: () => true,
  dictation_route: () => ROUTE,
  dictation_use_local: () => null,
  dictation_set_cloud_provider: () => null,

  /* local transcription */
  sherpa_status: () => SHERPA,
  sherpa_install_model: () => null,
  sherpa_use_model: () => null,
  local_model_status: () => LOCAL_MODELS,
  local_model_install: () => "whisper-large-v3-turbo-q5",
  local_server_start: () => 8178,
  local_server_stop: () => null,

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
  meeting_start: (a) => {
    const wantsSystemAudio = arg<boolean>(a, "systemAudio") ?? false;
    const sources: Sources = {
      microphone: true,
      systemAudio: wantsSystemAudio,
      systemAudioReason: wantsSystemAudio
        ? null
        : "system audio was not requested",
    };
    meetingStatus = {
      ...meetingStatus,
      state: "recording",
      provider: arg<string>(a, "provider") ?? "meeting:deepgram",
      elapsedSeconds: 0,
      sources,
    };
    return sources;
  },
  meeting_stop: () => {
    meetingStatus = {
      ...meetingStatus,
      state: "idle",
      provider: null,
      elapsedSeconds: 0,
      sources: null,
    };
    return {
      outcome: "saved",
      noteId: 101,
      durationSeconds: 1847,
      recordingPath:
        "C:\\Users\\dev\\AppData\\Roaming\\KnegisWhisper\\recordings\\2026-08-26T09-00.wav",
      gaps: 0,
      transcriptIsPartials: false,
    } satisfies MeetingOutcome;
  },
  meeting_cancel: () => {
    meetingStatus = {
      ...meetingStatus,
      state: "idle",
      provider: null,
      elapsedSeconds: 0,
      sources: null,
    };
    return null;
  },
  meeting_dismiss: () => null,
  meeting_set_prompt: (a) => {
    meetingStatus = {
      ...meetingStatus,
      promptOnDetect: arg<boolean>(a, "enabled") ?? false,
    };
    return null;
  },
  meeting_language: () => meetingLanguage,
  meeting_set_language: (a) => {
    meetingLanguage = arg<string>(a, "language") ?? meetingLanguage;
    return null;
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
  media_import_install_sidecar: () => null,
  pick_import_file: () => "C:\\Users\\dev\\Music\\interview.m4a",
  media_import_audio: () =>
    ({
      text: "So the localisation pass is the blocker, and Anna owns it.",
      provider: "openai",
      model: "gpt-4o-transcribe",
      durationMs: 184_000,
      source: { kind: "audio" },
      // A cloud provider answers with prose and reports no timings, so the
      // screen writes its one honest fallback cue. Left empty on purpose:
      // these two fixtures exist to show both halves.
      segments: [],
    }) satisfies MediaImportResult,
  media_import_video: () =>
    ({
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
    }) satisfies MediaImportResult,
  /* calendars */
  calendar_status: () => CALENDAR,
  calendar_set_client_id: () => null,
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

  meeting_detect_lists: () => detectLists,
  meeting_set_detect_lists: (a) => {
    detectLists = arg<Lists>(a, "lists") ?? detectLists;
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
    void emit("dictation-state", state);
  },
  meetingState(state: MeetingState) {
    meetingStatus = { ...meetingStatus, state };
    void emit("meeting-state", state);
  },
  transcript(text: string, isFinal = true) {
    void emit("meeting-transcript", {
      text,
      isFinal,
      timestamp: meetingStatus.elapsedSeconds,
      speaker: "Anna",
      source: "mic",
    } satisfies TranscriptEvent);
  },
  offer(name = "Zoom") {
    void emit("meeting-offer", {
      provider: "meeting-detect:zoom",
      name,
      evidence: "CptHost.exe",
    } satisfies Offer);
  },
  clearOffer() {
    void emit("meeting-offer", null);
  },
  modelProgress(written: number, total: number | null = 574_000_000) {
    void emit("local-model-progress", {
      stage: "model",
      id: "whisper-large-v3-turbo-q5",
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

export function installDevFixtures(): void {
  markThePageAsInvented();
  mockWindows("main");
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
    ].join("\n  "),
  );
}
