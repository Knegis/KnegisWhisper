#!/usr/bin/env node
/**
 * Runs repeatable measurements of the control-bar chips that change a setting
 * in place (KNEG-385) against dev fixtures: where each menu opens, its
 * keyboard, what choosing does to the chip and the privacy line, the holds
 * while a switch or a dictation is running, the Tidy up and Summary toggles,
 * Meetings' hero buttons, the 720x520 fit, and the menus in light and dark.
 *
 * States the fixture defaults cannot reach -- a slow write, a refused one, an
 * unconfigured Tidy up, a route with a caveat -- are made by wrapping the
 * page's `invoke` for the one command (`installInvokeStub`), after the shell
 * loads and again after every reload. The fixtures are never changed.
 */
import { mkdir, writeFile } from "node:fs/promises";
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { parseShellArgs, shellUsage } from "./args.mjs";
import { launchEdge, killBrowser } from "./browser.mjs";
import { connectCdp } from "./cdp.mjs";
import { createReport, addCheck, failedChecks, renderMarkdown } from "./report.mjs";
import * as snippets from "./snippets.mjs";

const delay = (ms) => new Promise((resolveDelay) => setTimeout(resolveDelay, ms));
const isMain = process.argv[1]
  && resolve(process.argv[1]) === fileURLToPath(import.meta.url);

const RAIL_ITEM = "nav.rail .rail-item";
/** What a held chip says, as `routeHeld` in the app words it. */
const HELD = "Can't change during a dictation";
/** The fixture's greyed Dictation row: a provider with no key stored. */
const NO_KEY_ROW = "Corti";
const KEYS = {
  ArrowDown: 40, ArrowUp: 38, Home: 36, End: 35, Enter: 13, Escape: 27, Tab: 9, " ": 32,
};

/** A page expression's property, for polling. */
const field = (snippet, path) => `(${snippet})?.${path}`;
const chipField = (prefix, path) => field(snippets.chipState(prefix), path);

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

async function viewport(cdp, width, height) {
  await cdp.send("Emulation.setDeviceMetricsOverride", {
    width, height, deviceScaleFactor: 1, mobile: false,
  });
  await delay(200);
}

async function theme(cdp, scheme) {
  await cdp.send("Emulation.setEmulatedMedia", {
    features: [{ name: "prefers-color-scheme", value: scheme }],
  });
  await delay(200);
}

async function key(cdp, name) {
  const code = name === " " ? "Space" : name;
  const text = name === "Enter" ? "\r" : name === " " ? " " : undefined;
  const base = { key: name, code, windowsVirtualKeyCode: KEYS[name], modifiers: 0 };
  await cdp.send("Input.dispatchKeyEvent", {
    type: text ? "keyDown" : "rawKeyDown", ...base, ...(text ? { text } : {}),
  });
  await cdp.send("Input.dispatchKeyEvent", { type: "keyUp", ...base });
  await delay(150);
}

/** A real pointer press at a measured point, as a person's click is. */
async function press(cdp, { x, y }) {
  await cdp.send("Input.dispatchMouseEvent", { type: "mouseMoved", x, y });
  await cdp.send("Input.dispatchMouseEvent", { type: "mousePressed", x, y, button: "left", clickCount: 1 });
  await cdp.send("Input.dispatchMouseEvent", { type: "mouseReleased", x, y, button: "left", clickCount: 1 });
  await delay(150);
}

const pressChip = async (cdp, prefix) => press(cdp, await cdp.evaluate(snippets.chipCenter(prefix)));
const pressOption = async (cdp, name) => press(cdp, await cdp.evaluate(snippets.optionCenter(name)));

const screenshot = async (cdp, out, name) => {
  await cdp.screenshot(resolve(out, name));
  return name;
};

const shotName = (id, what, scheme) => `${id}-${what}-${scheme}.png`;

/** Opens a menu by pressing its chip, and waits for it to be placed. */
async function openMenu(cdp, prefix) {
  await pressChip(cdp, prefix);
  await waitFor(cdp, snippets.menuVisible(), `the ${prefix} menu`);
  await delay(120);
  return cdp.evaluate(snippets.menuState());
}

async function closeMenu(cdp) {
  await key(cdp, "Escape");
  await waitFor(cdp, "!document.querySelector('.chip-menu')", "the menu to close");
}

/** Whether the menu's box is inside the window, and above the control bar. */
const placedAboveBar = (menu) => menu.open
  && menu.box.left >= 0 && menu.box.top >= 0
  && menu.box.right <= menu.viewport.width && menu.box.bottom <= menu.viewport.height
  && menu.box.bottom <= menu.bar.top + 1 && menu.side === "above";
const placedInside = (menu) => menu.open
  && menu.box.left >= 0 && menu.box.top >= 0
  && menu.box.right <= menu.viewport.width && menu.box.bottom <= menu.viewport.height;

/** Waits for the shell, then removes the fixture banner. */
async function shellReady(cdp) {
  await waitFor(
    cdp,
    "Boolean(document.querySelector('nav.rail') && document.querySelector('.card .split-main, .card .content'))",
    "the fixture shell",
  );
  await cdp.evaluate(snippets.removeFixtureBanner());
}

