/**
 * GENERATED FILE â€” DO NOT EDIT.
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

/* ------------------------------------------------------------- vocabulary */
/**
 * One dictionary entry.
 */
export type DictionaryWord = { id: number, word: string,
/**
 * `manual`, or the feature that added it.
 */
source: string, createdAt: string | null, };

/**
 * What an update changed.
 */
export type DictionaryChange = { added: number,
/**
 * Words that were already there. Reported rather than counted as added, so
 * a caller retrying a request can tell nothing new happened.
 */
alreadyPresent: number, removed: number,
/**
 * Words asked to be removed that were not there.
 */
notPresent: number, };

export type OmittedTerm = { word: string, reason: string, };

export type VocabularyReview = { supported: boolean, model: string | null, included: Array<string>, omitted: Array<OmittedTerm>, limit: string, };

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
 * Whether dictation is recording and its indicator window is visible.
 * An idle persistent microphone does not count as a recording indicator.
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
 * True when the per-user path contained a non-ASCII character, which stops
 * the native whisper and sherpa binaries opening the model at all -- and
 * does so without saying why, which is what makes it worth relocating for.
 * Worth surfacing here too: it means models are shared machine-wide.
 *
 * A space no longer counts. It never did; see
 * `kw_storage::paths`'s module documentation for the measurement.
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
 * Toggle switches recording once per activation; Hold records while pressed.
 * Modifier-only toggles qualify on full release without an intervening key.
 * Standard accelerators and mouse buttons toggle on press. Hold mode still
 * begins immediately and may report InsufficientSpeech after a very short tap.
 */
export type HotkeyMode = "toggle" | "hold";

/**
 * What an accelerator string can be registered as.
 */
export type Accelerator = { "kind": "standard", accelerator: string, } | { "kind": "modifier-only", accelerator: string, modifiers: Array<string>, } | { "kind": "mouse-button", accelerator: string,
/**
 * What the click still does elsewhere, because the hook does not
 * consume it. `Back` or `Forward`.
 */
alsoDoes: string, } | { "kind": "invalid", input: string, reason: string, };

/**
 * One action's configured hotkeys, classified.
 */
export type Binding = { action: HotkeyAction, accelerators: Array<Accelerator>,
/**
 * Whether these accelerators toggle or are held. See [`HotkeyMode`].
 */
mode: HotkeyMode, };

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
 * A hotkey that works, and also does something the user did not ask for.
 *
 * Deliberately not a [`Conflict`] and not a `problem`. Both of those mean the
 * hotkey will not do its job; this one will, and will *additionally* fire when
 * the user is doing something else entirely. Folding it into either would put
 * "this does not work" and "this works too often" under one word, and the
 * remedies are opposite.
 */
export type Caution = { action: HotkeyAction, accelerator: string, reason: string, };

/**
 * One action's hotkeys, classified, with any problems spelled out.
 */
export type HotkeyReport = { registrations: Array<RegistrationReport>, portalSupported: boolean, portalEnabled: boolean, bindings: Array<Binding>,
/**
 * Accelerators claimed by more than one action.
 */
conflicts: Array<Conflict>,
/**
 * Actions that are dead because nothing handles them, with the reason.
 * Registration outcome is reported separately by
 * [`crate::shortcuts::RegistrationReport::is_live`].
 */
unhandled: Array<UnhandledAction>,
/**
 * Hotkeys that work and also fire when the user meant something else.
 *
 * A caution means the hotkey works and also fires at times nobody asked
 * for. Today the sources are AltGr and modifier-only prefix chords.
 */
cautions: Array<Caution>, };

/**
 * What happened when we tried to register one accelerator.
 */
export type Registration = { "state": "portal", accelerator: string, granted: string | null, } | { "state": "registered", accelerator: string, } | { "state": "hooked", accelerator: string, } | { "state": "rejected", accelerator: string, reason: string, } | { "state": "not-registrable", accelerator: string, reason: string, } | { "state": "unhandled", accelerator: string, reason: string, };

/**
 * The outcome of registering every configured hotkey.
 */
export type RegistrationReport = { action: HotkeyAction, results: Array<Registration>, };

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
live: boolean,
/**
 * What this hotkey will *also* do. Returned here as well as from
 * [`hotkeys`] because the moment to learn that `Control+Alt` is AltGr is
 * when you press Set, not the next time the screen is opened.
 */
cautions: Array<Caution>, };

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
 * Which feature a custom endpoint belongs to; `None` for a provider, and
 * for `custom`, your own server's key, which cleanup and summaries share.
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

export type SummarySettings = { enabled: boolean, provider: string, model: string, selfHostedUrl: string | null, enterprise: boolean, language: string,
/**
 * Total context budget, including instructions and output; never a secret.
 */
contextBudget: number, };

export type SummaryReference = { id: string, rowIndex: number, };

export type SummaryState = { noteId: number, content: string | null, revision: number, edited: boolean, outdated: boolean, legacy: boolean, status: string | null, completed: number, total: number, error: string | null, language: string | null, generatedAt: string | null,
/**
 * The model that wrote the summary, as the settings named it when it
 * succeeded: an id such as `qwen3.5-9b-q4_k_m`, not a display name.
 * `None` until a generation succeeds. Not where the transcript went:
 * `provider` alone can't say that (a typed URL wins over it), so the
 * summary names only the model (KNEG-367).
 */
model: string | null, references: Array<SummaryReference>, };

export type SummarySetupTarget = "summary-settings" | "providers" | "local-text-models" | "company-accounts";

/**
 * Configuration readiness, not a claim that a provider has answered a live request.
 */
