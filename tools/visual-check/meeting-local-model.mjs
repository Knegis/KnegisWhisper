#!/usr/bin/env node
/**
 * Measures, against dev fixtures, the Model row on Settings > Meetings: there
 * only for the `local` provider, listing Automatic and what's installed, saved
 * on change, a refused write in view, a chosen model that has gone named both
 * there and on Meetings before Start, held while a meeting runs, and no sideways
 * scroll at 720x520. Then the same choice in the Meetings page's transcription
 * chip: the models listed under their own heading, what each row writes, what the
 * chip says, including while a meeting runs, a failed write in view, and the
 * menu and the chips fitting at 720x520.
 *
 * `kw.localMeetingProvider()` makes that provider usable in the fixtures, which
 * keep it only until a reload, so each check calls it after its fresh load. They
 * have no `local-streaming` to measure the row's absence on.
 *
 * What the choice does -- which model loads, what the note is made by, dictation's
 * model coming back -- is Rust's and is pinned by `cargo test` (`local_model.rs`,
 * `meeting.rs`). The fixtures only store the setting, so none of that is here.
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
const MODEL_SELECT = "Meeting model";
const PROVIDER_SELECT = "Meeting provider";
const INSTALLED = "whisper-small-q5";
const NOT_INSTALLED = "whisper-medium-q5";
const NOT_INSTALLED_PROBLEM = "The model chosen for meetings isn't installed.";
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

/** A fresh load with the invoke stub in, and each fixture call in `before` made before anything reads it. */
async function freshShell(cdp, { size = WINDOW, before = [] } = {}) {
  await cdp.send("Emulation.setDeviceMetricsOverride", { ...size, deviceScaleFactor: 1, mobile: false });
  await reload(cdp);
  await waitFor(cdp, "Boolean(document.querySelector('.app-titlebar') && document.querySelector('.split-main, .content'))", "the first paint");
  await cdp.evaluate(snippets.removeFixtureBanner());
  await cdp.evaluate(snippets.installInvokeStub());
  await cdp.evaluate("kw.localMeetingProvider(); true");
  for (const [command, args] of before) await cdp.evaluate(snippets.invokeFixture(command, args));
  await cdp.evaluate(snippets.clearStubCalls());
}

const selectFound = (label) => `Boolean(document.querySelector('select[aria-label=${JSON.stringify(label)}]'))`;

async function openSettingsMeetings(cdp) {
  await cdp.evaluate(snippets.openSettingsScreen("Meetings"));
  await waitFor(cdp, selectFound(PROVIDER_SELECT), "Settings > Meetings");
  await delay(250);
}

/** The row as drawn: the select's value and options, and the sentence under it. */
const row = (cdp) => cdp.evaluate(`(() => {
  const select = document.querySelector('select[aria-label=${JSON.stringify(MODEL_SELECT)}]');
  if (!select) return null;
  return {
    value: select.value,
    disabled: select.disabled,
    options: [...select.options].map((option) => ({ id: option.value, label: option.textContent, disabled: option.disabled })),
    detail: select.closest('.row-value').querySelector('.row-detail')?.textContent ?? null,
    label: select.closest('.row').querySelector('.row-label')?.textContent ?? null,
  };
})()`);

const choose = async (cdp, label, value) => {
  await cdp.evaluate(snippets.chooseSelect(label, value));
  await delay(250);
};

const saved = (cdp) => cdp.evaluate(snippets.invoke("meeting_local_model"));
const savedProvider = (cdp) => cdp.evaluate(snippets.invoke("meeting_provider"));

const CHIP = 'button.chip[aria-label^="Meeting transcription"]';
const AUTOMATIC_ROW = "Local windowed model";
const SMALL_ROW = "Whisper Small (Q5) · 488 MB";
const SMALL_NAME = "Whisper Small (Q5)";
const MODELS_HEADING = "Local windowed models";
const CLOUD_ROW = "Deepgram";

async function openMeetings(cdp) {
  await cdp.evaluate(snippets.clickText(NAV_PILL, "Meetings"));
  await waitFor(cdp, `Boolean(document.querySelector(${JSON.stringify(CHIP)}))`, "the transcription chip");
  await delay(400);
}

const chipText = (cdp) => cdp.evaluate(`document.querySelector(${JSON.stringify(CHIP)})?.querySelector('.chip-label')?.textContent ?? null`);