/**
 * A fresh document on `page`, with the invoke stub installed. The stub goes in
 * after the shell has loaded, so the page is left and re-entered once: what it
 * reads on mounting is then read through the stub.
 */
async function freshPage(cdp, page) {
  await reload(cdp);
  await shellReady(cdp);
  await cdp.evaluate(snippets.installInvokeStub());
  await enterPage(cdp, page);
}

const CHIP_OF = { Dictation: "Transcription: ", Meetings: "Meeting transcription: " };

/** Leaves the page for the other one and comes back, so it reads again; the stub stays. */
async function enterPage(cdp, page) {
  const other = page === "Meetings" ? "Dictation" : "Meetings";
  await cdp.evaluate(snippets.clickText(RAIL_ITEM, other));
  await delay(250);
  await cdp.evaluate(snippets.clickText(RAIL_ITEM, page));
  await waitFor(cdp, snippets.chipState(CHIP_OF[page]), `the ${page} chips`);
  await delay(450);
}

const lastState = (cdp) => cdp.evaluate(snippets.taskPageState());

/** 1. Each menu opens inside the window and above the bar. */
async function checkPlacement(cdp, out, report) {
  const measured = {};
  await freshPage(cdp, "Dictation");
  measured.microphone = await openMenu(cdp, "Microphone: ");
  const shot = await screenshot(cdp, out, "01-microphone-menu-1000x720.png");
  await closeMenu(cdp);
  measured.route = await openMenu(cdp, "Transcription: ");
  await closeMenu(cdp);

  // The language chip only exists on local Whisper.
  await pressChip(cdp, "Transcription: ");
  await waitFor(cdp, snippets.menuVisible(), "the route menu");
  await pressOption(cdp, "Whisper Large v3 Turbo (Q5)");
  await waitFor(cdp, `${chipField("Language: ", "name")}`, "the language chip on a local Whisper route", 12_000);
  await waitFor(cdp, `${chipField("Transcription: ", "ariaDisabled")} === null`, "the switch to settle", 12_000);
  measured.language = await openMenu(cdp, "Language: ");
  await closeMenu(cdp);

  await freshPage(cdp, "Meetings");
  measured.provider = await openMenu(cdp, "Meeting transcription: ");
  await closeMenu(cdp);
  measured.meetingLanguage = await openMenu(cdp, "Language: ");
  await closeMenu(cdp);

  const summary = Object.fromEntries(Object.entries(measured).map(([name, menu]) => [name, {
    kind: menu.kind, name: menu.name, side: menu.side, box: menu.box, barTop: menu.bar?.top,
    viewport: menu.viewport, ok: placedAboveBar(menu),
  }]));
  addCheck(report, {
    id: "1",
    claim: "At 1000x720 each menu opens fully inside the window and above the control bar.",
    measured: summary,
    expectation: [
      "for the microphone, route, Dictation language, Meetings provider and Meetings language menus:",
      "an open menu with side above, box.bottom <= the bar's top (1px), left/top >= 0, right/bottom inside the window;",
      "the language panel is a dialog named Language, the others listboxes",
    ].join(" "),
    matches: Object.values(measured).every(placedAboveBar)
      && measured.language.kind === "dialog" && measured.language.name === "Language"
      && ["microphone", "route", "provider", "meetingLanguage"].every((name) => measured[name].kind === "listbox"),
    screenshots: [shot],
    note: "The language panel is reached after switching Dictation to a local Whisper model, as the chip is hidden elsewhere.",
  });
}

/** Presses `name` until `stop(state)` holds, at most `limit` times. */
async function walk(cdp, name, stop, limit = 12) {
  for (let i = 0; i < limit; i += 1) {
    await key(cdp, name);
    const state = await cdp.evaluate(snippets.menuState());
    if (stop(state)) return state;
  }
  return cdp.evaluate(snippets.menuState());
}

