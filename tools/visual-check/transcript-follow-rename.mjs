/** KNEG-453/454 regression checks. Only invented browser fixtures are changed. */
import assert from "node:assert/strict";
import { mkdir, writeFile } from "node:fs/promises";
import { resolve } from "node:path";
import { parseShellArgs } from "./args.mjs";
import { launchEdge, killBrowser } from "./browser.mjs";
import { connectCdp } from "./cdp.mjs";
import { delay, reload, waitFor, press } from "./interactions.mjs";
import { clickText, openNotesEntry, centerOfText, removeFixtureBanner } from "./snippets.mjs";

const options = parseShellArgs(process.argv.slice(2));
const out = resolve(options.out);
await mkdir(out, { recursive: true });
let browser, cdp;
const results = [];
const key = async name => {
  await cdp.send("Input.dispatchKeyEvent", { type: "keyDown", key: name });
  await cdp.send("Input.dispatchKeyEvent", { type: "keyUp", key: name });
  await delay(100);
};
const follow = () => cdp.evaluate("document.querySelector('.follow-chip')?.getAttribute('aria-pressed') === 'true'");
const click = async selector => press(cdp, await cdp.evaluate(centerOfText(selector, null)));
try {
  browser = await launchEdge({ ...options, out, extraArgs: ["--mute-audio", "--inprivate"] });
  cdp = await connectCdp(browser.target.webSocketDebuggerUrl);
  await cdp.send("Page.enable");
  for (const scheme of ["light", "dark"]) for (const width of [1000, 1440]) {
    await cdp.send("Emulation.setDeviceMetricsOverride", { width, height: 900, deviceScaleFactor: 1, mobile: false });
    await cdp.send("Emulation.setEmulatedMedia", { features: [{ name: "prefers-color-scheme", value: scheme }] });
    await reload(cdp);
    await waitFor(cdp, "Boolean(document.querySelector('nav.rail'))");
    await cdp.evaluate(removeFixtureBanner());
    await cdp.evaluate(clickText("nav.rail .rail-item", "Notes"));
    await waitFor(cdp, "Boolean(document.querySelector('.notes-entry'))");
    await cdp.evaluate(openNotesEntry("Roadmap review — long transcript"));
    if (width < 1280) {
      await waitFor(cdp, "Boolean(document.querySelector('.note-detail [role=tab]'))");
      await cdp.evaluate(clickText(".note-detail [role=tab]", "Transcript"));
    }
    await waitFor(cdp, "document.querySelectorAll('.transcript-line').length === 54 && Boolean(document.querySelector('.follow-chip'))");
    await delay(350);
    const scroller = width < 1280 ? ".notes-main" : ".note-side-body";
    assert.equal(await follow(), true);
    assert.equal(await cdp.evaluate("[...document.querySelectorAll('.transcript-speaker')].filter(e => e.tagName === 'BUTTON' && e.tabIndex >= 0).length"), 0);
    assert.equal(await cdp.evaluate("[...document.querySelectorAll('.transcript-line')].filter(e => e.tabIndex === 0).length"), 1);
    await cdp.evaluate(`document.querySelector(${JSON.stringify(scroller)}).scrollTop += 150`);
    await waitFor(cdp, "document.querySelector('.follow-chip')?.getAttribute('aria-pressed') === 'false'", "unowned scroll releases Follow");
    await click(".follow-chip");
    await delay(700);
    assert.equal(await follow(), true, "app recenter keeps Follow on");
    await cdp.evaluate("document.querySelector('.transcript-line').focus({preventScroll:true})");
    await key("F2");
    await waitFor(cdp, "Boolean(document.querySelector('.transcript-rename input:focus'))");
    assert.equal(await follow(), false, "renaming releases Follow");
    await click(".follow-chip");
    assert.equal(await follow(), false, "Follow cannot restart while renaming");
    await click(".transcript-rename input");
    await key("Escape");
    assert.equal(await cdp.evaluate("document.activeElement === document.querySelector('.transcript-line')"), true);
    assert.equal(await follow(), false);
    const word = await cdp.evaluate("Boolean(document.querySelector('.transcript-line .transcript-word'))");
    if (word) {
      await cdp.evaluate("document.querySelector('.transcript-line .transcript-word').focus({preventScroll:true})");
      await key("F2");
      await waitFor(cdp, "Boolean(document.querySelector('.transcript-rename input:focus'))");
      await key("Escape");
    }
    await click(".follow-chip");
    // Interrupt the app's glide immediately; there is no 400 ms immunity.
    const point = await cdp.evaluate(centerOfText(scroller, null));
    await cdp.send("Input.dispatchMouseEvent", { type: "mouseWheel", ...point, deltaX: 0, deltaY: 160 });
    await waitFor(cdp, "document.querySelector('.follow-chip')?.getAttribute('aria-pressed') === 'false'", "wheel interrupts reveal");
    await cdp.screenshot(`${out}/${scheme}-${width}.png`);
    results.push({ scheme, width, checks: ["bounded rename Tab stops", "unowned scroll", "owned reveal", "F2 rename", "rename blocks Follow", "focus restoration", "word F2", "wheel interruption"] });
  }
  await writeFile(`${out}/report.json`, JSON.stringify({ scope: "headless Edge, invented fixtures", results }, null, 2));
  console.log(JSON.stringify({ passed: results.length, results }));
} finally {
  if (cdp) await cdp.close();
  if (browser) await killBrowser(browser.child);
}
