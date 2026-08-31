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
  AppleCalendarSupport,
  BinaryLocation,
  Binding,
  CaptureExclusion,
  ClientAvailability,
  ConfigReview,
  ConnectOutcome,
  Detected,
  DictationOutcome,
  DiscoveryPermissions,
  HotkeyChange,
  ImportReport,
  InstallProgress,
  KeytermSupport,
  LocalAvailability,
  MediaImportProgress,
  MediaImportSource,
  MeetingOutcome,
  NonActivating,
  RelocatedCredential,
  ShortcutHookStatus,
  Sources,
  SyncOutcome,
  SystemAudioSupport,
  TokenStatus,
  UpdateCheckResult,
  VerifiedBy,
} from "./ipc";

/**
 * A state and, where there is one, what backs it up.
 *
 * `caveat` is rarer: a sentence worth its own paragraph rather than a `Row`'s
 * detail line, for a state that needs more room to explain than "the reason
 * next to the value" affords. Optional, and absent for most states — most of
 * this file's callers never set it.
 */
export type Stated = { text: string; detail: string; caveat?: string };

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
 *
 * A caution is appended rather than replacing the verdict, because both are
 * true at once and the order matters: the hotkey *did* save and it *is* live,
 * and the surprise comes after. Leading with the warning would read as a
 * failure, which this is not -- KNEG-37's `Control+Alt` works perfectly, which
 * is the problem.
 */
export function changeLabel(change: HotkeyChange): string {
  const caution = change.cautions.map((c) => c.reason).join(" ");
  const withCaution = (verdict: string) =>
    caution ? `${verdict} ${caution}` : verdict;
  return withCaution(changeVerdict(change));
}