/** 2 and 3 (Dictation half). Keyboard, then what choosing from it does. */
async function checkKeyboard(cdp, report) {
  await freshPage(cdp, "Dictation");
  await cdp.evaluate(snippets.clearStubCalls());
  const measured = {};
  const before = await cdp.evaluate(snippets.chipState("Transcription: "));
  await cdp.evaluate(snippets.focusChip("Transcription: "));
  await key(cdp, "ArrowDown");
  await waitFor(cdp, snippets.menuVisible(), "the route menu from the keyboard");
  await delay(120);
  measured.opened = await cdp.evaluate(snippets.menuState());
  measured.openedChip = await cdp.evaluate(snippets.chipState("Transcription: "));

  const onGreyed = await walk(cdp, "ArrowDown", (state) => state.active === `option:${NO_KEY_ROW}`);
  measured.greyed = { active: onGreyed.active, row: onGreyed.options.find((row) => row.name === NO_KEY_ROW) };
  await key(cdp, "Enter");
  measured.afterGreyedEnter = {
    menu: (await cdp.evaluate(snippets.menuState())).open,
    chip: (await cdp.evaluate(snippets.chipState("Transcription: "))).text,
    writes: (await cdp.evaluate(snippets.stubCalls("dictation_set_cloud_provider"))).length,
  };

  // A normal row: step back off the greyed one and choose with Enter.
  const normal = await walk(cdp, "ArrowUp", (state) => {
    const row = state.options.find((option) => state.active === `option:${option.name}`);
    return row !== undefined && !row.greyed && !row.selected;
  });
  measured.normalRow = normal.active;
  await key(cdp, "Enter");
  await waitFor(cdp, `${chipField("Transcription: ", "text")} !== ${JSON.stringify(before.text)}`, "the chip to follow the choice");
  await delay(250);
  measured.chosen = {
    chip: await cdp.evaluate(snippets.chipState("Transcription: ")),
    menuOpen: (await cdp.evaluate(snippets.menuState())).open,
    writes: await cdp.evaluate(snippets.stubCalls("dictation_set_cloud_provider")),
    privacy: (await lastState(cdp)).privacy,
  };

  // Escape, then Tab, each from a menu the keyboard opened.
  await key(cdp, "Enter");
  await waitFor(cdp, snippets.menuVisible(), "the route menu to reopen");
  await delay(120);
  const open = await cdp.evaluate(snippets.chipState("Transcription: "));
  const openMenuState = await cdp.evaluate(snippets.menuState());
  measured.whileOpen = { expanded: open.expanded, controls: open.controls, menuId: openMenuState.id };
  await key(cdp, "Escape");
  measured.afterEscape = {
    menu: (await cdp.evaluate(snippets.menuState())).open,
    chip: await cdp.evaluate(snippets.chipState("Transcription: ")),
  };
  await key(cdp, "Enter");
  await waitFor(cdp, snippets.menuVisible(), "the route menu to reopen");
  await delay(120);
  await key(cdp, "Tab");
  measured.afterTab = {
    menu: (await cdp.evaluate(snippets.menuState())).open,
    chip: await cdp.evaluate(snippets.chipState("Transcription: ")),
  };

  const chosenLabel = measured.chosen.chip.text;
  addCheck(report, {
    id: "2",
    claim: "The keyboard walks a menu: ArrowDown lands on a greyed row and Enter there does nothing; Enter on a normal row chooses it; Escape returns focus to the chip; Tab closes; aria-expanded and aria-controls follow.",
    measured,
    expectation: [
      "on the Dictation route menu opened with ArrowDown on the chip (expanded true, aria-controls the listbox's id):",
      `ArrowDown reaches ${NO_KEY_ROW}, greyed with a reason; Enter leaves the menu open, the chip as it was and no dictation_set_cloud_provider call;`,
      "ArrowUp to a normal row and Enter closes the menu, writes once and changes the chip;",
      "Escape closes with focus on the chip, aria-expanded false and no aria-controls; Tab closes with aria-expanded false and no aria-controls",
    ].join(" "),
    matches: measured.opened.open
      && measured.openedChip.expanded === "true" && measured.openedChip.controls === measured.opened.id
      && measured.greyed.active === `option:${NO_KEY_ROW}`
      && measured.greyed.row?.greyed === true && Boolean(measured.greyed.row.reason)
      && measured.afterGreyedEnter.menu === true && measured.afterGreyedEnter.chip === before.text
      && measured.afterGreyedEnter.writes === 0
      && measured.chosen.menuOpen === false && measured.chosen.writes.length === 1
      && chosenLabel !== before.text && measured.chosen.chip.focused
      && measured.whileOpen.expanded === "true" && measured.whileOpen.controls === measured.whileOpen.menuId
      && measured.afterEscape.menu === false && measured.afterEscape.chip.focused
      && measured.afterEscape.chip.expanded === "false" && measured.afterEscape.chip.controls === null
      && measured.afterTab.menu === false && measured.afterTab.chip.expanded === "false"
      && measured.afterTab.chip.controls === null,
    note: "Focus on the chip after Enter is the menu's own return, measured before Escape is pressed. Keys are real CDP key events.",
  });
  return measured.chosen;
}

