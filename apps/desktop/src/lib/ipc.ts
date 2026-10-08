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

import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import type { ShareOptions, SharePreview, ShareStatus } from "../generated/sharing-types.js";
export type { ShareOptions, SharePreview, ShareStatus } from "../generated/sharing-types.js";
export const folderSharePreview = (folderId: number) => invoke<SharePreview>("folder_share_preview", { folderId });
export const folderShareExport = (folderId: number, noteIds: number[], options: ShareOptions) => invoke<string>("folder_share_export", { folderId, noteIds, options });
export const folderSharePickImport = () => invoke<string>("folder_share_pick_import");
export const folderShareStatus = () => invoke<ShareStatus | null>("folder_share_status");
export const folderShareImport = (operationId: string, folderName: string) => invoke<void>("folder_share_import", { operationId, folderName });
export const folderShareCancel = (operationId: string) => invoke<void>("folder_share_cancel", { operationId });
export const folderShareDismiss = (operationId: string) => invoke<void>("folder_share_dismiss", { operationId });
export const folderShareShowExport = (operationId: string) => invoke<void>("folder_share_show_export", { operationId });
import { emitTo, listen, type UnlistenFn } from "@tauri-apps/api/event";

import type {
  Accelerator,
  AccountStatus,
  AppleCalendarSupport,
  AudioLevel,
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
  Delivered,
  Detected,
  Diagnostics,
  DiarizationModelStatus,
  DiarizationStatus,
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
  GgufIntegrity,
  HotkeyAction,
  HotkeyChange,
  HotkeyMode,
  HotkeyReport,
  ImportPickKind,
  ImportReport,
  InputDevice,
  InstallProgress,
  KeytermSupport,
  LevelSession,
  LevelSource,
  Lists,
  LocalApiStatus,
  LocalLlmModelStatus,
  LocalLlmProgress,
  LocalLlmStatus,
  LocalRuntime,
  LocalStatus,
  McpStatus,
  MediaImportProgress,
  MediaImportResult,
  ImportSnapshot,
  MediaImportSource,
  MediaSidecar,
  MeetingOutcome,
  MeetingPlan,
  RecoverableRecording,
  MeetingProviderSummary,
  MeetingState,
  MeetingStatus,
  MicrophoneCheck,
  MicrophoneSupport,
  ModelCacheStatus,
  ModelStatus,
  NewNoteRequest,
  NonActivating,
  Note,
  NoteRecording,
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
  RecheckOutcome,
  Registration,
  RefreshOutcome,
  RelocatedCredential,
  RouteReport,
  SearchHit,
  SessionState,
  ShortcutHookStatus,
  SherpaModelStatus,
  SherpaStatus,
  SidecarFreshness,
  SidecarStatus,
  SkipReason,
  Sources,
  SyncOutcome,
  SyncRecord,
  SystemAudioSupport,
  TableReport,
  TokenStatus,
  UsedFor,
  TranscriptEvent,
  Transcription,
  TranscriptRow,
  UnhandledAction,
  UpdateCheckResult,
  VerifiedBy,
  Word,
} from "../generated/ipc-types.js";

/*
 * Re-exported so components import from one place, and aliased where the Rust
 * name is ambiguous outside its own module. Each alias is a rename and nothing
 * more — the shape is still whatever Rust declares.
 */
