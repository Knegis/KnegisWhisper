#!/usr/bin/env node
import { waitFor, reload, press, pressText } from "./interactions.mjs";
/**
 * Measures, against dev fixtures, what Meetings does when the meeting window
 * doesn't answer Stop's or Discard's request to save its notes (KNEG-418):
 * the caveat and the two "anyway" buttons, what Discard anyway asks, that it
 * deletes only after a yes and only the meeting it asked about, and -- with
 * the window answering -- that Stop saves and Discard discards.
 *
 * Stop and Discard first ask the meeting window to save what was typed in it
 * (`flushMeetingPanelNotes`, `apps/desktop/src/lib/ipc.ts`): an event sent to
 * the `meeting` window, then up to 5 s for its answer. Before KNEG-418 the
 * fixtures had nothing behind `plugin:event|emit_to`, so neither could finish
 * here. The fixtures now answer for the meeting window, and
 * `kw.meetingWindowSilent(true)` stops them, as a window that hung would.
 *
 * the confirmation dialog is answered by a script that keeps each
 * question (`scriptConfirm`); the page's `invoke` is wrapped
 * (`installInvokeStub`) to log calls in order and, for check 4, to make the
 * fixtures report another meeting while the question is open. Each check
 * starts from a fresh load at 1000x720 with a meeting started from Meetings'
 * Start.
 */
import { mkdir, writeFile } from "node:fs/promises";
import { resolve } from "node:path";
import { parseShellArgs, shellUsage } from "./args.mjs";
import { launchEdge, killBrowser } from "./browser.mjs";
import { connectCdp } from "./cdp.mjs";
import { createReport, addCheck, failedChecks, renderMarkdown } from "./report.mjs";
import * as snippets from "./snippets.mjs";
import { isEntryPoint } from "../ci/entry.mjs";

const delay = (ms) => new Promise((resolveDelay) => setTimeout(resolveDelay, ms));
const isMain = isEntryPoint(import.meta.url);

const NAV_PILL = "button.nav-pill";
const ACTION = ".live-actions button";
/** How long `flushMeetingPanelNotes` waits for the meeting window (`lib/ipc.ts`). */
const FLUSH_TIMEOUT_MS = 5000;
/** The caveat Meetings shows once that wait has run out (`views/Meetings.tsx`). */
const CAVEAT = "The meeting window didn't confirm it saved your latest notes. Stop recording anyway saves the meeting; Discard anyway deletes the recording. Either way, notes the app already has are kept, and your most recent edit may be missing.";
/** Discard anyway's question, for a meeting the page has timed at under a minute (`discardQuestion(…, true)`). */
const ANYWAY_QUESTION = "Discard this meeting? The recording so far (under a minute) will be deleted and can't be recovered. Notes the app already has are kept as a separate note; your latest edit may be missing.";
/** Discard's own question, likewise (`discardQuestion(…)`). */
const DISCARD_QUESTION = "Discard this meeting? The recording so far (under a minute) will be deleted and can't be recovered. Any notes you typed are kept as a separate note.";
/** `meetingOutcomeLabel` for a cancelled meeting with no notes kept (`lib/labels.ts`). */
const DISCARDED = "Discarded. The recording was deleted.";
/** The calls whose order the checks read; `askConfirm` is the scripted question, logged with them. */
const WATCHED = ["meeting_status", "askConfirm", "plugin:event|emit_to", "meeting_cancel", "meeting_stop"];
/** What Discard anyway's guard does, in order, before it deletes: read, ask, read again (KNEG-394). */
const GUARD = ["meeting_status", "askConfirm", "meeting_status"];

const field = (snippet, path) => `(${snippet})?.${path}`;
const LIVE = "Boolean(document.querySelector('.meeting-live'))";
const IDLE = "Boolean(document.querySelector('.control-start')) && !document.querySelector('.meeting-live')";
const START_ENABLED = "(() => { const start = document.querySelector('.control-start'); return Boolean(start) && !start.disabled && start.textContent.trim() === 'Start recording'; })()";
const ANYWAY_SHOWN = `${field(snippets.meetingPageState(), "actions")}.includes('Stop recording anyway')`;