/** 3 (Dictation) and 4. Choosing a cloud row and a local one, the second slowly. */
async function checkDictationChoices(cdp, out, report, cloudChoice) {
  const measured = { cloud: cloudChoice };
  await cdp.evaluate(snippets.stubInvoke({ delay: { local_server_start: 1500 } }));
  await pressChip(cdp, "Transcription: ");
  await waitFor(cdp, snippets.menuVisible(), "the route menu");
  await pressOption(cdp, "Whisper Large v3 Turbo (Q5)");
  await delay(150);
  measured.during = {
    chip: await cdp.evaluate(snippets.chipState("Transcription: ")),
    page: await lastState(cdp),
    menuOpen: (await cdp.evaluate(snippets.menuState())).open,
  };
  const shot = await screenshot(cdp, out, "04-starting-route.png");
  await waitFor(cdp, `${chipField("Transcription: ", "ariaDisabled")} === null`, "the switch to settle", 12_000);
  await delay(250);
  measured.after = {
    chip: await cdp.evaluate(snippets.chipState("Transcription: ")),
    page: await lastState(cdp),
    startCalls: (await cdp.evaluate(snippets.stubCalls("local_server_start"))).length,
  };
  await cdp.evaluate(snippets.stubInvoke({ delay: { local_server_start: null } }));

  const cloudName = cloudChoice.chip.text;
  addCheck(report, {
    id: "3a",
    claim: "Choosing updates the chip and the privacy line on Dictation: a cloud provider says where audio is sent, a local model says it stays on this computer.",
    measured: {
      cloud: { chip: cloudName, privacy: cloudChoice.privacy },
      local: { chip: measured.after.chip.text, privacy: measured.after.page.privacy },
    },
    expectation: [
      `after choosing ${cloudName} (cloud): first privacy line "Audio is sent to ${cloudName}";`,
      "after choosing Whisper Large v3 Turbo (Q5) (local): the chip names it and the first line is \"Audio stays on this computer\"",
    ].join(" "),
    matches: cloudChoice.privacy[0] === `Audio is sent to ${cloudName}`
      && measured.after.chip.text === "Whisper Large v3 Turbo (Q5)"
      && measured.after.page.privacy[0] === "Audio stays on this computer",
    note: "The Meetings half of this claim is check 3b.",
  });
  addCheck(report, {
    id: "4",
    claim: "While a route switch is in flight the chip says Starting <model>…, cannot be opened, and Start is held.",
    measured: { during: measured.during, after: measured.after.chip },
    expectation: [
      "with local_server_start stubbed 1.5s slower: the chip's text starts with Starting and ends with …, it is aria-disabled with the name 'Transcription: Starting …',",
      "no menu is open, Start is disabled; once settled the chip is live again and Start is enabled",
    ].join(" "),
    matches: measured.during.chip.text.startsWith("Starting ") && measured.during.chip.text.endsWith("…")
      && measured.during.chip.name === `Transcription: ${measured.during.chip.text}`
      && measured.during.chip.ariaDisabled === "true" && measured.during.menuOpen === false
      && measured.during.page.start?.disabled === true
      && measured.after.chip.ariaDisabled === null && measured.after.page.start?.disabled === false
      && measured.after.startCalls === 1,
    screenshots: [shot],
    note: "The busy chip is aria-disabled rather than disabled, so a keyboard user's focus is not dropped.",
  });
}

/** 5. During a dictation the route, language and Tidy up chips are held; Stop is not. */
async function checkDictationHeld(cdp, out, report) {
  const read = async () => ({
    route: await cdp.evaluate(snippets.chipState("Transcription: ")),
    language: await cdp.evaluate(snippets.chipState("Language: ")),
    tidy: await cdp.evaluate(snippets.chipState("Tidy up")),
    page: await lastState(cdp),
  });
  const measured = { idle: await read() };
  await cdp.evaluate(snippets.emitDictationState("recording"));
  await waitFor(cdp, field(snippets.taskPageState(), "stop"), "the Stop button");
  await delay(250);
  measured.recording = await read();
  await pressChip(cdp, "Transcription: ");
  measured.recording.menuOpenedByPress = (await cdp.evaluate(snippets.menuState())).open;
  const shot = await screenshot(cdp, out, "05-dictation-held.png");
  await cdp.evaluate(snippets.emitDictationState("idle"));
  await waitFor(cdp, `${chipField("Transcription: ", "ariaDisabled")} === null`, "the chips to be released");
  await delay(250);
  measured.after = await read();

  const heldMenu = (chip) => chip.ariaDisabled === "true" && chip.description === HELD && chip.title === HELD;
  const { recording: rec } = measured;
  addCheck(report, {
    id: "5",
    claim: "During a dictation the route, language and Tidy up chips are held, with the reason; Stop and transcribe stays enabled.",
    measured,
    expectation: [
      `while recording (state announced through the fixtures' dev console): the route and language chips keep their labels, are aria-disabled with title and aria-description "${HELD}" and do not open when pressed;`,
      `the Tidy up chip is aria-disabled, not disabled (so focus stays), with title and aria-description "${HELD}"; Stop and transcribe is enabled;`,
      "after the session goes idle all three are live again",
    ].join(" "),
    matches: measured.idle.route.ariaDisabled === null && measured.idle.language.ariaDisabled === null
      && heldMenu(rec.route) && rec.route.text === measured.idle.route.text
      && heldMenu(rec.language) && rec.language.text === measured.idle.language.text
      && rec.menuOpenedByPress === false
      && heldMenu(rec.tidy) && rec.tidy.disabled === false
      && rec.page.stop?.disabled === false
      && measured.after.route.ariaDisabled === null && measured.after.language.ariaDisabled === null
      && measured.after.tidy.ariaDisabled === null && measured.after.tidy.disabled === false && measured.after.page.start?.disabled === false,
    screenshots: [shot],
    note: "The session state is the fixtures' own dev-console announcement, not a real recording; this measures what the page does with the state.",
  });
}

