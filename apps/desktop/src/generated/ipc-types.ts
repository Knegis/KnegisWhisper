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

/**
 * Whether a low-level keyboard hook is available.
 *
 * Mirrors `platform::keyhook::HookSupport`, converted here rather than derived
 * there: `kw-platform` has no reason to depend on `ts-rs`, and the boundary type
 * belongs on the side that crosses the boundary.
 */
export type ShortcutHookStatus = { "state": "available", mechanism: string, } | { "state": "unsupported", reason: string, };

/**
 * Whether this platform can capture system audio, and how.
 *
 * Four states rather than a boolean, for the same reason
 * `overlay::CaptureExclusion` has four: "the platform cannot" and "the platform
 * can and this machine has no such device" are different problems with
 * different fixes, and neither is "it works".
 */
export type SystemAudioSupport = { "state": "available",
/**
 * The mechanism, named so the user can recognise it.
 */
mechanism: string,
/**
 * The device that will be captured.
 */
device: string,
/**
 * True where the mechanism has never been run on real hardware by
 * anyone working on this project. Reported, not hidden.
 */
unverified: boolean, } | { "state": "no-device", mechanism: string, reason: string, } | { "state": "unsupported", reason: string, };

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
modelCache: ModelCacheStatus,
/**
 * Whether a modifier-only shortcut can work here.
 *
 * Reported rather than assumed by the UI. The hotkey editor has to say
 * something about `Control+Super`, and what it should say depends entirely
 * on whether this build has a keyboard hook — which is a fact about the
 * platform that only Rust knows.
 */
modifierOnlyShortcuts: ShortcutHookStatus,
/**
 * Whether what the speakers are playing can be recorded, and how.
 *
 * A real device enumeration, not a statement about the platform: it names
 * the device that would be captured.
 */
systemAudio: SystemAudioSupport, };

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
 * An action nothing in the application handles, and the sentence saying so.
 *
 * Reported per action rather than per accelerator because that is the shape of
 * the fact. Nothing about the accelerator is wrong; there is no handler behind
 * it, and typing a different chord would not change that.
 */
export type UnhandledAction = { action: HotkeyAction, reason: string, };

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
 * Actions with no working hotkey at all, for any reason — a bad accelerator
 * or a missing handler. The union, so a caller asking "is this action
 * dead?" gets one answer.
 */
unusable: Array<HotkeyAction>,
/**
 * The subset of `unusable` that is dead because nothing handles it, with
 * the reason. Separate because the two need different words: one is the
 * user's to fix and the other is not.
 */
unhandled: Array<UnhandledAction>, };

/**
 * What happened when we tried to register one accelerator.
 */
export type Registration = { "state": "registered", accelerator: string, } | { "state": "hooked", accelerator: string, } | { "state": "rejected", accelerator: string, reason: string, } | { "state": "not-registrable", accelerator: string, reason: string, } | { "state": "unhandled", accelerator: string, reason: string, };

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
export type CredentialKind = "provider" | "custom-endpoint" | "enterprise";

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
 * Everything an enterprise connector needs, as the user supplies it.
 *
 * One value per provider rather than a bag of optional fields: Bedrock without
 * a region and Azure without a deployment are not "partly configured", they are
 * unconfigured, and an enum makes that unrepresentable rather than checked.
 *
 * Stored as JSON in a single OS-credential-store slot. Not in SQLite — some of
 * these fields are secrets, and a struct where two fields are secret and three
 * are not is a struct that gets split across two stores and reassembled wrongly.
 */
export type EnterpriseConfig = { "provider": "bedrock",
/**
 * e.g. `us-east-1`. Part of the host *and* of the signature scope.
 */
region: string, accessKeyId: string, secretAccessKey: string,
/**
 * Present for temporary credentials, absent for long-lived ones.
 */
sessionToken?: string | null, } | { "provider": "azure",
/**
 * The resource endpoint, e.g. `https://my-org.openai.azure.com`.
 */
endpoint: string,
/**
 * The deployment name. **This selects the model**, which is why the
 * registry gives Azure no model list — the name is the org's own.
 */
deployment: string,
/**
 * e.g. `2024-10-21`. A query parameter, not a header; Azure rejects a
 * request without it rather than defaulting.
 */
apiVersion: string, apiKey: string, } | { "provider": "vertex", projectId: string,
/**
 * e.g. `europe-west4`, or `global`.
 */
region: string,
/**
 * The service-account JSON key, verbatim as downloaded.
 */
serviceAccountJson: string, };

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
export type ReasoningDispatch = { "state": "ready" } | { "state": "needs-config", reason: string, } | { "state": "needs-sidecar", reason: string, } | { "state": "unresolvable", reason: string, };

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
 * One enterprise connector's configuration, as the screen sees it.
 *
 * Field *names* and whether each is filled, never a value. There is no command
 * that reads an enterprise configuration back, for the same reason there is
 * none for an API key: a screen that cannot display a secret cannot leak one.
 */
