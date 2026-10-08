#!/usr/bin/env node
import { waitFor, reload, pressText as press } from "./interactions.mjs";
/**
 * Measures, against dev fixtures, that a transcript row local Whisper timed
 * word by word highlights the word being said as its recording plays, and
 * that clicking a word goes there (KNEG-408).
 *
 * Fixture note 105, "Word timings" (`apps/desktop/src/lib/devFixtures.ts`):
 * row 1 has fourteen words, one every 0.65 s from 0.5 s; row 2 eleven; row 3
 * none, as every note saved before KNEG-408 has none. The fixtures play their
 * two-minute WAV for it (`lib/fixtureWav.ts`). Words and times are read from
 * the page's own `note_transcript_rows`, so the check and the page agree on
 * what the row holds.
 *
 * Each check starts from a fresh load at 1440x900 on Notes with note 105 open
 * from the library, its transcript in the side column. A row is played with
 * its own Play, pressed with a real pointer press so the page has the user
 * activation a click gives it. Edge runs with `--mute-audio`.
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

const EDGE_ARGS = ["--mute-audio"];
const RAIL_ITEM = "nav.rail .rail-item";
const WIDE = { width: 1440, height: 900 };
const NOTE_ID = 105;
const TITLE = "Word timings";
/** How far behind the recording the lit word may be: the frame between the page's last step and the read. */
const LAG_SECONDS = 0.1;
/** How close to a clicked word's start the recording must land. */
const LANDING_SECONDS = 0.05;
/** Time between the three samples of a playing row. */
const SAMPLE_GAP_MS = 1200;
/** The later word clicked in check 2: "nästa", row 1's thirteenth word, at 8.3 s. */
const LATER_WORD = 12;

const noteOpen = `(() => { const input = document.querySelector('.note-detail input[aria-label="Title"]'); return Boolean(input) && !input.disabled && input.value === ${JSON.stringify(TITLE)}; })()`;
const playButton = (row) => `.note-side button[aria-label="Play transcript row ${row + 1}"]`;
const playingFrom = (seconds) => `(() => { const audio = document.querySelector('audio'); return Boolean(audio) && !audio.paused && audio.readyState >= 3 && audio.currentTime >= ${seconds}; })()`;
const rowIsCurrent = (row) => `(document.querySelectorAll('.note-detail .transcript .transcript-rows li')[${row}]?.getAttribute('aria-current') === 'true')`;





/** A real pointer press at the centre of the first `selector` reading `text` (any text for `null`). */


/** The last word that has started by `seconds`, as `lib/wordTiming.ts`'s `wordAt` answers it. */
function wordAt(words, seconds) {
  let found = null;
  words.forEach((word, at) => { if (word.start <= seconds) found = at; });
  return found;
}

/** A fresh load at 1440x900 with note 105 open and its rows, words and all, as the page has them. */
async function openNote(cdp) {
  await cdp.send("Emulation.setDeviceMetricsOverride", { ...WIDE, deviceScaleFactor: 1, mobile: false });
  await reload(cdp);
  await waitFor(cdp, "Boolean(document.querySelector('nav.rail') && document.querySelector('.card .split-main, .card .content'))", "the fixture shell");
  await cdp.evaluate(snippets.removeFixtureBanner());
  await cdp.evaluate(snippets.clickText(RAIL_ITEM, "Notes"));
  await waitFor(cdp, "Boolean(document.querySelector('.list-panel .notes-entry'))", "the Notes library");
  await cdp.evaluate(snippets.openNotesEntry(TITLE));
  await waitFor(cdp, noteOpen, `note "${TITLE}" to open`);
  await waitFor(cdp, "Boolean(document.querySelector('.note-side .transcript-rows li')) && (document.querySelector('.note-side-player audio')?.readyState ?? 0) >= 1", "the transcript column and the recording's metadata");
  return cdp.evaluate(snippets.noteTranscriptRows(NOTE_ID));
}

/** Plays row `row` with its own Play and waits until the recording is `into` seconds past the row's start. */
async function playRow(cdp, rows, row, into) {
  await press(cdp, playButton(row), null);
  await waitFor(cdp, `${playingFrom(rows[row].startSeconds + into)} && ${rowIsCurrent(row)}`, `row ${row + 1} to play`);
}

