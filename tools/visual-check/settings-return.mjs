/** KNEG-548 navigation regression over invented browser fixtures. */
import assert from "node:assert/strict";
import { mkdir, writeFile } from "node:fs/promises";
import { resolve } from "node:path";
import { parseShellArgs } from "./args.mjs";
import { launchEdge, killBrowser } from "./browser.mjs";
import { connectCdp } from "./cdp.mjs";
import { delay, waitFor } from "./interactions.mjs";
import { clickText, openNotesEntry, removeFixtureBanner } from "./snippets.mjs";
const options = parseShellArgs(process.argv.slice(2)), out = resolve(options.out);
await mkdir(out, { recursive: true });
const browser = await launchEdge({ ...options, out });
const c = await connectCdp(browser.target.webSocketDebuggerUrl);
const results = [];
const click = async (selector, text) => { await c.evaluate(text ? clickText(selector, text) : `document.querySelector(${JSON.stringify(selector)}).click()`); await delay(100); };
const pill = name => click('button.nav-pill', name);
const tile = name => click('button.sheet-tile', name);
const ready = () => waitFor(c, 'Boolean(document.querySelector(".task-grid"))');
const sheet = () => click('.back-to-settings');
const close = () => click('.sheet-close');
const page = () => c.evaluate('document.querySelector("h1")?.textContent');
const dialog = () => waitFor(c, 'Boolean(document.querySelector(".settings-drafts-dialog[open]"))');
const returnCheck = async name => {
  await waitFor(c, `document.querySelector('button.nav-pill[aria-current="page"]')?.textContent.trim() === ${JSON.stringify(name)}`);
  await waitFor(c, 'document.activeElement?.tagName === "H1"');
  assert.equal(await c.evaluate('Boolean(document.querySelector(".settings-sheet[open]"))'), false);
};
try {
  await ready(); await c.evaluate(removeFixtureBanner());
  for (const name of ['Dictation', 'Meetings']) {
    await pill(name); await ready(); await pill('Settings'); await close();
    await returnCheck(name);
    for (const route of ['button', 'escape', 'backdrop']) {
      await pill('Settings'); await tile('Local text models');
      assert.equal(await page(), 'Local text models');
      await sheet();
      if (route === 'button') await close();
      if (route === 'escape') {
        await c.send('Input.dispatchKeyEvent', { type: 'keyDown', key: 'Escape', code: 'Escape', windowsVirtualKeyCode: 27 });
        await c.send('Input.dispatchKeyEvent', { type: 'keyUp', key: 'Escape', code: 'Escape', windowsVirtualKeyCode: 27 });
      }
      if (route === 'backdrop') {
        await c.send('Input.dispatchMouseEvent', { type: 'mousePressed', x: 2, y: 200, button: 'left', clickCount: 1 });
        await c.send('Input.dispatchMouseEvent', { type: 'mouseReleased', x: 2, y: 200, button: 'left', clickCount: 1 });
      }
      await returnCheck(name); results.push(`${name}: ${route}`);
    }
  }
  await pill('Notes');
  await waitFor(c, 'Boolean(document.querySelector(".notes-entry"))');
  await c.evaluate(openNotesEntry("Roadmap review \u2014 long transcript"));
  await waitFor(c, 'Boolean(document.querySelector(".note-detail h1"))');
  const note = await page();
  await pill('Settings'); await tile('Local text models'); await sheet(); await tile('Shortcuts');
  await sheet(); await close(); await returnCheck('Notes'); assert.equal(await page(), note);
  results.push('selected note through multiple settings screens');
  // Direct task link, then back to the sheet and dismiss.
  await pill('Dictation'); await ready(); await click('button[aria-label^="Transcription:"]'); await click('.chip-menu-footer');
  await waitFor(c, 'Boolean(document.querySelector(".back-to-settings"))');
  await sheet(); await close(); await returnCheck('Dictation'); results.push('direct settings link');
  for (const decision of ['Stay', 'Discard changes', 'Save changes', 'failed save']) {
    await pill('Settings'); await tile('Cleanup & summaries');
    await waitFor(c, 'Boolean(document.querySelector("#summary-model"))');
    await c.evaluate('document.querySelector("#summary-model").focus(); document.querySelector("#summary-model").select()');
    await c.send('Input.insertText', { text: `fixture-${decision}` });
    if (decision === 'failed save') await c.evaluate(`window.__originalInvoke = window.__TAURI_INTERNALS__.invoke; window.__TAURI_INTERNALS__.invoke = (cmd, args) => cmd === 'summary_settings_set' ? Promise.reject(Error('Synthetic save failure')) : window.__originalInvoke(cmd, args)`);
    await sheet(); await close(); await dialog();
    await click('.settings-drafts-dialog button', decision === 'failed save' ? 'Save changes' : decision);
    if (decision === 'Stay') {
      assert.equal(await c.evaluate('document.querySelector("#summary-model").value'), 'fixture-Stay');
      await sheet(); await close(); await dialog(); await click('.settings-drafts-dialog button', 'Discard changes');
    }
    if (decision === 'failed save') {
      await waitFor(c, 'Boolean(document.querySelector(".settings-drafts-dialog .error"))');
      assert.equal(await c.evaluate('document.querySelector("#summary-model").value'), 'fixture-failed save');
      await c.evaluate('window.__TAURI_INTERNALS__.invoke = window.__originalInvoke');
      await click('.settings-drafts-dialog button', 'Stay');
      await sheet(); await close(); await dialog(); await click('.settings-drafts-dialog button', 'Discard changes');
    }
    await returnCheck('Dictation'); results.push(decision);
  }
  await pill('Settings'); await tile('Local text models'); await sheet();
  // A real backdrop click on the title-bar Meetings pill must navigate once.
  const point = await c.evaluate(`(() => {const b=[...document.querySelectorAll('button.nav-pill')].find(b=>b.textContent.trim()==='Meetings').getBoundingClientRect();return {x:b.x+b.width/2,y:b.y+2};})()`);
  for (const type of ['mousePressed','mouseReleased']) await c.send('Input.dispatchMouseEvent', { type, ...point, button:'left', clickCount:1 });
  await returnCheck('Meetings'); await click('[aria-label="Go back"]');
  assert.equal(await page(), 'Local text models');
  results.push('title-bar click-through and Back without duplicate return');
  console.log(JSON.stringify(results, null, 2));
  await writeFile(resolve(out, 'results.json'), JSON.stringify(results, null, 2));
} catch (error) { console.error(await c.evaluate('document.body.innerText.slice(0,2500)')); await c.screenshot(resolve(out, 'failure.png')); throw error; } finally { await c.close(); await killBrowser(browser.child); }