const CAVEAT_ROUTE = {
  state: "ready",
  destination: "anthropic",
  model: "claude-sonnet-5",
  leavesDevice: true,
  caveat: "no key is stored for anthropic. Cleanup will leave the transcript as it is until one is, on the Providers screen.",
};
const NEVER_CONFIGURED = { enabled: false, provider: null, model: null, selfHostedUrl: null, enterprise: false };
const SUMMARY_SETTINGS = {
  enabled: false, provider: "openai", model: "gpt-4o-mini", selfHostedUrl: null,
  enterprise: false, language: "match", contextBudget: 8192,
};

/** 6. The two toggles: configured, unconfigured, a caveat and a refusal. */
async function checkToggles(cdp, out, report) {
  const measured = { tidy: {}, summary: {} };
  const settled = (prefix, pressed) => waitFor(cdp, `${chipField(prefix, "pressed")} === ${JSON.stringify(pressed)}`, `${prefix} to read ${pressed}`);

  // Tidy up, configured and on in the fixtures.
  await freshPage(cdp, "Dictation");
  measured.tidy.initial = await cdp.evaluate(snippets.chipState("Tidy up"));
  await pressChip(cdp, "Tidy up");
  await settled("Tidy up", "false");
  measured.tidy.off = await cdp.evaluate(snippets.chipState("Tidy up"));
  await pressChip(cdp, "Tidy up");
  await settled("Tidy up", "true");
  measured.tidy.on = await cdp.evaluate(snippets.chipState("Tidy up"));
  // A refused write.
  await cdp.evaluate(snippets.stubInvoke({
    reject: { reasoning_set_cleanup: "cleanup needs a model. Providers disagree about parameter names, so there is no safe default to fall back on." },
  }));
  await pressChip(cdp, "Tidy up");
  await waitFor(cdp, `${field(snippets.taskPageState(), "errors.length")} > 0`, "the error line");
  measured.tidy.refused = { chip: await cdp.evaluate(snippets.chipState("Tidy up")), errors: (await lastState(cdp)).errors };
  const shotRefused = await screenshot(cdp, out, "06-tidy-refused.png");
  // A route with a caveat.
  await cdp.evaluate(snippets.stubInvoke({
    reject: { reasoning_set_cleanup: null },
    merge: { reasoning_status: { route: CAVEAT_ROUTE } },
  }));
  await enterPage(cdp, "Dictation");
  measured.tidy.caveat = {
    chip: await cdp.evaluate(snippets.chipState("Tidy up")),
    notes: (await lastState(cdp)).notes,
    privacy: (await lastState(cdp)).privacy,
  };
  const shotCaveat = await screenshot(cdp, out, "06-tidy-caveat.png");
  // Never configured.
  await cdp.evaluate(snippets.clearStubCalls());
  await cdp.evaluate(snippets.stubInvoke({
    merge: { reasoning_status: { settings: NEVER_CONFIGURED, route: { state: "off" } } },
  }));
  await enterPage(cdp, "Dictation");
  measured.tidy.unconfigured = { chip: await cdp.evaluate(snippets.chipState("Tidy up")) };
  await pressChip(cdp, "Tidy up");
  await delay(400);
  measured.tidy.unconfigured.leftThePage = await cdp.evaluate("!document.querySelector('.control-bar')");
  measured.tidy.unconfigured.writes = (await cdp.evaluate(snippets.stubCalls("reasoning_set_cleanup"))).length;

  // Summary: never configured in the fixtures, so a link first.
  await freshPage(cdp, "Meetings");
  measured.summary.unconfigured = { chip: await cdp.evaluate(snippets.chipState("Summary")) };
  await pressChip(cdp, "Summary");
  await delay(400);
  measured.summary.unconfigured.leftThePage = await cdp.evaluate("!document.querySelector('.control-bar')");
  // Configured through the fixture's own write, then read afresh.
  await cdp.evaluate(snippets.invokeFixture("summary_settings_set", { settings: SUMMARY_SETTINGS }));
  await enterPage(cdp, "Meetings");
  measured.summary.off = await cdp.evaluate(snippets.chipState("Summary"));
  await pressChip(cdp, "Summary");
  await settled("Summary", "true");
  measured.summary.on = await cdp.evaluate(snippets.chipState("Summary"));
  await cdp.evaluate(snippets.stubInvoke({ reject: { summary_settings_set: "Choose a summary provider and model." } }));
  await pressChip(cdp, "Summary");
  await waitFor(cdp, `${field(snippets.taskPageState(), "errors.length")} > 0`, "the Summary error line");
  measured.summary.refused = { chip: await cdp.evaluate(snippets.chipState("Summary")), errors: (await lastState(cdp)).errors };
  const shotSummary = await screenshot(cdp, out, "06-summary-refused.png");

  const t = measured.tidy;
  const s = measured.summary;
  addCheck(report, {
    id: "6",
    claim: "Tidy up and Summary toggle in place once set up, are links until then, show a route's caveat with a link, and show a refused write without changing.",
    measured,
    expectation: [
      "Tidy up (fixture: configured, on): a button with aria-pressed true, no chevron; a press flips it to false and back; with the write stubbed to reject, the message is on the error line and aria-pressed stays true;",
      "with reasoning_status stubbed to a route with a caveat: the chip reads needs setup and a status line shows the caveat with the link Open Cleanup & summaries, and the privacy line does not say text is sent;",
      "with reasoning_status stubbed to never-configured: no aria-pressed, a chevron, and a press leaves the page and writes nothing;",
      "Summary (fixture: unconfigured): the link Summary: needs setup, which leaves the page; once configured through the fixture's own write, aria-pressed false, a press makes it true, and a rejected write shows on the error line with aria-pressed still true",
    ].join(" "),
    matches: t.initial.pressed === "true" && !t.initial.chevron && t.initial.text === "Tidy up: on"
      && t.off.pressed === "false" && t.off.text === "Tidy up: off" && t.on.pressed === "true"
      && t.refused.errors.some((text) => text.startsWith("cleanup needs a model")) && t.refused.chip.pressed === "true"
      && t.caveat.chip.text === "Tidy up: needs setup" && t.caveat.notes.length === 1
      && t.caveat.notes[0].text.includes("no key is stored for anthropic") && t.caveat.notes[0].link === "Open Cleanup & summaries"
      && t.caveat.privacy.length > 0 && !t.caveat.privacy.some((line) => line.startsWith("Text is sent"))
      && t.unconfigured.chip.pressed === null && t.unconfigured.chip.chevron
      && t.unconfigured.leftThePage && t.unconfigured.writes === 0
      && s.unconfigured.chip.text === "Summary: needs setup" && s.unconfigured.chip.pressed === null
      && s.unconfigured.chip.chevron && s.unconfigured.leftThePage
      && s.off.text === "Summary" && s.off.pressed === "false" && s.on.pressed === "true"
      && s.refused.errors.includes("Choose a summary provider and model.") && s.refused.chip.pressed === "true",
    screenshots: [shotRefused, shotCaveat, shotSummary],
    note: "Every unusual state here is the page's invoke stubbed for that one command; the fixture defaults are unchanged. 'Left the page' means the control bar is gone after the press.",
  });
}