/** One sample of row `row`: the time, the word lit, and the word expected then and a frame before. */
async function sample(cdp, words, row) {
  const state = await cdp.evaluate(snippets.wordHighlightState());
  const drawn = state.rows[row];
  const expected = wordAt(words, state.time);
  const frameBefore = wordAt(words, state.time - LAG_SECONDS);
  const lit = drawn.lit;
  return {
    time: Math.round(state.time * 1000) / 1000,
    paused: state.paused,
    current: drawn.current,
    lit,
    litText: lit.map((at) => words[at]?.text ?? null),
    expected,
    inStep: lit.length === 1 && (lit[0] === expected || lit[0] === frameBefore),
    spans: drawn.words,
  };
}

const SETUP_NOTE = "Each check starts from a fresh load at 1440x900 on Notes with fixture note 105 open from the library, its transcript in the side column. Rows are played with their own Play, pressed with a real CDP pointer press. Words and times come from the page's note_transcript_rows.";

export async function runWordHighlight(options) {
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

    /** Runs one check; a step that throws is the check measured false, with the rows as they were. */
    const check = async ({ id, claim, expectation }, run) => {
      try {
        const result = await run();
        addCheck(report, { id, claim, expectation, note: SETUP_NOTE, ...result });
      } catch (error) {
        let state = null;
        try { state = await cdp.evaluate(snippets.wordHighlightState()); } catch { /* no page to read */ }
        addCheck(report, { id, claim, expectation, measured: { failedAt: error.message, state }, matches: false, note: SETUP_NOTE });
      } finally {
        await cdp.send("Emulation.setEmulatedMedia", { features: [] }).catch(() => undefined);
      }
    };

    // 1. Row 1 plays: the lit word follows the recording.
    await check({
      id: "1",
      claim: "Playing a row local Whisper timed word by word lights the word being said, and the light moves on with the recording.",
      expectation: [
        `three samples ${SAMPLE_GAP_MS}ms apart while row 1 plays: row 1 is the playing row and draws one span per word;`,
        "each sample lights exactly one word, the last to have started at the recording's time (or",
        `${LAG_SECONDS}s before it, the frame between the page's step and the read); the lit word is later in each sample than the one before`,
      ].join(" "),
    }, async () => {
      const rows = await openNote(cdp);
      const words = rows[0].words;
      await playRow(cdp, rows, 0, 0.8);
      const samples = [];
      for (let taken = 0; taken < 3; taken += 1) {
        if (taken > 0) await delay(SAMPLE_GAP_MS);
        samples.push(await sample(cdp, words, 0));
      }
      const shot = "01-word-lit-1440x900.png";
      await cdp.screenshot(resolve(out, shot));
      const advances = samples.every((item, at) => at === 0 || (item.lit[0] ?? -1) > (samples[at - 1].lit[0] ?? -1));
      return {
        measured: { words: words.length, rowText: rows[0].text, samples, advances },
        matches: words.length === 14 && samples.every((item) => item.current && !item.paused && item.inStep && item.spans === words.length) && advances,
        screenshots: [shot],
      };
    });

    // 2. A later word clicked: the recording goes there, and that word lights.
    await check({
      id: "2",
      claim: "Clicking a later word of the playing row moves the recording to that word's start, lights that word, keeps playing, and leaves the row's selection alone.",
      expectation: [
        `with row 1 playing about 1s in, a real press on "${"nästa"}" (word ${LATER_WORD + 1}):`,
        `the seek ends within ${LANDING_SECONDS}s of the word's start; 300ms later that word is lit, the recording is not paused,`,
        "and row 1 is no more selected than before the press",
      ].join(" "),
    }, async () => {
      const rows = await openNote(cdp);
      const words = rows[0].words;
      const target = words[LATER_WORD];
      await playRow(cdp, rows, 0, 0.5);
      const before = await sample(cdp, words, 0);
      await cdp.evaluate(snippets.watchSeek());
      await press(cdp, ".note-side .transcript-word", target.text);
      await waitFor(cdp, "typeof window.__seek?.seeked === 'number'", "the seek to end", 3_000);
      const seek = await cdp.evaluate(snippets.seekRecord());
      await delay(300);
      const after = await sample(cdp, words, 0);
      const state = await cdp.evaluate(snippets.wordHighlightState());
      const landing = Math.abs(seek.seeked - target.start);
      return {
        measured: {
          word: target,
          before: { time: before.time, lit: before.litText, selected: state.rows[0].selected },
          seek: { seeking: seek.seeking, seeked: seek.seeked, fromStart: Math.round(landing * 1000) / 1000 },
          after,
          selectedAfter: state.rows[0].selected,
        },
        matches: before.lit[0] < LATER_WORD && landing <= LANDING_SECONDS
          && after.lit.length === 1 && after.lit[0] === LATER_WORD && !after.paused
          && state.rows[0].selected === false,
      };
    });

    // 3. A row without words: plain text, as before KNEG-408.
    await check({
      id: "3",
      claim: "A row with no word timings plays as a row: its text is drawn whole, with no word spans and nothing lit, and so is every row that is not playing.",
      expectation: "row 3 playing (aria-current) for 1s: its drawn text is the row's text, it draws no word spans and no row lights a word; rows 1 and 2 keep their seek controls without lighting a word",
    }, async () => {
      const rows = await openNote(cdp);
      await playRow(cdp, rows, 2, 0.3);
      await delay(1000);
      const state = await cdp.evaluate(snippets.wordHighlightState());
      const shot = "03-row-without-words-1440x900.png";
      await cdp.screenshot(resolve(out, shot));
      return {
        measured: { fixtureWords: rows[2].words.length, rows: state.rows, time: state.time, paused: state.paused },
        matches: rows[2].words.length === 0 && state.rows[2].current && !state.paused
          && state.rows[2].text === rows[2].text && state.rows.every((row) => row.lit.length === 0) && state.rows[2].words === 0,
        screenshots: [shot],
      };
    });

    // 4. Reduced motion: no fade, and the light still moves.
    await check({
      id: "4",
      claim: "With reduced motion asked for, the lit word jumps rather than fades, and still follows the recording.",
      expectation: [
        "row 1 playing: a word span's computed transition-duration is not 0s without the preference (the control),",
        "and 0s for every property with prefers-reduced-motion: reduce emulated; under it, two samples",
        `${SAMPLE_GAP_MS}ms apart each light exactly the expected word, the second later than the first`,
      ].join(" "),
    }, async () => {
      const rows = await openNote(cdp);
      const words = rows[0].words;
      await playRow(cdp, rows, 0, 0.8);
      const plain = (await cdp.evaluate(snippets.wordHighlightState())).rows[0].transition;
      await cdp.send("Emulation.setEmulatedMedia", { features: [{ name: "prefers-reduced-motion", value: "reduce" }] });
      const reduced = (await cdp.evaluate(snippets.wordHighlightState())).rows[0].transition;
      const first = await sample(cdp, words, 0);
      await delay(SAMPLE_GAP_MS);
      const second = await sample(cdp, words, 0);
      const zero = (value) => typeof value === "string" && value.split(",").every((part) => part.trim() === "0s");
      return {
        measured: { transition: { plain, reduced }, samples: [first, second] },
        matches: typeof plain === "string" && !zero(plain) && zero(reduced)
          && first.inStep && second.inStep && second.lit[0] > first.lit[0],
      };
    });


    await check({id:"5",claim:"Keyboard words seek with a roving tab stop; pausing clears the animation highlight.",expectation:"ArrowRight advances focus, Enter seeks, and a paused player has no highlighted word."},async () => {
      await openNote(cdp);
      await cdp.evaluate("document.querySelector('.note-side .transcript-word').focus()");
      for (const key of ["ArrowRight","Enter"]) {
        await cdp.send("Input.dispatchKeyEvent",{type:"keyDown",key,code:key,windowsVirtualKeyCode:key==="Enter"?13:39});
        await cdp.send("Input.dispatchKeyEvent",{type:"keyUp",key,code:key,windowsVirtualKeyCode:key==="Enter"?13:39});
      }
      await delay(100);
      const keyboard = await cdp.evaluate("({time:document.querySelector('audio').currentTime, focused:document.activeElement?.textContent, stops:document.querySelector('.note-side .transcript-text').querySelectorAll('[tabindex=\"0\"]').length})");
      await press(cdp,playButton(0),null);
      await waitFor(cdp,"document.querySelectorAll('.transcript-word-current').length===1","a playing word");
      await cdp.evaluate("document.querySelector('audio').pause()");
      await waitFor(cdp,"document.querySelectorAll('.transcript-word-current').length===0","pause cleanup");
      await delay(250);
      const clean = await cdp.evaluate("document.querySelectorAll('.transcript-word-current').length===0");
      return {measured:{keyboard,clean},matches:Math.abs(keyboard.time-1.15)<0.05 && keyboard.stops===1 && clean};
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
    if (options.help) process.stdout.write(`${shellUsage().replace("shell-rail.mjs", "word-highlight.mjs")}\n`);
    else {
      const failed = failedChecks(await runWordHighlight(options));
      // The run completed; a check measured false still fails it, so a wrapper or CI step sees it.
      if (failed.length) {
        console.error(`word-highlight: ${failed.length} check(s) false: ${failed.join(", ")}`);
        process.exitCode = 1;
      }
    }
  } catch (error) {
    console.error(`word-highlight failed: ${error.message}`);
    process.exitCode = 1;
  }
}
