/**
 * GENERATED FILE — DO NOT EDIT.
 *
 * Source:    the Rust types themselves, via `#[derive(ts_rs::TS)]`
 * Generator: apps/desktop/src-tauri/src/ipc_types.rs
 *
 * Regenerate with:
 *   KW_WRITE_IPC_TYPES=1 cargo test -p knegiswhisper --lib ipc_types
 *
 * `cargo test` fails when this file and the Rust types disagree, so a
 * frontend type can no longer describe a field Rust does not send.
 * Hand-written `invoke()` wrappers live in ../lib/ipc.ts.
 */

/* ----------------------------------------------------------- capabilities */
/**
 * How we know a capture-exclusion claim is true.
 */
export type Evidence = { "kind": "read-back",
/**
 * The API consulted, e.g. `NSWindow.sharingType`.
 */
api: string,
/**
 * The value it returned.
 */
value: string, };

/**
 * The result of attempting to exclude a window from screen capture.
 *
 * Four states rather than a boolean, because "we called the API and it did not
 * complain" is genuinely different from "we know this works".
 */
export type CaptureExclusion = { "state": "active", verifiedBy: Evidence, } | { "state": "unsupported", reason: string, } | { "state": "failed", error: string, } | { "state": "unknown", reason: string, };

/**
 * Whether the window declines activation.
 */
export type NonActivating = { "state": "active", verifiedBy: string, } | { "state": "unsupported", reason: string, } | { "state": "failed", error: string, };

/**
 * What the overlay is actually doing, as reported to the settings screen.
 */
export type OverlayStatus = {
/**
 * Whether the window exists.
 */
created: boolean,
/**
 * The real result of asking the platform to exclude it from capture.
 */
captureExclusion: CaptureExclusion,
/**
 * Whether the window refuses focus, and how we know.
 */
nonActivating: NonActivating,
/**
 * Whether the overlay is visible right now.
 *
 * This was a hardcoded `true` while nothing recorded, which made the
 * guarantee it was supposed to express vacuous. It is now a real read of the
 * window, and [`set_visible`] is the only thing that changes it — called
 * from the dictation state sink, so "recording" and "the user can see it"
 * cannot come apart.
 */
recordingIndicatorVisible: boolean,
/**
 * What the platform's display server is, for honest failure messages.
 */
displayServer: string, };

/**
 * Whether the OS credential store is usable.
 */
export type CredentialStoreStatus = { "state": "available", backend: string, } | { "state": "unavailable", reason: string, };

/**
 * Where downloaded models live.
 */
export type ModelCacheStatus = { path: string,
/**
 * True when the per-user path contained a space or a non-ASCII character,
 * which makes the native whisper and sherpa binaries fail in ways that do
 * not point at the path. Worth surfacing: it means models are shared
 * machine-wide.
 */
fellBack: boolean, };

export type Diagnostics = { appVersion: string, os: string, arch: string, webviewVersion: string | null, credentialStore: CredentialStoreStatus,
/**
 * The live state of the real overlay window, not a prediction about the
 * platform. Before the window exists this reports `created: false` and
 * claims nothing.
 */
overlay: OverlayStatus, allowedEgressPurposes: Array<string>,
/**
 * Where the model cache ended up, and whether the per-user location had to
 * be abandoned because the native ASR binaries cannot read it.
 */
modelCache: ModelCacheStatus, };

/* ---------------------------------------------------------------- hotkeys */
/**
 * The five things a hotkey can trigger.
 *
 * Upstream names these `DICTATION_KEY`, `CHAT_AGENT_KEY`, `VOICE_AGENT_KEY`,
 * `TRANSLATION_KEY`, and `MEETING_KEY`.
 */
export type HotkeyAction = "dictation" | "chat-agent" | "voice-agent" | "translation" | "meeting";

/**
 * What an accelerator string can be registered as.
 */
export type Accelerator = { "kind": "standard", accelerator: string, } | { "kind": "modifier-only", accelerator: string, modifiers: Array<string>, } | { "kind": "invalid", input: string, reason: string, };

