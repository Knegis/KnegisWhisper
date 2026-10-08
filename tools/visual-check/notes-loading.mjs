#!/usr/bin/env node
/** Browser fixture evidence for KNEG-420; Rust/native checks prove storage and IPC. */
import assert from "node:assert/strict";
import { mkdir, writeFile } from "node:fs/promises";
import { resolve } from "node:path";
import { launchEdge, killBrowser } from "./browser.mjs";
import { connectCdp } from "./cdp.mjs";
import { waitFor } from "./interactions.mjs";
import * as snippets from "./snippets.mjs";
const out = resolve("tools/visual-check/out/kneg-420");
await mkdir(out, { recursive: true });
const browser = await launchEdge({ port: 9238, out, extraArgs: ["--disable-gpu"], url: process.argv[2] ?? "http://127.0.0.1:5198/" });
const cdp = await connectCdp(browser.target.webSocketDebuggerUrl);
const checks = [];
const pause = ms => new Promise(done => setTimeout(done, ms));
const wait = expression => waitFor(cdp, expression);
async function check(name, expression) { assert.ok(await cdp.evaluate(expression), name); checks.push(name); }
const input = '.note-detail input[aria-label="Title"]';
async function typeTitle(value) {
  await cdp.evaluate(`document.querySelector(${JSON.stringify(input)}).focus()`);
  await cdp.send("Input.dispatchKeyEvent", { type: "keyDown", key: "a", code: "KeyA", modifiers: 2, windowsVirtualKeyCode: 65 });
  await cdp.send("Input.dispatchKeyEvent", { type: "keyUp", key: "a", code: "KeyA", modifiers: 2, windowsVirtualKeyCode: 65 });
  await cdp.send("Input.insertText", { text: value });
}
try {
  await wait("Boolean(window.__TAURI_INTERNALS__ && document.querySelector('nav.rail'))");
  await cdp.evaluate(snippets.removeFixtureBanner());
  await cdp.evaluate(snippets.installInvokeStub());
  await cdp.evaluate(snippets.clickText("nav.rail button", "Notes"));
  await wait(`document.querySelector(${JSON.stringify(input)}) && !document.querySelector(${JSON.stringify(input)}).disabled`);
  await check("Library uses metadata and selected note uses full read", "window.__chipStub.log.some(x=>x.cmd==='notes_list_summaries') && window.__chipStub.log.some(x=>x.cmd==='notes_get') && !window.__chipStub.log.some(x=>x.cmd==='notes_list')");
  const items = await cdp.evaluate("window.__TAURI_INTERNALS__.invoke('notes_list_summaries',{})");
  assert.ok(items.every(item => !('transcript' in item) && !('content' in item))); checks.push("Metadata payload excludes full note bodies");
  await cdp.evaluate("window.__editorBefore=document.querySelector('.ProseMirror'); true");
  await cdp.evaluate(snippets.clearStubCalls());
  await typeTitle("Saved metadata title");
  await wait("[...document.querySelectorAll('.notes-entry')].some(x=>x.textContent.includes('Saved metadata title'))");
  await check("Autosave uses one metadata read without library, folders or Trash reads", "window.__chipStub.log.filter(x=>x.cmd==='notes_get_summary').length===1 && !window.__chipStub.log.some(x=>['notes_list','notes_list_summaries','folders_list','notes_trash_list','notes_get'].includes(x.cmd))");
  await check("Autosave preserves editor and keyboard focus", "window.__editorBefore===document.querySelector('.ProseMirror') && document.activeElement===document.querySelector('.note-detail input[aria-label=Title]')");
  await cdp.evaluate(snippets.stubInvoke({ delay: { notes_get_summary: 1400 } }));
  await typeTitle("First delayed title");
  await wait("window.__chipStub.log.filter(x=>x.cmd==='notes_get_summary').length===2");
  await typeTitle("Newest typing survives");
  await pause(800);
  await check("Delayed acknowledgment does not overwrite newer typing", "document.querySelector('.note-detail input[aria-label=Title]').value==='Newest typing survives'");
  await wait("[...document.querySelectorAll('.notes-entry')].some(x=>x.textContent.includes('Newest typing survives'))");
  await cdp.evaluate(snippets.stubInvoke({ delay: { notes_get_summary: null }, reject: { notes_get_summary: "Synthetic metadata failure" } }));
  await typeTitle("Committed despite refresh failure");
  await wait("document.body.innerText.includes('could not be refreshed')");
  await check("A committed write with refresh failure is still Saved", "document.body.innerText.includes('Saved') && !document.body.innerText.includes('Save failed')");
  await cdp.evaluate(snippets.stubInvoke({ reject: { notes_get_summary: null, notes_update: "Synthetic save failure" } }));
  await typeTitle("Retry my edit");
  await wait("document.body.innerText.includes('Synthetic save failure')");
  await check("Failed write retains the typed title", "document.querySelector('.note-detail input[aria-label=Title]').value==='Retry my edit'");
  await cdp.evaluate(snippets.stubInvoke({ reject: { notes_update: null } }));
  await cdp.evaluate(snippets.clickText(".note-detail button", "Retry save"));
  await wait("[...document.querySelectorAll('.notes-entry')].some(x=>x.textContent.includes('Retry my edit'))");
  checks.push("Retry writes and acknowledges the retained edit");
  // Two reads complete in reverse order: the latest chosen note must win.
  await cdp.evaluate(`window.__raceOriginal=window.__TAURI_INTERNALS__.invoke.bind(window.__TAURI_INTERNALS__); window.__TAURI_INTERNALS__.invoke=async(command,args,options)=>{if(command==='notes_get'&&args.id===102)await new Promise(done=>setTimeout(done,1400));return window.__raceOriginal(command,args,options);}; true`);
  const second = items.find(item => item.id === 102);
  const third = items.find(item => item.id === 103);
  await cdp.evaluate(`[...document.querySelectorAll('.notes-entry')].find(item=>item.textContent.includes(${JSON.stringify(second.title)})).click(); true`);
  await pause(100);
  await cdp.evaluate(`[...document.querySelectorAll('.notes-entry')].find(item=>item.textContent.includes(${JSON.stringify(third.title)})).click(); true`);
  await wait(`document.querySelector('.note-detail input[aria-label=Title]')?.value===${JSON.stringify(third.title)}`);
  await pause(1600);
  await check("Latest selection wins over a delayed previous read", `document.querySelector('.note-detail input[aria-label=Title]')?.value===${JSON.stringify(third.title)}`);
  await cdp.evaluate("window.__TAURI_INTERNALS__.invoke=window.__raceOriginal; true");
  await cdp.evaluate(`[...document.querySelectorAll('.notes-person')].find(item=>item.textContent.includes('Anna Lindqvist')).click(); true`);
  await wait("[...document.querySelectorAll('.panel-label')].some(item=>item.textContent==='Notes with Anna Lindqvist')");
  await cdp.evaluate("[...document.querySelectorAll('.notes-entry')][0].click(); true");
  await wait("document.querySelector('.note-detail input[aria-label=Title]')?.value==='Retry my edit'");
  await cdp.evaluate(snippets.clearStubCalls());
  await typeTitle("Filtered saved title");
  await wait("[...document.querySelectorAll('.notes-entry')].some(item=>item.textContent.includes('Filtered saved title'))");
  await check("Autosave updates an active People list without refetching it", "[...document.querySelectorAll('.panel-label')].some(item=>item.textContent==='Notes with Anna Lindqvist') && !window.__chipStub.log.some(item=>item.cmd==='notes_list_summaries') && document.querySelectorAll('.notes-entry').length===1");
  await cdp.send("Emulation.setDeviceMetricsOverride", { width: 720, height: 520, deviceScaleFactor: 1, mobile: false });
  await check("Minimum-window layout fits", "document.documentElement.scrollWidth<=innerWidth");
  await cdp.screenshot(resolve(out, "minimum.png"));
} finally {
  await writeFile(resolve(out, "report.json"), JSON.stringify({ evidence: "Headless Edge with invented fixtures", checks }, null, 2));
  await cdp.close(); await killBrowser(browser.child);
}
console.log(`Notes loading: ${checks.length} checks passed`);
