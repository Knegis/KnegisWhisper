#!/usr/bin/env node
/**
 * Measures, against dev fixtures with scrollbars actually drawn, the app's one
 * scrollbar: a 6px rounded handle with no arrows, inside the note sheet and clear
 * of its corners, hidden until you scroll or point at it, still draggable, moving
 * no content when it appears, and the system's own in forced colors.
 *
 * Every other check here runs with `--hide-scrollbars`, so this one asks
 * `launchEdge` not to. The handle is found in the screenshot's pixels, because
 * nothing in the DOM says where a scrollbar's thumb was painted. Not the real
 * window; WebView2 is the same engine, but this says nothing about WebKit on the
 * other platforms.
 */
import { mkdir, writeFile } from "node:fs/promises";
import { resolve } from "node:path";
import { inflateSync } from "node:zlib";
import { parseShellArgs, shellUsage } from "./args.mjs";
import { launchEdge, killBrowser } from "./browser.mjs";
import { connectCdp } from "./cdp.mjs";
import { createReport, addCheck, failedChecks, renderMarkdown } from "./report.mjs";
import * as snippets from "./snippets.mjs";
import { isEntryPoint } from "../ci/entry.mjs";

const delay = (ms) => new Promise((resolveDelay) => setTimeout(resolveDelay, ms));
const isMain = isEntryPoint(import.meta.url);

const NAV_PILL = "button.nav-pill";
const WINDOW = { width: 1280, height: 800 };
const SMALL = { width: 720, height: 520 };
const NOTE = ".note-body";
/** Longer than `idleScrollbars.ts` keeps a handle after the last scroll (900ms). */
const IDLE_MS = 1300;

/** 8-bit RGB or RGBA, not interlaced, which is what Page.captureScreenshot writes. */
function readPng(buffer) {
  let at = 8;
  let width = 0;
  let height = 0;
  let channels = 0;
  const data = [];
  while (at < buffer.length) {
    const length = buffer.readUInt32BE(at);
    const kind = buffer.toString("latin1", at + 4, at + 8);
    const body = buffer.subarray(at + 8, at + 8 + length);
    if (kind === "IHDR") {
      width = body.readUInt32BE(0);
      height = body.readUInt32BE(4);
      channels = body[9] === 6 ? 4 : 3;
    } else if (kind === "IDAT") data.push(body);
    at += 12 + length;
  }
  const raw = inflateSync(Buffer.concat(data));
  const stride = width * channels;
  const rows = [];
  let previous = new Uint8Array(stride);
  let from = 0;
  for (let y = 0; y < height; y += 1) {
    const filter = raw[from];
    const row = Uint8Array.from(raw.subarray(from + 1, from + 1 + stride));
    from += 1 + stride;
    for (let i = 0; i < stride; i += 1) {
      const a = i >= channels ? row[i - channels] : 0;
      const b = previous[i];
      const c = i >= channels ? previous[i - channels] : 0;
      let add = 0;
      if (filter === 1) add = a;
      else if (filter === 2) add = b;
      else if (filter === 3) add = (a + b) >> 1;
      else if (filter === 4) {
        const p = a + b - c;
        const pa = Math.abs(p - a);
        const pb = Math.abs(p - b);
        const pc = Math.abs(p - c);
        add = pa <= pb && pa <= pc ? a : pb <= pc ? b : c;
      }
      row[i] = (row[i] + add) & 255;
    }
    rows.push(row);
    previous = row;
  }
  const luminance = (x, y) => {
    const row = rows[y];
    const i = x * channels;
    return (row[i] * 299 + row[i + 1] * 587 + row[i + 2] * 114) / 1000;
  };
  return { width, height, luminance };
}

const boxOf = (cdp, selector) => cdp.evaluate(`(() => {
  const r = document.querySelector(${JSON.stringify(selector)}).getBoundingClientRect();
  return { left: r.left, top: r.top, right: r.right, bottom: r.bottom };
})()`);

/**
 * What's painted in `selector`'s last `track` pixels on the right, against the
 * track's own colour: where the handle is, relative to the box, or `null` for none.
 */
