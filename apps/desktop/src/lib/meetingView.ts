/**
 * What the Meetings page says about a call, a running meeting, and the
 * meetings behind it.
 *
 * Pure, and with no React: `tests/meetingView.test.ts` imports this directly,
 * and `views/Meetings.tsx` only draws what it returns.
 *
 * # The summary line
 *
 * A saved meeting is summarised **automatically** when automatic summaries
 * are switched on -- nobody presses anything. `notes::create`
 * (`crates/notes-core/src/notes.rs:60`, and `:140` for the recovered
 * meeting's `create_with_recording_on`) queues the job in the same
 * transaction that saves the note, through `summary::enqueue_automatic`
 * (`crates/notes-core/src/summary.rs:96-106`), for a meeting note with a
 * transcript whenever `reasoning.summaries` has `enabled` set; the worker in
 * `apps/desktop/src-tauri/src/summaries.rs:229-265` claims it and sends the
 * transcript wherever `route` resolves (`:275`, `resolve_chat`). So when that
 * route leaves this computer, the transcript does too, and the control bar
 * says so beside the audio line. Read from the code, not watched happening.
 *
 * `summaryPrivacyLine` mirrors that route rather than asking Rust, because
 * `summary_settings_get` returns the settings and not the route. Where it
 * cannot tell -- a provider id Rust would refuse as unknown -- it says the
 * transcript is sent: a privacy line that errs errs towards saying so.
 */

import type { Detected, Folder, InputDevice, MicrophoneCheck, Sources, SummarySettings } from "../generated/ipc-types.js";
import { PROVIDER_REGISTRY, type ProviderRegistry } from "../generated/providers.js";
import type { MenuOption } from "./chipMenu.js";
import { destinationName, LANGUAGES, languageLabel, microphoneMenu } from "./controlBar.js";
import { parseStoredTime, timeOfDay } from "./dayGroups.js";

/**
 * The meeting application that is in a call, if one is.
 *
 * `inCall` rather than the list: "Teams is open" is in `detected` too, and
 * a card saying Teams is in a call because it is running would be the
 * false positive `Detected::in_call` exists to prevent.
 */
export function activeCall(detected: readonly Detected[]): Detected | null {
  return detected.find((d) => d.inCall) ?? null;
}

/**
 * When an interrupted recording started, from its storage key.
 *
 * Rust names a meeting's WAV `%Y%m%d-%H%M%S` in UTC, then a UUID
 * (`start`, `src-tauri/src/meeting.rs:1100-1102`); keys from before the UUID
 * have none. Anything else -- or digits that are not a real date -- is
 * `null`, so a caller falls back rather than showing an invented time.
 */
export function recordingStartedAt(storageKey: string): Date | null {
  const match = /^(\d{4})(\d{2})(\d{2})-(\d{2})(\d{2})(\d{2})(?:-[^\\/]+)?\.wav$/.exec(storageKey);
  if (match === null) return null;
  const [year, month, day, hour, minute, second] = match.slice(1).map(Number) as [number, number, number, number, number, number];
  const date = new Date(Date.UTC(year, month - 1, day, hour, minute, second));
  // `Date.UTC` rolls 31 February into March; a key that does that is not a date.
  const same =
    date.getUTCFullYear() === year &&
    date.getUTCMonth() === month - 1 &&
    date.getUTCDate() === day &&
    date.getUTCHours() === hour &&
    date.getUTCMinutes() === minute &&
    date.getUTCSeconds() === second;
  return same ? date : null;
}

/**
 * What Discard asks before it deletes a running meeting's recording
 * (KNEG-394). It sat beside Stop and save and deleted the recording on one
 * click.
 *
 * The length is the page's clock, rounded as the saved outcome rounds it
 * ("Saved as a note (38 min)."). The notes sentence is true whatever was
 * typed: `MeetingRuntime::cancel` keeps typed notes as a note when there are
 * any, and when there are none there is nothing to lose.
 *
 * `unconfirmed` is "Discard anyway"'s question (KNEG-418), asked after the
 * meeting window did not confirm it saved the notes. `cancel` still keeps the
 * notes Rust already holds (`notes.take()`), so only an edit the window had
 * not sent yet is lost -- the autosave's last 800 ms, or everything since a
 * save that failed -- and the last sentence says that instead.
 */
export function discardQuestion(elapsedSeconds: number, unconfirmed = false): string {
  const length = elapsedSeconds < 60 ? "under a minute" : `${Math.round(elapsedSeconds / 60)} min`;
  const notes = unconfirmed
    ? "Notes the app already has are kept as a separate note; your latest edit may be missing."
    : "Any notes you typed are kept as a separate note.";
  return `Discard this meeting? The recording so far (${length}) will be deleted and can't be recovered. ${notes}`;
}

