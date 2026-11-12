/** KNEG-547: controlled read delays/errors over invented fixtures. */
import assert from "node:assert/strict";
import { mkdir, writeFile } from "node:fs/promises";
import { resolve } from "node:path";
import { parseShellArgs } from "./args.mjs";
import { launchEdge, killBrowser } from "./browser.mjs";
import { connectCdp } from "./cdp.mjs";
import { delay, waitFor } from "./interactions.mjs";
import { clickText, removeFixtureBanner } from "./snippets.mjs";
const options = parseShellArgs(process.argv.slice(2)), out = resolve(options.out);
await mkdir(out, { recursive: true });
const browser = await launchEdge({ ...options, out });
const c = await connectCdp(browser.target.webSocketDebuggerUrl), results = [];
const pill = name => c.evaluate(clickText('button.nav-pill', name));
const loaded = () => waitFor(c, 'Boolean(document.querySelector(".task-arrival"))');
const notes = async () => { await pill('Notes'); await delay(100); };
const release = () => c.evaluate('window.__gate.command = ""; window.__gate.waiting.splice(0).forEach(resolve => resolve())');
const gate = (task, fail = false) => c.evaluate(`window.__gate.command = ${JSON.stringify(task === 'Dictation' ? 'input_devices' : 'meeting_status')}; window.__gate.fail = ${fail}`);
try {
  await loaded(); await c.evaluate(removeFixtureBanner());
  await c.evaluate(`(() => {
    const original = window.__TAURI_INTERNALS__.invoke;
    window.__gate = { command: '', fail: false, waiting: [] };
    window.__TAURI_INTERNALS__.invoke = async (command, args) => {
      const gate = window.__gate;
      if (command === gate.command) {
        if (gate.fail) throw Error('Synthetic loading failure');
        await new Promise(resolve => gate.waiting.push(resolve));
      }
      return original(command, args);
    };
  })()`);
  for (const scheme of ['light', 'dark']) for (const width of [720,1280]) for (const task of ['Dictation','Meetings']) {
    await c.send('Emulation.setDeviceMetricsOverride', {width, height: width === 720 ? 520 : 800, deviceScaleFactor:1, mobile:false});
    await c.send('Emulation.setEmulatedMedia', {features:[{name:'prefers-color-scheme',value:scheme}]});
    await notes(); await gate(task); await pill(task);
    await waitFor(c, 'Boolean(document.querySelector(".task-loading"))');
    assert.equal(await c.evaluate('getComputedStyle(document.querySelector(".task-placeholder")).visibility'), 'hidden');
    await waitFor(c, 'document.querySelector(".task-loading")?.dataset.details === "true"');
    assert.equal(await c.evaluate('getComputedStyle(document.querySelector(".task-placeholder")).visibility'), 'visible');
    assert.equal(await c.evaluate('document.querySelectorAll(".task-loading button, .task-loading input, .task-loading select").length'), 0);
    assert.equal(await c.evaluate('document.querySelectorAll("h1").length'), 1);
    assert.equal(await c.evaluate('document.querySelectorAll(".task-loading [role=status]").length'), 1);
    assert.equal(await c.evaluate('document.querySelector(".task-loading section").getAttribute("aria-busy")'), 'true');
    assert.equal(await c.evaluate('document.querySelector(".split-main").scrollWidth > document.querySelector(".split-main").clientWidth + 1'), false);
    await c.screenshot(resolve(out, `${task}-${scheme}-${width}.png`));
    await release(); await loaded();
    assert.equal(await c.evaluate('getComputedStyle(document.querySelector(".task-arrival")).animationDuration'), '0.12s');
    results.push(`${task}: ${scheme} ${width}, delayed placeholders and fresh content`);
  }
  for (const task of ['Dictation','Meetings']) {
    await notes(); await gate(task, true); await pill(task);
    await waitFor(c, 'Boolean(document.querySelector(".task-loading [role=alert]"))');
    assert.equal(await c.evaluate('document.querySelector(".task-loading [role=alert]").textContent'), 'Synthetic loading failure');
    assert.equal(await c.evaluate('document.querySelector(".task-loading section").getAttribute("aria-busy")'), 'false');
    await notes(); await gate(task); await pill(task);
    await waitFor(c, 'Boolean(document.querySelector(".task-loading"))');
    await notes(); await release(); await delay(220);
    assert.equal(await c.evaluate('Boolean(document.querySelector(".task-loading, .task-arrival"))'), false);
    await c.evaluate(`window.__flashes = 0; window.__watch = new MutationObserver(() => { if(document.querySelector('.task-loading[data-details="true"]')) window.__flashes++; }); window.__watch.observe(document.body,{subtree:true,childList:true,attributes:true});`);
    await pill(task); await loaded(); await delay(180);
    assert.equal(await c.evaluate('window.__flashes'), 0); await c.evaluate('window.__watch.disconnect()');
    results.push(`${task}: error, unmount with outstanding read, fast revisit`);
  }
  await pill('Dictation'); await loaded(); await gate('Dictation');
  await c.evaluate(`document.querySelector('button[aria-label^="Microphone:"]').click()`);
  await waitFor(c, 'Boolean(document.querySelector(".chip-menu-option"))');
  await c.evaluate(`document.querySelector('.chip-menu-option:not([aria-disabled="true"])').click()`);
  await waitFor(c, 'window.__gate.waiting.length > 0');
  assert.equal(await c.evaluate('Boolean(document.querySelector(".task-arrival"))'), true);
  assert.equal(await c.evaluate('Boolean(document.querySelector(".task-loading"))'), false);
  await release(); results.push('background refresh retains loaded content');
  await c.send('Emulation.setEmulatedMedia', {features:[{name:'prefers-reduced-motion',value:'reduce'}]});
  await pill('Dictation'); await loaded();
  assert.equal(await c.evaluate('getComputedStyle(document.querySelector(".task-arrival")).animationName'), 'none');
  results.push('reduced motion disables arrival');
  await writeFile(resolve(out,'results.json'),JSON.stringify(results,null,2)); console.log(JSON.stringify(results,null,2));
} catch (error) { console.error(await c.evaluate('document.body.innerText.slice(0,1800)')); await c.screenshot(resolve(out,'failure.png')); throw error; }
finally {await c.close(); await killBrowser(browser.child);}