async function handleIn(cdp, selector, track = 16) {
  const box = await boxOf(cdp, selector);
  const shot = await cdp.send("Page.captureScreenshot", { format: "png", captureBeyondViewport: false });
  const image = readPng(Buffer.from(shot.data, "base64"));
  const right = Math.round(box.right);
  const left = right - track;
  const top = Math.max(0, Math.round(box.top));
  const bottom = Math.min(image.height, Math.round(box.bottom));
  let found = null;
  for (let y = top; y < bottom; y += 1) {
    // The middle of the box's height is white sheet or page, and this edge pixel is
    // in the track's transparent border in either case.
    const ground = image.luminance(left, y);
    for (let x = left; x < right; x += 1) {
      if (Math.abs(image.luminance(x, y) - ground) > 40) {
        found ??= { left: x, right: x + 1, top: y, bottom: y + 1 };
        found.left = Math.min(found.left, x);
        found.right = Math.max(found.right, x + 1);
        found.bottom = y + 1;
      }
    }
  }
  if (found === null) return null;
  return {
    width: found.right - found.left,
    fromEdge: right - found.right,
    fromTop: found.top - top,
    fromBottom: bottom - found.bottom,
    centreX: (found.left + found.right) / 2,
    centreY: (found.top + found.bottom) / 2,
  };
}

const pseudo = (cdp, selector) => cdp.evaluate(`(() => {
  const el = document.querySelector(${JSON.stringify(selector)});
  if (!el) return null;
  return {
    track: getComputedStyle(el, '::-webkit-scrollbar').width,
    arrows: getComputedStyle(el, '::-webkit-scrollbar-button').display,
    color: getComputedStyle(el).scrollbarColor,
    barWidth: getComputedStyle(el).scrollbarWidth,
  };
})()`);

const scrollTop = (cdp, selector) => cdp.evaluate(`document.querySelector(${JSON.stringify(selector)}).scrollTop`);
const moveTo = (cdp, x, y, buttons = 0) => cdp.send("Input.dispatchMouseEvent", { type: "mouseMoved", x, y, buttons, button: buttons ? "left" : "none" });
/** Over the title bar's logo: on no scroller's edge. */
const moveAway = (cdp) => moveTo(cdp, 640, 28);

async function pointAtTrack(cdp, selector) {
  const box = await boxOf(cdp, selector);
  await moveTo(cdp, box.right - 8, (box.top + box.bottom) / 2);
  await delay(200);
}

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

/** A fresh load, with the pointer away from every scroller. */
async function freshShell(cdp, size = WINDOW) {
  await cdp.send("Emulation.setDeviceMetricsOverride", { ...size, deviceScaleFactor: 1, mobile: false });
  await moveAway(cdp);
  await reload(cdp);
  await waitFor(cdp, "Boolean(document.querySelector('.app-titlebar') && document.querySelector('.split-main, .content'))", "the first paint");
  await cdp.evaluate(snippets.removeFixtureBanner());
}

/** Notes, at the size where the note and the transcript scroll on their own, with a note that overflows. */
async function openNotes(cdp) {
  await freshShell(cdp);
  await cdp.evaluate(snippets.clickText(NAV_PILL, "Notes"));
  await waitFor(cdp, `(() => { const b = document.querySelector(${JSON.stringify(NOTE)}); return Boolean(b) && b.scrollHeight > b.clientHeight; })()`, "a note that overflows");
  await delay(500);
}

const SETUP_NOTE = "Each check starts from a fresh load at 1280x800 unless it says otherwise, with scrollbars drawn: this one check runs without --hide-scrollbars.";

