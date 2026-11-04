#!/usr/bin/env node
/**
 * Measures, against dev fixtures, the live sound-level meters (KNEG-371):
 * Dictation's while it records, the Meetings page's "You" and "Others", and
 * the overlay's, in its label mode and as the floating microphone, each at the
 * overlay window's real size.
 *
 * Levels reach the page as Rust's publishers send them, one `audio-level`
 * event per reading. The fixtures' dev console sends them here: `kw.level`
 * one reading, `kw.levels` ten a second, and the fixtures' own `meeting_start`
 * ten a second per capture that opened, with a last 0 at Stop or Discard.
 * Dictation's recording is announced with `kw.dictationState`, since the
 * fixtures never start one. Whether a real microphone moves a meter is not
 * measured here: no audio reaches a browser tab.
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

const NAV_PILL = "button.nav-pill";
const ACTION = ".live-actions button";
const HERO_METER = ".hero-meter .level-meter";
/** `STALE_MS` in `apps/desktop/src/lib/levelMeter.ts`. */
const STALE_MS = 400;
/** `REDUCED_MOTION_INTERVAL_MS` there: at most four changes a second. */
const REDUCED_MOTION_INTERVAL_MS = 250;
/** The overlay window's sizes: `overlay.rs` (label mode) and `persistent_overlay.rs`' `SIZE` (the floating microphone). */
const LABEL_WINDOW = { width: 280, height: 96 };
const FLOATING_WINDOW = { width: 96, height: 96 };

const LIVE = "Boolean(document.querySelector('.meeting-live'))";
const START_ENABLED = "(() => { const start = document.querySelector('.control-start'); return Boolean(start) && !start.disabled && start.textContent.trim() === 'Start recording'; })()";
const SHELL = "Boolean(document.querySelector('.app-titlebar') && document.querySelector('.split-main, .content'))";
const meterShows = (selector, level) => `document.querySelector(${JSON.stringify(selector)})?.dataset.level === ${JSON.stringify(level.toFixed(2))}`;
const noMeters = "document.querySelectorAll('.level-meter').length === 0";

async function waitFor(cdp, expression, description, timeout = 10_000) {
  const deadline = Date.now() + timeout;
  while (Date.now() < deadline) {
    try {
      if (await cdp.evaluate(expression)) return;
    } catch {
      // Between two documents during a load there is no context to evaluate in.
    }
    await delay(40);
  }
  throw new Error(`Timed out waiting for ${description}`);
}

async function navigate(cdp, url, timeout = 15_000) {
  let off = () => {};
  let timer;
  const loaded = new Promise((resolveLoad, reject) => {
    timer = setTimeout(() => reject(new Error(`Timed out loading ${url}`)), timeout);
    off = cdp.on("Page.loadEventFired", () => resolveLoad());
  });
  try {
    await cdp.send("Page.navigate", { url });
    await loaded;
  } finally {
    clearTimeout(timer);
    off();
  }
}

/** A fresh document at `url`, by way of a blank page, so a change of `#fragment` alone is a real load too. */
async function load(cdp, url) {
  await navigate(cdp, "about:blank");
  await navigate(cdp, url);
}

async function viewport(cdp, { width, height }) {
  await cdp.send("Emulation.setDeviceMetricsOverride", { width, height, deviceScaleFactor: 1, mobile: false });
  await delay(150);
}

async function reducedMotion(cdp, on) {
  await cdp.send("Emulation.setEmulatedMedia", { features: [{ name: "prefers-reduced-motion", value: on ? "reduce" : "no-preference" }] });
  await delay(150);
}

/** A real pointer press at a measured point, as a person's click is. */
async function press(cdp, { x, y }) {
  await cdp.send("Input.dispatchMouseEvent", { type: "mouseMoved", x, y });
  await cdp.send("Input.dispatchMouseEvent", { type: "mousePressed", x, y, button: "left", clickCount: 1 });
  await cdp.send("Input.dispatchMouseEvent", { type: "mouseReleased", x, y, button: "left", clickCount: 1 });
  await delay(150);
}

const meters = (cdp) => cdp.evaluate(snippets.levelMeters());

/**
 * Waits for the meter at `selector` to show `level`, then for its 80 ms fill
 * swap to finish, so the colours read are the meter's and not a frame of the
 * transition -- still well inside the 400 ms before a reading goes stale.
 */
async function settlesAt(cdp, selector, level, description) {
  await waitFor(cdp, meterShows(selector, level), description, 2_000);
  await delay(150);
}

