/**
 * Typed wrappers over the Tauri command boundary.
 *
 * Every call into Rust goes through this file. React components never call
 * `invoke` directly, so the set of commands the frontend can reach is greppable
 * in one place, and the payload types live next to the call rather than being
 * re-declared at each use site.
 *
 * **The payload types are not written here.** They come from
 * `../generated/ipc-types`, which `apps/desktop/src-tauri/src/ipc_types.rs`
 * renders from the Rust types themselves and `cargo test` checks. That used to
 * be hand-written, and three of the five defects found in the first hand-check
 * of the dashboard were the same defect twice over: a TypeScript type describing
 * a Rust struct and quietly disagreeing with it. An `electronPresent` field that
 * existed in no Rust struct rendered "not present" without checking anything.
 *
 * So: types are generated, wrappers are hand-written. The defect was never in
 * the wrappers. What this file adds on top of the generated declarations is the
 * command name, the argument names, and a handful of predicates that mirror a
 * Rust method and are asserted against it by a Rust test.
 */

import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

import type {
  Accelerator,
  AccountStatus,
  AppleCalendarSupport,
  AudioSource,
  Availability,
  BinaryLocation,
  Binding,
  CalendarProvider,
  CalendarStatus,
  CalendarSummary,
  CaptureExclusion,
  CleanupRoute,
  CleanupSettings,
  ClientAvailability,
  CliStatus,
  ConfigReview,
  Conflict,
  ConnectOutcome,
  CredentialKind,
  CredentialSlotStatus,
  CredentialStoreStatus,
  Detected,
  Diagnostics,
  Dispatch,
  DiscoveryPermissions,
  DraftStatus,
  EnterpriseConfig,
  EnterpriseField,
  EnterpriseStatus,
  Evidence,
  ExportedTranscript,
  Folder,
  Format,
  GateReason,
  HotkeyAction,
  HotkeyChange,
  HotkeyReport,
  ImportPickKind,
  ImportReport,
  InputDevice,
  InstallProgress,
  KeytermSupport,
  Lists,
  LocalApiStatus,
  LocalLlmModelStatus,
  LocalLlmProgress,
  LocalLlmStatus,
  LocalStatus,
  McpStatus,
  MediaImportProgress,
  MediaImportResult,
  MediaImportSource,
  MediaSidecar,
  MeetingOutcome,
  MeetingProviderSummary,
  MeetingState,
  MeetingStatus,
  ModelCacheStatus,
  ModelStatus,
  NewNoteRequest,
  NonActivating,
  Note,
  NoteUpdateRequest,
  Offer,
  Outcome,
  OverlayStatus,
  ProbeKind,
  ProbeOutcome,
  ProbeReport,
  ProbeResult,
  ProviderStatus,
  ProviderSummary,
  ReasoningDispatch,
  ReasoningModelSummary,
  ReasoningProviderSummary,
  ReasoningStatus,
  Registration,
  RelocatedCredential,
  RouteReport,
  SearchHit,
  SessionState,
  ShortcutHookStatus,
  SidecarStatus,
  SkipReason,
  Sources,
  SyncOutcome,
  SyncRecord,
  SystemAudioSupport,
  TableReport,
  TokenStatus,
  TranscriptEvent,
  Transcription,
  UnhandledAction,
  UpdateCheckResult,
  VerifiedBy,
} from "../generated/ipc-types";

/*
 * Re-exported so components import from one place, and aliased where the Rust
 * name is ambiguous outside its own module. Each alias is a rename and nothing
 * more — the shape is still whatever Rust declares.
 */
