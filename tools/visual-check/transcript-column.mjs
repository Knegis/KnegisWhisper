#!/usr/bin/env node
/**
 * Measures, against dev fixtures, the transcript column beside a note in a
 * wide window as the M6 redesign draws it (KNEG-448). The claims arrive with
 * the tasks that make them true, so the ids are the plan's and are not
 * contiguous: 1, 11, 13, 14 and 16 are the column's header, sections, turns,
 * rename and hover, and 2 and 3 the player card.
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

/** A real key press: Escape. */
async function escape(cdp) {
  const base = { key: "Escape", code: "Escape", windowsVirtualKeyCode: 27 };
  await cdp.send("Input.dispatchKeyEvent", { type: "keyDown", ...base });
  await cdp.send("Input.dispatchKeyEvent", { type: "keyUp", ...base });
  await delay(150);
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
        `${TURNS} .transcript-turn-head, each with a ${MARKER}x${MARKER}px .transcript-turn-marker; ${LINES} lines; over .note-side .transcript`,
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
        schemes[scheme] = { drawn, ...judged, state };
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

    // 13. The header stays at the top of whatever scrolls the transcript, in both layouts.
    await check({
      id: "13",
      claim: "The header stays in view while the transcript scrolls (both layouts).",
      expectation: [
        `in each scheme: at 1440x900, .note-side-body scrolled down by ${HEAD_SCROLL}px (from 0, and it moves the full ${HEAD_SCROLL}px),`,
        `.transcript-head's top within ${HEAD_TOLERANCE}px of the scroller's top inside its border; at 1000x720 on the Transcript tab,`,
        `.notes-main scrolled down by ${HEAD_SCROLL}px the same way, the tab's .transcript-head's top within ${HEAD_TOLERANCE}px of .notes-main's`,
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
      for (const scheme of SCHEMES) {
        const drawn = await useScheme(cdp, scheme);
        await cdp.evaluate("document.querySelector('.notes-main').scrollTo({ top: 0, behavior: 'instant' })");
        narrow[scheme] = { drawn, ...judge(await cdp.evaluate(snippets.scrollAndReadHead(".notes-main", HEAD_SCROLL))) };
        const shot = `13-narrow-scrolled-${scheme}-1000x720.png`;
        await cdp.screenshot(resolve(out, shot));
        screenshots.push(shot);
      }
      return {
        measured: { wide, narrow },
        matches: SCHEMES.every((scheme) => wide[scheme].matches && narrow[scheme].matches),
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
        `in each scheme, nothing playing: a real pointer moved onto line ${HOVER_LINE + 1} (inside its left padding) leaves it :hover, not`,
        "aria-current, with a computed background equal to the resolved --panel-hover; then that line played with its own",
        `button.transcript-play ("Play transcript row ${HOVER_LINE + 1}") and the pointer moved onto it again: aria-current, :hover, and its`,
        "computed background equal to the resolved --band-wash; the two tokens resolve differently",
      ].join(" "),
    }, async () => {
      await openNote(cdp);
      const schemes = {};
      const screenshots = [];
      for (const scheme of SCHEMES) {
        const drawn = await useScheme(cdp, scheme);
        await stopPlaying(cdp);
        const at = await cdp.evaluate(snippets.lineHoverState(HOVER_LINE));
        await hover(cdp, at.point);
        const idle = await cdp.evaluate(snippets.lineHoverState(HOVER_LINE));
        const idleShot = `16-hover-${scheme}-1440x900.png`;
        await cdp.screenshot(resolve(out, idleShot));
        screenshots.push(idleShot);
        await pressAt(cdp, `.note-side button[aria-label="Play transcript row ${HOVER_LINE + 1}"]`);
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