/**
 * One action's configured hotkeys, classified.
 */
export type Binding = { action: HotkeyAction, accelerators: Array<Accelerator>, };

/**
 * A conflict between two actions bound to the same accelerator.
 */
export type Conflict = { accelerator: string, actions: Array<HotkeyAction>, };

/**
 * One action's hotkeys, classified, with any problems spelled out.
 */
export type HotkeyReport = { bindings: Array<Binding>,
/**
 * Accelerators claimed by more than one action.
 */
conflicts: Array<Conflict>,
/**
 * Actions with no working hotkey at all.
 */
unusable: Array<HotkeyAction>, };

/**
 * What happened when we tried to register one accelerator.
 */
export type Registration = { "state": "registered", accelerator: string, } | { "state": "rejected", accelerator: string, reason: string, } | { "state": "not-registrable", accelerator: string, reason: string, };

/**
 * What changing one action's hotkey actually achieved.
 *
 * `binding` is what was stored and how it classifies; `registrations` is what
 * the platform said when asked to take it. Both, because they fail
 * independently: a well-formed accelerator can still be refused because another
 * application owns it, and reporting "saved" for that is the kind of confident
 * success this project treats as a defect.
 */
export type HotkeyChange = { binding: Binding, registrations: Array<Registration>,
/**
 * True when at least one accelerator for this action is now live.
 */
live: boolean, };

/* -------------------------------------------------------------- providers */
/**
 * What would happen if dictation were pointed at this provider right now.
 *
 * Three states rather than a boolean, because "we have not written the client"
 * and "you have not filled in the settings this provider needs" are different
 * problems with different fixes, and neither is "it works".
 *
 * Derived by asking `route::resolve` rather than by listing provider ids here.
 * A hand-written list of which providers dispatch would be a second copy of the
 * match in `crates/transcription-core/src/route.rs`, and the two would part
 * company the first time a client was written.
 */
export type Dispatch = { "state": "ready" } | { "state": "needs-client", reason: string, } | { "state": "unresolvable", reason: string, };

/**
 * A provider and whether it has a credential stored — *not* whether it works.
 *
 * The distinction is the point. `configured` means a key is present;
 * `connected` requires a passing connection test, which only
 * [`probe_providers`] can establish; and `dispatch` is a third thing again —
 * whether this build can even send the request.
 */
export type ProviderSummary = { id: string, name: string, credentialSlot: string, configured: boolean, modelCount: number,
/**
 * The models on offer, so a picker never writes a model id by hand.
 */
models: Array<string>, dispatch: Dispatch, };

/**
 * What a credential slot is for.
 *
 * The distinction a UI has to make. A `Provider` key is enough on its own to
 * try that service; a `CustomEndpoint` key configures nothing without the base
 * URL that lives with the feature it belongs to, so a screen that stores one
 * must say so rather than implying the key was all that was needed.
 */
export type CredentialKind = "provider" | "custom-endpoint";

/**
 * One credential slot and whether a secret is stored in it.
 *
 * `stored` is deliberately the only thing said about the value. There is no
 * command that reads a credential back, so a screen cannot display one and a
 * compromised renderer cannot exfiltrate one.
 */
export type CredentialSlotStatus = { slot: string, name: string, kind: CredentialKind,
/**
 * Which parts of the application use this key, from the registry.
 */
usedFor: Array<string>,
/**
 * Which feature a custom endpoint belongs to; `None` for a provider.
 */
feature: string | null,
/**
 * A secret is present. This is NOT the same as the provider working.
 */
stored: boolean, };

/**
 * Which probe ran.
 */
export type ProbeKind = "catalog" | "chat" | "voice";

/**
 * Why a probe did not run.
 */
export type SkipReason = { "reason": "no-credential" } | { "reason": "not-supported", detail: string, };

/**
 * What a probe concluded.
 *
 * Deliberately three states with no "warning". A probe either demonstrated the
 * provider works, demonstrated it does not, or did not run.
 */