export type EnterpriseField = { name: string, filled: boolean, };

/**
 * What is configured for one enterprise connector.
 */
export type EnterpriseStatus = { provider: string, name: string,
/**
 * Whether anything is stored at all.
 */
configured: boolean,
/**
 * Present when something is stored: which fields it has.
 */
fields: Array<EnterpriseField>,
/**
 * Present when something is stored and cannot be read, or is incomplete.
 */
problem: string | null, };

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
export type ReasoningStatus = { settings: CleanupSettings, route: CleanupRoute, providers: Array<ReasoningProviderSummary>, enterprise: Array<EnterpriseStatus>, };

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

/* --------------------------------------------------------------- meetings */
/**
 * Which capture a chunk came from.
 */
export type AudioSource = "mic" | "system";

/**
 * One piece of transcript from a provider.
 */
export type TranscriptEvent = { text: string,
/**
 * False while the provider may still revise this text.
 */
isFinal: boolean,
/**
 * Seconds from the start of the session.
 */
timestamp: number, speaker: string | null, source: AudioSource, };

/**
 * How a provider expects keyterms on the wire.
 *
 * The differences are not cosmetic: each needs different serialisation, and one
 * of them silently truncates.
 */
export type KeytermWire = "json-query-param" | "repeated-query-param" | "session-config";

/**
 * Whether a provider does anything with keyterms.
 */
export type KeytermSupport = { "state": "unsupported" } | { "state": "supported",
/**
 * `None` means no documented limit.
 */
max: number | null, wire: KeytermWire, };

/**
 * Whether a provider can be used, and if not, why not.
 *
 * Three states rather than a boolean, because the three need different words on
 * the screen and lead to different actions.
 *
 * Named `Client…` rather than plain `Availability` because `local_model` already
 * exports one — whether a *server binary* is published for this platform — and
 * two Rust types cannot share a TypeScript name. `ipc_types.rs` asserts on that,
 * and caught this.
 */
export type ClientAvailability = { "state": "available" } | { "state": "no-client", reason: string, } | { "state": "declined", reason: string, };

/**
 * One provider as the settings screen needs it.
 *
 * A flattened view rather than [`MeetingProviderCapabilities`] itself: that type
 * carries a `Duration`, which serialises as `{secs, nanos}` and is not what a
 * screen wants, and `&'static [u32]`. Named `Meeting…` because
 * `provider-registry` already exports a `ProviderSummary` and two Rust types
 * cannot share one TypeScript name — `ipc_types.rs` asserts on exactly that.
 */
export type MeetingProviderSummary = { id: string, name: string,
/**
 * First, because it is the first thing a privacy-conscious user asks.
 */
audioLeavesDevice: boolean, keyterms: KeytermSupport, sessionLimitSeconds: number | null, tokenRotation: boolean, sampleRates: Array<number>, speakerLabels: boolean, partials: boolean, languageDetection: boolean, reconnect: boolean, batchFallback: boolean, availability: ClientAvailability,
/**
 * The same honest sentence [`MeetingProviderCapabilities::summary`] builds,
 * rendered once here rather than reassembled in TypeScript.
 */
summary: string, };

/**
 * What a configuration will actually do, once the provider's limits apply.
 */
export type ConfigReview = { keytermsRequested: number, keytermsUsed: number,
/**
 * Non-zero when terms will be silently discarded. Worth telling the user.
 */
keytermsDropped: number,
/**
 * True when the provider cannot detect a language and none was given.
 */
languageRequired: boolean, sessionLimitSeconds: number | null, };

/**
 * What a meeting session is doing.
 */
export type MeetingState = "idle" | "recording" | "finishing";