export type {
  Accelerator,
  AccountStatus,
  AppleCalendarSupport,
  AudioLevel,
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
  Delivered,
  Detected,
  Diagnostics,
  DiarizationModelStatus,
  DiarizationStatus,
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
  GgufIntegrity,
  HotkeyAction,
  HotkeyChange,
  HotkeyMode,
  HotkeyReport,
  ImportPickKind,
  ImportReport,
  InputDevice,
  InstallProgress,
  KeytermSupport,
  LevelSession,
  LevelSource,
  Lists,
  LocalApiStatus,
  LocalLlmModelStatus,
  LocalLlmProgress,
  LocalLlmStatus,
  LocalRuntime,
  LocalStatus,
  McpStatus,
  MediaImportProgress,
  MediaImportResult,
  ImportSnapshot,
  MediaImportSource,
  MediaSidecar,
  MeetingOutcome,
  MeetingPlan,
  RecoverableRecording,
  MeetingProviderSummary,
  MeetingState,
  MeetingStatus,
  MicrophoneCheck,
  MicrophoneSupport,
  ModelCacheStatus,
  ModelStatus,
  NonActivating,
  Note,
  NoteRecording,
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
  RecheckOutcome,
  Registration,
  RefreshOutcome,
  RelocatedCredential,
  RouteReport,
  SearchHit,
  SessionState,
  ShortcutHookStatus,
  SherpaModelStatus,
  SherpaStatus,
  SidecarFreshness,
  SidecarStatus,
  Sources,
  SyncOutcome,
  SyncRecord,
  SystemAudioSupport,
  TableReport,
  TokenStatus,
  UsedFor,
  TranscriptEvent,
  Transcription,
  TranscriptRow,
  UnhandledAction,
  UpdateCheckResult,
  VerifiedBy,
  Word,
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

export function startHotkeyCapture(): Promise<number> {
  return invoke<number>("hotkey_capture_start");
}
export function endHotkeyCapture(token: number): Promise<void> {
  return invoke<void>("hotkey_capture_end", { token });
}

export async function setHotkeyPortal(enabled: boolean): Promise<void> {
  return invoke<void>("hotkey_set_portal", { enabled });
}

export async function getHotkeys(): Promise<HotkeyReport> {
  return invoke<HotkeyReport>("hotkeys");
}

export async function setHotkey(
  action: HotkeyAction,
  accelerators: string,
): Promise<HotkeyChange> {
  return invoke<HotkeyChange>("hotkey_set", { action, accelerators });
}

/**
 * Choose whether an action toggles on each press or runs while the key is held.
 *
 * Its own command rather than a second argument to `setHotkey`, because the two
 * are separate settings keys and separate controls: rebinding a key is not
 * choosing push-to-talk. Returns the same `HotkeyChange` so the row can say what
 * became of the binding either way.
 */
export async function setHotkeyMode(
  action: HotkeyAction,
  mode: HotkeyMode,
): Promise<HotkeyChange> {
  return invoke<HotkeyChange>("hotkey_set_mode", { action, mode });
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
export async function getRecoverableRecordings(): Promise<RecoverableRecording[]> {
  return invoke<RecoverableRecording[]>("meeting_recoverable_recordings");
}
export async function recoverMeetingRecording(storageKey: string): Promise<MeetingOutcome> {
  return invoke<MeetingOutcome>("meeting_recover_recording", { storageKey });
}
export async function saveMeetingNotificationPlacement(): Promise<void> { return invoke("meeting_notification_placement_save"); }
export async function resetMeetingNotificationPlacement(): Promise<void> { return invoke("meeting_notification_placement_reset"); }

/**
 * Start recording a meeting, saved under `plan`'s name in its folder
 * (KNEG-365). Rust keeps the plan with the meeting, so it applies however the
 * meeting ends; without one, the meeting is unnamed and in no folder. Whether
 * its recording is deleted once saved is not sent: Rust reads Settings ›
 * Meetings' choice at Start (KNEG-409).
 *
 * Returns which captures actually started: a meeting works microphone-only, and
 * the user should be told which one they got rather than left to assume.
 */
export async function startMeeting(
  provider: string,
  systemAudio: boolean,
  plan: Omit<MeetingPlan, "deleteRecording"> = { title: null, folderId: null },
): Promise<Sources> {
  return invoke<Sources>("meeting_start", { provider, systemAudio, title: plan.title, folderId: plan.folderId });
}

export async function stopMeeting(): Promise<MeetingOutcome> {
  return invoke<MeetingOutcome>("meeting_stop");
}
export async function updateMeetingNotes(markdown: string): Promise<void> { return invoke("meeting_notes_update", { markdown }); }
export async function getMeetingNotes(): Promise<string> { return invoke<string>("meeting_notes_get"); }

type MeetingNotesFlush = { requestId: string; error: string | null };
let nextMeetingNotesFlush = 0;

/** Settings can stop or discard a meeting while the other window has an unsaved edit. */
export async function flushMeetingPanelNotes(): Promise<void> {
  if (!("__TAURI_INTERNALS__" in window)) return;
  const requestId = `${Date.now()}-${++nextMeetingNotesFlush}`;
  let receive!: (reply: MeetingNotesFlush) => void;
  const reply = new Promise<MeetingNotesFlush>(resolve => { receive = resolve; });
  const unlisten = await listen<MeetingNotesFlush>("meeting-notes-flushed", event => {
    if (event.payload.requestId === requestId) receive(event.payload);
  });
  const timeout = window.setTimeout(() => receive({ requestId, error: "The meeting panel did not confirm that your notes were saved. Try again." }), 5000);
  try {
    await emitTo("meeting", "meeting-notes-flush", { requestId });
    const result = await reply;
    if (result.error) throw new Error(result.error);
  } finally {
    window.clearTimeout(timeout);
    unlisten();
  }
}

export async function onMeetingNotesFlush(handler: (requestId: string) => void): Promise<UnlistenFn> {
  return listen<{ requestId: string }>("meeting-notes-flush", event => handler(event.payload.requestId));
}

export async function confirmMeetingNotesFlush(requestId: string, error: string | null): Promise<void> {
  return emitTo("main", "meeting-notes-flushed", { requestId, error } satisfies MeetingNotesFlush);
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

/**
 * Whether each meeting's recording is deleted once its transcript is saved
 * (KNEG-409). Off until it is turned on. A meeting reads it when it starts,
 * so a running meeting's own choice is `MeetingStatus.plan.deleteRecording`.
 */
export async function getMeetingDeleteRecordings(): Promise<boolean> {
  return invoke<boolean>("meeting_delete_recordings");
}

export async function setMeetingDeleteRecordings(enabled: boolean): Promise<void> {
  return invoke("meeting_set_delete_recordings", { enabled });
}

/** The two diarization models, and whether speaker labels can run. */
export async function getDiarizationStatus(): Promise<DiarizationStatus> {
  return invoke<DiarizationStatus>("diarization_status");
}

/**
 * Install one of the two models, and the sherpa runtime with the first of
 * them.
 *
 * Resolves when the install finishes. Progress arrives on the
 * `diarization://install` event, which this wrapper does not subscribe to —
 * the screen decides whether it wants the detail.
 */
export async function installDiarizationModel(modelId: string): Promise<void> {
  return invoke("diarization_install_model", { modelId });
}

/**
 * Turn speaker labels on or off.
 *
 * Rejects when turning on something that cannot run, with a message naming
 * which half is missing. Accepting it and skipping silently at save time is
 * how somebody comes to believe the feature is on for three meetings.
 */
export async function setDiarizationEnabled(enabled: boolean): Promise<void> {
  return invoke("diarization_set_enabled", { enabled });
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

/**
 * The provider chosen on Settings > Meetings, or `null` before anybody has
 * chosen one. Remembered across restarts. Rust does not read it at start --
 * `startMeeting` is given a provider -- so the callers read it: the Meetings
 * page passes it when it can be used, and the offer's Record button passes it
 * as it is, so the ways into a meeting agree once a usable one is saved.
 */
export async function getMeetingProvider(): Promise<string | null> {
  return invoke<string | null>("meeting_provider");
}

/** Rejects an id this build has no client for; the setting never holds one. */
export async function setMeetingProvider(provider: string): Promise<void> {
  return invoke("meeting_set_provider", { provider });
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

/**
 * Whether each dictation is written down at all.
 *
 * On by default, and read by the sink before every row, so switching it off
 * applies to the next thing said rather than after a relaunch.
 *
 * Off is not empty. It stops new rows and deletes none of the old ones --
 * `clearHistory` above is the only thing that removes what is already stored.
 */
export async function getHistoryEnabled(): Promise<boolean> {
  return invoke<boolean>("history_enabled");
}

export async function setHistoryEnabled(enabled: boolean): Promise<void> {
  return invoke<void>("history_set_enabled", { enabled });
}

/**
 * Whether the pre-cleanup transcript is kept beside the cleaned one.
 *
 * Off by default. Switched on, a history row that carries a `rawText` is proof
 * cleanup changed something on that dictation -- which is otherwise only
 * answerable by dictating the same sentence twice.
 *
 * Subordinate to `getHistoryEnabled`: with history off there are no rows for a
 * second version to be kept on.
 */
export async function getKeepRawText(): Promise<boolean> {
  return invoke<boolean>("history_keep_raw_text");
}

export async function setKeepRawText(enabled: boolean): Promise<void> {
  return invoke<void>("history_set_keep_raw_text", { enabled });
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

/**
 * Move a note into a folder, or out of every folder with `null`. Returns
 * whether the note existed. `updateNote` cannot unfile: its `folderId` is
 * absent or set.
 */
export async function setNoteFolder(id: number, folderId: number | null): Promise<boolean> {
  return invoke<boolean>("notes_set_folder", { id, folderId });
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

/* --------------------------------------------------------------- recordings */

/**
 * The recording a note was made from, or `null` for a typed note and for any
 * older note that has not yet had its original recording attached.
 *
 * Nothing here is a path. `storageKey` is the file's bare name inside the
 * application's recordings directory, and playback goes through
 * `recordingSrc`, so the renderer never names a file to Rust in either
 * direction.
 */
export async function getNoteRecording(
  id: number,
): Promise<NoteRecording | null> {
  return invoke<NoteRecording | null>("note_recording_get", { id });
}

/** Rust picks and copies the original WAV; cancellation returns null. */
export async function attachNoteRecording(id: number): Promise<NoteRecording | null> {
  return invoke<NoteRecording | null>("note_recording_attach_from_dialog", { id });
}

/**
 * The stored transcript as playable rows: seconds from the start of the
 * recording, speaker resolved the way export resolves it. Empty for a note
 * with no transcript. Rust does the normalising so this screen and the export
 * files cannot disagree about a timestamp or a name.
 */
export async function getNoteTranscriptRows(
  id: number,
): Promise<TranscriptRow[]> {
  return invoke<TranscriptRow[]>("note_transcript_rows", { id });
}

/** Name one diarised speaker of a note. An empty name removes the name; the rows go back to "Speaker N". */
export async function renameNoteSpeaker(id: number, speakerId: string, name: string): Promise<void> {
  return invoke("note_speaker_rename", { id, speakerId, name });
}

/** Names worth offering while typing: this note's calendar participants, then names used on other notes. */
export async function getNoteSpeakerSuggestions(id: number): Promise<string[]> {
  return invoke<string[]>("note_speaker_suggestions", { id });
}

/**
 * The URL an `<audio>` element plays a note's recording from.
 *
 * The `recording` scheme is served by Rust (`src-tauri/src/recordings.rs`),
 * which looks the note up in `note_recordings` and streams the file from inside
 * the recordings directory with HTTP range support. The only thing in the URL
 * is the note id, so no path crosses the boundary in either direction.
 * `convertFileSrc` spells the scheme the way the platform's webview needs:
 * `http://recording.localhost/<id>` on Windows, `recording://localhost/<id>`
 * elsewhere.
 */
export function recordingSrc(noteId: number): string {
  return convertFileSrc(String(noteId), "recording");
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

/**
 * Every settings key that currently has a stored value. Values are not returned.
 *
 * Not the set of keys Rust will accept, which this said until 2026-08-31: the
 * command reads the `settings` table, so a key at its default has never been
 * written and does not appear. The list of keys the application has, and which
 * screen changes each one, is `config/settings-keys.json`, gated by
 * `tools/settings-audit/scan.mjs`.
 *
 * This and the two wrappers above it have no caller. Every screen goes through
 * a command named for the setting it changes instead, which is what lets that
 * gate tell a key with a way in from one without -- a generic writer would make
 * every key look reachable.
 */
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
 * hold is one a real OS dialog returned from this call. **And that is now
 * checked rather than assumed.** Rust records the canonical form of every
 * path it returns from here, for as long as the app runs, and the three
 * commands that take a path -- `importAudioFile`, `importVideoFile` and
 * `importOpenWhispr` -- refuse a path that is not in that set. So a path this
 * screen holds can be imported, re-imported after a failure, and imported
 * again; a path invented anywhere else cannot.
 *
 * Two consequences worth knowing on this side. The string returned is the
 * display form, which is what to show the user and what to derive a note
 * title from -- Rust canonicalises it again on the way back in, so do not
 * rewrite or normalise it here or it will stop matching. And a path whose
 * name does not survive lossy conversion cannot be imported at all; it is
 * refused rather than silently read.
 *
 * See `pick_import_file`'s doc comment in `commands.rs`, `docs/SECURITY.md`'s
 * Webview section, and `docs/BENCH_CHECK.md`'s Task 12 entry -- which still
 * carries the by-hand check that the policy is consulted on the real path.
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

/**
 * Choose the microphone dictation and meetings record from, or `null` for the
 * system default: one choice for both, made on either page, saved before it
 * is applied (`audio.inputDevice`, KNEG-341). A meeting already running keeps
 * the microphone it opened.
 */
export async function setInputDevice(device: string | null): Promise<void> {
  return invoke<void>("dictation_set_device", { device });
}

/**
 * The microphone the next recording will use, or `null` for the system
 * default. Read back so the screen can show the choice rather than assume
 * one -- the select was uncontrolled and showed "System default" whatever
 * was chosen. Saved (`audio.inputDevice`) and brought back at startup since
 * KNEG-341, and meetings read the same choice at Start.
 */
export async function getInputDevice(): Promise<string | null> {
  return invoke<string | null>("dictation_device");
}

/**
 * The Meetings page's "Test microphone" (KNEG-364): five seconds of the saved
 * microphone, then what was heard. Its level arrives meanwhile as
 * `audio-level` with session `microphone-check`. Nothing is recorded or sent.
 * Refused, in a sentence starting "Not now: ", while a meeting is under way,
 * a dictation records, or another check runs.
 */
export async function checkMicrophone(): Promise<MicrophoneCheck> {
  return invoke<MicrophoneCheck>("microphone_check");
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

/* ------------------------------------------------------------ sound levels */

/**
 * Every capture's live level (KNEG-371): about ten a second while it runs,
 * and a last 0 when it stops. One event for all of them; `session` and
 * `source` say whose, so a meter keeps only its own (`useAudioLevel`).
 */
export async function onAudioLevel(
  handler: (reading: AudioLevel) => void,
): Promise<UnlistenFn> {
  return listen<AudioLevel>("audio-level", (event) => handler(event.payload));
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

/**
 * Replace the installed server binary with the one this build pins.
 *
 * Stops the running server, installs, and starts it again. The outcome says
 * whether it came back -- see `RefreshOutcome`, which reports that separately
 * from the replacement because they can go different ways.
 */
export async function refreshLocalServer(): Promise<RefreshOutcome> {
  return invoke<RefreshOutcome>("local_server_refresh");
}

/* ---------------------------------------------------- local ASR (sherpa) */

/** The sherpa models, what is installed, and which one is selected. */
export async function getSherpaStatus(): Promise<SherpaStatus> {
  return invoke<SherpaStatus>("sherpa_status");
}

/**
 * Install one model, and the offline CLI if it is not already there.
 *
 * Reports through the same `InstallProgress` event the whisper installer
 * uses -- one install at a time either way, and two progress channels for one
 * progress bar would be a way for them to disagree.
 */
export async function installSherpaModel(modelId: string): Promise<void> {
  return invoke<void>("sherpa_install_model", { modelId });
}

/** Send dictation to one installed sherpa model. */
export async function useSherpaModel(modelId: string): Promise<void> {
  return invoke<void>("sherpa_use_model", { modelId });
}

/**
 * Record meetings with one installed streaming model.
 *
 * A separate call writing a separate setting, not a second use of
 * {@link useSherpaModel}: the two engines run different models, and one
 * selection would mean choosing a Parakeet model for dictation silently
 * changed what a meeting recorded with.
 *
 * Does not start the server. That happens when a meeting starts, because a
 * 630 MB model loaded by a settings click is a cost the click did not
 * advertise.
 */
export async function useSherpaStreamModel(modelId: string): Promise<void> {
  return invoke<void>("sherpa_use_stream_model", { modelId });
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
 * Replace the installed llama.cpp server binary with the build this version
 * pins.
 *
 * The same stop-replace-restart `refreshLocalServer` performs for
 * `whisper-server`, and it resolves with the same `RefreshOutcome` -- one shape
 * for both, because the two answer the same three questions and a second Rust
 * type rendering to the same TypeScript name is a failing `ipc_types` test
 * rather than a duplicate nobody notices.
 *
 * The llama server stops itself when idle, and a refresh that restarts it
 * re-arms that timer. Without it the replaced server would hold several
 * gigabytes until the application exits.
 */
export async function refreshLocalLlmServer(): Promise<RefreshOutcome> {
  return invoke<RefreshOutcome>("local_llm_refresh_server");
}

/**
 * Hash an installed model against its pinned digest.
 *
 * **Slow, on purpose, and only when asked.** Everything else the screen reports
 * about an installed file compares two integers against a record written when it
 * was downloaded; this reads the whole file back, which is a minute or more for
 * the largest models. It is the only thing that proves the bytes.
 *
 * Resolves with the finding rather than rejecting on one: a file that does not
 * match its digest is an answer, not a transport failure, and the two must not
 * arrive through the same channel. It rejects only when the check could not be
 * made at all.
 */
export async function recheckLocalLlmModel(
  modelId: string,
): Promise<RecheckOutcome> {
  return invoke<RecheckOutcome>("local_llm_recheck_model", { modelId });
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

export function getMediaImportSnapshot(): Promise<ImportSnapshot | null> {
  return invoke<ImportSnapshot | null>("media_import_snapshot");
}


export interface OverlayPreferences { supported: boolean; enabled: boolean; vertical: boolean }
export async function getOverlayPreferences(): Promise<OverlayPreferences> { return invoke("overlay_preferences"); }
export async function setOverlayPreferences(enabled: boolean): Promise<OverlayPreferences> { return invoke("overlay_preferences_set", { enabled }); }
export async function resetOverlayPosition(): Promise<void> { return invoke("overlay_position_reset"); }
export function onOverlayPreferences(handler: (value: OverlayPreferences) => void) { return listen<OverlayPreferences>("overlay-preferences", e => handler(e.payload)); }
export async function dragOverlay(phase: "start" | "move" | "end" | "cancel"): Promise<void> { return invoke("overlay_drag", { phase }); }
export async function setOverlayMessage(visible: boolean): Promise<void> { return invoke("overlay_message", { visible }); }

export async function getLocalDictationLanguages(): Promise<string[]> {
  return invoke("dictation_local_language");
}
export async function setLocalDictationLanguages(languages: string[]): Promise<void> {
  return invoke("dictation_set_local_language", { languages });
}

export function onLocalDictationLanguages(handler: (languages: string[]) => void) {
  return listen<string[]>("dictation-languages", event => handler(event.payload));
}

export async function setOverlayLanguageMenu(visible: boolean): Promise<void> { return invoke("overlay_message", { visible, menu: true }); }

/* Shared vocabulary: types generated from Rust. */
export type { DictionaryWord, DictionaryChange, VocabularyReview } from "../generated/ipc-types.js";
import type { DictionaryWord, DictionaryChange, VocabularyReview } from "../generated/ipc-types.js";
export const getVocabulary = () => invoke<DictionaryWord[]>("vocabulary_list");
export const updateVocabulary = (add: string[], remove: string[]) => invoke<DictionaryChange>("vocabulary_update", { add, remove });
export const editVocabulary = (id: number, word: string) => invoke<void>("vocabulary_edit", { id, word });
export const reviewVocabulary = (provider: string, model: string | null = null) => invoke<VocabularyReview>("vocabulary_review", { provider, model });

export const deleteVocabulary = (id: number) => invoke<void>("vocabulary_delete", { id });
export const restoreVocabulary = () => invoke<void>("vocabulary_restore");


export type { SummarySettings, SummaryState, SummaryReference } from "../generated/ipc-types.js";
import type { SummarySettings, SummaryState } from "../generated/ipc-types.js";
export const getSummarySettings = () => invoke<SummarySettings>("summary_settings_get");
export async function setSummarySettings(settings: SummarySettings): Promise<void> { return invoke<void>("summary_settings_set", { settings }); }
export const getNoteSummary = (id: number) => invoke<SummaryState>("note_summary_get", { id });
export const generateNoteSummary = (id: number, revision: number, replaceEdited: boolean, retry: boolean) => invoke<SummaryState>("note_summary_generate", { id, revision, replaceEdited, retry });
export const cancelNoteSummary = (id: number) => invoke<void>("note_summary_cancel", { id });
export const updateNoteSummary = (id: number, revision: number, content: string) => invoke<SummaryState>("note_summary_update", { id, revision, content });
export const exportNoteSummary = (id: number, format: "md" | "txt") => invoke<string | null>("note_summary_export", { id, format });
export const onNoteSummaryChanged = (handler: (id: number) => void) => listen<number>("note-summary-changed", event => handler(event.payload));