export type {
  Accelerator,
  AccountStatus,
  AppleCalendarSupport,
  AudioSource,
  BinaryLocation,
  Binding,
  CalendarProvider,
  CalendarStatus,
  CalendarSummary,
  CaptureExclusion,
  CleanupRoute,
  CleanupSettings,
  ClientAvailability,
  CliStatus,
  ConfigReview,
  Conflict,
  ConnectOutcome,
  CredentialKind,
  CredentialSlotStatus,
  CredentialStoreStatus,
  Detected,
  Diagnostics,
  Dispatch,
  DiscoveryPermissions,
  DraftStatus,
  EnterpriseConfig,
  EnterpriseField,
  EnterpriseStatus,
  Evidence,
  ExportedTranscript,
  Folder,
  GateReason,
  HotkeyAction,
  HotkeyChange,
  HotkeyReport,
  ImportPickKind,
  ImportReport,
  InputDevice,
  InstallProgress,
  KeytermSupport,
  Lists,
  LocalApiStatus,
  LocalLlmModelStatus,
  LocalLlmProgress,
  LocalLlmStatus,
  LocalStatus,
  McpStatus,
  MediaImportProgress,
  MediaImportResult,
  MediaImportSource,
  MediaSidecar,
  MeetingOutcome,
  MeetingProviderSummary,
  MeetingState,
  MeetingStatus,
  ModelCacheStatus,
  ModelStatus,
  NonActivating,
  Note,
  Offer,
  OverlayStatus,
  ProbeKind,
  ProbeOutcome,
  ProbeReport,
  ProbeResult,
  ProviderStatus,
  ProviderSummary,
  ReasoningDispatch,
  ReasoningModelSummary,
  ReasoningProviderSummary,
  ReasoningStatus,
  Registration,
  RelocatedCredential,
  RouteReport,
  SearchHit,
  SessionState,
  ShortcutHookStatus,
  SidecarStatus,
  Sources,
  SyncOutcome,
  SyncRecord,
  SystemAudioSupport,
  TableReport,
  TokenStatus,
  TranscriptEvent,
  Transcription,
  UnhandledAction,
  UpdateCheckResult,
  VerifiedBy,
};

/** `local_model::Availability` — whether a server binary exists for this platform. */
export type LocalAvailability = Availability;
/** `kw_app_core::dictation::Outcome` — how a dictation ended. */
export type DictationOutcome = Outcome;
/** `kw_notes_core::transcript::Format`. */
export type ExportFormat = Format;
/** `commands::NewNoteRequest`. */
export type NewNote = NewNoteRequest;
/** `commands::NoteUpdateRequest`. */
export type NoteUpdate = NoteUpdateRequest;
/** `provider_probe::SkipReason` — why a probe did not run. */
export type { SkipReason };

/* ------------------------------------------------------------- capabilities */

/** Only `active` means the overlay is genuinely hidden from screen capture. */
export function isCaptureExclusionConfirmed(c: CaptureExclusion): boolean {
  return c.state === "active";
}

export async function getDiagnostics(): Promise<Diagnostics> {
  return invoke<Diagnostics>("diagnostics");
}

/* ----------------------------------------------------------------- hotkeys */

export async function getHotkeys(): Promise<HotkeyReport> {
  return invoke<HotkeyReport>("hotkeys");
}

export async function setHotkey(
  action: HotkeyAction,
  accelerators: string,
): Promise<HotkeyChange> {
  return invoke<HotkeyChange>("hotkey_set", { action, accelerators });
}

/** Fired when a registered hotkey is pressed. Mirrors `shortcuts::HotkeyFired`. */
export type HotkeyFired = {
  action: HotkeyAction;
  accelerator: string;
};

export async function onHotkey(
  handler: (fired: HotkeyFired) => void,
): Promise<UnlistenFn> {
  return listen<HotkeyFired>("hotkey", (event) => handler(event.payload));
}

/* ----------------------------------------------------------------- meetings */

/**
 * Every meeting provider, with whether it can be used.
 *
 * The list comes from Rust, and so does the reason a provider cannot be chosen.
 * A screen that decided that for itself would be a second source of truth about
 * which clients exist — which is exactly the defect that had all six providers
 * reporting as permitted with no client behind any of them.
 */
export async function getMeetingProviders(): Promise<MeetingProviderSummary[]> {
  return invoke<MeetingProviderSummary[]>("meeting_providers");
}

