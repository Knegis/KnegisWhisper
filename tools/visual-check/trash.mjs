#!/usr/bin/env node
/**
 * Measures, against dev fixtures, Trash in Notes (KNEG-401): Delete moving a
 * note to Trash without a question, saying so with Undo, and the note gone
 * from the library and from search; Trash listing it with when it went and how
 * long its recording runs, and saying what the recordings take, with Empty
 * trash; a note in Trash opening read-only; Restore bringing it back; Delete
 * forever and Empty trash asking first, in words with no file name in them;
 * Back to a note moved to Trash showing it in Trash; and the read-only note at
 * 720x520.
 *
 * Buttons are pressed with real pointer presses, the Trash row after scrolling
 * the panel to it, since it sits under People below the panel's fold at
 * 1000x720. A library row's menu is opened with a `contextmenu` event, as a
 * right-click opens it. `window.confirm` is scripted (`scriptConfirm`) and
 * keeps every question asked, and `invoke` is wrapped to log what reached
 * Rust's commands. The fixtures answer as Rust's `trash` module does -- a note
 * in Trash left out of every list and search, its recording kept and measured
 * -- but that Rust leaves it out of every query, keeps its file from crash
 * recovery and empties Trash durably is pinned by `cargo test`, not here.
 *
 * Checks 1 to 6 are one sequence from one fresh load, as a person would go
 * through them; 7 and 8 each start from a fresh load.
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
const WINDOW = { width: 1000, height: 720 };
const SMALL = { width: 720, height: 520 };
/**
 * Fixture notes (`apps/desktop/src/lib/devFixtures.ts`). 101 has a 120 s recording made in a
 * meeting, 105 one copied from an imported "genomgang.m4a", 103 and 104 none. 101 is the newest,
 * so it is the one that opens in place of another.
 */
const WEEKLY = "Weekly sync — platform";
const NORDBANK = "Customer call — Nordbank";
const SCRATCH = "Scratch";
const LONG = "A deliberately long title that exists so the notes list can be judged when someone types far more than the column was designed for";
const WORDS = "Word timings";
/** A word only in Scratch's typed notes, for the panel's search. */
const SCRATCH_WORD = "WASAPI";
/** What a 120 s fixture recording takes: 16 kHz mono 16-bit, as the fixtures count it, 3,840,044 bytes. */
const ONE_RECORDING = "3.8 MB";

/** Empties the library's search field through its value setter and an input event, as typing does. */
const CLEAR_SEARCH = "(() => { const input = document.querySelector('.panel-search input'); Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set.call(input, ''); input.dispatchEvent(new Event('input', { bubbles: true })); return true; })()";
const noteOpen = (title) => `(() => { const input = document.querySelector('.note-detail input[aria-label="Title"]'); return Boolean(input) && !input.disabled && input.value === ${JSON.stringify(title)}; })()`;
const TODAY = "new Date().toLocaleDateString(undefined, { day: 'numeric', month: 'short' })";

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

async function viewport(cdp, { width, height }) {
  await cdp.send("Emulation.setDeviceMetricsOverride", { width, height, deviceScaleFactor: 1, mobile: false });
}

/** A real pointer press at the centre of the element matching `selector` and reading `text` (any, when null), which must be on screen. */
async function pressText(cdp, selector, text) {
  const point = await cdp.evaluate(snippets.centerOfText(selector, text));
  const size = await cdp.evaluate("({ width: innerWidth, height: innerHeight })");
  if (point.x < 0 || point.y < 0 || point.x > size.width || point.y > size.height) {
    throw new Error(`${selector} "${text}" is off screen at ${JSON.stringify(point)}`);
  }
  const { x, y } = point;
  await cdp.send("Input.dispatchMouseEvent", { type: "mouseMoved", x, y });
  await cdp.send("Input.dispatchMouseEvent", { type: "mousePressed", x, y, button: "left", clickCount: 1 });
  await cdp.send("Input.dispatchMouseEvent", { type: "mouseReleased", x, y, button: "left", clickCount: 1 });
  await delay(150);
}