/**
 * Whether the meeting running after Discard's question is the one it asked
 * about (KNEG-394). The question holds the page, so in the meantime the
 * meeting may have been stopped from its own window, or stopped and another
 * started.
 *
 * Compared by when each began, from Rust: `elapsedSeconds` counts from the
 * start and does not stop at a stall, so the same meeting gives the same start
 * on every read, and a meeting started later gives a later one. `beganMs` is
 * the start read before the question; `after` is the status read once it was
 * answered, at `nowMs`. Two reads of one meeting still differ by the time
 * each took to come back from Rust before the page read its clock, hence the
 * tolerance.
 */
export function sameMeetingStart(
  beganMs: number,
  after: { elapsedSeconds: number },
  nowMs: number,
  toleranceMs = 2000,
): boolean {
  return Math.abs(nowMs - after.elapsedSeconds * 1000 - beganMs) <= toleranceMs;
}

/** A meeting's running time as `hh:mm:ss`, the canvas' recording clock. */
export function elapsedLabel(seconds: number): string {
  const s = Math.max(0, Math.floor(seconds));
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${pad(Math.floor(s / 3600))}:${pad(Math.floor((s % 3600) / 60))}:${pad(s % 60)}`;
}

/**
 * A running meeting's clock, in seconds: from when it started to now, or to
 * the moment it stalled.
 *
 * Read off the clock rather than counted. A timer counting one second per
 * tick under-reports in a window the system throttles in the background,
 * where ticks arrive seldom, and never catches up. And held while stalled:
 * nothing is being recorded then, and a clock still running would say
 * otherwise. `startedAt` is when Rust's `elapsedSeconds` says the meeting
 * began; Rust's own count goes on through a stall (`live.started.elapsed()`,
 * `src-tauri/src/meeting.rs`), which is why the stall time is the page's.
 */
export function liveElapsed(startedAt: Date, now: Date, stalledAt: Date | null): number {
  return Math.max(0, ((stalledAt ?? now).getTime() - startedAt.getTime()) / 1000);
}

/**
 * Which captures a running meeting actually opened, as the live view's
 * sub-line says it. From `Sources`, which `meeting_start` reports, so a
 * meeting recording one source is never described as recording two. The
 * reason the second did not open is kept: it may be a failure. `null` while
 * the sources are not known yet.
 */
export function recordedSources(sources: Sources | null): string | null {
  if (sources === null) return null;
  if (sources.microphone && sources.systemAudio) return "Your microphone and the other participants";
  if (sources.microphone) {
    return sources.systemAudioReason ? `Your microphone only — ${sources.systemAudioReason}` : "Your microphone only";
  }
  return "Nothing is being recorded";
}

/**
 * The control bar's sentence about the automatic summary, or `null` when no
 * summary would leave this computer. See the module comment.
 */
export function summaryPrivacyLine(settings: SummarySettings | null): string | null {
  if (settings === null || !settings.enabled) return null;
  // `SummarySettings::validate` refuses these before anything is sent.
  if (settings.model.trim() === "") return null;
  if (settings.provider.trim() === "" && settings.selfHostedUrl === null) return null;
  // `resolve_chat`: a typed URL wins over everything, a blank provider is
  // OpenAI, and `local` and `lan` are the llama.cpp server on this computer.
  if ((settings.selfHostedUrl ?? "").trim() !== "") {
    return "The transcript is sent to your self-hosted server for a summary";
  }
  const provider = settings.provider.trim() || "openai";
  if (!settings.enterprise && (provider === "local" || provider === "lan")) return null;
  return `The transcript is sent to ${destinationName(provider)} for a summary`;
}

/**
 * The name of the model that wrote a summary, from the id stored when it
 * succeeded (`SummaryState.model`): as the menus name it when the registry
 * lists it -- `qwen3.5-9b-q4_k_m` is "Qwen3.5 9B" -- and as stored when it
 * doesn't, since an Azure deployment or a typed id has no other name.
 *
 * Only the model. Where the transcript went isn't stored with the summary,
 * and `provider` can't stand in for it (a typed URL wins over it), so the
 * summary doesn't claim one (KNEG-367).
 */
export function summaryModelName(model: string | null): string | null {
  if (model === null) return null;
  const registry: ProviderRegistry = PROVIDER_REGISTRY;
  const models: readonly { readonly id: string; readonly name: string }[] = [
    ...registry.localLlm.flatMap((provider) => provider.models),
    ...registry.reasoning.flatMap((provider) => provider.models),
  ];
  return models.find((m) => m.id === model)?.name ?? model;
}

/**
 * A short local date -- "26 Aug", or "Aug 26" in a US locale -- with the
 * year when it is not this one. The same format `groupByDay` gives its
 * groups, so a date reads one way in every panel.
 */
function shortDate(d: Date, now: Date): string {
  return d.toLocaleDateString(
    undefined,
    d.getFullYear() === now.getFullYear()
      ? { day: "numeric", month: "short" }
      : { day: "numeric", month: "short", year: "numeric" },
  );
}

/**
 * A Recent row's meta line: the day it was recorded and how long it ran,
 * "26 Aug · 31 min". Either half is left out when it is not known -- a note
 * with no stored duration says only its day -- rather than showing "0 min".
 */
export function recentMeta(createdAt: string | null, durationSeconds: number | null, now: Date): string {
  const date = parseStoredTime(createdAt);
  return [
    date === null ? null : shortDate(date, now),
    durationSeconds === null ? null : `${Math.max(1, Math.round(durationSeconds / 60))} min`,
  ]
    .filter((part) => part !== null)
    .join(" · ");
}

/**
 * An Interrupted row's title: "Recording from 1 Sep, 10:15", local time.
 * A key that does not carry its start time is "Interrupted recording"; the
 * row's tooltip still has the key.
 */
export function interruptedTitle(storageKey: string, now: Date): string {
  const started = recordingStartedAt(storageKey);
  return started === null ? "Interrupted recording" : `Recording from ${shortDate(started, now)}, ${timeOfDay(started)}`;
}

/** What a configuration review was asked about. */
export interface ReviewKey {
  provider: string | null;
  language: string;
  keytermsText: string;
}

/**
 * Whether a review verdict answers the configuration shown now. The verdict
 * arrives after the inputs change, so for a moment it describes the previous
 * ones; Start must not trust it then. No provider needs no verdict.
 */
export function reviewIsFor(
  asked: ReviewKey | null,
  provider: string | null,
  language: string,
  keytermsText: string,
): boolean {
  if (provider === null) return true;
  return (
    asked !== null &&
    asked.provider === provider &&
    asked.language === language &&
    asked.keytermsText === keytermsText
  );
}

/**
 * The Meetings page's language menu: "Detect language", then the eight
 * languages the other pickers offer.
 *
 * Rust stores any string for `meetings.language` and Settings > Meetings takes
 * free text, so a saved value that is not one of the eight is listed as
 * itself, ticked, rather than the menu showing no current row.
 *
 * "Detect language" is greyed, with the reason, for a provider that cannot
 * detect (`languageDetection`): choosing it would leave a language the
 * provider requires blank, which `start_live` refuses. With no provider
 * nothing is known, so nothing is greyed.
 */
export function meetingLanguageMenu(
  language: string,
  provider: { name: string; languageDetection: boolean } | null,
): MenuOption[] {
  const code = language.trim();
  const row = (id: string, label: string): MenuOption => ({
    id,
    label,
    group: null,
    current: id === code,
    disabled: false,
    reason: null,
  });
  const unknown = code !== "" && !LANGUAGES.some(([known]) => known === code);
  const detect = row("", languageLabel([]));
  return [
    provider !== null && !provider.languageDetection
      ? { ...detect, disabled: true, reason: `${provider.name} can't detect the language. Choose one.` }
      : detect,
    ...LANGUAGES.map(([id, label]) => row(id, label)),
    ...(unknown ? [row(code, code)] : []),
  ];
}

