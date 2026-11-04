#!/usr/bin/env node
/** KNEG-422/424 browser regressions. Invented fixtures, no microphone or network ASR. */
import assert from "node:assert/strict";
import { mkdir, writeFile } from "node:fs/promises";
import { resolve } from "node:path";
import {parseShellArgs} from "./args.mjs";
import {launchEdge,killBrowser} from "./browser.mjs";
import {connectCdp} from "./cdp.mjs";
import {waitFor,reload,pressText,delay} from "./interactions.mjs";
import * as snippets from "./snippets.mjs";
const options=parseShellArgs(process.argv.slice(2));
const out=resolve(options.out);
await mkdir(out,{recursive:true});
const browser=await launchEdge({...options,out,width:1440,height:900,extraArgs:["--disable-gpu","--mute-audio"]});
const cdp=await connectCdp(browser.target.webSocketDebuggerUrl);
await cdp.send("Page.enable");
const checks=[];
const input=".note-detail textarea[aria-label=Title]";
async function check(name,expression) {assert.ok(await cdp.evaluate(expression),name);checks.push(name);}
async function fresh() {
 await reload(cdp); await waitFor(cdp,"Boolean(document.querySelector('.app-titlebar')&&window.__TAURI_INTERNALS__)");
 await cdp.evaluate(snippets.removeFixtureBanner()); await cdp.evaluate(snippets.installInvokeStub());
}
async function notes(title="Word timings") {
 await cdp.evaluate(snippets.clickText("button.nav-pill","Notes"));
 await waitFor(cdp,"Boolean(document.querySelector('.notes-entry'))");
 await cdp.evaluate(snippets.openNotesEntry(title));
 await waitFor(cdp,`document.querySelector(${JSON.stringify(input)})?.value===${JSON.stringify(title)}`);
 await waitFor(cdp,"Boolean(document.querySelector('.ProseMirror'))");
}
async function title(value) {
 await cdp.evaluate(`document.querySelector(${JSON.stringify(input)}).focus()`);
 await cdp.send("Input.dispatchKeyEvent",{type:"keyDown",key:"a",code:"KeyA",modifiers:2,windowsVirtualKeyCode:65});
 await cdp.send("Input.dispatchKeyEvent",{type:"keyUp",key:"a",code:"KeyA",modifiers:2,windowsVirtualKeyCode:65});
 await cdp.send("Input.insertText",{text:value});
}
try {
 await fresh();
 await cdp.evaluate(`window.__savedInvoke=window.__TAURI_INTERNALS__.invoke.bind(window.__TAURI_INTERNALS__);window.__deleted=false;window.__retryCount=0;
 window.__TAURI_INTERNALS__.invoke=async(cmd,args,options)=>{
  if(cmd==='note_recording_state'&&args.id===105)return {status:window.__deleted?'deleted':'deleted-update-failed',canRetry:!window.__deleted,error:window.__deleted?null:'Synthetic database commit failure'};
  if(cmd==='note_recording_retry_deletion'&&args.id===105){window.__retryCount++;await new Promise(done=>setTimeout(done,350));window.__deleted=true;return {status:'deleted',canRetry:false,error:null};}
  if(cmd==='note_recording_get'&&args.id===105&&window.__deleted)return null;
  return window.__savedInvoke(cmd,args,options);
 };true`);
 await notes(); await pressText(cdp,".note-tabs button","Replay");
 await waitFor(cdp,"document.body.innerText.includes('attachment still needs cleanup')");
 await check("File-gone state avoids a broken player and offers explicit retry","!document.querySelector('audio')&&[...document.querySelectorAll('button')].some(x=>x.textContent==='Retry deletion')");
 await cdp.evaluate("[...document.querySelectorAll('button')].find(x=>x.textContent==='Retry deletion').click();[...document.querySelectorAll('button')].find(x=>x.textContent.includes('Retry')).click();true");
 await waitFor(cdp,"document.body.innerText.includes('as chosen for this meeting')");
 await check("Repeated retry presses issue one command and show intentional deletion","window.__retryCount===1&&document.body.innerText.includes('another copy')&&!document.querySelector('audio')");
 await cdp.screenshot(resolve(out,"recording-deleted.png"));
 await fresh(); await notes();
 await cdp.evaluate(snippets.stubInvoke({reject:{notes_update:"Synthetic save failure"}}));
 await title("Keep my unsaved draft");
 await waitFor(cdp,"document.body.innerText.includes('Synthetic save failure')");
 await cdp.evaluate("window.__editorBefore=document.querySelector('.ProseMirror');window.__TAURI_INTERNALS__.invoke('notes_trash',{id:105})");
 await waitFor(cdp,"document.body.innerText.includes('moved to Trash elsewhere')");
 await check("External Trash freezes editing and preserves the mounted dirty draft",`document.querySelector(${JSON.stringify(input)}).value==='Keep my unsaved draft'&&document.querySelector(${JSON.stringify(input)}).disabled&&window.__editorBefore===document.querySelector('.ProseMirror')&&document.querySelector('.ProseMirror').getAttribute('contenteditable')==='false'`);
 await check("Selected note polling reads metadata without full library refresh","window.__chipStub.log.filter(x=>x.cmd==='notes_get_summary').length>0");
 await cdp.screenshot(resolve(out,"external-trash.png"));
 await cdp.evaluate(snippets.stubInvoke({reject:{notes_update:null}}));
 await pressText(cdp,".note-detail button","Restore note");
 await waitFor(cdp,"document.body.innerText.includes('The note was restored')");
 await check("Restore requires an explicit retained draft save",`document.querySelector(${JSON.stringify(input)}).disabled&&document.querySelector(${JSON.stringify(input)}).value==='Keep my unsaved draft'`);
 await pressText(cdp,".note-detail button","Save retained draft");
 await waitFor(cdp,`!document.querySelector(${JSON.stringify(input)}).disabled&&document.body.innerText.includes('Saved')`);
 await check("Explicit save writes the retained draft","(async()=> (await window.__TAURI_INTERNALS__.invoke('notes_get',{id:105})).title==='Keep my unsaved draft')()");
 await fresh();
 await cdp.evaluate(`window.__pageOriginal=window.__TAURI_INTERNALS__.invoke.bind(window.__TAURI_INTERNALS__);window.__people=Array.from({length:65},(_,at)=>({email:'person'+at+'@example.com',name:'Person '+at,noteCount:1}));window.__TAURI_INTERNALS__.invoke=(cmd,args,opt)=>cmd==='people_page'?Promise.resolve(window.__people.slice(args.offset,args.offset+args.limit)):window.__pageOriginal(cmd,args,opt);true`);
 await notes(); await waitFor(cdp,"document.querySelectorAll('.notes-person').length===50");
 await check("People starts with a bounded page","document.querySelectorAll('.notes-person').length===50&&document.body.innerText.includes('Show more people')");
 await pressText(cdp,".list-panel button","Show more people");
 await waitFor(cdp,"document.querySelectorAll('.notes-person').length===65");checks.push("Show more fetches the next page without duplicates");
 await fresh(); await notes("Scratch");
 await cdp.evaluate(snippets.stubInvoke({reject:{notes_get_summary:"Synthetic participant refresh failure"}}));
 await pressText(cdp,".note-people-add","Add person");
 await pressText(cdp,".note-people-form input[type=email]",null);
 await cdp.send("Input.insertText",{text:"saved@example.com"});
 await pressText(cdp,".note-people-form button[type=submit]","Add");
 await waitFor(cdp,"document.body.innerText.includes('Saved, but the people could not be refreshed')");
 await check("Committed participant changes remain saved when metadata refresh fails","document.body.innerText.includes('saved@example.com')&&!document.querySelector('.note-people-form')&&document.body.innerText.includes('Retry refresh')");
 await fresh();
 await cdp.evaluate(snippets.clickText("button.nav-pill","Settings"));
 await waitFor(cdp,"Boolean(document.querySelector('dialog.settings-sheet[open]'))");
 await cdp.evaluate(`(async()=>{window.__installOriginal=window.__TAURI_INTERNALS__.invoke.bind(window.__TAURI_INTERNALS__);window.__hiddenModel=(await window.__installOriginal('sherpa_status')).models.find(model=>model.language==='en'&&model.supportedLanguages?.length===1);window.__TAURI_INTERNALS__.invoke=(cmd,args,opt)=>cmd==='speech_installations'?Promise.resolve({revision:1,jobs:[{id:window.__hiddenModel.id,progress:{id:window.__hiddenModel.id,stage:'model',written:10,total:100}}]}):window.__installOriginal(cmd,args,opt);return true;})()`);
 await cdp.evaluate(snippets.clickText(snippets.SHEET_TILE,"Transcription"));
 await waitFor(cdp,"Boolean(document.querySelector('[aria-label=\"Speech model installation progress\"]'))");
 await check("An install begun elsewhere is visible from the backend snapshot","document.querySelector('[aria-label=\"Speech model installation progress\"]').textContent.includes(window.__hiddenModel.name)");
 await cdp.evaluate(snippets.chooseSelect("Show models for","sv"));
 await check("Install status remains visible when filtering model rows","Boolean(document.querySelector('[aria-label=\"Speech model installation progress\"]'))&&!([...document.querySelectorAll('[data-surface=\"settings.transcription.local\"] ul.list > li')].some(row=>row.textContent.includes(window.__hiddenModel.name)))");
 await cdp.screenshot(resolve(out,"install-progress.png"));
 await cdp.evaluate("window.__TAURI_INTERNALS__.invoke('plugin:event|emit',{event:'speech-installations',payload:{revision:3,jobs:[]}})");
 await waitFor(cdp,"!document.querySelector('[aria-label=\"Speech model installation progress\"]')");
 await cdp.evaluate("window.__TAURI_INTERNALS__.invoke('plugin:event|emit',{event:'speech-installations',payload:{revision:2,jobs:[{id:'base',progress:null}]}})");
 await delay(150);
 await check("An older progress event cannot resurrect a finished install","!document.querySelector('[aria-label=\"Speech model installation progress\"]')");
 await cdp.evaluate(snippets.stubInvoke({delay:{dictation_set_local_language:600}}));
 await pressText(cdp,".local-language-options button.language-pill","Detect automatically");
 await pressText(cdp,".local-language-options button.language-pill","Swedish");
 await pressText(cdp,".local-language-options button.language-pill","English");
 await waitFor(cdp,"!document.querySelector('.local-language-picker').getAttribute('aria-busy')");
 await check("Rapid language selections are committed in order and keep the newest choice","window.__chipStub.log.filter(x=>x.cmd==='dictation_set_local_language').slice(-3).map(x=>JSON.stringify(x.args.languages)).join('|')==='[]|[\"sv\"]|[\"sv\",\"en\"]'&&document.querySelectorAll('.local-language-options button.language-pill[aria-pressed=true]').length===2");

 await fresh();
 await cdp.evaluate(snippets.clickText("button.nav-pill","Meetings"));
 await waitFor(cdp,"Boolean(document.querySelector('.control-start'))");
 await cdp.evaluate(`window.__micOriginal=window.__TAURI_INTERNALS__.invoke.bind(window.__TAURI_INTERNALS__);window.__TAURI_INTERNALS__.invoke=(cmd,args,opt)=>cmd==='microphone_check'?new Promise(done=>{window.__finishCheck=done;}):window.__micOriginal(cmd,args,opt);true`);
 await cdp.evaluate(snippets.clickText("button.chip","System default"));
 await waitFor(cdp,"Boolean(document.querySelector('.chip-menu'))");
 await pressText(cdp,".chip-menu button","Test microphone");
 await waitFor(cdp,"typeof window.__finishCheck==='function'");
 await cdp.evaluate("kw.meetingState('recording');true");
 await waitFor(cdp,"Boolean(document.querySelector('.meeting-live'))");
 await cdp.evaluate("kw.meetingState('idle');true");
 await waitFor(cdp,"Boolean(document.querySelector('.control-start'))");
 await cdp.evaluate("window.__finishCheck({device:'Stale microphone',fellBack:null,heard:true,peakLevel:0.7});true");
 await delay(150);
 await check("A microphone result from before an external meeting cannot return afterward","!document.body.innerText.includes('Stale microphone')&&!document.body.innerText.includes('Heard you')");

} finally {
 await writeFile(resolve(out,"report.json"),JSON.stringify({evidence:"Headless Edge with invented fixtures",checks},null,2));
 await cdp.close(); await killBrowser(browser.child);
}
console.log(`Follow-ups: ${checks.length} checks passed`);