/** A real pointer press at a measured point, as a person's click is. */


/** Presses the element's centre, which must be on screen for the press to reach it. */


const pageState = (cdp) => cdp.evaluate(snippets.meetingPageState());
const sequence = (cdp) => cdp.evaluate(snippets.stubSequence(WATCHED));
const count = (calls, cmd) => calls.filter((name) => name === cmd).length;
/** The calls up to and including the first `meeting_cancel`, or all of them when there is none. */
const upToCancel = (calls) => calls.slice(0, calls.indexOf("meeting_cancel") + 1 || calls.length);
const flushes = (calls) => calls.filter((args) => args?.target?.label === "meeting" && args?.event === "meeting-notes-flush");

/**
 * A fresh load at 1000x720 on Meetings with a meeting started by its Start
 * button, the invoke stub installed, and the meeting window answering a notes
 * flush or, with `silent`, not.
 */
async function recordingMeeting(cdp, { silent }) {
  await reload(cdp);
  await waitFor(cdp, "Boolean(document.querySelector('.app-titlebar') && document.querySelector('.split-main, .content'))", "the fixture shell");
  await cdp.evaluate(snippets.removeFixtureBanner());
  await cdp.evaluate(snippets.installInvokeStub());
  if (silent) await cdp.evaluate(snippets.meetingWindowSilent(true));
  await cdp.evaluate(snippets.clickText(NAV_PILL, "Meetings"));
  await waitFor(cdp, START_ENABLED, "Start recording to be enabled");
  await pressText(cdp, ".control-start", "Start recording");
  await waitFor(cdp, LIVE, "the live meeting view");
  await delay(300);
}

/**
 * Stop and save with the meeting window silent, and the page's wait for its
 * answer running out. Returns how long that took, measured from here.
 */
async function unansweredStop(cdp) {
  await cdp.evaluate(snippets.clearStubCalls());
  const at = Date.now();
  await pressText(cdp, ACTION, "Stop and save");
  await waitFor(cdp, ANYWAY_SHOWN, "the meeting window's answer to run out", FLUSH_TIMEOUT_MS + 5000);
  const waitedMs = Date.now() - at;
  await delay(250);
  return {
    waitedMs,
    page: await pageState(cdp),
    calls: await sequence(cdp),
    flushes: flushes(await cdp.evaluate(snippets.stubCalls("plugin:event|emit_to"))),
  };
}

/** A recording meeting whose Stop went unanswered, the call log emptied after it. */
async function anywayButtons(cdp) {
  await recordingMeeting(cdp, { silent: true });
  const stopped = await unansweredStop(cdp);
  await cdp.evaluate(snippets.clearStubCalls());
  return stopped;
}

const SETUP_NOTE = "Each check starts from a fresh load at 1000x720 with a meeting started from Meetings' Start (the fixtures' meeting_start). Buttons are pressed with real CDP pointer presses; the confirmation dialog is scripted and logged in order with the calls.";

