#!/usr/bin/env node
/**
 * Measures, against dev fixtures, Settings > Transcription's one list of local
 * speech models (KNEG-330): Whisper and Parakeet models in one list, the
 * order they are listed in, what each row's button calls and which rows read
 * as in use afterwards, Install's progress line, the language filter, the
 * Parakeet engine's caveat, and the page at 720x520.
 *
 * Buttons are pressed with real CDP pointer presses. The page's `invoke` is
 * wrapped (`installInvokeStub`) to log the calls the buttons make, and to slow
 * one install so its progress line can be read. The fixtures keep the
 * selections in memory as Rust keeps them in settings: `sherpa_use_model` and
 * `local_server_start` move the dictation choice, `sherpa_use_stream_model`
 * the meetings one. The in-use rules themselves are `lib/localModels.ts`'s,
 * pinned by `tests/localModels.test.ts`; this measures that the screen shows
 * them.
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
const PANEL_ITEM = ".list-panel .panel-item";

const WIDE = { width: 1000, height: 720 };
const SMALL = { width: 720, height: 520 };

/** The fixtures' models, in the order the list must give them: dictation, then meetings, each smallest first. */
const ORDER = [
  "Whisper Small (Q5)",
  "Whisper Large v3 Turbo (Q5)",
  "Parakeet Unified EN 0.6B",
  "Parakeet TDT 0.6B v3",
  "KB Whisper Large (Swedish, Q5)",
  "Whisper Medium (Q5)",
  "Whisper Base (English)",
  "Nemotron Speech Streaming EN 0.6B",
  "Nemotron 3.5 ASR Streaming 0.6B",
];
const SWEDISH = [
  "Whisper Small (Q5)",
  "Whisper Large v3 Turbo (Q5)",
  "Parakeet TDT 0.6B v3",
  "KB Whisper Large (Swedish, Q5)",
  "Whisper Medium (Q5)",
];
const FILTER_OPTIONS = ["Any language", "Swedish", "English", "Danish", "Norwegian", "Finnish", "German", "French", "Spanish"];
const CAVEAT = "This engine can't be updated from here yet. To reinstall it, close the app and delete its folder by hand.";
const LOADED_BUT = "Loaded, but dictation goes to Parakeet Unified EN 0.6B";
const ROUTE_CALLS = ["sherpa_use_model", "sherpa_use_stream_model", "dictation_use_local", "local_server_start"];
const INSTALL_CALLS = ["local_model_install", "sherpa_install_model"];

const SECTION_READY = `Boolean(document.querySelector('[data-surface="settings.transcription.local"] ul.list > li'))`;

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

/** A real pointer press at a measured point, as a person's click is. */
async function press(cdp, { x, y }) {
  await cdp.send("Input.dispatchMouseEvent", { type: "mouseMoved", x, y });
  await cdp.send("Input.dispatchMouseEvent", { type: "mousePressed", x, y, button: "left", clickCount: 1 });
  await cdp.send("Input.dispatchMouseEvent", { type: "mouseReleased", x, y, button: "left", clickCount: 1 });
  await delay(150);
}

/** Presses the button of the row named `name`, after checking it reads `expected`. */
async function pressRow(cdp, name, expected) {
  const point = await cdp.evaluate(snippets.localModelButton(name));
  if (point.text !== expected) throw new Error(`${name}'s button reads "${point.text}", not "${expected}"`);
  const size = await cdp.evaluate("({ width: innerWidth, height: innerHeight })");
  if (point.x < 0 || point.y < 0 || point.x > size.width || point.y > size.height) {
    throw new Error(`${name}'s button is off screen at ${JSON.stringify(point)}`);
  }
  await press(cdp, point);
}

/** The fixtures' model ids, read through the same commands the page reads. */
async function modelIds(cdp) {
  const local = await cdp.evaluate(snippets.invoke("local_model_status"));
  const sherpa = await cdp.evaluate(snippets.invoke("sherpa_status"));
  return [...local.models, ...sherpa.models].map((m) => m.id);
}

