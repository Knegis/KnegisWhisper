#!/usr/bin/env node
/**
 * Measures, against dev fixtures, the form that names a speaker in a note's
 * transcript: a named speaker's form holds the name and offers Remove name, an
 * unnamed one's starts empty with its label as the placeholder and doesn't, the
 * suggestions are the app's own pills and narrow as you type, a press fills the
 * field and saves nothing, Escape closes from a pill, and nothing leaves the
 * column in either scheme or at 720x520.
 *
 * Fixture note 101 has speakers "Anna" and "Ravi" and suggests "Anna Lind" and
 * "Erik Berg". Each check starts from a fresh load, which resets the fixtures.
 * Which names Rust suggests is pinned by `cargo test`, not here.
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
const WIDE = { width: 1440, height: 900 };
const SMALL = { width: 720, height: 520 };
const TITLE = "Weekly sync — platform";
const FORM = "form.transcript-rename";
const noteOpen = `(() => { const input = document.querySelector('.note-detail textarea[aria-label="Title"]'); return Boolean(input) && input.value === ${JSON.stringify(TITLE)}; })()`;

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

/** A fresh load with fixture note 101 open and its transcript in view. */
async function openNote(cdp, size = WIDE) {
  await cdp.send("Emulation.setDeviceMetricsOverride", { ...size, deviceScaleFactor: 1, mobile: false });
  await reload(cdp);
  await waitFor(cdp, "Boolean(document.querySelector('.app-titlebar') && document.querySelector('.split-main, .content'))", "the fixture shell");
  await cdp.evaluate(snippets.removeFixtureBanner());
  await cdp.evaluate(snippets.clickText(NAV_PILL, "Notes"));
  await waitFor(cdp, "Boolean(document.querySelector('.list-panel .notes-entry'))", "the Notes library");
  await cdp.evaluate(snippets.openNotesEntry(TITLE));
  await waitFor(cdp, noteOpen, "the note to open");
  await waitFor(cdp, "Boolean(document.querySelector('button.transcript-speaker'))", "the transcript's speakers");
  await delay(200);
}

const heads = (cdp) => cdp.evaluate("[...document.querySelectorAll('button.transcript-speaker')].map((button) => button.textContent.trim())");

async function openForm(cdp, speaker) {
  await cdp.evaluate(`(() => {
    const button = [...document.querySelectorAll('button.transcript-speaker')].find((item) => item.textContent.trim() === ${JSON.stringify(speaker)});
    if (!button) throw new Error('No speaker button reads ' + ${JSON.stringify(speaker)});
    button.click();
  })()`);
  await waitFor(cdp, `Boolean(document.querySelector('${FORM} input:focus'))`, "the form to open with focus in its field");
  await delay(150);
}

/** The form as it stands. */
const formState = (cdp) => cdp.evaluate(`(() => {
  const form = document.querySelector('${FORM}');
  if (!form) return null;
  const input = form.querySelector('input');
  const pills = [...form.querySelectorAll('.transcript-rename-suggestions button')];
  const actions = [...form.querySelectorAll('.transcript-rename-actions button')];
  const save = actions.find((button) => button.textContent.trim() === 'Save');
  return {
    value: input.value,
    placeholder: input.getAttribute('placeholder'),
    focusInField: document.activeElement === input,
    datalists: document.querySelectorAll('datalist').length,
    list: input.getAttribute('list'),
    pills: pills.map((pill) => pill.textContent.trim()),
    pressed: pills.filter((pill) => pill.getAttribute('aria-pressed') === 'true').map((pill) => pill.textContent.trim()),
    actions: actions.map((button) => button.textContent.trim()),
    saveDisabled: save?.disabled ?? null,
    label: form.querySelector('label')?.textContent.trim() ?? null,
    accessibleName: form.getAttribute('aria-label'),
  };
})()`);

