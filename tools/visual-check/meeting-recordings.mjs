#!/usr/bin/env node
import { waitFor, reload, press, pressText } from "./interactions.mjs";
/**
 * Measures, against dev fixtures, deleting each meeting recording once its
 * transcript is saved (KNEG-409): the switch on Settings > Meetings, off by
 * default, and what pressing it writes; what the Meetings page says while it
 * is on; the live view's sub-line for a meeting started with it on; the
 * outcome when such a meeting is stopped; and the saved note, which has its
 * transcript and no player.
 *
 * The checkbox and the buttons are pressed with real CDP pointer presses. The
 * page's `invoke` is wrapped (`installInvokeStub`) to log what reached the
 * setting's writer. The fixtures keep the delete choice in the plan the
 * meeting started with, as Rust does, and their `meeting_stop` saves a note
 * with two transcript rows: with the recording attached when the plan keeps
 * it, and without one, its details saying why, when the plan deletes it.
 * Rust's own deletion -- the attachment and the file in one transaction, and
 * what is kept when the transcript may be incomplete -- is covered by
 * `cargo test`, not here.
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

const RAIL_ITEM = "nav.rail .rail-item";
const PANEL_ITEM = ".list-panel .panel-item";
const ACTION = ".live-actions button";

/** The copy this measures, verbatim from the brief (`SettingsMeetings.tsx`, `Meetings.tsx`, `labels.ts`). */
const CHECKBOX = "Delete each recording once its transcript is saved";
const CAVEAT = "The note keeps the transcript and your typed notes, but can't be played back. If the transcript may be incomplete, the recording is kept.";
const LEARN_MORE = "Changing this doesn't affect a meeting that's already recording. Discard always deletes the recording.";
const CONTROL_NOTE = "Recordings are deleted once each meeting is transcribed.";
const FOOT_ON = "Recordings are deleted after transcribing.";
const FOOT_OFF = "Recordings are saved on this computer.";
const LIVE_PART = "Recording deleted after saving";
const DELETED = "The recording was deleted; the transcript is in the note.";
const DETAILS = "The recording was deleted after transcribing, as chosen for this meeting.";
const NAME = "Board review";

const WIDE = { width: 1440, height: 900 };
const SMALL = { width: 720, height: 520 };

const LIVE = "Boolean(document.querySelector('.meeting-live'))";
const IDLE = "Boolean(document.querySelector('.control-start')) && !document.querySelector('.meeting-live')";
const START_ENABLED = "(() => { const start = document.querySelector('.control-start'); return Boolean(start) && !start.disabled; })()";
const NOTE_OPEN = "Boolean(document.querySelector('.note-detail input[aria-label=\"Title\"]:enabled'))";
const SETTING_SHOWN = `(${snippets.recordingsSetting(CHECKBOX)}).found`;





async function viewport(cdp, { width, height }) {
  await cdp.send("Emulation.setDeviceMetricsOverride", { width, height, deviceScaleFactor: 1, mobile: false });
}

/** A real pointer press at a measured point, as a person's click is. */


/** Presses the element's centre, which must be on screen for the press to reach it. */


const setting = (cdp) => cdp.evaluate(snippets.recordingsSetting(CHECKBOX));
const notesOnPage = (cdp) => cdp.evaluate(snippets.meetingRecordingNotes());
const pageState = (cdp) => cdp.evaluate(snippets.meetingPageState());
const noteState = (cdp) => cdp.evaluate(snippets.noteRecordingState());

/** A fresh load at `size`, the invoke stub installed, the fixture setting as `on` says. */
async function fresh(cdp, { on, size = WIDE }) {
  await viewport(cdp, size);
  await reload(cdp);
  await waitFor(cdp, "Boolean(document.querySelector('nav.rail') && document.querySelector('.card .split-main, .card .content'))", "the fixture shell");
  await cdp.evaluate(snippets.removeFixtureBanner());
  await cdp.evaluate(snippets.installInvokeStub());
  if (on) await cdp.evaluate(snippets.invokeFixture("meeting_set_delete_recordings", { enabled: true }));
  await cdp.evaluate(snippets.clearStubCalls());
}