/**
 * Which captures are actually running.
 *
 * Reported rather than assumed, because a meeting works microphone-only and the
 * user should know which one they got. Zero system-audio samples means nobody
 * else spoke; *no system capture at all* is a different fact.
 */
export type Sources = { microphone: boolean, systemAudio: boolean,
/**
 * Why system audio is not being captured, when it is not.
 */
systemAudioReason: string | null, };

/**
 * What can be said about the meeting session.
 */
export type MeetingStatus = { state: MeetingState,
/**
 * The provider in use, while there is one.
 */
provider: string | null, elapsedSeconds: number, sources: Sources | null,
/**
 * What this platform can do about system audio, whether or not a meeting is
 * running.
 */
systemAudio: SystemAudioSupport,
/**
 * Meeting applications seen running at the last poll.
 *
 * Everything found, not only what would prompt: the screen shows "Teams is
 * open" even though Teams cannot be prompted for, because that is more
 * useful than silence and it is where the reason lives.
 */
detected: Array<Detected>,
/**
 * Whether a detected meeting offers to record.
 */
promptOnDetect: boolean,
/**
 * Whether the notification window is confirmed absent from screen shares.
 *
 * Reported rather than assumed. On a platform where exclusion fails the
 * window still shows: an indicator visible in a screen share is a cost the
 * user can be told about, and no indicator at all is the thing the brief
 * forbids.
 */
notificationExcluded: CaptureExclusion | null, };

/**
 * How a meeting ended.
 */
export type MeetingOutcome = { "outcome": "saved", noteId: number, durationSeconds: number,
/**
 * Where the recording is, so the note is not the only copy.
 */
recordingPath: string,
/**
 * Non-zero when audio was never transcribed. A transcript with a hole in
 * it must not look complete.
 */
gaps: number,
/**
 * True when the whole-file pass failed and the overlapping live
 * partials were saved instead.
 */
transcriptIsPartials: boolean, } | { "outcome": "nothing-transcribed", recordingPath: string, } | { "outcome": "cancelled" } | { "outcome": "failed", message: string, };

/**
 * An offer to record a meeting that appears to be happening.
 */
export type Offer = {
/**
 * The `meeting-detect:*` id.
 */
provider: string,
/**
 * What to call it on screen.
 */
name: string,
/**
 * The process that gave it away, so the offer can say why it appeared.
 */
evidence: string, };

/**
 * A meeting application that appears to be running.
 */
export type Detected = {
/**
 * The `meeting-detect:*` id, so a caller can name the integration.
 */
provider: string,
/**
 * What to call it on screen.
 */
name: string,
/**
 * The process name that matched.
 *
 * Carried so a false positive can be diagnosed rather than argued about: a
 * prompt that says *why* it appeared can be told it is wrong.
 */
evidence: string,
/**
 * True when the matched process only exists during a call, rather than
 * whenever the application is open.
 *
 * The difference between "in a meeting" and "has Zoom installed", and the
 * reason a caller can choose to prompt only on the stronger signal.
 */
inCall: boolean, };

/**
 * The user's own process names.
 *
 * `meeting-detect:generic` upstream also covered calendar events, microphone
 * activity and system-audio activity. Only the lists are built: calendars are
 * chunk F, and microphone activity needs the `IAudioSessionManager2` listener
 * that is still unwritten. Named here rather than left implied, because a
 * half-built detector that looks whole is how a user comes to trust one that
 * misses half the meetings.
 */
export type Lists = {
/**
 * Extra process names that count as a meeting.
 */
allow: Array<string>,
/**
 * Names that never do, whatever else matches.
 *
 * Applied after the built-in table as well as after `allow`, so a user can
 * silence a false positive this crate is wrong about without waiting for a
 * release.
 */
block: Array<string>, };

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

/**
 * Which file `pick_import_file` is being asked to find.
 *
 * A closed enum rather than a filter typed by the frontend, the same
 * reasoning as `media::MediaSidecar`: the title and extension list are fixed
 * by this command, not invented by whoever calls it.
 */
export type ImportPickKind = "audio" | "video" | "open-whispr-database";

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

/* -------------------------------------------------- local text generation */
/**
 * A drafter alongside the main model.
 */
export type DraftStatus = { fileName: string, sizeBytes: number | null, installed: boolean, };

/**
 * One local model on offer.
 */