export async function runMeetingDiscard(options) {
  const out = resolve(options.out);
  await mkdir(out, { recursive: true });
  let browser;
  let cdp;
  try {
    browser = await launchEdge({ ...options, out });
    cdp = await connectCdp(browser.target.webSocketDebuggerUrl);
    await cdp.send("Page.enable");
    await cdp.send("Emulation.setDeviceMetricsOverride", { width: 1000, height: 720, deviceScaleFactor: 1, mobile: false });
    const report = createReport({
      url: options.url,
      edgeVersion: browser.version.Browser ?? browser.version,
      fixtureBannerRemoved: true,
      timestamp: new Date().toISOString(),
      measurementScope: "measured in headless Edge against dev fixtures, not in the Tauri window",
    });

    /** Runs one check; a step that throws is the check measured false, with the page as it was. */
    const check = async ({ id, claim, expectation, note = null }, run) => {
      const notes = [SETUP_NOTE, ...(note ? [note] : [])].join(" ");
      try {
        const result = await run();
        addCheck(report, { id, claim, expectation, note: notes, ...result });
      } catch (error) {
        let page = null;
        let calls = null;
        try {
          page = await pageState(cdp);
          calls = await sequence(cdp);
        } catch { /* no page to read */ }
        addCheck(report, { id, claim, expectation, measured: { failedAt: error.message, page, calls }, matches: false, note: notes });
      }
    };

    await check({
      id: "1",
      claim: "With the meeting window silent, Stop and save waits out the meeting window's answer, then shows the caveat and both Stop recording anyway and Discard anyway, and stops nothing.",
      expectation: [
        `kw.meetingWindowSilent(true), then Stop and save: after at least ${FLUSH_TIMEOUT_MS}ms the live view is still up with the caveat "${CAVEAT}"`,
        "and the buttons Stop recording anyway, Discard anyway, Discard and Stop and save;",
        "exactly one plugin:event|emit_to, to the meeting window's meeting-notes-flush, and no meeting_stop or meeting_cancel",
      ].join(" "),
    }, async () => {
      await recordingMeeting(cdp, { silent: true });
      const stopped = await unansweredStop(cdp);
      const shot = "01-unanswered-flush-1000x720.png";
      await cdp.screenshot(resolve(out, shot));
      const { page, calls } = stopped;
      return {
        measured: stopped,
        matches: stopped.waitedMs >= FLUSH_TIMEOUT_MS - 250
          && page.live && page.caveats.includes(CAVEAT)
          && ["Stop recording anyway", "Discard anyway", "Discard", "Stop and save"].every((name) => page.actions.includes(name))
          && count(calls, "plugin:event|emit_to") === 1 && stopped.flushes.length === 1
          && count(calls, "meeting_stop") === 0 && count(calls, "meeting_cancel") === 0,
        screenshots: [shot],
      };
    });

    await check({
      id: "2",
      claim: "Discard anyway asks first, saying the latest edit may be missing, and a no deletes nothing.",
      expectation: [
        "after check 1's state, Discard anyway with the confirmation dialog answering false: one question, exactly",
        `"${ANYWAY_QUESTION}";`,
        "the meeting's status read, then the question; no meeting_cancel and no plugin:event|emit_to;",
        "the live view still up with both anyway buttons",
      ].join(" "),
    }, async () => {
      await anywayButtons(cdp);
      await cdp.evaluate(snippets.scriptConfirm(false));
      await pressText(cdp, ACTION, "Discard anyway");
      await waitFor(cdp, `${snippets.confirmQuestions()}.length > 0`, "the question");
      await delay(800);
      const questions = await cdp.evaluate(snippets.confirmQuestions());
      const calls = await sequence(cdp);
      const page = await pageState(cdp);
      return {
        measured: { questions, calls, page },
        // A no returns before the second read; any read after the question is the page's own reload.
        matches: questions.length === 1 && questions[0] === ANYWAY_QUESTION
          && JSON.stringify(calls.slice(0, 2)) === JSON.stringify(GUARD.slice(0, 2))
          && count(calls, "meeting_cancel") === 0 && count(calls, "plugin:event|emit_to") === 0
          && page.live && page.actions.includes("Stop recording anyway") && page.actions.includes("Discard anyway"),
      };
    });

    await check({
      id: "3",
      claim: "Discard anyway with a yes deletes the meeting it asked about without asking the meeting window again, the page goes idle saying it was discarded, and the next meeting starts without the anyway buttons.",
      expectation: [
        "after check 1's state, Discard anyway with the confirmation dialog answering true: the status read, the question, the status read again, then exactly one meeting_cancel;",
        "no plugin:event|emit_to and no meeting_stop; the page idle with the caveat",
        `"${DISCARDED}"; then Start again: the live view with only Discard and Stop and save, and not the caveat`,
      ].join(" "),
      note: "The fixtures' meeting_cancel reports the cancelled outcome as Rust's cancel does, with no note kept: this tab has no meeting window to have typed in. It does not announce the idle state, so the page's own clearing of the anyway buttons is what the restart shows.",
    }, async () => {
      await anywayButtons(cdp);
      await cdp.evaluate(snippets.scriptConfirm(true));
      await pressText(cdp, ACTION, "Discard anyway");
      await waitFor(cdp, IDLE, "the page to go idle");
      await delay(400);
      const questions = await cdp.evaluate(snippets.confirmQuestions());
      const calls = await sequence(cdp);
      const page = await pageState(cdp);
      const shot = "03-discarded-1000x720.png";
      await cdp.screenshot(resolve(out, shot));
      await waitFor(cdp, START_ENABLED, "Start recording to be enabled again");
      await pressText(cdp, ".control-start", "Start recording");
      await waitFor(cdp, LIVE, "the next meeting's live view");
      await delay(400);
      const next = await pageState(cdp);
      return {
        measured: { questions, calls, page, next },
        matches: questions.length === 1 && questions[0] === ANYWAY_QUESTION
          && JSON.stringify(upToCancel(calls)) === JSON.stringify([...GUARD, "meeting_cancel"])
          && count(calls, "meeting_cancel") === 1 && count(calls, "plugin:event|emit_to") === 0
          && count(calls, "meeting_stop") === 0
          && !page.live && page.caveats.includes(DISCARDED)
          && next.live && JSON.stringify(next.actions) === JSON.stringify(["Discard", "Stop and save"])
          && !next.caveats.includes(CAVEAT),
        screenshots: [shot],
      };
    });

    await check({
      id: "4",
      claim: "If, while Discard anyway's question is open, the meeting running becomes another one, a yes deletes nothing.",
      expectation: [
        "after check 1's state, with meeting_status stubbed to elapsedSeconds 600, and the scripted question switching it to 1 before answering true:",
        "the status read, the question, the status read again, and no meeting_cancel; the live view still up with both anyway buttons.",
        "Check 3 is the same press with the meeting unchanged, which does delete",
      ].join(" "),
    }, async () => {
      await anywayButtons(cdp);
      await cdp.evaluate(snippets.stubInvoke({ merge: { meeting_status: { elapsedSeconds: 600 } } }));
      await cdp.evaluate(snippets.scriptConfirm(true, { meeting_status: { elapsedSeconds: 1 } }));
      await pressText(cdp, ACTION, "Discard anyway");
      await waitFor(cdp, `${snippets.stubSequence(["meeting_status"])}.length >= 2`, "the status to be read again");
      await delay(800);
      const questions = await cdp.evaluate(snippets.confirmQuestions());
      const calls = await sequence(cdp);
      const page = await pageState(cdp);
      const mergedAfter = await cdp.evaluate("window.__chipStub.merge.meeting_status ?? null");
      return {
        measured: { questions, calls, mergedAfter, page },
        matches: questions.length === 1
          && calls.some((call, at) => call === "askConfirm" && calls[at - 1] === "meeting_status" && calls[at + 1] === "meeting_status")
          && mergedAfter?.elapsedSeconds === 1
          && count(calls, "meeting_cancel") === 0
          && page.live && page.actions.includes("Discard anyway"),
      };
    });

    await check({
      id: "5",
      claim: "With the meeting window answering, Stop and save asks it to save its notes once and saves the meeting, without waiting out the timeout.",
      expectation: [
        "Stop and save: exactly one plugin:event|emit_to, to the meeting window's meeting-notes-flush, then one meeting_stop and no meeting_cancel;",
        `the page saying so in under ${FLUSH_TIMEOUT_MS}ms, in a notice reading a sentence starting "Saved as a note", no error, and not the caveat`,
      ].join(" "),
    }, async () => {
      await recordingMeeting(cdp, { silent: false });
      await cdp.evaluate(snippets.clearStubCalls());
      const at = Date.now();
      await pressText(cdp, ACTION, "Stop and save");
      await waitFor(cdp, snippets.MEETING_SAVED, "the page to say the meeting was saved", FLUSH_TIMEOUT_MS + 5000);
      const idleAfterMs = Date.now() - at;
      await delay(300);
      const calls = await sequence(cdp);
      const sent = flushes(await cdp.evaluate(snippets.stubCalls("plugin:event|emit_to")));
      const page = await pageState(cdp);
      const shot = "05-saved-1000x720.png";
      await cdp.screenshot(resolve(out, shot));
      return {
        measured: { idleAfterMs, calls, flushes: sent, page },
        matches: idleAfterMs < FLUSH_TIMEOUT_MS
          && count(calls, "plugin:event|emit_to") === 1 && sent.length === 1
          && calls.indexOf("plugin:event|emit_to") < calls.indexOf("meeting_stop")
          && count(calls, "meeting_stop") === 1 && count(calls, "meeting_cancel") === 0
          && (page.saved ?? "").startsWith("Saved as a note")
          && page.errors.length === 0 && !page.caveats.includes(CAVEAT),
        screenshots: [shot],
      };
    });

    await check({
      id: "6",
      claim: "With the meeting window answering, Discard asks its own question, asks the meeting window to save its notes, then deletes the meeting.",
      expectation: [
        `Discard with the confirmation dialog answering true: one question, exactly "${DISCARD_QUESTION}";`,
        "the status read, the question, the status read again, one plugin:event|emit_to to the meeting window's meeting-notes-flush, then exactly one meeting_cancel;",
        `the page idle with the caveat "${DISCARDED}"`,
      ].join(" "),
      note: "Discard's guard is the one Discard anyway now shares (confirmDiscard in Meetings.tsx); this is Discard's whole path, which the fixtures could not finish before they answered for the meeting window.",
    }, async () => {
      await recordingMeeting(cdp, { silent: false });
      await cdp.evaluate(snippets.clearStubCalls());
      await cdp.evaluate(snippets.scriptConfirm(true));
      await pressText(cdp, ACTION, "Discard");
      await waitFor(cdp, IDLE, "the page to go idle", FLUSH_TIMEOUT_MS + 5000);
      await delay(400);
      const questions = await cdp.evaluate(snippets.confirmQuestions());
      const calls = await sequence(cdp);
      const page = await pageState(cdp);
      return {
        measured: { questions, calls, page },
        matches: questions.length === 1 && questions[0] === DISCARD_QUESTION
          && JSON.stringify(upToCancel(calls)) === JSON.stringify([...GUARD, "plugin:event|emit_to", "meeting_cancel"])
          && count(calls, "meeting_cancel") === 1 && count(calls, "meeting_stop") === 0
          && !page.live && page.caveats.includes(DISCARDED),
      };
    });

    const markdown = renderMarkdown(report);
    await writeFile(resolve(out, "report.json"), `${JSON.stringify(report, null, 2)}\n`);
    await writeFile(resolve(out, "report.md"), markdown);
    process.stdout.write(markdown);
    return report;
  } finally {
    try {
      await cdp?.close();
    } finally {
      await killBrowser(browser?.child);
    }
  }
}

if (isMain) {
  try {
    const options = parseShellArgs(process.argv.slice(2));
    if (options.help) process.stdout.write(`${shellUsage().replace("shell-title-bar.mjs", "meeting-discard.mjs")}\n`);
    else {
      const failed = failedChecks(await runMeetingDiscard(options));
      // The run completed; a check measured false still fails it, so a wrapper or CI step sees it.
      if (failed.length) {
        console.error(`meeting-discard: ${failed.length} check(s) false: ${failed.join(", ")}`);
        process.exitCode = 1;
      }
    }
  } catch (error) {
    console.error(`meeting-discard failed: ${error.message}`);
    process.exitCode = 1;
  }
}