async function openMeetings(cdp) {
  await cdp.evaluate(snippets.clickText(RAIL_ITEM, "Meetings"));
  await waitFor(cdp, START_ENABLED, "Start recording to be enabled");
  await delay(300);
}

async function openMeetingSettings(cdp) {
  await cdp.evaluate(snippets.clickText(RAIL_ITEM, "Settings"));
  await waitFor(cdp, "Boolean(document.querySelector('.list-panel .panel-item'))", "the Settings panel");
  await cdp.evaluate(snippets.clickText(PANEL_ITEM, "Meetings settings"));
  await waitFor(cdp, SETTING_SHOWN, "the Recordings checkbox");
  await delay(200);
}

/** Names the meeting, starts it with Start recording, and waits for the live view. */
async function begin(cdp) {
  await press(cdp, await cdp.evaluate(snippets.centerOfText(".control-row input.meeting-name", null)));
  await cdp.send("Input.insertText", { text: NAME });
  await pressText(cdp, ".control-start", "Start recording");
  await waitFor(cdp, LIVE, "the live meeting view");
  await delay(300);
}

/** Stop and save, with the fixtures answering for the meeting window, then the page idle. */
async function stopAndSave(cdp) {
  await pressText(cdp, ACTION, "Stop and save");
  await waitFor(cdp, IDLE, "the page to go idle");
  await delay(400);
}

/** Opens Recent's first row in Notes and its Replay tab, and reads the note. */
async function openSavedNote(cdp) {
  const title = `${PANEL_ITEM} .panel-item-title`;
  await waitFor(cdp, `[...document.querySelectorAll(${JSON.stringify(title)})].some((item) => item.textContent.trim() === ${JSON.stringify(NAME)})`, "the meeting in Recent");
  await pressText(cdp, title, NAME);
  await waitFor(cdp, `${NOTE_OPEN} && document.querySelector('.note-detail input[aria-label="Title"]').value === ${JSON.stringify(NAME)}`, "the saved note to open");
  await waitFor(cdp, "document.querySelectorAll('.note-detail .transcript-rows .transcript-text').length > 0", "its transcript rows");
  await pressText(cdp, ".note-detail [role=\"tab\"]", "Replay");
  await waitFor(cdp, "document.querySelector('.note-detail [role=\"tab\"][aria-selected=\"true\"]')?.textContent.trim() === 'Replay'", "the Replay tab");
  await delay(600);
  return noteState(cdp);
}

const SETUP_NOTE = "Each check starts from a fresh load on the fixtures, at 1440x900 unless it says otherwise, with the setting as the check says (turned on through the fixtures' meeting_set_delete_recordings before the page reads it). Buttons and the checkbox are pressed with real CDP pointer presses.";