async function openChip(cdp) {
  await cdp.evaluate(`document.querySelector(${JSON.stringify(CHIP)}).click()`);
  await waitFor(cdp, snippets.menuVisible(), "the menu");
  return cdp.evaluate(snippets.menuState());
}

/** Opens the chip's menu and presses the row with this name. */
async function chooseRow(cdp, name) {
  await openChip(cdp);
  await cdp.evaluate(`(() => {
    const row = [...document.querySelectorAll('.chip-menu [role="option"]')]
      .find((item) => document.getElementById(item.getAttribute('aria-labelledby'))?.textContent === ${JSON.stringify(name)});
    if (!row) throw new Error('The open menu has no row named ' + ${JSON.stringify(name)});
    row.click();
  })()`);
  await delay(350);
}

const closeMenu = (cdp) => cdp.send("Input.dispatchKeyEvent", { type: "keyDown", key: "Escape", code: "Escape", windowsVirtualKeyCode: 27 });

const SETUP_NOTE = "Each check starts from a fresh load on the fixtures, at 1000x720 unless it says otherwise.";

export async function runMeetingLocalModel(options) {
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
      claim: "The row is there only for the local provider, offering Automatic and what's installed.",
      expectation: "no select on a cloud provider; with local chosen, a Model row on Automatic, with only installed models, and a sentence naming the model loaded",
    }, async () => {
      await freshShell(cdp, { before: [["meeting_set_provider", { provider: "meeting:deepgram" }]] });
      await openSettingsMeetings(cdp);
      const cloud = await row(cdp);
      await choose(cdp, PROVIDER_SELECT, "local");
      await waitFor(cdp, selectFound(MODEL_SELECT), "the Model row");
      const local = await row(cdp);
      const statuses = await cdp.evaluate(snippets.invoke("local_model_status"));
      const installed = statuses.models.filter((model) => model.installed).map((model) => model.id).sort();
      const shot = "01-automatic-1000x720.png";
      await cdp.screenshot(resolve(out, shot));
      const listed = local?.options.map((option) => option.id).filter((id) => id !== "").sort();
      return {
        measured: { cloud, local, installed },
        matches: cloud === null
          && local?.label === "Model" && local.value === "" && local.options[0]?.label === "Automatic"
          && JSON.stringify(listed) === JSON.stringify(installed)
          && local.detail.includes("the Whisper model loaded now"),
        screenshots: [shot],
      };
    });

    await check({
      id: "2",
      claim: "Choosing writes once and reads back, and Automatic clears it.",
      expectation: `one write of ${INSTALLED}, the setting and the select agree, the sentence says it loads at Start, and it survives leaving the screen; choosing Automatic writes null`,
    }, async () => {
      await freshShell(cdp, { before: [["meeting_set_provider", { provider: "local" }]] });
      await openSettingsMeetings(cdp);
      await waitFor(cdp, selectFound(MODEL_SELECT), "the Model row");
      await choose(cdp, MODEL_SELECT, INSTALLED);
      const writes = await cdp.evaluate(snippets.stubCalls("meeting_set_local_model"));
      const after = { row: await row(cdp), saved: await saved(cdp) };
      const shot = "02-chosen-1000x720.png";
      await cdp.screenshot(resolve(out, shot));
      await cdp.evaluate(snippets.clickText(NAV_PILL, "Meetings"));
      await waitFor(cdp, START_ENABLED, "Start recording to be enabled");
      await openSettingsMeetings(cdp);
      await waitFor(cdp, selectFound(MODEL_SELECT), "the Model row");
      const returned = await row(cdp);
      await cdp.evaluate(snippets.clearStubCalls());
      await choose(cdp, MODEL_SELECT, "");
      const cleared = {
        writes: await cdp.evaluate(snippets.stubCalls("meeting_set_local_model")),
        saved: await saved(cdp),
        value: (await row(cdp))?.value,
      };
      return {
        measured: { writes, after, returnedValue: returned?.value, cleared },
        matches: writes.length === 1 && writes[0].model === INSTALLED
          && after.saved === INSTALLED && after.row?.value === INSTALLED
          && after.row.detail.startsWith("Loaded when a meeting starts")
          && returned?.value === INSTALLED
          && cleared.writes.length === 1 && cleared.writes[0].model === null
          && cleared.saved === null && cleared.value === "",
        screenshots: [shot],
      };
    });

    await check({
      id: "3",
      claim: "A write that fails is in view, and the select goes back to what is saved.",
      expectation: "the failure's words on the screen, the select back on Automatic and the setting unchanged",
    }, async () => {
      const failure = "the meeting model could not be saved here";
      await freshShell(cdp, { before: [["meeting_set_provider", { provider: "local" }]] });
      await openSettingsMeetings(cdp);
      await waitFor(cdp, selectFound(MODEL_SELECT), "the Model row");
      await cdp.evaluate(snippets.stubInvoke({ reject: { meeting_set_local_model: failure } }));
      await choose(cdp, MODEL_SELECT, INSTALLED);
      const after = await row(cdp);
      const shown = await cdp.evaluate(`document.body.textContent.includes(${JSON.stringify(failure)})`);
      await cdp.evaluate(snippets.stubInvoke({ reject: { meeting_set_local_model: null } }));
      return {
        measured: { value: after?.value, shown, saved: await saved(cdp) },
        matches: after?.value === "" && shown && (await saved(cdp)) === null,
      };
    });

    await check({
      id: "4",
      claim: "A chosen model that isn't installed stays named, greyed, and Meetings says so before Start.",
      expectation: `the select shows ${NOT_INSTALLED} as a disabled option with the sentence saying so; Meetings shows "${NOT_INSTALLED_PROBLEM}" and Start can't be pressed`,
    }, async () => {
      const before = [
        ["meeting_set_provider", { provider: "local" }],
        ["meeting_set_local_model", { model: NOT_INSTALLED }],
      ];
      await freshShell(cdp, { before });
      await openSettingsMeetings(cdp);
      await waitFor(cdp, selectFound(MODEL_SELECT), "the Model row");
      const settings = await row(cdp);
      const greyed = settings?.options.find((option) => option.id === NOT_INSTALLED);
      const shot = "04-not-installed-1000x720.png";
      await cdp.screenshot(resolve(out, shot));
      await freshShell(cdp, { before });
      await cdp.evaluate(snippets.clickText(NAV_PILL, "Meetings"));
      const problem = `document.body.textContent.includes(${JSON.stringify(NOT_INSTALLED_PROBLEM)})`;
      await waitFor(cdp, problem, "the problem on Meetings");
      const startEnabled = await cdp.evaluate(START_ENABLED);
      const page = "04-meetings-held-1000x720.png";
      await cdp.screenshot(resolve(out, page));
      return {
        measured: { value: settings?.value, greyed, detail: settings?.detail, startEnabled },
        matches: settings?.value === NOT_INSTALLED && greyed?.disabled === true
          && greyed.label.endsWith("not installed") && settings.detail.startsWith("This model isn't installed.")
          && startEnabled === false,
        screenshots: [shot, page],
      };
    });

    await check({
      id: "5",
      claim: "While a meeting records, the model can't be changed, and the screen says why.",
      expectation: "the select disabled, with the sentence naming the model among what can't change",
    }, async () => {
      await freshShell(cdp, { before: [["meeting_set_provider", { provider: "local" }]] });
      await cdp.evaluate(snippets.clickText(NAV_PILL, "Meetings"));
      await waitFor(cdp, START_ENABLED, "Start recording to be enabled");
      await cdp.evaluate(snippets.clickText(".control-start", "Start recording"));
      await waitFor(cdp, LIVE, "the live meeting view");
      await openSettingsMeetings(cdp);
      await waitFor(cdp, selectFound(MODEL_SELECT), "the Model row");
      await delay(250);
      const during = await row(cdp);
      const said = await cdp.evaluate("[...document.querySelectorAll('.caveat')].some((line) => /provider, its model and the\\s+language can't be changed/.test(line.textContent))");
      return { measured: { disabled: during?.disabled, said }, matches: during?.disabled === true && said };
    });

    await check({
      id: "6",
      claim: "At the smallest window the row scrolls nothing sideways.",
      expectation: "at 720x520 with the longest sentence showing, nothing is wider than its scroller and the select fits inside the window",
    }, async () => {
      await freshShell(cdp, { size: SMALL, before: [["meeting_set_provider", { provider: "local" }], ["meeting_set_local_model", { model: INSTALLED }]] });
      await openSettingsMeetings(cdp);
      await waitFor(cdp, selectFound(MODEL_SELECT), "the Model row");
      await cdp.evaluate(`document.querySelector('select[aria-label=${JSON.stringify(MODEL_SELECT)}]').scrollIntoView({ block: 'center' })`);
      await delay(150);
      const measured = await cdp.evaluate(`(() => {
        const box = document.querySelector('select[aria-label=${JSON.stringify(MODEL_SELECT)}]').getBoundingClientRect();
        const page = document.scrollingElement;
        const main = document.querySelector('.split-main, .content');
        return { left: box.left, right: box.right, innerWidth: window.innerWidth, pageScroll: page.scrollWidth, pageClient: page.clientWidth, mainScroll: main?.scrollWidth ?? 0, mainClient: main?.clientWidth ?? 0 };
      })()`);
      const shot = "06-chosen-720x520.png";
      await cdp.screenshot(resolve(out, shot));
      return {
        measured,
        matches: measured.left >= 0 && measured.right <= measured.innerWidth
          && measured.pageScroll <= measured.pageClient && measured.mainScroll <= measured.mainClient,
        screenshots: [shot],
      };
    });

    await check({
      id: "7",
      claim: "The transcription chip lists the models under their own heading, and choosing one saves it and the provider that uses it.",
      expectation: `from a cloud provider: a "${MODELS_HEADING}" heading with the installed models; pressing "${SMALL_ROW}" writes the model and the provider once each, the chip says "${SMALL_NAME}", and that row is ticked alone`,
    }, async () => {
      await freshShell(cdp, { before: [["meeting_set_provider", { provider: "meeting:deepgram" }]] });
      await openMeetings(cdp);
      const before = await openChip(cdp);
      const shot = "07-menu-1000x720.png";
      await cdp.screenshot(resolve(out, shot));
      await closeMenu(cdp);
      await delay(150);
      await cdp.evaluate(snippets.clearStubCalls());
      await chooseRow(cdp, SMALL_ROW);
      const writes = {
        model: await cdp.evaluate(snippets.stubCalls("meeting_set_local_model")),
        provider: await cdp.evaluate(snippets.stubCalls("meeting_set_provider")),
      };
      const after = { chip: await chipText(cdp), model: await saved(cdp), provider: await savedProvider(cdp) };
      const reopened = await openChip(cdp);
      const ticked = reopened.options.filter((option) => option.selected).map((option) => option.name);
      const chosenShot = "07-chosen-1000x720.png";
      await cdp.screenshot(resolve(out, chosenShot));
      await closeMenu(cdp);
      return {
        measured: { groups: before.groups, rows: before.options.map((option) => option.name), writes, after, ticked },
        matches: before.groups.includes(MODELS_HEADING) && before.options.some((option) => option.name === SMALL_ROW)
          && writes.model.length === 1 && writes.model[0].model === INSTALLED
          && writes.provider.length === 1 && writes.provider[0].provider === "local"
          && after.chip === SMALL_NAME && after.model === INSTALLED && after.provider === "local"
          && ticked.length === 1 && ticked[0] === SMALL_ROW,
        screenshots: [shot, chosenShot],
      };
    });

    await check({
      id: "8",
      claim: "The provider's own row is Automatic, another provider leaves the model alone, and pressing what's chosen writes nothing.",
      expectation: `pressing the chosen row again: no writes; "${CLOUD_ROW}": the provider changes and the model stays; "${AUTOMATIC_ROW}": the provider returns, the model clears and the chip names the provider`,
    }, async () => {
      await freshShell(cdp, { before: [["meeting_set_provider", { provider: "local" }], ["meeting_set_local_model", { model: INSTALLED }]] });
      await openMeetings(cdp);
      await chooseRow(cdp, SMALL_ROW);
      const again = {
        model: await cdp.evaluate(snippets.stubCalls("meeting_set_local_model")),
        provider: await cdp.evaluate(snippets.stubCalls("meeting_set_provider")),
      };
      await chooseRow(cdp, CLOUD_ROW);
      const cloud = { chip: await chipText(cdp), model: await saved(cdp), provider: await savedProvider(cdp) };
      await chooseRow(cdp, AUTOMATIC_ROW);
      const automatic = { chip: await chipText(cdp), model: await saved(cdp), provider: await savedProvider(cdp) };
      return {
        measured: { again, cloud, automatic },
        matches: again.model.length === 0 && again.provider.length === 0
          && cloud.chip === CLOUD_ROW && cloud.model === INSTALLED && cloud.provider === "meeting:deepgram"
          && automatic.chip === AUTOMATIC_ROW && automatic.model === null && automatic.provider === "local",
      };
    });

    await check({
      id: "9",
      claim: "A write that fails is in view, and the chip doesn't claim the choice.",
      expectation: `with the model's write refused, the failure's words on the page and the chip still "${CLOUD_ROW}"`,
    }, async () => {
      const failure = "the meeting model could not be saved here";
      await freshShell(cdp, { before: [["meeting_set_provider", { provider: "meeting:deepgram" }]] });
      await openMeetings(cdp);
      await cdp.evaluate(snippets.stubInvoke({ reject: { meeting_set_local_model: failure } }));
      await chooseRow(cdp, SMALL_ROW);
      const shown = await cdp.evaluate(`document.body.textContent.includes(${JSON.stringify(failure)})`);
      const chip = await chipText(cdp);
      await cdp.evaluate(snippets.stubInvoke({ reject: { meeting_set_local_model: null } }));
      const provider = await savedProvider(cdp);
      return { measured: { shown, chip, provider }, matches: shown && chip === CLOUD_ROW && provider === "meeting:deepgram" };
    });

    await check({
      id: "10",
      claim: "While a meeting runs, the chip says which model it's using, as a statement.",
      expectation: `a chip reading "${SMALL_NAME}" in the live view that opens no menu`,
    }, async () => {
      await freshShell(cdp, { before: [["meeting_set_provider", { provider: "local" }], ["meeting_set_local_model", { model: INSTALLED }]] });
      await openMeetings(cdp);
      await waitFor(cdp, START_ENABLED, "Start recording to be enabled");
      await cdp.evaluate(snippets.clickText(".control-start", "Start recording"));
      await waitFor(cdp, LIVE, "the live meeting view");
      await delay(300);
      const measured = await cdp.evaluate(`(() => {
        const chip = [...document.querySelectorAll('.meeting-live .chip, .chip')].find((item) => item.textContent.trim() === ${JSON.stringify(SMALL_NAME)});
        return chip ? { tag: chip.tagName.toLowerCase(), popup: chip.getAttribute('aria-haspopup') } : null;
      })()`);
      const shot = "10-recording-1000x720.png";
      await cdp.screenshot(resolve(out, shot));
      return { measured, matches: measured?.tag === "span" && measured.popup === null, screenshots: [shot] };
    });

    await check({
      id: "11",
      claim: "At the smallest window the menu stays inside it, and the chips scroll nothing sideways.",
      expectation: "at 720x520 the open menu's box is inside the window, and .split-main scrollWidth is no more than its clientWidth",
    }, async () => {
      await freshShell(cdp, { size: SMALL, before: [["meeting_set_provider", { provider: "local" }], ["meeting_set_local_model", { model: INSTALLED }]] });
      await openMeetings(cdp);
      // It's below the fold at this size, and a person scrolls to it first. A
      // scroll closes an open menu, so this comes before the press.
      await cdp.evaluate(`document.querySelector(${JSON.stringify(CHIP)}).scrollIntoView({ block: 'center' })`);
      await delay(250);
      const menu = await openChip(cdp);
      await delay(400);
      const shot = "11-menu-720x520.png";
      await cdp.screenshot(resolve(out, shot));
      await closeMenu(cdp);
      const main = await cdp.evaluate("(() => { const m = document.querySelector('.split-main'); return { scrollWidth: m.scrollWidth, clientWidth: m.clientWidth }; })()");
      const box = menu.box;
      return {
        measured: { box, viewport: menu.viewport, main },
        matches: box.left >= 0 && box.top >= 0 && box.right <= menu.viewport.width && box.bottom <= menu.viewport.height
          && main.scrollWidth <= main.clientWidth,
        screenshots: [shot],
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
    if (options.help) process.stdout.write(`${shellUsage().replace("shell-title-bar.mjs", "meeting-local-model.mjs")}\n`);
    else {
      const failed = failedChecks(await runMeetingLocalModel(options));
      if (failed.length) {
        console.error(`meeting-local-model: ${failed.length} check(s) false: ${failed.join(", ")}`);
        process.exitCode = 1;
      }
    }
  } catch (error) {
    console.error(`meeting-local-model failed: ${error.message}`);
    process.exitCode = 1;
  }
}
