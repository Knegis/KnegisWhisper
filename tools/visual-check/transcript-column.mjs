#!/usr/bin/env node
/**
 * Measures, against dev fixtures, the transcript column beside a note in a
 * wide window as the M6 redesign draws it (KNEG-448). The claims arrive with
 * the tasks that make them true, so the ids are the plan's and are not
 * contiguous: 1, 11, 13, 14 and 16 are the column's header, sections, turns,
 * rename and hover, 2 and 3 the player card, 4, 8 and 12 the click model
 * (a press plays from a line, Shift selects a range the card offers to play,
 * one tab stop with the arrow keys between lines), and 5, 6, 7 and 17 Follow
 * (the card's chip keeps the playing line in view until the person scrolls,
 * and gives way to a search hit's row).
 *
 * Fixture note 106, "Roadmap review — long transcript"
 * (`apps/desktop/src/lib/devFixtures.ts`, its rows in `lib/fixtureTranscript.ts`):
 * a 690 s recording, which the fixtures serve as a playable WAV of that
 * length, and a transcript in three five-minute sections starting at 0, 300
 * and 600 s. The player ticks the scrubber where every section after the
 * first starts.
 *
 * Each check starts from a fresh load at 1440x900 on Notes with note 106
 * opened from the library (13 also at 1000x720 on the Transcript tab), and
 * measures in both colour schemes: `prefers-color-scheme` emulated over CDP,
 * which is what the stylesheet reads (`styles.css`,
 * `@media (prefers-color-scheme: light)`), with `data-theme` set on the root
 * to match. Presses are real CDP pointer presses. Edge runs with
 * `--mute-audio`, and a count of `play()` calls is installed before the
 * page's own code runs, as `transcript-search.mjs` installs it.
 */
import { mkdir, writeFile } from "node:fs/promises";
import { resolve } from "node:path";
import { parseShellArgs, shellUsage } from "./args.mjs";
import { launchEdge, killBrowser } from "./browser.mjs";
import { connectCdp } from "./cdp.mjs";
import { createReport, addCheck, failedChecks, renderMarkdown } from "./report.mjs";
import { delay, press, reload, waitFor } from "./interactions.mjs";
import * as snippets from "./snippets.mjs";
import { isEntryPoint } from "../ci/entry.mjs";

const isMain = isEntryPoint(import.meta.url);

const EDGE_ARGS = ["--mute-audio"];
const RAIL_ITEM = "nav.rail .rail-item";
const WIDE = { width: 1440, height: 900 };
const TITLE = "Roadmap review — long transcript";
const SCHEMES = ["dark", "light"];
const CARD = ".note-side .note-side-player";
/** Note 106's recording, and where its second and third sections start (`lib/fixtureTranscript.ts`). */
const DURATION = 690;
const SECTION_STARTS = [300, 600];
/** The card's distance from the column's left, right and bottom edges. */
const INSET = 12;
const INSET_TOLERANCE = 0.5;
const PLAY_SIZE = 34;
/** How far a tick's centre may sit from where the thumb would at that time. */
const MARK_TOLERANCE = 1;
const SKIP_FROM = 100;
const SKIP_TOLERANCE = 0.1;
const CLOCK = /^\d+:\d\d \/ \d+:\d\d$/;
const NARROW = { width: 1000, height: 720 };
const SIDE_BODY = ".note-side .note-side-body";
/**
 * Note 106 as `transcriptSections(longTranscriptRows())` lays it out (`lib/transcriptLayout.ts`
 * over `lib/fixtureTranscript.ts`), computed in node from the compiled `.test-build` on
 * 2026-10-12: three sections of 26, 19 and 9 lines, in 19, 14 and 8 turns.
 * `tests/fixtureTranscript.test.ts` pins the same titles and counts, so a change to the fixture
 * fails there before it fails here.
 */
const SECTIONS = [
  { title: "0:00 – 5:00", lines: 26 },
  { title: "5:00 – 10:00", lines: 19 },
  { title: "10:00 – 11:30", lines: 9 },
];
const LINES = 54;
const TURNS = 41;
const MARKER = 6;
/** How far claim 13 scrolls the transcript before reading where its head is. */
const HEAD_SCROLL = 600;
const HEAD_TOLERANCE = 1;
/** Claim 3's second part plays from here: inside "Tack alla för idag" (648 to 656 s), in the last section. */
const PLAY_FROM = 650;
/** Claim 11 renames the second turn's speaker, Jonas, whose first line starts at 0:22. */
const RENAME_NAME = "Jonas";
/** Claim 16 hovers line 2, "Vi går igenom tre saker…" (6 to 14 s), which has no word timings. */
const HOVER_LINE = 1;
const LOADING = "Loading transcript and recording…";
/** How long claim 14 holds `note_transcript_rows` back. */
const ROWS_DELAY_MS = 4000;
const NOTE_ID = 106;
/** Claim 4 presses line 4, "Bygget går igenom…" (22 to 30 s, Jonas's turn), which has no word timings. */
const CLICK_LINE = 3;
const CLICK_SPEAKER = "Jonas";
/** How far from a line's start the recording may be once it plays from it. */
const LANDING_TOLERANCE = 0.3;
/** Claim 8 selects lines 6 to 8, across three turns: Jonas 41–50 s, Maja 53–60 s, Speaker 3 63–72 s. */
const RANGE_FROM = 5;
const RANGE_TO = 7;
const RANGE_LINES = [5, 6, 7];
/** How far from the selection's end its playback may stop. */
const STOP_TOLERANCE = 0.25;
/** Claim 8 plays its selection at this speed, so the 31 s it spans take about 16. */
const RANGE_RATE = "2";
/** Note 106's one untimed line (line 40, Maja's "Det är i alla fall…") and the lines either side of it. */
const BEFORE_UNTIMED = 38;
const UNTIMED = 39;
const AFTER_UNTIMED = 40;
/** How many Tabs claim 12 may press walking through the transcript: it holds 3 toggles, 41 turn names and the line stops. */
const TAB_LIMIT = 150;
/** CDP's modifier bit for Shift. */
const SHIFT = 8;
const KEY_CODES = { Escape: 27, Enter: 13, Tab: 9, ArrowDown: 40, ArrowUp: 38, PageDown: 34 };
/**
 * Claims 5 and 6 start playback with a press on line 25, "Den tar jag, jag har skrivit om hälften."
 * (Jonas, 4:35 to 4:45), the second-last line of the first section. The press brings it to the
 * foot of the visible band, so the lines after it start below the band.
 */
const FOLLOW_LINE = 24;
/** The speed claims 5 to 7 play at, so the line changes they wait on come twice as fast. */
const FOLLOW_RATE = "2";
/** How long after a line starts playing claims 5 to 7 read where it stands: time for a smooth scroll it set off to end. */
const SETTLE_MS = 800;
/** How many line changes after the pressed line claim 5 watches. */
const FOLLOW_CHANGES = 2;
/** How far up claim 6's real wheel scrolls the column. */
const WHEEL = 400;
/** How far the playing line's centre may sit from the visible band's once Follow is pressed again. */
const CENTRE_TOLERANCE = 40;
/** Claim 7 plays from here, inside line 26 (4:49 to 4:58), so playback crosses into the second section at 5:02. */
const CROSS_FROM = 295;
const SECOND_SECTION = SECTIONS[1].title;
/** The first line of the second section: line 27, "Då går vi vidare…" (5:02). */
const SECOND_SECTION_FIRST = SECTIONS[0].lines;
/**
 * Claim 17 searches for a word only line 4 says, "Bygget går igenom på alla tre plattformarna…"
 * (0:22), with the player paused at 9:20, inside line 43 (9:20 to 9:30), far from it.
 */
const HIT_QUERY = "plattformarna";
const HIT_ROW = 3;
const HIT_FROM = 560;
const MINI = ".note-detail .note-mini-player";

/** Counts `play()` from before the app's own code runs (as `transcript-search.mjs` does). */
const COUNT_PLAYS = `(() => {
  window.__plays = 0;
  const play = HTMLMediaElement.prototype.play;
  HTMLMediaElement.prototype.play = function (...args) { window.__plays += 1; return play.apply(this, args); };
})();`;

const noteOpen = `(() => { const input = document.querySelector('.note-detail input[aria-label="Title"]'); return Boolean(input) && !input.disabled && input.value === ${JSON.stringify(TITLE)}; })()`;
const near = (value, expected, tolerance) => typeof value === "number" && Math.abs(value - expected) <= tolerance;
const round = (value) => (typeof value === "number" ? Math.round(value * 100) / 100 : value);
const noPlayingLine = "!document.querySelector('.note-detail .transcript-rows li[aria-current=\"true\"]')";

/** A fresh load at `size` on Notes with note 106 opened from the library; `stub` runs before it opens. */
async function loadNote(cdp, size, stub = null) {
  await cdp.send("Emulation.setDeviceMetricsOverride", { ...size, deviceScaleFactor: 1, mobile: false });
  await reload(cdp);
  await waitFor(cdp, "Boolean(document.querySelector('nav.rail') && document.querySelector('.card .split-main, .card .content'))", "the fixture shell");
  await cdp.evaluate(snippets.removeFixtureBanner());
  if (stub) await stub();
  await cdp.evaluate(snippets.clickText(RAIL_ITEM, "Notes"));
  await waitFor(cdp, "Boolean(document.querySelector('.list-panel .notes-entry'))", "the Notes library");
  await cdp.evaluate(snippets.openNotesEntry(TITLE));
  await waitFor(cdp, noteOpen, `note "${TITLE}" to open`);
}

/** A fresh load at 1440x900 with note 106 open, its transcript in the column and its recording's length known. */
async function openNote(cdp) {
  await loadNote(cdp, WIDE);
  await waitFor(
    cdp,
    `Boolean(document.querySelector('.note-side .transcript-rows li')) && (document.querySelector(${JSON.stringify(`${CARD} audio`)})?.readyState ?? 0) >= 1`,
    "the transcript column and the recording's metadata",
  );
  // The player renders its ticks once `loadedmetadata` has given it the length.
  await delay(250);
}