export async function runScrollbars(options) {
  const out = resolve(options.out);
  await mkdir(out, { recursive: true });
  let browser;
  let cdp;
  try {
    browser = await launchEdge({ ...options, out, width: WINDOW.width, height: WINDOW.height, hideScrollbars: false });
    cdp = await connectCdp(browser.target.webSocketDebuggerUrl);
    await cdp.send("Page.enable");
    const report = createReport({
      url: options.url,
      edgeVersion: browser.version.Browser ?? browser.version,
      fixtureBannerRemoved: true,
      timestamp: new Date().toISOString(),
      colorScheme: "as each check says",
      measurementScope: "measured in headless Edge against dev fixtures, not in the Tauri window",
    });

    /** Runs one check; a step that throws is the check measured false. */
    const check = async ({ id, claim, expectation }, run) => {
      try {
        addCheck(report, { id, claim, expectation, note: SETUP_NOTE, ...(await run()) });
      } catch (error) {
        addCheck(report, { id, claim, expectation, measured: { failedAt: error.message }, matches: false, note: SETUP_NOTE });
      }
    };

    await check({
      id: "1",
      claim: "The note sheet's handle is slim, inside the sheet, clear of its corners, and has no arrows.",
      expectation: "pointed at: 6px wide, 5px in from the sheet's edge, at least 32px from its top and bottom, and the arrows' display none",
    }, async () => {
      await openNotes(cdp);
      await pointAtTrack(cdp, NOTE);
      const handle = await handleIn(cdp, NOTE);
      const style = await pseudo(cdp, NOTE);
      const shot = "01-pointed-at-1280x800.png";
      await cdp.screenshot(resolve(out, shot));
      return {
        measured: { handle, style },
        matches: handle?.width === 6 && handle.fromEdge === 5 && handle.fromTop >= 32 && handle.fromBottom >= 32 && style.arrows === "none",
        screenshots: [shot],
      };
    });

    await check({
      id: "2",
      claim: "At rest nothing is drawn; scrolling shows the handle, and it goes again once scrolling stops.",
      expectation: "no handle before; a handle 150ms after a wheel, with the note scrolled; none again after 1300ms",
    }, async () => {
      await openNotes(cdp);
      const before = await handleIn(cdp, NOTE);
      const box = await boxOf(cdp, NOTE);
      await cdp.send("Input.dispatchMouseEvent", { type: "mouseWheel", x: (box.left + box.right) / 2, y: (box.top + box.bottom) / 2, deltaX: 0, deltaY: 120 });
      await delay(150);
      const during = await handleIn(cdp, NOTE);
      const scrolled = await scrollTop(cdp, NOTE);
      await delay(IDLE_MS);
      const after = await handleIn(cdp, NOTE);
      return {
        measured: { before, during, scrolled, after },
        matches: before === null && during !== null && scrolled > 0 && after === null,
      };
    });

    await check({
      id: "3",
      claim: "Pointing at the track shows the handle for as long as the pointer stays, and leaving hides it.",
      expectation: "a handle while pointed at and still after 1300ms there; none 1300ms after leaving",
    }, async () => {
      await openNotes(cdp);
      await pointAtTrack(cdp, NOTE);
      const pointed = await handleIn(cdp, NOTE);
      await delay(IDLE_MS);
      const stayed = await handleIn(cdp, NOTE);
      await moveAway(cdp);
      await delay(IDLE_MS);
      const left = await handleIn(cdp, NOTE);
      return { measured: { pointed, stayed, left }, matches: pointed !== null && stayed !== null && left === null };
    });

    await check({
      id: "4",
      claim: "The handle can still be dragged.",
      expectation: "pressing the handle and moving down 100px scrolls the note further than it was",
    }, async () => {
      await openNotes(cdp);
      await pointAtTrack(cdp, NOTE);
      const handle = await handleIn(cdp, NOTE);
      const box = await boxOf(cdp, NOTE);
      const x = Math.round(box.right) - 8;
      const y = handle.centreY;
      await moveTo(cdp, x, y);
      await cdp.send("Input.dispatchMouseEvent", { type: "mousePressed", x, y, button: "left", buttons: 1, clickCount: 1 });
      await moveTo(cdp, x, y + 50, 1);
      await moveTo(cdp, x, y + 100, 1);
      await cdp.send("Input.dispatchMouseEvent", { type: "mouseReleased", x, y: y + 100, button: "left", buttons: 0, clickCount: 1 });
      await delay(150);
      const scrolled = await scrollTop(cdp, NOTE);
      return { measured: { handle, scrolled }, matches: scrolled > 0 };
    });

    await check({
      id: "5",
      claim: "The handle appearing moves no content.",
      expectation: "the note page's left and width are the same at rest and pointed at",
    }, async () => {
      await openNotes(cdp);
      const page = () => cdp.evaluate("(() => { const r = document.querySelector('.note-page').getBoundingClientRect(); return { left: r.left, width: r.width }; })()");
      const rest = await page();
      await pointAtTrack(cdp, NOTE);
      const pointed = await page();
      return { measured: { rest, pointed }, matches: rest.left === pointed.left && rest.width === pointed.width };
    });

    await check({
      id: "6",
      claim: "It's the same bar on the app's other scrollers, and the notes list keeps its width.",
      expectation: "no arrows and no scrollbar-color or scrollbar-width on the list, the transcript column and the note; a 10px track on the list, 16px elsewhere; the list's content 290 wide",
    }, async () => {
      await openNotes(cdp);
      const list = await pseudo(cdp, ".list-panel");
      const column = await pseudo(cdp, ".note-side-body");
      const note = await pseudo(cdp, NOTE);
      const contentWidth = await cdp.evaluate("document.querySelector('.list-panel > .panel-surface').getBoundingClientRect().width");
      const native = (style) => style.color === "auto" && style.barWidth === "auto" && style.arrows === "none";
      return {
        measured: { list, column, note, contentWidth },
        matches: native(list) && native(column) && native(note)
          && list.track === "10px" && column.track === "16px" && note.track === "16px" && contentWidth === 290,
      };
    });

    await check({
      id: "7",
      claim: "A settings screen's page scroller shows the same handle, and the smallest window scrolls nothing sideways.",
      expectation: "at 720x520 on Settings > Meetings: pointed at the page's track, a 6px handle; .split-main scrollWidth no more than its clientWidth",
    }, async () => {
      await freshShell(cdp, SMALL);
      await cdp.evaluate(snippets.openSettingsScreen("Meetings"));
      await waitFor(cdp, "Boolean(document.querySelector('.page-header h1'))", "Settings > Meetings");
      await delay(500);
      await pointAtTrack(cdp, ".split-main");
      const handle = await handleIn(cdp, ".split-main");
      const fit = await cdp.evaluate("(() => { const m = document.querySelector('.split-main'); return { scrollWidth: m.scrollWidth, clientWidth: m.clientWidth }; })()");
      const shot = "07-settings-720x520.png";
      await cdp.screenshot(resolve(out, shot));
      return { measured: { handle, fit }, matches: handle?.width === 6 && fit.scrollWidth <= fit.clientWidth, screenshots: [shot] };
    });

    await check({
      id: "8",
      claim: "In dark, the handle shows against the sheet.",
      expectation: "with the dark scheme emulated, a 6px handle when pointed at",
    }, async () => {
      await cdp.send("Emulation.setEmulatedMedia", { features: [{ name: "prefers-color-scheme", value: "dark" }] });
      try {
        await openNotes(cdp);
        await pointAtTrack(cdp, NOTE);
        const handle = await handleIn(cdp, NOTE);
        const shot = "08-dark-1280x800.png";
        await cdp.screenshot(resolve(out, shot));
        return { measured: { handle }, matches: handle?.width === 6, screenshots: [shot] };
      } finally {
        await cdp.send("Emulation.setEmulatedMedia", { features: [] });
      }
    });

    await check({
      id: "9",
      claim: "In forced colors the system's own scrollbar is left alone.",
      expectation: "with forced-colors active, the track's computed width is no longer 16px",
    }, async () => {
      await cdp.send("Emulation.setEmulatedMedia", { features: [{ name: "forced-colors", value: "active" }] });
      try {
        await openNotes(cdp);
        const style = await pseudo(cdp, NOTE);
        return { measured: style, matches: style.track !== "16px" };
      } finally {
        await cdp.send("Emulation.setEmulatedMedia", { features: [] });
      }
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
    if (options.help) process.stdout.write(`${shellUsage().replace("shell-title-bar.mjs", "scrollbars.mjs")}\n`);
    else {
      const failed = failedChecks(await runScrollbars(options));
      if (failed.length) {
        console.error(`scrollbars: ${failed.length} check(s) false: ${failed.join(", ")}`);
        process.exitCode = 1;
      }
    }
  } catch (error) {
    console.error(`scrollbars failed: ${error.message}`);
    process.exitCode = 1;
  }
}