const state = (cdp, ids = []) => cdp.evaluate(snippets.localModelsState(ids));
const goesTo = (cdp) => cdp.evaluate(snippets.dictationGoesTo());

/** The row whose line is `name`, or begins with it and " · ". */
const rowNamed = (page, name) => page.rows.find((r) => r.line === name || r.line.startsWith(`${name} · `)) ?? null;
const nameOf = (line) => ORDER.find((name) => line === name || line.startsWith(`${name} · `)) ?? line;
const buttonOf = (page, name) => rowNamed(page, name)?.button ?? null;
const inUse = (page, name, text) => {
  const button = buttonOf(page, name);
  return button !== null && button.text === text && button.pressed === "true" && button.marked;
};
const notInUse = (page, name, text) => {
  const button = buttonOf(page, name);
  return button !== null && button.text === text && button.pressed === "false" && !button.marked;
};

/** A fresh load at `size` on Settings > Transcription, the invoke stub installed and its log empty. */
async function fresh(cdp, size = WIDE) {
  await viewport(cdp, size);
  await reload(cdp);
  await waitFor(cdp, "Boolean(document.querySelector('nav.rail') && document.querySelector('.card .split-main, .card .content'))", "the fixture shell");
  await cdp.evaluate(snippets.removeFixtureBanner());
  await cdp.evaluate(snippets.installInvokeStub());
  await cdp.evaluate(snippets.clickText(RAIL_ITEM, "Settings"));
  await waitFor(cdp, "Boolean(document.querySelector('.list-panel .panel-item'))", "the Settings panel");
  await cdp.evaluate(snippets.clickText(PANEL_ITEM, "Transcription"));
  await waitFor(cdp, SECTION_READY, "the local models list");
  await delay(300);
  await cdp.evaluate(snippets.clearStubCalls());
}

/** Waits for the row named `name` to read `text`, after a press that reloads the page's data. */
async function waitForButton(cdp, name, text, timeout = 10_000) {
  const deadline = Date.now() + timeout;
  while (Date.now() < deadline) {
    const page = await state(cdp);
    if (buttonOf(page, name)?.text === text) return page;
    await delay(80);
  }
  throw new Error(`Timed out waiting for ${name} to read "${text}"`);
}

const SETUP_NOTE = "Each check starts from a fresh load of the fixtures at 1000x720 unless it says otherwise, on Settings > Transcription, with the page's invoke wrapped to log calls. Buttons are pressed with real CDP pointer presses.";