export type SummaryReadiness = { "state": "ready" } | { "state": "needs-setup", reason: string, target: SummarySetupTarget, };

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
 * Four states rather than a boolean, because each needs different words on the
 * screen and leads to a different action.
 *
 * [`Self::NeedsCredential`] was added after a bench run on 2026-08-27 found
 * `Settings > Meetings` reading *ready to use* beside AssemblyAI and Corti with
 * no key stored for either, while `MeetingRuntime::start` refused them by
 * design. The screen and the command disagreed and the screen was the
 * optimistic one, which is the shape of claim this project keeps removing.
 *
 * Named `Client…` rather than plain `Availability` because `local_model` already
 * exports one — whether a *server binary* is published for this platform — and
 * two Rust types cannot share a TypeScript name. `ipc_types.rs` asserts on that,
 * and caught this.
 */
export type ClientAvailability = { "state": "available" } | { "state": "no-client", reason: string, } | { "state": "declined", reason: string, } | { "state": "needs-credential", slot: string, };

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
 * The credential slot this provider's client reads its key from, from
 * [`credential_slot`]; `None` for a provider that needs no key or has no
 * client. Settings › Providers reads it to say which keys live meetings
 * use (KNEG-373). The registry's `usedFor` cannot say that, on purpose:
 * see `assemblyai_and_deepgram_are_provider_slots_upstream_never_needed`
 * in `crates/provider-registry/src/lib.rs`.
 */
credentialSlot: string | null,
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
export type MeetingState = "idle" | "recording" | "recording-without-transcript" | "stalled" | "finishing";

/**
 * Which captures are actually running.
 *
 * Reported rather than assumed, because a meeting works microphone-only and the
 * user should know which one they got. *No system capture at all* is a
 * different fact from a capture that produced nothing.
 *
 * It used to say here that zero system-audio samples means nobody else spoke.
 * It does not, and that was KNEG-38: an output device with nothing playing
 * delivers no callback at all, so zero samples is equally what a capture that
 * never worked produces. [`Delivered`] carries which of the possible answers
 * applies, and says plainly that two of them cannot be told apart.
 */
export type Sources = { microphone: boolean,
/**
 * The microphone the meeting opened, by its device name; `None` when
 * the device gave none (KNEG-364). From the capture
 * (`kw_audio_core::Capture::device_name`), not from the setting: a choice
 * that was not connected falls back to the default, and a choice made
 * while the meeting runs applies to the next one, so the live view names
 * this rather than what is chosen now.
 */
microphoneName: string | null, systemAudio: boolean,
/**
 * Why system audio is not being captured, when it is not.
 */
systemAudioReason: string | null, };

/**
 * What a capture actually delivered over a meeting, as opposed to whether it
 * opened.
 *
 * **The distinction KNEG-38 is about.** WASAPI loopback from an output device
 * with nothing playing delivers **no callback at all**, not a stream of zeros.
 * So at the API a capture that never worked and a call where nobody else spoke
 * are the same thing, and for a meeting recording that difference is the whole
 * point. `MeetingOutcome::NothingTranscribed` was where both landed, together
 * with a third case that is neither.
 *
 * This does not make the first two distinguishable — nothing this side can,
 * and claiming otherwise would be the invention the rest of this file refuses.
 * What it does is *name* which of them happened, separate the third, and say
 * on screen why the first two look alike.
 */
export type Delivered = "not-captured" | "nothing" | "silence" | "audio";

/**
 * What a meeting is saved as, chosen before it starts (KNEG-365).
 *
 * Held by the session and written beside the recording
 * ([`draft::write_plan`]) as it starts, so every way a meeting ends applies
 * it: Stop or Discard from the meeting window with the Meetings page not
 * open, the local API, and recovery after a crash. The page that chose it
 * keeps nothing.
 *
 * `#[serde(default)]` on the whole struct, so a plan written before a field
 * was added still reads, with that field's default.
 */
export type MeetingPlan = {
/**
 * The meeting's name, tidied as the note's title will be
 * ([`note_title`]). `None` names the note for when the meeting started.
 */
title: string | null,
/**
 * The folder the note is saved to, or `None` for no folder. A folder
 * deleted before the note is saved is left out, and the outcome says so
 * (`MeetingOutcome::Saved::folder_missing`).
 */
folderId: number | null,
/**
 * Whether the recording is deleted once its transcript is saved
 * (KNEG-409): Settings > Meetings' choice, read at Start
 * ([`plan_at_start`]), never a caller's. A plan written before this
 * existed reads `false`, and keeps its recording.
 */
deleteRecording: boolean,
/**
 * The actual live provider selected at Start, never a caller's claim.
 * Older sidecars have no provider to disclose.
 */
liveProviderId: string | null, };

/**
 * What became of a saved meeting's recording, when it was to be deleted.
 *
 * `None` in the outcome where it was not asked to be.
 */
export type RecordingRemoval = "deleted" | "kept-incomplete" | "kept-untranscribed" | "deleted-update-failed" | "delete-failed";

export type Stage = "preparing-recording" | "finishing-live-transcript" | "transcribing-recording" | "analysing-speakers" | "saving-note";

export type Kind = "stop" | "recovery";

export type FinalisationProgress = { operationId: number, kind: Kind, stage: Stage, elapsedSeconds: number, };

export type FinalisationUpdate = { revision: number, progress: FinalisationProgress | null, };

/**
 * What can be said about the meeting session.
 */