const state = (cdp) => cdp.evaluate(snippets.trashState());
const calls = (cdp, command) => cdp.evaluate(snippets.stubCalls(command));
const titles = (rows) => rows.map((row) => row.title);

/** A fresh load at 1000x720 with the shell drawn, the fixture banner gone, the invoke stub installed and Notes open. */
async function freshNotes(cdp) {
  await viewport(cdp, WINDOW);
  await reload(cdp);
  await waitFor(cdp, "Boolean(document.querySelector('nav.rail') && document.querySelector('.card .split-main, .card .content'))", "the fixture shell");
  await cdp.evaluate(snippets.removeFixtureBanner());
  await cdp.evaluate(snippets.installInvokeStub());
  await cdp.evaluate(snippets.clickText(RAIL_ITEM, "Notes"));
  await waitFor(cdp, "Boolean(document.querySelector('.list-panel .notes-entry'))", "the Notes library");
  await waitFor(cdp, noteOpen(WEEKLY), "the newest note to open");
}

/** The library row titled `title`, opened as a person opens it. */
async function openNote(cdp, title) {
  await cdp.evaluate(snippets.openNotesEntry(title));
  await waitFor(cdp, noteOpen(title), `note "${title}" to open`);
  await delay(200);
}

/** Delete from the library row's menu, then wait for the line that says where the note went. */
async function deleteRow(cdp, title) {
  await cdp.evaluate(snippets.openRowMenu(title));
  await waitFor(cdp, "Boolean(document.querySelector('.notes-context-menu'))", "the note's menu");
  await pressText(cdp, ".notes-context-menu > button", "Delete");
  await waitFor(cdp, `document.querySelector('.notes-moved')?.textContent.includes(${JSON.stringify(`“${title}”`)}) ?? false`, `"${title}" to be moved to Trash`);
}

/** The panel's Trash row, scrolled to and pressed. */
async function pressTrash(cdp) {
  if (!(await cdp.evaluate(snippets.scrollToText(".list-panel button.notes-trash .notes-trash-label", "Trash")))) throw new Error("No Trash row");
  await delay(150);
  await pressText(cdp, ".list-panel button.notes-trash .notes-trash-label", "Trash");
}

/** The Trash list's row titled `title`, pressed. */
async function openTrashed(cdp, title) {
  await pressText(cdp, ".list-panel .notes-trashed .notes-entry-title", title);
  await waitFor(cdp, `document.querySelector('.trashed-note .trashed-note-title')?.textContent === ${JSON.stringify(title)}`, `"${title}" to open in Trash`);
}

const SETUP_NOTE = "1000x720 unless stated. Checks 1-6 run in sequence from one fresh load; 7 and 8 each start from a fresh load. Buttons are pressed with real CDP pointer presses, the Trash row after scrolling it into view; a library row's menu is opened with a contextmenu event; window.confirm is scripted and keeps each question; invoke is wrapped to log notes_trash, notes_restore, notes_delete_forever, notes_empty_trash and notes_update.";

