#!/usr/bin/env node
/**
 * Measures, against dev fixtures, the note editor's Add and Drag handles: each icon
 * centred in the box that lights up under it, visible at rest, pointed at and pressed
 * in both themes, each box's whole width still its own, and Add still opening the
 * block menu. Crepe's other icons read the same variables, so the handles' colours
 * must stay scoped to them.
 *
 * Colours are composited in the page, on a canvas, because the icon's fill and the
 * hover are both translucent. Headless Edge, not the real window. Dragging a block
 * isn't exercised.
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
const WINDOW = { width: 1280, height: 800 };
const HANDLE = ".note-editor .milkdown-block-handle";
/** WCAG 1.4.11's floor for an icon that means something. */
const MINIMUM_CONTRAST = 3;
const ADD = 0;
const DRAG = 1;

const channel = (value) => {
  const c = value / 255;
  return c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
};
const luminance = ([r, g, b]) => 0.2126 * channel(r) + 0.7152 * channel(g) + 0.0722 * channel(b);
export function contrast(a, b) {
  const [high, low] = [luminance(a), luminance(b)].sort((x, y) => y - x);
  return Math.round(((high + 0.05) / (low + 0.05)) * 100) / 100;
}

/** Where each box and its drawn shape are, and the colours as they land on the sheet. */
export async function readHandles(cdp) {
  const read = await cdp.evaluate(`(() => {
    const handle = document.querySelector(${JSON.stringify(HANDLE)});
    const canvas = document.createElement('canvas');
    canvas.width = canvas.height = 1;
    const context = canvas.getContext('2d', { willReadFrequently: true });
    const paint = (...layers) => {
      context.clearRect(0, 0, 1, 1);
      for (const layer of layers) { context.fillStyle = layer; context.fillRect(0, 0, 1, 1); }
      return [...context.getImageData(0, 0, 1, 1).data];
    };
    let sheet = 'rgb(255, 255, 255)';
    for (let el = handle; el; el = el.parentElement) {
      const color = getComputedStyle(el).backgroundColor;
      if (paint(color)[3] === 255) { sheet = color; break; }
    }
    const box = (r) => ({ left: r.left, top: r.top, right: r.right, bottom: r.bottom });
    return {
      show: handle.dataset.show,
      handle: box(handle.getBoundingClientRect()),
      sheet: paint(sheet).slice(0, 3),
      items: [...handle.querySelectorAll('.operation-item')].map((item) => {
        const svg = item.querySelector('svg');
        const shape = svg.getBBox();
        const m = svg.getScreenCTM();
        const fill = getComputedStyle(svg.querySelector('path')).fill;
        const background = getComputedStyle(item).backgroundColor;
        return {
          box: box(item.getBoundingClientRect()),
          drawn: {
            left: m.a * shape.x + m.e,
            top: m.d * shape.y + m.f,
            right: m.a * (shape.x + shape.width) + m.e,
            bottom: m.d * (shape.y + shape.height) + m.f,
          },
          back: paint(sheet, background).slice(0, 3),
          icon: paint(sheet, background, fill).slice(0, 3),
        };
      }),
    };
  })()`);
  return { ...read, items: read.items.map((item) => ({ ...item, contrast: contrast(item.icon, item.back) })) };
}

const moveTo = (cdp, x, y) => cdp.send("Input.dispatchMouseEvent", { type: "mouseMoved", x, y });
const pointer = (cdp, type, x, y) => cdp.send("Input.dispatchMouseEvent", { type, x, y, button: "left", buttons: type === "mousePressed" ? 1 : 0, clickCount: 1 });
/** Over the title bar's logo: on no block. */
const moveAway = (cdp) => moveTo(cdp, 640, 28);
const centre = ({ left, top, right, bottom }) => ({ x: (left + right) / 2, y: (top + bottom) / 2 });

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

/** A fresh load on Notes, pointing at a paragraph so that its handles show. */
export async function showHandles(cdp) {
  await cdp.send("Emulation.setDeviceMetricsOverride", { ...WINDOW, deviceScaleFactor: 1, mobile: false });
  await moveAway(cdp);
  await reload(cdp);
  await waitFor(cdp, "Boolean(document.querySelector('.app-titlebar') && document.querySelector('.split-main, .content'))", "the first paint");
  await cdp.evaluate(snippets.removeFixtureBanner());
  await cdp.evaluate(snippets.clickText(NAV_PILL, "Notes"));
  await waitFor(cdp, "Boolean(document.querySelector('.note-editor .ProseMirror p'))", "a note with a paragraph");
  await delay(400);
  const text = await cdp.evaluate(`(() => {
    const r = document.querySelector('.note-editor .ProseMirror p').getBoundingClientRect();
    return { left: r.left, top: r.top, bottom: r.bottom };
  })()`);
  await moveTo(cdp, text.left + 20, (text.top + text.bottom) / 2);
  await waitFor(cdp, `document.querySelector(${JSON.stringify(HANDLE)})?.dataset.show === 'true'`, "the handles to show");
  // The handle's own `transition: all .2s`.
  await delay(300);
  return text;
}