/** The saved-and-registered half of [`changeLabel`], without any caution. */
function changeVerdict(change: HotkeyChange): string {
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

/**
 * How a model's bytes were checked, said plainly rather than implied.
 *
 * The two digest cases are deliberately not the same phrase. Both mean the file
 * hashed to the digest it was held to; they differ in where that digest came
 * from, and that is the whole of the difference between "nobody who serves this
 * download could have chosen the answer" and "the publisher could". Collapsing
 * them into "checksum verified" would be the exact rounding-up that
 * `LOCAL_LLM_PINS` carries a per-row flag to prevent.
 */
export function verificationLabel(verifiedBy: VerifiedBy): string {
  switch (verifiedBy) {
    case "digest":
      return "checksum verified";
    case "stated-digest":
      return "checksum verified against the publisher's own";
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

/* ------------------------------------------------------------ media import */

/**
 * Progress of installing a media-import sidecar, or of a running import, as
 * one line.
 *
 * Shares `bytes()` with `progressLabel` above, but is a separate function
 * because `MediaImportProgress` is a distinct Rust type from
 * `InstallProgress` — see that type's own doc on why: `ipc_types` asserts the
 * two cannot collide in the generated TypeScript, and merging their labels
 * here would quietly assume they always agree.
 */
export function mediaImportProgressLabel(progress: MediaImportProgress): string {
  switch (progress.stage) {
    case "downloading":
      return `Downloading ${progress.sidecar} — ${bytes(progress.written, progress.total)}`;
    case "verifying":
      return "Verifying…";
    case "extracting":
      return "Unpacking…";
    case "fetching":
      return "Fetching the url…";
    case "converting":
      return "Converting…";
    case "transcribing":
      return "Transcribing…";
    case "done":
      return "Done.";
  }
}

/**
 * Why a media-import sidecar cannot be installed here, or `null` when it can
 * be. `unknown` gets its own honest sentence rather than being folded into
 * "not published": nothing here is being claimed about the platform, only
 * that this build could not tell.
 */
export function sidecarUnavailableReason(a: LocalAvailability): string | null {
  switch (a.state) {
    case "installable":
      return null;
    case "not-published":
      return a.reason;
    case "unknown":
      return "this platform could not be determined, so installing has not been offered";
  }
}

/** Where an imported transcript came from, in a few words. */
export function mediaImportSourceLabel(source: MediaImportSource): string {
  switch (source.kind) {
    case "audio":
      return "an imported audio file";
    case "video":
      return "an imported video file";
    case "url":
      return `imported from ${source.host}`;
  }
}

/**
 * What an OpenWhispr database import copied and skipped, in one line.
 *
 * Never silent about a skip: a count with no acknowledgement that some rows
 * did not make it across would be exactly the "quietly discarded rows" defect
 * this screen exists to rule out.
 */
export function importReportSummary(report: ImportReport): string {
  const copied = report.tables.reduce((n, t) => n + t.copied, 0);
  const skipped = report.tables.reduce((n, t) => n + t.skipped, 0);
  return skipped === 0
    ? `${copied} row${copied === 1 ? "" : "s"} copied.`
    : `${copied} row${copied === 1 ? "" : "s"} copied, ${skipped} skipped.`;
}

/**
 * Which columns were dropped from which table, for the rows worth showing —
 * a table that lost no columns is left out rather than listed with nothing
 * next to it.
 *
 * Mirrors `storage::import::dropped_by_table`'s grouping. Computed here
 * rather than by a second command: the fields it groups
 * (`ImportReport.tables[].droppedColumns`) already crossed the wire in the
 * report the import itself returned, so a round trip to reshape them would
 * ask Rust for a fact the frontend already has.
 */
export function droppedColumnsByTable(
  report: ImportReport,
): { table: string; columns: string[] }[] {
  return report.tables
    .filter((t) => t.droppedColumns.length > 0)
    .map((t) => ({ table: t.table, columns: t.droppedColumns }));
}

/** A relocated credential, without ever naming its value. */
export function relocatedCredentialLabel(c: RelocatedCredential): string {
  return `${c.account} — moved from ${c.sourceTable} into credential slot ${c.slot}`;
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
    // Named as something the reader can act on, and naming the screen to act
    // on it -- the same sentence `MeetingRuntime::start` refuses with, so the
    // list and the click cannot say different things.
    case "needs-credential":
      return `no key stored for ${a.slot}. Add one under Settings > Providers.`;
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
 *
 * `support` distinguishes *why* terms were dropped, because `ConfigReview`
 * cannot: `KeytermSupport::effective_count` returns 0 for every requested
 * term when a provider is `Unsupported` (OpenAI Realtime, Tinfoil, and the
 * local model all ignore keyterms entirely), which is not the same fact as a
 * numeric cap being exceeded. Saying "over this provider's cap" in the first
 * case would be a false statement about why the user's input did nothing.
 */
export function keytermReviewLabel(
  review: ConfigReview,
  support: KeytermSupport,
): {
  value: string;
  detail: string | undefined;
} {
  return {
    value: `${review.keytermsUsed} of ${review.keytermsRequested} used`,
    detail:
      review.keytermsDropped === 0
        ? undefined
        : support.state === "unsupported"
          ? "dropped — this provider ignores keyterms entirely"
          : `${review.keytermsDropped} dropped — over this provider's cap`,
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
      // Still not a failure, and still not one named cause — but a narrower
      // set of causes than this used to cover, and the sentence says which.
      //
      // A provider that *reports* a fault no longer arrives here. All four
      // cloud clients now record an error frame, a close frame's reason, a
      // dead transport, or an unreadable first frame into `SessionFault`, and
      // `send_audio` and `finish` turn that into an `Err`, so a rejected
      // session-config frame ends the meeting as `failed` carrying what the
      // provider said. That was the confusion this comment used to describe
      // and it is gone.
      //
      // What still lands here is genuinely ambiguous and is meant to be: a
      // quiet room, and a provider that accepted the session, said nothing was
      // wrong, and produced no transcript anyway — a silently misconfigured
      // session, or a system-audio loopback with no samples in it. Nothing
      // this side can separate those, so the sentence names both rather than
      // picking one. Saying "failed" would send somebody hunting a fault that
      // may not exist.
      return `Nothing was transcribed — no speech was recognised, or the provider returned nothing without reporting a fault. The recording is at ${outcome.recordingPath}.`;
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

/* ------------------------------------------------------------------ updater */

/**
 * What a check for an update found, honestly.
 *
 * `no-public-key-configured` gets its own sentence rather than being folded
 * into "up to date" or left silent: this build has no updater public key set,
 * so a downloaded update's authenticity could never be checked, and that is a
 * different fact from "nothing new was published". See `docs/SECURITY.md`.
 */
export function updateCheckLabel(result: UpdateCheckResult): Stated {
  switch (result.state) {
    case "no-public-key-configured":
      return {
        text: "Update checking is not securely configured",
        detail:
          "no updater public key is set, so a downloaded update's authenticity could never be checked. See docs/SECURITY.md for how to generate one.",
      };
    case "check-failed":
      return { text: "Could not check for an update", detail: result.error };
    case "current-version-unparseable":
      return {
        text: "This build's own version could not be read",
        detail: `unparseable version: ${result.version}`,
      };
    case "latest-version-unparseable":
      return {
        text: "The published release could not be read",
        detail: `unparseable tag: ${result.tag}`,
      };
    case "up-to-date":
      return { text: "Up to date", detail: `version ${result.current}` };
    case "update-available":
      return {
        text: `Update available: v${result.latest}`,
        detail: `you have v${result.current} — ${result.url}`,
      };
  }
}

/* ----------------------------------------------------------------- calendars */

/**
 * A connected account's token, without overclaiming.
 *
 * `active` means the cached access token has not yet passed its recorded
 * expiry — a local fact, not a network probe. `needs-refresh` is not shown
 * as "disconnected": nothing has tried to refresh it yet since it expired,
 * and the next sync does that automatically, most of the time silently.
 * `missing` covers both "never connected" and the credential having been
 * removed while a database row lingers — see `calendar.rs::token_status` —
 * and both read the same to a person: not connected. `refresh-failed` is the
 * state `needs-refresh` must never be confused with: it means a sync
 * already tried to refresh this exact token and recorded that it failed
 * (`calendar.rs::combine_token_status`), so this reads as not connected
 * rather than as an automatic retry that is quietly pending.
 */
export function tokenStatusLabel(status: TokenStatus): Stated {
  switch (status.state) {
    case "active":
      return { text: "Connected", detail: "the cached access token is still valid" };
    case "needs-refresh":
      return {
        text: "Connected",
        detail: "the access token has expired; the next sync refreshes it automatically",
      };
    case "missing":
      return { text: "Not connected", detail: "" };
    case "refresh-failed":
      return {
        text: "Not connected",
        detail: `the last attempt to refresh the access token failed (${status.message}); reconnect the account`,
      };
  }
}

/** What clicking "Connect" produced. */
export function connectOutcomeLabel(outcome: ConnectOutcome): string {
  switch (outcome.state) {
    case "connected":
      return `Connected as ${outcome.accountEmail} (${outcome.calendars} calendar${outcome.calendars === 1 ? "" : "s"}).`;
    case "denied":
      return `Not connected: access was declined (${outcome.reason}).`;
  }
}

/**
 * What one sync attempt found, distinguishing "nothing changed" from
 * "could not check". An empty count on `completed` is a real fact — nothing
 * changed since the last sync — never used to paper over `token-unavailable`
 * or `failed`, which get their own sentences naming what went wrong.
 */
export function syncOutcomeLabel(outcome: SyncOutcome): string {
  switch (outcome.state) {
    case "completed": {
      const parts = [
        `Synced: ${outcome.eventsUpserted} event${outcome.eventsUpserted === 1 ? "" : "s"} updated, ${outcome.eventsRemoved} removed.`,
      ];
      if (outcome.calendarErrors.length > 0) {
        parts.push(
          `${outcome.calendarErrors.length} calendar(s) could not be synced: ${outcome.calendarErrors.join("; ")}`,
        );
      }
      return parts.join(" ");
    }
    case "token-unavailable":
      return `Could not sync: the token could not be refreshed (${outcome.message}). Reconnect the account.`;
    case "failed":
      return `Sync failed: ${outcome.message}`;
  }
}

/**
 * What this build can do about Apple Calendar. Always the reason EventKit is
 * unimplemented — see `kw_calendar_core::apple`'s module doc — never an
 * empty calendar list that would read as "you have none".
 */
export function appleCalendarSupportLabel(support: AppleCalendarSupport): Stated {
  return { text: "Not supported in this build", detail: support.reason };
}

/* ------------------------------------------------------------ developer tools */

/**
 * The discovery file's real permission state, said plainly.
 *
 * The one rule this exists to enforce: a state nothing checked never reads as
 * a pass. That line is checked versus unchecked, not strict versus lax —
 * `owner-and-administrators` deliberately lets administrators read the file
 * and is still a pass, because the access list was read back entry by entry
 * and an administrator can take ownership of anything regardless;
 * `inherited-from-profile` may well be perfectly restrictive and is still not,
 * because nothing looked. A bench check found the original of that mistake:
 * the backend's `is_as_expected()` returned `true` for the inherited case,
 * correct as "the best this build currently achieves" and wrong read as
 * "checked and confirmed". Every case here names its evidence, the same
 * discipline `captureExclusionLabel` follows for `unknown`.
 *
 * The wording is per-state and per-platform on purpose. `owner-only` says
 * `0600`, which is a Unix fact and is only ever produced on Unix.
 * `owner-and-administrators` is the Windows result and names administrators,
 * because they really can read it and a sentence that left them out would be
 * the overstatement this whole vocabulary exists to avoid.
 *
 * `inherited-from-profile`'s `caveat` used to be a paragraph of JSX,
 * hand-written identically in both `Integrations > API` and
 * `Integrations > CLI`. Two copies of the same sentence is the exact
 * "definitions that can drift apart" shape this task was about, one layer
 * up — an edit to one screen would silently leave the other telling the user
 * something different about the same file. It lives here once; both screens
 * render whatever this function says.
 */
export function discoveryPermissionsLabel(p: DiscoveryPermissions): Stated {
  switch (p.state) {
    case "owner-only":
      return {
        text: "Restricted to your user account",
        detail: "verified by reading the file's mode back: 0600",
      };
    case "owner-and-administrators":
      return {
        text: "Restricted to you, the system and administrators",
        detail:
          "verified by reading the file's access list back: your account, NT AUTHORITY\\SYSTEM and BUILTIN\\Administrators, nobody else, and nothing inherited from the folder above",
        caveat:
          "An administrator on this machine can still read the token. That is deliberate: an administrator can take ownership of any file and grant themselves access anyway, so leaving them out would look stricter without being stricter.",
      };
    case "inherited-from-profile":
      return {
        text: "Not independently restricted",
        detail:
          "it has whatever permissions the folder holding it happens to give. Nothing here narrowed them and nothing here read them back, so this is not the same as the file having been locked down on its own",
        caveat:
          "Anything that can read files in that folder can use the local API. This build has no way to narrow permissions on this platform, so what the folder allows is what the token allows.",
      };
    case "wider":
      return {
        text: "More readable than intended",
        detail: `mode ${p.mode} — anything that can read it can use the local API`,
      };
    case "extra-trustees":
      return {
        text: "Readable by accounts you did not choose",
        detail: `the access list grants ${p.count} account${p.count === 1 ? "" : "s"} or group${p.count === 1 ? "" : "s"} beyond your own, the system and administrators — each of them can read the token and drive the local API`,
        caveat:
          "Which accounts is in the application log rather than on this screen, so a screenshot of this page does not carry your machine's account and group names.",
      };
    case "still-inheriting":
      return {
        text: "Inheriting its permissions from the folder above",
        detail:
          "the accounts it grants are the right ones today, but they come from the folder rather than from the file, so they change whenever the folder's do — without anything here being asked",
      };
    case "grants-everyone":
      return {
        text: "Readable by every account on this machine",
        detail:
          "the access list grants an everyone-style group, or there is no access list at all — which permits everything rather than nothing. Any process on this machine can use the local API",
      };
    case "incomplete":
      return {
        text: "Not the permissions that were applied",
        detail:
          "nothing unexpected is in the access list, but what was asked for is not all there. The application may be unable to rewrite this file on its next start",
      };
    case "directory-not-restricted":
      return {
        text: "In a folder anything can write to",
        detail:
          "the file's own permissions are right, but anything that can write to the folder can delete it and put its own file there — and whatever reads it next would get that port and that token instead",
      };
    case "unknown":
      return {
        text: "Permissions could not be determined",
        detail: `${p.reason} — treat the local API as reachable by anything running as you`,
      };
  }
}

/**
 * Where a companion binary (the CLI, or the MCP server) was actually found,
 * or the plain admission that it was not found anywhere this checked.
 *
 * Never "not installed": this only ever checked next to the running
 * application and on `PATH`, and says exactly that rather than a broader
 * claim neither check supports.
 */
export function binaryLocationLabel(b: BinaryLocation): Stated {
  switch (b.state) {
    case "sibling":
      return {
        text: "Found",
        detail: `${b.path} — built alongside this application`,
      };
    case "on-path":
      return { text: "Found on PATH", detail: b.path };
    case "not-found":
      return {
        text: "Not found",
        detail:
          "not next to this application and not on PATH. Build it with cargo and either place it alongside this application's own binary or put it on PATH",
      };
  }
}