export type MeetingStatus = { state: MeetingState, finalisation: FinalisationUpdate,
/**
 * Why the live transcription stopped, when it has.
 *
 * The provider's own sentence, naming which of the two sockets died --
 * [`kw_meeting_core::MeetingError::ProviderFault`] renders it and
 * [`live_transcript_ended`] formats it, so this is the same sentence the
 * outcome at Stop will carry rather than a second wording of it.
 *
 * `None` while the transcription is running, and also for the one ending
 * that has no sentence: a pump task that panicked or was aborted, which
 * [`live_state`] still reports as
 * [`MeetingState::RecordingWithoutTranscript`] because a state with no
 * explanation is closer to the truth than a claim that the transcript is
 * running.
 */
transcriptFailure: string | null,
/**
 * The provider in use, while there is one.
 */
provider: string | null,
/**
 * What the running meeting will be saved as (KNEG-365). `None` unless a
 * meeting is recording: like `provider`, it belongs to the session.
 */
plan: MeetingPlan | null, elapsedSeconds: number, sources: Sources | null,
/**
 * What this platform can do about system audio, whether or not a meeting is
 * running.
 */
systemAudio: SystemAudioSupport,
/**
 * Whether microphone activity can be read, from the last poll.
 *
 * `None` until the first poll has run. Reported rather than folded into
 * `detected`, because "Teams is open and not in a call" and "Teams is open
 * and nobody looked" are the same sentence with opposite meanings, and on
 * Linux and macOS it is always the second one.
 */
microphone: MicrophoneSupport | null,
/**
 * Meeting applications seen running at the last poll.
 *
 * Everything found, not only what would prompt: the screen shows "Teams is
 * open" even though Teams cannot be prompted for, because that is more
 * useful than silence and it is where the reason lives.
 */
detected: Array<Detected>,
/**
 * The offer waiting in the notification window, if one exists.
 *
 * A broadcast event is not recoverable: a window created hidden may mount
 * before or after it was sent. This is the read-back path the update
 * notification already has through `get_last_update_check`, so either
 * mounting order reaches the same outstanding offer.
 */
offer: Offer | null,
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
notificationExcluded: CaptureExclusion | null,
/**
 * Read back from the native window where supported.
 */
notificationActivatable: boolean | null, };

/**
 * How a meeting ended.
 */
export type MeetingOutcome = { "outcome": "saved", noteId: number, durationSeconds: number,
/**
 * Where the recording is, so the note is not the only copy.
 */
recordingPath: string,
/**
 * False when the note was saved but its recording could not be attached.
 */
recordingAttached: boolean,
/**
 * Non-zero when audio was never transcribed. A transcript with a hole in
 * it must not look complete.
 */
gaps: number,
/**
 * How many scheduled live-session reconnects completed. Kept separate
 * from gaps: reconnecting is expected for providers with a session
 * limit, while missing audio is not.
 */
reconnects: number,
/**
 * True when the whole-file pass failed and the overlapping live
 * partials were saved instead.
 */
transcriptIsPartials: boolean,
/**
 * True when the folder chosen before Start was deleted before the
 * note was saved, so the note is in no folder (KNEG-365).
 */
folderMissing: boolean,
/**
 * What became of the recording when the meeting was to delete it
 * once saved (KNEG-409); `None` when it was not, or when it could
 * not be attached (`recording_attached` says that).
 */
recordingRemoval: RecordingRemoval | null,
/**
 * Why deleting it failed, with `RecordingRemoval::DeleteFailed`.
 */
recordingRemovalError: string | null, } | { "outcome": "nothing-transcribed", recordingPath: string, mic: Delivered, system: Delivered,
/**
 * Scheduled live-session reconnects still happened even when no words
 * were produced. Keeping the count makes this successful provider work
 * visible instead of discarding it on the empty-transcript branch.
 */
reconnects: number, } | { "outcome": "cancelled", noteId: number | null, } | { "outcome": "failed", message: string, };

export type RecoverableRecording = { storageKey: string, hasNotes: boolean,
/**
 * The name the meeting was given before Start, from the plan beside it
 * (KNEG-365), tidied as the recovered note's title will be. `None` when
 * it was given none, or has no plan.
 */
title: string | null, };

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
evidence: string,
/**
 * Which signal fired, so the offer can say how sure it is.
 *
 * Never [`CallSignal::None`]: `consider` only offers on a call. Carried as
 * the enum rather than as a sentence because the wording belongs on the
 * screen, and because a microphone is the weaker of the two -- an offer
 * that appeared because Teams was running its own audio test should say
 * that is what it saw.
 */
signal: CallSignal, };

/**
 * What says a call is in progress, when anything does.
 *
 * Two signals, and they are **not** equally strong, which is the whole reason
 * this is an enum rather than the boolean it replaces the inside of:
 *
 * - [`Self::CallProcess`] is a process that cannot exist unless a call is
 *   running. Zoom's `CptHost.exe` is not started by opening Zoom.
 * - [`Self::Microphone`] is the application holding a capture device. Real
 *   evidence, and weaker: Teams holds the microphone for its own audio test
 *   too, and so does a call the user placed and nobody joined.
 *
 * A caller that offers to record is entitled to know which one fired, because
 * the two deserve different words on screen and a user told the wrong one has
 * no way to work out which signal was wrong.
 */
export type CallSignal = "none" | "call-process" | "microphone";

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
 * True when something says a call is in progress, rather than only that the
 * application is open.
 *
 * The difference between "in a meeting" and "has Zoom installed", and the
 * reason a caller can choose to prompt only on the stronger signal.
 *
 * **Private, unlike every other field here**, and serialised all the same
 * so a screen can read `inCall` without knowing about signals. It is
 * [`CallSignal::means_call`] of [`Self::signal`] and nothing else, and the
 * only way to keep two serialised fields carrying one fact from drifting is
 * to make the second one impossible to set. Read it with [`Self::in_call`].
 */
inCall: boolean,
/**
 * Which signal made [`Self::in_call`] true, and how strong it is.
 */
signal: CallSignal, };

