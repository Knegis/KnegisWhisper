#!/usr/bin/env node
/**
 * Runs repeatable title-bar, Settings-sheet and list-panel measurements against
 * dev fixtures: the four pills and the one that reads as current, the Settings
 * sheet and its Developer section, the list panel's own toggle, the 720x520
 * minimum window, keyboard focus, Ctrl+K, and Notes: the transcript column a
 * wide window gives a note, the 720x520 fit of a note beside its library, and
 * the note's button that hides and shows that column; and whether any screen
 * scrolls sideways at 720 or 1000 wide.
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

/** A screenshot clip for a measured rectangle, in page coordinates. */
export function clipRect({ rect, scrollX = 0, scrollY = 0 }) {
  return {
    x: rect.left + scrollX,
    y: rect.top + scrollY,
    width: rect.width,
    height: rect.height,
  };
}

const TOGGLE = 'button.list-toggle[aria-controls="list-panel"]';
const NAV_PILL = snippets.NAV_PILL;
const BACK_TO_SHEET = ".page-back button.back-to-settings";
// Every screen a pill or a tile reaches, in `lib/navigation.ts`' order.
const PAGES = ["Dictation", "Meetings", "Notes"];
const TILES = ["Shortcuts", "Meetings", "Vocabulary", "Transcription", "Cleanup & summaries", "Local text models", "Providers", "Calendar", "Local API", "CLI", "MCP", "Diagnostics"];
// Fixture note 104 (`apps/desktop/src/lib/devFixtures.ts`), whose title is there to be too long.
const LONG_TITLE = "A deliberately long title that exists so the notes list can be judged when someone types far more than the column was designed for";
const NOTE_OPEN = "Boolean(document.querySelector('.note-detail textarea[aria-label=\"Title\"]:enabled'))";

const screenshot = async (cdp, out, name, options) => {
  await cdp.screenshot(resolve(out, name), options);
  return name;
};

async function waitFor(cdp, expression, description, timeout = 8_000) {
  const deadline = Date.now() + timeout;
  while (Date.now() < deadline) {
    try {
      if (await cdp.evaluate(expression)) return;
    } catch {
      // Between two documents during a reload there is no context to evaluate in.
    }
    await delay(100);
  }
  throw new Error(`Timed out waiting for ${description}`);
}

/** Reloads the page and resolves once the new document has loaded. */
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

async function click(cdp, selector, text) {
  const result = await cdp.evaluate(snippets.clickText(selector, text));
  await delay(100);
  return result;
}

async function viewport(cdp, width, height) {
  await cdp.send("Emulation.setDeviceMetricsOverride", {
    width,
    height,
    deviceScaleFactor: 1,
    mobile: false,
  });
  await delay(150);
}

async function key(cdp, { key: name, code, keyCode, modifiers = 0 }) {
  await cdp.send("Input.dispatchKeyEvent", {
    type: "rawKeyDown",
    key: name,
    code,
    windowsVirtualKeyCode: keyCode,
    modifiers,
  });
  await cdp.send("Input.dispatchKeyEvent", {
    type: "keyUp",
    key: name,
    code,
    windowsVirtualKeyCode: keyCode,
    modifiers,
  });
}

/**
 * Waits for the shell, then removes the fixture banner unless asked not to.
 *
 * "A page" is `.split-main` or `.content`: the title bar's pages draw their own
 * `.split-main` with no `.content`; the settings screens and Notes > Upload have both.
 */
async function ready(cdp, options) {
  await waitFor(cdp, snippets.SHELL_READY, "the fixture shell");
  if (options.keepFixtureBanner) {
    return {
      existed: Boolean(
        await cdp.evaluate("document.querySelector('[data-dev-fixtures=\"banner\"]')"),
      ),
      kept: true,
    };
  }
  return { ...(await cdp.evaluate(snippets.removeFixtureBanner())), kept: false };
}