/** How it is drawn: the shared pieces, and whether anything leaves the form or the window. */
const drawn = (cdp) => cdp.evaluate(`(() => {
  const form = document.querySelector('${FORM}');
  const box = form.getBoundingClientRect();
  const parts = [...form.querySelectorAll('label, input, button')];
  const input = getComputedStyle(form.querySelector('input'));
  const save = [...form.querySelectorAll('button')].find((button) => button.textContent.trim() === 'Save');
  const label = getComputedStyle(form.querySelector('label'));
  return {
    formRadius: getComputedStyle(form).borderRadius,
    inputRadius: input.borderRadius,
    inputHeight: input.height,
    inputBorder: input.borderTopWidth,
    labelFont: label.fontFamily.split(',')[0],
    saveImage: getComputedStyle(save).backgroundImage.slice(0, 15),
    overflow: form.scrollWidth - form.clientWidth,
    outside: parts.filter((part) => { const r = part.getBoundingClientRect(); return r.left < box.left - 0.5 || r.right > box.right + 0.5 || r.right > innerWidth; }).map((part) => part.textContent.trim() || part.tagName),
    pageOverflow: document.documentElement.scrollWidth - innerWidth,
  };
})()`);

async function typeInto(cdp, text) {
  await cdp.evaluate(`(() => { const input = document.querySelector('${FORM} input'); input.focus(); input.select(); })()`);
  if (text === "") await cdp.send("Input.dispatchKeyEvent", { type: "keyDown", key: "Delete", code: "Delete", windowsVirtualKeyCode: 46 });
  else await cdp.send("Input.insertText", { text });
  if (text === "") await cdp.send("Input.dispatchKeyEvent", { type: "keyUp", key: "Delete", code: "Delete", windowsVirtualKeyCode: 46 });
  await delay(120);
}

async function pressKey(cdp, key, code = key, windowsVirtualKeyCode = 0) {
  await cdp.send("Input.dispatchKeyEvent", { type: "keyDown", key, code, windowsVirtualKeyCode, text: key === "Enter" ? "\r" : undefined });
  await cdp.send("Input.dispatchKeyEvent", { type: "keyUp", key, code, windowsVirtualKeyCode });
  await delay(200);
}

/** A real pointer press on the first button in the form reading `words`. */
async function press(cdp, words) {
  const point = await cdp.evaluate(`(() => {
    const button = [...document.querySelectorAll('${FORM} button')].find((item) => item.textContent.trim() === ${JSON.stringify(words)});
    if (!button) throw new Error('No form button reads ' + ${JSON.stringify(words)});
    button.scrollIntoView({ block: 'nearest' });
    const r = button.getBoundingClientRect();
    return { x: (r.left + r.right) / 2, y: (r.top + r.bottom) / 2 };
  })()`);
  await cdp.send("Input.dispatchMouseEvent", { type: "mouseMoved", ...point });
  await cdp.send("Input.dispatchMouseEvent", { type: "mousePressed", ...point, button: "left", clickCount: 1 });
  await cdp.send("Input.dispatchMouseEvent", { type: "mouseReleased", ...point, button: "left", clickCount: 1 });
  await delay(200);
}

const SETUP_NOTE = "Each check starts from a fresh load of fixture note 101. Buttons in the form are pressed with a real pointer; a speaker's head is a DOM click, since only its name matters here.";

