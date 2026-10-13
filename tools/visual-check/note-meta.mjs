#!/usr/bin/env node
/**
 * Measures, against dev fixtures, the line a note's meta row gives where its
 * transcript was made (KNEG-368): "Transcribed on this computer",
 * "Transcribed by {name}", or, for a meeting whose live captions came from
 * off this computer, "Transcribed on this computer · live captions by
 * {provider}" -- and nothing at all for a note saved before it was recorded.
 *
 * The fixtures store it as Rust does (`Note.transcribedBy`, Rust's
 * `TranscribedBy` as JSON): note 101 is a meeting transcribed on this
 * computer, 104 an import transcribed by OpenAI, 102 a meeting with no
 * record. Their `meeting_stop` saves a meeting's note with the record Rust's
 * `transcribed_by` makes for its provider. Which route Rust names, and when,
 * is pinned by `cargo test` (`transcribed_by.rs`, `meeting/notes.rs`), not
 * here.
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
const WINDOW = { width: 1000, height: 720 };
const SMALL = { width: 720, height: 520 };
/** `computer` in `apps/desktop/src/lib/icons.ts`. */
const COMPUTER = "M5.75 5.75h12.5v9H5.75zM3 18.25h18";
/** Fixture notes (`apps/desktop/src/lib/devFixtures.ts`). */
const NOTE_101 = "Weekly sync — platform";
const NOTE_102 = "Customer call — Nordbank";
const NOTE_104 = "A deliberately long title that exists so the notes list can be judged when someone types far more than the column was designed for";
const HERE = "Transcribed on this computer";
const BY_OPENAI = "Transcribed by OpenAI";
/** The fixtures' default meeting provider, Deepgram, sends audio off this computer. */
const CAPTIONED = "Transcribed on this computer · live captions by Deepgram Realtime";
/** Rust's `note_title` for a meeting given no name. */
const UNNAMED = /^Meeting — \d{4}-\d{2}-\d{2} \d{2}:\d{2}$/;

const LIVE = "Boolean(document.querySelector('.meeting-live'))";
const IDLE = "Boolean(document.querySelector('.control-start')) && !document.querySelector('.meeting-live')";
const START_ENABLED = "(() => { const start = document.querySelector('.control-start'); return Boolean(start) && !start.disabled; })()";
const noteOpen = (title) => `(() => { const input = document.querySelector('.note-detail input[aria-label="Title"]'); return Boolean(input) && !input.disabled && input.value === ${JSON.stringify(title)}; })()`;

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

/** A real pointer press at the element's centre, which must be on screen. */
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

/** A fresh load at 1000x720 with the shell drawn and the fixture banner gone. */
async function freshShell(cdp) {
  await viewport(cdp, WINDOW);
  await reload(cdp);
  await waitFor(cdp, "Boolean(document.querySelector('nav.rail') && document.querySelector('.card .split-main, .card .content'))", "the fixture shell");
  await cdp.evaluate(snippets.removeFixtureBanner());
}

/** Notes from the rail, then the note titled `title` from its library, and its meta row. */
async function openNote(cdp, title) {
  await cdp.evaluate(snippets.clickText(RAIL_ITEM, "Notes"));
  await waitFor(cdp, "Boolean(document.querySelector('.list-panel .notes-entry'))", "the Notes library");
  await cdp.evaluate(snippets.openNotesEntry(title));
  await waitFor(cdp, noteOpen(title), `note "${title.slice(0, 40)}" to open`);
  await delay(200);
  return cdp.evaluate(snippets.noteMetaState());
}

const transcribedItems = (meta) => meta.items.filter((item) => item.text.startsWith("Transcribed"));

/** Exactly one item reading `line`, drawn with the computer icon, inside the row and the window. */
const saysOnce = (meta, line) => {
  const found = transcribedItems(meta);
  return meta.row && found.length === 1 && found[0].text === line && found[0].icon === COMPUTER
    && found[0].right <= meta.rowRight && found[0].right <= meta.innerWidth;
};

const SETUP_NOTE = "Each check starts from a fresh load at 1000x720. Notes are opened from the Notes library by their title. \"In the row\" is each item's right edge at or inside .note-meta's and the window's.";