const MAIN_WINDOW = { width: 1000, height: 720 };
/** The application's minimum window, where chip-menus check 8 measures the idle pages. */
const SMALLEST_WINDOW = { width: 720, height: 520 };

/** The main window, loaded at `size`, on `page`, the fixture banner removed. */
async function mainPage(cdp, url, page, size = MAIN_WINDOW) {
  await viewport(cdp, size);
  await load(cdp, url);
  await waitFor(cdp, SHELL, "the fixture shell");
  await cdp.evaluate(snippets.removeFixtureBanner());
  await cdp.evaluate(snippets.clickText(NAV_PILL, page));
}

/** Dictation, recording as the session announces it, with its meter mounted. */
async function dictationRecording(cdp, url, size = MAIN_WINDOW) {
  await mainPage(cdp, url, "Dictation", size);
  await waitFor(cdp, "Boolean(document.querySelector('.task-hero'))", "the Dictation hero");
  await cdp.evaluate(snippets.emitDictationState("recording"));
  await waitFor(cdp, `Boolean(document.querySelector(${JSON.stringify(HERO_METER)}))`, "the Dictation meter");
}

/** Meetings, a meeting started from Start recording with the Other participants chip `systemAudio`. */
async function meetingRecording(cdp, url, systemAudio, size = MAIN_WINDOW) {
  await mainPage(cdp, url, "Meetings", size);
  await waitFor(cdp, START_ENABLED, "Start recording to be enabled");
  const pressed = await cdp.evaluate(snippets.startMeetingRecording(systemAudio));
  await waitFor(cdp, LIVE, "the live meeting view");
  // Five of the fixtures' readings, 100 ms apart.
  await delay(500);
  return pressed;
}

/** The overlay page at `size`, the banner removed, in the floating microphone's mode or the label's. */
async function overlay(cdp, url, size, floating, vertical = false) {
  await viewport(cdp, size);
  await load(cdp, new URL("#overlay", url).href);
  await waitFor(cdp, "Boolean(globalThis.kw) && Boolean(document.querySelector('.overlay, .microphone-overlay'))", "the overlay");
  await cdp.evaluate(snippets.removeFixtureBanner());
  if (floating) {
    await cdp.evaluate(`(() => { globalThis.kw.overlayFloating(true, ${JSON.stringify(vertical)}); return true; })()`);
    await waitFor(cdp, "Boolean(document.querySelector('.floating-microphone'))", "the floating microphone");
  }
  await cdp.evaluate(snippets.emitDictationState("recording"));
}

const inside = (inner, outer) => inner.left >= outer.left - 0.5 && inner.top >= outer.top - 0.5
  && inner.right <= outer.right + 0.5 && inner.bottom <= outer.bottom + 0.5;
const inViewport = (meter) => meter.box.width > 0 && meter.box.height > 0
  && inside(meter.box, { left: 0, top: 0, right: meter.viewport.width, bottom: meter.viewport.height });
/**
 * Inside the window's width, wherever it is down the page. Meetings stacks at the
 * smallest window, and the page scrolls there by design, so a meter's height on the
 * page is not the claim; a meter cut off at the side would be.
 */
const inWidth = (meter) => meter.box.width > 0 && meter.box.height > 0
  && meter.box.left >= 0 && meter.box.right <= meter.viewport.width;
/** Decoration, as ruled: hidden from assistive technology, no `role`. */
const decoration = (meter) => meter.hiddenFromAssistiveTechnology && meter.role === null;
const drawsWhatItSays = (meter) => meter.litDrawn === meter.lit && meter.colours.lit !== meter.colours.unlit;

/** Each `audio-level` change of a meter, as gaps between them. */
function pace(changes) {
  const gaps = changes.slice(1).map((change, index) => Math.round(change.t - changes[index].t));
  return { changes: changes.length, minGapMs: gaps.length ? Math.min(...gaps) : null, gaps };
}

const SETUP_NOTE = "Measured in headless Edge against dev fixtures, not in the Tauri window. Levels are the fixtures' `audio-level` events (`kw.level`, `kw.levels`, `meeting_start`), sent as Rust's publishers send them; no audio reaches a browser tab, so whether a real microphone moves a meter is not measured here. The main window is 1000x720; the overlay is measured at its own window's size.";