/** Draws the page in `scheme`: the emulated media feature, and `data-theme` to match. */
async function useScheme(cdp, scheme) {
  await cdp.send("Emulation.setEmulatedMedia", { features: [{ name: "prefers-color-scheme", value: scheme }] });
  const drawn = await cdp.evaluate(snippets.markColorScheme(scheme));
  await delay(100);
  return drawn;
}

/** A real pointer press at the centre of the first `selector` reading `text` (any, for null), which must be on screen. */
async function pressAt(cdp, selector, text = null) {
  const point = await cdp.evaluate(snippets.centerOfText(selector, text));
  await pressPoint(cdp, point, selector);
}

/** A real pointer press at `point`, which must be on screen. */
async function pressPoint(cdp, point, what) {
  const size = await cdp.evaluate("({ width: innerWidth, height: innerHeight })");
  if (point.x < 0 || point.y < 0 || point.x > size.width || point.y > size.height) {
    throw new Error(`${what} is off screen at ${JSON.stringify(point)}`);
  }
  await press(cdp, point);
}

/** A real pointer move to `point`, with no press. */
async function hover(cdp, point) {
  await cdp.send("Input.dispatchMouseEvent", { type: "mouseMoved", x: point.x, y: point.y });
  await delay(150);
}

/** A real key press of `name` (one of `KEY_CODES`), with CDP's `modifiers`, then `settle` ms. */
async function key(cdp, name, { modifiers = 0, settle = 150 } = {}) {
  const base = { key: name, code: name, windowsVirtualKeyCode: KEY_CODES[name], modifiers };
  await cdp.send("Input.dispatchKeyEvent", { type: "keyDown", ...base });
  await cdp.send("Input.dispatchKeyEvent", { type: "keyUp", ...base });
  await delay(settle);
}

/** A real key press: Escape. */
async function escape(cdp) {
  await key(cdp, "Escape");
}

/** A real wheel at `point`, `deltaY` pixels (negative scrolls up), after moving the pointer there. */
async function wheel(cdp, point, deltaY) {
  await cdp.send("Input.dispatchMouseEvent", { type: "mouseMoved", x: point.x, y: point.y });
  await cdp.send("Input.dispatchMouseEvent", { type: "mouseWheel", x: point.x, y: point.y, deltaX: 0, deltaY });
}

/** A real drag with the left button held, from `from` to `to` in `steps` moves, as a person selects text. */
async function drag(cdp, from, to, steps = 8) {
  await cdp.send("Input.dispatchMouseEvent", { type: "mouseMoved", x: from.x, y: from.y });
  await cdp.send("Input.dispatchMouseEvent", { type: "mousePressed", x: from.x, y: from.y, button: "left", buttons: 1, clickCount: 1 });
  for (let step = 1; step <= steps; step += 1) {
    const x = from.x + ((to.x - from.x) * step) / steps;
    const y = from.y + ((to.y - from.y) * step) / steps;
    await cdp.send("Input.dispatchMouseEvent", { type: "mouseMoved", x, y, button: "left", buttons: 1 });
    await delay(16);
  }
  await cdp.send("Input.dispatchMouseEvent", { type: "mouseReleased", x: to.x, y: to.y, button: "left", buttons: 0, clickCount: 1 });
  await delay(150);
}

/**
 * `scroller`'s `scrollTop` once it has held still for three reads 100 ms apart -- a smooth scroll
 * or a wheel's animation having ended -- or the last read after `timeout`.
 */
async function scrollSettled(cdp, scroller, timeout = 3_000) {
  const until = Date.now() + timeout;
  let last = null;
  let still = 0;
  while (Date.now() < until) {
    const now = await cdp.evaluate(`document.querySelector(${JSON.stringify(scroller)})?.scrollTop ?? null`);
    if (last !== null && now !== null && Math.abs(now - last) < 0.5) {
      still += 1;
      if (still >= 3) return now;
    } else {
      still = 0;
    }
    last = now;
    await delay(100);
  }
  return last;
}

/** One line `watchPlayingLines` recorded, judged against the visible band of claim 5: under the head's bottom, over the card's top. */
function bandLine(change) {
  const at = change.settled;
  return {
    index: change.index,
    scrollTopAtStart: round(change.scrollTop),
    scrollTop: round(at.scrollTop),
    top: round(at.top),
    bottom: round(at.bottom),
    headBottom: round(at.headBottom),
    cardTop: round(at.cardTop),
    current: at.current,
    follow: at.follow,
    inBand: at.drawn && at.headBottom !== null && at.top >= at.headBottom - 0.5 && at.bottom <= at.cardTop + 0.5,
  };
}

/**
 * Types `query` into the title bar's search and chooses the result for `title`, as
 * `transcript-search.mjs` does: the hit selects that note's transcript row and shows it (KNEG-406).
 */
async function chooseSearchHit(cdp, query, title) {
  await cdp.evaluate("(() => { const field = document.querySelector('.titlebar-search input'); field.focus(); field.select(); return true; })()");
  await cdp.send("Input.insertText", { text: query });
  await waitFor(cdp, "document.querySelectorAll('#titlebar-search-results [role=option]').length > 0", `results for ${query}`);
  const chosen = await cdp.evaluate(`(() => {
    const item = [...document.querySelectorAll('#titlebar-search-results [role=option]')].find((element) => element.textContent.includes(${JSON.stringify(title)}));
    item?.click();
    return Boolean(item);
  })()`);
  if (!chosen) throw new Error(`No search result for ${title}`);
}

/** The open note's recording, wherever its one player is drawn, is playing. */
const notePlaying = "(() => { const audio = document.querySelector('.note-detail audio'); return Boolean(audio) && !audio.paused; })()";

/** The time as the card's clock and the selection's label write it, m:ss (`lib/clock.ts`). */
const clock = (seconds) => `${Math.floor(seconds / 60)}:${String(Math.floor(seconds % 60)).padStart(2, "0")}`;

/** Whether the card's recording is playing. */
const playingNow = `(() => { const audio = document.querySelector(${JSON.stringify(`${CARD} audio`)}); return Boolean(audio) && !audio.paused && audio.readyState >= 3; })()`;

/** Moves focus to transcript line `index` from the page, as a step before a real key press. */
const focusLine = (index) => `(() => { const line = document.querySelectorAll('.note-detail .transcript .transcript-rows li')[${index}]; line?.focus(); return document.activeElement === line; })()`;

/** A real press in line `index`'s left padding (`linePadPoint`), which must land on the line itself and on screen. */
async function pressLine(cdp, index, modifiers = 0) {
  const point = await cdp.evaluate(snippets.linePadPoint(index));
  if (!point.onLine) throw new Error(`line ${index + 1}'s left padding is not the line at ${JSON.stringify(point)}`);
  const size = await cdp.evaluate("({ width: innerWidth, height: innerHeight })");
  if (point.x < 0 || point.y < 0 || point.x > size.width || point.y > size.height) {
    throw new Error(`line ${index + 1} is off screen at ${JSON.stringify(point)}`);
  }
  await press(cdp, point, { modifiers });
  return point;
}

/**
 * Runs `act` -- a press or a key that should play from somewhere -- and says what it did: how
 * many `play()` calls it made, where the recording landed once it played (`watchLanding`), and
 * the lines afterwards.
 */
async function played(cdp, act) {
  const before = await cdp.evaluate("window.__plays ?? null");
  await cdp.evaluate(snippets.watchLanding(CARD));
  await act();
  const landed = await waited(cdp, "(window.__landing?.playingAt ?? null) !== null", "the recording to play", 5_000);
  const landing = await cdp.evaluate(snippets.landingRecord());
  const after = await cdp.evaluate("window.__plays ?? null");
  return {
    landed,
    seeked: round(landing?.seeked ?? null),
    playingAt: round(landing?.playingAt ?? null),
    plays: typeof before === "number" && typeof after === "number" ? after - before : null,
    lines: await cdp.evaluate(snippets.lineModelState()),
  };
}

/** A zoomed screenshot of the card in `CARD`, `name`d, at 3x. */
async function cardShot(cdp, out, name) {
  const card = await cdp.evaluate(`(() => { const rect = document.querySelector(${JSON.stringify(CARD)}).getBoundingClientRect(); return { left: rect.left, top: rect.top, width: rect.width, height: rect.height }; })()`);
  await cdp.screenshot(resolve(out, name), { clip: { x: card.left - INSET, y: card.top - INSET, width: card.width + 2 * INSET, height: card.height + 2 * INSET }, scale: 3 });
  return name;
}

/** Pauses whatever plays in the card and waits for no line to be the playing one. */
async function stopPlaying(cdp) {
  await cdp.evaluate(`document.querySelector(${JSON.stringify(`${CARD} audio`)})?.pause()`);
  await waitFor(cdp, noPlayingLine, "no line to be playing", 3_000);
}

/** A fresh load at 1000x720 with note 106 open on its Transcript tab, opened with a real press. */
async function openNarrowTranscript(cdp) {
  await loadNote(cdp, NARROW);
  await pressAt(cdp, ".note-detail [role=\"tab\"]", "Transcript");
  await waitFor(
    cdp,
    "Boolean(document.querySelector('.note-detail [role=\"tabpanel\"]:not([hidden]) .transcript-rows li'))",
    "the Transcript tab's lines",
  );
}

/** The recording in the card once any seek has ended. */
async function settledTime(cdp) {
  await waitFor(cdp, `!${snippets.recordingAt(CARD)}.seeking`, "the seek to end", 3_000);
  return cdp.evaluate(snippets.recordingAt(CARD));
}