export async function runTrash(options) {
  const out = resolve(options.out);
  await mkdir(out, { recursive: true });
  let browser;
  let cdp;
  try {
    browser = await launchEdge({ ...options, out });
    cdp = await connectCdp(browser.target.webSocketDebuggerUrl);
    await cdp.send("Page.enable");
    const report = createReport({
      url: options.url,
      edgeVersion: browser.version.Browser ?? browser.version,
      fixtureBannerRemoved: true,
      timestamp: new Date().toISOString(),
      measurementScope: "measured in headless Edge against dev fixtures, not in the Tauri window",
    });

    /** Runs one check; a step that throws is the check measured false, with the Trash state as it was. */
    const check = async ({ id, claim, expectation }, run) => {
      try {
        const result = await run();
        addCheck(report, { id, claim, expectation, note: SETUP_NOTE, ...result });
      } catch (error) {
        let trash = null;
        try { trash = await state(cdp); } catch { /* no page to read */ }
        addCheck(report, { id, claim, expectation, measured: { failedAt: error.message, trash }, matches: false, note: SETUP_NOTE });
      }
    };

    await check({
      id: "1",
      claim: "Delete moves a note to Trash without a question, says so with Undo, and the note is gone from the library and from search.",
      expectation: [
        `"${SCRATCH}" open; Delete in its menu: no question asked; notes_trash sent { id: 103 };`,
        `"Moved “${SCRATCH}” to Trash." with an Undo button; "${SCRATCH}" not in the library; "${WEEKLY}" open in its place;`,
        `Trash shows 1; the library's search for "${SCRATCH_WORD}" says nothing matches`,
      ].join(" "),
    }, async () => {
      await freshNotes(cdp);
      await openNote(cdp, SCRATCH);
      await cdp.evaluate(snippets.scriptConfirm(false));
      await cdp.evaluate(snippets.clearStubCalls());
      await deleteRow(cdp, SCRATCH);
      await waitFor(cdp, noteOpen(WEEKLY), "the newest note to open in its place");
      const after = await state(cdp);
      const questions = await cdp.evaluate(snippets.confirmQuestions());
      const sent = await calls(cdp, "notes_trash");
      await pressText(cdp, ".panel-search input", null);
      await cdp.send("Input.insertText", { text: SCRATCH_WORD });
      await waitFor(cdp, "[...document.querySelectorAll('.list-panel .panel-note')].some((note) => note.textContent.startsWith('Nothing matches'))", "the search to answer");
      const searched = (await state(cdp)).panelNotes;
      const shot = "01-moved-to-trash-1000x720.png";
      await cdp.screenshot(resolve(out, shot));
      // The search emptied, as a person clears the field, and the library back for the checks that follow.
      await cdp.evaluate(CLEAR_SEARCH);
      await waitFor(cdp, "document.querySelectorAll('.list-panel .notes-entry').length > 0", "the library to come back");
      return {
        measured: { after, questions, sent, searched },
        matches: questions.length === 0
          && JSON.stringify(sent) === JSON.stringify([{ id: 103 }])
          && after.moved?.text === `Moved “${SCRATCH}” to Trash.` && JSON.stringify(after.moved?.undo) === JSON.stringify(["Undo"])
          && !after.library.includes(SCRATCH) && after.title === WEEKLY
          && after.entry?.count === 1
          && searched.includes(`Nothing matches "${SCRATCH_WORD}"`),
        screenshots: [shot],
      };
    });

    await check({
      id: "2",
      claim: "Undo brings the note back to the library and opens it again.",
      expectation: `the line from check 1 still above "${WEEKLY}"; press its Undo: notes_restore sent { id: 103 }; "${SCRATCH}" open and in the library; the line gone; Trash shows 0`,
    }, async () => {
      if ((await state(cdp)).moved?.text !== `Moved “${SCRATCH}” to Trash.`) throw new Error("The line from check 1 is gone");
      await cdp.evaluate(snippets.clearStubCalls());
      await pressText(cdp, ".notes-moved button", "Undo");
      await waitFor(cdp, noteOpen(SCRATCH), `"${SCRATCH}" to open again`);
      const after = await state(cdp);
      const sent = await calls(cdp, "notes_restore");
      return {
        measured: { after, sent },
        matches: JSON.stringify(sent) === JSON.stringify([{ id: 103 }])
          && after.title === SCRATCH && after.library.includes(SCRATCH)
          && after.moved === null && after.entry?.count === 0,
      };
    });

    let today = null;
    await check({
      id: "3",
      claim: "Trash lists a deleted note with when it went and how long its recording runs, says what the recordings take, and opens the note read-only.",
      expectation: [
        `delete "${WEEKLY}" (a 120 s recording), press Trash: the panel's "Trash" section holds "${WEEKLY}" with "Deleted {today} · 2 min" and Show all notes, the Trash row pressed;`,
        `the main area says "Recordings in Trash take ${ONE_RECORDING} on this computer." with Empty trash enabled;`,
        `press "${WEEKLY}" there: "This note is in Trash." with Restore and Delete forever, its title, no field to type in and no workspace; no notes_update in the next second`,
      ].join(" "),
    }, async () => {
      today = await cdp.evaluate(TODAY);
      await openNote(cdp, WEEKLY);
      await deleteRow(cdp, WEEKLY);
      await pressTrash(cdp);
      await waitFor(cdp, "document.querySelectorAll('.list-panel .notes-trashed').length > 0", "the Trash list");
      const listed = await state(cdp);
      await cdp.evaluate(snippets.clearStubCalls());
      await openTrashed(cdp, WEEKLY);
      await delay(1000);
      const opened = await state(cdp);
      const saves = await calls(cdp, "notes_update");
      const shot = "03-note-in-trash-1000x720.png";
      await cdp.screenshot(resolve(out, shot));
      return {
        measured: { today, listed, opened, saves },
        matches: listed.entry?.pressed === true && listed.sections.includes("Trash")
          && JSON.stringify(listed.trashed.map(({ title, meta }) => ({ title, meta }))) === JSON.stringify([{ title: WEEKLY, meta: `Deleted ${today} · 2 min` }])
          && listed.space?.lines[0] === `Recordings in Trash take ${ONE_RECORDING} on this computer.`
          && listed.space?.emptyTrash?.text === "Empty trash" && listed.space.emptyTrash.disabled === false
          && opened.view?.banner === "This note is in Trash."
          && JSON.stringify(opened.view?.buttons) === JSON.stringify(["Restore", "Delete forever"])
          && opened.view?.title === WEEKLY && opened.view.inputs === 0
          && opened.workspace === false && opened.trashed[0]?.current === true
          && saves.length === 0,
        screenshots: [shot],
      };
    });

    await check({
      id: "4",
      claim: "Restore brings the note back into the library, in its folder, open as an ordinary note.",
      expectation: `press Restore: notes_restore sent { id: 101 }; Trash closed and showing 0; "${WEEKLY}" in the library and open in a workspace with its title field`,
    }, async () => {
      await cdp.evaluate(snippets.clearStubCalls());
      await pressText(cdp, ".trashed-note-banner button", "Restore");
      await waitFor(cdp, noteOpen(WEEKLY), `"${WEEKLY}" to open in a workspace`);
      const after = await state(cdp);
      const folders = await cdp.evaluate(snippets.notesFolders());
      const sent = await calls(cdp, "notes_restore");
      return {
        measured: { after, folders, sent },
        matches: JSON.stringify(sent) === JSON.stringify([{ id: 101 }])
          && after.entry?.pressed === false && after.entry.count === 0
          && after.view === null && after.space === null && after.workspace === true
          && Object.values(folders.folders).some((filed) => filed.includes(WEEKLY)),
      };
    });

    await check({
      id: "5",
      claim: "Delete forever asks first, in words with the note's title and its recording's length and no file name, and only a yes deletes it.",
      expectation: [
        `delete "${WEEKLY}" again, open it from Trash, Delete forever answered No: the question is exactly`,
        `"Delete “${WEEKLY}” forever? Its recording (2 min) is deleted with it. This can't be undone." -- no ".wav", no stored name -- and nothing is sent;`,
        "answered Yes: notes_delete_forever sent { id: 101 }; notes_get gives null; Trash empty, its page saying \"Trash is empty.\" with Empty trash disabled",
      ].join(" "),
    }, async () => {
      await deleteRow(cdp, WEEKLY);
      await pressTrash(cdp);
      await waitFor(cdp, "document.querySelectorAll('.list-panel .notes-trashed').length > 0", "the Trash list");
      await openTrashed(cdp, WEEKLY);
      await cdp.evaluate(snippets.clearStubCalls());
      await cdp.evaluate(snippets.scriptConfirm(false));
      await pressText(cdp, ".trashed-note-banner button", "Delete forever");
      await delay(400);
      const declined = { questions: await cdp.evaluate(snippets.confirmQuestions()), sent: await calls(cdp, "notes_delete_forever"), trash: await state(cdp) };
      await cdp.evaluate(snippets.scriptConfirm(true));
      await pressText(cdp, ".trashed-note-banner button", "Delete forever");
      await waitFor(cdp, "document.querySelectorAll('.list-panel .notes-trashed').length === 0", "Trash to empty");
      const questions = await cdp.evaluate(snippets.confirmQuestions());
      const sent = await calls(cdp, "notes_delete_forever");
      const stored = await cdp.evaluate(snippets.invoke("notes_get", { id: 101 }));
      const after = await state(cdp);
      const expected = `Delete “${WEEKLY}” forever? Its recording (2 min) is deleted with it. This can't be undone.`;
      return {
        measured: { declined, questions, sent, stored, after },
        matches: JSON.stringify(declined.questions) === JSON.stringify([expected]) && declined.sent.length === 0
          && declined.trash.view?.title === WEEKLY
          && JSON.stringify(questions) === JSON.stringify([expected]) && !/\.wav|20260901-101500/.test(questions.join(" "))
          && JSON.stringify(sent) === JSON.stringify([{ id: 101 }]) && stored === null
          && after.entry?.count === 0 && after.view === null
          && after.space?.lines[0] === "Trash is empty." && after.space.emptyTrash?.disabled === true,
      };
    });

    await check({
      id: "6",
      claim: "Empty trash asks first, with how many notes and what their recordings take, and deletes every note in Trash.",
      expectation: [
        `Show all notes, delete "${WORDS}" (a recording copied from an import) and the long-titled note (none), press Trash, then Empty trash answered Yes:`,
        `the question is exactly "Delete 2 notes forever? Its recording (${ONE_RECORDING}) is deleted with it. Your original file isn't touched. This can't be undone.";`,
        "notes_empty_trash sent once; Trash empty and showing 0; notes_get gives null for 105 and 104",
      ].join(" "),
    }, async () => {
      await pressText(cdp, ".list-panel .panel-action", "Show all notes");
      await waitFor(cdp, "document.querySelectorAll('.list-panel .notes-entry').length > 0", "the library");
      await deleteRow(cdp, WORDS);
      // A second later, so the two are told apart by when they went, as Rust's second-long stamps tell them.
      await delay(1100);
      await deleteRow(cdp, LONG);
      await pressTrash(cdp);
      await waitFor(cdp, "document.querySelectorAll('.list-panel .notes-trashed').length === 2", "the Trash list");
      const listed = await state(cdp);
      await cdp.evaluate(snippets.clearStubCalls());
      await cdp.evaluate(snippets.scriptConfirm(true));
      await pressText(cdp, ".trash-space button", "Empty trash");
      await waitFor(cdp, "document.querySelectorAll('.list-panel .notes-trashed').length === 0", "Trash to empty");
      const questions = await cdp.evaluate(snippets.confirmQuestions());
      const sent = await calls(cdp, "notes_empty_trash");
      const stored = [await cdp.evaluate(snippets.invoke("notes_get", { id: 105 })), await cdp.evaluate(snippets.invoke("notes_get", { id: 104 }))];
      const after = await state(cdp);
      const shot = "06-trash-emptied-1000x720.png";
      await cdp.screenshot(resolve(out, shot));
      return {
        measured: { listed, questions, sent, stored, after },
        matches: JSON.stringify(titles(listed.trashed)) === JSON.stringify([LONG, WORDS])
          && listed.space?.lines[0] === `Recordings in Trash take ${ONE_RECORDING} on this computer.`
          && JSON.stringify(questions) === JSON.stringify([`Delete 2 notes forever? Its recording (${ONE_RECORDING}) is deleted with it. Your original file isn't touched. This can't be undone.`])
          && !/genomgang|\.wav|\.m4a/.test(questions.join(" "))
          && sent.length === 1 && stored.every((note) => note === null)
          && after.entry?.count === 0 && after.space?.lines[0] === "Trash is empty.",
        screenshots: [shot],
      };
    });

    await check({
      id: "7",
      claim: "Back to a note moved to Trash since shows it in Trash, read-only, and nothing saves into it.",
      expectation: [
        `fresh load; open "${NORDBANK}", then "${SCRATCH}"; delete "${NORDBANK}" from its library row; press Go back:`,
        `"${NORDBANK}" shows "This note is in Trash." with Restore and Delete forever, no field to type in and no workspace, Trash not pressed; no notes_update in the next second`,
      ].join(" "),
    }, async () => {
      await freshNotes(cdp);
      await openNote(cdp, NORDBANK);
      await openNote(cdp, SCRATCH);
      await deleteRow(cdp, NORDBANK);
      await cdp.evaluate(snippets.clearStubCalls());
      await pressText(cdp, ".app-titlebar button[aria-label=\"Go back\"]", null);
      await waitFor(cdp, `document.querySelector('.trashed-note .trashed-note-title')?.textContent === ${JSON.stringify(NORDBANK)}`, "the note in Trash");
      await delay(1000);
      const after = await state(cdp);
      const saves = await calls(cdp, "notes_update");
      const shot = "07-back-to-trashed-1000x720.png";
      await cdp.screenshot(resolve(out, shot));
      return {
        measured: { after, saves },
        matches: after.view?.banner === "This note is in Trash."
          && JSON.stringify(after.view?.buttons) === JSON.stringify(["Restore", "Delete forever"])
          && after.view?.inputs === 0 && after.workspace === false
          && after.entry?.pressed === false && after.entry.count === 1
          && saves.length === 0,
        screenshots: [shot],
      };
    });

    await check({
      id: "8",
      claim: "At 720x520 a note in Trash and its Restore and Delete forever stay inside the window.",
      expectation: `fresh load; delete "${SCRATCH}", press Trash and open it, then 720x520: the banner's buttons and the title inside the window, nothing scrolling sideways`,
    }, async () => {
      await freshNotes(cdp);
      await openNote(cdp, SCRATCH);
      await deleteRow(cdp, SCRATCH);
      await pressTrash(cdp);
      await waitFor(cdp, "document.querySelectorAll('.list-panel .notes-trashed').length > 0", "the Trash list");
      await openTrashed(cdp, SCRATCH);
      await viewport(cdp, SMALL);
      await delay(500);
      const after = await state(cdp);
      const boxes = await cdp.evaluate(`(() => [...document.querySelectorAll('.trashed-note-banner button, .trashed-note-title, .trash-space button')].map((item) => { const box = item.getBoundingClientRect(); return { text: item.textContent.trim().slice(0, 40), left: Math.round(box.left), right: Math.round(box.right) }; }))()`);
      const shot = "08-note-in-trash-720x520.png";
      await cdp.screenshot(resolve(out, shot));
      return {
        measured: { after, boxes },
        matches: after.view?.title === SCRATCH && boxes.length === 4
          && boxes.every((box) => box.left >= 0 && box.right <= after.innerWidth)
          && after.scrollWidth <= after.innerWidth,
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
    if (options.help) process.stdout.write(`${shellUsage().replace("shell-rail.mjs", "trash.mjs")}\n`);
    else {
      const failed = failedChecks(await runTrash(options));
      // The run completed; a check measured false still fails it, so a wrapper or CI step sees it.
      if (failed.length) {
        console.error(`trash: ${failed.length} check(s) false: ${failed.join(", ")}`);
        process.exitCode = 1;
      }
    }
  } catch (error) {
    console.error(`trash failed: ${error.message}`);
    process.exitCode = 1;
  }
}