export type ProbeOutcome = { "outcome": "pass", detail: string, } | { "outcome": "fail", detail: string, } | { "outcome": "skipped" } & SkipReason;

/**
 * One probe's result.
 */
export type ProbeResult = { provider: string, kind: ProbeKind, outcome: ProbeOutcome, };

/**
 * Whether the audio used for the voice probe can prove recognition.
 */
export type AudioKind = "speech" | "tone";

/**
 * The result of probing one or more providers.
 */
export type ProbeReport = { results: Array<ProbeResult>,
/**
 * Which audio the voice probes used, so a pass is not over-read.
 */
audio: AudioKind, };

/* -------------------------------------------------------------- reasoning */
/**
 * What the user configured, as stored.
 *
 * Kept separate from [`ChatSettings`] rather than reusing it, because the
 * reasoning crate's own comment is worth honouring: chat settings must never be
 * read as cleanup settings. Two structs cannot be confused for each other; one
 * struct with a mode field can.
 */
export type CleanupSettings = {
/**
 * Off unless explicitly switched on.
 */
enabled: boolean,
/**
 * A cloud or enterprise provider id from the registry.
 */
provider?: string | null,
/**
 * The model to send to. Required: there is no safe default across seven
 * providers, and `params::shape_for` needs the real id to pick the right
 * token parameter.
 */
model?: string | null,
/**
 * An endpoint the user runs. Wins over `provider`, as in `resolve_chat`.
 */
selfHostedUrl?: string | null,
/**
 * Whether `provider` names an enterprise connector rather than a cloud one.
 */
enterprise: boolean, };

/**
 * Whether a reasoning provider can actually be used for a request today.
 *
 * The same three-state shape as `commands::Dispatch`, and a separate type
 * because the reasons differ: a transcription provider is blocked on a client,
 * a reasoning provider on a request *shape* or on a sidecar.
 */
export type ReasoningDispatch = { "state": "ready" } | { "state": "needs-shape", reason: string, } | { "state": "needs-sidecar", reason: string, } | { "state": "unresolvable", reason: string, };

/**
 * One model a reasoning provider offers.
 */
export type ReasoningModelSummary = { id: string, name: string,
/**
 * `max_tokens` or `max_completion_tokens`. Shown because getting it wrong
 * is a 400 that reads as a broken integration rather than a wrong field.
 */
tokenParam: string, supportsThinking: boolean, supportsVision: boolean, };

/**
 * One reasoning provider, and whether it could be used.
 */
export type ReasoningProviderSummary = { id: string, name: string,
/**
 * `None` for enterprise connectors, whose credentials are multi-field.
 */
credentialSlot: string | null,
/**
 * A credential is stored. Not the same as the provider working.
 */
configured: boolean, enterprise: boolean, dispatch: ReasoningDispatch, models: Array<ReasoningModelSummary>, };

/**
 * Where a cleanup request would go right now.
 *
 * The *resolved* destination rather than the stored setting, for the same
 * reason `dictation::RouteReport` is: a provider with no credential resolves
 * differently from what the setting says, and showing the setting would be
 * showing an intention.
 */
export type CleanupRoute = { "state": "off" } | { "state": "ready",
/**
 * A short phrase for the UI: `openai`, `self-hosted`, and so on.
 */
destination: string, model: string,
/**
 * Whether the transcript leaves this machine on this route.
 */
leavesDevice: boolean,
/**
 * Set when the destination resolves but could not actually be used —
 * no credential stored, or a shape that is not implemented.
 */
caveat: string | null, } | { "state": "problem", reason: string, };

/**
 * Everything the Intelligence screen shows.
 */
export type ReasoningStatus = { settings: CleanupSettings, route: CleanupRoute, providers: Array<ReasoningProviderSummary>, };