/**
 * The user's own process names.
 *
 * `meeting-detect:generic` upstream also covered calendar events, microphone
 * activity and system-audio activity. Microphone activity is now built and
 * measured -- it reaches [`detect`] as its `capturing` argument, and a name on
 * this list holding a capture device counts as being in a call. **Calendar
 * events are still chunk F and system-audio activity is still unbuilt.** Named
 * here rather than left implied, because a half-built detector that looks whole
 * is how a user comes to trust one that misses half the meetings.
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

/**
 * Whether this platform can say which applications are capturing.
 *
 * Three states rather than a boolean, for the reason
 * `kw_audio_core::system_audio::SystemAudioSupport` has four: "this platform
 * cannot" and "this platform can and the call failed" are different problems
 * with different fixes, and neither of them is "nothing is capturing".
 *
 * That distinction is the whole point here. An empty list of capturing
 * processes and an enumeration that did not run look identical to a caller, and
 * one of them means "nobody is in a call" while the other means nothing at all.
 */
export type MicrophoneSupport = { "state": "available",
/**
 * The mechanism, named so a reader can recognise it.
 */
mechanism: string,
/**
 * How many capture endpoints were examined.
 *
 * Zero is not a failure — a machine with no microphone has none — but
 * it does mean an empty answer says nothing about who is in a call.
 */
endpoints: number, } | { "state": "failed", mechanism: string, reason: string, } | { "state": "unsupported", reason: string, };

/* ------------------------------------------------------------------ notes */
/**
 * A note, as listed.
 */
export type Note = { id: number, title: string, content: string, recordingDetails: string | null, noteType: string, enhancedContent: string | null, folderId: number | null, transcript: string | null, participants: string | null, audioDurationSeconds: number | null,
/**
 * Where the transcript was made: a [`TranscribedBy`] as JSON, kept raw
 * and read leniently, as `participants` is (KNEG-368). `None` when it was
 * never recorded -- notes from before then, typed notes, and OpenWhispr
 * and shared-folder imports.
 */
transcribedBy: string | null, createdAt: string | null, updatedAt: string | null,
/**
 * When the note was moved to Trash (KNEG-401), `None` while it is not in
 * it. A note in Trash is the same row with this set: it keeps its
 * recording, its search index rows and its people, and every list,
 * search and count leaves it out until it is restored. Only [`get`]
 * returns it, so the screen can say it is in Trash.
 */
trashedAt: string | null, };

/**
 * Lightweight desktop library entry. Large note bodies stay in `get`.
 */
export type NoteListItem = { id: number, title: string, noteType: string, folderId: number | null, participants: string | null, audioDurationSeconds: number | null, createdAt: string | null, updatedAt: string | null, trashedAt: string | null,
/**
 * A stored nonblank transcript; the full reader validates its contents.
 */
hasTranscript: boolean, };

/**
 * A folder.
 */
export type Folder = { id: number, name: string, isDefault: boolean, sortOrder: number, };

/**
 * A search hit.
 */
export type SearchHit = { id: number, title: string,
/**
 * A snippet with the match marked by `[` and `]`. The row's own when
 * `transcript_row` names one, so the time and the words agree.
 */
snippet: string,
/**
 * Where in the transcript the words were said, when every word is in a
 * meeting transcript and one row holds them all (KNEG-406). `None` when
 * the notes hold every word, when the words are only in the title or are
 * split between fields, and when no single row can be named: the note
 * then opens as it always has, rather than at a guess. A title that also
 * holds them doesn't stop a row being named.
 */
transcriptRow: SearchHitRow | null, };

/**
 * The transcript row a search hit's words are in.
 */
export type SearchHitRow = {
/**
 * Its position in the stored transcript: `TranscriptRow::index`.
 */
index: number,
/**
 * Seconds from the start of the recording, as the Transcript tab shows
 * it. `None` when the row has no time; it is still the row.
 */
startSeconds: number | null, };

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
 * One of a note's people, as the renderer sends them: an email, a name, or
 * both (KNEG-362). Checked and tidied by `people::set_participants`.
 */
export type ParticipantRequest = { email?: string | null, displayName?: string | null, };

/**
 * Someone named by email on at least one note: a row of the Notes panel's
 * People list.
 */
export type Person = {
/**
 * Trimmed and lower-case, which is how a person is told apart, and what
 * [`list_by_person`] is given to find their notes.
 */
email: string,
/**
 * The display name on the most recently updated note that gives one.
 * `None` when no note names them.
 */
name: string | null,
/**
 * How many notes they are on.
 */
noteCount: number, };

/**
 * A note in Trash, as the Trash lists it.
 */
export type TrashedNote = { id: number, title: string,
/**
 * When it was moved to Trash, as SQLite's `CURRENT_TIMESTAMP` writes it.
 */
trashedAt: string,
/**
 * Whether a recording is attached: it is deleted with the note.
 */
hasRecording: boolean,
/**
 * Recording duration, falling back to the note's meeting duration after
 * intentional recording deletion. `None` when neither was measured.
 */
recordingSeconds: number | null,
/**
 * What the recording's file takes on disk, read from the file itself. 0
 * with no recording, or one whose file is missing.
 */
recordingBytes: number,
/**
 * The name of the file the recording was copied from, for an import. The
 * original was never the app's and deleting forever doesn't touch it.
 */
originalFilename: string | null, };

/**
 * Everything in Trash, most recently trashed first, and what their
 * recordings take on this computer altogether.
 */
export type TrashSummary = { notes: Array<TrashedNote>, recordingBytes: number, };

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

/**
 * One timed word of a [`Segment`] (KNEG-408): seconds from the start of the
 * recording, or epoch milliseconds when its segment's are.
 */
export type Word = { text: string, start: number, end: number, };