export type LocalLlmModelStatus = { id: string, name: string,
/**
 * The family it belongs to, e.g. `qwen`.
 */
provider: string,
/**
 * `None` when the registry does not say. Shown before anyone commits to a
 * download that can be six gigabytes.
 */
sizeBytes: number | null, quantization: string | null, contextLength: number | null, recommended: boolean, supportsThinking: boolean, installed: boolean,
/**
 * Whether this model has a speculative-decoding drafter, and whether that
 * drafter is installed. Absent for the models without one.
 */
draft: DraftStatus | null, };

/**
 * What the Intelligence > Local screen needs, with no network access.
 */
export type LocalLlmStatus = {
/**
 * Whether a server binary is published for this platform.
 */
availability: Availability, serverInstalled: boolean, models: Array<LocalLlmModelStatus>,
/**
 * The port the server is on, if it is running.
 */
port: number | null,
/**
 * Which model that server loaded.
 */
runningModel: string | null, installDir: string,
/**
 * Minutes of inactivity before the server stops itself.
 */
idleStopMinutes: number, };

/**
 * Progress of an install, and the one lifecycle event worth reporting.
 *
 * Named for its sidecar rather than `InstallProgress`, because
 * `local_model` already has one of those and a TypeScript file cannot hold
 * two declarations of the same name. `ipc_types` refuses the collision.
 */
export type LocalLlmProgress = { "stage": "server", written: number, total: number | null, } | { "stage": "model", id: string, written: number, total: number | null, } | { "stage": "draft", id: string, written: number, total: number | null, } | { "stage": "extracting" } | { "stage": "done", id: string, } | { "stage": "stopped-when-idle" };

/* -------------------------------------------------------------- local api */
/**
 * The discovery file's permissions, shown as a state and its evidence — not
 * reduced to a boolean.
 *
 * Mirrors `kw_local_api::Permissions` variant for variant, converted here
 * rather than derived there: `kw-local-api` has no reason to depend on
 * `ts-rs`, the same reasoning `diagnostics.rs`'s `ShortcutHookStatus` and
 * `local_model.rs`'s `VerifiedBy` already follow for their own core types.
 *
 * This type exists because of one specific defect.
 * `Permissions::is_as_expected()` returns `true` for `InheritedFromProfile`
 * — correct as "the best this build currently achieves on this platform",
 * read by a screen that only has the boolean as "verified restricted", which
 * it is not: nothing was checked, it is simply what a file under
 * `%USERPROFILE%` gets by default, and two other trustees turned up in that
 * default on a real machine. See `docs/BENCH_CHECK.md`, 2026-08-26 finding 2.
 * `Integrations > CLI` switches on `state` and gives `InheritedFromProfile`
 * its own honest sentence rather than a green tick that reuses the boolean.
 */
export type DiscoveryPermissions = { "state": "owner-only" } | { "state": "inherited-from-profile" } | { "state": "wider",
/**
 * Octal, e.g. `"644"`, matching how a person would type it to `chmod`.
 */
mode: string, } | { "state": "unknown", reason: string, };

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
 * The discovery file's real permission state, evidence and all -- the
 * same [`DiscoveryPermissions`] `Integrations > CLI` renders, so the two
 * screens agree on one vocabulary rather than each describing this file
 * in its own words. `None` when it is not running, because there is no
 * file to describe.
 *
 * This used to carry a second field, `discovery_permissions_as_expected:
 * Option<bool>`, derived from `Permissions::is_as_expected()`. That
 * boolean is `true` for `InheritedFromProfile`, so the settings screen
 * warned only when it was `false` and stayed silent on an inherited ACL
 * -- reassuring where nothing had been verified. Removed rather than
 * redefined: a screen that reads the real state has no use for a second,
 * looser summary of the same fact. See docs/BENCH_CHECK.md, 2026-08-26
 * finding 2.
 */
discoveryPermissions: DiscoveryPermissions | null, };

/* -------------------------------------------------------- developer tools */
/**
 * Where a companion binary was found, if this checked and found it anywhere.
 *
 * Three states rather than a boolean. "Next to this executable" and
 * "somewhere on PATH" are different facts — the first means the same build
 * that produced this application also produced the tool; the second means
 * something else put a same-named file somewhere the shell would run it —
 * and collapsing either into "not found" would be exactly the
 * boolean-collapse this project's `Permissions` pattern exists to avoid.
 */
