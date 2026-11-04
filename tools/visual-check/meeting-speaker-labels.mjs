#!/usr/bin/env node
/**
 * Measures, against dev fixtures, the Speaker labels chip on Meetings: a link
 * to Settings > Meetings until the models and runtime are there, then a toggle
 * that writes `meetings.diarize` and says what it costs, a refused write in
 * view, and a statement rather than a control while a meeting runs.
 *
 * `kw.speakerModels()` installs the models in the fixtures. The fixtures keep
 * that only until a reload, so each check calls it after its fresh load and
 * before it opens Meetings. What the setting does to a saved note is pinned by
 * `cargo test` (`meeting/notes.rs`), not here.
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
const WINDOW = { width: 1000, height: 720 };
const SMALL = { width: 720, height: 520 };
const LABEL = "Speaker labels";
const NEEDS_SETUP = "Speaker labels: needs setup";
const START_ENABLED = "(() => { const start = document.querySelector('.control-start'); return Boolean(start) && !start.disabled; })()";
const LIVE = "Boolean(document.querySelector('.meeting-live'))";

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

async function freshShell(cdp, { width, height } = WINDOW) {
  await cdp.send("Emulation.setDeviceMetricsOverride", { width, height, deviceScaleFactor: 1, mobile: false });
  await reload(cdp);
  await waitFor(cdp, "Boolean(document.querySelector('.app-titlebar') && document.querySelector('.split-main, .content'))", "the first paint");
  await cdp.evaluate(snippets.removeFixtureBanner());
}

/** The chip with these words, as drawn: what it is, and whether it reads as on. */
const chip = (cdp, words) => cdp.evaluate(`(() => {
  const found = [...document.querySelectorAll('.chip')].find((item) => item.textContent.trim() === ${JSON.stringify(words)});
  if (!found) return null;
  return { tag: found.tagName.toLowerCase(), pressed: found.getAttribute('aria-pressed'), title: found.getAttribute('title') };
})()`);

const costShown = (cdp) => cdp.evaluate("[...document.querySelectorAll('.control-note')].some((note) => note.textContent.includes('five minutes'))");

async function openMeetings(cdp, words) {
  await cdp.evaluate(snippets.clickText(NAV_PILL, "Meetings"));
  await waitFor(cdp, `[...document.querySelectorAll('.chip')].some((item) => item.textContent.trim() === ${JSON.stringify(words)})`, `the "${words}" chip`);
}

async function press(cdp, words) {
  await cdp.evaluate(snippets.clickText("button.chip", words));
  await delay(250);
}

const SETUP_NOTE = "Each check starts from a fresh load. The chip is pressed with a DOM click, since the card can be below the fold at the smaller sizes.";