export async function runLevelMeters(options) {
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

    /** Runs one check; a step that throws is the check measured false, with the meters as they were. */
    const check = async ({ id, claim, expectation, note = null }, run) => {
      const notes = [SETUP_NOTE, ...(note ? [note] : [])].join(" ");
      try {
        const result = await run();
        addCheck(report, { id, claim, expectation, note: notes, ...result });
      } catch (error) {
        let seen = null;
        try {
          seen = await meters(cdp);
        } catch { /* no page to read */ }
        addCheck(report, { id, claim, expectation, measured: { failedAt: error.message, meters: seen }, matches: false, note: notes });
      } finally {
        await reducedMotion(cdp, false).catch(() => {});
      }
    };

    await check({
      id: "1",
      claim: "While a dictation records, the Dictation hero shows one meter, hidden from assistive technology, that follows the level it is sent; at idle it is gone.",
      expectation: [
        "after `dictation-state` recording: exactly one `.level-meter`, in `.hero-meter`, inside the window, `aria-hidden` and no `role`, 9 segments, showing 0;",
        "after one reading of 0.75: `data-level` 0.75 and 7 of 9 segments lit and drawn lit, in a colour other than the unlit ones;",
        "after `dictation-state` idle: no `.level-meter` on the page;",
        "on a fresh load at 720x520, recording: the meter inside the window, and nothing wider than the window",
      ].join(" "),
    }, async () => {
      await dictationRecording(cdp, options.url);
      const atStart = await meters(cdp);
      await cdp.evaluate(snippets.emitLevel(0.75));
      await settlesAt(cdp, HERO_METER, 0.75, "the meter to show 0.75");
      const following = await meters(cdp);
      // Moving levels for the picture, since a single reading goes stale in 400 ms.
      await cdp.evaluate(snippets.fakeLevels("dictation", true));
      await delay(400);
      const shot = "01-dictation-recording-1000x720.png";
      await cdp.screenshot(resolve(out, shot));
      await cdp.evaluate(snippets.fakeLevels("dictation", false));
      await cdp.evaluate(snippets.emitDictationState("idle"));
      await waitFor(cdp, noMeters, "the meter to go at idle", 3_000);
      const atIdle = await meters(cdp);

      await dictationRecording(cdp, options.url, SMALLEST_WINDOW);
      const smallest = await meters(cdp);
      const overflow = await cdp.evaluate(snippets.overflowState());
      const [meter] = following;
      return {
        measured: { atStart, following, atIdle, smallest, overflow },
        matches: atStart.length === 1 && atStart[0].className === "level-meter level-wave" && atStart[0].level === 0 && atStart[0].lit === 0
          && following.length === 1 && meter.ariaHidden === "true" && decoration(meter) && meter.segments === 9
          && meter.level === 0.75 && meter.lit === 7 && drawsWhatItSays(meter) && inViewport(meter)
          && atIdle.length === 0
          && smallest.length === 1 && inViewport(smallest[0])
          && overflow.scrollWidth <= overflow.innerWidth
          && (overflow.mainScrollWidth === null || overflow.mainScrollWidth <= overflow.mainClientWidth),
        screenshots: [shot],
      };
    });

    await check({
      id: "2",
      claim: "A level not heard again for 400 ms falls to 0 while the meter is still up, so a meter cannot stay lit over a capture that went away without its last 0.",
      expectation: `one reading of 0.75 shows 7 of the hero's 9 segments lit; after ${STALE_MS + 250} ms with no reading the same meter shows 0 with none lit, and is still mounted`,
    }, async () => {
      await dictationRecording(cdp, options.url);
      await cdp.evaluate(snippets.emitLevel(0.75));
      await settlesAt(cdp, HERO_METER, 0.75, "the meter to show 0.75");
      const fresh = await meters(cdp);
      await delay(STALE_MS + 250);
      const stale = await meters(cdp);
      return {
        measured: { fresh, stale },
        matches: fresh.length === 1 && fresh[0].lit === 7
          && stale.length === 1 && stale[0].level === 0 && stale[0].lit === 0 && stale[0].litDrawn === 0,
      };
    });

    await check({
      id: "3",
      claim: "A meeting recording both sources shows \"You\" and \"Others\" meters in its live header, each with its own level, without making the smallest window scroll sideways; one recording the microphone only shows \"You\" alone; Stop and save takes them away.",
      expectation: [
        "Start recording with Other participants pressed: two meters in `.live-levels` labelled you and others, both inside the window and hidden from assistive technology with no role, a segment's transition-duration 0.08s, both above 0 and lit differently (the fixtures' microphone moves around 0.62, the others around 0.3);",
        "after Stop and save, the page saying so with no `.level-meter`;",
        "on a fresh page, Start recording with the chip released: one meter, labelled you, above 0;",
        "on a fresh load at 720x520, Start recording with the chip pressed: both meters inside the window's width, the page scrolling down to them by design, and neither the document nor `.split-main` wider than it shows (scrollWidth <= innerWidth, as chip-menus check 8 measures)",
      ].join(" "),
    }, async () => {
      const bothPressed = await meetingRecording(cdp, options.url, true);
      const both = await meters(cdp);
      const shot = "03-meeting-both-sources-1000x720.png";
      await cdp.screenshot(resolve(out, shot));
      await press(cdp, await cdp.evaluate(snippets.centerOfText(ACTION, "Stop and save")));
      await waitFor(cdp, snippets.MEETING_SAVED, "the page to say the meeting was saved");
      const afterStop = await meters(cdp);

      const micOnlyPressed = await meetingRecording(cdp, options.url, false);
      const micOnly = await meters(cdp);
      const shotMic = "03-meeting-microphone-only-1000x720.png";
      await cdp.screenshot(resolve(out, shotMic));

      await meetingRecording(cdp, options.url, true, SMALLEST_WINDOW);
      const smallest = await meters(cdp);
      const overflow = await cdp.evaluate(snippets.overflowState());
      const shotSmall = "03-meeting-both-sources-720x520.png";
      await cdp.screenshot(resolve(out, shotSmall));
      const [you, others] = both;
      return {
        measured: { bothPressed, both, afterStop, micOnlyPressed, micOnly, smallest, overflow },
        matches: bothPressed.systemAudio === true && both.length === 2
          && you.label === "you" && others.label === "others"
          && both.every((meter) => decoration(meter) && inViewport(meter) && meter.transition === "0.08s" && meter.level > 0 && meter.lit > 0 && drawsWhatItSays(meter))
          && you.level !== others.level && you.lit !== others.lit
          && afterStop.length === 0
          && micOnlyPressed.systemAudio === false && micOnly.length === 1 && micOnly[0].label === "you" && micOnly[0].level > 0
          && smallest.length === 2 && smallest.every(inWidth)
          && overflow.scrollWidth <= overflow.innerWidth
          && (overflow.mainScrollWidth === null || overflow.mainScrollWidth <= overflow.mainClientWidth),
        screenshots: [shot, shotMic, shotSmall],
      };
    });

    await check({
      id: "4",
      claim: "Under reduced motion a meter has no transition and changes at most four times a second; without it every reading shows.",
      expectation: [
        "on the Dictation meter with the fixtures sending ten readings a second for 2 s each way:",
        "without emulation, at least 12 changes (the hero's wave draws no transition of its own: `.level-wave .level-segment`; check 3 holds the 80 ms fill swap on the meters that keep it);",
        `with prefers-reduced-motion: reduce emulated, 0.01ms or less (styles.css' rule for every element), at most 10 changes, and none closer than ${REDUCED_MOTION_INTERVAL_MS - 10} ms to the one before`,
      ].join(" "),
      note: "Changes are the meter's `data-level` attribute, timestamped by a MutationObserver in the page; a reading equal to the one shown changes nothing, so the counts are at most the readings sent. The 10 ms allowance is for timer and commit jitter.",
    }, async () => {
      await dictationRecording(cdp, options.url);
      const sample = async () => {
        await cdp.evaluate(snippets.watchLevelChanges(HERO_METER));
        await cdp.evaluate(snippets.fakeLevels("dictation", true));
        await delay(2_000);
        await cdp.evaluate(snippets.fakeLevels("dictation", false));
        const changes = await cdp.evaluate(snippets.levelChanges());
        const [meter] = await meters(cdp);
        return { transition: meter?.transition ?? null, ...pace(changes) };
      };
      const normal = await sample();
      await reducedMotion(cdp, true);
      await delay(STALE_MS + 100);
      const reduced = await sample();
      return {
        measured: { normal, reduced },
        matches: normal.transition === "0s" && normal.changes >= 12
          && Number.parseFloat(reduced.transition) <= 0.00001 && reduced.changes <= 10
          && reduced.minGapMs !== null && reduced.minGapMs >= REDUCED_MOTION_INTERVAL_MS - 10,
      };
    });

    await check({
      id: "5",
      claim: "The overlay's label mode shows the meter beside \"Recording\" inside its 280x96 window while dictation records, and not at idle.",
      expectation: [
        "`#overlay` at 280x96 with `dictation-state` recording: one `.overlay-level` meter of 8 segments, inside the window and to the right of the \"Recording\" text, decoration;",
        "after one reading of 0.75, 6 of 8 lit; after `dictation-state` idle, none",
      ].join(" "),
    }, async () => {
      await overlay(cdp, options.url, LABEL_WINDOW, false);
      await waitFor(cdp, "Boolean(document.querySelector('.overlay .level-meter'))", "the overlay's meter");
      await cdp.evaluate(snippets.emitLevel(0.75));
      await settlesAt(cdp, ".overlay .level-meter", 0.75, "the overlay meter to show 0.75");
      const recording = await meters(cdp);
      const text = await cdp.evaluate("(() => { const text = document.querySelector('.overlay-text'); const rect = text.getBoundingClientRect(); return { text: text.textContent.trim(), right: rect.right }; })()");
      await cdp.evaluate(snippets.fakeLevels("dictation", true));
      await delay(400);
      const shot = "05-overlay-label-280x96.png";
      await cdp.screenshot(resolve(out, shot));
      await cdp.evaluate(snippets.fakeLevels("dictation", false));
      await cdp.evaluate(snippets.emitDictationState("idle"));
      await waitFor(cdp, noMeters, "the overlay's meter to go at idle", 3_000);
      const [meter] = recording;
      return {
        measured: { recording, text },
        matches: recording.length === 1 && meter.className === "level-meter overlay-level" && meter.segments === 8
          && meter.lit === 6 && drawsWhatItSays(meter) && decoration(meter) && inViewport(meter)
          && text.text === "Recording" && meter.box.left >= text.right,
        screenshots: [shot],
      };
    });

    await check({
      id: "6",
      claim: "The floating microphone shows a strip of the level inside its button while dictation records, horizontal and vertical, inside its 96x96 window.",
      expectation: [
        "`#overlay` at 96x96 in the floating microphone's mode, horizontal and then vertical, `dictation-state` recording:",
        "one `.microphone-level` meter of 6 segments inside the `.floating-microphone` button and the window, decoration;",
        "after one reading of 0.75, 5 of 6 lit (4.5 rounded), drawn in a colour other than the unlit ones; after `dictation-state` idle, none",
      ].join(" "),
    }, async () => {
      const measured = {};
      const screenshots = [];
      for (const vertical of [false, true]) {
        const name = vertical ? "vertical" : "horizontal";
        await overlay(cdp, options.url, FLOATING_WINDOW, true, vertical);
        await waitFor(cdp, "Boolean(document.querySelector('.floating-microphone .level-meter'))", "the floating microphone's meter");
        await cdp.evaluate(snippets.emitLevel(0.75));
        await settlesAt(cdp, ".floating-microphone .level-meter", 0.75, "the strip to show 0.75");
        const recording = await meters(cdp);
        await cdp.evaluate(snippets.fakeLevels("dictation", true));
        await delay(400);
        const shot = `06-floating-${name}-96x96.png`;
        await cdp.screenshot(resolve(out, shot));
        screenshots.push(shot);
        await cdp.evaluate(snippets.fakeLevels("dictation", false));
        await cdp.evaluate(snippets.emitDictationState("idle"));
        await waitFor(cdp, noMeters, "the strip to go at idle", 3_000);
        measured[name] = recording;
      }
      const placed = (recording) => recording.length === 1
        && recording[0].className === "level-meter microphone-level" && recording[0].segments === 6 && recording[0].lit === 5
        && drawsWhatItSays(recording[0]) && decoration(recording[0]) && inViewport(recording[0])
        && recording[0].host?.className.includes("floating-microphone") && inside(recording[0].box, recording[0].host.box);
      return { measured, matches: placed(measured.horizontal) && placed(measured.vertical), screenshots };
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
    if (options.help) process.stdout.write(`${shellUsage().replace("shell-title-bar.mjs", "level-meters.mjs")}\n`);
    else {
      const failed = failedChecks(await runLevelMeters(options));
      // The run completed; a check measured false still fails it, so a wrapper or CI step sees it.
      if (failed.length) {
        console.error(`level-meters: ${failed.length} check(s) false: ${failed.join(", ")}`);
        process.exitCode = 1;
      }
    }
  } catch (error) {
    console.error(`level-meters failed: ${error.message}`);
    process.exitCode = 1;
  }
}
