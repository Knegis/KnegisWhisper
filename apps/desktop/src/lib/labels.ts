/**
 * Backend states rendered as plain language, in one place.
 *
 * Every function here turns a tagged union from Rust into words. They live
 * together because the rule they follow is a single rule, and it is easier to
 * hold to when the cases sit next to each other: **never render a state as more
 * certain than it is**. `unknown` gets the same neutral treatment as
 * `unsupported`, a stored credential is never called "connected", and a
 * dictation that heard silence is not called a failure.
 *
 * That rule came out of the upstream audit: Pluely sets Tauri's
 * `contentProtected` flag, reports nothing, and its README promises
 * "undetectable everywhere" — while the mechanism is a no-op on Linux and is
 * never verified anywhere. See `docs/PLATFORM_SUPPORT.md`.
 */

import type {
  Accelerator,
  Binding,
  CaptureExclusion,
  DictationOutcome,
  HotkeyChange,
  InstallProgress,
  NonActivating,
  ShortcutHookStatus,
  VerifiedBy,
} from "./ipc";

/** A state and, where there is one, what backs it up. */
export type Stated = { text: string; detail: string };

/**
 * Render a capture-exclusion state without overclaiming.
 *
 * `unknown` deliberately gets the same neutral treatment as `unsupported`
 * rather than a checkmark: the call succeeded but nothing verified it, and
 * telling the user they are hidden on that basis is exactly the failure mode
 * documented in docs/UPSTREAM_AUDIT.md.
 */
export function captureExclusionLabel(c: CaptureExclusion): Stated {
  switch (c.state) {
    case "active":
      return {
        text: "Hidden from screen capture",
        detail: `verified via ${c.verifiedBy.api} = ${c.verifiedBy.value}`,
      };
    case "unsupported":
      return { text: "Not supported on this system", detail: c.reason };
    case "failed":
      return { text: "Could not be applied", detail: c.error };
    case "unknown":
      return { text: "Applied, but cannot be verified", detail: c.reason };
  }
}

export function nonActivatingLabel(n: NonActivating): Stated {
  switch (n.state) {
    case "active":
      return {
        text: "Will not take focus",
        detail: `verified via ${n.verifiedBy}`,
      };
    case "unsupported":
      return { text: "Not supported on this system", detail: n.reason };
    case "failed":
      return { text: "Could not be applied", detail: n.error };
  }
}

/**
 * What one accelerator reads as, and what is wrong with it if anything.
 *
 * `hook` is the platform's answer about modifier-only chords, from
 * `Diagnostics.modifierOnlyShortcuts`. It is a parameter rather than a hardcoded
 * sentence because the honest text differs by platform: this file said
 * "not implemented yet, so this shortcut does nothing" on every platform, and
 * that became false on Windows the moment the keyboard hook landed. A frontend
 * string asserting a platform fact is the same defect as a frontend type
 * asserting a Rust one.
 *
 * `undefined` means the answer is not known yet — diagnostics has not loaded —
 * and the caveat says that rather than guessing either way.
 */
export function acceleratorLabel(
  a: Accelerator,
  hook?: ShortcutHookStatus,
): {
  text: string;
  problem: string | null;
} {
  switch (a.kind) {
    case "standard":
      return { text: a.accelerator, problem: null };
    case "modifier-only":
      return {
        text: a.accelerator,
        problem:
          hook === undefined
            ? "Uses only modifier keys. Whether that works here is still being checked."
            : hook.state === "available"
              ? `Uses only modifier keys, so it runs through ${hook.mechanism} rather than the system shortcut register. It works, and other applications still see the same keys.`
              : `Uses only modifier keys, which does nothing here. ${hook.reason}`,
      };
    case "invalid":
      return { text: a.input, problem: a.reason };
  }
}

/** What an action's accelerators read as, for prefilling the editor. */
export function acceleratorText(b: Binding): string {
  return b.accelerators.map((a) => acceleratorLabel(a).text).join(", ");
}

/**
 * What a hotkey change achieved, in one line.
 *
 * Saved and registered are different events and the user needs to tell them
 * apart: a well-formed accelerator that another application already owns is
 * stored successfully and still does nothing.
 */
export function changeLabel(change: HotkeyChange): string {
  if (change.live) {
    // Two mechanisms can make a shortcut live, and the difference is one the
    // user can notice: a hooked chord does not take the keystroke away from
    // other applications, so `Control+Super` still opens the Start menu.
    // Reporting both as plain "live" is the small lie that surfaces the first
    // time somebody wonders why.
    const hooked = change.registrations.find((r) => r.state === "hooked");
    if (hooked) {
      return `${hooked.accelerator} is live through a keyboard hook. Other applications still see the same keys.`;
    }
    const taken = change.registrations.find((r) => r.state === "registered");
    return `${taken?.accelerator ?? acceleratorText(change.binding)} is live.`;
  }
  // Nothing handles the action, which is not the same failure and must not
  // borrow the same words. "Not working" sends somebody looking for a mistake in
  // an accelerator that is spelled perfectly well; the shortcut is saved,
  // correct, and waiting for a feature.
  const unhandled = change.registrations.find((r) => r.state === "unhandled");
  if (unhandled) {
    return `Saved. ${unhandled.reason}`;
  }
  // Narrowed to the refusal variants, which are the ones carrying a reason.
  const refused = change.registrations.find(
    (r) => r.state === "rejected" || r.state === "not-registrable",
  );
  return refused
    ? `Saved, but not working: ${refused.reason}`
    : "Saved, but nothing was registered.";
}

/** A dictation outcome as one line of plain language. */
export function outcomeLabel(outcome: DictationOutcome): string {
  switch (outcome.outcome) {
    case "inserted":
      return outcome.clipboardRestored
        ? `Inserted ${outcome.text.length} characters; your clipboard was put back.`
        : `Inserted ${outcome.text.length} characters. Your clipboard was left as you changed it.`;
    case "nothing-to-transcribe":
      // Not an error. Saying "failed" here would send someone looking for a
      // fault when the microphone simply heard nothing.
      return outcome.reason === "silence"
        ? "Nothing was sent: the recording was silent."
        : `Nothing was sent: ${outcome.reason.replace("-", " ")}.`;
    case "cancelled":
      return "Cancelled.";
    case "failed":
      return outcome.message;
  }
}

/** How a model's bytes were checked, said plainly rather than implied. */
export function verificationLabel(verifiedBy: VerifiedBy): string {
  switch (verifiedBy) {
    case "digest":
      return "checksum verified";
    case "size":
      // The honest wording. A file of about the right length has not been proven
      // to be the file the publisher built.
      return "size checked only";
    case "nothing":
      return "not verified";
  }
}

export function bytes(written: number, total: number | null): string {
  const mb = (n: number) => `${(n / 1_000_000).toFixed(1)} MB`;
  return total ? `${mb(written)} of ${mb(total)}` : mb(written);
}

/** Progress as one short line. */
export function progressLabel(progress: InstallProgress): string {
  switch (progress.stage) {
    case "server":
      return `Downloading the server — ${bytes(progress.written, progress.total)}`;
    case "model":
      return `Downloading ${progress.id} — ${bytes(progress.written, progress.total)}`;
    case "verifying":
      return "Verifying…";
    case "extracting":
      return "Unpacking…";
    case "done":
      return `Installed (${verificationLabel(progress.verifiedBy)})`;
  }
}
