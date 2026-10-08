#!/usr/bin/env node
import { waitFor, reload, press, pressText } from "./interactions.mjs";
/**
 * Measures, against dev fixtures, choosing the microphone on the Meetings page
 * and testing it (KNEG-364): the chip is a listbox menu under a heading that
 * says the choice is shared, choosing writes `dictation_set_device` once and
 * Dictation's chip agrees, a live meeting's chip names the microphone the
 * meeting opened however the choice changes while it runs, and "Test
 * microphone" listens for five seconds with a hidden meter while Start is
 * held, then says what it heard -- or why it couldn't, in view.
 *
 * Choices are made by pressing the chip and its row, as a person would. The
 * page's `invoke` is wrapped (`installInvokeStub`) to log the calls and, for
 * one failure, to refuse the check. The fixtures' `microphone_check` sends
 * fake levels on the check's own session for 5 s and answers as Rust would,
 * refusing while a dictation records (`kw.dictationState`); their
 * `meeting_start` names the microphone it opened, as Rust's `Sources` does.
 * No audio reaches a browser tab, so a real microphone is not measured here
 * (the Rust check's ignored test is, by hand). Each check starts from a fresh
 * load at 1000x720 on Meetings, except where it says otherwise.
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
const CHIP = "Microphone: ";
/** The Meetings menu's heading and footer, and the chip's title (`lib/meetingView.ts`, `views/Meetings.tsx`). */
const HEADING = "Used by meetings and dictation";
const FOOTER = "Test microphone";
const SHARED = "Meetings and dictation use the same microphone. Choosing one here changes it for dictation too.";
/** The fixtures' devices (`apps/desktop/src/lib/devFixtures.ts`). */
const DEFAULT = "Microphone Array (Realtek(R) Audio)";
const HEADSET = "Headset (Jabra Evolve2 65)";
const LINE_IN = "Line In (Focusrite Scarlett 2i2)";
const UNPLUGGED = "USB Mic";
const ROWS = ["System default", `${DEFAULT} (default)`, HEADSET, LINE_IN];
/** What the check says (`MICROPHONE_CHECK_LISTENING`, `microphoneCheckLine`). */
const LISTENING = "Listening for 5 seconds — say something. Nothing is recorded.";
const HEARD_DEFAULT = `Heard you on ${DEFAULT}.`;
const HEARD_NOTHING = `Heard nothing from ${DEFAULT}. Check it isn't muted, or choose another microphone.`;
const FELL_BACK = `${UNPLUGGED} isn't connected, so this tested ${DEFAULT}. Heard you on ${DEFAULT}.`;
const NOT_CONNECTED = "Not connected, so meetings and dictation use the system default until it is plugged back in.";
const DICTATION_REFUSAL = "Not now: a dictation is recording. Test the microphone once it has finished.";
const FAILURE = "The microphone couldn't be tested.";
/** Rust's `CaptureError::NoInputDevice`, as the check would pass it on. */
const NO_DEVICE = "no input device is available. Check that a microphone is connected and that this application is allowed to use it.";
/** The live chip's title for a meeting that opened the headset (`liveMicrophone`). */
const LIVE_TITLE = `This meeting is recording from ${HEADSET}. A microphone chosen now is used from the next recording.`;

const field = (snippet, path) => `(${snippet})?.${path}`;
const SHELL = "Boolean(document.querySelector('nav.rail') && document.querySelector('.card .split-main, .card .content'))";
const LIVE = "Boolean(document.querySelector('.meeting-live'))";
const START_ENABLED = "(() => { const start = document.querySelector('.control-start'); return Boolean(start) && !start.disabled; })()";
const checkState = (cdp) => cdp.evaluate(snippets.microphoneCheckState());
const chipText = (prefix) => field(snippets.chipState(prefix), "text");
const firstLine = field(snippets.microphoneCheckState(), "lines[0]?.text");





