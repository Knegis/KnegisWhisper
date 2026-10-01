#!/usr/bin/env node
/**
 * Runs repeatable rail, list-panel and title-bar measurements against dev
 * fixtures: the 68px rail and its one current entry, the Settings panel and
 * its Developer section, the title bar's panel toggle, the 720x520 minimum
 * window, keyboard focus, Ctrl+K, and Notes: the transcript column a wide
 * window gives a note, the 720x520 fit of a note beside its library, and the
 * note's button that hides and shows that column.
 */
import { mkdir, writeFile } from "node:fs/promises";
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { parseShellArgs, shellUsage } from "./args.mjs";
import { launchEdge, killBrowser } from "./browser.mjs";
import { connectCdp } from "./cdp.mjs";
import { createReport, addCheck, renderMarkdown } from "./report.mjs";
import * as snippets from "./snippets.mjs";

const delay = (ms) => new Promise((resolveDelay) => setTimeout(resolveDelay, ms));
const isMain = process.argv[1]
  && resolve(process.argv[1]) === fileURLToPath(import.meta.url);

/** A screenshot clip for a measured rectangle, in page coordinates. */
export function clipRail({ rect, scrollX = 0, scrollY = 0 }) {
  return {
    x: rect.left + scrollX,
    y: rect.top + scrollY,
    width: rect.width,
    height: rect.height,
  };
}

const TOGGLE = '.app-titlebar button[aria-controls="list-panel"]';
const RAIL_ITEM = "nav.rail .rail-item";
const PANEL_ITEM = ".list-panel .panel-item";
// Fixture note 104 (`apps/desktop/src/lib/devFixtures.ts`), whose title is there to be too long.
const LONG_TITLE = "A deliberately long title that exists so the notes list can be judged when someone types far more than the column was designed for";
const NOTE_OPEN = "Boolean(document.querySelector('.note-detail input[aria-label=\"Title\"]:enabled'))";

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
 * "A page in the card" is `.split-main` or `.content`: Dictation, the opening
 * screen, draws its own split with no `.content` since KNEG-363's Task 3, as
 * Meetings and Notes do; the settings screens and Notes > Upload have both.
 */
