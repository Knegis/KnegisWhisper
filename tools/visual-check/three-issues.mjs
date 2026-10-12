/** KNEG-455/400/395 browser checks over invented fixtures only. */
import assert from "node:assert/strict";
import { mkdir, writeFile } from "node:fs/promises";
import { resolve } from "node:path";
import { parseShellArgs } from "./args.mjs";
import { launchEdge, killBrowser } from "./browser.mjs";
import { connectCdp } from "./cdp.mjs";
import { delay, reload, waitFor } from "./interactions.mjs";
import { clickText, openNotesEntry, removeFixtureBanner } from "./snippets.mjs";
const options = parseShellArgs(process.argv.slice(2)), out = resolve(options.out);
await mkdir(out, { recursive: true });
let browser, cdp;
const results = [];
const click = async (selector, text) => { await cdp.evaluate(text === null ? `document.querySelector(${JSON.stringify(selector)})?.click()` : clickText(selector, text)); await delay(150); };
const rail = name => click("nav.rail .rail-item", name);
const section = name => click('nav[aria-label="Settings sections"] .panel-item', name === 'Meetings' ? 'Meetings settings' : name);
const input = async (selector, value) => {
  await cdp.evaluate(`(() => { const input=document.querySelector(${JSON.stringify(selector)}); input.scrollIntoView({block:'center'}); input.focus(); input.select(); })()`);
  await cdp.send("Input.insertText", { text: value }); await delay(100);
};
const key = async (name, modifiers = 0) => {
  await cdp.send("Input.dispatchKeyEvent", { type: "keyDown", key: name, modifiers });
  await cdp.send("Input.dispatchKeyEvent", { type: "keyUp", key: name, modifiers }); await delay(120);
};
const viewport = async width => { await cdp.send("Emulation.setDeviceMetricsOverride", { width, height: 900, deviceScaleFactor: 1, mobile: false }); await delay(180); };
const ready = async () => { await waitFor(cdp, "Boolean(document.querySelector('nav.rail'))"); await cdp.evaluate(removeFixtureBanner()); };
const dialog = () => waitFor(cdp, "Boolean(document.querySelector('.settings-drafts-dialog[open]'))");
const noDialog = () => waitFor(cdp, "!document.querySelector('.settings-drafts-dialog')");
const summary = async () => { await rail("Settings"); await waitFor(cdp, "Boolean(document.querySelector('nav[aria-label=\"Settings sections\"]'))"); await section("Cleanup & summaries"); await waitFor(cdp, "Boolean(document.querySelector('#summary-model'))"); };
try {
  browser = await launchEdge({ ...options, out, extraArgs: ["--inprivate", "--mute-audio"] });
  cdp = await connectCdp(browser.target.webSocketDebuggerUrl); await cdp.send("Page.enable");
  for (const scheme of ["light", "dark"]) {
    await cdp.send("Emulation.setEmulatedMedia", { features: [{ name: "prefers-color-scheme", value: scheme }] });
    console.log(scheme, "transcript");
    await viewport(1440); await reload(cdp); await ready(); await rail("Notes");
    await waitFor(cdp, "Boolean(document.querySelector('.notes-entry'))");
    await cdp.evaluate(openNotesEntry("Roadmap review \u2014 long transcript"));
    await waitFor(cdp, "document.querySelectorAll('.transcript-line').length===54");
    await cdp.evaluate("document.querySelector('.transcript-line').focus({preventScroll:true})"); await key("f", 2);
    await waitFor(cdp, "Boolean(document.querySelector('.transcript-find-input:focus'))");
    await input(".transcript-find-input", "review");
    await cdp.evaluate("window.__panel=document.querySelector('.transcript'); document.querySelectorAll('.transcript-section-toggle')[1]?.click(); document.querySelector('.transcript-line').focus({preventScroll:true});"); await key("F2");
    await waitFor(cdp, "Boolean(document.querySelector('.transcript-rename input:focus'))");
    await input(".transcript-rename input", "Unsaved speaker fixture");
    await cdp.evaluate("window.__collapsed=[...document.querySelectorAll('.transcript-section-toggle')].map(e=>e.getAttribute('aria-expanded')); window.__focused=document.activeElement; void 0");
    for (const width of [1000, 1280, 1440]) {
      await viewport(width);
      assert.equal(await cdp.evaluate("window.__panel===document.querySelector('.transcript')"), true, "same transcript instance");
      assert.equal(await cdp.evaluate("document.querySelector('.transcript-find-input').value"), "review");
      assert.equal(await cdp.evaluate("document.querySelector('.transcript-rename input').value"), "Unsaved speaker fixture");
      assert.equal(await cdp.evaluate("document.activeElement===window.__focused"), true, "focus survives resize");
      assert.equal(await cdp.evaluate("JSON.stringify(window.__collapsed)===JSON.stringify([...document.querySelectorAll('.transcript-section-toggle')].map(e=>e.getAttribute('aria-expanded')))"), true);
    }
    await key("Escape");
    await cdp.evaluate("document.querySelector('.transcript-line').click()");
    await waitFor(cdp, "Boolean(document.querySelector('.player-header-clear'))");
    await click(".player-header-clear", null);
    assert.equal(await cdp.evaluate("Boolean(document.querySelector('.player-header-clear'))"), false);
    await click('.note-detail [role="tab"]', "Replay");
    await waitFor(cdp, "Boolean(document.querySelector('.note-full-player .follow-chip'))");
    await cdp.evaluate("document.querySelector('audio').pause(); document.querySelector('audio').src='/missing-recording-fixture.wav'");
    await waitFor(cdp, "Boolean(document.querySelector('audio')?.error)");
    assert.equal(await cdp.evaluate("[...document.querySelectorAll('.recording-play,.recording-skip,input[aria-label=\"Recording position\"]')].every(e=>e.disabled)"), true);
    await cdp.evaluate("document.querySelectorAll('.transcript-line')[5].click()");
    assert.equal(await cdp.evaluate("document.querySelector('.recording-clock').textContent.split(' ').join('')"), "0:00/0:00");
    results.push({ scheme, group: "transcript", checks: ["Ctrl+F", "same instance", "Find/rename/collapse/focus at 1000/1280/1440", "single-line Clear", "wide Replay header", "failed media controls and imperative playback"] });

    console.log(scheme, "drafts");
    await reload(cdp); await ready(); await summary();
    await input("#summary-model", "unsaved-fixture"); await rail("Notes"); await dialog();
    await click(".settings-drafts-dialog button", "Stay"); await noDialog();
    assert.equal(await cdp.evaluate("document.querySelector('#summary-model').value"), "unsaved-fixture");
    await rail("Notes"); await dialog(); await click(".settings-drafts-dialog button", "Discard changes"); await noDialog();
    await summary(); assert.equal(await cdp.evaluate("document.querySelector('#summary-model').value==='unsaved-fixture'"), false);
    await cdp.evaluate(`(() => { const original=window.__TAURI_INTERNALS__.invoke; window.__savedWrites=[]; window.__holdSave=false; window.__failSave=false; window.__TAURI_INTERNALS__.invoke=async(command,args)=>{ if(command==='summary_settings_set'){window.__savedWrites.push(args.settings); if(window.__failSave)throw Error('Synthetic write failure'); if(window.__holdSave)await new Promise(resolve=>window.__releaseSave=resolve); } return original(command,args); }; })()`);
    await input("#summary-model", "first-fixture");
    await cdp.evaluate("window.__holdSave=true"); await click('[data-surface="settings.intelligence.summaries"] button', "Save summary settings");
    await waitFor(cdp, "Boolean(window.__releaseSave)"); await input("#summary-model", "newer-fixture");
    await cdp.evaluate("window.__holdSave=false; window.__releaseSave()");
    await waitFor(cdp, "document.querySelector('[data-surface=\"settings.intelligence.summaries\"] .settings-draft-status')?.textContent==='Unsaved changes'");
    await rail("Notes"); await dialog(); await click(".settings-drafts-dialog button", "Save changes"); await noDialog();
    assert.equal(await cdp.evaluate("window.__savedWrites.at(-1).model"), "newer-fixture");
    await summary(); await input("#summary-model", "failed-fixture");
    await cdp.evaluate("window.__failSave=true"); await rail("Notes"); await dialog(); await click(".settings-drafts-dialog button", "Save changes");
    await waitFor(cdp, "document.querySelector('.settings-drafts-dialog [role=alert]')?.textContent.includes('Synthetic write failure')");
    assert.equal(await cdp.evaluate("document.querySelector('#summary-model').value"), "failed-fixture");
    await click(".settings-drafts-dialog button", "Stay"); await cdp.evaluate("window.__failSave=false");
    await rail("Notes"); await dialog(); await click(".settings-drafts-dialog button", "Discard changes"); await noDialog();
    results.push({ scheme, group: "summary drafts", checks: ["Stay", "Discard", "Save before navigation", "edit during save", "failed write retains draft"] });

    await summary();
    await input("#summary-model", "partial-fixture");
    await input('[data-surface="settings.intelligence.enterprise"] input[type="text"]', "partial-region");
    await cdp.evaluate(`(() => { const original=window.__TAURI_INTERNALS__.invoke; window.__failCompany=true; window.__TAURI_INTERNALS__.invoke=async(command,args)=>{ if(command==='enterprise_config_set'&&window.__failCompany)throw Error('Synthetic company failure'); return original(command,args); }; })()`);
    const priorWrites=await cdp.evaluate("window.__savedWrites.length");
    await rail("Notes"); await dialog(); await click(".settings-drafts-dialog button", "Save changes");
    await waitFor(cdp, "document.querySelector('.settings-drafts-dialog [role=alert]')?.textContent.includes('Synthetic company failure')");
    await waitFor(cdp, "![...document.querySelectorAll('.settings-drafts-dialog li')].some(e=>e.textContent==='Meeting summaries')");
    assert.equal(await cdp.evaluate("window.__savedWrites.length"), priorWrites+1);
    await cdp.evaluate("window.__failCompany=false"); await click(".settings-drafts-dialog button", "Save changes"); await noDialog();
    assert.equal(await cdp.evaluate("window.__savedWrites.length"), priorWrites+1, "retry doesn't repeat the committed group");
    results.push({ scheme, group: "partial settings save", checks: ["screen order", "committed group removed from pending list", "failed group retained", "retry without duplicate writes"] });

    const groups = [
      ["Shortcuts", 'input[aria-label="accelerator for dictation"]', "Control+Shift+F8"],
      ["Vocabulary", '.vocabulary-add input', "Uncommitted fixture term"],
      ["Transcription", 'input[aria-label="Corti tenant"]', "fixture-tenant"],
      ["Cleanup & summaries", '[data-surface="settings.intelligence.enterprise"] input[type="text"]', "fixture-region"],
    ];
    for (const [name, selector, value] of groups) {
      console.log("group", name);
      await rail("Settings"); await waitFor(cdp, "Boolean(document.querySelector('nav[aria-label=\"Settings sections\"]'))"); await section(name);
      await waitFor(cdp, `Boolean(document.querySelector(${JSON.stringify(selector)}))`);
      await input(selector, value); await rail("Notes"); await dialog();
      await click(".settings-drafts-dialog button", "Discard changes"); await noDialog();
      results.push({ scheme, group: name, checks: ["registered persistent draft", "protected navigation", "discard"] });
    }
    await rail("Settings"); await section("Providers"); await waitFor(cdp, "Boolean(document.querySelector('.provider-change,.provider-add'))");
    await cdp.evaluate("document.querySelector('.provider-change,.provider-add').click()"); await waitFor(cdp, "Boolean(document.querySelector('.provider-form input'))");
    await input(".provider-form input", "synthetic-secret-not-real");
    await cdp.evaluate("[...document.querySelectorAll('.provider-change,.provider-add')].find(e=>e.getAttribute('aria-expanded')==='false').click()");
    await dialog(); assert.equal(await cdp.evaluate("document.querySelector('.settings-drafts-dialog').textContent.includes('synthetic-secret-not-real')"), false);
    await click(".settings-drafts-dialog button", "Stay"); assert.equal(await cdp.evaluate("document.querySelector('.provider-form input').value"), "synthetic-secret-not-real");
    await rail("Notes"); await dialog(); await click(".settings-drafts-dialog button", "Discard changes"); await noDialog();
    results.push({ scheme, group: "credential drafts", checks: ["row replacement guard", "Stay preserves key", "dialog contains no secret", "discard"] });

    // Blur-save groups: an in-flight save holds navigation; a failed write retains the draft.
    await rail("Settings"); await section("Meetings");
    await waitFor(cdp, "Boolean(document.querySelector('input[aria-label=\"Meeting language\"]'))");
    await cdp.evaluate(`(() => { const original=window.__TAURI_INTERNALS__.invoke; window.__holdAuto=true; window.__failAuto=false; window.__autoWrites=[]; window.__TAURI_INTERNALS__.invoke=async(command,args)=>{ if(command==='meeting_set_language'){window.__autoWrites.push(args.language); if(window.__failAuto)throw Error('Synthetic language failure'); if(window.__holdAuto)await new Promise(resolve=>window.__releaseAuto=resolve); } return original(command,args); }; })()`);
    await input('input[aria-label="Meeting language"]', "sv"); await key("Tab");
    await waitFor(cdp, "Boolean(window.__releaseAuto)"); await rail("Notes");
    assert.equal(await cdp.evaluate("Boolean(document.querySelector('input[aria-label=\"Meeting language\"]'))"), true);
    assert.equal(await cdp.evaluate("Boolean(document.querySelector('.settings-drafts-dialog'))"), false);
    await cdp.evaluate("window.__holdAuto=false; window.__releaseAuto()");
    await waitFor(cdp, "Boolean(document.querySelector('.notes-entry'))");
    await rail("Settings"); await section("Meetings"); await waitFor(cdp, "Boolean(document.querySelector('input[aria-label=\"Meeting language\"]'))");
    await cdp.evaluate("window.__failAuto=true"); await input('input[aria-label="Meeting language"]', "en"); await key("Tab");
    await waitFor(cdp, "document.querySelector('.settings-draft-status.error')?.textContent.includes('Synthetic language failure')");
    await rail("Notes"); await dialog(); await click(".settings-drafts-dialog button", "Stay");
    assert.equal(await cdp.evaluate("document.querySelector('input[aria-label=\"Meeting language\"]').value"), "en");
    await cdp.evaluate("window.__failAuto=false"); await rail("Notes"); await dialog(); await click(".settings-drafts-dialog button", "Save changes"); await noDialog();
    assert.equal(await cdp.evaluate("window.__autoWrites.at(-1)"), "en");
    results.push({ scheme, group: "meeting language", checks: ["navigation waits for blur save", "no duplicate write", "failed blur save retains draft", "explicit retry"] });

    await rail("Settings"); await section("Calendar"); await waitFor(cdp, "Boolean(document.querySelector('input[aria-label=\"Google Calendar client ID\"]'))");
    await cdp.evaluate(`(() => { const original=window.__TAURI_INTERNALS__.invoke; window.__failCalendar=true; window.__TAURI_INTERNALS__.invoke=async(command,args)=>{ if(command==='calendar_set_client_id'&&window.__failCalendar)throw Error('Synthetic calendar failure'); return original(command,args); }; })()`);
    await input('input[aria-label="Google Calendar client ID"]', "fixture-client"); await key("Tab");
    await waitFor(cdp, "document.querySelector('.settings-draft-status.error')?.textContent.includes('Synthetic calendar failure')");
    await rail("Notes"); await dialog(); await click(".settings-drafts-dialog button", "Stay");
    assert.equal(await cdp.evaluate("document.querySelector('input[aria-label=\"Google Calendar client ID\"]').value"), "fixture-client");
    await cdp.evaluate("window.__failCalendar=false"); await rail("Notes"); await dialog(); await click(".settings-drafts-dialog button", "Discard changes"); await noDialog();
    results.push({ scheme, group: "calendar", checks: ["failed blur save", "Stay preserves client ID", "discard"] });

    await rail("Settings"); await section("Meetings"); await waitFor(cdp, "Boolean(document.querySelector('input[aria-label=\"Meeting language\"]'))");
    await cdp.evaluate(`(() => { const original=window.__TAURI_INTERNALS__.invoke; window.__failRetention=true; window.__failPrompt=false; window.__preferenceWrites=[]; window.__TAURI_INTERNALS__.invoke=async(command,args)=>{ if(command==='meeting_set_delete_recordings'||command==='meeting_set_prompt'){window.__preferenceWrites.push(command); if(command==='meeting_set_delete_recordings'&&window.__failRetention)throw Error('Synthetic retention failure'); if(command==='meeting_set_prompt'&&window.__failPrompt)throw Error('Synthetic prompt failure'); } return original(command,args); }; })()`);
    const togglePreference=label=>cdp.evaluate(`(() => { const label=[...document.querySelectorAll('label')].find(e=>e.textContent.includes(${JSON.stringify(label)})); if(!label)throw Error('Missing preference'); label.querySelector('input').click(); })()`);
    await togglePreference("Delete each recording"); await waitFor(cdp, "document.querySelector('.settings-draft-status.error')?.textContent.includes('Synthetic retention failure')");
    await togglePreference("Show a window when a meeting starts"); await delay(250);
    assert.equal(await cdp.evaluate("[...document.querySelectorAll('.settings-draft-status.error')].some(e=>e.textContent.includes('Synthetic retention failure'))"), true, "another successful setting cannot hide an earlier failure");
    await cdp.evaluate("window.__failPrompt=true"); await togglePreference("Show a window when a meeting starts");
    await waitFor(cdp, "document.querySelector('.settings-draft-status.error')?.textContent.includes('Synthetic prompt failure')");
    await rail("Notes"); await dialog(); await cdp.evaluate("window.__failRetention=false; window.__failPrompt=false");
    await click(".settings-drafts-dialog button", "Save changes"); await noDialog();
    assert.equal(await cdp.evaluate("window.__preferenceWrites.filter(e=>e==='meeting_set_delete_recordings').length"), 2);
    assert.equal(await cdp.evaluate("window.__preferenceWrites.filter(e=>e==='meeting_set_prompt').length"), 3);
    results.push({ scheme, group: "immediate meeting preferences", checks: ["failures remain pending", "successful independent write preserves earlier failure", "retry each failed intent once"] });

    await rail("Settings"); await section("Shortcuts"); await waitFor(cdp, "Boolean(document.querySelector('select[aria-label=\"mode for dictation\"]'))");
    await cdp.evaluate(`(() => { const original=window.__TAURI_INTERNALS__.invoke; window.__failMode=true; window.__modeWrites=0; window.__TAURI_INTERNALS__.invoke=async(command,args)=>{ if(command==='hotkey_set_mode'){window.__modeWrites++; if(window.__failMode)throw Error('Synthetic mode failure'); } return original(command,args); }; const input=document.querySelector('select[aria-label="mode for dictation"]'); input.value=input.value==='hold'?'toggle':'hold'; input.dispatchEvent(new Event('change',{bubbles:true})); })()`);
    await waitFor(cdp, "document.querySelector('.settings-draft-status.error')?.textContent.includes('Synthetic mode failure')");
    await rail("Notes"); await dialog(); await cdp.evaluate("window.__failMode=false"); await click(".settings-drafts-dialog button", "Save changes"); await noDialog();
    assert.equal(await cdp.evaluate("window.__modeWrites"), 2);
    await rail("Settings"); await section("Transcription"); await waitFor(cdp, "Boolean(document.querySelector('[data-surface=\"settings.microphone\"] input:not(:disabled)'))");
    await cdp.evaluate(`(() => { const original=window.__TAURI_INTERNALS__.invoke; window.__micWrites=0; window.__micReadFailure=false; window.__TAURI_INTERNALS__.invoke=async(command,args)=>{ if(command==='overlay_preferences_set'){window.__micWrites++; const result=await original(command,args); window.__micReadFailure=true; return result;} if(command==='overlay_preferences'&&window.__micReadFailure)throw Error('Synthetic refresh failure'); return original(command,args); }; document.querySelector('[data-surface="settings.microphone"] input').click(); })()`);
    await waitFor(cdp, "document.querySelector('[data-surface=\"settings.microphone\"]')?.textContent.includes('The setting changed')");
    await rail("Notes"); await waitFor(cdp, "Boolean(document.querySelector('.notes-entry'))");
    assert.equal(await cdp.evaluate("Boolean(document.querySelector('.settings-drafts-dialog'))"), false);
    assert.equal(await cdp.evaluate("window.__micWrites"), 1);
    results.push({ scheme, group: "immediate shortcuts/microphone", checks: ["failed mode remains pending", "retry exact mode intent", "refresh failure doesn't repeat committed write"] });

    console.log(scheme, "progress");
    await rail("Meetings"); await waitFor(cdp, "Boolean(globalThis.kw)");
    for (const [index, stage] of ["preparing-recording", "finishing-live-transcript", "transcribing-recording", "analysing-speakers", "saving-note"].entries()) {
      await cdp.evaluate(`kw.meetingProgress({revision:${index+1},progress:{operationId:10,kind:'stop',stage:${JSON.stringify(stage)},elapsedSeconds:${index+12}}})`);
      await waitFor(cdp, "Boolean(document.querySelector('.meeting-progress'))");
      assert.equal(await cdp.evaluate("document.querySelector('.meeting-progress .muted').textContent.startsWith('Processing ')") , true);
    }
    const latest=await cdp.evaluate("document.querySelector('.meeting-progress [role=status]').textContent");
    await cdp.evaluate("kw.meetingProgress({revision:2,progress:{operationId:9,kind:'stop',stage:'preparing-recording',elapsedSeconds:1}},false)"); await delay(120);
    assert.equal(await cdp.evaluate("document.querySelector('.meeting-progress [role=status]').textContent"), latest);
    await rail("Notes"); await rail("Meetings"); await waitFor(cdp, "document.querySelector('.meeting-progress [role=status]')?.textContent==='Saving note'");
    await cdp.screenshot(`${out}/${scheme}-main.png`);
    results.push({ scheme, group: "main progress", checks: ["all five stages", "processing clock", "stale events", "page re-entry"] });
  }
  await cdp.close(); await killBrowser(browser.child); cdp = null; browser = null;
  browser = await launchEdge({ ...options, port: options.port + 1, out: `${out}/compact`, url: options.url + "#meeting", width: 392, height: 92, extraArgs: ["--inprivate", "--mute-audio"] });
  cdp = await connectCdp(browser.target.webSocketDebuggerUrl); await cdp.send("Page.enable");
  await cdp.send("Emulation.setDeviceMetricsOverride", { width: 392, height: 92, deviceScaleFactor: 1, mobile: false });
  for (const scheme of ["light", "dark"]) {
    await cdp.send("Emulation.setEmulatedMedia", { features: [{ name: "prefers-color-scheme", value: scheme }] });
    await reload(cdp); await waitFor(cdp, "Boolean(globalThis.kw)"); await cdp.evaluate(removeFixtureBanner());
    await cdp.evaluate("kw.meetingProgress({revision:1,progress:{operationId:20,kind:'recovery',stage:'analysing-speakers',elapsedSeconds:72}})");
    await waitFor(cdp, "document.querySelector('.notify .meeting-progress [role=status]')?.textContent==='Analysing speakers'");
    assert.equal(await cdp.evaluate("document.querySelector('.meeting-progress').getBoundingClientRect().bottom <= innerHeight"), true);
    await cdp.screenshot(`${out}/${scheme}-compact.png`);
    await cdp.evaluate("kw.meetingProgress({revision:2,progress:null})"); await waitFor(cdp, "!document.querySelector('.meeting-progress')");
    results.push({ scheme, group: "compact progress", checks: ["recovery stages", "392x92 layout", "completion clears"] });
  }
  await writeFile(`${out}/report.json`, JSON.stringify({ scope: "headless Edge, invented fixtures only", results }, null, 2));
  console.log(JSON.stringify({ passed: results.length, results }));
} catch (error) { if(cdp) await cdp.screenshot(`${out}/failure.png`).catch(()=>{}); console.error(error); process.exitCode=1; }
finally { if (cdp) await cdp.close(); if (browser) await killBrowser(browser.child); }