async function viewport(cdp, width, height) {
  await cdp.send("Emulation.setDeviceMetricsOverride", { width, height, deviceScaleFactor: 1, mobile: false });
  await delay(150);
}

/** A real pointer press at a measured point, as a person's click is. */


async function escape(cdp) {
  const base = { key: "Escape", code: "Escape", windowsVirtualKeyCode: 27, modifiers: 0 };
  await cdp.send("Input.dispatchKeyEvent", { type: "rawKeyDown", ...base });
  await cdp.send("Input.dispatchKeyEvent", { type: "keyUp", ...base });
  await waitFor(cdp, "!document.querySelector('.chip-menu')", "the menu to close");
}

/** Presses the element's centre, which must be on screen for the press to reach it. */


/** Opens the microphone menu by pressing its chip, and reads it once placed. */
async function openMenu(cdp) {
  await press(cdp, await cdp.evaluate(snippets.chipCenter(CHIP)));
  await waitFor(cdp, snippets.menuVisible(), "the Microphone menu");
  await delay(120);
  return cdp.evaluate(snippets.menuState());
}

/** Opens the menu and chooses the row named `name`. */
async function choose(cdp, name) {
  await openMenu(cdp);
  await press(cdp, await cdp.evaluate(snippets.optionCenter(name)));
}

/** Each page's surface, so a switch waits for the page it went to rather than finding the last one's chip. */
const SURFACE = { Meetings: "meetings.record", Dictation: "dictation.task" };

/** Goes to `page` in the rail and waits for it, and for its microphone menu chip unless `chip` is false (a live meeting has none). */
async function goTo(cdp, page, chip = true) {
  await cdp.evaluate(snippets.clickText(RAIL_ITEM, page));
  await waitFor(cdp, `Boolean(document.querySelector('[data-surface="${SURFACE[page]}"]'))`, `the ${page} page`);
  if (chip) await waitFor(cdp, snippets.chipState(CHIP), `the ${page} microphone chip`);
  await delay(300);
}

/** A fresh load on Meetings with the invoke stub installed, Start enabled. */
async function fresh(cdp) {
  await reload(cdp);
  await waitFor(cdp, SHELL, "the fixture shell");
  await cdp.evaluate(snippets.removeFixtureBanner());
  await cdp.evaluate(snippets.installInvokeStub());
  await goTo(cdp, "Meetings");
  await waitFor(cdp, START_ENABLED, "Start recording to be enabled");
}

/** Presses Test microphone in the chip's menu, and waits for it to listen. */
async function testMicrophone(cdp) {
  await openMenu(cdp);
  await pressText(cdp, ".chip-menu-footer", FOOTER);
  await waitFor(cdp, `${firstLine} === ${JSON.stringify(LISTENING)}`, "the check to listen", 3_000);
}

/** Waits for the check to end with `line`: five seconds and a little. */
async function checkEnds(cdp, line) {
  await waitFor(cdp, `${firstLine} === ${JSON.stringify(line)}`, `the check to end with "${line}"`, 9_000);
}

const placedAboveBar = (menu) => menu.open && menu.box.left >= 0 && menu.box.top >= 0
  && menu.box.right <= menu.viewport.width && menu.box.bottom <= menu.viewport.height
  && menu.box.bottom <= menu.bar.top + 1 && menu.side === "above";

const SETUP_NOTE = "Measured in headless Edge against dev fixtures, not in the Tauri window. Each check starts from a fresh load on Meetings (1000x720 unless it says otherwise) with the page's invoke wrapped to log calls; chips, rows and buttons are pressed with real CDP pointer presses. The fixtures' microphone_check sends fake levels on the microphone-check session for 5 s and always hears you; no audio reaches a browser tab, so a real microphone is not measured here.";