/** 3b and 2b. Meetings: choosing changes the chip and the privacy line; a greyed row on the keyboard. */
async function checkMeetingsChoices(cdp, out, report) {
  await freshPage(cdp, "Meetings");
  const measured = { initial: { chip: await cdp.evaluate(snippets.chipState("Meeting transcription: ")), page: await lastState(cdp) } };
  await pressChip(cdp, "Meeting transcription: ");
  await waitFor(cdp, snippets.menuVisible(), "the provider menu");
  await pressOption(cdp, "OpenAI Realtime");
  await waitFor(cdp, `${chipField("Meeting transcription: ", "text")} === "OpenAI Realtime"`, "the chip to read OpenAI Realtime");
  await delay(300);
  measured.openai = { chip: await cdp.evaluate(snippets.chipState("Meeting transcription: ")), page: await lastState(cdp) };

  // OpenAI Realtime cannot detect a language, so its language menu greys "Detect language".
  await cdp.evaluate(snippets.focusChip("Language: "));
  await key(cdp, "ArrowDown");
  await waitFor(cdp, snippets.menuVisible(), "the language menu from the keyboard");
  await delay(120);
  const greyed = await walk(cdp, "ArrowUp", (state) => state.options.some((row) => row.greyed && state.active === `option:${row.name}`), 12);
  await cdp.evaluate(snippets.clearStubCalls());
  await key(cdp, "Enter");
  measured.greyedLanguage = {
    active: greyed.active,
    row: greyed.options.find((row) => greyed.active === `option:${row.name}`),
    menuStillOpen: (await cdp.evaluate(snippets.menuState())).open,
    writes: (await cdp.evaluate(snippets.stubCalls("meeting_set_language"))).length,
  };
  await closeMenu(cdp);
  await pressChip(cdp, "Language: ");
  await waitFor(cdp, snippets.menuVisible(), "the language menu");
  await pressOption(cdp, "Swedish");
  await waitFor(cdp, `${chipField("Language: ", "text")} === "Swedish"`, "the chip to read Swedish");
  measured.swedish = await cdp.evaluate(snippets.chipState("Language: "));
  // And back to Deepgram, which the fixtures start on: the line follows again.
  await pressChip(cdp, "Meeting transcription: ");
  await waitFor(cdp, snippets.menuVisible(), "the provider menu");
  await pressOption(cdp, "Deepgram");
  await waitFor(cdp, `${chipField("Meeting transcription: ", "text")} === "Deepgram"`, "the chip to read Deepgram");
  await delay(300);
  measured.deepgram = { chip: await cdp.evaluate(snippets.chipState("Meeting transcription: ")), page: await lastState(cdp) };

  const privacy = (state) => state.page.privacy.join(" | ");
  addCheck(report, {
    id: "3b",
    claim: "Choosing a Meetings provider updates the chip and the privacy line; the language menu greys what the provider cannot do, and the keyboard cannot choose it.",
    measured: {
      privacy: { initial: privacy(measured.initial), deepgram: privacy(measured.deepgram), openai: privacy(measured.openai) },
      chips: [measured.initial.chip.text, measured.openai.chip.text, measured.swedish.text, measured.deepgram.chip.text],
      greyedLanguage: measured.greyedLanguage,
    },
    expectation: [
      "starting on the fixtures' Deepgram, choosing OpenAI Realtime and then Deepgram again: the chip names each, the privacy line changes each time and names that provider;",
      "on OpenAI Realtime the language menu's Detect language row is aria-disabled with a reason, ArrowUp reaches it, Enter there leaves the menu open and writes nothing;",
      "choosing Swedish changes the chip to Swedish",
    ].join(" "),
    matches: measured.initial.chip.text === "Deepgram" && measured.openai.chip.text === "OpenAI Realtime"
      && measured.deepgram.chip.text === "Deepgram"
      && privacy(measured.openai) !== privacy(measured.initial)
      && privacy(measured.deepgram) !== privacy(measured.openai)
      && privacy(measured.initial).includes("Deepgram") && privacy(measured.openai).includes("OpenAI Realtime")
      && privacy(measured.deepgram).includes("Deepgram")
      && measured.greyedLanguage.row?.greyed === true && Boolean(measured.greyedLanguage.row.reason)
      && measured.greyedLanguage.menuStillOpen === true && measured.greyedLanguage.writes === 0
      && measured.swedish.text === "Swedish",
    note: "Meetings provider wording is the page's own; this checks it changes and names the provider, not its exact sentence.",
  });
}