export async function pointAt(cdp, item) {
  const { x, y } = centre(item.box);
  await moveTo(cdp, x, y);
  await delay(120);
  return readHandles(cdp);
}

/** Runs `body` with the given scheme emulated, whatever happens in it. */
async function inScheme(cdp, scheme, body) {
  await cdp.send("Emulation.setEmulatedMedia", { features: [{ name: "prefers-color-scheme", value: scheme }] });
  try {
    return await body();
  } finally {
    await cdp.send("Emulation.setEmulatedMedia", { features: [] });
  }
}

/** The handles and a little of the text beside them, large enough to see. */
async function closeUp(cdp, out, name) {
  const { handle } = await readHandles(cdp);
  const clip = { x: handle.left - 12, y: handle.top - 8, width: handle.right - handle.left + 80, height: handle.bottom - handle.top + 16, scale: 5 };
  const { data } = await cdp.send("Page.captureScreenshot", { format: "png", clip });
  await writeFile(resolve(out, name), Buffer.from(data, "base64"));
  return name;
}

const SETUP_NOTE = "Each check starts from a fresh load on Notes at 1280x800, pointing at the note's first paragraph.";

export async function runEditorHandles(options) {
  const out = resolve(options.out);
  await mkdir(out, { recursive: true });
  let browser;
  let cdp;
  try {
    browser = await launchEdge({ ...options, out, width: WINDOW.width, height: WINDOW.height });
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
      claim: "Each icon is centred in the box that lights up under it.",
      expectation: "for Add and Drag, the drawn shape's centre within 0.5px of its box's on both axes, and the shape inside the box",
    }, async () => {
      await showHandles(cdp);
      const { items } = await readHandles(cdp);
      const offsets = items.map(({ box, drawn }) => ({
        x: Math.round((centre(drawn).x - centre(box).x) * 100) / 100,
        y: Math.round((centre(drawn).y - centre(box).y) * 100) / 100,
        inside: drawn.left >= box.left && drawn.right <= box.right && drawn.top >= box.top && drawn.bottom <= box.bottom,
      }));
      return { measured: { offsets }, matches: items.length === 2 && offsets.every((o) => Math.abs(o.x) <= 0.5 && Math.abs(o.y) <= 0.5 && o.inside) };
    });

    await check({
      id: "2",
      claim: "The handles stay inside the note sheet, clear of the text.",
      expectation: "the handle's left at or inside the scroller's, and its right at or left of the paragraph's left",
    }, async () => {
      const text = await showHandles(cdp);
      const { handle } = await readHandles(cdp);
      const scroller = await cdp.evaluate(`document.querySelector('.note-body').getBoundingClientRect().left`);
      return { measured: { handle, scroller, text: text.left }, matches: handle.left >= scroller && handle.right <= text.left };
    });

    for (const [id, scheme] of [["3", "light"], ["4", "dark"]]) {
      await check({
        id,
        claim: `In ${scheme}, both icons can be seen at rest and pointed at, and the hover is lighter than a control's.`,
        expectation: `at least ${MINIMUM_CONTRAST}:1 at rest and on each one's hover; the hover shows against the sheet, closer to it than \`--control\` on the sheet`,
      }, () => inScheme(cdp, scheme, async () => {
        await showHandles(cdp);
        const rest = await readHandles(cdp);
        const shots = [await closeUp(cdp, out, `${id}-${scheme}-rest.png`)];
        const add = await pointAt(cdp, rest.items[ADD]);
        shots.push(await closeUp(cdp, out, `${id}-${scheme}-hover-add.png`));
        const drag = await pointAt(cdp, rest.items[DRAG]);
        shots.push(await closeUp(cdp, out, `${id}-${scheme}-hover-drag.png`));
        const control = await cdp.evaluate(`(() => {
          const canvas = document.createElement('canvas');
          canvas.width = canvas.height = 1;
          const context = canvas.getContext('2d');
          context.fillStyle = ${JSON.stringify(`rgb(${rest.sheet.join(", ")})`)};
          context.fillRect(0, 0, 1, 1);
          context.fillStyle = getComputedStyle(document.documentElement).getPropertyValue('--control').trim();
          context.fillRect(0, 0, 1, 1);
          return [...context.getImageData(0, 0, 1, 1).data].slice(0, 3);
        })()`);
        const hover = add.items[ADD].back;
        const measured = {
          sheet: rest.sheet,
          restIcon: rest.items[ADD].icon,
          restContrast: rest.items.map((item) => item.contrast),
          hover,
          control,
          hoverContrast: [add.items[ADD].contrast, drag.items[DRAG].contrast],
        };
        const away = (colour) => Math.abs(luminance(colour) - luminance(rest.sheet));
        return {
          measured,
          matches: [...measured.restContrast, ...measured.hoverContrast].every((value) => value >= MINIMUM_CONTRAST)
            && away(hover) > 0 && away(hover) < away(control),
          screenshots: shots,
        };
      }));
    }

    await check({
      id: "5",
      claim: "The pressed Add can be seen too, on its darker fill.",
      expectation: `at least ${MINIMUM_CONTRAST}:1 while the button is held, in light and in dark`,
    }, async () => {
      const measured = {};
      for (const scheme of ["light", "dark"]) {
        await inScheme(cdp, scheme, async () => {
          await showHandles(cdp);
          const { items } = await readHandles(cdp);
          const { x, y } = centre(items[ADD].box);
          await moveTo(cdp, x, y);
          await pointer(cdp, "mousePressed", x, y);
          await delay(120);
          const held = await readHandles(cdp);
          measured[scheme] = { back: held.items[ADD].back, icon: held.items[ADD].icon, contrast: held.items[ADD].contrast };
          await closeUp(cdp, out, `5-${scheme}-pressed.png`);
          // Let go elsewhere, so nothing is added to the fixture's note.
          await moveAway(cdp);
          await pointer(cdp, "mouseReleased", 640, 28);
        });
      }
      return {
        measured,
        matches: Object.values(measured).every((state) => state.contrast >= MINIMUM_CONTRAST),
        screenshots: ["5-light-pressed.png", "5-dark-pressed.png"],
      };
    });

    await check({
      id: "6",
      claim: "A box lights up only with the pointer inside it, though the icons run past their boxes.",
      expectation: "1px inside each box's left and right edges hits that box; 1px outside them hits neither box",
    }, async () => {
      await showHandles(cdp);
      const { items } = await readHandles(cdp);
      const hits = await cdp.evaluate(`(() => {
        const items = [...document.querySelectorAll(${JSON.stringify(`${HANDLE} .operation-item`)})];
        const owner = (x, y) => items.indexOf(document.elementFromPoint(x, y)?.closest('.operation-item'));
        return ${JSON.stringify(items.map((item) => ({ ...item.box, y: centre(item.box).y })))}
          .map((b) => [owner(b.left + 1, b.y), owner(b.right - 1, b.y), owner(b.left - 1, b.y), owner(b.right + 1, b.y)]);
      })()`);
      return { measured: { hits }, matches: JSON.stringify(hits) === JSON.stringify([[ADD, ADD, -1, -1], [DRAG, DRAG, -1, -1]]) };
    });

    await check({
      id: "7",
      claim: "Pressing Add still opens the block menu.",
      expectation: "a press and release on Add's icon shows `.milkdown-slash-menu`",
    }, async () => {
      await showHandles(cdp);
      const { items } = await readHandles(cdp);
      const { x, y } = centre(items[ADD].drawn);
      await moveTo(cdp, x, y);
      await pointer(cdp, "mousePressed", x, y);
      await pointer(cdp, "mouseReleased", x, y);
      await waitFor(cdp, "document.querySelector('.note-editor .milkdown-slash-menu')?.dataset.show === 'true'", "the block menu", 3000);
      return { measured: { menu: "shown" }, matches: true };
    });

    await check({
      id: "8",
      claim: "Crepe's other icons keep their colours; only the handles' are changed.",
      expectation: "on `.milkdown`, the outline and hover variables still read `--faint` and `--control`; on the handle, they don't",
    }, async () => {
      await showHandles(cdp);
      const read = await cdp.evaluate(`(() => {
        const value = (el, name) => getComputedStyle(el).getPropertyValue(name).trim();
        const root = document.documentElement;
        const editor = document.querySelector('.note-editor .milkdown');
        const handle = document.querySelector(${JSON.stringify(HANDLE)});
        return {
          faint: value(root, '--faint'),
          control: value(root, '--control'),
          editorOutline: value(editor, '--crepe-color-outline'),
          editorHover: value(editor, '--crepe-color-hover'),
          handleOutline: value(handle, '--crepe-color-outline'),
          handleHover: value(handle, '--crepe-color-hover'),
        };
      })()`);
      return {
        measured: read,
        matches: read.editorOutline === read.faint && read.editorHover === read.control
          && read.handleOutline !== read.faint && read.handleHover !== read.control,
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
    if (options.help) process.stdout.write(`${shellUsage().replace("shell-title-bar.mjs", "editor-handles.mjs")}\n`);
    else {
      const failed = failedChecks(await runEditorHandles(options));
      if (failed.length) {
        console.error(`editor-handles: ${failed.length} check(s) false: ${failed.join(", ")}`);
        process.exitCode = 1;
      }
    }
  } catch (error) {
    console.error(`editor-handles failed: ${error.message}`);
    process.exitCode = 1;
  }
}
