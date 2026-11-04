/** KNEG-399 setup/return checks against invented fixtures, with no provider request. */
import assert from "node:assert/strict";
import { mkdir, writeFile } from "node:fs/promises";
import { resolve } from "node:path";
import { parseShellArgs } from "./args.mjs";
import { launchEdge, killBrowser } from "./browser.mjs";
import { connectCdp } from "./cdp.mjs";
import { delay, reload, waitFor, pressText } from "./interactions.mjs";
import { clickText, confirmQuestions, openNotesEntry, openSettingsScreen, removeFixtureBanner, scriptConfirm } from "./snippets.mjs";

const options = parseShellArgs(process.argv.slice(2)), out = resolve(options.out);
await mkdir(out, { recursive: true });
let browser, cdp;
const results = [];
const invoke = (command, args = {}) => cdp.evaluate(`window.__TAURI_INTERNALS__.invoke(${JSON.stringify(command)}, ${JSON.stringify(args)})`);
const config = { enabled: false, provider: "", model: "", selfHostedUrl: null, enterprise: false, language: "match", contextBudget: 8192 };
const title = "Weekly sync — platform";
const text = selector => cdp.evaluate(`document.querySelector(${JSON.stringify(selector)})?.textContent.trim()`);
const input = async (selector, value) => {
  await cdp.evaluate(`(() => { const input = document.querySelector(${JSON.stringify(selector)}); input.scrollIntoView({block:'center'}); input.focus(); input.select(); })()`);
  await cdp.send("Input.insertText", { text: value });
  await delay(120);
};
try {
  browser = await launchEdge({ ...options, out, extraArgs: ["--inprivate"] }); cdp = await connectCdp(browser.target.webSocketDebuggerUrl);
  await cdp.send("Page.enable");
  for (const scheme of ["light", "dark"]) for (const width of [720, 1440]) {
    console.log(`Checking summary setup: ${scheme}, ${width}px`);
    await cdp.send("Emulation.setDeviceMetricsOverride", { width, height: width === 720 ? 520 : 900, deviceScaleFactor: 1, mobile: false });
    await cdp.send("Emulation.setEmulatedMedia", { features: [{ name: "prefers-color-scheme", value: scheme }] });
    await reload(cdp); await waitFor(cdp, "Boolean(document.querySelector('.app-titlebar'))");
    console.log("Shell loaded");
    await cdp.evaluate(removeFixtureBanner());
    await cdp.evaluate(scriptConfirm(true));
    await invoke("summary_settings_set", { settings: config });
    console.log("Saved incomplete setup");
    await cdp.evaluate(`(() => { window.__generationCalls = 0; const invoke = window.__TAURI_INTERNALS__.invoke; window.__TAURI_INTERNALS__.invoke = (command, args) => { if (command === 'note_summary_generate') ++window.__generationCalls; return invoke(command, args); }; })()`);
    await cdp.evaluate(clickText("button.nav-pill", "Notes"));
    await waitFor(cdp, "Boolean(document.querySelector('.notes-entry'))");
    await cdp.evaluate(openNotesEntry(title));
    await waitFor(cdp, "document.querySelector('[data-summary-action]')?.textContent === 'Set up summaries'");
    console.log("Note ready for setup");
    await pressText(cdp, "[data-summary-action]", null);
    await waitFor(cdp, "document.activeElement?.id === 'summary-provider'", "focused summary setup");
    console.log("Settings focused");
    assert.equal(await cdp.evaluate("window.__generationCalls"), 0, "setup does not enqueue a summary");
    assert.equal(await text(".summary-setup-return button"), "Return to note");
    // Complete the existing form with an explicit self-hosted choice. It remains automatic-off.
    await cdp.evaluate("(() => { const select = document.querySelector('#summary-provider'); select.value = 'self-hosted'; select.dispatchEvent(new Event('change', {bubbles:true})); })()");
    await waitFor(cdp, "Boolean(document.querySelector('#summary-endpoint'))");
    await input("#summary-endpoint", "http://127.0.0.1:18881/v1");
    await input("#summary-model", "fixture");
    await pressText(cdp, '[data-surface="settings.intelligence.summaries"] button', "Save summary settings");
    await waitFor(cdp, "[...document.querySelectorAll('[data-surface=\"settings.intelligence.summaries\"] [role=status]')].some(e => e.textContent === 'Saved')");
    await pressText(cdp, ".summary-setup-return button", "Return to note");
    console.log("Returning to note");
    await waitFor(cdp, "document.querySelector('.note-detail textarea[aria-label=Title]')?.value === " + JSON.stringify(title));
    await waitFor(cdp, "document.activeElement?.matches('[data-summary-action]')", "summary action focused after returning");
    assert.notEqual(await text("[data-summary-action]"), "Set up summaries");
    assert.equal(await cdp.evaluate("window.__generationCalls"), 0, "returning never generates automatically");
    assert.equal((await invoke("summary_settings_get")).enabled, false);
    await pressText(cdp, "[data-summary-action]", null);
    await waitFor(cdp, "document.querySelector('.summary-pane [role=status]')?.textContent === 'Summary completed'");
    await waitFor(cdp, "document.querySelector('.summary-pane')?.textContent.includes('A fixture summary of')", "generated content replaces the old editor seed");
    assert.equal(await cdp.evaluate("window.__generationCalls"), 1);
    // Only this test's synthetic edited summary is replaced.
    assert.ok((await cdp.evaluate(confirmQuestions())).every(question => question.startsWith("Replace your edited summary?")));
    await cdp.screenshot(`${out}/${scheme}-${width}.png`);
    // An unreadable setup is an error and Retry, rather than a false needs-setup conclusion.
    await cdp.evaluate(`(() => { const invoke = window.__TAURI_INTERNALS__.invoke; window.__TAURI_INTERNALS__.invoke = (command,args) => command === 'summary_readiness_get' ? Promise.reject(new Error('synthetic read failure')) : invoke(command,args); })()`);
    await waitFor(cdp, "document.querySelector('[data-summary-action]')?.textContent === 'Retry setup check'");
    assert.ok((await text(".summary-pane")).includes("synthetic read failure"));
    await cdp.evaluate(openSettingsScreen("Cleanup & summaries"));
    await waitFor(cdp, "Boolean(document.querySelector('#summary-model'))", "setup form survives a readiness read failure");
    assert.equal(await cdp.evaluate("document.querySelector('#summary-model').value"), "fixture");
    await waitFor(cdp, "[...document.querySelectorAll('[data-surface=\"settings.intelligence.summaries\"] button')].some(e => e.textContent === 'Retry setup check')");
    results.push({ scheme, width, checks: ["setup before enqueue", "focused form", "explicit save", "same note and action focus", "automatic off", "no generation on return", "manual generation", "setup read failure", "form survives readiness error"] });
  }
  await writeFile(`${out}/report.json`, JSON.stringify({ scope: "headless Edge, invented fixtures", results }, null, 2));
  console.log(JSON.stringify({ passed: results.length, results }));
} finally { if (cdp) await cdp.close(); if (browser) await killBrowser(browser.child); }
