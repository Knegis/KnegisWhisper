#!/usr/bin/env node
/** Fixture-only enrollment and settings checks. Never opens the real application DB. */
import assert from "node:assert/strict";
import { resolve } from "node:path";
import { launchEdge, killBrowser } from "./browser.mjs";
import { connectCdp } from "./cdp.mjs";
import { clickText, openNotesEntry, openSettingsScreen } from "./snippets.mjs";
const url=process.argv[2] ?? "http://127.0.0.1:5197/?fixtures=1";
const out=resolve(process.argv[3] ?? ".validation/known-voices-remediation");
const browser=await launchEdge({port:9298,out,url,width:1440,height:900});
let cdp;
async function until(expression) {
  const deadline=Date.now()+10000;
  while(Date.now()<deadline) {if(await cdp.evaluate(expression))return;await new Promise(r=>setTimeout(r,50));}
  throw Error(`Timed out: ${expression}`);
}
try {
  cdp=await connectCdp(browser.target.webSocketDebuggerUrl);await cdp.send("Page.enable");
  await until("!!window.kw && !!document.querySelector('.app-titlebar')");
  await cdp.evaluate(openSettingsScreen("Meetings"));
  await until("document.body.innerText.includes('awaiting validation')");
  assert.equal(await cdp.evaluate(`(()=>{const group=[...document.querySelectorAll('h2')].find(x=>x.textContent==='Known voices').parentElement;return group.querySelector('input').disabled && !group.querySelector('input').checked;})()`),true);
  // Deliberately override IPC only inside this isolated fixture browser.
  await cdp.evaluate(`(()=>{window.voiceCalls=[];window.voiceReason='Keep the meeting recording to remember a voice.';window.voiceEnabled=true;window.voiceProfiles=[{id:1,displayName:'Ada',compatible:true}];const invoke=window.__TAURI_INTERNALS__.invoke;window.__TAURI_INTERNALS__.invoke=async(cmd,args,options)=>{if(cmd==='known_voices_status'){window.voiceCalls.push({cmd,args});return {enabled:window.voiceEnabled,ready:true,reason:null,enrollmentReason:args?.noteId ? window.voiceReason : null,voices:window.voiceProfiles};}if(cmd==='known_voices_set_enabled'){window.voiceEnabled=args.enabled;return;}if(cmd==='known_voice_delete'){window.voiceProfiles=[];return;}if(cmd==='known_voice_enroll'){window.voiceCalls.push({cmd,args});return 1;}return invoke(cmd,args,options);};})()`);
  await cdp.evaluate(clickText("button.nav-pill","Notes"));
  await until("!!document.querySelector('.notes-entry-title')");
  const title=await cdp.evaluate("[...document.querySelectorAll('.notes-entry-title')].map(x=>x.textContent).find(x=>x.startsWith('Weekly sync'))");
  await cdp.evaluate(openNotesEntry(title));await until("!!document.querySelector('button.transcript-speaker')");
  const open=async()=>{await cdp.evaluate("document.querySelector('button.transcript-speaker').click()");await until("!!document.querySelector('.transcript-rename input[type=checkbox]')");};
  await open();
  assert.equal(await cdp.evaluate("document.querySelector('.transcript-rename input[type=checkbox]').disabled"),true);
  assert.equal(await cdp.evaluate("document.querySelector('.transcript-rename').textContent.includes('Keep the meeting recording')"),true);
  await cdp.evaluate(clickText(".transcript-rename button","Cancel"));
  await cdp.evaluate("window.voiceReason='This note has no trustworthy voice turn metadata. Record a new meeting to remember this voice.'");
  await open();assert.equal(await cdp.evaluate("document.querySelector('.transcript-rename input[type=checkbox]').disabled"),true);
  await cdp.evaluate(clickText(".transcript-rename button","Cancel"));
  await cdp.evaluate("window.voiceReason=null");await open();
  assert.equal(await cdp.evaluate("document.querySelector('.transcript-rename input[type=checkbox]').disabled"),false);
  await cdp.evaluate("document.querySelector('.transcript-rename input[type=checkbox]').click()");
  await cdp.evaluate(clickText(".transcript-rename button","Save"));
  await until("!document.querySelector('.transcript-rename')");
  const enrollment=await cdp.evaluate("window.voiceCalls.find(x=>x.cmd==='known_voice_enroll')");
  assert.equal(enrollment.args.noteId,101);assert.ok(enrollment.args.speakerId);assert.equal(enrollment.args.replaceId,null);
  await cdp.evaluate(openSettingsScreen("Meetings"));await until("document.body.innerText.includes('Forget voice')");
  await cdp.evaluate(`(()=>{const g=[...document.querySelectorAll('h2')].find(x=>x.textContent==='Known voices').parentElement;g.querySelector('input[type=checkbox]').click();})()`);
  await until("window.voiceEnabled===false");
  assert.equal(await cdp.evaluate("document.body.innerText.includes('Forget voice')"),true);
  await cdp.evaluate(clickText("button","Forget voice"));await until("!document.body.innerText.includes('Forget voice')");
  assert.equal(await cdp.evaluate("document.documentElement.scrollWidth > innerWidth"),false);
  await cdp.evaluate("[...document.querySelectorAll('h2')].find(x=>x.textContent==='Known voices').scrollIntoView({block:'center'})");
  await cdp.screenshot(resolve(out,"settings.png"));
  console.log("Known voices UI passed: release gate, missing recording, missing turns, explicit enrollment, disable retains profile, forget while off, no overflow.");
} finally {await cdp?.close();await killBrowser(browser.child);}
