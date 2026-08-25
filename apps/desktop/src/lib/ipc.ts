/**
 * Typed wrappers over the Tauri command boundary.
 *
 * Every call into Rust goes through this file. React components never call
 * `invoke` directly, so the set of commands the frontend can reach is greppable
 * in one place, and the payload types live next to the call rather than being
 * re-declared at each use site.
 *
 * These types must match the Rust structs they mirror. Where a field here does
 * not exist in Rust, it silently reads `undefined` and any UI built on it is
 * showing a fiction — which is how the `electronPresent` field, present in an
 * earlier version of this file, came to render "not present" without checking
 * anything. If you add a field, add it on both sides.
 */

import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

/* ------------------------------------------------------------- capabilities */

/** How we know a capture-exclusion claim is true. Mirrors `overlay::Evidence`. */
export type Evidence = {
  kind: "read-back";
  api: string;
  value: string;
};

/**
 * Mirrors `overlay::CaptureExclusion`. Four states, not a boolean — see
 * `docs/PLATFORM_SUPPORT.md` for why.
 */
export type CaptureExclusion =
  | { state: "active"; verifiedBy: Evidence }
  | { state: "unsupported"; reason: string }
  | { state: "failed"; error: string }
  | { state: "unknown"; reason: string };

/** Only `active` means the overlay is genuinely hidden from screen capture. */
export function isCaptureExclusionConfirmed(c: CaptureExclusion): boolean {
  return c.state === "active";
}

/** Mirrors `overlay::NonActivating`. */
export type NonActivating =
  | { state: "active"; verifiedBy: string }
  | { state: "unsupported"; reason: string }
  | { state: "failed"; error: string };

export type OverlayStatus = {
  created: boolean;
  captureExclusion: CaptureExclusion;
  nonActivating: NonActivating;
  /** The local user always sees an indicator while the microphone is live. */
  recordingIndicatorVisible: boolean;
  displayServer: string;
};

export type CredentialStoreStatus =
  | { state: "available"; backend: string }
  | { state: "unavailable"; reason: string };

export type ModelCacheStatus = {
  path: string;
  /** True when the per-user path was unusable by the native ASR binaries. */
  fellBack: boolean;
};

export type Diagnostics = {
  appVersion: string;
  os: string;
  arch: string;
  /** The system webview version, or null if it could not be determined. */
  webviewVersion: string | null;
  credentialStore: CredentialStoreStatus;
  overlay: OverlayStatus;
  /** Egress purposes the network policy layer will accept. */
  allowedEgressPurposes: string[];
  modelCache: ModelCacheStatus;
};

export async function getDiagnostics(): Promise<Diagnostics> {
  return invoke<Diagnostics>("diagnostics");
}

/* ----------------------------------------------------------------- hotkeys */

export type HotkeyAction =
  | "dictation"
  | "chat-agent"
  | "voice-agent"
  | "translation"
  | "meeting";

/** Mirrors `platform::hotkey::Accelerator`. */
export type Accelerator =
  | { kind: "standard"; accelerator: string }
  | { kind: "modifier-only"; accelerator: string; modifiers: string[] }
  | { kind: "invalid"; input: string; reason: string };

export type Binding = {
  action: HotkeyAction;
  accelerators: Accelerator[];
};

export type Conflict = {
  accelerator: string;
  actions: HotkeyAction[];
};

export type HotkeyReport = {
  bindings: Binding[];
  conflicts: Conflict[];
  /** Actions with no working hotkey at all. */
  unusable: HotkeyAction[];
};

export async function getHotkeys(): Promise<HotkeyReport> {
  return invoke<HotkeyReport>("hotkeys");
}

export async function setHotkey(
  action: HotkeyAction,
  accelerators: string,
): Promise<Binding> {
  return invoke<Binding>("hotkey_set", { action, accelerators });
}

/** Fired when a registered hotkey is pressed. */
export type HotkeyFired = {
  action: HotkeyAction;
  accelerator: string;
};

export async function onHotkey(
  handler: (fired: HotkeyFired) => void,
): Promise<UnlistenFn> {
  return listen<HotkeyFired>("hotkey", (event) => handler(event.payload));
}

/* ---------------------------------------------------------------- providers */

export type ProviderSummary = {
  id: string;
  name: string;
  credentialSlot: string;
  /** A credential is stored. This is NOT the same as the provider working. */
  configured: boolean;
  modelCount: number;
};

export async function getProviders(): Promise<ProviderSummary[]> {
  return invoke<ProviderSummary[]>("providers");
}

export async function setProviderCredential(
  slot: string,
  value: string,
): Promise<void> {
  return invoke("provider_credential_set", { slot, value });
}

export type ProbeKind = "catalog" | "chat" | "voice";

export type SkipReason =
  | { reason: "no-credential" }
  | { reason: "not-supported"; detail: string };