export async function getMeetingStatus(): Promise<MeetingStatus> {
  return invoke<MeetingStatus>("meeting_status");
}

/**
 * Start recording a meeting.
 *
 * Returns which captures actually started: a meeting works microphone-only, and
 * the user should be told which one they got rather than left to assume.
 */
export async function startMeeting(
  provider: string,
  systemAudio: boolean,
): Promise<Sources> {
  return invoke<Sources>("meeting_start", { provider, systemAudio });
}

export async function stopMeeting(): Promise<MeetingOutcome> {
  return invoke<MeetingOutcome>("meeting_stop");
}

export async function cancelMeeting(): Promise<void> {
  return invoke("meeting_cancel");
}

export async function onMeetingState(
  handler: (state: MeetingState) => void,
): Promise<UnlistenFn> {
  return listen<MeetingState>("meeting-state", (event) => handler(event.payload));
}

/** Live transcript, a window at a time. Every one is a partial. */
export async function onMeetingTranscript(
  handler: (event: TranscriptEvent) => void,
): Promise<UnlistenFn> {
  return listen<TranscriptEvent>("meeting-transcript", (event) =>
    handler(event.payload),
  );
}

export async function onMeetingOutcome(
  handler: (outcome: MeetingOutcome) => void,
): Promise<UnlistenFn> {
  return listen<MeetingOutcome>("meeting-outcome", (event) =>
    handler(event.payload),
  );
}

/** `meeting_detect::Lists` — the user's own process names. */
export type DetectLists = Lists;

/**
 * Turn away an offer to record.
 *
 * The provider stays suppressed until its process goes away, so a dismissed
 * offer does not come back five seconds later.
 */
export async function dismissMeetingOffer(): Promise<void> {
  return invoke("meeting_dismiss");
}

export async function setMeetingPrompt(enabled: boolean): Promise<void> {
  return invoke("meeting_set_prompt", { enabled });
}

export async function getMeetingDetectLists(): Promise<DetectLists> {
  return invoke<DetectLists>("meeting_detect_lists");
}

export async function setMeetingDetectLists(lists: DetectLists): Promise<void> {
  return invoke("meeting_set_detect_lists", { lists });
}

/**
 * The language to give the provider, or the empty string for automatic
 * detection where the provider supports it.
 */
export async function getMeetingLanguage(): Promise<string> {
  return invoke<string>("meeting_language");
}

export async function setMeetingLanguage(language: string): Promise<void> {
  return invoke("meeting_set_language", { language });
}

/** Terms to bias recognition towards. How many survive depends on the provider. */
export async function getMeetingKeyterms(): Promise<string[]> {
  return invoke<string[]>("meeting_keyterms");
}

export async function setMeetingKeyterms(keyterms: string[]): Promise<void> {
  return invoke("meeting_set_keyterms", { keyterms });
}

/**
 * Preview what `provider` will actually do with `language` and `keyterms`,
 * without saving anything or starting a meeting.
 *
 * `kw_meeting_core::validate` does the arithmetic; this only carries its
 * answer across the wire, so the screen never has to know a provider's cap or
 * whether it detects a language on its own.
 */
export async function reviewMeetingConfig(
  provider: string,
  language: string,
  keyterms: string[],
): Promise<ConfigReview> {
  return invoke<ConfigReview>("meeting_review_config", {
    provider,
    language,
    keyterms,
  });
}

/**
 * A meeting appears to be in progress, or `null` to clear the offer.
 *
 * Only fires for a meeting actually in a call. A client merely being open never
 * produces one — see `meeting.rs` on why, which is a measurement rather than a
 * preference.
 */
export async function onMeetingOffer(
  handler: (offer: Offer | null) => void,
): Promise<UnlistenFn> {
  return listen<Offer | null>("meeting-offer", (event) => handler(event.payload));
}

/* ---------------------------------------------------------------- providers */

export async function getProviders(): Promise<ProviderSummary[]> {
  return invoke<ProviderSummary[]>("providers");
}