export async function runMeetingRecordings(options) {
  const out = resolve(options.out);
  await mkdir(out, { recursive: true });
  let browser;
  let cdp;
  try {
    browser = await launchEdge({ ...options, out });
    cdp = await connectCdp(browser.target.webSocketDebuggerUrl);
    await cdp.send("Page.enable");
    await viewport(cdp, WIDE);
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
        try {
          page = { meetings: await notesOnPage(cdp), setting: await setting(cdp), note: await noteState(cdp) };
        } catch { /* no page to read */ }
        addCheck(report, { id, claim, expectation, measured: { failedAt: error.message, page }, matches: false, note: notes });
      }
    };

    await check({
      id: "1",
      claim: "Settings > Meetings has a Recordings switch, off by default, and pressing it writes the setting once.",
      expectation: [
        `a section headed "Recordings" with an unticked checkbox labelled "${CHECKBOX}", the caveat "${CAVEAT}"`,
        `and Learn more "${LEARN_MORE}"; one press: exactly one meeting_set_delete_recordings call, {enabled: true}, and the box ticked`,
      ].join(" "),
    }, async () => {
      await fresh(cdp, { on: false });
      await openMeetingSettings(cdp);
      const before = await setting(cdp);
      const shot = "01-settings-recordings-1440x900.png";
      await cdp.evaluate(`[...document.querySelectorAll('label')].find((item) => item.textContent.trim() === ${JSON.stringify(CHECKBOX)})?.scrollIntoView({ block: "center" })`);
      await delay(200);
      await cdp.screenshot(resolve(out, shot));
      await pressText(cdp, "label", CHECKBOX);
      await waitFor(cdp, `(${snippets.recordingsSetting(CHECKBOX)}).checked === true`, "the box to be ticked", 5_000).catch(() => {});
      await delay(300);
      const calls = await cdp.evaluate(snippets.stubCalls("meeting_set_delete_recordings"));
      const after = await setting(cdp);
      return {
        measured: { before, calls, after },
        matches: before.found && before.checked === false && before.heading === "Recordings"
          && before.caveats.includes(CAVEAT) && before.learnMore.includes(LEARN_MORE)
          && calls.length === 1 && calls[0]?.enabled === true && after.checked === true,
        screenshots: [shot],
      };
    });

    await check({
      id: "2",
      claim: "While the switch is on, the Meetings page says so beside Start and at the foot of its list, and still fits a 720x520 window.",
      expectation: [
        `off: no control note "${CONTROL_NOTE}" and the panel foot "${FOOT_OFF}";`,
        `on: that control note and the panel foot "${FOOT_ON}"; at 720x520 with it on, document, .split-main and .control-bar scrollWidth <= clientWidth`,
      ].join(" "),
    }, async () => {
      await fresh(cdp, { on: false });
      await openMeetings(cdp);
      const off = await notesOnPage(cdp);
      await fresh(cdp, { on: true });
      await openMeetings(cdp);
      const on = await notesOnPage(cdp);
      const shot = "02-meetings-on-1440x900.png";
      await cdp.screenshot(resolve(out, shot));
      await fresh(cdp, { on: true, size: SMALL });
      await openMeetings(cdp);
      const small = { notes: await notesOnPage(cdp), scroll: (await cdp.evaluate(snippets.taskPageState())).scroll };
      const smallShot = "02-meetings-on-720x520.png";
      await cdp.screenshot(resolve(out, smallShot));
      const fits = ({ scroll }) => scroll.scrollWidth <= scroll.innerWidth
        && scroll.mainScrollWidth <= scroll.mainClientWidth && scroll.barScrollWidth <= scroll.barClientWidth;
      return {
        measured: { off, on, small },
        matches: !off.controlNotes.includes(CONTROL_NOTE) && off.panelFoot.includes(FOOT_OFF)
          && on.controlNotes.includes(CONTROL_NOTE) && on.panelFoot.includes(FOOT_ON) && !on.panelFoot.includes(FOOT_OFF)
          && small.notes.controlNotes.includes(CONTROL_NOTE) && fits(small),
        screenshots: [shot, smallShot],
        note: "Off and on are two fresh loads. At 720x520 the control bar is measured as chip-menus check 8 measures it.",
      };
    });

    await check({
      id: "3",
      claim: "A meeting started with the switch on says so in its live sub-line, and keeps saying so after the switch is turned off; one started with it off does not.",
      expectation: [
        `started on: .live-sub includes "${LIVE_PART}", and meeting_start was not sent a delete choice;`,
        "after meeting_set_delete_recordings {enabled: false} and a remount of the page (Settings, then Meetings): still included;",
        "started off on a fresh page: not included",
      ].join(" "),
      note: "The fixtures, like Rust, read the setting into the plan at meeting_start and report that plan on meeting_status.",
    }, async () => {
      await fresh(cdp, { on: true });
      await openMeetings(cdp);
      await begin(cdp);
      const started = await notesOnPage(cdp);
      const startCalls = await cdp.evaluate(snippets.stubCalls("meeting_start"));
      const shot = "03-live-on-1440x900.png";
      await cdp.screenshot(resolve(out, shot));
      await cdp.evaluate(snippets.invokeFixture("meeting_set_delete_recordings", { enabled: false }));
      await cdp.evaluate(snippets.clickText(RAIL_ITEM, "Settings"));
      await waitFor(cdp, "Boolean(document.querySelector('.list-panel .panel-item'))", "the Settings panel");
      await cdp.evaluate(snippets.clickText(RAIL_ITEM, "Meetings"));
      await waitFor(cdp, LIVE, "the live meeting view again");
      await delay(400);
      const turnedOff = await notesOnPage(cdp);
      await fresh(cdp, { on: false });
      await openMeetings(cdp);
      await begin(cdp);
      const startedOff = await notesOnPage(cdp);
      return {
        measured: { started, startCalls, turnedOff, startedOff },
        matches: (started.liveSub ?? "").includes(LIVE_PART)
          && startCalls.length === 1 && !("deleteRecording" in (startCalls[0] ?? {}))
          && (turnedOff.liveSub ?? "").includes(LIVE_PART)
          && startedOff.liveSub !== null && !startedOff.liveSub.includes(LIVE_PART),
        screenshots: [shot],
      };
    });

    await check({
      id: "4",
      claim: "Stopping a meeting started with the switch on says the recording was deleted, and gives no path.",
      expectation: `after Start and Stop and save: a caveat starting "Saved as a note (31 min). ${DELETED}"; its Learn more is "Note N." with no ".wav" or "AppData" anywhere in the caveat or its Learn more`,
    }, async () => {
      await fresh(cdp, { on: true });
      await openMeetings(cdp);
      await begin(cdp);
      await stopAndSave(cdp);
      const page = await pageState(cdp);
      const said = await notesOnPage(cdp);
      const shot = "04-deleted-outcome-1440x900.png";
      await cdp.screenshot(resolve(out, shot));
      const caveat = page.caveats.find((text) => text.startsWith("Saved as a note")) ?? null;
      const more = said.learnMore.find((text) => text.startsWith("Note ")) ?? null;
      return {
        measured: { caveats: page.caveats, more },
        matches: caveat !== null && caveat.startsWith(`Saved as a note (31 min). ${DELETED}`)
          && more !== null && /^Note \d+\.$/.test(more)
          && !/\.wav|AppData/.test(`${caveat} ${more}`),
        screenshots: [shot],
      };
    });

    await check({
      id: "5",
      claim: "The note a meeting saved with the switch on keeps its transcript and has no player: Replay has no audio and no line plays when pressed, and its details say the recording was deleted.",
      expectation: [
        `on: the note "${NAME}" opened from Recent, Replay selected: 0 audio, 0 .recording-player, 0 .transcript-line-playable, 2 transcript rows,`,
        `recording details "${DETAILS}" and the "no recording attached" block;`,
        "off, the same steps: 1 or more audio and 2 .transcript-line-playable, so the probe sees playable lines where there are some",
      ].join(" "),
    }, async () => {
      await fresh(cdp, { on: true });
      await openMeetings(cdp);
      await begin(cdp);
      await stopAndSave(cdp);
      const on = await openSavedNote(cdp);
      const shot = "05-note-replay-on-1440x900.png";
      await cdp.screenshot(resolve(out, shot));
      await fresh(cdp, { on: false });
      await openMeetings(cdp);
      await begin(cdp);
      await stopAndSave(cdp);
      const off = await openSavedNote(cdp);
      const offShot = "05-note-replay-off-1440x900.png";
      await cdp.screenshot(resolve(out, offShot));
      return {
        measured: { on, off },
        matches: on.title === NAME && on.tab === "Replay"
          && on.audios === 0 && on.players === 0 && on.playableLines === 0 && on.rows.length === 2
          && on.disposition.includes(DETAILS) && on.noRecording.length === 1
          && off.audios >= 1 && off.playableLines === 2 && off.rows.length === 2 && off.noRecording.length === 0,
        screenshots: [shot, offShot],
        note: "The off run is the control: the same probe on a note whose recording the fixtures attached.",
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
    if (options.help) process.stdout.write(`${shellUsage().replace("shell-rail.mjs", "meeting-recordings.mjs")}\n`);
    else {
      const failed = failedChecks(await runMeetingRecordings(options));
      // The run completed; a check measured false still fails it, so a wrapper or CI step sees it.
      if (failed.length) {
        console.error(`meeting-recordings: ${failed.length} check(s) false: ${failed.join(", ")}`);
        process.exitCode = 1;
      }
    }
  } catch (error) {
    console.error(`meeting-recordings failed: ${error.message}`);
    process.exitCode = 1;
  }
}
