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
 * each page's union names only the pages its own fixes go to -- Dictation's
 * `"transcription" | "shortcuts"`, Meetings' `"transcription"` plus the two
 * menus it opens itself (`MeetingFix`). That is what lets each page hand
 * `fix.view` to its own `onGoTo: (id: ViewId) => void` with no cast: the
 * check that every fix target is a real page happens at the page that calls
 * `onGoTo`, where both types are in view, rather than being assumed here.
 */

import type {
  HotkeyReport,
  MeetingProviderSummary,
  ProviderSummary,
  Registration,
  RouteReport,
  SystemAudioSupport,
} from "./ipc.js";

export type Fix<T extends string> = { view: T; label: string };

export type Readiness<T extends string> =
  | { state: "ready"; caution: string | null }
  | { state: "needs-setup"; problem: string; detail: string | null; fix: Fix<T> };

export type DictationFix = "transcription" | "shortcuts";
/**
 * Where a meeting fix goes. `"transcription"` is a page; the two `*-menu`
 * fixes are not -- they are the provider and language chips' own menus on the
 * Meetings page, which the page opens instead of navigating (`ChipMenu`'s
 * `open()`), because the value they ask for is chosen there.
 */
export type MeetingFix = "transcription" | "provider-menu" | "language-menu";

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
  /**
   * The cloud speech services, for whether the one dictation is set to has a
   * key. Optional: without it this answers as it did before KNEG-329.
   */
  providers?: readonly Pick<ProviderSummary, "id" | "name" | "configured">[];
}): Readiness<DictationFix> {
  if (i.route.problem) {
    return {
      state: "needs-setup",
      problem: "Choose where your speech is turned into text.",
      detail: i.route.problem,
      fix: { view: "transcription", label: "Open Transcription" },
    };
  }

  // A service with no key (KNEG-329). The route resolves without one -- a new
  // profile's default is OpenAI, and the resolver has no case for a missing
  // key -- so the page read "ready" and named the service, and the first
  // dictation failed at Stop with "no credential is stored". Nothing was sent
  // meanwhile; it only looked as if it would work.
  const service = i.route.isLocal ? undefined : i.providers?.find((p) => p.id === i.route.destination);
  if (service && !service.configured) {
    return {
      state: "needs-setup",
      problem: `${service.name} needs a key before it can transcribe.`,
      detail: `Dictation is set to ${service.name}, and no key is saved for it. Add its key, or install a model that runs on this computer.`,
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

/** Why the chosen local meeting provider cannot start. See [`localProviderProblem`]. */
/**
 * `fix` names a way out other than Transcription, for a problem nothing there
 * can fix: a streaming server that cannot be kept to this computer leaves only
 * another provider (KNEG-485).
 */
export type LocalProblem = { problem: string; detail: string; fix?: "provider-menu" };

/**
 * Why the chosen meeting provider, when it is one of the two that run on this
 * computer, cannot start -- or `null` when it can, or when that is not known
 * yet (KNEG-387).
 *
 * Rust lists both as always available, because neither needs a key, and until
 * this they failed only at Start. What each needs:
 *
 * - `local` (windowed Whisper): its server running, or the engine and at least
 *   one Whisper model installed. Start loads the remembered model, or the first
 *   installed one, when the server is not running (`local_model.rs`
 *   `meeting_revive_handle`). A model chosen for meetings has to be installed
 *   itself, because Start loads that one and no other (`MeetingServer`).
 * - `local-streaming`: a streaming model chosen and downloaded, and the
 *   sherpa-onnx runtime that serves it (`sherpa_stream.rs` `start`) -- a
 *   streaming server that can be kept to this computer, which `start` checks
 *   before every launch and refuses otherwise (KNEG-484, KNEG-485).
 *
 * A status that has not arrived is `null` and holds nothing: Start still says
 * what went wrong if it does.
 */
export function localProviderProblem(
  selected: string | null,
  whisper:
    | { port: number | null; serverInstalled: boolean; models: readonly { id: string; installed: boolean }[] }
    | null,
  sherpa:
    | {
        cliInstalled: boolean;
        selectedStream: string | null;
        models: readonly { id: string; installed: boolean }[];
        /** `null` when not known: no server installed, or its help unreadable. */
        streamBindsLoopback: boolean | null;
      }
    | null,
  /** `meetings.localModel`: the model chosen for meetings, or `null` for Automatic. */
  meetingModel: string | null = null,
): LocalProblem | null {
  if (selected === "local" && whisper !== null) {
    if (meetingModel !== null && !whisper.models.some((model) => model.id === meetingModel && model.installed)) {
      return {
        problem: "The model chosen for meetings isn't installed.",
        detail: "Install it under Settings > Transcription > Local, or choose another model.",
      };
    }
    const startable = whisper.serverInstalled && whisper.models.some((model) => model.installed);
    if (whisper.port !== null || startable) return null;
    return {
      problem: "Local transcription needs a model installed on this computer first.",
      detail: "Install one under Settings > Transcription > Local. Start then loads it.",
    };
  }
  if (selected === "local-streaming" && sherpa !== null) {
    if (sherpa.selectedStream === null) {
      return {
        problem: "Choose a streaming model for live meetings first.",
        detail: "It runs on this computer and is chosen under Settings > Transcription > Local.",
      };
    }
    const downloaded = sherpa.models.some(
      (model) => model.id === sherpa.selectedStream && model.installed,
    );
    if (!downloaded || !sherpa.cliInstalled) {
      return {
        problem: "The streaming model for live meetings isn't downloaded yet.",
        detail: "Download it under Settings > Transcription > Local.",
      };
    }
    if (sherpa.streamBindsLoopback === false) {
      return {
        problem: "The streaming server for live meetings can't be limited to this computer.",
        detail: "This version of it listens on the network, so it isn't started. Choose Local windowed model or a cloud provider for live meetings.",
        fix: "provider-menu",
      };
    }
  }
  return null;
}

/**
 * The one sentence for a saved meeting provider that cannot be used now.
 *
 * `name` is `null` for a provider this version of the app does not offer,
 * whose only name is its id: "local-streaming, the provider you chose" was
 * the raw id in plain view (KNEG-354). The caller puts the id in the detail.
 */
function chosenCannotBeUsed(name: string | null): string {
  return name === null
    ? "The provider you chose can't be used right now."
    : `${name}, the provider you chose, can't be used right now.`;
}

/**
 * Whether meeting transcription is ready, and if not, what to fix and where.
 *
 * `usable` is already filtered to providers that can be chosen right now
 * (`ClientAvailability.state === "available"`) -- that filtering, and picking
 * `selected` out of it, is `selectProvider`'s job, not this function's, so
 * this only ever asks how many usable providers there are and whether one is
 * currently chosen.
 *
 * `savedUnusable` is the provider the person saved, when it cannot be used
 * right now, named and with the reason. It is its own needs-setup rather than
 * a caution because `selectProvider` falls back to the first usable provider,
 * and the fallback can be the opposite privacy choice: somebody who chose the
 * local provider, whose model server is not running and who has a cloud key
 * stored, would otherwise see Ready over a Start that sends the meeting to
 * the cloud. So Start waits for a choice, and the fix is to make one. It comes
 * after "nothing usable" -- with nothing to choose between, choosing is not
 * the fix, and the detail says which saved choice it was instead -- and
 * before the language, which is a question about a provider nobody has chosen
 * yet.
 */
export function meetingReadiness(i: {
  usable: readonly MeetingProviderSummary[];
  selected: string | null;
  savedUnusable: { name: string | null; reason: string } | null;
  languageWarning: string | null;
  /** From [`localProviderProblem`]: the chosen local provider cannot start. */
  localProblem: LocalProblem | null;
  systemAudio: SystemAudioSupport;
}): Readiness<MeetingFix> {
  if (i.usable.length === 0 || i.selected === null) {
    return {
      state: "needs-setup",
      // Both ways out are something to do now, so both are said here: the one
      // fix button goes to Transcription, which is the local path, and the
      // cloud path must not be the half left behind Learn more.
      problem:
        "Meeting transcription isn't set up yet. Set up local transcription, or add an API key for a cloud service.",
      detail: [
        ...(i.savedUnusable
          ? [chosenCannotBeUsed(i.savedUnusable.name), i.savedUnusable.reason]
          : []),
        "API keys for cloud services are added under Settings > Providers.",
      ].join(" "),
      fix: { view: "transcription", label: "Set up transcription" },
    };
  }

  if (i.savedUnusable !== null) {
    return {
      state: "needs-setup",
      problem: chosenCannotBeUsed(i.savedUnusable.name),
      detail: i.savedUnusable.reason,
      fix: { view: "provider-menu", label: "Choose a provider" },
    };
  }

  // After a saved choice that can't be used -- that is about which provider,
  // this is about the one chosen -- and before the language, which is no use
  // to a provider that cannot start.
  if (i.localProblem !== null) {
    return {
      state: "needs-setup",
      problem: i.localProblem.problem,
      detail: i.localProblem.detail,
      fix: i.localProblem.fix === "provider-menu"
        ? { view: "provider-menu", label: "Choose another provider" }
        : { view: "transcription", label: "Set up local transcription" },
    };
  }

  if (i.languageWarning !== null) {
    return {
      state: "needs-setup",
      problem: "Choose a meeting language first.",
      detail: i.languageWarning,
      fix: { view: "language-menu", label: "Choose a language" },
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