/**
 * Every credential slot, in registry order, with whether each is filled.
 *
 * One call rather than one `isProviderCredentialPresent` per slot: there are
 * twenty and the screen re-reads after every save. The list comes from
 * `config/provider-registry.json` through Rust, so no screen writes a slot name.
 */
export async function getCredentialSlots(): Promise<CredentialSlotStatus[]> {
  return invoke<CredentialSlotStatus[]>("credential_slots");
}

/**
 * Store a provider credential in the OS credential store.
 *
 * Write-only by design: there is no command that reads a secret back out, so a
 * screen cannot display one and a compromised renderer cannot exfiltrate one.
 * See `docs/SECURITY.md`.
 */
export async function setProviderCredential(
  slot: string,
  value: string,
): Promise<void> {
  return invoke("provider_credential_set", { slot, value });
}

/** Whether a secret is stored for this slot. Not whether it works. */
export async function isProviderCredentialPresent(
  slot: string,
): Promise<boolean> {
  return invoke<boolean>("provider_credential_present", { slot });
}

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

/* ---------------------------------------------------------------- reasoning */

/**
 * What tidies a dictation, and where that request would go.
 *
 * `route` is the resolved destination rather than the stored setting: a provider
 * with no credential resolves differently from what the setting says.
 */
export async function getReasoningStatus(): Promise<ReasoningStatus> {
  return invoke<ReasoningStatus>("reasoning_status");
}

/**
 * Store a cleanup configuration and apply it to the running session.
 *
 * Rust refuses a configuration that cannot resolve rather than storing it, so a
 * rejection here means nothing was saved. Returns where cleanup now goes.
 */
export async function setCleanup(
  settings: CleanupSettings,
): Promise<CleanupRoute> {
  return invoke<CleanupRoute>("reasoning_set_cleanup", { settings });
}

/**
 * Store one enterprise connector's configuration.
 *
 * The whole document goes into a single OS-credential-store slot, because some
 * of its fields are secrets and splitting it would put half in SQLite. Rust
 * validates before storing, so a rejection means nothing was saved.
 *
 * Write-only, like every other credential path: nothing reads a configuration
 * back, which is why `EnterpriseStatus` reports field names and not values.
 */
export async function setEnterpriseConfig(
  config: EnterpriseConfig,
): Promise<void> {
  return invoke("enterprise_config_set", { config });
}

export async function clearEnterpriseConfig(provider: string): Promise<void> {
  return invoke("enterprise_config_clear", { provider });
}

/* ------------------------------------------------------------------ history */

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

export async function deleteHistoryEntry(id: number): Promise<boolean> {
  return invoke<boolean>("history_delete", { id });
}

export async function clearHistory(): Promise<number> {
  return invoke<number>("history_clear");
}

/* --------------------------------------------------------------------- notes */

/**
 * Notes, most recently updated first.
 *
 * There is no cursor and that is deliberate — notes are ordered by `updatedAt`,
 * so an id cursor would skip or repeat rows as notes are edited. The local API
 * refuses one for the same reason.
 */
export async function getNotes(options?: {
  noteType?: string;
  folderId?: number;
  limit?: number;
}): Promise<Note[]> {
  return invoke<Note[]>("notes_list", {
    noteType: options?.noteType ?? null,
    folderId: options?.folderId ?? null,
    limit: options?.limit ?? null,
  });
}

/** Full-text search. The query is prose; Rust escapes it into FTS5 syntax. */
export async function searchNotes(
  query: string,
  limit?: number,
): Promise<SearchHit[]> {
  return invoke<SearchHit[]>("notes_search", {
    query,
    limit: limit ?? null,
  });
}

export async function getNote(id: number): Promise<Note | null> {
  return invoke<Note | null>("notes_get", { id });
}

export async function createNote(request: NewNote): Promise<number> {
  return invoke<number>("notes_create", { request });
}

/** Returns whether the note existed. */
export async function updateNote(
  id: number,
  request: NoteUpdate,
): Promise<boolean> {
  return invoke<boolean>("notes_update", { id, request });
}