export type ProbeOutcome =
  | { outcome: "pass"; detail: string }
  | { outcome: "fail"; detail: string }
  | { outcome: "skipped" } & Partial<SkipReason>;

export type ProbeResult = {
  provider: string;
  kind: ProbeKind;
  outcome: ProbeOutcome;
};

export type ProbeReport = {
  results: ProbeResult[];
  /** `tone` means a pass proves endpoint and credential, not recognition. */
  audio: "speech" | "tone";
};

/**
 * Run real connection tests. This is the only thing that may establish that a
 * provider is connected — a stored credential does not.
 */
export async function probeProviders(): Promise<ProbeReport> {
  return invoke<ProbeReport>("probe_providers");
}

/**
 * Whether `provider` may be shown as connected: at least one probe ran for it
 * and none failed. Mirrors `ProbeReport::is_connected` in Rust; kept in step by
 * a test on the Rust side and deliberately duplicated here rather than
 * recomputed ad hoc in a component.
 */
export function isProviderConnected(
  report: ProbeReport,
  provider: string,
): boolean {
  const mine = report.results.filter((r) => r.provider === provider);
  const ran = mine.some((r) => r.outcome.outcome !== "skipped");
  return ran && !mine.some((r) => r.outcome.outcome === "fail");
}

/* ------------------------------------------------------------------ history */

export type Transcription = {
  id: number;
  text: string;
  rawText: string | null;
  createdAt: string | null;
  hasAudio: boolean;
  audioDurationMs: number | null;
  provider: string | null;
  model: string | null;
  status: string;
  errorMessage: string | null;
  errorCode: string | null;
  routeKind: string | null;
};

export async function getHistory(
  limit?: number,
  after?: number,
): Promise<Transcription[]> {
  // `after` is a cursor, not an offset: entries are ordered by id descending,
  // so paging by id stays stable while new dictations arrive.
  return invoke<Transcription[]>("history", {
    limit: limit ?? null,
    after: after ?? null,
  });
}

export async function clearHistory(): Promise<number> {
  return invoke<number>("history_clear");
}

/* ------------------------------------------------------------------ settings */

export async function getSetting(key: string): Promise<unknown> {
  return invoke<unknown>("settings_get", { key });
}

export async function setSetting(key: string, value: unknown): Promise<void> {
  return invoke("settings_set", { key, value });
}

/* -------------------------------------------------------------------- import */

export type TableReport = {
  table: string;
  copied: number;
  skipped: number;
  absent_columns: string[];
  dropped_columns: string[];
  skip_reason: string | null;
};

export type ImportReport = {
  source: string;
  tables: TableReport[];
  relocated_credentials: {
    source_table: string;
    account: string;
    slot: string;
  }[];
  absent_tables: string[];
  fts_rows: number;
};

export async function importOpenWhispr(path: string): Promise<ImportReport> {
  return invoke<ImportReport>("import_openwhispr", { request: { path } });
}

/* ---------------------------------------------------------------- dictation */

/**
 * Mirrors `kw_app_core::dictation::SessionState`.
 *
 * `recording` is the one that matters: whenever the session is in it, the
 * overlay is visible. The local user is never silently recorded.
 */
export type SessionState = "idle" | "recording" | "transcribing" | "inserting";

/**
 * Why the speech gate decided what it did. Mirrors `kw_audio_core::GateReason`,
 * kebab-cased.
 *
 * `unavailable` means nothing could be measured, and the gate fails *open* in
 * that case — discarding a dictation someone spoke is worse than transcribing
 * silence.
 */
export type GateReason =
  | "unavailable"
  | "silence"
  | "insufficient-speech"
  | "speech-detected";

/** How a dictation ended. Mirrors `kw_app_core::dictation::Outcome`. */
export type DictationOutcome =
  | {
      outcome: "inserted";
      text: string;
      clipboardRestored: boolean;
      historyId: number | null;
    }
  | { outcome: "nothing-to-transcribe"; reason: GateReason }
  | { outcome: "cancelled" }
  | { outcome: "failed"; message: string; historyId: number | null };

/** A microphone. Mirrors `kw_audio_core::InputDevice`. */
export type InputDevice = {
  name: string;
  isDefault: boolean;
};

export async function getDictationState(): Promise<SessionState> {
  return invoke<SessionState>("dictation_state");
}

/** Start recording, or finish and transcribe. One call, because one hotkey. */
export async function toggleDictation(): Promise<DictationOutcome> {
  return invoke<DictationOutcome>("dictation_toggle");
}

export async function cancelDictation(): Promise<DictationOutcome> {
  return invoke<DictationOutcome>("dictation_cancel");
}

export async function getInputDevices(): Promise<InputDevice[]> {
  return invoke<InputDevice[]>("input_devices");
}

