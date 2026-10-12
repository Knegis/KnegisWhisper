/** KNEG-421: a failed later purge refreshes the fixture library and can be retried. */
import assert from "node:assert/strict";
import { mkdir, writeFile } from "node:fs/promises";
import { resolve } from "node:path";
import { parseShellArgs } from "./args.mjs";
import { launchEdge, killBrowser } from "./browser.mjs";
import { connectCdp } from "./cdp.mjs";
import { waitFor, pressText } from "./interactions.mjs";
import { clickText, removeFixtureBanner } from "./snippets.mjs";
const options=parseShellArgs(process.argv.slice(2)), out=resolve(options.out);
await mkdir(out,{recursive:true}); let browser,cdp;
const invoke=(command,args={})=>cdp.evaluate(`window.__TAURI_INTERNALS__.invoke(${JSON.stringify(command)},${JSON.stringify(args)})`);
try {
  browser=await launchEdge({...options,out,extraArgs:["--inprivate"]}); cdp=await connectCdp(browser.target.webSocketDebuggerUrl);
  await cdp.send("Page.enable");
  cdp.on("Page.javascriptDialogOpening",event=>{void cdp.send("Page.handleJavaScriptDialog",{accept:event.type==="confirm"&&event.message.startsWith("Delete ")});});
  await waitFor(cdp,"Boolean(document.querySelector('nav.rail'))"); await cdp.evaluate(removeFixtureBanner());
  const first=await invoke("notes_create",{request:{title:"First partial purge fixture",content:""}});
  const second=await invoke("notes_create",{request:{title:"Remaining partial purge fixture",content:""}});
  await invoke("notes_trash",{id:first}); await invoke("notes_trash",{id:second});
  await cdp.evaluate(`(() => { const original=window.__TAURI_INTERNALS__.invoke; window.__purgeCalls=0; window.__TAURI_INTERNALS__.invoke=async(command,args)=>{
    if(command==='notes_empty_trash' && ++window.__purgeCalls===1){await original('notes_delete_forever',{id:${first}});throw Error('Deletion stopped after deleting 1 of the requested notes. A remaining recording could not be deleted. Retry to finish.');}
    return original(command,args);
  }; })()`);
  await cdp.evaluate(clickText("nav.rail .rail-item","Notes"));
  await waitFor(cdp,"Boolean(document.querySelector('.list-panel .notes-entry'))");
  await cdp.evaluate("[...document.querySelectorAll('.list-panel button')].find(e=>e.textContent.trim().startsWith('Trash')).click()");
  await waitFor(cdp,"Boolean(document.querySelector('.trash-space'))");
  await pressText(cdp,".trash-space button","Empty trash");
  await waitFor(cdp,"document.querySelector('.notes-notices')?.textContent.includes('after deleting 1')","partial completion error");
  await waitFor(cdp,"!document.querySelector('.list-panel')?.textContent.includes('First partial purge fixture') && document.querySelector('.list-panel')?.textContent.includes('Remaining partial purge fixture')","refreshed Trash after error");
  const held=await invoke("notes_trash_list");
  assert.equal(held.notes.some(note=>note.id===first),false); assert.equal(held.notes.some(note=>note.id===second),true);
  await cdp.screenshot(`${out}/partial-failure.png`);
  await pressText(cdp,".trash-space button","Empty trash");
  await waitFor(cdp,"document.querySelector('.trash-space')?.textContent.includes('Trash is empty.')","retry empties the remainder");
  assert.equal((await invoke("notes_trash_list")).notes.length,0);
  const result={scope:"invented browser fixtures",checks:["partial completion surfaced","library refreshed after error","retry deletes remainder"]};
  await writeFile(`${out}/report.json`,JSON.stringify(result,null,2)); console.log(JSON.stringify(result));
} finally {if(cdp)await cdp.close();if(browser)await killBrowser(browser.child);}
