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

import type { Detected, Sources, SummarySettings } from "../generated/ipc-types.js";
import { destinationName } from "./controlBar.js";
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

/** A meeting's running time as `hh:mm:ss`, the canvas' recording clock. */
export function elapsedLabel(seconds: number): string {
  const s = Math.max(0, Math.floor(seconds));
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${pad(Math.floor(s / 3600))}:${pad(Math.floor((s % 3600) / 60))}:${pad(s % 60)}`;
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