/**
 * One transcript segment as a playable row: normalised to seconds from the
 * start of the recording, with its speaker resolved the way export resolves
 * it, so the screen and the export files never disagree about who said what.
 *
 * Rows rather than merged paragraphs: [`merge`] joins consecutive utterances
 * for readable export, but a row that is played back has to keep the
 * segment's own boundaries or it plays the wrong stretch of audio.
 */
export type TranscriptRow = {
/**
 * Position in the stored segment list, so a selection can be stable.
 */
index: number,
/**
 * Seconds from the start of the recording. `None` when the segment carried
 * no timestamp at all, in which case the row cannot be played.
 */
startSeconds: number | null,
/**
 * Seconds from the start of the recording. When the segment carried no end
 * of its own -- a live partial does not -- this is the next segment's
 * start, and `None` for the last such segment, meaning "to the end".
 */
endSeconds: number | null,
/**
 * The display name, resolved as [`speaker_name`] does, or `None` for an
 * anonymous segment -- one carrying no name, label or capture source --
 * which export leaves unlabelled for the same reason.
 */
speaker: string | null,
/**
 * The raw diarisation label a rename addresses; absent for typed names,
 * capture sources, and anonymous segments without a label.
 */
speakerId: string | null, text: string,
/**
 * The row word by word, on the row's clock, for highlighting each word
 * as the recording plays (KNEG-408). Empty unless the segment's words
 * still spell its text, whitespace aside: a row whose text changed after
 * it was transcribed is highlighted as a row, never at words it no
 * longer has. Empty, too, for every note saved before KNEG-408 and every
 * engine but local Whisper.
 */
words: Array<Word>, };

/**
 * A recording attached to a note, as stored.
 */
export type NoteRecording = { noteId: number,
/**
 * The file name inside the application's recordings directory. Never a
 * path, and never resolved anywhere but inside that directory.
 */
storageKey: string, durationSeconds: number | null,
/**
 * What the file was called before it became application-owned, where
 * there was such a name. A meeting recording has none.
 */
originalFilename: string | null, createdAt: string | null, };

export type NoteRecordingState = {
/**
 * Unknown for older/imported notes. No text-based backfill.
 */
status: string, error: string | null, canRetry: boolean, };

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
 * Whether the resolved route is the local model server.
 *
 * A boolean rather than leaving the screen to match `destination` against
 * the string `"local model"`: that phrase exists to be read by a person
 * and rewording it must not silently change what a control does.
 */
isLocal: boolean,
/**
 * Whether audio leaves the user's own control on this route. False for
 * the local model, and also for an `HttpBatch` endpoint on a private host
 * (`is_private_host`): that audio does leave this machine, for a server
 * on the user's network, so a screen must not read false as "stays on
 * this computer" without also checking `is_local` (KNEG-352).
 */
leavesDevice: boolean,
/**
 * Present when the route cannot be resolved at all.
 */
problem: string | null,
/**
 * The language a cloud route sends with the audio, as `route::resolve`
 * settled it from Dictation's languages and each provider's own rule
 * (xAI and Mistral drop one outside the languages they list, Corti
 * needs one). `None` where the
 * provider detects the language itself, on the local route, which reads
 * the languages on its own, and on a route with a problem (KNEG-370).
 * Read rather than re-derived, so a screen never repeats those rules.
 */
cloudLanguage: string | null, };

/* ----------------------------------------------------------- sound levels */
/**
 * What a level is for, so each meter listens only to its own.
 */
export type LevelSession = "dictation" | "meeting" | "microphone-check";

/**
 * Which capture a level is from.
 */
export type LevelSource = "microphone" | "system";

/**
 * One reading of one capture's level.
 */
export type AudioLevel = { session: LevelSession, source: LevelSource,
/**
 * 0 at −60 dBFS or quieter, 1 at full scale, linear in decibels
 * (`kw_audio_core::level_from_rms`). The loudest block since the last
 * reading, so a short sound is not missed between two.
 */
level: number, };

/**
 * What a microphone check found.
 */
export type MicrophoneCheck = {
/**
 * The microphone the check opened, by its device name; `None` when the
 * device gave none.
 */
device: string | null,
/**
 * The microphone chosen, when it was not connected and the check tested
 * the system default (`device`) instead; `None` when it tested the one
 * chosen, or none was chosen.
 */
fellBack: string | null,
/**
 * Whether it heard someone: [`HEARD_READINGS`] readings at
 * [`HEARD_LEVEL`] or louder.
 */
heard: boolean,
/**
 * The loudest reading, 0..1 as the meter shows it.
 */
peakLevel: number, };

/* ---------------------------------------------------- local transcription */
/**
 * Whether the local server can be installed here.
 */
export type Availability = { "state": "installable" } | { "state": "not-published", reason: string, tracking: string, } | { "state": "unknown" };

/**
 * How firmly a model's identity can be established.
 *
 * Four states rather than a boolean, and an enum rather than a string. It was
 * a `&'static str` whose three values the frontend re-declared by hand as a
 * union; nothing held the two in step, so a fourth state added here would have
 * reached a UI that did not know about it. Now it cannot -- and the fourth
 * state duly arrived, as [`Self::StatedDigest`].
 *
 * Shared between the whisper models and the local-LLM GGUFs, which need
 * different subsets of it: whisper reaches [`Self::Digest`] on all six and
 * [`Self::StatedDigest`] on none, and the GGUFs are the other way round for
 * thirty-two of thirty-nine objects. A variant unreachable on one path is kept
 * rather than split into two enums, for the reason
 * `models::Verification::SizeOnly` is kept: it is still the honest answer for a
 * model somebody adds tomorrow.
 */
export type VerifiedBy = "digest" | "stated-digest" | "size" | "nothing";

