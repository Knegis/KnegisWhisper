#!/usr/bin/env node
/**
 * Measures, against dev fixtures, naming a meeting and choosing its folder
 * before it is recorded (KNEG-365): that Start and Record this call hand both
 * to `meeting_start`, that the live view and the list panel's Now row show the
 * name, that the saved note has the name and sits in that folder in Notes, and
 * that a folder deleted while the meeting ran is said in the outcome.
 *
 * The name is typed with CDP's `Input.insertText` into the focused field, and
 * the folder chosen by pressing the Folder chip and its row, as a person
 * would. The page's `invoke` is wrapped (`installInvokeStub`) to log what
 * reached `meeting_start`. The fixtures keep the plan on `meeting_status`
 * while the meeting runs, as Rust keeps it with the session, and save a new
 * note at Stop as Rust's `keep` does. Each check starts from a fresh load at
 * 1000x720.
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
const ACTION = ".live-actions button";
/** The name typed before Start. */
const NAME = "Board review";
/** Fixture folder 3 (`apps/desktop/src/lib/devFixtures.ts`). */
const FOLDER = { id: 3, name: "Internal" };
/** The Folder chip's name prefix: `ChipMenu` names a chip "<name>: <label>". */
const FOLDER_CHIP = "Folder: ";
/** What the name field says and is called (`views/Meetings.tsx`). */
const PLACEHOLDER = "Name this meeting (optional)";
const FIELD_LABEL = "Meeting name (optional)";
/** The outcome's sentence for a folder deleted while the meeting ran (`meetingOutcomeLabel`). */
const FOLDER_GONE = "The folder you chose was deleted, so the note isn't in a folder.";
/** Rust's `note_title` for a meeting given no name. */
const UNNAMED = /^Meeting — \d{4}-\d{2}-\d{2} \d{2}:\d{2}$/;

const field = (snippet, path) => `(${snippet})?.${path}`;
const LIVE = "Boolean(document.querySelector('.meeting-live'))";
const IDLE = "Boolean(document.querySelector('.control-start')) && !document.querySelector('.meeting-live')";
const START_ENABLED = "(() => { const start = document.querySelector('.control-start'); return Boolean(start) && !start.disabled; })()";
const RECORD_ENABLED = "(() => { const button = [...document.querySelectorAll('.call-card button')].find((item) => item.textContent.trim() === 'Record this call'); return Boolean(button) && !button.disabled; })()";

async function waitFor(cdp, expression, description, timeout = 10_000) {
  const deadline = Date.now() + timeout;
  while (Date.now() < deadline) {
    try {
      if (await cdp.evaluate(expression)) return;
    } catch {
      // Between two documents during a reload there is no context to evaluate in.
    }
    await delay(60);
  }
  throw new Error(`Timed out waiting for ${description}`);
}

async function reload(cdp, timeout = 15_000) {
  let off = () => {};
  let timer;
  const loaded = new Promise((resolveLoad, reject) => {
    timer = setTimeout(() => reject(new Error("Timed out waiting for the reload")), timeout);
    off = cdp.on("Page.loadEventFired", () => resolveLoad());
  });
  try {
    await cdp.send("Page.reload", { ignoreCache: true });
    await loaded;
  } finally {
    clearTimeout(timer);
    off();
  }
}

/** A real pointer press at a measured point, as a person's click is. */
async function press(cdp, { x, y }) {
  await cdp.send("Input.dispatchMouseEvent", { type: "mouseMoved", x, y });
  await cdp.send("Input.dispatchMouseEvent", { type: "mousePressed", x, y, button: "left", clickCount: 1 });
  await cdp.send("Input.dispatchMouseEvent", { type: "mouseReleased", x, y, button: "left", clickCount: 1 });
  await delay(150);
}