/** 7. The hero's Choose a provider, and Start held during a slow write. */
async function checkMeetingsHero(cdp, out, report) {
  await freshPage(cdp, "Meetings");
  // The fixture has no provider; one the page cannot use leaves the hero asking for a choice.
  await cdp.evaluate(snippets.invokeFixture("meeting_set_provider", { provider: "meeting:local" }));
  await enterPage(cdp, "Meetings");
  const measured = { hero: await lastState(cdp) };
  const shotHero = await screenshot(cdp, out, "07-hero-choose-provider.png");
  await press(cdp, await cdp.evaluate(snippets.centerOfText(".task-hero button", "Choose a provider")));
  await waitFor(cdp, snippets.menuVisible(), "the provider menu from the hero");
  await delay(120);
  measured.opened = {
    menu: await cdp.evaluate(snippets.menuState()),
    chip: await cdp.evaluate(snippets.chipState("Meeting transcription: ")),
    stillOnMeetings: await cdp.evaluate("Boolean(document.querySelector('.control-bar'))"),
  };
  const shotMenu = await screenshot(cdp, out, "07-hero-opened-menu.png");
  // The current row is the fallback; Enter writes it, slowly.
  await cdp.evaluate(snippets.stubInvoke({ delay: { meeting_set_provider: 1500 } }));
  await cdp.evaluate(snippets.clearStubCalls());
  await key(cdp, "Enter");
  await delay(250);
  measured.writing = { page: await lastState(cdp), writes: (await cdp.evaluate(snippets.stubCalls("meeting_set_provider"))).length };
  await waitFor(cdp, `${field(snippets.taskPageState(), "start.disabled")} === false`, "Start to be released", 8_000);
  await delay(250);
  measured.after = { page: await lastState(cdp), chip: await cdp.evaluate(snippets.chipState("Meeting transcription: ")) };

  const greyed = measured.opened.menu.options.filter((row) => row.greyed);
  addCheck(report, {
    id: "7",
    claim: "Meetings' hero button Choose a provider opens the provider menu without leaving the page, and Start is held while the provider is being written.",
    measured: {
      heroButtons: measured.hero.hero, startBefore: measured.hero.start,
      opened: { name: measured.opened.menu.name, expanded: measured.opened.chip.expanded, greyed: greyed.map((row) => row.name), current: measured.opened.menu.options.filter((row) => row.selected).map((row) => row.name) },
      writing: measured.writing, after: measured.after,
    },
    expectation: [
      "with a saved provider the page cannot use (set through the fixture's own write): the hero offers Choose a provider and Start is disabled;",
      "pressing it opens the Meeting transcription menu (chip aria-expanded true, control bar still there) with a greyed row and one current row;",
      "Enter on the current row with meeting_set_provider stubbed 1.5s slower: one write, Start disabled while it is in flight and enabled after, the hero no longer offering the choice",
    ].join(" "),
    matches: measured.hero.hero.includes("Choose a provider") && measured.hero.start?.disabled === true
      && measured.opened.menu.name === "Meeting transcription" && measured.opened.chip.expanded === "true"
      && measured.opened.stillOnMeetings && greyed.length >= 1
      && measured.opened.menu.options.filter((row) => row.selected).length === 1
      && measured.writing.writes === 1 && measured.writing.page.start?.disabled === true
      && measured.after.page.start?.disabled === false && !measured.after.page.hero.includes("Choose a provider"),
    screenshots: [shotHero, shotMenu],
    note: "The saved provider is Local model, which the fixtures list as unusable here; Rust's states for it are not exercised.",
  });
}