/**
 * Whether the installed server binary is the build this version pins.
 *
 * The wire form of [`kw_sidecar::Freshness`]. A separate type for the reason
 * [`Availability`] is one: the crate's enum carries `Sha256Digest` and a
 * structured reason, neither of which should become a TypeScript shape that
 * then has to be kept in step across the boundary.
 *
 * **`Unknown` is not a rounded-down `Stale`.** An install this application did
 * not record cannot be compared with anything, and the screen says exactly
 * that rather than picking whichever of the other two answers reads better.
 * The machine this was written on is in that state right now.
 */
export type SidecarFreshness = { "state": "not-installed" } | { "state": "current" } | { "state": "stale", installed: string, expected: string, } | { "state": "unknown", why: string, };

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
 * What the local model server is doing (KNEG-357).
 *
 * Dictation words this plainly for colleagues who are not technical: nothing
 * while it is ready, and failures in plain view with the mechanism folded.
 * [`Self::message`] is the sentence Diagnostics shows whole, model id and
 * port included. Both come from here, so they cannot drift apart; until
 * KNEG-357 the sentence was all there was, and Dictation showed it as is:
 * "Local model base is ready on port 8178."
 */
export type LocalRuntime = { "state": "stopped" } | { "state": "checking", model: string, } | { "state": "loading", model: string, } | { "state": "ready", model: string, port: number, } | { "state": "failed", error: string, } | { "state": "exited", report: string, };

/**
 * What can be said about the local model server without touching the network.
 */
export type LocalStatus = { runtimeMessage: string,
/**
 * Whether this platform has a published server binary at all, and why not.
 */
availability: Availability,
/**
 * Whether the server binary is installed.
 */
serverInstalled: boolean,
/**
 * Whether the installed one is the build this version pins.
 *
 * Separate from `server_installed`, and it has to be: "a `whisper-server.exe`
 * is there" and "it is *our* `whisper-server.exe`" are different claims,
 * and until KNEG-113 only the first was ever made. The screen showed
 * *Installed* for a binary two releases behind the manifest and nothing
 * anywhere would have said so.
 */
serverFreshness: SidecarFreshness,
/**
 * The models on offer, and what is known about each.
 */
models: Array<ModelStatus>,
/**
 * The port the server is listening on, if it is running.
 */
port: number | null,
/**
 * Which model the running server loaded, if one is running.
 *
 * Separate from `port` because "a server is up" and "*this* model is the
 * one it loaded" are different claims, and the screen shows a row per
 * model. The runtime has always tracked this for the history row; it was
 * simply never reported, so no row could say which of them was live.
 */
runningModel: string | null,
/**
 * Where installs are kept, and whether the per-user location had to be
 * abandoned because the native binaries cannot cope with the path.
 */
installDir: string, installDirFellBack: boolean,
/**
 * What the server is doing, as data. `runtime_message` is this as one
 * sentence, for Diagnostics.
 */
runtime: LocalRuntime, };

/**
 * What a refresh of a local server binary did.
 *
 * **One type for both runtimes**, `whisper-server` and `llama-server`. Not
 * politeness: `ipc_types` asserts that no two Rust types render to the same
 * TypeScript name, so a second `RefreshOutcome` for the llama.cpp refresh
 * added in KNEG-128 would have been a failing test rather than a duplicate
 * nobody noticed. The two refreshes answer the same three questions -- what
 * was unpacked, what came back, and what did not -- so sharing the answer was
 * the right resolution rather than renaming one of them.
 */
export type RefreshOutcome = {
/**
 * How many files the replacement unpacked. Reported for the reason
 * [`kw_sidecar::Installed::files`] is: one would mean the sibling `ggml`
 * libraries were missed and the server will not start.
 */
files: number,
/**
 * The model the server was running before, now running again. A whisper
 * model id or a GGUF id, depending on which screen asked.
 */
restarted: string | null,
/**
 * Why it could not be started again, when it could not. Not an error —
 * the replacement succeeded — but not something to leave unsaid either.
 */
restartError: string | null, };

/**
 * Progress of an install, as reported to the frontend.
 */
export type InstallProgress = { "stage": "server", written: number, total: number | null, } | { "stage": "model", id: string, written: number, total: number | null, } | { "stage": "verifying" } | { "stage": "extracting" } | { "stage": "done",
/**
 * What the model on disk was actually checked against.
 */
verifiedBy: VerifiedBy, };

export type SpeechInstall = { id: string, progress: InstallProgress | null, };

export type SpeechInstallSnapshot = { revision: number, jobs: Array<SpeechInstall>, };

/**
 * What a model can be used for.
 *
 * Two engines, and a model runs on exactly one of them. Not a boolean, for the
 * reason every other three-state answer in this codebase is not a boolean: the
 * screen needs to name the use, not just permit it.
 */
export type UsedFor = "dictation" | "meetings";

/**
 * Why a model this build knows about is not on offer.
 *
 * A reason rather than absence, for the same purpose
 * `ClientAvailability::NoClient` serves: someone who read that this
 * application supports Nemotron should find out here why they cannot pick it,
 * rather than wonder whether the list is broken.
 */
export type SherpaModelStatus = { id: string, name: string, sizeMb: number | null, language: string | null,
/**
 * The registry's language codes, which the settings list filters by
 * (KNEG-330). `language` says only "multilingual" for two models that do
 * not share a language list: one has Swedish, the other does not.
 */
supportedLanguages: Array<string> | null,
/**
 * Whether the extracted directory has the measured transducer layout.
 *
 * Checked by running the same discovery `transcribe` will use, so a
 * half-extracted directory does not count as installed.
 */
installed: boolean,
/**
 * Which engine it runs on, and therefore which button it gets.
 */
usedFor: UsedFor, };