export async function runSpeakerRenameForm(options) {
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
      claim: "A named speaker's form holds the name, offers Remove name and the suggestions as the app's own pills, with no native list.",
      expectation: "field \"Anna\" with focus; pills Anna Lind and Erik Berg; actions Remove name, Cancel, Save, with Save off; no datalist; the form still named for the speaker",
    }, async () => {
      await openNote(cdp);
      await openForm(cdp, "Anna");
      const form = await formState(cdp);
      return {
        measured: form,
        matches: form.value === "Anna" && form.focusInField && form.datalists === 0 && form.list === null
          && form.pills.join() === "Anna Lind,Erik Berg" && form.actions.join() === "Remove name,Cancel,Save" && form.saveDisabled === true
          && form.label === "name this speaker" && form.accessibleName === "Name for Anna",
      };
    });

    await check({
      id: "2",
      claim: "Typing narrows the pills, and clearing brings them back.",
      expectation: "\"er\" leaves Erik Berg; \"zzz\" leaves none; cleared, both again",
    }, async () => {
      await openNote(cdp);
      await openForm(cdp, "Anna");
      await typeInto(cdp, "er");
      const some = await formState(cdp);
      await typeInto(cdp, "zzz");
      const none = await formState(cdp);
      await typeInto(cdp, "");
      const all = await formState(cdp);
      return {
        measured: { some: some.pills, none: none.pills, all: all.pills },
        matches: some.pills.join() === "Erik Berg" && none.pills.length === 0 && all.pills.join() === "Anna Lind,Erik Berg",
      };
    });

    await check({
      id: "3",
      claim: "A pill fills the field, reads as chosen, hands focus back and saves nothing; Enter then saves it.",
      expectation: "pressing Erik Berg: field Erik Berg with focus, that pill pressed, the other still there, Save on, the turn head still Anna; Enter: the form closes and the head reads Erik Berg",
    }, async () => {
      await openNote(cdp);
      await openForm(cdp, "Anna");
      await press(cdp, "Erik Berg");
      const form = await formState(cdp);
      const before = await heads(cdp);
      const shot = "03-pill-pressed-1440x900.png";
      await cdp.screenshot(resolve(out, shot));
      await pressKey(cdp, "Enter", "Enter", 13);
      await waitFor(cdp, `!document.querySelector('${FORM}')`, "the form to close after saving");
      const after = await heads(cdp);
      return {
        measured: { form, before, after },
        matches: form.value === "Erik Berg" && form.focusInField && form.pressed.join() === "Erik Berg" && form.pills.length === 2
          && form.saveDisabled === false && before.includes("Anna") && !before.includes("Erik Berg") && after.includes("Erik Berg") && !after.includes("Anna"),
        screenshots: [shot],
      };
    });

    await check({
      id: "4",
      claim: "Escape closes the form from a pill as well as from the field, and changes nothing.",
      expectation: "focus on a pill, Escape: no form, and the head still reads Anna",
    }, async () => {
      await openNote(cdp);
      await openForm(cdp, "Anna");
      await cdp.evaluate(`document.querySelector('${FORM} .transcript-rename-suggestions button').focus(); true`);
      await pressKey(cdp, "Escape", "Escape", 27);
      const closed = await cdp.evaluate(`!document.querySelector('${FORM}')`);
      const after = await heads(cdp);
      return { measured: { closed, after }, matches: closed && after.includes("Anna") };
    });

    await check({
      id: "5",
      claim: "With no name to remove, the form starts empty with the label as its placeholder, and doesn't offer Remove name.",
      expectation: "after Remove name the head reads Speaker 1; its form: empty field, placeholder Speaker 1, actions Cancel, Save, with Save off; typing Sara Holm and Enter names the speaker, and reopening holds that name with Remove name back",
    }, async () => {
      await openNote(cdp);
      await openForm(cdp, "Anna");
      await press(cdp, "Remove name");
      await waitFor(cdp, `!document.querySelector('${FORM}')`, "the form to close after removing");
      const removed = await heads(cdp);
      await openForm(cdp, "Speaker 1");
      const unnamed = await formState(cdp);
      const shot = "05-unnamed-1440x900.png";
      await cdp.screenshot(resolve(out, shot));
      await typeInto(cdp, "Sara Holm");
      await pressKey(cdp, "Enter", "Enter", 13);
      await waitFor(cdp, `!document.querySelector('${FORM}')`, "the form to close after saving");
      const after = await heads(cdp);
      await openForm(cdp, "Sara Holm");
      const reopened = await formState(cdp);
      return {
        measured: { removed, unnamed, after, reopened },
        matches: removed.includes("Speaker 1") && unnamed.value === "" && unnamed.placeholder === "Speaker 1"
          && unnamed.actions.join() === "Cancel,Save" && unnamed.saveDisabled === true
          && after.includes("Sara Holm") && reopened.value === "Sara Holm" && reopened.actions.join() === "Remove name,Cancel,Save",
        screenshots: [shot],
      };
    });

    await check({
      id: "6",
      claim: "It is drawn in the design's own pieces, and nothing leaves the form or the window, in either scheme or at the smallest window.",
      expectation: "pill field 36 high with no border; pixel label; Iris Save; no part outside the form; no sideways scroll; in dark and light at 1440x900, and at 720x520",
    }, async () => {
      const measured = {};
      const screenshots = [];
      for (const [name, size, scheme] of [["light-wide", WIDE, "light"], ["dark-wide", WIDE, "dark"], ["light-small", SMALL, "light"]]) {
        await cdp.send("Emulation.setEmulatedMedia", { features: [{ name: "prefers-color-scheme", value: scheme }] });
        await openNote(cdp, size);
        await cdp.evaluate(snippets.markColorScheme(scheme));
        // In a narrow window the transcript is behind its own tab.
        if (size === SMALL) {
          await cdp.evaluate(snippets.clickText('.note-detail [role="tab"]', "Transcript"));
          await waitFor(cdp, "document.querySelector('button.transcript-speaker')?.offsetParent != null", "the Transcript tab");
        }
        await openForm(cdp, "Anna");
        await delay(150);
        measured[name] = await drawn(cdp);
        const shot = `06-${name}.png`;
        await cdp.screenshot(resolve(out, shot));
        screenshots.push(shot);
      }
      const good = (d) => d.inputRadius === "100px" && d.inputHeight === "36px" && d.inputBorder === "0px" && /Tiny5/.test(d.labelFont)
        && d.saveImage.startsWith("linear-gradient") && d.overflow <= 0 && d.outside.length === 0 && d.pageOverflow <= 0;
      return { measured, matches: Object.values(measured).every(good), screenshots };
    });


    await check({
      id: "7",
      claim: "The collapsed library toggle and transcript label have separate hit areas, and keyboard toggling preserves focus.",
      expectation: "No overlap or horizontal overflow in both themes at 720x520; pointer hits the toggle; Enter expands and Space collapses with focus transferred",
    }, async () => {
      const measured = [];
      for (const scheme of ["light", "dark"]) {
        await openNote(cdp, SMALL);
        await cdp.evaluate(snippets.markColorScheme(scheme));
        await cdp.evaluate(snippets.clickText('.note-detail [role="tab"]', "Transcript"));
        await delay(200);
        const geometry = await cdp.evaluate(`(() => {
          const toggle = document.querySelector('.list-toggle[data-collapsed="true"]');
          const label = document.querySelector('.note-transcript-inline .transcript-label');
          const a = toggle.getBoundingClientRect(), b = label.getBoundingClientRect();
          const hit = document.elementFromPoint(a.x + a.width / 2, a.y + a.height / 2);
          toggle.focus();
          return { overlap: a.left < b.right && a.right > b.left && a.top < b.bottom && a.bottom > b.top,
            hit: toggle.contains(hit), overflow: document.documentElement.scrollWidth > innerWidth };
        })()`);
        await pressKey(cdp, "Enter", "Enter", 13);
        await delay(200);
        const expanded = await cdp.evaluate('document.activeElement?.getAttribute("aria-label") === "Hide list" && document.querySelector("#list-panel").hidden === false');
        await pressKey(cdp, " ", "Space", 32);
        await delay(200);
        const collapsed = await cdp.evaluate('document.activeElement?.getAttribute("aria-label") === "Show list" && document.querySelector("#list-panel").hidden === true');
        const point = await cdp.evaluate(`(() => { const r = document.activeElement.getBoundingClientRect(); return {x:r.x+r.width/2,y:r.y+r.height/2}; })()`);
        await cdp.send("Input.dispatchMouseEvent", {type:"mousePressed", ...point, button:"left", clickCount:1});
        await cdp.send("Input.dispatchMouseEvent", {type:"mouseReleased", ...point, button:"left", clickCount:1});
        await delay(200);
        const pointerExpanded = await cdp.evaluate('document.querySelector("#list-panel").hidden === false');
        measured.push({ scheme, ...geometry, expanded, collapsed, pointerExpanded });
      }
      return { measured, matches: measured.every(m => !m.overlap && m.hit && !m.overflow && m.expanded && m.collapsed && m.pointerExpanded) };
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
    if (options.help) process.stdout.write(`${shellUsage().replace("shell-title-bar.mjs", "speaker-rename-form.mjs")}\n`);
    else {
      const failed = failedChecks(await runSpeakerRenameForm(options));
      if (failed.length) {
        console.error(`speaker-rename-form: ${failed.length} check(s) false: ${failed.join(", ")}`);
        process.exitCode = 1;
      }
    }
  } catch (error) {
    console.error(`speaker-rename-form failed: ${error.message}`);
    process.exitCode = 1;
  }
}