/** 8. The minimum window: no sideways scroll, and a menu still inside it. */
async function checkMinimumWindow(cdp, out, report) {
  await viewport(cdp, 720, 520);
  const measured = {};
  for (const page of ["Dictation", "Meetings"]) {
    await freshPage(cdp, page);
    const state = await lastState(cdp);
    measured[page] = state.scroll;
    const prefix = CHIP_OF[page];
    measured[`${page}Menu`] = await openMenu(cdp, prefix).then((menu) => ({ box: menu.box, viewport: menu.viewport, inside: placedInside(menu) }));
    await closeMenu(cdp);
  }
  const shot = await screenshot(cdp, out, "08-minimum-720x520.png");
  // `null <= null` is true, so a missing .split-main or .control-bar must fail
  // the check rather than pass it.
  const fits = (scroll) => scroll.mainClientWidth !== null && scroll.barClientWidth !== null
    && scroll.scrollWidth <= scroll.innerWidth
    && scroll.mainScrollWidth <= scroll.mainClientWidth && scroll.barScrollWidth <= scroll.barClientWidth;
  addCheck(report, {
    id: "8",
    claim: "At 720x520 nothing scrolls sideways on Dictation or Meetings, and a menu still opens inside the window.",
    measured,
    expectation: "after a load at 720x520, for each page: document, .split-main and .control-bar scrollWidth <= clientWidth; the page's first menu is inside the window",
    matches: fits(measured.Dictation) && fits(measured.Meetings)
      && measured.DictationMenu.inside && measured.MeetingsMenu.inside,
    screenshots: [shot],
    note: "The screenshot is of Meetings, the last page measured. The control bar's own width is measured too, since the page scrolls inside the card.",
  });
  await viewport(cdp, 1000, 720);
}

/** 9. An open menu on each page, in light and dark. */
async function checkThemes(cdp, out, report) {
  const measured = {};
  const screenshots = [];
  for (const [page, prefix, what] of [["Dictation", "Transcription: ", "dictation-route"], ["Meetings", "Meeting transcription: ", "meetings-provider"]]) {
    await freshPage(cdp, page);
    for (const scheme of ["light", "dark"]) {
      await theme(cdp, scheme);
      const menu = await openMenu(cdp, prefix);
      measured[`${what}-${scheme}`] = { ...(await cdp.evaluate(snippets.themeState())), inside: placedAboveBar(menu), rows: menu.options.length };
      screenshots.push(await screenshot(cdp, out, shotName("09", what, scheme)));
      await closeMenu(cdp);
    }
  }
  await theme(cdp, "light");
  const pairs = ["dictation-route", "meetings-provider"];
  addCheck(report, {
    id: "9",
    claim: "An open menu on each page is drawn in light and in dark.",
    measured,
    expectation: "prefers-color-scheme honoured (dark false then true) and the menu's background differs between the two schemes, on both pages, the menu open above the bar inside the window",
    matches: pairs.every((what) => measured[`${what}-light`].dark === false && measured[`${what}-dark`].dark === true
      && measured[`${what}-light`].menuBackground !== measured[`${what}-dark`].menuBackground
      && measured[`${what}-light`].inside && measured[`${what}-dark`].inside),
    screenshots,
    note: "Whether the dark menu reads well is for a person to judge from the screenshots; the check only shows the scheme changed.",
  });
}

export async function runChipMenus(options) {
  const out = resolve(options.out);
  await mkdir(out, { recursive: true });
  let browser;
  let cdp;
  try {
    browser = await launchEdge({ ...options, out });
    cdp = await connectCdp(browser.target.webSocketDebuggerUrl);
    await cdp.send("Page.enable");
    await theme(cdp, "light");
    await viewport(cdp, 1000, 720);
    await reload(cdp);
    await shellReady(cdp);
    const report = createReport({
      url: options.url,
      edgeVersion: browser.version.Browser ?? browser.version,
      remBasis: await cdp.evaluate("getComputedStyle(document.documentElement).fontSize"),
      fixtureBannerRemoved: true,
      timestamp: new Date().toISOString(),
      measurementScope: "measured in headless Edge against dev fixtures, not in the Tauri window",
    });

    await checkPlacement(cdp, out, report);
    const cloudChoice = await checkKeyboard(cdp, report);
    await checkDictationChoices(cdp, out, report, cloudChoice);
    await checkDictationHeld(cdp, out, report);
    await checkToggles(cdp, out, report);
    await checkMeetingsChoices(cdp, out, report);
    await checkMeetingsHero(cdp, out, report);
    await checkMinimumWindow(cdp, out, report);
    await checkThemes(cdp, out, report);

    // 3a (Dictation) and 3b (Meetings) are one claim measured in two places.
    report.checks.sort((a, b) => a.id.localeCompare(b.id, "en", { numeric: true }));
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
    if (options.help) process.stdout.write(`${shellUsage().replace("shell-rail.mjs", "chip-menus.mjs")}\n`);
    else {
      const failed = failedChecks(await runChipMenus(options));
      // The run completed; a check measured false still fails it, so a wrapper or CI step sees it.
      if (failed.length) {
        console.error(`chip-menus: ${failed.length} check(s) false: ${failed.join(", ")}`);
        process.exitCode = 1;
      }
    }
  } catch (error) {
    console.error(`chip-menus failed: ${error.message}`);
    process.exitCode = 1;
  }
}