async function ready(cdp, options) {
  await waitFor(
    cdp,
    "Boolean(document.querySelector('nav.rail') && document.querySelector('.card .split-main, .card .content'))",
    "the fixture shell",
  );
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

export async function runShellRail(options) {
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

    const initial = await cdp.evaluate(snippets.railState());
    const report = createReport({
      url: options.url,
      edgeVersion: browser.version.Browser ?? browser.version,
      remBasis: initial.rem,
      fixtureBannerRemoved: !options.keepFixtureBanner && banner.existed,
      timestamp: new Date().toISOString(),
      measurementScope: "measured in headless Edge against dev fixtures, not in the Tauri window",
    });

    const shot1 = await screenshot(cdp, out, "01-rail.png", {
      clip: clipRail(initial),
      scale: 2,
    });
    addCheck(report, {
      id: "1",
      claim: "The rail is 68px wide and marks exactly one entry current.",
      measured: initial,
      expectation: "nav.rail width 68px; exactly one .rail-item[aria-current=page]",
      matches: initial.rect.width === 68 && initial.current.length === 1,
      screenshots: [shot1],
      note: "Measured on the opening screen, before any navigation.",
    });

    await click(cdp, RAIL_ITEM, "Settings");
    const settings = await cdp.evaluate(snippets.panelState());
    const shot2 = await screenshot(cdp, out, "02-settings-panel.png");
    addCheck(report, {
      id: "2",
      claim: "The rail's Settings opens a list panel listing Providers under Settings and a Developer section.",
      measured: settings,
      expectation: ".list-panel visible, an item reading Providers, a .panel-label reading Developer",
      matches: settings.visible
        && settings.items.includes("Providers")
        && settings.labels.includes("Developer"),
      screenshots: [shot2],
      note: "Reached by clicking the rail's Settings from the opening screen.",
    });

    await click(cdp, PANEL_ITEM, "Providers");
    const providers = {
      panel: await cdp.evaluate(snippets.panelState()),
      rail: await cdp.evaluate(snippets.railState()),
      heading: await cdp.evaluate(snippets.pageHeading()),
    };
    const shot3 = await screenshot(cdp, out, "03-providers.png");
    addCheck(report, {
      id: "3",
      claim: "Choosing Providers marks exactly one panel item, and the rail still marks Settings.",
      measured: providers,
      expectation: "one .panel-item[aria-current] reading Providers; rail current is Settings alone",
      matches: providers.panel.current.length === 1
        && providers.panel.current[0] === "Providers"
        && providers.rail.current.length === 1
        && providers.rail.current[0] === "Settings",
      screenshots: [shot3],
      note: `The page heading read ${JSON.stringify(providers.heading)} after the click.`,
    });

    const beforeToggle = await cdp.evaluate(snippets.panelToggle());
    await cdp.evaluate(snippets.clickSelector(TOGGLE));
    await delay(100);
    const collapsed = {
      panel: await cdp.evaluate(snippets.panelState()),
      toggle: await cdp.evaluate(snippets.panelToggle()),
    };
    const shot4a = await screenshot(cdp, out, "04-panel-hidden.png");
    await cdp.evaluate(snippets.clickSelector(TOGGLE));
    await delay(100);
    const restored = {
      panel: await cdp.evaluate(snippets.panelState()),
      toggle: await cdp.evaluate(snippets.panelToggle()),
    };
    const shot4b = await screenshot(cdp, out, "04-panel-restored.png");
    addCheck(report, {
      id: "4",
      claim: "The title bar's toggle hides the list panel, and a second press restores it.",
      measured: { beforeToggle, collapsed, restored },
      expectation: [
        "after one press .list-panel is not rendered and the toggle reads Show list,",
        "aria-expanded false; after the second it is rendered again, Hide list, true",
      ].join(" "),
      matches: beforeToggle.expanded === "true"
        && !collapsed.panel.visible
        && collapsed.toggle.label === "Show list"
        && collapsed.toggle.expanded === "false"
        && restored.panel.visible
        && restored.toggle.label === "Hide list"
        && restored.toggle.expanded === "true",
      screenshots: [shot4a, shot4b],
      note: "Visibility is the computed display and width, not the hidden attribute alone.",
    });

    // The minimum window: a fresh load at 720x520, because the panel's
    // starting state is decided by the width at start-up.
    await viewport(cdp, 720, 520);
    await reload(cdp);
    await ready(cdp, options);
    await click(cdp, RAIL_ITEM, "Settings");
    const narrow = {
      panel: await cdp.evaluate(snippets.panelState()),
      toggle: await cdp.evaluate(snippets.panelToggle()),
      overflow: await cdp.evaluate(snippets.overflowState()),
    };
    const shot5 = await screenshot(cdp, out, "05-minimum-720x520.png");
    addCheck(report, {
      id: "5",
      claim: "At 720x520 the list panel starts hidden and nothing scrolls sideways.",
      measured: narrow,
      expectation: [
        "after a load at 720x520 and a press on Settings, .list-panel is not rendered;",
        "document.scrollingElement.scrollWidth <= innerWidth",
      ].join(" "),
      matches: narrow.panel.present
        && !narrow.panel.visible
        && narrow.overflow.scrollWidth <= narrow.overflow.innerWidth,
      screenshots: [shot5],
      note: [
        "The card's own .split-main widths are recorded too, since the root does not",
        "scroll in the main window and the page scrolls inside the card.",
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
        focused.filter((item) => item.kind === "rail").map((item) => item.text),
      );
      if (railReached.size === 4 && focused.some((item) => item.kind === "search")) break;
    }
    await cdp.evaluate(`(() => {
      [...document.querySelectorAll('nav.rail .rail-item')]
        .find((item) => item.textContent.trim() === 'Settings')?.focus();
    })()`);
    const shot6 = await screenshot(cdp, out, "06-focus.png", {
      clip: { x: 0, y: 0, width: 520, height: 300 },
      scale: 2,
    });
    const reached = focused.filter((item) => item.kind === "rail" || item.kind === "search");
    const railTexts = new Set(reached.filter((item) => item.kind === "rail").map((item) => item.text));
    addCheck(report, {
      id: "6",
      claim: "Tab reaches every rail entry and the search field, each drawing a 2px outline.",
      measured: { focused },
      expectation: [
        "the four rail entries and input[role=combobox] each receive focus from Tab,",
        "each with a 2px outline whose style is not none",
      ].join(" "),
      matches: railTexts.size === 4
        && reached.some((item) => item.kind === "search")
        && reached.every((item) => item.outlineWidth === "2px" && item.outlineStyle !== "none"),
      screenshots: [shot6],
      note: [
        "Tab was dispatched from the document body, up to 30 times, stopping once",
        "all four rail entries and the search field had been reached.",
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
    await click(cdp, RAIL_ITEM, "Notes");
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
        "340px second column and the Transcript tab is hidden; narrowing again brings the tab back.",
      ].join(" "),
      measured: { notesNarrow, notesWide, notesBack },
      expectation: [
        ".note-detail has 1 grid track and tabs Notes, Transcript, Replay at 1000x720;",
        "2 tracks, tabs Notes, Replay and a 340px .note-side at 1440x900; 1 track and three tabs again at 1000x720",
      ].join(" "),
      matches: notesNarrow.tracks === 1
        && same(notesNarrow.tabs, ["Notes", "Transcript", "Replay"])
        && notesNarrow.sideWidth === null
        && notesWide.tracks === 2
        && same(notesWide.tabs, ["Notes", "Replay"])
        && notesWide.sideWidth === 340
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
    await click(cdp, RAIL_ITEM, "Notes");
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
    await click(cdp, RAIL_ITEM, "Notes");
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
    const shownRight = (state) => state.grid.tracks === 2 && state.grid.sideWidth === 340
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
        "shown: 2 tracks, a 340px .note-side, tabs Notes, Replay, \"Hide transcript\" expanded, controlling",
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
        "The other note is fixture note 104. Playback and the reading position across the button are not",
        "measured here: the fixtures have no recording to play (see the NoteWorkspace hand-off).",
      ].join(" "),
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
    else await runShellRail(options);
  } catch (error) {
    console.error(`shell-rail failed: ${error.message}`);
    process.exitCode = 1;
  }
}