/**
 * The Meetings page's Folder chip: where the meeting's note will be saved,
 * chosen before Start (KNEG-365), and its menu -- "No folder", the word Notes'
 * own Move to folder uses, then every folder in the order Notes lists them.
 *
 * A chosen folder that is not in `folders` -- deleted in Notes since it was
 * chosen -- reads as no folder, so the chip never names a folder the note
 * cannot go to, and `folderId`, what Start sends, is the folder the chip
 * names. Rust makes the same call at Stop for a folder deleted while the
 * meeting ran, and the outcome says so.
 */
export function folderMenu(
  folders: readonly Folder[],
  chosen: number | null,
): { label: string; folderId: number | null; options: MenuOption[] } {
  const folder = folders.find((f) => f.id === chosen) ?? null;
  const row = (id: string, label: string, current: boolean): MenuOption => ({
    id,
    label,
    group: null,
    current,
    disabled: false,
    reason: null,
  });
  return {
    label: folder === null ? "Save to Notes" : `Save to ${folder.name}`,
    folderId: folder?.id ?? null,
    options: [
      row("", "No folder", folder === null),
      ...folders.map((f) => row(String(f.id), f.name, f.id === folder?.id)),
    ],
  };
}

/**
 * The Meetings hero's line under "Record a meeting", which was the page's
 * `PageHeader` intro (`lib/pages.ts`) -- and it promises a summary only when
 * one will come. A saved meeting is summarised by itself only while automatic
 * summaries are on (`enqueue_automatic`, `crates/notes-core/src/summary.rs`),
 * and they are off by default (`SummarySettings::default`); with them off a
 * summary is something to ask for in Notes, not something you get (R19).
 */
