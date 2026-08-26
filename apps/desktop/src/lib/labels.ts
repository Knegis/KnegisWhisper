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
  ClientAvailability,
  ConfigReview,
  Detected,
  DictationOutcome,
  HotkeyChange,
  InstallProgress,
  KeytermSupport,
  MeetingOutcome,
  NonActivating,
  ShortcutHookStatus,
  Sources,
  SystemAudioSupport,
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

/* ----------------------------------------------------------------- meetings */

/**
 * What can be said about system audio, without overclaiming.
 *
 * Lifted out of `Diagnostics.tsx` because the meetings screen needs the same
 * answer, and two copies of it would be two chances to describe the same state
 * differently.
 */
export function systemAudioValue(s: SystemAudioSupport): string {
  switch (s.state) {
    case "available":
      // Deliberately not "Available": the mechanism exists and, on Linux,
      // nobody has heard it produce sound.
      return s.unverified ? "Available, never tested" : "Available";
    case "no-device":
      return "No device offers it";
    case "unsupported":
      return "Not supported here";
  }
}

/**
 * Why a meeting provider can or cannot be chosen.
 *
 * The three states get three different sentences because they lead to three
 * different actions, and because "coming soon" over a deliberate refusal is the
 * quiet dishonesty this project keeps finding upstream.
 */
export function availabilityLabel(a: ClientAvailability): string {
  switch (a.state) {
    case "available":
      return "ready to use";
    case "no-client":
      return a.reason;
    case "declined":
      return `refused: ${a.reason}`;
  }
}

/** What a provider does with keyterms, including nothing. */
export function keytermLabel(k: KeytermSupport): string {
  switch (k.state) {
    case "unsupported":
      // Both providers that do this accept them without complaint, so saying
      // nothing here would leave a field that does nothing looking like one that
      // works.
      return "keyterms are accepted and ignored";
    case "supported":
      return k.max === null
        ? "keyterms are used"
        : `up to ${k.max} keyterms are used; any more are dropped`;
  }
}

/**
 * How many keyterms will actually reach the provider, from a `ConfigReview`.
 *
 * Always rendered, not only once something is dropped — the cap is worth
 * seeing before it is ever exceeded, and a screen that only speaks up once
 * something goes wrong is a screen that computed the truncation and dropped
 * it right back on the floor.
 */
export function keytermReviewLabel(review: ConfigReview): {
  value: string;
  detail: string | undefined;
} {
  return {
    value: `${review.keytermsUsed} of ${review.keytermsRequested} used`,
    detail:
      review.keytermsDropped > 0
        ? `${review.keytermsDropped} dropped — over this provider's cap`
        : undefined,
  };
}

/**
 * Why starting will be refused, when a provider needs a language and none is
 * set. `null` when there is nothing to warn about.
 *
 * Told before the click rather than only after it: `validate` is checked
 * here, live, so the warning is visible while there is still time to type a
 * language in, not just as the text of the error `meeting_start` returns.
 */
export function languageRequiredWarning(review: ConfigReview): string | null {
  return review.languageRequired
    ? "This provider cannot detect the language on its own. Set one below, or starting will be refused."
    : null;
}

/** Which captures a running meeting actually opened. */
export function sourcesLabel(s: Sources | null): string {
  if (!s) return "";
  if (s.microphone && s.systemAudio) {
    return "your microphone and what the other participants say";
  }
  if (s.microphone) {
    return s.systemAudioReason
      ? `your microphone only — ${s.systemAudioReason}`
      : "your microphone only";
  }
  return "nothing is being recorded";
}

/** How a meeting ended, in one line. */
export function meetingOutcomeLabel(outcome: MeetingOutcome): string {
  switch (outcome.outcome) {
    case "saved": {
      const minutes = Math.round(outcome.durationSeconds / 60);
      const parts = [`Saved as note ${outcome.noteId} (${minutes} min).`];
      if (outcome.transcriptIsPartials) {
        // Said plainly rather than left for the reader to notice repeated
        // phrases and wonder.
        parts.push(
          "The whole-recording pass did not complete, so the saved transcript is the live one and some phrases repeat.",
        );
      }
      if (outcome.gaps > 0) {
        parts.push(
          `${outcome.gaps} stretch(es) of audio were never transcribed — the transcriber could not keep up.`,
        );
      }
      parts.push(`The recording is at ${outcome.recordingPath}.`);
      return parts.join(" ");
    }
    case "nothing-transcribed":
      // Not a failure. Saying "failed" would send somebody looking for a fault
      // when the room was simply quiet.
      return `Nothing was transcribed — no speech was recognised. The recording is at ${outcome.recordingPath}.`;
    case "cancelled":
      return "Discarded. The recording was deleted.";
    case "failed":
      return outcome.message;
  }
}

/**
 * What a detected meeting application means, and whether it can be offered.
 *
 * The distinction is measured, not chosen. `meeting_detect_probe` found
 * `ms-teams.exe` running with no call in progress, because new Teams runs the
 * call inside its own process — so there is no separate signal to offer on, and
 * offering on "the client is open" would prompt on every launch for anyone whose
 * Teams starts with Windows. Saying so here is what keeps that a known limit
 * rather than a bug somebody has to work out.
 */
export function detectedLabel(d: Detected): string {
  return d.inCall
    ? `in a call — detected as ${d.evidence}`
    : `open, but not detectably in a call, so recording is not offered for it — detected as ${d.evidence}`;
}