/** Presses the element's centre, which must be on screen for the press to reach it. */
async function pressText(cdp, selector, text) {
  const point = await cdp.evaluate(snippets.centerOfText(selector, text));
  const size = await cdp.evaluate("({ width: innerWidth, height: innerHeight })");
  if (point.x < 0 || point.y < 0 || point.x > size.width || point.y > size.height) {
    throw new Error(`${selector} "${text}" is off screen at ${JSON.stringify(point)}`);
  }
  await press(cdp, point);
}

const planState = (cdp) => cdp.evaluate(snippets.meetingPlanState());
const pageState = (cdp) => cdp.evaluate(snippets.meetingPageState());
const startCalls = (cdp) => cdp.evaluate(snippets.stubCalls("meeting_start"));

/**
 * A fresh load at 1000x720 on Meetings with the invoke stub installed, the
 * name typed into its field and the folder chosen in the Folder chip --
 * either left out when `null`.
 */
async function prepared(cdp, { name, folder }) {
  await reload(cdp);
  await waitFor(cdp, "Boolean(document.querySelector('nav.rail') && document.querySelector('.card .split-main, .card .content'))", "the fixture shell");
  await cdp.evaluate(snippets.removeFixtureBanner());
  await cdp.evaluate(snippets.installInvokeStub());
  await cdp.evaluate(snippets.clickText(RAIL_ITEM, "Meetings"));
  await waitFor(cdp, START_ENABLED, "Start recording to be enabled");
  await waitFor(cdp, snippets.chipState(FOLDER_CHIP), "the Folder chip");
  if (name !== null) {
    await press(cdp, await cdp.evaluate(snippets.centerOfText(".control-row input.meeting-name", null)));
    await cdp.send("Input.insertText", { text: name });
    await waitFor(cdp, `${field(snippets.meetingPlanState(), "name.value")} === ${JSON.stringify(name)}`, "the name to be typed");
  }
  if (folder !== null) {
    await press(cdp, await cdp.evaluate(snippets.chipCenter(FOLDER_CHIP)));
    await waitFor(cdp, snippets.menuVisible(), "the Folder menu");
    await press(cdp, await cdp.evaluate(snippets.optionCenter(folder)));
    await waitFor(cdp, `${field(snippets.chipState(FOLDER_CHIP), "text")} === ${JSON.stringify(`Save to ${folder}`)}`, "the Folder chip to name the folder");
  }
  await cdp.evaluate(snippets.clearStubCalls());
}

/** Starts the meeting by Start recording (`"start"`) or the call card's Record this call (`"record"`), and waits for the live view. */
async function begin(cdp, how) {
  if (how === "record") {
    await waitFor(cdp, RECORD_ENABLED, "Record this call to be enabled");
    await pressText(cdp, ".call-card button", "Record this call");
  } else {
    await pressText(cdp, ".control-start", "Start recording");
  }
  await waitFor(cdp, LIVE, "the live meeting view");
  await delay(300);
}

/** Stop and save, with the fixtures answering for the meeting window, then the page idle. */
async function stopAndSave(cdp) {
  await pressText(cdp, ACTION, "Stop and save");
  await waitFor(cdp, IDLE, "the page to go idle", 10_000);
  await delay(400);
}

/** Opens Notes from the rail and reads its library's folders. */
async function notesLibrary(cdp) {
  await cdp.evaluate(snippets.clickText(RAIL_ITEM, "Notes"));
  await waitFor(cdp, "Boolean(document.querySelector('.list-panel .notes-entry'))", "the Notes library");
  await delay(300);
  return cdp.evaluate(snippets.notesFolders());
}

const planOf = (call) => ({ title: call?.title ?? null, folderId: call?.folderId ?? null });

const SETUP_NOTE = "Each check starts from a fresh load at 1000x720 on Meetings. The name is typed with CDP Input.insertText into the focused field and the folder chosen by pressing the Folder chip and its row; buttons are pressed with real CDP pointer presses. The fixtures keep the plan on meeting_status while the meeting runs and save a new note at Stop, as Rust does; Rust's own saving is covered by its tests, not here.";