export type BinaryLocation = { "state": "sibling", path: string, } | { "state": "on-path", path: string, } | { "state": "not-found" };

/**
 * What `Integrations > CLI` shows.
 */
export type CliStatus = {
/**
 * The binary's name on this platform, e.g. `knegiswhisper.exe`.
 */
binaryName: string,
/**
 * Where this actually found it, if anywhere.
 */
binaryLocation: BinaryLocation,
/**
 * Where the CLI looks for the port and token. Deterministic, so shown
 * whether or not the local API is running right now.
 */
discoveryPath: string,
/**
 * Whether the local API is running right now, and so whether the
 * discovery file exists at all.
 */
localApiRunning: boolean,
/**
 * The discovery file's real permission state, evidence and all. `None`
 * exactly when `local_api_running` is `false`: the file goes with the
 * server, so there is then nothing to describe.
 */
discoveryPermissions: DiscoveryPermissions | null, };

/**
 * What `Integrations > MCP` shows.
 */
export type McpStatus = {
/**
 * The binary's name on this platform, e.g. `knegiswhisper-mcp.exe`.
 */
binaryName: string,
/**
 * Where this actually found it, if anywhere.
 */
binaryLocation: BinaryLocation,
/**
 * The value to put in an MCP client's `command` field: the resolved
 * path when this found the binary, or the bare name otherwise — legible
 * either way, but nothing here claims a bare name will actually resolve.
 */
command: string,
/**
 * Whether the local API is running right now. The server is a second
 * front end over it and does nothing on its own, so a client configured
 * with `command` will fail every call until this is `true`.
 */
localApiRunning: boolean, };

/* ----------------------------------------------------------- media import */
/**
 * The two sidecars this module drives. Deliberately a closed enum rather than
 * a catalogue name typed by the frontend: a stringly-typed sidecar id would
 * let a caller ask this command to install `whisper` or `llama` too, which is
 * not what "media import" means.
 */
export type MediaSidecar = "ffmpeg" | "yt-dlp";

/**
 * What can be said about one media-import sidecar without touching the
 * network.
 */
export type SidecarStatus = { sidecar: MediaSidecar,
/**
 * Reused from `local_model` rather than redeclared: the same three
 * states (installable / not published for this platform / a gap in the
 * manifest) apply to any sidecar, and `local_llm` already reuses it for
 * the same reason.
 */
availability: Availability, installed: boolean, };

/**
 * Where an imported transcript came from.
 *
 * Never the full URL: a query string can carry a token, and this value may
 * end up quoted back into a note. The host is enough to show the user where
 * it came from.
 */
export type MediaImportSource = { "kind": "audio" } | { "kind": "video" } | { "kind": "url", host: string, };

/**
 * What a finished import produced. The frontend turns this into a note; this
 * module does not write one itself.
 */
export type MediaImportResult = { text: string, provider: string, model: string | null,
/**
 * Computed from the converted WAV, not asked of the source file, so it
 * reflects what was actually transcribed.
 */
durationMs: number, source: MediaImportSource, };

/**
 * Progress of installing a media-import sidecar, or of running an import
 * once both are ready.
 *
 * Shares the shape of `local_model::InstallProgress` and
 * `local_llm::LocalLlmProgress` -- a staged, tagged enum reported on one
 * event -- but is a distinctly named type so it cannot collide with either
 * in the generated TypeScript. `ipc_types` asserts against exactly that
 * collision.
 */
export type MediaImportProgress = { "stage": "downloading", sidecar: string, written: number, total: number | null, } | { "stage": "verifying" } | { "stage": "extracting" } | { "stage": "fetching" } | { "stage": "converting" } | { "stage": "transcribing" } | { "stage": "done" };

/* ---------------------------------------------------------------- updater */
/**
 * What checking for an update found.
 */
export type UpdateCheckResult = { "state": "no-public-key-configured" } | { "state": "check-failed", error: string, } | { "state": "current-version-unparseable", version: string, } | { "state": "latest-version-unparseable", tag: string, } | { "state": "up-to-date", current: string, latest: string, } | { "state": "update-available", current: string, latest: string, url: string, };

/* -------------------------------------------------------------- calendars */
/**
 * Which calendar provider a command is about, as it crosses the wire.
 * Mirrors `kw_calendar_core::CalendarProviderKind`, which has no `Serialize`
 * of its own -- that crate has no reason to know about JSON at all, so the
 * wire vocabulary lives here instead.
 */