export async function runNoteMeta(options) {
  const out = resolve(options.out);
  await mkdir(out, { recursive: true });
  let browser;
  let cdp;
  try {
    browser = await launchEdge({ ...options, out });
    cdp = await connectCdp(browser.target.webSocketDebuggerUrl);
    await cdp.send("Page.enable");
    if (options.colorScheme) await cdp.send("Emulation.setEmulatedMedia", {
      features: [{ name: "prefers-color-scheme", value: options.colorScheme }],
    });
    const report = createReport({
      url: options.url,
      edgeVersion: browser.version.Browser ?? browser.version,
      fixtureBannerRemoved: true,
      timestamp: new Date().toISOString(),
      colorScheme: options.colorScheme ?? "system",
      measurementScope: "measured in headless Edge against dev fixtures, not in the Tauri window",
    });

    /** Runs one check; a step that throws is the check measured false, with the meta row as it was. */
    const check = async ({ id, claim, expectation }, run) => {
      try {
        const result = await run();
        addCheck(report, { id, claim, expectation, note: SETUP_NOTE, ...result });
      } catch (error) {
        let meta = null;
        try { meta = await cdp.evaluate(snippets.noteMetaState()); } catch { /* no page to read */ }
        addCheck(report, { id, claim, expectation, measured: { failedAt: error.message, meta }, matches: false, note: SETUP_NOTE });
      }
    };

    await check({
      id: "1",
      claim: "A meeting transcribed on this computer says so in its meta row, with the computer icon.",
      expectation: `fixture note 101: exactly one meta item starting "Transcribed", reading "${HERE}", its icon the computer path, inside the row and the window; the date, length and participants still there`,
    }, async () => {
      await freshShell(cdp);
      const meta = await openNote(cdp, NOTE_101);
      const shot = "01-transcribed-here-1000x720.png";
      await cdp.screenshot(resolve(out, shot));
      return {
        measured: meta,
        matches: saysOnce(meta, HERE) && meta.items.length === 4,
        screenshots: [shot],
      };
    });

    await check({
      id: "2",
      claim: "An import transcribed by a cloud provider names it.",
      expectation: `fixture note 104: exactly one meta item starting "Transcribed", reading "${BY_OPENAI}", its icon the computer path, inside the row and the window`,
    }, async () => {
      await freshShell(cdp);
      const meta = await openNote(cdp, NOTE_104);
      const shot = "02-transcribed-by-openai-1000x720.png";
      await cdp.screenshot(resolve(out, shot));
      return { measured: meta, matches: saysOnce(meta, BY_OPENAI), screenshots: [shot] };
    });

    await check({
      id: "3",
      claim: "A note with no record of where it was transcribed says nothing about it, and its meta row keeps its other facts.",
      expectation: "fixture note 102: no meta item starting \"Transcribed\"; the row still there with its date, length and participant",
    }, async () => {
      await freshShell(cdp);
      const meta = await openNote(cdp, NOTE_102);
      return {
        measured: meta,
        matches: meta.row && transcribedItems(meta).length === 0 && meta.items.length === 3,
      };
    });

    await check({
      id: "4",
      claim: "A meeting recorded with live captions from off this computer, then stopped, says it was transcribed here and names the captions' provider; the line stays in the window at 720x520.",
      expectation: [
        "Start recording on Meetings with the fixtures' default provider, then Stop and save; the new note, opened from Notes,",
        `has exactly one meta item starting "Transcribed", reading "${CAPTIONED}", with the computer icon, inside the row and the window;`,
        "at 720x520 the same, with nothing scrolling sideways",
      ].join(" "),
    }, async () => {
      await freshShell(cdp);
      await cdp.evaluate(snippets.installInvokeStub());
      await cdp.evaluate(snippets.clickText(RAIL_ITEM, "Meetings"));
      await waitFor(cdp, START_ENABLED, "Start recording to be enabled");
      await pressText(cdp, ".control-start", "Start recording");
      await waitFor(cdp, LIVE, "the live meeting view");
      await delay(300);
      const started = await cdp.evaluate(snippets.stubCalls("meeting_start"));
      await pressText(cdp, ACTION, "Stop and save");
      await waitFor(cdp, IDLE, "the page to go idle");
      await delay(300);
      const newest = await cdp.evaluate(snippets.invoke("notes_list", { noteType: null, folderId: null, limit: 1 }));
      const title = newest?.[0]?.title ?? null;
      if (typeof title !== "string" || !UNNAMED.test(title)) throw new Error(`the newest note is ${JSON.stringify(title)}, not the meeting`);
      const wide = await openNote(cdp, title);
      const shot = "04-live-captions-1000x720.png";
      await cdp.screenshot(resolve(out, shot));
      await viewport(cdp, SMALL);
      await delay(500);
      const small = await cdp.evaluate(snippets.noteMetaState());
      const smallShot = "04-live-captions-720x520.png";
      await cdp.screenshot(resolve(out, smallShot));
      return {
        measured: { provider: started[0]?.provider ?? null, wide, small },
        matches: saysOnce(wide, CAPTIONED) && saysOnce(small, CAPTIONED) && small.scrollWidth <= small.innerWidth,
        screenshots: [shot, smallShot],
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
    if (options.help) process.stdout.write(`${shellUsage().replace("shell-rail.mjs", "note-meta.mjs")}\n`);
    else {
      const failed = failedChecks(await runNoteMeta(options));
      // The run completed; a check measured false still fails it, so a wrapper or CI step sees it.
      if (failed.length) {
        console.error(`note-meta: ${failed.length} check(s) false: ${failed.join(", ")}`);
        process.exitCode = 1;
      }
    }
  } catch (error) {
    console.error(`note-meta failed: ${error.message}`);
    process.exitCode = 1;
  }
}