export async function deleteNote(id: number): Promise<boolean> {
  return invoke<boolean>("notes_delete", { id });
}

export async function getFolders(): Promise<Folder[]> {
  return invoke<Folder[]>("folders_list");
}

export async function createFolder(name: string): Promise<number> {
  return invoke<number>("folder_create", { name });
}

/** Delete a folder. Its notes survive, with no folder. */
export async function deleteFolder(id: number): Promise<boolean> {
  return invoke<boolean>("folder_delete", { id });
}

/* --------------------------------------------------------- transcript export */

/**
 * Every format, so a picker cannot miss one. Mirrors `Format::all`.
 *
 * The one list here that is not generated: `ts-rs` declares the union type but
 * cannot emit a runtime array. `commands::tests::every_export_format_has_the_\
 * wire_spelling_the_frontend_sends` asserts these exact strings on the Rust
 * side, so a renamed variant fails a test rather than a render.
 */
export const EXPORT_FORMATS: readonly ExportFormat[] = [
  "txt",
  "md",
  "json",
  "srt",
  "vtt",
];

/** Render a transcript without writing anything. */
export async function exportNote(
  id: number,
  format: ExportFormat,
): Promise<ExportedTranscript> {
  return invoke<ExportedTranscript>("note_export", { id, format });
}

/**
 * Render a transcript and write it to a file the user picks.
 *
 * Returns the path written, or `null` if the user cancelled. The save dialog is
 * opened by Rust, so no path is ever passed from here — which is why this needs
 * no filesystem capability. See `docs/SECURITY.md`.
 */
export async function exportNoteToFile(
  id: number,
  format: ExportFormat,
): Promise<string | null> {
  return invoke<string | null>("note_export_to_file", { id, format });
}

/* ------------------------------------------------------------------ settings */

export async function getSetting(key: string): Promise<unknown> {
  return invoke<unknown>("settings_get", { key });
}

export async function setSetting(key: string, value: unknown): Promise<void> {
  return invoke("settings_set", { key, value });
}

/** Every settings key Rust will accept, so a screen cannot invent one. */
export async function getSettingsKeys(): Promise<string[]> {
  return invoke<string[]>("settings_keys");
}

/* -------------------------------------------------------------------- import */

export async function importOpenWhispr(path: string): Promise<ImportReport> {
  return invoke<ImportReport>("import_openwhispr", { request: { path } });
}

/**
 * Open a native "choose a file" dialog for one of the files `Notes > Upload`
 * needs, and return the path picked, or `null` if the user cancelled.
 *
 * The renderer has no independent way to obtain a path here -- there is no
 * filesystem or dialog capability, so the only path this screen can ever
 * hold is one a real OS dialog returned from this call. **That is not the
 * same guarantee `exportNoteToFile` gets from its save dialog.** That command
 * fuses picking and writing into one atomic call on the Rust side, so the
 * frontend never sees or supplies a path at all. This one hands the chosen
 * path back, and the screen passes it on to a later, separate import call --
 * see `pick_import_file`'s doc comment in `commands.rs` for the gap that
 * leaves open and the hardening options being weighed (also
 * `docs/SECURITY.md`'s Webview section and `docs/BENCH_CHECK.md`'s Task 12
 * entry).
 */
export async function pickImportFile(
  kind: ImportPickKind,
): Promise<string | null> {
  return invoke<string | null>("pick_import_file", { kind });
}

/* ---------------------------------------------------------------- dictation */

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
  return listen<SessionState>("dictation-state", (event) =>
    handler(event.payload),
  );
}

export async function onDictationOutcome(
  handler: (outcome: DictationOutcome) => void,
): Promise<UnlistenFn> {
  return listen<DictationOutcome>("dictation-outcome", (event) =>
    handler(event.payload),
  );
}

/* -------------------------------------------------------- local transcription */

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
  return listen<InstallProgress>("local-model-progress", (event) =>
    handler(event.payload),
  );
}