/* ---------------------------------------------------------------- history */
/**
 * A stored history entry.
 *
 * Serialised in camelCase because that is what `apps/desktop/src/lib/ipc.ts`
 * declares. They disagreed once — the struct had no `rename_all`, so it sent
 * `error_message` where the UI read `errorMessage`, and a failed dictation
 * rendered as "(empty)" instead of its reason.
 */
export type Transcription = { id: number, text: string, rawText: string | null, createdAt: string | null, hasAudio: boolean, audioDurationMs: number | null, provider: string | null, model: string | null, status: string, errorMessage: string | null, errorCode: string | null, routeKind: string | null, };

/* ------------------------------------------------------------------ notes */
/**
 * A note, as listed.
 */
export type Note = { id: number, title: string, content: string, noteType: string, enhancedContent: string | null, folderId: number | null, transcript: string | null, participants: string | null, audioDurationSeconds: number | null, createdAt: string | null, updatedAt: string | null, };

/**
 * A folder.
 */
export type Folder = { id: number, name: string, isDefault: boolean, sortOrder: number, };

/**
 * A search hit.
 */
export type SearchHit = { id: number, title: string,
/**
 * A snippet with the match marked by `[` and `]`.
 */
snippet: string, };

/**
 * A new note, as the renderer sends it.
 *
 * A separate type from `notes::NewNote`, which is not `Deserialize`. That is
 * the right boundary: this is where an untrusted payload is validated, and the
 * domain type stays free of serialization concerns.
 *
 * `deny_unknown_fields` is deliberate. A mistyped field name would otherwise be
 * dropped in silence and the note saved without it — the same shape of defect
 * as a frontend type declaring a field the backend never sends.
 */
export type NewNoteRequest = { title: string, content: string, noteType?: string | null, folderId?: number | null,
/**
 * A JSON array of transcript segments, as stored.
 */
transcript?: string | null,
/**
 * A JSON array of `{displayName, email}`, as stored.
 */
participants?: string | null, audioDurationSeconds?: number | null, };

/**
 * The fields of a note to change. Absent means leave it alone.
 */
export type NoteUpdateRequest = { title?: string | null, content?: string | null, folderId?: number | null, };

/**
 * Which format to emit.
 */
export type Format = "txt" | "md" | "json" | "srt" | "vtt";

/**
 * A rendered transcript, ready to preview or write.
 */
export type ExportedTranscript = { format: Format,
/**
 * A suggested filename, including the extension.
 */
filename: string, content: string,
/**
 * How many segments the transcript held. Zero is a legitimate answer for a
 * note that was typed rather than recorded, and the UI should be able to
 * say so rather than showing an export that looks empty for no reason.
 */
segmentCount: number, };

/* ----------------------------------------------------------------- import */
/**
 * What one table's copy achieved.
 */
export type TableReport = { table: string,
/**
 * Rows written.
 */
copied: number,
/**
 * Rows the source had that could not be written, almost always because
 * they referenced a row that was itself not imported.
 */
skipped: number,
/**
 * Columns this importer wanted that the source did not have.
 */
absentColumns: Array<string>,
/**
 * Columns the source had that were deliberately not copied.
 */
droppedColumns: Array<string>,
/**
 * Why the first skipped row was skipped. A count on its own tells the user
 * nothing they can act on.
 */
skipReason: string | null, };

/**
 * A token moved out of SQLite and into the OS credential store.
 */
export type RelocatedCredential = {
/**
 * The source table it came from.
 */
sourceTable: string,
/**
 * The account it belongs to, so the user can recognise it.
 */
account: string,
/**
 * The credential-store slot it now lives in.
 */
slot: string, };

/**
 * The outcome of an import.
 */
export type ImportReport = { source: string, tables: Array<TableReport>, relocatedCredentials: Array<RelocatedCredential>,
/**
 * Tables named in the copy plan that the source did not have at all.
 */
absentTables: Array<string>,
/**
 * Rows in `notes_fts` after the import. The triggers populate it as notes
 * are inserted, so this is an assertion that they fired, not a rebuild.
 */
ftsRows: number, };

