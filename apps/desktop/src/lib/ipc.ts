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
  | { state: "active"; verified_by: Evidence }
  | { state: "unsupported"; reason: string }
  | { state: "failed"; error: string }
  | { state: "unknown"; reason: string };

/** Only `active` means the overlay is genuinely hidden from screen capture. */
export function isCaptureExclusionConfirmed(c: CaptureExclusion): boolean {
  return c.state === "active";
}

/** Mirrors `overlay::NonActivating`. */
export type NonActivating =
  | { state: "active"; verified_by: string }
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

export async function getHistory(limit?: number): Promise<Transcription[]> {
  return invoke<Transcription[]>("history", { limit: limit ?? null });
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