export async function runShellTitleBar(options) {
  const out = resolve(options.out);
  await mkdir(out, { recursive: true });
  let browser;
  let cdp;

  try {
    browser = await launchEdge({ ...options, out });
    cdp = await connectCdp(browser.target.webSocketDebuggerUrl);
    await cdp.send("Page.enable");
    await viewport(cdp, 1000, 720);
    // The panel's collapsed state is read from the window width once, at
    // start-up, so the 1000x720 measurements begin from a load at that width.
    await reload(cdp);
    const banner = await ready(cdp, options);

    const initial = await cdp.evaluate(snippets.titleBarState());
    const report = createReport({
      url: options.url,
      edgeVersion: browser.version.Browser ?? browser.version,
      remBasis: initial.rem,
      fixtureBannerRemoved: !options.keepFixtureBanner && banner.existed,
      timestamp: new Date().toISOString(),
      measurementScope: "measured in headless Edge against dev fixtures, not in the Tauri window",
    });

    const shot1 = await screenshot(cdp, out, "01-title-bar.png", {
      clip: clipRect(initial),
      scale: 2,
    });
    addCheck(report, {
      id: "1",
      claim: "The title bar carries the four pills and marks exactly one current.",
      measured: initial,
      expectation: "button.nav-pill reading Dictation, Meetings, Notes, Settings; exactly one [aria-current=page], Dictation",
      matches: JSON.stringify(initial.items) === JSON.stringify(["Dictation", "Meetings", "Notes", "Settings"])
        && JSON.stringify(initial.current) === JSON.stringify(["Dictation"]),
      screenshots: [shot1],
      note: "Measured on the opening screen, before any navigation.",
    });

    await click(cdp, NAV_PILL, "Settings");
    const settings = await cdp.evaluate(snippets.sheetState());
    const shot2 = await screenshot(cdp, out, "02-settings-sheet.png");
    addCheck(report, {
      id: "2",
      claim: "The Settings pill opens a modal sheet listing Providers under settings and a developer section, as one tab stop.",
      measured: settings,
      expectation: "dialog.settings-sheet open and modal; a tile reading Providers; .settings-sheet-label reading settings and developer; one tile with tabindex 0",
      matches: settings.open
        && settings.modal
        && settings.visible
        && settings.tiles.includes("Providers")
        && settings.labels.includes("settings")
        && settings.labels.includes("developer")
        && settings.tabStops === 1,
      screenshots: [shot2],
      note: "Reached by clicking the Settings pill from the opening screen.",
    });

    await click(cdp, snippets.SHEET_TILE, "Providers");
    await waitFor(cdp, "document.querySelector('.page-header h1')?.textContent.trim().length > 0", "the Providers page");
    const closed = await cdp.evaluate(snippets.sheetState());
    const providers = {
      sheetOpen: closed.open,
      bar: await cdp.evaluate(snippets.titleBarState()),
      heading: await cdp.evaluate(snippets.pageHeading()),
      headingFocused: await cdp.evaluate("document.activeElement === document.querySelector('.split-main h1')"),
    };
    await click(cdp, BACK_TO_SHEET, "Settings");
    providers.reopened = await cdp.evaluate(snippets.sheetState());
    const shot3 = await screenshot(cdp, out, "03-providers.png");
    await key(cdp, { key: "Escape", code: "Escape", keyCode: 27 });
    await delay(100);
    providers.afterEscape = (await cdp.evaluate(snippets.sheetState())).open;
    addCheck(report, {
      id: "3",
      claim: "Choosing Providers closes the sheet, marks that tile and the Settings pill alone, and puts focus on the page's title; the page's pill reopens the sheet and Escape closes it.",
      measured: providers,
      expectation: "sheet closed; pill current is Settings alone; the h1 focused; after .page-back, sheet open with Providers the one current tile; after Escape, closed",
      matches: !providers.sheetOpen
        && providers.bar.current.length === 1
        && providers.bar.current[0] === "Settings"
        && providers.headingFocused
        && providers.reopened.open
        && providers.reopened.current.length === 1
        && providers.reopened.current[0] === "Providers"
        && providers.afterEscape === false,
      screenshots: [shot3],
      note: `The page heading read ${JSON.stringify(providers.heading)} after the click.`,
    });

    // Only Notes has a list panel now, and the toggle sits in it.
    await click(cdp, NAV_PILL, "Notes");
    await waitFor(cdp, NOTE_OPEN, "a note to open");
    const beforeToggle = await cdp.evaluate(snippets.panelToggle());
    await cdp.evaluate(snippets.clickSelector(TOGGLE));
    await delay(150);
    const collapsed = {
      panel: await cdp.evaluate(snippets.panelState()),
      toggle: await cdp.evaluate(snippets.panelToggle()),
    };
    const shot4a = await screenshot(cdp, out, "04-panel-hidden.png");
    await cdp.evaluate(snippets.clickSelector(TOGGLE));
    await delay(150);
    const restored = {
      panel: await cdp.evaluate(snippets.panelState()),
      toggle: await cdp.evaluate(snippets.panelToggle()),
    };
    const shot4b = await screenshot(cdp, out, "04-panel-restored.png");
    addCheck(report, {
      id: "4",
      claim: "On Notes the panel's toggle hides the list panel, a second press restores it, and focus stays on the toggle each time.",
      measured: { beforeToggle, collapsed, restored },
      expectation: [
        "after one press .list-panel is not rendered and the toggle reads Show list,",
        "aria-expanded false, focused; after the second it is rendered again, Hide list, true, focused",
      ].join(" "),
      matches: beforeToggle.expanded === "true"
        && !collapsed.panel.visible
        && collapsed.toggle.label === "Show list"
        && collapsed.toggle.expanded === "false"
        && collapsed.toggle.focused
        && restored.panel.visible
        && restored.toggle.label === "Hide list"
        && restored.toggle.expanded === "true"
        && restored.toggle.focused,
      screenshots: [shot4a, shot4b],
      note: "Visibility is the computed display and width, not the hidden attribute alone. The press removes the button pressed, so the focus half is ListToggle's replacement taking it.",
    });

    // The minimum window: a fresh load at 720x520, because the panel's
    // starting state is decided by the width at start-up.
    await viewport(cdp, 720, 520);
    await reload(cdp);
    await ready(cdp, options);
    await click(cdp, NAV_PILL, "Notes");
    await waitFor(cdp, NOTE_OPEN, "a note to open");
    const narrow = {
      panel: await cdp.evaluate(snippets.panelState()),
      toggle: await cdp.evaluate(snippets.panelToggle()),
      overflow: await cdp.evaluate(snippets.overflowState()),
    };
    await click(cdp, NAV_PILL, "Settings");
    narrow.sheet = await cdp.evaluate(snippets.sheetState());
    const shot5 = await screenshot(cdp, out, "05-minimum-720x520.png");
    await key(cdp, { key: "Escape", code: "Escape", keyCode: 27 });
    addCheck(report, {
      id: "5",
      claim: "At 720x520 the list panel starts hidden, nothing scrolls sideways, and the Settings sheet opens.",
      measured: narrow,
      expectation: [
        "after a load at 720x520 and a press on Notes, .list-panel is not rendered;",
        "document.scrollingElement.scrollWidth <= innerWidth; the Settings pill then shows the sheet",
      ].join(" "),
      matches: narrow.panel.present
        && !narrow.panel.visible
        && narrow.overflow.scrollWidth <= narrow.overflow.innerWidth
        && narrow.sheet.open
        && narrow.sheet.visible,
      screenshots: [shot5],
      note: [
        "The page's own .split-main widths are recorded too, since the root does not",
        "scroll in the main window and the page scrolls inside it. Check 12 holds those for every screen.",
      ].join(" "),
    });

    await viewport(cdp, 1000, 720);
    await reload(cdp);
    await ready(cdp, options);
    await cdp.evaluate(snippets.blurActive());
    const focused = [];
    for (let i = 0; i < 30; i += 1) {
      await key(cdp, { key: "Tab", code: "Tab", keyCode: 9 });
      const state = await cdp.evaluate(snippets.focusState());
      if (state) focused.push(state);
      const railReached = new Set(
        focused.filter((item) => item.kind === "pill").map((item) => item.text),
      );
      if (railReached.size === 4 && focused.some((item) => item.kind === "search")) break;
    }
    await cdp.evaluate(`(() => {
      [...document.querySelectorAll('button.nav-pill')]
        .find((item) => item.textContent.trim() === 'Settings')?.focus();
    })()`);
    const shot6 = await screenshot(cdp, out, "06-focus.png", {
      clip: { x: 480, y: 0, width: 520, height: 120 },
      scale: 2,
    });
    const reached = focused.filter((item) => item.kind === "pill" || item.kind === "search");
    const railTexts = new Set(reached.filter((item) => item.kind === "pill").map((item) => item.text));
    addCheck(report, {
      id: "6",
      claim: "Tab reaches every pill and the search field, each drawing a 2px outline.",
      measured: { focused },
      expectation: [
        "the four pills and input[role=combobox] each receive focus from Tab,",
        "each with a 2px outline whose style is not none",
      ].join(" "),
      matches: railTexts.size === 4
        && reached.some((item) => item.kind === "search")
        && reached.every((item) => item.outlineWidth === "2px" && item.outlineStyle !== "none"),
      screenshots: [shot6],
      note: [
        "Tab was dispatched from the document body, up to 30 times, stopping once",
        "all four pills and the search field had been reached.",
      ].join(" "),
    });

    await cdp.evaluate(snippets.blurActive());
    const beforeShortcut = await cdp.evaluate(snippets.searchState());
    await key(cdp, { key: "k", code: "KeyK", keyCode: 75, modifiers: 2 });
    await delay(100);
    const afterShortcut = await cdp.evaluate(snippets.searchState());
    await cdp.send("Input.insertText", { text: "meeting" });
    await delay(400);
    const typed = await cdp.evaluate(snippets.searchState());
    const shot7 = await screenshot(cdp, out, "07-search.png");
    await key(cdp, { key: "Escape", code: "Escape", keyCode: 27 });
    await delay(100);
    const afterEscape = await cdp.evaluate(snippets.searchState());
    addCheck(report, {
      id: "7",
      claim: "Ctrl+K focuses the search field.",
      measured: { beforeShortcut, afterShortcut, typed, afterEscape },
      expectation: [
        "input[role=combobox] is the active element after Ctrl+K and not before;",
        "Escape then empties it and moves focus away",
      ].join(" "),
      matches: !beforeShortcut.focused
        && afterShortcut.focused
        && !afterEscape.focused
        && afterEscape.value === "",
      screenshots: [shot7],
      note: [
        "The Escape half is the field's other shortcut, measured in the same pass;",
        "the screenshot is taken after typing \"meeting\", with the results open.",
      ].join(" "),
    });

    // Notes, from a fresh load at 1000x720: the page opens on fixture note 101,
    // which has a transcript. The viewport then widens past 1280px and narrows
    // back on the same loaded page, so what re-lays the note out is the page's
    // own `(min-width: 1280px)` change listener, not a reload.
    await viewport(cdp, 1000, 720);
    await reload(cdp);
    await ready(cdp, options);
    await click(cdp, NAV_PILL, "Notes");
    await waitFor(cdp, NOTE_OPEN, "a note to open");
    const notesNarrow = await cdp.evaluate(snippets.notesGrid());
    const shot8a = await screenshot(cdp, out, "08-notes-1000x720.png");
    await viewport(cdp, 1440, 900);
    await waitFor(cdp, "Boolean(document.querySelector('.note-detail-wide .note-side'))", "the transcript column");
    const notesWide = await cdp.evaluate(snippets.notesGrid());
    const shot8b = await screenshot(cdp, out, "08-notes-1440x900.png");
    await viewport(cdp, 1000, 720);
    await waitFor(cdp, "!document.querySelector('.note-detail-wide')", "the transcript column to go");
    const notesBack = await cdp.evaluate(snippets.notesGrid());
    const same = (a, b) => JSON.stringify(a) === JSON.stringify(b);
    addCheck(report, {
      id: "8",
      claim: [
        "Below 1280px a note is one column with three tabs; at 1440px its transcript is a",
        "330px second column and the Transcript tab is hidden; narrowing again brings the tab back.",
      ].join(" "),
      measured: { notesNarrow, notesWide, notesBack },
      expectation: [
        ".note-detail has 1 grid track and tabs Notes, Transcript, Replay at 1000x720;",
        "2 tracks, tabs Notes, Replay and a 330px .note-side at 1440x900; 1 track and three tabs again at 1000x720",
      ].join(" "),
      matches: notesNarrow.tracks === 1
        && same(notesNarrow.tabs, ["Notes", "Transcript", "Replay"])
        && notesNarrow.sideWidth === null
        && notesWide.tracks === 2
        && same(notesWide.tabs, ["Notes", "Replay"])
        && notesWide.sideWidth === 330
        && notesBack.tracks === 1
        && same(notesBack.tabs, ["Notes", "Transcript", "Replay"]),
      screenshots: [shot8a, shot8b],
      note: "Measured on fixture note 101, the note Notes opens first. Tabs are counted by computed display.",
    });

    // The minimum window again, on Notes with the library opened: the panel
    // starts hidden at 720px, so the toggle is pressed once.
    await viewport(cdp, 720, 520);
    await reload(cdp);
    await ready(cdp, options);
    await click(cdp, NAV_PILL, "Notes");
    await waitFor(cdp, NOTE_OPEN, "a note to open");
    const notesToggle = await cdp.evaluate(snippets.panelToggle());
    await cdp.evaluate(snippets.clickSelector(TOGGLE));
    await delay(150);
    const notesMinimum = {
      toggle: notesToggle,
      panel: await cdp.evaluate(snippets.panelState()),
      overflow: await cdp.evaluate(snippets.overflowState()),
      tabs: await cdp.evaluate(snippets.notesTabsContained()),
      longTitle: await cdp.evaluate(snippets.notesEntry(LONG_TITLE)),
    };
    const shot9 = await screenshot(cdp, out, "09-notes-720x520.png");
    addCheck(report, {
      id: "9",
      claim: [
        "At 720x520 with the library open, a note fits without sideways scroll, its tabs sit inside",
        ".notes-main, and note 104's long title is cut off with an ellipsis in the panel.",
      ].join(" "),
      measured: notesMinimum,
      expectation: [
        "the toggle starts collapsed and one press shows .list-panel; document and .notes-main",
        "scrollWidth <= their clientWidth; all three tabs inside .notes-main; the 104 row's title",
        "is nowrap with text-overflow ellipsis and scrollWidth > clientWidth",
      ].join(" "),
      matches: notesToggle.expanded === "false"
        && notesMinimum.panel.visible
        && notesMinimum.overflow.scrollWidth <= notesMinimum.overflow.innerWidth
        && notesMinimum.tabs.mainScrollWidth <= notesMinimum.tabs.mainClientWidth
        && notesMinimum.tabs.count === 3
        && notesMinimum.tabs.contained
        && notesMinimum.longTitle.ellipsised,
      screenshots: [shot9],
      note: "The tab comparison is the one notes-smoke.mjs makes in the native window; this is not that run.",
    });

    // The transcript column's own button, from a fresh load at 1440x900 on
    // fixture note 101. Hidden, the note goes back to one column with the
    // Transcript tab; crossing the width then moves nothing; another note opens
    // with the column still hidden; showing it gives an open Transcript tab way
    // to Notes. Each press focuses the button first, as a keyboard press would.
    await viewport(cdp, 1440, 900);
    await reload(cdp);
    await ready(cdp, options);
    await click(cdp, NAV_PILL, "Notes");
    await waitFor(cdp, NOTE_OPEN, "a note to open");
    await waitFor(cdp, "Boolean(document.querySelector('.note-detail-wide .note-side'))", "the transcript column");
    const column = {};
    column.shown = { grid: await cdp.evaluate(snippets.notesGrid()), toggle: await cdp.evaluate(snippets.transcriptToggle()) };
    await cdp.evaluate(snippets.pressTranscriptToggle());
    await waitFor(cdp, "!document.querySelector('.note-detail-wide')", "the transcript column to hide");
    column.hidden = { grid: await cdp.evaluate(snippets.notesGrid()), toggle: await cdp.evaluate(snippets.transcriptToggle()) };
    const shot10a = await screenshot(cdp, out, "10-notes-column-hidden-1440x900.png");
    await click(cdp, '.note-detail [role="tab"]', "Transcript");
    await viewport(cdp, 1000, 720);
    column.narrow = { grid: await cdp.evaluate(snippets.notesGrid()), toggle: await cdp.evaluate(snippets.transcriptToggle()) };
    await viewport(cdp, 1440, 900);
    await delay(150);
    column.widenedAgain = { grid: await cdp.evaluate(snippets.notesGrid()), toggle: await cdp.evaluate(snippets.transcriptToggle()) };
    const firstNote = await cdp.evaluate("document.querySelector('.note-detail').id");
    await cdp.evaluate(`[...document.querySelectorAll('.list-panel .notes-entry')].find((entry) => entry.querySelector('.notes-entry-title')?.textContent.startsWith(${JSON.stringify(LONG_TITLE.slice(0, 24))})).click()`);
    await waitFor(cdp, `document.querySelector('.note-detail')?.id !== ${JSON.stringify(firstNote)} && ${NOTE_OPEN}`, "another note to open");
    column.otherNote = { id: await cdp.evaluate("document.querySelector('.note-detail').id"), grid: await cdp.evaluate(snippets.notesGrid()), toggle: await cdp.evaluate(snippets.transcriptToggle()) };
    await click(cdp, '.note-detail [role="tab"]', "Transcript");
    await cdp.evaluate(snippets.pressTranscriptToggle());
    await waitFor(cdp, "Boolean(document.querySelector('.note-detail-wide .note-side'))", "the transcript column to come back");
    column.shownAgain = { grid: await cdp.evaluate(snippets.notesGrid()), toggle: await cdp.evaluate(snippets.transcriptToggle()) };
    const shot10b = await screenshot(cdp, out, "10-notes-column-shown-1440x900.png");
    const shownRight = (state) => state.grid.tracks === 2 && state.grid.sideWidth === 330
      && same(state.grid.tabs, ["Notes", "Replay"])
      && state.toggle.label === "Hide transcript" && state.toggle.expanded === "true" && state.toggle.controlsExists === true;
    const hiddenRight = (state) => state.grid.tracks === 1 && state.grid.sideWidth === null
      && same(state.grid.tabs, ["Notes", "Transcript", "Replay"])
      && state.toggle.label === "Show transcript" && state.toggle.expanded === "false" && state.toggle.controls === null;
    addCheck(report, {
      id: "10",
      claim: [
        "At 1440x900 the note's actions hide and show the transcript column: hidden, the note is one",
        "column with three tabs at any width, and stays so for the next note opened; shown, the column is",
        "back and an open Transcript tab gives way to Notes. The button keeps focus throughout.",
      ].join(" "),
      measured: column,
      expectation: [
        "shown: 2 tracks, a 330px .note-side, tabs Notes, Replay, \"Hide transcript\" expanded, controlling",
        "an element that exists; hidden: 1 track, no .note-side, three tabs, \"Show transcript\" collapsed with",
        "no aria-controls, focused; at 1000x720 no button, 1 track, Transcript still selected; back at 1440x900",
        "still hidden; another note opens hidden; shown again with Notes selected and the button focused",
      ].join(" "),
      matches: shownRight(column.shown)
        && hiddenRight(column.hidden) && column.hidden.toggle.focused
        && column.narrow.toggle.exists === false && column.narrow.grid.tracks === 1
        && column.narrow.toggle.selectedTab === "Transcript"
        && hiddenRight(column.widenedAgain) && column.widenedAgain.toggle.selectedTab === "Transcript"
        && column.otherNote.id !== firstNote && hiddenRight(column.otherNote)
        && shownRight(column.shownAgain) && column.shownAgain.toggle.focused
        && column.shownAgain.toggle.selectedTab === "Notes",
      screenshots: [shot10a, shot10b],
      note: [
        "The other note is fixture note 104. Playback across the button is note-playback.mjs check 4,",
        "against the fixtures' playable recording (KNEG-412); the reading position across it is not measured here.",
      ].join(" "),
    });

    // Leaving Notes and coming back, from check 10's note 104, which is not the
    // note Notes opens first; then a reload, which is the app opening again.
    const OPEN_ID = "document.querySelector('.note-detail')?.id ?? null";
    const CURRENT_ENTRY = "document.querySelector('.list-panel .notes-entry[aria-current=\"page\"] .notes-entry-title')?.textContent ?? null";
    const resume = { left: await cdp.evaluate(OPEN_ID) };
    await click(cdp, NAV_PILL, "Dictation");
    await waitFor(cdp, "Boolean(document.querySelector('[data-surface=\"dictation.task\"]'))", "the Dictation page");
    resume.away = await cdp.evaluate(OPEN_ID);
    await click(cdp, NAV_PILL, "Notes");
    await waitFor(cdp, NOTE_OPEN, "a note to open");
    resume.back = { id: await cdp.evaluate(OPEN_ID), current: await cdp.evaluate(CURRENT_ENTRY) };
    const shot11 = await screenshot(cdp, out, "11-notes-reopened-1440x900.png");
    await reload(cdp);
    await ready(cdp, options);
    await click(cdp, NAV_PILL, "Notes");
    await waitFor(cdp, NOTE_OPEN, "a note to open");
    resume.reloaded = await cdp.evaluate(OPEN_ID);
    addCheck(report, {
      id: "11",
      claim: [
        "Going to Dictation from note 104 and back through the pills reopens note 104, marked in the",
        "library; after a reload, as when the app starts, Notes opens its first note again.",
      ].join(" "),
      measured: resume,
      expectation: [
        "left is check 10's other note and not its first; no .note-detail on Dictation; back, the same",
        "id with the long-titled entry current; reloaded, check 10's first note",
      ].join(" "),
      matches: resume.left === column.otherNote.id && resume.left !== firstNote
        && resume.away === null
        && resume.back.id === resume.left
        && typeof resume.back.current === "string" && resume.back.current.startsWith(LONG_TITLE.slice(0, 24))
        && resume.reloaded === firstNote,
      screenshots: [shot11],
      note: [
        "Not measured: the fall-back when the remembered note has been deleted (the fixtures' delete",
        "asks the confirmation dialog), and a remembered note older than the library's 100 newest.",
      ].join(" "),
    });

    // Every screen at the minimum width and the default one. A fresh load per
    // width, so the panel starts as a window of that width starts it.
    const sideways = [];
    for (const [width, height] of [[720, 520], [1000, 720]]) {
      await viewport(cdp, width, height);
      await reload(cdp);
      await ready(cdp, options);
      const measure = async (screen) => {
        await delay(400);
        const state = await cdp.evaluate(snippets.overflowState());
        sideways.push({ width, screen, ...state, overflows: state.scrollWidth > state.innerWidth || (state.mainScrollWidth ?? 0) > (state.mainClientWidth ?? 0) });
      };
      for (const page of PAGES) {
        await click(cdp, NAV_PILL, page);
        await measure(page);
      }
      for (const tile of TILES) {
        await cdp.evaluate(snippets.openSettingsScreen(tile));
        await measure(`Settings > ${tile}`);
      }
    }
    const overflowing = sideways.filter((item) => item.overflows);
    const shot12 = await screenshot(cdp, out, "12-last-screen-1000x720.png");
    addCheck(report, {
      id: "12",
      claim: "No screen scrolls sideways at 720x520 or 1000x720.",
      measured: { overflowing, screens: sideways.length },
      expectation: "for the three pages and all twelve tiles, at both sizes: document and .split-main scrollWidth <= their client width",
      matches: overflowing.length === 0,
      screenshots: [shot12],
      note: "Each screen is measured as it opens on fixture data; a state a screen only reaches after a press is not measured here.",
    });

    const markdown = renderMarkdown(report);
    await writeFile(resolve(out, "report.json"), `${JSON.stringify(report, null, 2)}\n`);
    await writeFile(resolve(out, "report.md"), markdown);
    process.stdout.write(markdown);
    return report;
  } finally {
    await cdp?.close();
    await killBrowser(browser?.child);
  }
}

if (isMain) {
  try {
    const options = parseShellArgs(process.argv.slice(2));
    if (options.help) process.stdout.write(`${shellUsage()}\n`);
    else {
      const failed = failedChecks(await runShellTitleBar(options));
      // The run completed; a check measured false still fails it, so a wrapper or CI step sees it.
      if (failed.length) {
        console.error(`shell-title-bar: ${failed.length} check(s) false: ${failed.join(", ")}`);
        process.exitCode = 1;
      }
    }
  } catch (error) {
    console.error(`shell-title-bar failed: ${error.message}`);
    process.exitCode = 1;
  }
}