/* -------------------------------------------------------------- dictation */
/**
 * What the session is doing. Drives the overlay.
 */
export type SessionState = "idle" | "recording" | "transcribing" | "inserting";

/**
 * Why the gate decided what it did.
 */
export type GateReason = "unavailable" | "silence" | "insufficient-speech" | "speech-detected";

/**
 * How a dictation ended, as reported to the UI.
 */
export type Outcome = { "outcome": "inserted", text: string,
/**
 * Whether the user's clipboard came back as it was.
 */
clipboardRestored: boolean, historyId: number | null, } | { "outcome": "nothing-to-transcribe", reason: GateReason, } | { "outcome": "cancelled" } | { "outcome": "failed", message: string, historyId: number | null, };

/**
 * An audio input device the user can choose.
 */
export type InputDevice = { name: string, isDefault: boolean, };

/**
 * Where a dictation would go right now.
 *
 * The *resolved* route rather than the stored setting, because the two are not
 * the same thing: a mode of "local" with no server running, or a provider with
 * no credential, resolves differently from what the setting says. Showing the
 * setting would be showing an intention.
 */
export type RouteReport = {
/**
 * A short phrase for the UI: `local model`, `openai`, and so on.
 */
destination: string,
/**
 * Whether audio leaves this machine on this route.
 */
leavesDevice: boolean,
/**
 * Present when the route cannot be resolved at all.
 */
problem: string | null, };

/* ---------------------------------------------------- local transcription */
/**
 * Whether the local server can be installed here.
 */
export type Availability = { "state": "installable" } | { "state": "not-published", reason: string, tracking: string, } | { "state": "unknown" };

/**
 * How firmly a model's identity can be established.
 *
 * Three states rather than a boolean, and an enum rather than a string. It was
 * a `&'static str` whose three values the frontend re-declared by hand as a
 * union; nothing held the two in step, so a fourth state added here would have
 * reached a UI that did not know about it. Now it cannot.
 */
export type VerifiedBy = "digest" | "size" | "nothing";

/**
 * One model on offer.
 */
export type ModelStatus = { id: string, name: string, sizeMb: number | null, recommended: boolean, installed: boolean,
/**
 * What integrity check this model can be held to. Shown to the user rather
 * than implied, because the three are different guarantees.
 */
verifiedBy: VerifiedBy, };

/**
 * What can be said about the local model server without touching the network.
 */
export type LocalStatus = {
/**
 * Whether this platform has a published server binary at all, and why not.
 */
availability: Availability,
/**
 * Whether the server binary is installed.
 */
serverInstalled: boolean,
/**
 * The models on offer, and what is known about each.
 */
models: Array<ModelStatus>,
/**
 * The port the server is listening on, if it is running.
 */
port: number | null,
/**
 * Where installs are kept, and whether the per-user location had to be
 * abandoned because the native binaries cannot cope with the path.
 */
installDir: string, installDirFellBack: boolean, };

/**
 * Progress of an install, as reported to the frontend.
 */
export type InstallProgress = { "stage": "server", written: number, total: number | null, } | { "stage": "model", id: string, written: number, total: number | null, } | { "stage": "verifying" } | { "stage": "extracting" } | { "stage": "done",
/**
 * What the model on disk was actually checked against.
 */
verifiedBy: VerifiedBy, };

/* -------------------------------------------------------------- local api */
/**
 * What the settings screen shows about the local API.
 */
export type LocalApiStatus = {
/**
 * Whether the user has switched it on.
 */
enabled: boolean,
/**
 * Whether it is listening right now.
 */
running: boolean, port: number | null,
/**
 * Where a client looks for the port and token.
 */
discoveryPath: string | null,
/**
 * What the discovery file's permissions actually are, in plain words.
 * `None` when it is not running, because there is no file to describe.
 */
discoveryPermissions: string | null,
/**
 * True when the permissions are the best this platform offers.
 */
discoveryPermissionsAsExpected: boolean | null, };