export async function runMeetingMicrophone(options) {
  const out = resolve(options.out);
  await mkdir(out, { recursive: true });
  let browser;
  let cdp;
  try {
    browser = await launchEdge({ ...options, out });
    cdp = await connectCdp(browser.target.webSocketDebuggerUrl);
    await cdp.send("Page.enable");
    await viewport(cdp, 1000, 720);
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
          page = { check: await checkState(cdp), chip: await cdp.evaluate(snippets.chipState(CHIP)), menu: await cdp.evaluate(snippets.menuState()) };
        } catch { /* no page to read */ }
        addCheck(report, { id, claim, expectation, measured: { failedAt: error.message, page }, matches: false, note: notes });
      } finally {
        await viewport(cdp, 1000, 720).catch(() => {});
      }
    };

    await check({
      id: "1",
      claim: "On Meetings the microphone chip opens a listbox of the microphones under one heading saying the choice is shared, with Test microphone at its foot, and its hover says choosing here changes dictation's too.",
      expectation: [
        `the chip named "${CHIP}System default", aria-haspopup listbox, title "${SHARED}";`,
        `its menu: a listbox named Microphone, above the control bar inside the window, with the one heading "${HEADING}" over the rows ${JSON.stringify(ROWS)},`,
        `System default selected, and the footer "${FOOTER}"`,
      ].join(" "),
    }, async () => {
      await fresh(cdp);
      const chip = await cdp.evaluate(snippets.chipState(CHIP));
      const menu = await openMenu(cdp);
      const shot = "01-microphone-menu-1000x720.png";
      await cdp.screenshot(resolve(out, shot));
      await escape(cdp);
      return {
        measured: { chip, menu: { kind: menu.kind, name: menu.name, side: menu.side, box: menu.box, barTop: menu.bar?.top, groups: menu.groups, options: menu.options, footer: menu.footer } },
        matches: chip?.name === `${CHIP}System default` && chip.haspopup === "listbox" && chip.title === SHARED
          && menu.kind === "listbox" && menu.name === "Microphone" && placedAboveBar(menu)
          && JSON.stringify(menu.groups) === JSON.stringify([HEADING])
          && JSON.stringify(menu.options.map((o) => o.name)) === JSON.stringify(ROWS)
          && JSON.stringify(menu.options.filter((o) => o.selected).map((o) => o.name)) === JSON.stringify(["System default"])
          && menu.options.every((o) => !o.greyed) && menu.footer === FOOTER,
        screenshots: [shot],
      };
    });

    await check({
      id: "2",
      claim: "Choosing a microphone on Meetings writes dictation_set_device once, the chip names what was saved, and Dictation's chip names the same microphone.",
      expectation: `after pressing the "${HEADSET}" row: exactly one dictation_set_device call, with device "${HEADSET}"; the Meetings chip reads "${HEADSET}"; then on Dictation, its Microphone chip reads "${HEADSET}"`,
    }, async () => {
      await fresh(cdp);
      await cdp.evaluate(snippets.clearStubCalls());
      await choose(cdp, HEADSET);
      await waitFor(cdp, `${chipText(CHIP)} === ${JSON.stringify(HEADSET)}`, "the Meetings chip to name the headset");
      await delay(300);
      const calls = await cdp.evaluate(snippets.stubCalls("dictation_set_device"));
      const meetings = await cdp.evaluate(snippets.chipState(CHIP));
      await goTo(cdp, "Dictation");
      await waitFor(cdp, `${chipText(CHIP)} === ${JSON.stringify(HEADSET)}`, "the Dictation chip to name the headset", 5_000).catch(() => {});
      const dictation = await cdp.evaluate(snippets.chipState(CHIP));
      return {
        measured: { calls, meetings: meetings?.text ?? null, dictation: dictation?.text ?? null },
        matches: calls.length === 1 && calls[0]?.device === HEADSET
          && meetings?.text === HEADSET && dictation?.text === HEADSET,
      };
    });

    await check({
      id: "3",
      claim: "A live meeting's microphone chip names the microphone the meeting opened, and keeps naming it after dictation_set_device chooses another while it runs.",
      expectation: [
        `with "${HEADSET}" chosen, Start recording: the live view's first chip is a static span reading "${HEADSET}", title "${LIVE_TITLE}";`,
        `then on Dictation, "${LINE_IN}" chosen in its chip (one dictation_set_device call with that device, while the meeting runs) and its chip reads it;`,
        `back on Meetings, which reads the meeting and the microphone again, the live chip still reads "${HEADSET}"`,
      ].join(" "),
    }, async () => {
      await fresh(cdp);
      await choose(cdp, HEADSET);
      await waitFor(cdp, `${chipText(CHIP)} === ${JSON.stringify(HEADSET)}`, "the chip to name the headset");
      await pressText(cdp, ".control-start", "Start recording");
      await waitFor(cdp, LIVE, "the live meeting view");
      await delay(300);
      const before = await cdp.evaluate(snippets.liveChips());
      await cdp.evaluate(snippets.clearStubCalls());
      await goTo(cdp, "Dictation");
      await choose(cdp, LINE_IN);
      await waitFor(cdp, `${chipText(CHIP)} === ${JSON.stringify(LINE_IN)}`, "the Dictation chip to name the line in");
      const calls = await cdp.evaluate(snippets.stubCalls("dictation_set_device"));
      const status = await cdp.evaluate(snippets.invoke("meeting_status"));
      await goTo(cdp, "Meetings", false);
      await waitFor(cdp, LIVE, "the live meeting view again");
      await delay(600);
      const after = await cdp.evaluate(snippets.liveChips());
      const shot = "03-live-chip-after-change-1000x720.png";
      await cdp.screenshot(resolve(out, shot));
      return {
        measured: { before, calls, liveState: status.state, after },
        matches: before[0]?.tag === "span" && before[0].label === HEADSET && before[0].title === LIVE_TITLE
          && calls.length === 1 && calls[0]?.device === LINE_IN && status.state === "recording"
          && after[0]?.tag === "span" && after[0].label === HEADSET && after[0].title === LIVE_TITLE,
        screenshots: [shot],
      };
    });

    await check({
      id: "4",
      claim: "Test microphone listens for five seconds with a meter hidden from assistive technology while Start and Record this call are held and the footer is gone, then says it heard you, and Start is back.",
      expectation: [
        `after Test microphone: a live region (role status) reading "${LISTENING}" with a 12-segment meter beside it, hidden from assistive technology with no role, above 0 within a second;`,
        "Start recording and Record this call disabled; the menu opened again has no footer;",
        `then 4–7 s after the line appeared, the region reads "${HEARD_DEFAULT}" as a note, with no meter, and both buttons enabled again`,
      ].join(" "),
      note: "The fixtures answer after 5 s, as Rust's CHECK_LENGTH; the time is measured from the line appearing, which is a poll after the press, so it reads a little under that.",
    }, async () => {
      await fresh(cdp);
      await waitFor(cdp, `${field(snippets.microphoneCheckState(), "record")} !== null`, "the call card");
      await testMicrophone(cdp);
      const began = Date.now();
      await waitFor(cdp, `${field(snippets.microphoneCheckState(), "meter?.level")} > 0`, "the meter to move", 2_000);
      const listening = await checkState(cdp);
      const shot = "04-listening-1000x720.png";
      await cdp.screenshot(resolve(out, shot));
      const menu = await openMenu(cdp);
      await escape(cdp);
      await checkEnds(cdp, HEARD_DEFAULT);
      const tookMs = Date.now() - began;
      await waitFor(cdp, START_ENABLED, "Start to be enabled again", 2_000);
      const heard = await checkState(cdp);
      const shotHeard = "04-heard-1000x720.png";
      await cdp.screenshot(resolve(out, shotHeard));
      const meter = listening.meter;
      return {
        measured: { listening, menuFooterWhileListening: menu.footer, tookMs, heard },
        matches: listening.role === "status" && listening.lines[0]?.text === LISTENING
          && meter !== null && meter.hiddenFromAssistiveTechnology && meter.role === null && meter.segments === 12 && meter.level > 0
          && listening.start?.disabled === true && listening.record?.disabled === true
          && menu.footer === null
          && tookMs >= 4_000 && tookMs <= 7_000
          && heard.role === "status" && heard.lines.length === 1 && heard.lines[0].text === HEARD_DEFAULT
          && heard.lines[0].className === "control-note" && heard.meter === null
          && heard.start?.disabled === false && heard.record?.disabled === false && heard.failures.length === 0,
        screenshots: [shot, shotHeard],
      };
    });

    await check({
      id: "5",
      claim: "A chosen microphone that isn't connected is named as such with a note that the default is used, and the check says the default stood in for it.",
      expectation: [
        `after dictation_set_device "${UNPLUGGED}" (not one of the fixtures' devices) and the page reading the microphone again:`,
        `the chip reads "${UNPLUGGED} (not connected)" and the note "${NOT_CONNECTED}" is in view;`,
        `Test microphone ends with "${FELL_BACK}"`,
      ].join(" "),
    }, async () => {
      await fresh(cdp);
      await cdp.evaluate(snippets.invokeFixture("dictation_set_device", { device: UNPLUGGED }));
      await goTo(cdp, "Dictation");
      await goTo(cdp, "Meetings");
      await waitFor(cdp, `${chipText(CHIP)} === ${JSON.stringify(`${UNPLUGGED} (not connected)`)}`, "the chip to say not connected");
      const chip = await cdp.evaluate(snippets.chipState(CHIP));
      const notes = await cdp.evaluate(snippets.meetingRecordingNotes());
      await testMicrophone(cdp);
      await checkEnds(cdp, FELL_BACK);
      const ended = await checkState(cdp);
      const shot = "05-fell-back-1000x720.png";
      await cdp.screenshot(resolve(out, shot));
      return {
        measured: { chip: chip?.text ?? null, controlNotes: notes.controlNotes, ended },
        matches: chip?.text === `${UNPLUGGED} (not connected)` && notes.controlNotes.includes(NOT_CONNECTED)
          && ended.lines.length === 1 && ended.lines[0].text === FELL_BACK,
        screenshots: [shot],
      };
    });

    await check({
      id: "6",
      claim: "A refused check and a failed one stay in view, as alerts: the refusal as its own sentence, the failure in plain words with what the software said under Details; neither holds Start.",
      expectation: [
        `with kw.dictationState('recording'): Test microphone shows one failure line, role alert, reading "${DICTATION_REFUSAL}" with no Details, no live region, Start enabled;`,
        `then idle, with microphone_check refused by the stub with Rust's no-device sentence: one failure line reading "${FAILURE}", its Details holding "${NO_DEVICE}", Start enabled`,
      ].join(" "),
      note: "The refusal is the fixtures' own, in Rust's words (`microphone_check.rs`). The failure is the stub's, standing in for a capture that would not open.",
    }, async () => {
      await fresh(cdp);
      await cdp.evaluate(snippets.emitDictationState("recording"));
      await openMenu(cdp);
      await pressText(cdp, ".chip-menu-footer", FOOTER);
      await waitFor(cdp, `${field(snippets.microphoneCheckState(), "failures.length")} === 1`, "the refusal");
      const refused = await checkState(cdp);
      await cdp.evaluate(snippets.emitDictationState("idle"));
      await cdp.evaluate(snippets.stubInvoke({ reject: { microphone_check: NO_DEVICE } }));
      await openMenu(cdp);
      await pressText(cdp, ".chip-menu-footer", FOOTER);
      await waitFor(cdp, `${field(snippets.microphoneCheckState(), "failures[0]?.text")} === ${JSON.stringify(FAILURE)}`, "the failure");
      const failed = await checkState(cdp);
      const shot = "06-failed-1000x720.png";
      await cdp.screenshot(resolve(out, shot));
      await cdp.evaluate(snippets.stubInvoke({ reject: { microphone_check: null } }));
      return {
        measured: { refused, failed },
        matches: refused.failures.length === 1 && refused.failures[0].role === "alert"
          && refused.failures[0].text === DICTATION_REFUSAL && refused.failures[0].details.length === 0
          && refused.role === null && refused.start?.disabled === false
          && failed.failures.length === 1 && failed.failures[0].role === "alert" && failed.failures[0].text === FAILURE
          && failed.failures[0].summary === "Details" && JSON.stringify(failed.failures[0].details) === JSON.stringify([NO_DEVICE])
          && failed.role === null && failed.start?.disabled === false,
        screenshots: [shot],
      };
    });

    await check({
      id: "7",
      claim: "In the smallest window the listening line and its meter sit inside the window, and nothing scrolls sideways.",
      expectation: "after a load at 720x520 and Test microphone: the meter's box inside the window; the document, .split-main and .control-bar no wider than they show (scrollWidth <= clientWidth, as chip-menus check 8 measures)",
    }, async () => {
      await viewport(cdp, 720, 520);
      await fresh(cdp);
      await testMicrophone(cdp);
      await delay(400);
      const [meter] = (await cdp.evaluate(snippets.levelMeters())).filter((item) => item.label === null);
      const scroll = (await cdp.evaluate(snippets.taskPageState())).scroll;
      const shot = "07-listening-720x520.png";
      await cdp.screenshot(resolve(out, shot));
      await checkEnds(cdp, HEARD_DEFAULT);
      const inside = meter !== undefined && meter.box.width > 0 && meter.box.left >= 0 && meter.box.top >= 0
        && meter.box.right <= meter.viewport.width && meter.box.bottom <= meter.viewport.height;
      return {
        measured: { meter: meter ? { box: meter.box, viewport: meter.viewport } : null, scroll },
        matches: inside && scroll.mainClientWidth !== null && scroll.barClientWidth !== null
          && scroll.scrollWidth <= scroll.innerWidth
          && scroll.mainScrollWidth <= scroll.mainClientWidth && scroll.barScrollWidth <= scroll.barClientWidth,
        screenshots: [shot],
      };
    });

    await check({
      id: "8",
      claim: "A check that heard nothing says so as a caveat, naming the microphone and what to do.",
      expectation: `with heard false and peakLevel 0 laid over the fixtures' answer: the region reads "${HEARD_NOTHING}" as a caveat, with no meter, and Start enabled`,
      note: "The stub's merge stands in for a muted microphone; the fixtures' own always hear you.",
    }, async () => {
      await fresh(cdp);
      await cdp.evaluate(snippets.stubInvoke({ merge: { microphone_check: { heard: false, peakLevel: 0 } } }));
      await testMicrophone(cdp);
      await checkEnds(cdp, HEARD_NOTHING);
      const ended = await checkState(cdp);
      const shot = "08-heard-nothing-1000x720.png";
      await cdp.screenshot(resolve(out, shot));
      await cdp.evaluate(snippets.stubInvoke({ merge: { microphone_check: null } }));
      return {
        measured: { ended },
        matches: ended.role === "status" && ended.lines.length === 1 && ended.lines[0].text === HEARD_NOTHING
          && ended.lines[0].className === "caveat" && ended.meter === null && ended.start?.disabled === false,
        screenshots: [shot],
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
    if (options.help) process.stdout.write(`${shellUsage().replace("shell-rail.mjs", "meeting-microphone.mjs")}\n`);
    else {
      const failed = failedChecks(await runMeetingMicrophone(options));
      // The run completed; a check measured false still fails it, so a wrapper or CI step sees it.
      if (failed.length) {
        console.error(`meeting-microphone: ${failed.length} check(s) false: ${failed.join(", ")}`);
        process.exitCode = 1;
      }
    }
  } catch (error) {
    console.error(`meeting-microphone failed: ${error.message}`);
    process.exitCode = 1;
  }
}