export async function runMeetingSpeakerLabels(options) {
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
      claim: "Until the models and runtime are there, the chip is a link to Settings > Meetings and doesn't read as on.",
      expectation: `a button "${NEEDS_SETUP}" with no aria-pressed and no cost note; pressing it opens the screen with the speaker labels section`,
    }, async () => {
      await freshShell(cdp);
      await openMeetings(cdp, NEEDS_SETUP);
      const before = await chip(cdp, NEEDS_SETUP);
      const cost = await costShown(cdp);
      const shot = "01-needs-setup-1000x720.png";
      await cdp.screenshot(resolve(out, shot));
      await press(cdp, NEEDS_SETUP);
      await waitFor(cdp, "[...document.querySelectorAll('h2')].some((heading) => heading.textContent.trim() === 'speaker labels')", "Settings > Meetings");
      return {
        measured: { before, cost },
        matches: before?.tag === "button" && before.pressed === null && !cost,
        screenshots: [shot],
      };
    });

    await check({
      id: "2",
      claim: "With them there, the chip is a toggle: it writes the setting, reads it back, and says what it costs only while on.",
      expectation: "aria-pressed false with no note; pressed, true with the note and the setting on; pressed again, false with no note and the setting off",
    }, async () => {
      await freshShell(cdp);
      await cdp.evaluate("kw.speakerModels(); true");
      await openMeetings(cdp, LABEL);
      const off = { chip: await chip(cdp, LABEL), cost: await costShown(cdp) };
      await press(cdp, LABEL);
      const on = {
        chip: await chip(cdp, LABEL),
        cost: await costShown(cdp),
        saved: (await cdp.evaluate(snippets.invoke("diarization_status"))).enabled,
      };
      const shot = "02-on-1000x720.png";
      await cdp.screenshot(resolve(out, shot));
      await press(cdp, LABEL);
      const offAgain = {
        chip: await chip(cdp, LABEL),
        cost: await costShown(cdp),
        saved: (await cdp.evaluate(snippets.invoke("diarization_status"))).enabled,
      };
      return {
        measured: { off, on, offAgain },
        matches: off.chip?.pressed === "false" && !off.cost
          && on.chip?.pressed === "true" && on.cost && on.saved === true
          && offAgain.chip?.pressed === "false" && !offAgain.cost && offAgain.saved === false,
        screenshots: [shot],
      };
    });

    await check({
      id: "3",
      claim: "A write Rust refuses is in view, and the chip doesn't read as on.",
      expectation: "with the models removed after the page read them, pressing leaves aria-pressed false and shows the failure line",
    }, async () => {
      await freshShell(cdp);
      await cdp.evaluate("kw.speakerModels(); true");
      await openMeetings(cdp, LABEL);
      await cdp.evaluate("kw.speakerModels(false); true");
      await press(cdp, LABEL);
      const after = await chip(cdp, LABEL);
      const failure = await cdp.evaluate("document.body.textContent.includes(\"Speaker labels couldn't be read or saved.\")");
      return { measured: { after, failure }, matches: after?.pressed === "false" && failure };
    });

    await check({
      id: "4",
      claim: "While a meeting runs, the chip states what's on and can't be pressed; off, it isn't there.",
      expectation: "on: a static span reading Speaker labels in the live view; off: no chip with those words",
    }, async () => {
      const liveWith = async (enabled) => {
        await freshShell(cdp);
        await cdp.evaluate("kw.speakerModels(); true");
        await openMeetings(cdp, LABEL);
        if (enabled) await press(cdp, LABEL);
        await waitFor(cdp, START_ENABLED, "Start recording to be enabled");
        await cdp.evaluate(snippets.clickText(".control-start", "Start recording"));
        await waitFor(cdp, LIVE, "the live meeting view");
        await delay(300);
        return chip(cdp, LABEL);
      };
      const on = await liveWith(true);
      const shot = "04-recording-1000x720.png";
      await cdp.screenshot(resolve(out, shot));
      const off = await liveWith(false);
      return { measured: { on, off }, matches: on?.tag === "span" && on.pressed === null && off === null, screenshots: [shot] };
    });

    await check({
      id: "5",
      claim: "At the smallest window the chip and its note scroll nothing sideways.",
      expectation: "at 720x520 with the chip on, .split-main scrollWidth is no more than its clientWidth",
    }, async () => {
      await freshShell(cdp, SMALL);
      await cdp.evaluate("kw.speakerModels(); true");
      await openMeetings(cdp, LABEL);
      await press(cdp, LABEL);
      const measured = await cdp.evaluate("(() => { const main = document.querySelector('.split-main'); return { scrollWidth: main.scrollWidth, clientWidth: main.clientWidth }; })()");
      const shot = "05-on-720x520.png";
      await cdp.screenshot(resolve(out, shot));
      return { measured, matches: measured.scrollWidth <= measured.clientWidth, screenshots: [shot] };
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
    if (options.help) process.stdout.write(`${shellUsage().replace("shell-title-bar.mjs", "meeting-speaker-labels.mjs")}\n`);
    else {
      const failed = failedChecks(await runMeetingSpeakerLabels(options));
      if (failed.length) {
        console.error(`meeting-speaker-labels: ${failed.length} check(s) false: ${failed.join(", ")}`);
        process.exitCode = 1;
      }
    }
  } catch (error) {
    console.error(`meeting-speaker-labels failed: ${error.message}`);
    process.exitCode = 1;
  }
}