/**
 * Everything the sherpa half of the screen needs, with no network access.
 */
export type SherpaStatus = {
/**
 * Whether the pinned CLI is published for this platform at all, and why
 * not. The same three-state answer `local_model` gives for
 * `whisper-server`, and for the same reason: "not published here" and "not
 * installed yet" are different facts.
 */
availability: Availability,
/**
 * Whether the offline CLI itself is installed.
 */
cliInstalled: boolean,
/**
 * Whether what is installed is the build this version pins.
 *
 * `install` has written a record beside the extracted files since
 * KNEG-113 and nothing here read it, so `cli_installed` stayed true while
 * the runtime fell behind the manifest. KNEG-128.
 *
 * Derived from the same call as `cli_installed`, which is the point --
 * see [`sherpa_status`].
 *
 * **One archive, three consumers.** The same install directory serves
 * dictation here, `sherpa_stream`'s meeting server and `diarization`'s
 * speaker CLI, under one record, so this answer is about all three at once
 * and the screen says so.
 */
cliFreshness: SidecarFreshness, models: Array<SherpaModelStatus>,
/**
 * Which model dictation is set to use, if any.
 */
selected: string | null,
/**
 * Which streaming model meetings are set to use, if any.
 *
 * A separate setting from `selected`, not a second reading of it: the two
 * engines run different models and choosing one for dictation must not
 * silently change what a meeting records with.
 */
selectedStream: string | null,
/**
 * Whether the installed streaming server can be kept to this computer:
 * its `--help` offers a host option, which `sherpa_stream` checks before
 * every start and refuses to launch without (KNEG-484). `None` when no
 * streaming server is installed, or its help could not be read. The pinned
 * v1.13.4 answers `false`, so Meetings says local streaming needs setup
 * before Start, rather than letting Start fail (KNEG-485).
 */
streamBindsLoopback: boolean | null,
/**
 * Where models are kept. Shown because the native binaries cannot open a
 * file whose path holds a non-ASCII character, so this is sometimes not
 * where a person would expect.
 *
 * A space is fine, despite what this comment said until 2026-08-31 — see
 * `kw_storage::paths` for the measurement.
 */
modelsDir: string, };

/**
 * Which half of a diarization run a model does.
 *
 * Two files, two jobs, and neither works alone: the segmentation model finds
 * where speech is and where it changes, the embedding model turns each stretch
 * into a voice fingerprint to cluster. The CLI takes one of each and refuses
 * without both.
 */
export type DiarizationRole = "segmentation" | "embedding";

export type DiarizationModelStatus = { id: string, name: string, role: DiarizationRole | null, sizeMb: number | null, installed: boolean, };

export type DiarizationStatus = {
/**
 * Whether the sherpa sidecar can be installed on this platform at all.
 */
availability: Availability, cliInstalled: boolean, models: Array<DiarizationModelStatus>,
/**
 * The setting, which is a different fact from whether it can run.
 */
enabled: boolean,
/**
 * Both models present **and** the CLI installed. `enabled` without this is
 * a setting that will be skipped, and the screen has to be able to say
 * which of the two is missing rather than showing one green tick.
 */
ready: boolean, modelsDir: string, };

/* -------------------------------------------------- local text generation */
/**
 * What the Intelligence > Local screen may say about one GGUF.
 *
 * Five answers, and the third is the reason this is not a boolean. See the
 * module documentation for what each one is worth.
 *
 * **Not `Copy` since KNEG-130.** [`Self::FailedRecheck`] carries two 64-character
 * digests, and a state that can be duplicated by accident is one a caller can
 * report twice about a file it read once.
 */
export type GgufIntegrity = { "state": "not-installed" } | { "state": "unchanged-since-install", verifiedBy: VerifiedBy, } | { "state": "unrecorded" } | { "state": "changed", recordedBytes: number, bytes: number, } | { "state": "failed-recheck", expected: string, actual: string, };

/**
 * What a re-hash of an installed model found.
 *
 * Its own type rather than a `Result<(), String>` because **a file that does
 * not match its digest is a finding, not a failure**. Returning it as an error
 * would put it through the same red line a dropped connection uses, and the two
 * deserve entirely different sentences: one says try again, the other says the
 * bytes on this disk are not the bytes that were downloaded.
 */
export type RecheckOutcome = { "outcome": "matched", verifiedBy: VerifiedBy, } | { "outcome": "mismatched", fileName: string, expected: string, actual: string, } | { "outcome": "nothing-pinned" } | { "outcome": "not-installed" };

/**
 * A drafter alongside the main model.
 */
export type DraftStatus = { fileName: string, sizeBytes: number | null,
/**
 * On the same terms as [`LocalLlmModelStatus::integrity`], and tracked
 * apart from it because a drafter is a separate file that can rot on its
 * own.
 */
integrity: GgufIntegrity,
/**
 * On the same terms as [`LocalLlmModelStatus::verified_by`]. Reported
 * separately because a drafter is a different object in a different
 * repository, and one of the two can be the weaker of the pair.
 */
verifiedBy: VerifiedBy, };

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
sizeBytes: number | null, quantization: string | null, contextLength: number | null, recommended: boolean, supportsThinking: boolean,
/**
 * Whether the file is on disk and, if it is, what is still known about it.
 *
 * This replaced an `installed: bool`, and the replacement is the whole of
 * KNEG-107. A boolean over `path.exists()` cannot tell a file that was
 * hashed at install and has not moved since from one nothing has ever
 * looked at, and it rendered both as "installed" — so a GGUF corrupted
 * months after a successful install was loaded without complaint. The
 * states, and exactly what each is worth, are [`GgufIntegrity`].
 */