/** One scheme's card, judged: the expected tick centres beside the measured ones, and what failed. */
function judgeCard(state) {
  const range = state.range;
  const expectedMarks = range ? SECTION_STARTS.map((start) => round(range.left + 5 + (range.width - 10) * (start / DURATION))) : null;
  const playCentre = state.play ? state.play.top + state.play.height / 2 : null;
  const rowCentre = state.controls ? state.controls.top + state.controls.height / 2 : null;
  const parts = {
    inset: near(state.inset.left, INSET, INSET_TOLERANCE) && near(state.inset.right, INSET, INSET_TOLERANCE) && near(state.inset.bottom, INSET, INSET_TOLERANCE),
    border: state.borderWidths.every((width) => width === "1px") && state.borderStyles.every((style) => style === "solid")
      && state.borderColors.every((color) => color === state.resolved.rule),
    radius: state.radii.every((radius) => radius === "4px"),
    background: state.background === state.resolved.bg,
    play: state.play !== null && near(state.play.width, PLAY_SIZE, 0.5) && near(state.play.height, PLAY_SIZE, 0.5),
    playCentred: near(playCentre, rowCentre ?? Number.NaN, 0.5),
    skips: state.back && state.forward,
    clock: CLOCK.test(state.clock ?? ""),
    select: state.selects === 1,
    duration: near(state.duration, DURATION, 0.5),
    marks: expectedMarks !== null && state.marks.length === SECTION_STARTS.length
      && state.marks.every((mark, at) => near(mark.centre, expectedMarks[at], MARK_TOLERANCE)),
  };
  return {
    expectedMarks,
    playCentre: round(playCentre),
    rowCentre: round(rowCentre),
    failed: Object.entries(parts).filter(([, ok]) => !ok).map(([name]) => name),
    matches: Object.values(parts).every(Boolean),
  };
}

/** Waits as `waitFor` does, but answers whether the wait held rather than throwing, so a claim keeps what it measured. */
async function waited(cdp, expression, description, timeout) {
  try {
    await waitFor(cdp, expression, description, timeout);
    return true;
  } catch {
    return false;
  }
}

/** What claim 1 needs of one scheme's column, judged part by part. */
function judgeColumn(state) {
  const head = state.head;
  const parts = {
    sticky: head !== null && head.position === "sticky" && head.top === "0px",
    heading: head !== null && head.headings.length === 1 && head.headings[0].text.toLowerCase() === "transcript"
      && head.headings[0].drawn === "uppercase",
    find: head !== null && head.buttons.length === 1 && head.buttons[0].label === "Find in transcript" && head.buttons[0].disabled === true,
    sections: state.toggles.length === SECTIONS.length && state.toggles.every((toggle, at) => toggle.expanded === "true"
      && toggle.title === SECTIONS[at].title && toggle.count === `${SECTIONS[at].lines} lines`),
    turns: state.markers.length === TURNS,
    markers: state.markers.length > 0 && state.markers.every((marker) => marker !== null && near(marker.width, MARKER, 0.01) && near(marker.height, MARKER, 0.01)),
    lines: state.lines === LINES,
    noRowPlays: state.rowPlays === 0,
    corners: state.rounded.length === 0,
    weight: state.heavy.length === 0,
  };
  return { failed: Object.entries(parts).filter(([, ok]) => !ok).map(([name]) => name), matches: Object.values(parts).every(Boolean) };
}

const SETUP_NOTE = "Each check starts from a fresh load at 1440x900 on Notes with fixture note 106 opened from the library, its transcript in the side column and its 690 s recording's metadata loaded, and measures in both schemes (prefers-color-scheme emulated over CDP, data-theme set to match). Presses are real CDP pointer presses.";

