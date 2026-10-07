#!/usr/bin/env node
/**
 * Measures, against dev fixtures, that a search hit in a meeting transcript
 * opens the note at the row where the words were said, and only selects it
 * (KNEG-406): the hit says when, the note opens on that row, a second hit in
 * the open note moves to its row, Back shows the row again, a wide window
 * selects it in the side column, and the library filter does the same. No
 * step may start, move or pause playback -- the owner's ruling of
 * 2026-10-07 -- so every check also reads a count of `play()` calls and every
 * `audio` element's `paused`.
 *
 * Fixture note 101, "Weekly sync — platform" (`apps/desktop/src/lib/devFixtures.ts`):
 * transcript row 0 at 1:12 says "two more days", row 1 at 1:28 says "honest",
 * and its notes say "Thursday", which is also in row 1.
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

/** Counts `play()` from before the app's own code runs. */
const COUNT_PLAYS = `(() => {
  window.__plays = 0;
  const play = HTMLMediaElement.prototype.play;
  HTMLMediaElement.prototype.play = function (...args) { window.__plays += 1; return play.apply(this, args); };
})();`;

const PLAYBACK = `({
  plays: window.__plays,
  allPaused: [...document.querySelectorAll('audio')].every((audio) => audio.paused),
})`;

/** The open note's selected transcript rows, whether each is in view, and the tab that is on. */
const OPEN = `(() => {
  const selected = [...document.querySelectorAll('.transcript-row-current[aria-selected="true"]')]
    .filter((row) => row.getClientRects().length > 0)
    .map((row) => {
      const rect = row.getBoundingClientRect();
      return {
        text: row.querySelector('.transcript-text')?.textContent ?? '',
        inView: rect.top >= 0 && rect.bottom <= innerHeight,
        inSide: Boolean(row.closest('.note-side')),
      };
    });
  return {
    title: document.querySelector('.note-detail input[aria-label="Title"]')?.value ?? null,
    tab: document.querySelector('.note-detail [role="tab"][aria-selected="true"]')?.textContent.trim() ?? null,
    selected,
    marked: document.querySelector('.note-detail')?.__kwMark === true,
  };
})()`;

async function waitFor(cdp, expression, description, timeout = 8_000) {
  const deadline = Date.now() + timeout;
  while (Date.now() < deadline) {
    try {
      if (await cdp.evaluate(expression)) return;
    } catch {
      // Between two documents there is no context to evaluate in.
    }
    await delay(100);
  }
  throw new Error(`Timed out waiting for ${description}`);
}

async function viewport(cdp, width, height) {
  await cdp.send("Emulation.setDeviceMetricsOverride", { width, height, deviceScaleFactor: 1, mobile: false });
  await delay(150);
}

/** Types into the title bar's search and returns the result lines once they are in. */
async function titleSearch(cdp, query) {
  // The query already in the field is shown again by a click, as a person would get the results
  // back; typing the same text over it changes nothing, so React sees no change and opens nothing.
  const same = await cdp.evaluate(`(() => {
    const field = document.querySelector('.titlebar-search input');
    field.focus();
    if (field.value === ${JSON.stringify(query)}) { field.click(); return true; }
    field.select();
    return false;
  })()`);
  if (!same) await cdp.send("Input.insertText", { text: query });
  try {
    await waitFor(cdp, "document.querySelectorAll('#titlebar-search-results [role=option]').length > 0", `results for ${query}`);
  } catch (error) {
    const field = await cdp.evaluate("({ value: document.querySelector('.titlebar-search input')?.value, status: document.querySelector('.titlebar-search-status')?.textContent ?? null })");
    throw new Error(`${error.message}: ${JSON.stringify(field)}`);
  }
  return cdp.evaluate(`[...document.querySelectorAll('#titlebar-search-results [role=option]')].map((option) => option.querySelector('span')?.textContent ?? '')`);
}