export type CalendarProvider = "google" | "microsoft";

/**
 * What happened when someone clicked "Connect".
 *
 * Only the two outcomes that are not simply a refusal: a plain error (no
 * client ID configured, the loopback receiver could not bind, the token
 * exchange itself failed) crosses as `Err` the same as every other command,
 * because there is nothing more for the frontend to do with those than show
 * them. Being turned down at the consent screen is different -- an ordinary
 * outcome, not a bug -- so it gets its own state rather than the same
 * generic error text.
 */
export type ConnectOutcome = { "state": "connected", accountEmail: string, calendars: number, } | { "state": "denied", reason: string, };

/**
 * What one sync attempt found. Both the command's own return value and
 * what [`CalendarRuntime::record_sync`] persists to settings -- one shape
 * for both means the screen can never see a different answer from a sync
 * it just ran than from the record `calendar_status` reads back later.
 */
export type SyncOutcome = { "state": "completed", eventsUpserted: number, eventsRemoved: number,
/**
 * Per-calendar failures within an otherwise-completed sync -- a
 * calendar list refresh can succeed while one calendar's events
 * fail, and reporting only the aggregate would hide which one.
 */
calendarErrors: Array<string>, } | { "state": "token-unavailable", message: string, } | { "state": "failed", message: string, };

/**
 * A sync outcome together with when it happened, persisted to settings so
 * [`calendar_status`] can report it without a network call.
 */
export type SyncRecord = {
/**
 * RFC 3339.
 */
at: string, outcome: SyncOutcome, };

/**
 * Whether a token is on file and, if so, whether it is still within its
 * recorded expiry.
 *
 * A local read, not a network probe -- reaching the network on every status
 * poll is exactly what `docs/NETWORK_BOUNDARY.md` rules out for a passive
 * status command. `Missing` covers both "never connected" and "the
 * credential is gone even though the database row is still there", which is
 * exactly the state [`CalendarRuntime::disconnect`] leaves behind -- see its
 * doc comment on why the row itself is not deleted. Checking the credential
 * store directly, rather than trusting the row's mere existence, is what
 * keeps that decision from ever reading back as "connected".
 *
 * `RefreshFailed` exists because `expires_at` alone cannot distinguish "past
 * its expiry, never tried since" from "past its expiry, and the most recent
 * attempt to refresh it already failed" -- see [`combine_token_status`].
 * Reporting the second case as plain `NeedsRefresh`, the way this type used
 * to before a token could carry a known, recorded failure, is what let
 * `calendar_status` say "Connected -- the next sync refreshes it
 * automatically" in the same payload as a `SyncOutcome::TokenUnavailable`
 * telling the user to reconnect.
 */
export type TokenStatus = { "state": "active" } | { "state": "needs-refresh" } | { "state": "missing" } | { "state": "refresh-failed", message: string, };

export type CalendarSummary = { id: string, summary: string, isPrimary: boolean, isSelected: boolean,
/**
 * What sync actually wrote for this calendar, not the provider's own
 * total -- see `kw_calendar_core::events_repo::count_events`.
 */
eventCount: number, };

export type AccountStatus = { email: string, token: TokenStatus, calendars: Array<CalendarSummary>, lastSync: SyncRecord | null, };

export type ProviderStatus = { clientIdConfigured: boolean,
/**
 * The client ID itself, shown back rather than only "configured" or
 * not. It is not a secret -- there is no client secret in this flow at
 * all, see `kw_calendar_core`'s module doc -- so there is nothing to
 * protect by hiding it, and showing it is what makes "you supplied
 * this" a fact on screen rather than a claim in a comment.
 */
clientId: string | null, account: AccountStatus | null, };

export type CalendarStatus = { google: ProviderStatus, microsoft: ProviderStatus, apple: AppleCalendarSupport, };

/**
 * What this build can do about Apple Calendar, checked rather than assumed.
 *
 * One variant today because nothing is implemented anywhere -- see the
 * module doc. Shaped as a tagged enum, like
 * [`kw_audio_core::SystemAudioSupport`], so a future `Available` variant
 * slots in without changing how the frontend switches on it.
 */
export type AppleCalendarSupport = { "state": "unsupported", reason: string, };