export async function runTranscriptColumn(options) {
  const out = resolve(options.out);
  await mkdir(out, { recursive: true });
  let browser;
  let cdp;
  try {
    browser = await launchEdge({ ...options, out, extraArgs: EDGE_ARGS });
    cdp = await connectCdp(browser.target.webSocketDebuggerUrl);
    await cdp.send("Page.enable");
    await cdp.send("Page.addScriptToEvaluateOnNewDocument", { source: COUNT_PLAYS });
    const report = createReport({
      url: options.url,
      edgeVersion: browser.version.Browser ?? browser.version,
      edgeArgs: EDGE_ARGS,
      fixtureBannerRemoved: true,
      timestamp: new Date().toISOString(),
      measurementScope: "measured in headless Edge against dev fixtures, not in the Tauri window",
    });

    /** Runs one check; a step that throws is the check measured false, with the card as it was. */
    const check = async ({ id, claim, expectation }, run) => {
      try {
        const result = await run();
        addCheck(report, { id, claim, expectation, note: SETUP_NOTE, ...result });
      } catch (error) {
        let card = null;
        try { card = await cdp.evaluate(snippets.playerCardState()); } catch { /* no card to read */ }
        addCheck(report, { id, claim, expectation, measured: { failedAt: error.message, card }, matches: false, note: SETUP_NOTE });
      } finally {
        await cdp.send("Emulation.setEmulatedMedia", { features: [] }).catch(() => undefined);
      }
    };

    // 1. A plain sticky header, then sections and turns.
    await check({
      id: "1",
      claim: "The column has a plain header and is grouped into sections and turns.",
      expectation: [
        "in each scheme: .transcript-head is position: sticky at top 0px and holds exactly one heading, reading \"Transcript\"",
        "(case-insensitive) and drawn uppercase, and exactly one button, aria-label \"Find in transcript\", disabled; exactly",
        `${SECTIONS.length} button.transcript-section-toggle, each aria-expanded="true", titled ${SECTIONS.map((section) => `"${section.title}"`).join(", ")}`,
        `and counting ${SECTIONS.map((section) => `"${section.lines} lines"`).join(", ")} (transcriptSections(longTranscriptRows()));`,
        `${TURNS} .transcript-turn-head, each with a ${MARKER}x${MARKER}px .transcript-turn-marker; ${LINES} lines; no .transcript-play`,
        "(a per-line Play button) anywhere in .note-side; over .note-side .transcript",
        "and every element in it except controls (button, input, select, textarea, mark, .transcript-word), every computed",
        "corner radius 0px; and over all of it no computed font-weight above 400",
      ].join(" "),
    }, async () => {
      await openNote(cdp);
      const schemes = {};
      const screenshots = [];
      for (const scheme of SCHEMES) {
        const drawn = await useScheme(cdp, scheme);
        const state = await cdp.evaluate(snippets.transcriptColumnState());
        schemes[scheme] = { drawn, ...judgeColumn(state), state };
        const shot = `01-column-${scheme}-1440x900.png`;
        await cdp.screenshot(resolve(out, shot));
        screenshots.push(shot);
      }
      return {
        measured: { schemes },
        matches: SCHEMES.every((scheme) => schemes[scheme].matches),
        screenshots,
      };
    });

    // 2. The player is a card floating in the column.
    await check({
      id: "2",
      claim: "The player in the transcript column is a floating card.",
      expectation: [
        `in each scheme: .note-side-player inset ${INSET}px (±${INSET_TOLERANCE}) from .note-side's left, right and bottom edges`,
        "(its padding box, inside the column's left rule); a 1px solid border in the resolved --rule on all four sides, a 4px radius",
        `on all four corners and the resolved --bg behind it; inside it a ${PLAY_SIZE}x${PLAY_SIZE} button[aria-label="Play"] whose centre is`,
        "within 0.5px of its .recording-controls row's, button[aria-label=\"Back 15 seconds\"] and button[aria-label=\"Forward 15 seconds\"],",
        "a .recording-clock reading m:ss / m:ss, exactly one select, and exactly 2 .recording-mark whose centres sit within",
        `${MARK_TOLERANCE}px of 5px + (range width - 10px) x 300/690 and x 600/690 from the range input's left; the recording ${DURATION}s long;`,
        "a button.follow-chip reading \"Follow\", aria-pressed=\"true\" (on by default), its computed background the resolved --band;",
        "the two schemes resolve different --bg. Then, paused at 100 s, Forward 15 seconds lands on 115 and Back 15 seconds on 100 (±0.1 s)",
      ].join(" "),
    }, async () => {
      await openNote(cdp);
      const schemes = {};
      const screenshots = [];
      for (const scheme of SCHEMES) {
        const drawn = await useScheme(cdp, scheme);
        const state = await cdp.evaluate(snippets.playerCardState());
        const judged = judgeCard(state);
        const follow = await cdp.evaluate(snippets.followChipState(CARD));
        const followOn = follow.present && follow.text === "Follow" && follow.pressed === "true" && follow.drawn?.background === follow.band;
        if (!followOn) judged.failed.push("follow");
        judged.matches = judged.matches && followOn;
        schemes[scheme] = { drawn, ...judged, follow, state };
        const shot = `02-card-${scheme}-1440x900.png`;
        await cdp.screenshot(resolve(out, shot));
        const zoom = `02-card-${scheme}-zoom.png`;
        const { left, top, width, height } = state.card;
        await cdp.screenshot(resolve(out, zoom), { clip: { x: left - INSET, y: top - INSET, width: width + 2 * INSET, height: height + 2 * INSET }, scale: 3 });
        screenshots.push(shot, zoom);
      }
      const card = schemes.dark.state;
      let skip = null;
      if (card.back && card.forward) {
        await cdp.evaluate(snippets.pauseRecordingAt(CARD, SKIP_FROM));
        await waitFor(cdp, `(() => { const at = ${snippets.recordingAt(CARD)}; return at.paused && !at.seeking && Math.abs(at.time - ${SKIP_FROM}) < 0.01; })()`, `the recording paused at ${SKIP_FROM} s`);
        await pressAt(cdp, `${CARD} button[aria-label="Forward 15 seconds"]`);
        const forward = await settledTime(cdp);
        await pressAt(cdp, `${CARD} button[aria-label="Back 15 seconds"]`);
        const back = await settledTime(cdp);
        skip = { forward: { time: round(forward.time), paused: forward.paused }, back: { time: round(back.time), paused: back.paused } };
      }
      const skipped = skip !== null && near(skip.forward.time, SKIP_FROM + 15, SKIP_TOLERANCE) && near(skip.back.time, SKIP_FROM, SKIP_TOLERANCE)
        && skip.forward.paused && skip.back.paused;
      const schemesDiffer = schemes.dark.state.resolved.bg !== schemes.light.state.resolved.bg;
      return {
        measured: { schemes, schemesDiffer, skip },
        matches: SCHEMES.every((scheme) => schemes[scheme].matches) && schemesDiffer && skipped,
        screenshots,
      };
    });

    // 3. Scrolled to its end, the transcript's last line clears the card; and so does a line the
    // column scrolls into view because it started playing.
    await check({
      id: "3",
      claim: "The last transcript line is never hidden behind the card, and nor is the line the column scrolls to as it plays.",
      expectation: [
        "in each scheme, with .note-side-body scrolled to its end (scrollTop within 1px of its maximum): the card's top is above",
        "the scroller's bottom (it floats over the scroller, so there is a line to hide), and the last .transcript-rows li's bottom",
        `edge is at or above the card's top edge. Then, scrolled back to the top, the recording paused at ${PLAY_FROM} s and the card's`,
        "Play pressed: the line marked aria-current, which the column scrolled to (scrollTop above 0), has its bottom edge at or",
        "above the card's top edge and its top edge at or below the bottom of the column's .transcript-head (the scroller's top",
        "when there is no head)",
      ].join(" "),
    }, async () => {
      await openNote(cdp);
      const schemes = {};
      const screenshots = [];
      for (const scheme of SCHEMES) {
        const drawn = await useScheme(cdp, scheme);
        const scrolled = await cdp.evaluate(snippets.scrollSideToEnd());
        await delay(200);
        const state = await cdp.evaluate(snippets.lastLineState());
        const atEnd = near(state.scrollTop, state.maxScroll, 1) && state.maxScroll > 0;
        const floats = state.cardTop < state.scroller.bottom;
        const clear = state.lastBottom <= state.cardTop;
        const shot = `03-end-${scheme}-1440x900.png`;
        await cdp.screenshot(resolve(out, shot));
        screenshots.push(shot);

        // The line playing, scrolled to by the column itself (`TranscriptPanel`'s active-line scroll).
        await cdp.evaluate(`document.querySelector(${JSON.stringify(SIDE_BODY)}).scrollTo({ top: 0, behavior: 'instant' })`);
        await cdp.evaluate(snippets.pauseRecordingAt(CARD, PLAY_FROM));
        await settledTime(cdp);
        await pressAt(cdp, `${CARD} button[aria-label="Play"]`);
        const started = await waited(cdp, "Boolean(document.querySelector('.note-side .transcript-rows li[aria-current=\"true\"]'))", "a line to play", 5_000);
        await delay(700);
        const line = await cdp.evaluate(snippets.playingLineState());
        const playShot = `03-playing-${scheme}-1440x900.png`;
        await cdp.screenshot(resolve(out, playShot));
        screenshots.push(playShot);
        await stopPlaying(cdp);
        const playing = {
          started,
          ...line,
          top: round(line.top),
          bottom: round(line.bottom),
          clearTop: round(line.clearTop),
          cardTop: round(line.cardTop),
          gapToCard: line.bottom === null ? null : round(line.cardTop - line.bottom),
          scrolledTo: line.scrollTop > 0,
          aboveCard: line.bottom !== null && line.bottom <= line.cardTop,
          belowHead: line.top !== null && line.top >= line.clearTop,
        };
        playing.matches = started && playing.scrolledTo && playing.aboveCard && playing.belowHead;
        schemes[scheme] = {
          drawn,
          scrolled,
          ...state,
          lastBottom: round(state.lastBottom),
          cardTop: round(state.cardTop),
          gap: round(state.cardTop - state.lastBottom),
          atEnd,
          floats,
          clear,
          playing,
          matches: atEnd && floats && clear && playing.matches,
        };
      }
      return {
        measured: { schemes },
        matches: SCHEMES.every((scheme) => schemes[scheme].matches),
        screenshots,
      };
    });

    // 4. A press on a line plays from its start, selects it alone, and names its speaker on the card.
    await check({
      id: "4",
      claim: "Clicking a line plays from its start.",
      expectation: [
        `in each scheme, from a fresh load with nothing played: a real press in line ${CLICK_LINE + 1}'s left padding (linePadPoint: on the line`,
        `itself, not a word) makes exactly 1 play() call; once the recording plays (the first animation frame playing after its seek) it is`,
        `within ${LANDING_TOLERANCE}s of the line's start; that line is the only aria-selected="true" line; and the card's .player-header`,
        `names "${CLICK_SPEAKER}", the line's speaker, beside its 6x6px marker in the resolved --band. Pressed again about 1s later: exactly 1`,
        `more play() call, the recording back within ${LANDING_TOLERANCE}s of the line's start and playing, and that line still the only one selected.`,
        "Then, paused, a real drag inside that line's text (the left button pressed 2px into its first line box, moved up to 160px along it",
        "and released, lineTextDrag) leaves a text selection that reaches the line and makes no play() call",
      ].join(" "),
    }, async () => {
      const schemes = {};
      const screenshots = [];
      for (const scheme of SCHEMES) {
        await openNote(cdp);
        const drawn = await useScheme(cdp, scheme);
        const rows = await cdp.evaluate(snippets.noteTranscriptRows(NOTE_ID));
        const start = rows[CLICK_LINE].startSeconds;
        const first = await played(cdp, () => pressLine(cdp, CLICK_LINE));
        const header = await cdp.evaluate(snippets.cardHeaderState(CARD));
        const shot = `04-click-${scheme}-1440x900.png`;
        await cdp.screenshot(resolve(out, shot));
        screenshots.push(shot, await cardShot(cdp, out, `04-card-speaker-${scheme}-zoom.png`));
        await delay(1000);
        const second = await played(cdp, () => pressLine(cdp, CLICK_LINE));
        await stopPlaying(cdp);
        // Selecting a quote to copy it is not asking to listen (KNEG-448, Task 7's review).
        const playsBeforeDrag = await cdp.evaluate("window.__plays ?? null");
        const across = await cdp.evaluate(snippets.lineTextDrag(CLICK_LINE));
        await drag(cdp, across.from, across.to);
        const selectedText = await cdp.evaluate(snippets.textSelectionState());
        const dragPlays = (await cdp.evaluate("window.__plays ?? null")) - playsBeforeDrag;
        await delay(300);
        await cdp.evaluate("(() => { document.querySelector('.note-side .note-side-player audio')?.pause(); getSelection()?.removeAllRanges(); return true; })()");
        const only = (lines) => JSON.stringify(lines.selected) === JSON.stringify([CLICK_LINE]);
        const parts = {
          onePlay: first.plays === 1,
          landed: first.landed && near(first.playingAt, start, LANDING_TOLERANCE),
          onlySelected: only(first.lines),
          speaker: header.speaker === CLICK_SPEAKER,
          marker: header.marker !== null && near(header.marker.width, MARKER, 0.01) && near(header.marker.height, MARKER, 0.01) && header.marker.fill === header.band,
          againPlays: second.plays === 1,
          againLanded: second.landed && near(second.playingAt, start, LANDING_TOLERANCE),
          stillSelected: only(second.lines),
          dragOnText: across.onText,
          dragSelected: !selectedText.collapsed && selectedText.text.length > 0 && selectedText.lines.includes(CLICK_LINE),
          dragNoPlay: dragPlays === 0,
        };
        schemes[scheme] = {
          drawn,
          start,
          first,
          header,
          second,
          drag: { from: across.from, to: across.to, onText: across.onText, selected: selectedText, plays: dragPlays },
          failed: Object.entries(parts).filter(([, ok]) => !ok).map(([name]) => name),
          matches: Object.values(parts).every(Boolean),
        };
      }
      return {
        measured: { schemes },
        matches: SCHEMES.every((scheme) => schemes[scheme].matches),
        screenshots,
      };
    });

    // 5. While Follow is on, each line that starts playing is in view.
    await check({
      id: "5",
      claim: "Following keeps the playing line in view.",
      expectation: [
        `in each scheme, from a fresh load, at ${FOLLOW_RATE}x: a real press in line ${FOLLOW_LINE + 1}'s left padding (4:35, brought to the foot of the band by`,
        `linePadPoint) plays it, and then, for it and each of the next ${FOLLOW_CHANGES} lines to become aria-current="true" (watchPlayingLines, read`,
        `${SETTLE_MS} ms after each starts): the line is drawn, its top at or below the bottom of the column's .transcript-head and its bottom at or`,
        "above the card's top (±0.5px), and the card's button.follow-chip is aria-pressed=\"true\". The column scrolled for at least one of the",
        "new lines (its settled scrollTop differs from the line before's), so the lines did not start in view",
      ].join(" "),
    }, async () => {
      const schemes = {};
      const screenshots = [];
      for (const scheme of SCHEMES) {
        await openNote(cdp);
        const drawn = await useScheme(cdp, scheme);
        await cdp.evaluate(snippets.choosePlayerSpeed(CARD, FOLLOW_RATE));
        const first = await played(cdp, () => pressLine(cdp, FOLLOW_LINE));
        await cdp.evaluate(snippets.watchPlayingLines(SIDE_BODY, CARD, SETTLE_MS));
        const crossed = await waited(
          cdp,
          `(window.__playingLines?.changes ?? []).filter((change) => change.settled !== null).length >= ${FOLLOW_CHANGES + 1}`,
          `${FOLLOW_CHANGES} line changes`,
          30_000,
        );
        const record = await cdp.evaluate(snippets.playingLinesRecord(true));
        const shot = `05-following-${scheme}-1440x900.png`;
        await cdp.screenshot(resolve(out, shot));
        screenshots.push(shot);
        await stopPlaying(cdp);
        await cdp.evaluate(snippets.choosePlayerSpeed(CARD, "1"));
        const lines = (record?.changes ?? []).filter((change) => change.settled !== null).map(bandLine);
        const fresh = lines.slice(1);
        const parts = {
          started: first.landed && first.plays === 1,
          crossed: crossed && fresh.length >= FOLLOW_CHANGES,
          inBand: lines.length > 0 && lines.every((line) => line.inBand),
          followOn: lines.length > 0 && lines.every((line) => line.follow === "true"),
          // Against the line before's settled place: when a scroll starts, in the same commit or
          // the next frame, is not what this claims.
          moved: fresh.some((line, at) => Math.abs(line.scrollTop - lines[at].scrollTop) > 1),
        };
        schemes[scheme] = {
          drawn,
          first: { plays: first.plays, landed: first.landed, playingAt: first.playingAt },
          lines,
          failed: Object.entries(parts).filter(([, ok]) => !ok).map(([name]) => name),
          matches: Object.values(parts).every(Boolean),
        };
      }
      return {
        measured: { schemes },
        matches: SCHEMES.every((scheme) => schemes[scheme].matches),
        screenshots,
      };
    });

    // 6. A wheel lets go of the playing line; pressing Follow brings it back; PageDown lets go too.
    await check({
      id: "6",
      claim: "Scrolling turns Follow off; pressing Follow brings the line back.",
      expectation: [
        `in each scheme, from a fresh load, at ${FOLLOW_RATE}x: line ${FOLLOW_LINE + 1} pressed in its padding (it plays), then, while it plays, a real`,
        `mouseWheel of ${WHEEL}px up (Input.dispatchMouseEvent, deltaY -${WHEEL}) over .note-side-body: the column scrolls up and the card's`,
        "button.follow-chip goes to aria-pressed=\"false\"; at the next line to start playing, scrollTop is where the wheel left it (±1px), as",
        `the line starts and ${SETTLE_MS} ms later. Then, at 1x, a real press on the chip: it is aria-pressed="true" and, once the column has`,
        `stopped scrolling, the playing line's centre is within ±${CENTRE_TOLERANCE}px of the visible band's (followBandState: the scroller inside its`,
        "border, less the sticky head's height at the top and --player-h + 24px at the bottom). Then a real press on the column's label",
        "(not a control), focus moved to <body> and a real PageDown: the chip goes to aria-pressed=\"false\". Then the chip pressed on",
        "again, a real press in the note column's left margin (blankPoint: on no control) and, with focus on <body>, a real PageDown,",
        "which scrolls the note rather than the transcript: the chip stays aria-pressed=\"true\"",
      ].join(" "),
    }, async () => {
      const schemes = {};
      const screenshots = [];
      for (const scheme of SCHEMES) {
        await openNote(cdp);
        const drawn = await useScheme(cdp, scheme);
        await cdp.evaluate(snippets.choosePlayerSpeed(CARD, FOLLOW_RATE));
        const first = await played(cdp, () => pressLine(cdp, FOLLOW_LINE));
        // Past the 400 ms a wheel is not counted after Follow turns on, and past the press's own scroll.
        await delay(600);
        const before = await scrollSettled(cdp, SIDE_BODY);
        const point = await cdp.evaluate(snippets.scrollerPoint(SIDE_BODY));
        await wheel(cdp, point, -WHEEL);
        const afterWheel = await scrollSettled(cdp, SIDE_BODY);
        const released = await cdp.evaluate(snippets.followChipState(CARD));
        const playingThen = (await cdp.evaluate(snippets.lineModelState())).playing[0] ?? null;
        await cdp.evaluate(snippets.watchPlayingLines(SIDE_BODY, CARD, SETTLE_MS));
        const changed = await waited(
          cdp,
          `(window.__playingLines?.changes ?? []).some((change) => change.index !== ${JSON.stringify(playingThen)} && change.settled !== null)`,
          "the next line to play",
          20_000,
        );
        const record = await cdp.evaluate(snippets.playingLinesRecord(true));
        const next = (record?.changes ?? []).find((change) => change.index !== playingThen && change.settled !== null) ?? null;
        const offShot = `06-let-go-${scheme}-1440x900.png`;
        await cdp.screenshot(resolve(out, offShot));
        screenshots.push(offShot, await cardShot(cdp, out, `06-card-follow-off-${scheme}-zoom.png`));
        await cdp.evaluate(snippets.choosePlayerSpeed(CARD, "1"));
        const chip = await cdp.evaluate(snippets.followChipState(CARD));
        let pressedAgain = null;
        let centred = null;
        if (chip.point !== null) {
          await pressPoint(cdp, chip.point, "the Follow chip");
          pressedAgain = await cdp.evaluate(snippets.followChipState(CARD));
          await delay(150);
          await scrollSettled(cdp, SIDE_BODY);
          centred = await cdp.evaluate(snippets.followBandState(SIDE_BODY));
          const onShot = `06-follow-again-${scheme}-1440x900.png`;
          await cdp.screenshot(resolve(out, onShot));
          screenshots.push(onShot, await cardShot(cdp, out, `06-card-follow-on-${scheme}-zoom.png`));
        }
        // With focus on nothing, a key scrolls the scroller pressed last: here the column, by its label.
        await pressAt(cdp, ".note-side .transcript-head .transcript-label");
        const onBody = await cdp.evaluate(snippets.blurActive());
        await key(cdp, "PageDown");
        const afterKey = await cdp.evaluate(snippets.followChipState(CARD));
        // And here the note: PageDown scrolls the note column, not the transcript, so Follow stays on.
        let elsewhere = null;
        const offAgain = await cdp.evaluate(snippets.followChipState(CARD));
        if (offAgain.point !== null) {
          await pressPoint(cdp, offAgain.point, "the Follow chip");
          const on = (await cdp.evaluate(snippets.followChipState(CARD))).pressed;
          await delay(150);
          await scrollSettled(cdp, SIDE_BODY);
          const noteBefore = await cdp.evaluate("document.querySelector('.note-detail .note-body').scrollTop");
          const blank = await cdp.evaluate(snippets.blankPoint(".note-detail .note-body"));
          await pressPoint(cdp, blank, "the note column's margin");
          const bodyFocused = await cdp.evaluate(snippets.blurActive());
          await key(cdp, "PageDown", { settle: 400 });
          const noteAfter = await cdp.evaluate("document.querySelector('.note-detail .note-body').scrollTop");
          elsewhere = { on, blank, onBody: bodyFocused, chip: (await cdp.evaluate(snippets.followChipState(CARD))).pressed, noteScrolled: round(noteAfter - noteBefore) };
        }
        await stopPlaying(cdp);
        const parts = {
          started: first.landed && first.plays === 1,
          wheelScrolled: typeof before === "number" && typeof afterWheel === "number" && before - afterWheel > 0 && point.inside,
          released: released.pressed === "false",
          nextLine: changed && next !== null,
          stayed: next !== null && Math.abs(next.scrollTop - afterWheel) <= 1 && Math.abs(next.settled.scrollTop - afterWheel) <= 1,
          pressedAgain: pressedAgain?.pressed === "true",
          centred: centred !== null && centred.offset !== null && Math.abs(centred.offset) <= CENTRE_TOLERANCE,
          onBody,
          pageDownReleased: afterKey.pressed === "false",
          noteKeyKeeps: elsewhere !== null && elsewhere.on === "true" && elsewhere.blank.plain && elsewhere.onBody && elsewhere.chip === "true",
        };
        schemes[scheme] = {
          drawn,
          first: { plays: first.plays, landed: first.landed, playingAt: first.playingAt },
          wheel: { point, before: round(before), after: round(afterWheel), moved: round(before - afterWheel), chip: released.pressed },
          next: next === null ? null : { index: next.index, scrollTopAtStart: round(next.scrollTop), scrollTopSettled: round(next.settled.scrollTop), follow: next.settled.follow },
          pressedAgain: pressedAgain?.pressed ?? null,
          centred: centred === null ? null : { ...centred, offset: round(centred.offset) },
          pageDown: { onBody, chip: afterKey.pressed },
          notePageDown: elsewhere,
          failed: Object.entries(parts).filter(([, ok]) => !ok).map(([name]) => name),
          matches: Object.values(parts).every(Boolean),
        };
      }
      return {
        measured: { schemes },
        matches: SCHEMES.every((scheme) => schemes[scheme].matches),
        screenshots,
      };
    });

    // 7. Following opens a collapsed section the playing line is in; collapsing that section lets go.
    await check({
      id: "7",
      claim: "Following opens a collapsed section; collapsing the playing section lets go.",
      expectation: [
        `in each scheme, from a fresh load: a real press on the "${SECOND_SECTION}" toggle leaves it aria-expanded="false" and the card's`,
        `button.follow-chip aria-pressed="true"; then, the recording paused at ${CROSS_FROM} s, at ${FOLLOW_RATE}x and the card's Play pressed, once playback`,
        `crosses into that section (a line from line ${SECOND_SECTION_FIRST + 1} on becomes aria-current, read ${SETTLE_MS} ms later): the toggle is`,
        "aria-expanded=\"true\" and that line is drawn, its top at or below the head's bottom and its bottom at or above the card's top. Then a",
        "real press on that toggle, collapsing the section the playing line is in: the toggle is aria-expanded=\"false\" and the chip",
        "aria-pressed=\"false\", and 1.2 s later the section is still closed",
      ].join(" "),
    }, async () => {
      const schemes = {};
      const screenshots = [];
      for (const scheme of SCHEMES) {
        await openNote(cdp);
        const drawn = await useScheme(cdp, scheme);
        const toggle = await cdp.evaluate(snippets.sectionToggleState(SECOND_SECTION));
        await pressPoint(cdp, toggle, `the "${SECOND_SECTION}" toggle`);
        const closed = await cdp.evaluate(snippets.sectionExpanded(SECOND_SECTION));
        const followBefore = await cdp.evaluate(snippets.followChipState(CARD));
        await cdp.evaluate(snippets.pauseRecordingAt(CARD, CROSS_FROM));
        await settledTime(cdp);
        await cdp.evaluate(snippets.choosePlayerSpeed(CARD, FOLLOW_RATE));
        await pressAt(cdp, `${CARD} button[aria-label="Play"]`);
        await cdp.evaluate(snippets.watchPlayingLines(SIDE_BODY, CARD, SETTLE_MS));
        const crossed = await waited(
          cdp,
          `(window.__playingLines?.changes ?? []).some((change) => change.index >= ${SECOND_SECTION_FIRST} && change.settled !== null)`,
          `a line from line ${SECOND_SECTION_FIRST + 1} on to play`,
          15_000,
        );
        const record = await cdp.evaluate(snippets.playingLinesRecord(true));
        const into = (record?.changes ?? []).find((change) => change.index >= SECOND_SECTION_FIRST && change.settled !== null) ?? null;
        const opened = await cdp.evaluate(snippets.sectionExpanded(SECOND_SECTION));
        const shot = `07-opened-${scheme}-1440x900.png`;
        await cdp.screenshot(resolve(out, shot));
        screenshots.push(shot);
        const again = await cdp.evaluate(snippets.sectionToggleState(SECOND_SECTION));
        await pressPoint(cdp, again, `the "${SECOND_SECTION}" toggle`);
        const letGo = { expanded: await cdp.evaluate(snippets.sectionExpanded(SECOND_SECTION)), follow: (await cdp.evaluate(snippets.followChipState(CARD))).pressed };
        const letGoShot = `07-let-go-${scheme}-1440x900.png`;
        await cdp.screenshot(resolve(out, letGoShot));
        screenshots.push(letGoShot);
        await delay(1200);
        const later = await cdp.evaluate(snippets.sectionExpanded(SECOND_SECTION));
        await stopPlaying(cdp);
        await cdp.evaluate(snippets.choosePlayerSpeed(CARD, "1"));
        const line = into === null ? null : bandLine(into);
        const parts = {
          closedFirst: toggle.onToggle && closed === "false",
          followOn: followBefore.pressed === "true",
          crossed: crossed && into !== null,
          opened: opened === "true",
          inView: line !== null && line.inBand,
          collapsedAgain: again.onToggle && letGo.expanded === "false",
          letGo: letGo.follow === "false",
          stayedClosed: later === "false",
        };
        schemes[scheme] = {
          drawn,
          closed,
          followBefore: followBefore.pressed,
          line,
          opened,
          letGo,
          later,
          failed: Object.entries(parts).filter(([, ok]) => !ok).map(([name]) => name),
          matches: Object.values(parts).every(Boolean),
        };
      }
      return {
        measured: { schemes },
        matches: SCHEMES.every((scheme) => schemes[scheme].matches),
        screenshots,
      };
    });

    // 8. Shift selects a range of lines, which the card offers to play; Escape on a line clears it.
    await check({
      id: "8",
      claim: "Shift-click selects a range, and the card offers to play it.",
      expectation: [
        `in each scheme, from a fresh load: line ${RANGE_FROM + 1} pressed in its padding (it plays) and paused, then line ${RANGE_TO + 1}, two playable lines`,
        `later, pressed with Shift held: exactly lines ${RANGE_LINES.map((line) => line + 1).join(", ")} are aria-selected="true", the Shift press makes no play() call,`,
        "and the card's .player-header holds a button reading \"Play selection (a–c)\" -- line 6's start to line 8's end, an en dash between --",
        `and a "Clear selection" button. That button pressed, with the speed at ${RANGE_RATE}x: the recording plays and pauses within ${STOP_TOLERANCE}s of`,
        `line ${RANGE_TO + 1}'s end. Then, with focus on line ${RANGE_TO + 1}, a real Escape leaves no line selected and the header names a speaker again,`,
        "with no Play selection button",
      ].join(" "),
    }, async () => {
      const schemes = {};
      const screenshots = [];
      for (const scheme of SCHEMES) {
        await openNote(cdp);
        const drawn = await useScheme(cdp, scheme);
        const rows = await cdp.evaluate(snippets.noteTranscriptRows(NOTE_ID));
        const from = rows[RANGE_FROM];
        const to = rows[RANGE_TO];
        const label = `Play selection (${clock(from.startSeconds)}–${clock(to.endSeconds)})`;
        const first = await played(cdp, () => pressLine(cdp, RANGE_FROM));
        await stopPlaying(cdp);
        const playsBefore = await cdp.evaluate("window.__plays ?? null");
        await pressLine(cdp, RANGE_TO, SHIFT);
        const ranged = await cdp.evaluate(snippets.lineModelState());
        const offered = await cdp.evaluate(snippets.cardHeaderState(CARD));
        const shiftPlays = (await cdp.evaluate("window.__plays ?? null")) - playsBefore;
        const shot = `08-selection-${scheme}-1440x900.png`;
        await cdp.screenshot(resolve(out, shot));
        screenshots.push(shot, await cardShot(cdp, out, `08-card-selection-${scheme}-zoom.png`));
        let stopped = null;
        if (offered.playSelection !== null) {
          await cdp.evaluate(snippets.choosePlayerSpeed(CARD, RANGE_RATE));
          await pressAt(cdp, `${CARD} button.player-header-play`);
          const started = await waited(cdp, playingNow, "the selection to play", 5_000);
          const span = (to.endSeconds - from.startSeconds) / Number(RANGE_RATE);
          const ended = started && await waited(cdp, `document.querySelector(${JSON.stringify(`${CARD} audio`)}).paused`, "the selection to stop", span * 1000 + 8_000);
          const at = await cdp.evaluate(snippets.recordingAt(CARD));
          stopped = { started, ended, time: round(at.time), fromEnd: round(at.time - to.endSeconds) };
          await cdp.evaluate(snippets.choosePlayerSpeed(CARD, "1"));
        }
        const focused = await cdp.evaluate(focusLine(RANGE_TO));
        await escape(cdp);
        const cleared = await cdp.evaluate(snippets.lineModelState());
        const back = await cdp.evaluate(snippets.cardHeaderState(CARD));
        const parts = {
          firstPlayed: first.landed && first.plays === 1,
          threeSelected: JSON.stringify(ranged.selected) === JSON.stringify(RANGE_LINES),
          noPlayOnShift: shiftPlays === 0,
          offered: offered.playSelection === label && offered.clear === "Clear selection",
          stoppedAtEnd: stopped !== null && stopped.started && stopped.ended && Math.abs(stopped.fromEnd) <= STOP_TOLERANCE,
          escapeOnLine: focused,
          cleared: cleared.selected.length === 0,
          speakerBack: back.playSelection === null && back.speaker !== null,
        };
        schemes[scheme] = {
          drawn,
          label,
          first: { landed: first.landed, plays: first.plays, playingAt: first.playingAt },
          ranged: { selected: ranged.selected, shiftPlays },
          offered,
          stopped,
          afterEscape: { focusedFirst: focused, selected: cleared.selected, header: back },
          failed: Object.entries(parts).filter(([, ok]) => !ok).map(([name]) => name),
          matches: Object.values(parts).every(Boolean),
        };
      }
      return {
        measured: { schemes },
        matches: SCHEMES.every((scheme) => schemes[scheme].matches),
        screenshots,
      };
    });

    // 11. A turn's name opens the rename form under that turn, and nothing else happens.
    await check({
      id: "11",
      claim: "The rename form opens under the turn whose name was pressed.",
      expectation: [
        `in each scheme: a real press on the first button.transcript-speaker reading "${RENAME_NAME}" makes exactly one`,
        "form.transcript-rename, inside the same .transcript-turn as that button, with focus in its text input; the count of",
        "play() calls is the same as before the press, and every line's aria-selected is as it was. Escape closes it (no",
        "form.transcript-rename left)",
      ].join(" "),
    }, async () => {
      await openNote(cdp);
      const schemes = {};
      const screenshots = [];
      for (const scheme of SCHEMES) {
        const drawn = await useScheme(cdp, scheme);
        const before = { plays: await cdp.evaluate("window.__plays ?? null"), selection: await cdp.evaluate(snippets.lineSelection()) };
        const target = await cdp.evaluate(snippets.markSpeakerButton(RENAME_NAME));
        await pressPoint(cdp, target, `the ${RENAME_NAME} button`);
        const opened = await waited(cdp, "Boolean(document.querySelector('.note-detail form.transcript-rename'))", "the rename form", 3_000);
        const form = await cdp.evaluate(snippets.renameFormState());
        const after = { plays: await cdp.evaluate("window.__plays ?? null"), selection: await cdp.evaluate(snippets.lineSelection()) };
        const shot = `11-rename-${scheme}-1440x900.png`;
        await cdp.screenshot(resolve(out, shot));
        screenshots.push(shot);
        await escape(cdp);
        const closed = await waited(cdp, "!document.querySelector('.note-detail form.transcript-rename')", "the rename form to close", 3_000);
        const parts = {
          opened: opened && form.forms === 1,
          inPressedTurn: form.inPressedTurn,
          focusInInput: form.focusInInput,
          noPlay: typeof before.plays === "number" && after.plays === before.plays,
          selectionKept: before.selection.length > 0 && JSON.stringify(after.selection) === JSON.stringify(before.selection),
          closed,
        };
        schemes[scheme] = {
          drawn,
          target,
          form,
          plays: { before: before.plays, after: after.plays },
          selected: { before: before.selection.filter((value) => value === "true").length, after: after.selection.filter((value) => value === "true").length },
          failed: Object.entries(parts).filter(([, ok]) => !ok).map(([name]) => name),
          matches: Object.values(parts).every(Boolean),
        };
      }
      return {
        measured: { schemes },
        matches: SCHEMES.every((scheme) => schemes[scheme].matches),
        screenshots,
      };
    });

    // 12. The lines are one tab stop, and the arrow keys move between them.
    await check({
      id: "12",
      claim: "One tab stop, and the arrow keys move between lines.",
      expectation: [
        "in each scheme, from a fresh load with nothing selected or played: exactly one transcript line has tabindex=\"0\"; after a real press",
        `on the column's label, real Tabs (at most ${TAB_LIMIT}) walk into the transcript and out of it, and the only line focus lands on is`,
        "that one. From it, three real ArrowDowns focus the next three lines in turn. With focus on line",
        `${BEFORE_UNTIMED + 1}, the line before note 106's untimed line ${UNTIMED + 1}, one ArrowDown focuses line ${AFTER_UNTIMED + 1}, which then holds the only tab stop.`,
        `A real Enter there makes exactly 1 play() call, the recording plays from within ${LANDING_TOLERANCE}s of that line's start, and it is the only selected line`,
      ].join(" "),
    }, async () => {
      const schemes = {};
      const screenshots = [];
      for (const scheme of SCHEMES) {
        await openNote(cdp);
        const drawn = await useScheme(cdp, scheme);
        const rows = await cdp.evaluate(snippets.noteTranscriptRows(NOTE_ID));
        const initial = await cdp.evaluate(snippets.lineModelState());
        await pressAt(cdp, ".note-side .transcript-head .transcript-label");
        const walk = [];
        let entered = false;
        let left = false;
        for (let step = 0; step < TAB_LIMIT; step += 1) {
          await key(cdp, "Tab", { settle: 30 });
          const place = await cdp.evaluate(snippets.focusPlace());
          walk.push(place);
          if (place.inTranscript) entered = true;
          else if (entered) { left = true; break; }
        }
        const tabbedLines = walk.filter((place) => place.line >= 0).map((place) => place.line);
        const stop = initial.stops[0] ?? null;
        const steps = [];
        if (stop !== null) {
          await cdp.evaluate(focusLine(stop));
          for (let step = 0; step < 3; step += 1) {
            await key(cdp, "ArrowDown");
            steps.push((await cdp.evaluate(snippets.lineModelState())).focused);
          }
        }
        const onBefore = await cdp.evaluate(focusLine(BEFORE_UNTIMED));
        await key(cdp, "ArrowDown");
        const past = await cdp.evaluate(snippets.lineModelState());
        const shot = `12-arrow-${scheme}-1440x900.png`;
        await cdp.screenshot(resolve(out, shot));
        screenshots.push(shot);
        const entered12 = await played(cdp, () => key(cdp, "Enter"));
        await stopPlaying(cdp);
        const parts = {
          oneStop: initial.stops.length === 1,
          walked: entered && left,
          oneLineByTab: tabbedLines.length === 1 && tabbedLines[0] === stop,
          lineByLine: stop !== null && JSON.stringify(steps) === JSON.stringify([stop + 1, stop + 2, stop + 3]),
          untimed: rows[UNTIMED].startSeconds === null && onBefore && past.focused === AFTER_UNTIMED
            && JSON.stringify(past.stops) === JSON.stringify([AFTER_UNTIMED]),
          enterPlays: entered12.plays === 1 && entered12.landed && near(entered12.playingAt, rows[AFTER_UNTIMED].startSeconds, LANDING_TOLERANCE)
            && JSON.stringify(entered12.lines.selected) === JSON.stringify([AFTER_UNTIMED]),
        };
        schemes[scheme] = {
          drawn,
          initialStops: initial.stops,
          tabs: walk.length,
          tabbedLines,
          walk: walk.map((place) => place.element),
          arrowSteps: steps,
          pastUntimed: { focused: past.focused, stops: past.stops, untimedStart: rows[UNTIMED].startSeconds },
          enter: { plays: entered12.plays, landed: entered12.landed, playingAt: entered12.playingAt, start: rows[AFTER_UNTIMED].startSeconds, selected: entered12.lines.selected },
          failed: Object.entries(parts).filter(([, ok]) => !ok).map(([name]) => name),
          matches: Object.values(parts).every(Boolean),
        };
      }
      return {
        measured: { schemes },
        matches: SCHEMES.every((scheme) => schemes[scheme].matches),
        screenshots,
      };
    });

    // 13. The header stays at the top of whatever scrolls the transcript, in both layouts.
    await check({
      id: "13",
      claim: "The header stays in view while the transcript scrolls (both layouts), and in a narrow window the mini card shows before playback on the Transcript tab.",
      expectation: [
        `in each scheme: at 1440x900, .note-side-body scrolled down by ${HEAD_SCROLL}px (from 0, and it moves the full ${HEAD_SCROLL}px),`,
        `.transcript-head's top within ${HEAD_TOLERANCE}px of the scroller's top inside its border; at 1000x720 on the Transcript tab,`,
        `.notes-main scrolled down by ${HEAD_SCROLL}px the same way, the tab's .transcript-head's top within ${HEAD_TOLERANCE}px of .notes-main's.`,
        "At 1000x720, from a fresh load with nothing played: on the Transcript tab, in each scheme, .note-mini-player is there, not hidden,",
        "drawn and carrying the card's .player-header and a button.follow-chip; after a real press on the Notes tab it is hidden and not drawn;",
        "no play() call up to there. Then (in the last scheme, light) back on the Transcript tab, a real press in a line's padding plays it,",
        "and after a real press on the Notes tab the mini card is shown, drawn and holds no button.follow-chip (nor the header)",
      ].join(" "),
    }, async () => {
      const judge = (read) => ({
        ...read,
        headTop: round(read.headTop),
        scrollerTop: round(read.scrollerTop),
        moved: near(read.after - read.before, HEAD_SCROLL, 1),
        matches: read.before === 0 && near(read.after - read.before, HEAD_SCROLL, 1) && near(read.headTop, read.scrollerTop, HEAD_TOLERANCE),
      });
      const wide = {};
      const narrow = {};
      const screenshots = [];
      await openNote(cdp);
      for (const scheme of SCHEMES) {
        const drawn = await useScheme(cdp, scheme);
        await cdp.evaluate(`document.querySelector(${JSON.stringify(SIDE_BODY)}).scrollTo({ top: 0, behavior: 'instant' })`);
        wide[scheme] = { drawn, ...judge(await cdp.evaluate(snippets.scrollAndReadHead(SIDE_BODY, HEAD_SCROLL))) };
        const shot = `13-wide-scrolled-${scheme}-1440x900.png`;
        await cdp.screenshot(resolve(out, shot));
        screenshots.push(shot);
      }
      await openNarrowTranscript(cdp);
      const mini = {};
      for (const scheme of SCHEMES) {
        const drawn = await useScheme(cdp, scheme);
        await cdp.evaluate("document.querySelector('.notes-main').scrollTo({ top: 0, behavior: 'instant' })");
        mini[scheme] = await cdp.evaluate(snippets.miniPlayerState());
        const topShot = `13-narrow-top-${scheme}-1000x720.png`;
        await cdp.screenshot(resolve(out, topShot));
        narrow[scheme] = { drawn, ...judge(await cdp.evaluate(snippets.scrollAndReadHead(".notes-main", HEAD_SCROLL))) };
        const shot = `13-narrow-scrolled-${scheme}-1000x720.png`;
        await cdp.screenshot(resolve(out, shot));
        screenshots.push(topShot, shot);
      }
      // Back to the top, where the tabs are.
      await cdp.evaluate("document.querySelector('.notes-main').scrollTo({ top: 0, behavior: 'instant' })");
      await pressAt(cdp, ".note-detail [role=\"tab\"]", "Notes");
      await waited(cdp, "document.querySelector('.note-detail [role=\"tab\"][aria-selected=\"true\"]')?.textContent.trim() === 'Notes'", "the Notes tab", 3_000);
      const onNotes = await cdp.evaluate(snippets.miniPlayerState());
      const plays = await cdp.evaluate("window.__plays ?? null");
      // Follow belongs to the transcript: once playing, the mini card on Notes carries no chip (KNEG-448).
      await pressAt(cdp, ".note-detail [role=\"tab\"]", "Transcript");
      await waited(cdp, "Boolean(document.querySelector('.note-detail [role=\"tabpanel\"]:not([hidden]) .transcript-rows li'))", "the Transcript tab's lines", 3_000);
      await pressLine(cdp, CLICK_LINE);
      const playingNarrow = await waited(cdp, notePlaying, "the recording to play", 5_000);
      await cdp.evaluate("document.querySelector('.notes-main').scrollTo({ top: 0, behavior: 'instant' })");
      await pressAt(cdp, ".note-detail [role=\"tab\"]", "Notes");
      await waited(cdp, "document.querySelector('.note-detail [role=\"tab\"][aria-selected=\"true\"]')?.textContent.trim() === 'Notes'", "the Notes tab", 3_000);
      const notesPlaying = await cdp.evaluate(snippets.miniPlayerState());
      const notesShot = "13-narrow-notes-playing-1000x720.png";
      await cdp.screenshot(resolve(out, notesShot));
      screenshots.push(notesShot);
      await cdp.evaluate("document.querySelector('.note-detail audio')?.pause()");
      const miniShown = SCHEMES.every((scheme) => mini[scheme].tab === "Transcript" && mini[scheme].present && mini[scheme].hidden === false
        && mini[scheme].drawn && mini[scheme].header && mini[scheme].follow);
      const miniHiddenOnNotes = onNotes.tab === "Notes" && onNotes.present && onNotes.hidden === true && !onNotes.drawn;
      const noChipOnNotes = playingNarrow && notesPlaying.tab === "Notes" && notesPlaying.present && notesPlaying.hidden === false
        && notesPlaying.drawn && !notesPlaying.follow && !notesPlaying.header;
      return {
        measured: { wide, narrow, mini: { transcriptTab: mini, notesTab: onNotes, plays, playingNarrow, notesTabPlaying: notesPlaying } },
        matches: SCHEMES.every((scheme) => wide[scheme].matches && narrow[scheme].matches) && miniShown && miniHiddenOnNotes && plays === 0 && noChipOnNotes,
        screenshots,
      };
    });

    // 14. The column keeps its label while its rows load.
    await check({
      id: "14",
      claim: "While the rows load, the column still shows its label.",
      expectation: [
        `in each scheme: with note_transcript_rows held back ${ROWS_DELAY_MS} ms (installInvokeStub/stubInvoke), note 106 opened at`,
        "1440x900: while no line is drawn, .note-side holds exactly one heading, a .transcript-head .transcript-label reading",
        `"Transcript" (case-insensitive), and the status line "${LOADING}"; once the rows arrive, the loaded column's label is`,
        "drawn the same (text-transform, font family, size and colour equal; its head's height and the label's left within 0.5px)",
      ].join(" "),
    }, async () => {
      const schemes = {};
      const screenshots = [];
      for (const scheme of SCHEMES) {
        await cdp.send("Emulation.setEmulatedMedia", { features: [{ name: "prefers-color-scheme", value: scheme }] });
        await loadNote(cdp, WIDE, async () => {
          await cdp.evaluate(snippets.installInvokeStub());
          await cdp.evaluate(snippets.stubInvoke({ delay: { note_transcript_rows: ROWS_DELAY_MS } }));
        });
        const drawn = await cdp.evaluate(snippets.markColorScheme(scheme));
        const pending = await cdp.evaluate(snippets.columnLabelState());
        const shot = `14-loading-${scheme}-1440x900.png`;
        await cdp.screenshot(resolve(out, shot));
        screenshots.push(shot);
        const loadedIn = await waited(cdp, "Boolean(document.querySelector('.note-side .transcript-rows li'))", "the rows", ROWS_DELAY_MS + 10_000);
        const loaded = await cdp.evaluate(snippets.columnLabelState());
        await cdp.evaluate(snippets.stubInvoke({ delay: { note_transcript_rows: null } }));
        const parts = {
          pending: pending.lines === 0,
          label: pending.headings.length === 1 && (pending.label ?? "").trim().toLowerCase() === "transcript",
          status: pending.status === LOADING,
          loaded: loadedIn && loaded.lines > 0,
          sameLook: pending.drawn !== null && loaded.drawn !== null && JSON.stringify(pending.drawn) === JSON.stringify(loaded.drawn)
            && near(pending.head?.height, loaded.head?.height ?? Number.NaN, 0.5) && near(pending.head?.left, loaded.head?.left ?? Number.NaN, 0.5),
        };
        schemes[scheme] = {
          drawn,
          pending,
          loaded,
          failed: Object.entries(parts).filter(([, ok]) => !ok).map(([name]) => name),
          matches: Object.values(parts).every(Boolean),
        };
      }
      return {
        measured: { schemes },
        matches: SCHEMES.every((scheme) => schemes[scheme].matches),
        screenshots,
      };
    });

    // 16. A hovered line is a neutral fill; the playing line keeps its wash under the pointer.
    await check({
      id: "16",
      claim: "Hover is a neutral fill; the playing line keeps its wash.",
      expectation: [
        `in each scheme, from a fresh load with nothing playing or selected: a real pointer moved onto line ${HOVER_LINE + 1} (inside its left`,
        "padding) leaves it :hover, not aria-current, with a computed background equal to the resolved --panel-hover; then that line",
        "played by a real press in its left padding (linePadPoint), which also selects it, and the pointer moved onto it again:",
        "aria-current, :hover, and its computed background equal to the resolved --band-wash, not the selected line's --selected;",
        "the two tokens resolve differently",
      ].join(" "),
    }, async () => {
      const schemes = {};
      const screenshots = [];
      for (const scheme of SCHEMES) {
        // A fresh load each time: the press below leaves the line selected, and a selected line is not hovered.
        await openNote(cdp);
        const drawn = await useScheme(cdp, scheme);
        await stopPlaying(cdp);
        const at = await cdp.evaluate(snippets.lineHoverState(HOVER_LINE));
        await hover(cdp, at.point);
        const idle = await cdp.evaluate(snippets.lineHoverState(HOVER_LINE));
        const idleShot = `16-hover-${scheme}-1440x900.png`;
        await cdp.screenshot(resolve(out, idleShot));
        screenshots.push(idleShot);
        await pressLine(cdp, HOVER_LINE);
        const started = await waited(cdp, `document.querySelectorAll('.note-side .transcript-rows li')[${HOVER_LINE}]?.getAttribute('aria-current') === 'true'`, "the line to play", 5_000);
        const again = await cdp.evaluate(snippets.lineHoverState(HOVER_LINE));
        await hover(cdp, again.point);
        const playing = await cdp.evaluate(snippets.lineHoverState(HOVER_LINE));
        const playShot = `16-hover-playing-${scheme}-1440x900.png`;
        await cdp.screenshot(resolve(out, playShot));
        screenshots.push(playShot);
        await stopPlaying(cdp);
        const parts = {
          idleHovered: idle.hovered && !idle.current,
          idleFill: idle.background === idle.panelHover,
          started,
          playingHovered: playing.hovered && playing.current,
          playingWash: playing.background === playing.bandWash,
          tokensDiffer: idle.panelHover !== idle.bandWash,
        };
        schemes[scheme] = {
          drawn,
          idle,
          playing,
          failed: Object.entries(parts).filter(([, ok]) => !ok).map(([name]) => name),
          matches: Object.values(parts).every(Boolean),
        };
      }
      return {
        measured: { schemes },
        matches: SCHEMES.every((scheme) => schemes[scheme].matches),
        screenshots,
      };
    });

    // 17. A search hit that opens the Transcript tab shows its row, not the line the player is at.
    await check({
      id: "17",
      claim: "A search hit that opens the Transcript tab shows its row, though Follow is on and the player is elsewhere.",
      expectation: [
        `in each scheme, at 1000x720 from a fresh load on the Notes tab, nothing played and the recording paused at ${HIT_FROM} s`,
        `(inside line 43, far below): the title bar's search for "${HIT_QUERY}", and its result for "${TITLE}" chosen (as`,
        `transcript-search.mjs chooses one). Once .notes-main has stopped scrolling: the Transcript tab is selected, line ${HIT_ROW + 1}, the one`,
        "the hit names, is the only aria-selected=\"true\" line and is drawn wholly inside the visible band (lineBandState: .notes-main less",
        "the sticky head and --player-h + 24px, ±1px), the mini card's Follow chip is still aria-pressed=\"true\" (paused), and no play() call",
        "was made",
      ].join(" "),
    }, async () => {
      const schemes = {};
      const screenshots = [];
      for (const scheme of SCHEMES) {
        await cdp.send("Emulation.setEmulatedMedia", { features: [{ name: "prefers-color-scheme", value: scheme }] });
        await loadNote(cdp, NARROW);
        const drawn = await cdp.evaluate(snippets.markColorScheme(scheme));
        await waitFor(cdp, "(document.querySelector('.note-detail audio')?.readyState ?? 0) >= 1", "the recording's metadata");
        await cdp.evaluate(snippets.pauseRecordingAt(MINI, HIT_FROM));
        await waitFor(cdp, `(() => { const at = ${snippets.recordingAt(MINI)}; return at.paused && !at.seeking && Math.abs(at.time - ${HIT_FROM}) < 0.01; })()`, `the recording paused at ${HIT_FROM} s`);
        await delay(300);
        const playsBefore = await cdp.evaluate("window.__plays ?? null");
        await chooseSearchHit(cdp, HIT_QUERY, TITLE);
        await waited(cdp, "document.querySelector('.note-detail [role=\"tab\"][aria-selected=\"true\"]')?.textContent.trim() === 'Transcript'", "the Transcript tab", 5_000);
        // Past the two frames the transcript waits on coming on screen, and any scroll after them.
        await delay(600);
        await scrollSettled(cdp, ".notes-main");
        const tab = await cdp.evaluate("document.querySelector('.note-detail [role=\"tab\"][aria-selected=\"true\"]')?.textContent.trim() ?? null");
        const lines = await cdp.evaluate(snippets.lineModelState());
        const row = await cdp.evaluate(snippets.lineBandState(".notes-main", HIT_ROW));
        const follow = await cdp.evaluate(snippets.followChipState(MINI));
        const plays = (await cdp.evaluate("window.__plays ?? null")) - playsBefore;
        const shot = `17-search-hit-${scheme}-1000x720.png`;
        await cdp.screenshot(resolve(out, shot));
        screenshots.push(shot);
        const parts = {
          onTranscript: tab === "Transcript",
          selected: JSON.stringify(lines.selected) === JSON.stringify([HIT_ROW]),
          inView: row.inBand,
          followOn: follow.pressed === "true",
          noPlay: plays === 0,
        };
        schemes[scheme] = {
          drawn,
          tab,
          selected: lines.selected,
          row: { ...row, top: round(row.top), bottom: round(row.bottom), offset: round(row.offset), band: { top: round(row.band.top), bottom: round(row.band.bottom) } },
          follow: follow.pressed,
          plays,
          failed: Object.entries(parts).filter(([, ok]) => !ok).map(([name]) => name),
          matches: Object.values(parts).every(Boolean),
        };
      }
      return {
        measured: { schemes },
        matches: SCHEMES.every((scheme) => schemes[scheme].matches),
        screenshots,
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
    if (options.help) process.stdout.write(`${shellUsage().replace("shell-rail.mjs", "transcript-column.mjs")}\n`);
    else {
      const failed = failedChecks(await runTranscriptColumn(options));
      // The run completed; a check measured false still fails it, so a wrapper or CI step sees it.
      if (failed.length) {
        console.error(`transcript-column: ${failed.length} check(s) false: ${failed.join(", ")}`);
        process.exitCode = 1;
      }
    }
  } catch (error) {
    console.error(`transcript-column failed: ${error.message}`);
    process.exitCode = 1;
  }
}