integrity: GgufIntegrity,
/**
 * What integrity check this model can be held to, on the same terms the
 * whisper screen already states. Shown before the button, for the reason
 * the size is: a guarantee nobody was told about is a guarantee nobody can
 * weigh. All thirty-five carry a digest; seven of the thirty-nine objects
 * carry one this project measured, and the rest are the publisher's stated
 * `lfs.oid`, which is a different claim.
 */
verifiedBy: VerifiedBy,
/**
 * This model's speculative-decoding drafter, and what is known about it.
 * `None` for the models without one.
 */
draft: DraftStatus | null, };

/**
 * What the Intelligence > Local screen needs, with no network access.
 */
export type LocalLlmStatus = {
/**
 * Whether a server binary is published for this platform.
 */
availability: Availability, serverInstalled: boolean,
/**
 * Whether the binary on disk is the build this version pins.
 *
 * `server_installed` was the only claim this screen made about the server
 * binary, and it stays true while the binary falls releases behind the
 * manifest -- the state `local_model` reported for `whisper-server` in
 * KNEG-113 and the state this one has been in, unreported, ever since.
 * `install` has written the record since KNEG-113; nothing here read it.
 * KNEG-128.
 *
 * Derived from the same call as `server_installed`, so the two cannot
 * disagree. See [`server_state`].
 */
serverFreshness: SidecarFreshness, models: Array<LocalLlmModelStatus>,
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
export type LocalLlmProgress = { "stage": "server", written: number, total: number | null, } | { "stage": "model", id: string, written: number, total: number | null, } | { "stage": "draft", id: string, written: number, total: number | null, } | { "stage": "extracting" } | { "stage": "verifying", id: string, } | { "stage": "done", id: string,
/**
 * What was actually checked, or `None` when the files were already on
 * disk and this install re-checked nothing.
 *
 * The weaker of the model's verdict and its drafter's, where there is a
 * drafter; see [`crate::local_model::VerifiedBy::weaker`].
 */
verifiedBy: VerifiedBy | null, } | { "stage": "stopped-when-idle" };

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
 * `Permissions::is_as_expected()` returned `true` for `InheritedFromProfile`
 * — correct as "the best this build currently achieves on this platform",
 * read by a screen that only had the boolean as "verified restricted", which
 * it was not: nothing was checked, it was simply what a file under
 * `%USERPROFILE%` gets by default, and two other trustees turned up in that
 * default on a real machine. See `docs/BENCH_CHECK.md`, 2026-08-26 finding 2.
 * That helper has since been deleted; this type and its
 * [`Self::is_confirmed`] are what replaced it.
 */
export type DiscoveryPermissions = { "state": "owner-only" } | { "state": "owner-and-administrators" } | { "state": "inherited-from-profile" } | { "state": "wider",
/**
 * Octal, e.g. `"644"`, matching how a person would type it to `chmod`.
 */
mode: string, } | { "state": "extra-trustees",
/**
 * How many, not which. A trustee name is one of this machine's
 * account or group names, and this value is rendered on a screen that
 * ends up in screenshots and bug reports. The names go to the
 * application log instead.
 */
count: number, } | { "state": "still-inheriting" } | { "state": "grants-everyone" } | { "state": "incomplete" } | { "state": "directory-not-restricted" } | { "state": "unknown", reason: string, };

/**
 * What the settings screen shows about the local API.
 */
export type LocalApiStatus = { blockedReason: string | null,
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
 * boolean was `true` for `InheritedFromProfile`, so the settings screen
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
export type CliStatus = { blockedReason: string | null,
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
export type McpStatus = { blockedReason: string | null,
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
 * Where the MCP server reads the same port and token as the CLI.
 */
discoveryPath: string,
/**
 * Whether the local API is running right now. The server is a second
 * front end over it and does nothing on its own, so a client configured
 * with `command` will fail every call until this is `true`.
 */
localApiRunning: boolean,
/**
 * The same live permission read-back shown on the CLI screen. The two
 * clients consume one file, so they must never make different claims
 * about who can read it.
 */
discoveryPermissions: DiscoveryPermissions | null, };

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
 * One timed utterance of an imported transcript.
 *
 * A local mirror of `kw_transcription_core::local::TimedSegment` because that
 * crate has no `ts-rs` dependency and should not gain one to cross an IPC
 * boundary it knows nothing about. The field names match
 * `kw_notes_core::transcript::Segment`, which is what the screen writes into.
 */
export type ImportedSegment = { text: string,
/**
 * Seconds from the start of the recording.
 */
timestamp: number, endTimestamp: number, };

/**
 * A committed imported note, its recording, and the transcription result.
 */
export type MediaImportResult = { noteId: number, recording: NoteRecording, retainedBytes: number, text: string, provider: string, model: string | null,
/**
 * The transcript divided into timed utterances, when the engine timed it.
 *
 * **Empty means no timings were reported**, not "the recording was one
 * utterance". Only the local whisper server reports them, and only because
 * this path asks (`WhisperServer::timed`); a cloud provider answers with
 * prose, so an import routed to one still has no cues. The screen writes
 * one segment spanning the recording in that case and says as much.
 */
segments: Array<ImportedSegment>,
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
export type MediaImportProgress = { "stage": "downloading", sidecar: string, written: number, total: number | null, } | { "stage": "verifying" } | { "stage": "extracting" } | { "stage": "fetching" } | { "stage": "done" };

export type ImportStage = "preparing" | "converting" | "transcribing" | "saving" | "succeeded" | "failed" | "cancelled" | "interrupted";

export type ImportSnapshot = { operationId: string, stage: ImportStage, elapsedSeconds: number, provider: string | null, model: string | null, result: MediaImportResult | null, error: string | null, };

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