export async function runMeetingPlan(options) {
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
        let plan = null;
        try {
          page = await pageState(cdp);
          plan = await planState(cdp);
        } catch { /* no page to read */ }
        addCheck(report, { id, claim, expectation, measured: { failedAt: error.message, page, plan }, matches: false, note: notes });
      }
    };

    await check({
      id: "1",
      claim: "The name typed and the folder chosen before recording reach meeting_start, from Start recording and from Record this call alike.",
      expectation: [
        `the name field reads "${PLACEHOLDER}" and is named "${FIELD_LABEL}";`,
        `with "${NAME}" typed and ${FOLDER.name} (fixture folder ${FOLDER.id}) chosen, the Folder chip reads "Save to ${FOLDER.name}",`,
        `and one meeting_start call carries title "${NAME}" and folderId ${FOLDER.id} -- for Start recording, and again on a fresh page for Record this call`,
      ].join(" "),
    }, async () => {
      const measured = {};
      for (const how of ["start", "record"]) {
        await prepared(cdp, { name: NAME, folder: FOLDER.name });
        const before = { plan: await planState(cdp), chip: await cdp.evaluate(snippets.chipState(FOLDER_CHIP)) };
        if (how === "start") await cdp.screenshot(resolve(out, "01-named-before-start-1000x720.png"));
        await begin(cdp, how);
        measured[how] = { before, calls: (await startCalls(cdp)).map(planOf) };
      }
      const sent = (result) => result.calls.length === 1
        && result.calls[0].title === NAME && result.calls[0].folderId === FOLDER.id;
      return {
        measured,
        matches: ["start", "record"].every((how) => sent(measured[how])
          && measured[how].before.chip?.text === `Save to ${FOLDER.name}`
          && measured[how].before.chip?.name === `Folder: Save to ${FOLDER.name}`
          && measured[how].before.plan.name?.placeholder === PLACEHOLDER
          && measured[how].before.plan.name?.label === FIELD_LABEL),
        screenshots: ["01-named-before-start-1000x720.png"],
      };
    });

    await check({
      id: "2",
      claim: "While the meeting records, the live view's title and the list panel's Now row are its name.",
      expectation: `after Start with "${NAME}" typed: .live-title reads "${NAME}" and the Now section's one row reads "${NAME}"`,
    }, async () => {
      await prepared(cdp, { name: NAME, folder: FOLDER.name });
      await begin(cdp, "start");
      const plan = await planState(cdp);
      const shot = "02-live-title-1000x720.png";
      await cdp.screenshot(resolve(out, shot));
      return {
        measured: { liveTitle: plan.liveTitle, now: plan.sections.Now ?? null },
        matches: plan.liveTitle === NAME && JSON.stringify(plan.sections.Now) === JSON.stringify([NAME]),
        screenshots: [shot],
      };
    });

    await check({
      id: "3",
      claim: "Stop saves the meeting under its name in the folder chosen: Recent lists it by name, Notes files it in that folder, and the name field is empty for the next meeting while the folder stays chosen.",
      expectation: [
        `after Start and Stop and save: the page idle with a caveat starting "Saved as a note" and no "${FOLDER_GONE}";`,
        `Recent's first row "${NAME}"; the name field empty and the Folder chip still "Save to ${FOLDER.name}";`,
        `then in Notes, the ${FOLDER.name} folder lists "${NAME}" and "Not in a folder" does not`,
      ].join(" "),
    }, async () => {
      await prepared(cdp, { name: NAME, folder: FOLDER.name });
      await begin(cdp, "start");
      await stopAndSave(cdp);
      const page = await pageState(cdp);
      const plan = await planState(cdp);
      const chip = await cdp.evaluate(snippets.chipState(FOLDER_CHIP));
      const shot = "03-saved-1000x720.png";
      await cdp.screenshot(resolve(out, shot));
      const library = await notesLibrary(cdp);
      return {
        measured: { caveats: page.caveats, recent: plan.sections.Recent ?? null, name: plan.name, chip: chip?.text ?? null, library },
        matches: page.caveats.some((text) => text.startsWith("Saved as a note"))
          && !page.caveats.some((text) => text.includes(FOLDER_GONE))
          && plan.sections.Recent?.[0] === NAME
          && plan.name?.value === "" && chip?.text === `Save to ${FOLDER.name}`
          && (library.folders[FOLDER.name] ?? []).includes(NAME) && !library.unfiled.includes(NAME),
        screenshots: [shot],
      };
    });

    await check({
      id: "4",
      claim: "A folder deleted while the meeting records is left out of the save, and the outcome says so.",
      expectation: [
        `after Start with "${NAME}" in ${FOLDER.name}, folder_delete ${FOLDER.id} through the fixtures, then Stop and save:`,
        `a caveat starting "Saved as a note (31 min). ${FOLDER_GONE}"; the Folder chip back at "Save to Notes";`,
        `in Notes, "${NAME}" under "Not in a folder"`,
      ].join(" "),
      note: "The fixtures' meeting_stop makes Rust's call on the folder (`create_meeting_note`): a folder no longer listed is no folder, with folderMissing. The race Rust also covers -- deleted between its lookup and its insert -- is a Rust test.",
    }, async () => {
      await prepared(cdp, { name: NAME, folder: FOLDER.name });
      await begin(cdp, "start");
      await cdp.evaluate(snippets.invokeFixture("folder_delete", { id: FOLDER.id }));
      await stopAndSave(cdp);
      const page = await pageState(cdp);
      const chip = await cdp.evaluate(snippets.chipState(FOLDER_CHIP));
      const shot = "04-folder-deleted-1000x720.png";
      await cdp.screenshot(resolve(out, shot));
      const library = await notesLibrary(cdp);
      return {
        measured: { caveats: page.caveats, chip: chip?.text ?? null, library },
        matches: page.caveats.some((text) => text.startsWith(`Saved as a note (31 min). ${FOLDER_GONE}`))
          && chip?.text === "Save to Notes"
          && library.unfiled.includes(NAME) && !(FOLDER.name in library.folders),
        screenshots: [shot],
      };
    });

    await check({
      id: "5",
      claim: "A meeting started with no name and no folder chosen asks for neither, reads Meeting while it records, and is saved named for when it started, in no folder.",
      expectation: [
        "with the field left empty and the Folder chip at \"Save to Notes\": one meeting_start call with title null and folderId null;",
        ".live-title reads \"Meeting\"; after Stop and save, Recent's first row matches \"Meeting — YYYY-MM-DD HH:MM\" and Notes lists it under \"Not in a folder\"",
      ].join(" "),
    }, async () => {
      await prepared(cdp, { name: null, folder: null });
      const chip = await cdp.evaluate(snippets.chipState(FOLDER_CHIP));
      await begin(cdp, "start");
      const calls = (await startCalls(cdp)).map(planOf);
      const live = await planState(cdp);
      await stopAndSave(cdp);
      const saved = await planState(cdp);
      const title = saved.sections.Recent?.[0] ?? null;
      const library = await notesLibrary(cdp);
      return {
        measured: { chip: chip?.text ?? null, calls, liveTitle: live.liveTitle, recentFirst: title, library },
        matches: chip?.text === "Save to Notes"
          && calls.length === 1 && calls[0].title === null && calls[0].folderId === null
          && live.liveTitle === "Meeting"
          && title !== null && UNNAMED.test(title) && library.unfiled.includes(title),
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
    if (options.help) process.stdout.write(`${shellUsage().replace("shell-rail.mjs", "meeting-plan.mjs")}\n`);
    else {
      const failed = failedChecks(await runMeetingPlan(options));
      // The run completed; a check measured false still fails it, so a wrapper or CI step sees it.
      if (failed.length) {
        console.error(`meeting-plan: ${failed.length} check(s) false: ${failed.join(", ")}`);
        process.exitCode = 1;
      }
    }
  } catch (error) {
    console.error(`meeting-plan failed: ${error.message}`);
    process.exitCode = 1;
  }
}