export function meetingIntro(summariesOn: boolean): string {
  return summariesOn
    ? "You get a transcript and a summary, saved to Notes."
    : "You get a transcript, saved to Notes.";
}

/**
 * Which provider the Meetings privacy line describes.
 *
 * A running meeting's own, whatever is saved. Otherwise the one Start will
 * use -- except while Start waits on a saved provider that cannot be used.
 * Then nothing is sent anywhere until somebody chooses, and naming the
 * fallback the provider menu offers gave the page two answers: the hero said
 * the chosen provider can't be used, and the privacy line said audio is sent
 * to another (KNEG-354).
 */
export function privacyProvider<T>(live: T | null, selected: T | null, savedUnusable: boolean): T | null {
  return live ?? (savedUnusable ? null : selected);
}

/** The heading over the Meetings microphone menu's rows (KNEG-364). */
const MICROPHONE_HEADING = "Used by meetings and dictation";

/**
 * The Meetings microphone chip and its menu, before a meeting starts: what a
 * meeting will record from, chosen here (KNEG-364), and the sentence its
 * hover carries.
 *
 * One choice for both tasks, saved across restarts (`dictation.rs`
 * `INPUT_DEVICE_KEY`, KNEG-341), so the menu is Dictation's
 * (`microphoneMenu`) under a heading that says so, and the chip's `title`
 * says that choosing here changes dictation's microphone too. A chosen device
 * that is not connected is listed as itself, "(not connected)", and recorded
 * as the system default instead (`kw_audio_core::Capture::start`).
 *
 * `devices` is `null` until the list arrives. Until then the choice is listed
 * as itself and not called unplugged: nothing is known yet either way.
 */
export function meetingMicrophone(
  devices: readonly InputDevice[] | null,
  chosen: string | null,
): { label: string; title: string; missing: string | null; options: MenuOption[] } {
  const known = devices ?? (chosen === null ? [] : [{ name: chosen, isDefault: false }]);
  return {
    ...microphoneMenu(known, chosen, MICROPHONE_HEADING),
    title: "Meetings and dictation use the same microphone. Choosing one here changes it for dictation too.",
  };
}

/**
 * The live view's microphone chip: the microphone the running meeting opened,
 * as Rust reports it (`Sources.microphoneName`, from the capture), and not the
 * one chosen now. A choice made while it runs -- here before it started, or
 * on Dictation -- is used from the next recording, and the chip must not move
 * with it (KNEG-364). A device that gave no name, or sources not read yet, are
 * "your microphone" rather than a name invented for them.
 */
export function liveMicrophone(sources: Sources | null): { label: string; title: string } {
  const name = sources?.microphoneName ?? null;
  const later = "A microphone chosen now is used from the next recording.";
  return name === null
    ? { label: "Microphone", title: `This meeting is recording from your microphone. ${later}` }
    : { label: name, title: `This meeting is recording from ${name}. ${later}` };
}

/** What the page says while "Test microphone" listens (KNEG-364). */
export const MICROPHONE_CHECK_LISTENING = "Listening for 5 seconds — say something. Nothing is recorded.";

/**
 * What a finished microphone check found, as the Meetings page says it
 * (KNEG-364): which microphone it heard you on, or heard nothing from -- and
 * first, when the one chosen was not connected, that the default stood in.
 *
 * `device` is `null` only for a device that gave no name, and a chosen device
 * is found by its name, so that is the system default.
 */
export function microphoneCheckLine(check: MicrophoneCheck): string {
  const tested = check.device ?? "the system default microphone";
  const result = check.heard
    ? `Heard you on ${tested}.`
    : `Heard nothing from ${tested}. Check it isn't muted, or choose another microphone.`;
  return check.fellBack === null ? result : `${check.fellBack} isn't connected, so this tested ${tested}. ${result}`;
}

/**
 * Which of the design's three pictures a recent meeting's tile shows. Decoration,
 * so all it has to be is stable, and different from its neighbours: notes are
 * numbered in order, so meetings next to each other in the list usually differ.
 */
export function recentPicture(id: number): "dusk" | "streak" | "orb" {
  const pictures = ["dusk", "streak", "orb"] as const;
  return pictures[((id % 3) + 3) % 3] ?? "dusk";
}