export async function runLocalModels(options) {
  const out = resolve(options.out);
  await mkdir(out, { recursive: true });
  let browser;
  let cdp;
  try {
    browser = await launchEdge({ ...options, out });
    cdp = await connectCdp(browser.target.webSocketDebuggerUrl);
    await cdp.send("Page.enable");
    await viewport(cdp, WIDE);
    const report = createReport({
      url: options.url,
      edgeVersion: browser.version.Browser ?? browser.version,
      fixtureBannerRemoved: true,
      timestamp: new Date().toISOString(),
      measurementScope: "measured in headless Edge against dev fixtures, not in the Tauri window",
    });

    /** Runs one check; a step that throws is the check measured false, with the page as it was. */
    const check = async ({ id, claim, expectation, note = null }, run) => {
      const notes = [SETUP_NOTE, ...(note ? [note] : [])].join(" ");
      try {
        const result = await run();
        addCheck(report, { id, claim, expectation, note: notes, ...result });
      } catch (error) {
        let page = null;
        try {
          page = await state(cdp);
        } catch { /* no page to read */ }
        addCheck(report, { id, claim, expectation, measured: { failedAt: error.message, page }, matches: false, note: notes });
      }
    };

    await check({
      id: "1",
      claim: "The local models are one list: no separate Parakeet list or heading, and model ids only inside closed Details.",
      expectation: [
        "one ul in the local section, holding a row for each of the fixtures' nine models; its headings exactly",
        "h2 'Local transcription' and h3 'Engines'; every Details closed; no model id in the shown text, and every id somewhere in the section (under Details)",
      ].join(" "),
    }, async () => {
      await fresh(cdp);
      const ids = await modelIds(cdp);
      const page = await state(cdp, ids);
      const shot = "01-local-models-1000x720.png";
      await cdp.evaluate(`document.querySelector('[data-surface="settings.transcription.local"]').scrollIntoView({ block: "start" })`);
      await delay(200);
      await cdp.screenshot(resolve(out, shot));
      return {
        measured: { ids, lists: page.lists, rows: page.rows.length, headings: page.headings, openDetails: page.openDetails, idsShown: page.idsShown, idsAnywhere: page.idsAnywhere },
        matches: ids.length === 9 && page.lists === 1 && page.rows.length === 9
          && JSON.stringify(page.headings) === JSON.stringify(["h2:Local transcription", "h3:Engines"])
          && page.openDetails === 0 && page.idsShown.length === 0 && page.idsAnywhere.length === ids.length,
        screenshots: [shot],
      };
    });

    await check({
      id: "2",
      claim: "The list gives dictation models before meetings models, each smallest first, with an unknown size last.",
      expectation: `row names in exactly this order: ${ORDER.join(", ")}`,
    }, async () => {
      await fresh(cdp);
      const page = await state(cdp);
      const names = page.rows.map((r) => nameOf(r.line));
      return {
        measured: { names, lines: page.rows.map((r) => r.line), details: page.rows.map((r) => r.details.map((d) => d.text).join(" / ")) },
        matches: JSON.stringify(names) === JSON.stringify(ORDER),
      };
    });

    await check({
      id: "3",
      claim: "Use for meetings on Nemotron 3.5 moves meetings only: one sherpa_use_stream_model, and dictation is untouched.",
      expectation: [
        "after one press: exactly one sherpa_use_stream_model {modelId: nemotron-3.5-asr-streaming-0.6b} and no sherpa_use_model, dictation_use_local or local_server_start;",
        "Nemotron 3.5 reads 'In use for meetings' (pressed, marked), Nemotron EN 'Use for meetings'; 'Dictation goes to' reads as before",
      ].join(" "),
    }, async () => {
      await fresh(cdp);
      const before = { goesTo: await goesTo(cdp), page: await state(cdp) };
      await pressRow(cdp, "Nemotron 3.5 ASR Streaming 0.6B", "Use for meetings");
      const page = await waitForButton(cdp, "Nemotron 3.5 ASR Streaming 0.6B", "In use for meetings");
      const calls = await cdp.evaluate(snippets.stubSequence(ROUTE_CALLS));
      const stream = await cdp.evaluate(snippets.stubCalls("sherpa_use_stream_model"));
      const after = await goesTo(cdp);
      const shot = "03-use-for-meetings-1000x720.png";
      await cdp.screenshot(resolve(out, shot));
      return {
        measured: {
          calls,
          stream,
          goesTo: { before: before.goesTo, after },
          nemotron35: buttonOf(page, "Nemotron 3.5 ASR Streaming 0.6B"),
          nemotronEn: { before: buttonOf(before.page, "Nemotron Speech Streaming EN 0.6B"), after: buttonOf(page, "Nemotron Speech Streaming EN 0.6B") },
        },
        matches: JSON.stringify(calls) === JSON.stringify(["sherpa_use_stream_model"])
          && stream.length === 1 && stream[0]?.modelId === "nemotron-3.5-asr-streaming-0.6b"
          && before.goesTo !== null && after === before.goesTo
          && inUse(page, "Nemotron 3.5 ASR Streaming 0.6B", "In use for meetings")
          && inUse(before.page, "Nemotron Speech Streaming EN 0.6B", "In use for meetings")
          && notInUse(page, "Nemotron Speech Streaming EN 0.6B", "Use for meetings"),
        screenshots: [shot],
      };
    });

    await check({
      id: "4",
      claim: "Use for dictation on Parakeet Unified, then on Whisper Turbo: each makes its two calls in order, the labels flip, the loaded Whisper model says where dictation went, and the meetings model stays in use.",
      expectation: [
        "Parakeet: sherpa_use_model then dictation_use_local; Parakeet Unified 'In use for dictation'; Whisper Small (loaded) shows",
        `"${LOADED_BUT}" as a warning; 'Dictation goes to' 'This computer'.`,
        "Then Turbo: local_server_start then dictation_use_local; Turbo 'In use for dictation', Parakeet Unified 'Use for dictation', Whisper Small without the warning;",
        "Nemotron EN 'In use for meetings' throughout; no sherpa_use_stream_model",
      ].join(" "),
    }, async () => {
      await fresh(cdp);
      const before = await state(cdp);
      await pressRow(cdp, "Parakeet Unified EN 0.6B", "Use for dictation");
      const parakeet = await waitForButton(cdp, "Parakeet Unified EN 0.6B", "In use for dictation");
      const parakeetCalls = await cdp.evaluate(snippets.stubSequence(ROUTE_CALLS));
      const parakeetGoesTo = await goesTo(cdp);
      const shot = "04a-parakeet-for-dictation-1000x720.png";
      await cdp.evaluate(snippets.localModelButton("Whisper Small (Q5)"));
      await delay(150);
      await cdp.screenshot(resolve(out, shot));
      await cdp.evaluate(snippets.clearStubCalls());
      await pressRow(cdp, "Whisper Large v3 Turbo (Q5)", "Use for dictation");
      const turbo = await waitForButton(cdp, "Whisper Large v3 Turbo (Q5)", "In use for dictation", 12_000);
      const turboCalls = await cdp.evaluate(snippets.stubSequence(ROUTE_CALLS));
      const turboShot = "04b-turbo-for-dictation-1000x720.png";
      await cdp.screenshot(resolve(out, turboShot));
      const warnings = (page, name) => (rowNamed(page, name)?.details ?? []).filter((d) => d.warn).map((d) => d.text);
      return {
        measured: {
          parakeetCalls,
          turboCalls,
          parakeetGoesTo,
          afterParakeet: {
            unified: buttonOf(parakeet, "Parakeet Unified EN 0.6B"),
            smallWarnings: warnings(parakeet, "Whisper Small (Q5)"),
            nemotronEn: buttonOf(parakeet, "Nemotron Speech Streaming EN 0.6B"),
          },
          afterTurbo: {
            turbo: buttonOf(turbo, "Whisper Large v3 Turbo (Q5)"),
            unified: buttonOf(turbo, "Parakeet Unified EN 0.6B"),
            smallWarnings: warnings(turbo, "Whisper Small (Q5)"),
            nemotronEn: buttonOf(turbo, "Nemotron Speech Streaming EN 0.6B"),
          },
          beforeUnified: buttonOf(before, "Parakeet Unified EN 0.6B"),
        },
        matches: JSON.stringify(parakeetCalls) === JSON.stringify(["sherpa_use_model", "dictation_use_local"])
          && JSON.stringify(turboCalls) === JSON.stringify(["local_server_start", "dictation_use_local"])
          && notInUse(before, "Parakeet Unified EN 0.6B", "Use for dictation")
          && inUse(parakeet, "Parakeet Unified EN 0.6B", "In use for dictation")
          && notInUse(parakeet, "Whisper Small (Q5)", "Use for dictation")
          && JSON.stringify(warnings(parakeet, "Whisper Small (Q5)")) === JSON.stringify([LOADED_BUT])
          && parakeetGoesTo === "This computer"
          && inUse(turbo, "Whisper Large v3 Turbo (Q5)", "In use for dictation")
          && notInUse(turbo, "Parakeet Unified EN 0.6B", "Use for dictation")
          && warnings(turbo, "Whisper Small (Q5)").length === 0
          && inUse(parakeet, "Nemotron Speech Streaming EN 0.6B", "In use for meetings")
          && inUse(turbo, "Nemotron Speech Streaming EN 0.6B", "In use for meetings"),
        screenshots: [shot, turboShot],
        note: "The fixtures' local_server_start takes 1.5 s, as a real server takes seconds to load a model.",
      };
    });

    await check({
      id: "5",
      claim: "Nothing installs until Install is pressed; Install on Whisper Medium makes one call and puts its progress line on that row.",
      expectation: [
        "after the load: no local_model_install or sherpa_install_model; with local_model_install held 4 s, one press: exactly one local_model_install {modelId: whisper-medium-q5}",
        "and no sherpa_install_model; Whisper Medium's row reads 'Starting…' and no other row has a progress line; a progress event for it then reads",
        "'Downloading Whisper Medium (Q5) — …' on the same row",
      ].join(" "),
      note: "The progress event is sent through the fixtures' dev console (kw.modelProgress), as Rust's install sends local-model-progress.",
    }, async () => {
      await fresh(cdp);
      const beforePress = await cdp.evaluate(snippets.stubSequence(INSTALL_CALLS));
      await cdp.evaluate(snippets.stubInvoke({ delay: { local_model_install: 4000 } }));
      await pressRow(cdp, "Whisper Medium (Q5)", "Install");
      await delay(200);
      const calls = await cdp.evaluate(snippets.stubCalls("local_model_install"));
      const sherpaCalls = await cdp.evaluate(snippets.stubCalls("sherpa_install_model"));
      const starting = await state(cdp);
      await cdp.evaluate("globalThis.kw.modelProgress(612000000, 1530000000, 'whisper-medium-q5')");
      await delay(250);
      const downloading = await state(cdp);
      const shot = "05-install-progress-1000x720.png";
      await cdp.screenshot(resolve(out, shot));
      const progressRows = (page) => page.rows
        .map((r) => ({ name: nameOf(r.line), lines: r.details.map((d) => d.text).filter((t) => t === "Starting…" || t.startsWith("Downloading")) }))
        .filter((r) => r.lines.length > 0);
      await cdp.evaluate(snippets.stubInvoke({ delay: { local_model_install: null } }));
      return {
        measured: { beforePress, calls, sherpaCalls, starting: progressRows(starting), downloading: progressRows(downloading) },
        matches: beforePress.length === 0 && calls.length === 1 && calls[0]?.modelId === "whisper-medium-q5" && sherpaCalls.length === 0
          && JSON.stringify(progressRows(starting)) === JSON.stringify([{ name: "Whisper Medium (Q5)", lines: ["Starting…"] }])
          && progressRows(downloading).length === 1 && progressRows(downloading)[0].name === "Whisper Medium (Q5)"
          && progressRows(downloading)[0].lines[0].startsWith("Downloading Whisper Medium (Q5) — "),
        screenshots: [shot],
      };
    });

    await check({
      id: "6",
      claim: "The language filter starts at Any language, offers the eight dictation languages, and Swedish keeps only models listed for it.",
      expectation: [
        `options exactly ${FILTER_OPTIONS.join(", ")}, 'Any language' chosen with all nine rows;`,
        `Swedish: rows exactly ${SWEDISH.join(", ")}`,
      ].join(" "),
    }, async () => {
      await fresh(cdp);
      const before = await state(cdp);
      await cdp.evaluate(snippets.chooseSelect("Show models for", "sv"));
      await delay(250);
      const swedish = await state(cdp);
      const names = swedish.rows.map((r) => nameOf(r.line));
      const shot = "06-swedish-1000x720.png";
      await cdp.evaluate(`document.querySelector('select[aria-label="Show models for"]').scrollIntoView({ block: "start" })`);
      await delay(150);
      await cdp.screenshot(resolve(out, shot));
      return {
        measured: { filter: before.filter, rowsBefore: before.rows.length, swedishFilter: swedish.filter, names },
        matches: JSON.stringify(before.filter?.options) === JSON.stringify(FILTER_OPTIONS)
          && before.filter?.shown === "Any language" && before.filter?.value === "" && before.rows.length === 9
          && swedish.filter?.shown === "Swedish" && JSON.stringify(names) === JSON.stringify(SWEDISH),
        screenshots: [shot],
      };
    });

    await check({
      id: "7",
      claim: "Under Engines, the Parakeet engine's caveat is in view with its fold, beside the speech engine's row and the two engines' Learn more.",
      expectation: `the caveat "${CAVEAT}"; Details summaries include 'Why, and which folder' and 'About the two engines'; engine rows 'Speech engine' and 'Parakeet engine'`,
      note: "The fixtures' Parakeet engine reports its build as unknown, which is when the caveat shows.",
    }, async () => {
      await fresh(cdp);
      const page = await state(cdp);
      const shot = "07-engines-1000x720.png";
      await cdp.evaluate(`[...document.querySelectorAll('[data-surface="settings.transcription.local"] h3')].find((h) => h.textContent.trim() === 'Engines')?.scrollIntoView({ block: "start" })`);
      await delay(150);
      await cdp.screenshot(resolve(out, shot));
      return {
        measured: { caveats: page.caveats, summaries: page.summaries, engines: page.engines },
        matches: page.caveats.includes(CAVEAT)
          && page.summaries.includes("Why, and which folder") && page.summaries.includes("About the two engines")
          && page.engines.every((line) => line !== null),
        screenshots: [shot],
      };
    });

    await check({
      id: "8",
      claim: "At 720x520 nothing on Settings > Transcription scrolls sideways, with every Details in the local section closed and then open.",
      expectation: "after a load at 720x520, closed and then with every local Details open: document scrollWidth <= innerWidth, .split-main and the section scrollWidth <= clientWidth, and nothing in the section reaching past the card",
    }, async () => {
      await fresh(cdp, SMALL);
      const closed = await cdp.evaluate(snippets.localModelsOverflow());
      const shot = "08-local-models-720x520.png";
      await cdp.evaluate(snippets.localModelButton("Parakeet TDT 0.6B v3"));
      await delay(150);
      await cdp.screenshot(resolve(out, shot));
      const opened = await cdp.evaluate(snippets.openLocalDetails(true));
      await delay(200);
      const open = await cdp.evaluate(snippets.localModelsOverflow());
      const openShot = "08-local-models-details-open-720x520.png";
      await cdp.evaluate(snippets.localModelButton("Parakeet TDT 0.6B v3"));
      await delay(150);
      await cdp.screenshot(resolve(out, openShot));
      const fits = (m) => m.scrollWidth <= m.innerWidth && m.mainScrollWidth <= m.mainClientWidth
        && m.sectionScrollWidth <= m.sectionClientWidth && m.pastTheCard.length === 0;
      return {
        measured: { closed, opened, open },
        matches: fits(closed) && opened > 0 && fits(open),
        screenshots: [shot, openShot],
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
    if (options.help) process.stdout.write(`${shellUsage().replace("shell-rail.mjs", "local-models.mjs")}\n`);
    else {
      const failed = failedChecks(await runLocalModels(options));
      // The run completed; a check measured false still fails it, so a wrapper or CI step sees it.
      if (failed.length) {
        console.error(`local-models: ${failed.length} check(s) false: ${failed.join(", ")}`);
        process.exitCode = 1;
      }
    }
  } catch (error) {
    console.error(`local-models failed: ${error.message}`);
    process.exitCode = 1;
  }
}
