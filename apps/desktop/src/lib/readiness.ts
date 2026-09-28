/**
 * Whether dictation and meeting transcription are ready to use, and if not,
 * what to do about it and where.
 *
 * Both functions here are pure: given the reports a screen already holds,
 * they return one verdict, rather than each screen deciding for itself
 * whether a `RouteReport.problem` outranks an empty accelerator list. That
 * verdict is `Readiness<T>` -- ready, with an optional caution that does not
 * block anything, or needs-setup, with one problem, an optional detail, and
 * one fix.
 *
 * **This file deliberately does not import `ViewId`.** `Fix<T>`'s `view` is a
 * bare string literal type, generic per caller, rather than a fixed page name:
 * `"shortcuts"` is a real page on Dictation (Task 6) and does not exist yet on
 * Meetings, which gets `"meeting-settings"` instead (Task 7). A per-page fix
 * union is what lets each page's own `onGoTo: (id: ViewId) => void` type-check
 * as `onFix` once its targets actually exist on that page, with no cast
 * standing in for a check nobody ran and no dead rail entry for a page that
 * cannot go anywhere.
 */

import type {
  HotkeyReport,
  MeetingProviderSummary,
  Registration,
  RouteReport,
  SystemAudioSupport,
} from "./ipc.js";

export type Fix<T extends string> = { view: T; label: string };

export type Readiness<T extends string> =
  | { state: "ready"; caution: string | null }
  | { state: "needs-setup"; problem: string; detail: string | null; fix: Fix<T> };

export type DictationFix = "transcription" | "shortcuts";
export type MeetingFix = "transcription" | "meeting-settings";

/**
 * Whether one registration result means the shortcut actually fires.
 *
 * The same three live states as Rust's `Registration::succeeded`
 * (`shortcuts.rs:188-193`): a hooked chord and a portal grant are both live,
 * the same as an ordinary OS registration, and `rejected`, `not-registrable`
 * and `unhandled` are not, whatever reason they carry.
 */
function isLive(result: Registration): boolean {
  return result.state === "portal" || result.state === "registered" || result.state === "hooked";
}

/** The reason a registration did not go live, or `null` for one that did. */
function registrationReason(result: Registration): string | null {
  switch (result.state) {
    case "portal":
    case "registered":
    case "hooked":
      return null;
    case "rejected":
    case "not-registrable":
    case "unhandled":
      return result.reason;
  }
}

/**
 * Whether dictation is ready, and if not, what to fix and where.
 *
 * Checked in order, because the four causes are not independent: a route with
 * no destination is worth fixing before a shortcut that would do nothing
 * useful anyway, and "never set" is a different fix from "set, and refused by
 * the platform" even though both leave the feature equally dead.
 *
 * **A `registrations` entry missing for `"dictation"` is read exactly like one
 * present with no live result, not as a separate case.** It was checked
 * whether that gap is reachable rather than assumed: `shortcuts::register_all`
 * (`shortcuts.rs:340-479`) and the Linux portal path it defers to
 * (`shortcut_portal.rs:27-97`) each build one `RegistrationReport` per binding
 * and call `Hooks::set_reports` before returning, in every mode, and
 * `commands::load_bindings` always produces one `Binding` per
 * `HotkeyAction::ALL_ACTIONS`, dictation included. `lib.rs`'s setup hook calls
 * `register_all` and only then creates the dashboard window
 * (`lib.rs:395-414`, with the comment there saying so directly), so by the
 * time anything can call the `hotkeys` command, a dictation entry already
 * exists. So an empty `registrations` list, or one with no dictation entry, is
 * not live here today -- but `HotkeyReport`'s type does not promise that, and
 * this function answers the same honest way either way rather than trusting a
 * guarantee it cannot see.
 */
export function dictationReadiness(i: {
  route: RouteReport;
  hotkeys: HotkeyReport;
  canInsert: boolean;
}): Readiness<DictationFix> {
  if (i.route.problem) {
    return {
      state: "needs-setup",
      problem: "Choose where your speech is turned into text.",
      detail: i.route.problem,
      fix: { view: "transcription", label: "Open Transcription" },
    };
  }

  const binding = i.hotkeys.bindings.find((b) => b.action === "dictation");
  if (!binding || binding.accelerators.length === 0) {
    return {
      state: "needs-setup",
      problem: "No dictation shortcut is set.",
      detail: null,
      fix: { view: "shortcuts", label: "Set a shortcut" },
    };
  }

  const report = i.hotkeys.registrations.find((r) => r.action === "dictation");
  const results = report?.results ?? [];
  if (!results.some(isLive)) {
    const first = results[0];
    return {
      state: "needs-setup",
      problem: "The dictation shortcut doesn't work on this computer.",
      detail: first ? registrationReason(first) : null,
      fix: { view: "shortcuts", label: "Change the shortcut" },
    };
  }

  return {
    state: "ready",
    caution: i.canInsert
      ? null
      : "Dictations are transcribed, but can't be typed into other apps on this computer.",
  };
}

/**
 * Whether meeting transcription is ready, and if not, what to fix and where.
 *
 * `usable` is already filtered to providers that can be chosen right now
 * (`ClientAvailability.state === "available"`) -- that filtering, and picking
 * `selected` out of it, is `selectProvider`'s job, not this function's, so
 * this only ever asks how many usable providers there are and whether one is
 * currently chosen.
 */
export function meetingReadiness(i: {
  usable: readonly MeetingProviderSummary[];
  selected: string | null;
  languageWarning: string | null;
  systemAudio: SystemAudioSupport;
}): Readiness<MeetingFix> {
  if (i.usable.length === 0 || i.selected === null) {
    return {
      state: "needs-setup",
      problem: "Meeting transcription isn't set up yet.",
      detail:
        "Set up local transcription, or add an API key for a cloud service under Settings > Providers.",
      fix: { view: "transcription", label: "Set up transcription" },
    };
  }

  if (i.languageWarning !== null) {
    return {
      state: "needs-setup",
      problem: "Choose a meeting language first.",
      detail: i.languageWarning,
      fix: { view: "meeting-settings", label: "Open meeting settings" },
    };
  }

  return {
    state: "ready",
    caution:
      i.systemAudio.state === "available"
        ? null
        : "Only your microphone can be recorded on this computer, not the other participants.",
  };
}