/** Clicks the first `selector` whose text contains `text`: a hit's text is its title and its snippet. */
async function clickIncluding(cdp, selector, text) {
  const clicked = await cdp.evaluate(`(() => {
    const item = [...document.querySelectorAll(${JSON.stringify(selector)})].find((element) => element.textContent.includes(${JSON.stringify(text)}));
    item?.click();
    return Boolean(item);
  })()`);
  if (!clicked) throw new Error(`No ${selector} contains ${text}`);
}

async function chooseTitleHit(cdp, title) {
  await clickIncluding(cdp, "#titlebar-search-results [role=option]", title);
  // The note opens, then the row scrolls smoothly into view.
  await delay(1200);
}

async function ready(cdp) {
  await waitFor(cdp, "Boolean(document.querySelector('nav.rail') && document.querySelector('.card .split-main, .card .content'))", "the fixture shell");
  await cdp.evaluate(snippets.removeFixtureBanner());
}

export async function runTranscriptSearch(options) {
  const out = resolve(options.out);
  await mkdir(out, { recursive: true });
  let browser;
  let cdp;
  try {
    browser = await launchEdge({ ...options, out, url: "about:blank" });
    cdp = await connectCdp(browser.target.webSocketDebuggerUrl);
    await cdp.send("Page.enable");
    await cdp.send("Page.addScriptToEvaluateOnNewDocument", { source: COUNT_PLAYS });
    await viewport(cdp, 1000, 720);
    await cdp.send("Page.navigate", { url: options.url });
    await ready(cdp);
    const report = createReport({ url: options.url, renderer: "headless Edge against dev fixtures, not the Tauri window" });

    // 1. The hit says when, and opens the note on that row.
    const lines = await titleSearch(cdp, "honest");
    await chooseTitleHit(cdp, "Weekly sync");
    const opened = await cdp.evaluate(OPEN);
    const playback1 = await cdp.evaluate(PLAYBACK);
    const shot1 = "01-hit-opens-row-1000x720.png";
    await cdp.screenshot(resolve(out, shot1));
    addCheck(report, {
      id: "1",
      claim: "A title-bar hit in note 101's transcript says when its words were said, and choosing it opens the note on the Transcript tab with that row selected and in view, without playing.",
      measured: { lines, opened, playback: playback1 },
      expectation: "a line starting \"1:28 · \"; the note open on Transcript with exactly the \"honest\" row selected and in view; 0 plays, every audio paused",
      matches: lines.some((line) => line.startsWith("1:28 · ") && line.includes("[honest]"))
        && opened.tab === "Transcript"
        && opened.selected.length === 1 && opened.selected[0].text.includes("honest") && opened.selected[0].inView
        && playback1.plays === 0 && playback1.allPaused,
      screenshots: [shot1],
    });

    // 2. A second hit in the note already open moves to its row, without a remount, and the
    //    same hit chosen again selects it again after the row was deselected by hand.
    await cdp.evaluate("(() => { document.querySelector('.note-detail').__kwMark = true; return true; })()");
    await titleSearch(cdp, "more days");
    await chooseTitleHit(cdp, "Weekly sync");
    const moved = await cdp.evaluate(OPEN);
    await cdp.evaluate(`[...document.querySelectorAll('.transcript-row-current[aria-selected="true"]')].find((row) => row.getClientRects().length > 0)?.click()`);
    await delay(200);
    const cleared = await cdp.evaluate(OPEN);
    await titleSearch(cdp, "more days");
    await chooseTitleHit(cdp, "Weekly sync");
    const again = await cdp.evaluate(OPEN);
    const playback2 = await cdp.evaluate(PLAYBACK);
    addCheck(report, {
      id: "2",
      claim: "In the note already open, another hit moves the selection to its row without remounting the note, and the same hit chosen again selects its row again.",
      measured: { moved, cleared, again, playback: playback2 },
      expectation: "\"more days\" row selected with the note element unchanged; none selected after a click on it; selected again; 0 plays, every audio paused",
      matches: moved.marked && moved.selected.length === 1 && moved.selected[0].text.includes("two more days")
        && cleared.selected.length === 0
        && again.selected.length === 1 && again.selected[0].text.includes("two more days")
        && playback2.plays === 0 && playback2.allPaused,
    });

    // 3. Back from another page shows the row again.
    await cdp.evaluate(snippets.clickText("nav.rail .rail-item", "Dictation"));
    await delay(400);
    await cdp.evaluate("(() => { document.querySelector('.app-titlebar button[aria-label=\"Go back\"]').click(); return true; })()");
    await delay(1500);
    const back = await cdp.evaluate(OPEN);
    const playback3 = await cdp.evaluate(PLAYBACK);
    addCheck(report, {
      id: "3",
      claim: "Going to Dictation and pressing Back returns to the note with the row the last hit chose selected again.",
      measured: { back, playback: playback3 },
      expectation: "note 101 on Transcript with the \"more days\" row selected; 0 plays",
      matches: back.title === "Weekly sync — platform" && back.tab === "Transcript"
        && back.selected.length === 1 && back.selected[0].text.includes("two more days")
        && playback3.plays === 0 && playback3.allPaused,
    });

    // 4. A wide window selects the row in the side column and leaves the Notes tab on.
    await viewport(cdp, 1440, 900);
    await cdp.evaluate(snippets.clickText("nav.rail .rail-item", "Dictation"));
    await delay(300);
    await titleSearch(cdp, "honest");
    await chooseTitleHit(cdp, "Weekly sync");
    const wide = await cdp.evaluate(OPEN);
    const playback4 = await cdp.evaluate(PLAYBACK);
    const shot4 = "04-wide-side-column-1440x900.png";
    await cdp.screenshot(resolve(out, shot4));
    addCheck(report, {
      id: "4",
      claim: "At 1440x900 the hit's row is selected in the transcript column beside the note, and the note's own tab stays on Notes.",
      measured: { wide, playback: playback4 },
      expectation: "the \"honest\" row selected inside .note-side and in view; tab Notes; 0 plays",
      matches: wide.tab === "Notes" && wide.selected.length === 1 && wide.selected[0].inSide
        && wide.selected[0].text.includes("honest") && wide.selected[0].inView
        && playback4.plays === 0 && playback4.allPaused,
      screenshots: [shot4],
    });

    // 5. The library filter takes the same path, and a hit in the notes says no time.
    await viewport(cdp, 1000, 720);
    await cdp.evaluate(snippets.clickText("nav.rail .rail-item", "Notes"));
    await delay(500);
    const filter = async (query) => {
      await cdp.evaluate(`(() => { const field = document.querySelector('input[aria-label="Filter the library"]'); field.focus(); field.select(); return true; })()`);
      await cdp.send("Input.insertText", { text: query });
      await waitFor(cdp, "document.querySelectorAll('.notes-hit').length > 0", `library hits for ${query}`);
      return cdp.evaluate("[...document.querySelectorAll('.notes-hit')].map((hit) => hit.textContent)");
    };
    const bodyHits = await filter("thursday");
    const rowHits = await filter("honest");
    await clickIncluding(cdp, ".notes-hit", "Weekly sync");
    await delay(1200);
    const library = await cdp.evaluate(OPEN);
    const playback5 = await cdp.evaluate(PLAYBACK);
    addCheck(report, {
      id: "5",
      claim: "The library filter's transcript hit says when and opens on its row; a hit in the note's own text says no time.",
      measured: { bodyHits, rowHits, library, playback: playback5 },
      expectation: "\"thursday\" hits carry no \"m:ss · \"; the \"honest\" hit carries \"1:28 · \" and opens on that row; 0 plays",
      matches: bodyHits.length > 0 && bodyHits.every((text) => !/\d:\d\d · /.test(text))
        && rowHits.some((text) => text.includes("1:28 · "))
        && library.tab === "Transcript" && library.selected.length === 1 && library.selected[0].text.includes("honest")
        && playback5.plays === 0 && playback5.allPaused,
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
      const failed = failedChecks(await runTranscriptSearch(options));
      if (failed.length) {
        console.error(`transcript-search: ${failed.length} check(s) false: ${failed.join(", ")}`);
        process.exitCode = 1;
      }
    }
  } catch (error) {
    console.error(`transcript-search failed: ${error.message}`);
    process.exitCode = 1;
  }
}