export async function setInputDevice(device: string | null): Promise<void> {
  return invoke<void>("dictation_set_device", { device });
}

/**
 * Whether this platform can put text at the cursor at all.
 *
 * Asked up front so the UI can say so, rather than the user discovering it at
 * the end of their first dictation. False on Linux today.
 */
export async function isInsertionSupported(): Promise<boolean> {
  return invoke<boolean>("insertion_supported");
}

export async function onDictationState(
  handler: (state: SessionState) => void,
): Promise<UnlistenFn> {
  return listen<SessionState>("dictation-state", (event) => handler(event.payload));
}

export async function onDictationOutcome(
  handler: (outcome: DictationOutcome) => void,
): Promise<UnlistenFn> {
  return listen<DictationOutcome>("dictation-outcome", (event) => handler(event.payload));
}

/* -------------------------------------------------------- local transcription */

/** Whether a local server binary is published for this platform. */
export type LocalAvailability =
  | { state: "installable" }
  | { state: "not-published"; reason: string; tracking: string }
  | { state: "unknown" };

/**
 * One model on offer. Mirrors `local_model::ModelStatus`.
 *
 * `verifiedBy` is `"digest"` when a measured SHA-256 is known for this model and
 * `"size"` when only a declared size is. Shown rather than implied: they are
 * different guarantees, and a model checked by size alone has not been proven to
 * be the file the publisher built.
 */
export type ModelStatus = {
  id: string;
  name: string;
  sizeMb: number | null;
  recommended: boolean;
  installed: boolean;
  verifiedBy: "digest" | "size" | "nothing";
};

/** Mirrors `local_model::LocalStatus`. */
export type LocalStatus = {
  availability: LocalAvailability;
  serverInstalled: boolean;
  models: ModelStatus[];
  port: number | null;
  installDir: string;
  /**
   * True when the per-user location had to be abandoned because the native
   * binaries cannot cope with a path containing a space or a non-ASCII
   * character. Worth showing: it means models are shared machine-wide.
   */
  installDirFellBack: boolean;
};

/** Progress of an install. Mirrors `local_model::InstallProgress`. */
export type InstallProgress =
  | { stage: "server"; written: number; total: number | null }
  | { stage: "model"; id: string; written: number; total: number | null }
  | { stage: "verifying" }
  | { stage: "extracting" }
  | { stage: "done"; verifiedBy: "digest" | "size" | "nothing" };

export async function getLocalStatus(): Promise<LocalStatus> {
  return invoke<LocalStatus>("local_model_status");
}

/** Download and verify the server and one model. Returns how it was verified. */
export async function installLocalModel(modelId: string): Promise<string> {
  return invoke<string>("local_model_install", { modelId });
}

export async function startLocalServer(modelId: string): Promise<number> {
  return invoke<number>("local_server_start", { modelId });
}

export async function stopLocalServer(): Promise<void> {
  return invoke<void>("local_server_stop");
}

export async function onInstallProgress(
  handler: (progress: InstallProgress) => void,
): Promise<UnlistenFn> {
  return listen<InstallProgress>("local-model-progress", (event) => handler(event.payload));
}

/* ---------------------------------------------------------------- local api */

/**
 * Mirrors `local_api::LocalApiStatus`.
 *
 * `discoveryPermissions` is the permissions the file actually has, read back
 * rather than assumed — on Windows it says administrators can read it, because
 * there is no `chmod` and claiming owner-only would be false.
 */
export type LocalApiStatus = {
  enabled: boolean;
  running: boolean;
  port: number | null;
  discoveryPath: string | null;
  discoveryPermissions: string | null;
  discoveryPermissionsAsExpected: boolean | null;
};

export async function getLocalApiStatus(): Promise<LocalApiStatus> {
  return invoke<LocalApiStatus>("local_api_status");
}

export async function startLocalApi(): Promise<number> {
  return invoke<number>("local_api_start");
}

export async function stopLocalApi(): Promise<void> {
  return invoke<void>("local_api_stop");
}

/* ------------------------------------------------------------------- route */

/**
 * Where a dictation would go right now. Mirrors `dictation::RouteReport`.
 *
 * The *resolved* route, not the stored setting: a mode of "local" with no server
 * running resolves differently from what the setting says, and showing the
 * setting would be showing an intention rather than a fact.
 */
export type RouteReport = {
  destination: string;
  /** Whether audio leaves this machine on this route. */
  leavesDevice: boolean;
  /** Set when the route cannot be resolved at all. */
  problem: string | null;
};

export async function getDictationRoute(): Promise<RouteReport> {
  return invoke<RouteReport>("dictation_route");
}

/** Point dictation at the local model, or back at the configured provider. */
export async function useLocalModel(useLocal: boolean): Promise<void> {
  return invoke<void>("dictation_use_local", { useLocal });
}
