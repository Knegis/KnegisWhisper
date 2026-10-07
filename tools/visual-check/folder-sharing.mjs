#!/usr/bin/env node
/** Measures the actual sharing UI against invented browser data, not native IPC. */
import { mkdir, writeFile } from "node:fs/promises";
import { resolve } from "node:path";
import { launchEdge, killBrowser } from "./browser.mjs";
import { connectCdp } from "./cdp.mjs";
import { clickText, clickSelector, removeFixtureBanner } from "./snippets.mjs";

const targetUrl = new URL(process.argv[2] ?? "http://127.0.0.1:5197/");
targetUrl.searchParams.set("sharingSlow", "1");
const url = targetUrl.href;
const out = resolve("tools/visual-check/out/kneg-411");
await mkdir(out, { recursive: true });
const browser = await launchEdge({ port: 9237, out, url });
const cdp = await connectCdp(browser.target.webSocketDebuggerUrl);
const checks = [];
async function wait(expression) {
  const deadline = Date.now() + 15000;
  while (Date.now() < deadline) { if (await cdp.evaluate(expression)) return; await new Promise(resolveWait => setTimeout(resolveWait, 100)); }
  throw new Error(`Timed out: ${expression}`);
}
async function check(name, expression) {
  const passed = await cdp.evaluate(expression);
  checks.push({ name, passed });
  if (!passed) throw new Error(`${name}; focus: ${await cdp.evaluate("document.activeElement?.outerHTML.slice(0, 300)")}`);
}
try {
  await wait("Boolean(document.querySelector('nav.rail'))");
  await cdp.evaluate(removeFixtureBanner());
  await cdp.evaluate(clickText("nav.rail button", "Notes"));
  await wait("Boolean(document.querySelector('.notes-folder-more'))");
  await cdp.evaluate(clickSelector(".notes-folder-more"));
  await wait("Boolean(document.querySelector('.notes-context-menu'))");
  await cdp.evaluate(clickText(".notes-context-menu button", "Share folder…"));
  await wait("Boolean(document.querySelector('dialog[open] .folder-sharing-notes input'))");
  await check("All notes and content selected", "[...document.querySelectorAll('dialog[open] input[type=checkbox]')].every(input => input.checked)");
  await cdp.evaluate(clickText("dialog[open] button", "Select none"));
  await check("An empty selection cannot export", "[...document.querySelectorAll('dialog[open] button')].find(button => button.textContent === 'Save sharing file…').disabled");
  await cdp.evaluate(clickText("dialog[open] button", "Select all"));
  await cdp.evaluate("[...document.querySelectorAll('dialog[open] fieldset label')].find(label => label.textContent==='Transcripts').querySelector('input').click()");
  await check("Excluding transcripts explains summary links", "document.querySelector('dialog[open]').textContent.includes('Summary source links will be removed')");
  await cdp.screenshot(resolve(out, "share-wide.png"));
  await cdp.send("Emulation.setDeviceMetricsOverride", { width: 720, height: 520, deviceScaleFactor: 1, mobile: false });
  await check("Sharing dialog fits minimum window", "(() => {const dialog=document.querySelector('dialog[open]');const rect=dialog.getBoundingClientRect();return rect.left>=0&&rect.right<=innerWidth&&rect.top>=0&&rect.bottom<=innerHeight&&dialog.scrollWidth<=dialog.clientWidth})()");
  await cdp.screenshot(resolve(out, "share-narrow.png"));
  await cdp.send("Input.dispatchKeyEvent", { type: "keyDown", key: "Escape", code: "Escape", windowsVirtualKeyCode: 27 });
  await wait("!document.querySelector('dialog[open]')");
  await wait("document.activeElement?.getAttribute('aria-controls')==='list-panel'");
  await check("Closing returns focus when the opener is hidden", "document.activeElement?.getAttribute('aria-controls')==='list-panel'");
  await cdp.send("Emulation.setDeviceMetricsOverride", { width: 1000, height: 720, deviceScaleFactor: 1, mobile: false });
  await cdp.evaluate("new Promise(resolveFrame => requestAnimationFrame(() => requestAnimationFrame(resolveFrame)))");
  await cdp.evaluate(clickSelector('button[aria-controls="list-panel"]'));
  await wait("!document.querySelector('#list-panel').hidden");
  await cdp.evaluate(clickSelector(".notes-folder-more"));
  await wait("Boolean(document.querySelector('.notes-context-menu'))");
  await cdp.evaluate(clickText(".notes-context-menu button", "Share folder…"));
  await wait("Boolean(document.querySelector('dialog[open] .folder-sharing-notes input'))");
  await cdp.evaluate(clickText("dialog[open] button", "Save sharing file…"));
  await wait("document.querySelector('dialog[open]')?.textContent.includes('Copying notes and recordings')");
  await cdp.evaluate(clickText("dialog[open] button", "Keep working"));
  await wait("!document.querySelector('dialog[open]')");
  await cdp.evaluate(clickText("nav.rail button", "Settings"));
  await wait("!document.querySelector('.notes-folder-more')");
  await cdp.evaluate(clickText("nav.rail button", "Notes"));
  await wait("Boolean(document.querySelector('dialog[open]'))");
  await check("Returning to Notes reconnects to the running operation", "document.querySelector('dialog[open]').textContent.includes('Copying notes and recordings')");
  await wait("Boolean(document.querySelector('dialog[open] .folder-sharing-path'))");
  await check("Export success displays saved location", "document.querySelector('dialog[open]').textContent.includes('Project.knegis')");
  await cdp.evaluate(clickText("dialog[open] button", "Close"));
  await wait("!document.querySelector('dialog[open]')");
  await cdp.evaluate(clickText(".list-panel button", "Import shared folder…"));
  await wait("Boolean(document.querySelector('dialog[open] .folder-sharing-name input'))");
  await check("Import preview explains independent copy", "document.querySelector('dialog[open]').textContent.includes('Existing folders and notes won’t change')");
  await cdp.screenshot(resolve(out, "import-preview.png"));
  await cdp.evaluate(clickText("dialog[open] button", "Import folder"));
  await wait("document.querySelector('dialog[open]')?.textContent.includes('1 note imported.')");
  await cdp.evaluate(clickText("dialog[open] button", "Close"));
  await wait("!document.querySelector('dialog[open]')");
  await check("Imported folder and note appear in library", "[...document.querySelectorAll('.notes-folder-name')].some(element=>element.textContent==='Shared project')&&document.querySelector('.note-detail').textContent.includes('Invented shared personal notes')");
  await cdp.screenshot(resolve(out, "imported.png"));
} finally {
  await writeFile(resolve(out, "report.json"), JSON.stringify({ evidence: "Headless Edge with invented browser fixtures; not native app validation", checks }, null, 2));
  await cdp.close(); await killBrowser(browser.child);
}
console.log(`Sharing UI: ${checks.length} checks passed; ${out}`);