/* --------------------------------------------------- local text generation */

export async function getLocalLlmStatus(): Promise<LocalLlmStatus> {
  return invoke<LocalLlmStatus>("local_llm_status");
}

/**
 * Download the server and one model, and its drafter if it has one.
 *
 * These are gigabytes. The caller shows the size before offering the button.
 */
export async function installLocalLlm(modelId: string): Promise<void> {
  return invoke<void>("local_llm_install", { modelId });
}

/** Start the server on a model, and return the port it took. */
export async function startLocalLlm(modelId: string): Promise<number> {
  return invoke<number>("local_llm_start", { modelId });
}

export async function stopLocalLlm(): Promise<void> {
  return invoke<void>("local_llm_stop");
}

/**
 * Install progress, and the idle-stop notice.
 *
 * One channel for both because they change the same screen: a server that
 * stopped itself is as much news as a download that finished.
 */
export async function onLocalLlmProgress(
  handler: (progress: LocalLlmProgress) => void,
): Promise<UnlistenFn> {
  return listen<LocalLlmProgress>("local-llm-progress", (event) =>
    handler(event.payload),
  );
}

/* ---------------------------------------------------------------- local api */

export async function getLocalApiStatus(): Promise<LocalApiStatus> {
  return invoke<LocalApiStatus>("local_api_status");
}

export async function startLocalApi(): Promise<number> {
  return invoke<number>("local_api_start");
}

export async function stopLocalApi(): Promise<void> {
  return invoke<void>("local_api_stop");
}

/* ---------------------------------------------------------------- CLI, MCP */

export async function getCliStatus(): Promise<CliStatus> {
  return invoke<CliStatus>("cli_status");
}

export async function getMcpStatus(): Promise<McpStatus> {
  return invoke<McpStatus>("mcp_status");
}

/* ------------------------------------------------------------ media import */

/** ffmpeg's and yt-dlp's availability and install state, for `Notes > Upload`. */
export async function getMediaImportStatus(): Promise<SidecarStatus[]> {
  return invoke<SidecarStatus[]>("media_import_status");
}

/** Download, verify and unpack one media-import sidecar. Nothing installs on its own. */
export async function installMediaImportSidecar(
  sidecar: MediaSidecar,
): Promise<void> {
  return invoke<void>("media_import_install_sidecar", { sidecar });
}

/** Convert a local audio file with ffmpeg, then transcribe it. */
export async function importAudioFile(
  path: string,
): Promise<MediaImportResult> {
  return invoke<MediaImportResult>("media_import_audio", { path });
}

/** Convert a local video file with ffmpeg (which drops the video track), then transcribe it. */
export async function importVideoFile(
  path: string,
): Promise<MediaImportResult> {
  return invoke<MediaImportResult>("media_import_video", { path });
}

/**
 * Fetch `url` with yt-dlp, convert the result with ffmpeg, then transcribe it.
 *
 * Only the url itself is checked against the network boundary before yt-dlp
 * ever sees it. yt-dlp's own fetch -- its DNS resolution, any redirect it
 * follows, and its own per-site extraction -- runs in a separate process and
 * is not observed by `kw_network_policy`. See `media.rs`'s module doc.
 */
export async function importMediaFromUrl(
  url: string,
): Promise<MediaImportResult> {
  return invoke<MediaImportResult>("media_import_url", { url });
}

/** Progress of installing a media-import sidecar, or of a running import. */
export async function onMediaImportProgress(
  handler: (progress: MediaImportProgress) => void,
): Promise<UnlistenFn> {
  return listen<MediaImportProgress>("media-import-progress", (event) =>
    handler(event.payload),
  );
}

/* ------------------------------------------------------------------- route */

export async function getDictationRoute(): Promise<RouteReport> {
  return invoke<RouteReport>("dictation_route");
}

/** Point dictation at the local model, or back at the configured provider. */
export async function useLocalModel(useLocal: boolean): Promise<void> {
  return invoke<void>("dictation_use_local", { useLocal });
}

