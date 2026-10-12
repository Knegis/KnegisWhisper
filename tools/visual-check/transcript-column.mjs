#!/usr/bin/env node
/**
 * Measures, against dev fixtures, the transcript column beside a note in a
 * wide window as the M6 redesign draws it (KNEG-448). The claims arrive with
 * the tasks that make them true, so the ids are the plan's and are not
 * contiguous yet: 2 and 3 are the player card.
 *
 * Fixture note 106, "Roadmap review — long transcript"
 * (`apps/desktop/src/lib/devFixtures.ts`, its rows in `lib/fixtureTranscript.ts`):
 * a 690 s recording, which the fixtures serve as a playable WAV of that
 * length, and a transcript in three five-minute sections starting at 0, 300
 * and 600 s. The player ticks the scrubber where every section after the
 * first starts.
 *
 * Each check starts from a fresh load at 1440x900 on Notes with note 106
 * opened from the library, and measures in both colour schemes:
 * `prefers-color-scheme` emulated over CDP, which is what the stylesheet reads
 * (`styles.css`, `@media (prefers-color-scheme: light)`), with `data-theme`
 * set on the root to match. Presses are real CDP pointer presses. Edge runs
 * with `--mute-audio`.
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

const noteOpen = `(() => { const input = document.querySelector('.note-detail input[aria-label="Title"]'); return Boolean(input) && !input.disabled && input.value === ${JSON.stringify(TITLE)}; })()`;
const near = (value, expected, tolerance) => typeof value === "number" && Math.abs(value - expected) <= tolerance;
const round = (value) => (typeof value === "number" ? Math.round(value * 100) / 100 : value);

/** A fresh load at 1440x900 with note 106 open, its transcript in the column and its recording's length known. */
async function openNote(cdp) {
  await cdp.send("Emulation.setDeviceMetricsOverride", { ...WIDE, deviceScaleFactor: 1, mobile: false });
  await reload(cdp);
  await waitFor(cdp, "Boolean(document.querySelector('nav.rail') && document.querySelector('.card .split-main, .card .content'))", "the fixture shell");
  await cdp.evaluate(snippets.removeFixtureBanner());
  await cdp.evaluate(snippets.clickText(RAIL_ITEM, "Notes"));
  await waitFor(cdp, "Boolean(document.querySelector('.list-panel .notes-entry'))", "the Notes library");
  await cdp.evaluate(snippets.openNotesEntry(TITLE));
  await waitFor(cdp, noteOpen, `note "${TITLE}" to open`);
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

/** A real pointer press at the centre of the first `selector`, which must be on screen. */
async function pressAt(cdp, selector) {
  const point = await cdp.evaluate(snippets.centerOfText(selector, null));
  const size = await cdp.evaluate("({ width: innerWidth, height: innerHeight })");
  if (point.x < 0 || point.y < 0 || point.x > size.width || point.y > size.height) {
    throw new Error(`${selector} is off screen at ${JSON.stringify(point)}`);
  }
  await press(cdp, point);
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

    // 3. Scrolled to its end, the transcript's last line clears the card.
    await check({
      id: "3",
      claim: "The last transcript line is never hidden behind the card.",
      expectation: [
        "in each scheme, with .note-side-body scrolled to its end (scrollTop within 1px of its maximum): the card's top is above",
        "the scroller's bottom (it floats over the scroller, so there is a line to hide), and the last .transcript-rows li's bottom",
        "edge is at or above the card's top edge",
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
          matches: atEnd && floats && clear,
        };
        const shot = `03-end-${scheme}-1440x900.png`;
        await cdp.screenshot(resolve(out, shot));
        screenshots.push(shot);
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