/**
 * Point dictation at a cloud provider and model.
 *
 * Both together, never one at a time: a provider change that left the previous
 * provider's model behind resolves to a model the new provider does not offer.
 * Rust refuses an unknown pair rather than storing it.
 */
export async function setCloudProvider(
  provider: string,
  model: string,
  corti?: { environment: string; tenant: string },
): Promise<void> {
  return invoke<void>("dictation_set_cloud_provider", {
    provider,
    model,
    // Corti only. Rust refuses a Corti route with either missing rather than
    // guessing: the environment is in the hostname and the tenant scopes the
    // account, so a guess reaches a real endpoint under someone else's tenancy.
    cortiEnvironment: corti?.environment ?? null,
    cortiTenant: corti?.tenant ?? null,
  });
}

/* -------------------------------------------------------------- updater */

/**
 * Check this project's own GitHub releases for a newer version.
 *
 * Never called except by an explicit click — nothing subscribes to this on a
 * timer or on load, which is what "opt-in, never at startup" means from the
 * frontend's side of the boundary. See `updater.rs`.
 */
export async function checkForUpdate(): Promise<UpdateCheckResult> {
  return invoke<UpdateCheckResult>("update_check");
}

/** What the last check found, without starting a new one. `null` if nobody has ever checked. */
export async function getLastUpdateCheck(): Promise<UpdateCheckResult | null> {
  return invoke<UpdateCheckResult | null>("update_check_last");
}

/** Hide the update notification window without forgetting what it found. */
export async function dismissUpdateNotification(): Promise<void> {
  return invoke("update_dismiss");
}

/** Fired whenever a check completes, whatever it found. */
export async function onUpdateCheckResult(
  handler: (result: UpdateCheckResult) => void,
): Promise<UnlistenFn> {
  return listen<UpdateCheckResult>("update-check-result", (event) =>
    handler(event.payload),
  );
}

/* -------------------------------------------------------------- calendars */

/**
 * The whole calendar picture: both OAuth providers plus Apple's unsupported
 * state. A pure local read — see `calendar.rs`'s module doc — so calling
 * this on every mount and after every mutation, the way `useRefresh` does,
 * never reaches the network on its own.
 */
export async function getCalendarStatus(): Promise<CalendarStatus> {
  return invoke<CalendarStatus>("calendar_status");
}

/**
 * Save (or, given a blank string, clear) the OAuth client ID for `provider`.
 * Never a secret — there is no client secret in this flow at all — so this
 * is a plain settings write, not a credential-store one.
 */
export async function setCalendarClientId(
  provider: CalendarProvider,
  clientId: string,
): Promise<void> {
  return invoke("calendar_set_client_id", { provider, clientId });
}

/**
 * Run the loopback OAuth flow for `provider`: opens the system browser,
 * waits for it to come back, and identifies the account from its own
 * calendar list. Takes as long as the person takes at the consent screen —
 * up to five minutes — so this is only ever an explicit click, never
 * anything automatic.
 */
export async function connectCalendar(
  provider: CalendarProvider,
): Promise<ConnectOutcome> {
  return invoke<ConnectOutcome>("calendar_connect", { provider });
}

/** Disconnect whichever account is connected for `provider`, if any. */
export async function disconnectCalendar(
  provider: CalendarProvider,
): Promise<void> {
  return invoke("calendar_disconnect", { provider });
}

/**
 * Refresh the calendar list and every selected calendar's events for the
 * connected account. Also an explicit click — the only two commands that
 * reach the network on this screen are this one and `connectCalendar`.
 */
export async function syncCalendar(
  provider: CalendarProvider,
): Promise<SyncOutcome> {
  return invoke<SyncOutcome>("calendar_sync", { provider });
}

/** Toggle whether one calendar is synced. */
export async function setCalendarSelected(
  provider: CalendarProvider,
  calendarId: string,
  selected: boolean,
): Promise<void> {
  return invoke("calendar_set_selected", { provider, calendarId, selected });
}
