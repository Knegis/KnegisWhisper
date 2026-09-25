// Run only against a desktop started with a disposable KW_AUTOMATION_PROFILE.
// This test creates synthetic notes and changes summary settings in that profile.
// Start Vite on 5183 and WebView2 CDP on 9337, then run:
// node tools/visual-check/summary-smoke.mjs --isolated-profile
if (!process.argv.includes('--isolated-profile')) throw Error('Use --isolated-profile only after starting the disposable test instance.');
﻿import {createServer} from 'node:http';
import assert from 'node:assert/strict';
import {writeFile} from 'node:fs/promises';
import {connectCdp} from './cdp.mjs';
const targets=await(await fetch('http://127.0.0.1:9337/json/list')).json();
const cdp=await connectCdp(targets.find(t=>t.url==='http://127.0.0.1:5183/').webSocketDebuggerUrl);
const title = `Summary verification ${Date.now()} ? synthetic meeting`;
let requests=0;
const server=createServer(async(req,res)=>{
 let raw='';for await(const chunk of req)raw+=chunk;
 const body=JSON.parse(raw);requests++;
 const source=body.messages.at(-1).content;
 const refs=[...source.matchAll(/\[(s\d+)\]|"(s\d+)"/g)].map(m=>m[1]??m[2]);
 const sources=[...new Set(refs)].slice(0,2);
 if(body.model==='delayed')await new Promise(r=>setTimeout(r,3000));
 const summary={language:'sv',overview:[{text:'Teamet diskuterade lanseringen.',sources}],decisions:[{text:'Lanseringen flyttas till oktober.',sources}],actions:[{text:'Åsa skickar rapporten på fredag.',sources}],questions:[]};
 const reply=body.model==='bad'?'not JSON':JSON.stringify(summary);
 res.setHeader('content-type','application/json');res.end(JSON.stringify({choices:[{finish_reason:'stop',message:{content:reply}}]}));
});
await new Promise(r=>server.listen(18881,'127.0.0.1',r));
const invoke=(command,args={})=>cdp.evaluate(`window.__TAURI_INTERNALS__.invoke(${JSON.stringify(command)},${JSON.stringify(args)})`);
const config={enabled:true,provider:'self-hosted',model:'fixture',selfHostedUrl:'http://127.0.0.1:18881/v1',enterprise:false,language:'match',contextBudget:8192};
const wait=async(id,wanted)=>{const end=Date.now()+30000;while(Date.now()<end){const s=await invoke('note_summary_get',{id});if(wanted.includes(s.status))return s;await new Promise(r=>setTimeout(r,200));}throw Error('Summary timeout');};
try{
 await invoke('summary_settings_set',{settings:config});
 const segments=Array.from({length:36},(_,i)=>({text:('Vi diskuterar projektets leverans. ').repeat(9)+(i===0?'Vi beslutar att flytta lanseringen till oktober. Åsa skickar rapporten på fredag.':''),timestamp:i*60,endTimestamp:(i+1)*60}));
 const id=await invoke('notes_create',{request:{title,content:'Original notes stay here.',noteType:'meeting',transcript:JSON.stringify(segments)}});
 const generated=await wait(id,['succeeded','failed']);assert.equal(generated.status,'succeeded',generated.error);assert.ok(requests>1);assert.equal(generated.completed,generated.total);assert.ok(generated.content.includes('#summary-source-s0'));
 const updated=await invoke('note_summary_update',{id,revision:generated.revision,content:generated.content+'\nUser-edited closing remark.\n'});assert.equal(updated.edited,true);
 await invoke('summary_settings_set',{settings:{...config,model:'bad'}});
 await invoke('note_summary_generate',{id,revision:updated.revision,replaceEdited:true,retry:false});
 const failed=await wait(id,['failed']);assert.equal(failed.content,updated.content);
 await invoke('summary_settings_set',{settings:{...config,model:'delayed'}});
 await invoke('note_summary_generate',{id,revision:failed.revision,replaceEdited:true,retry:false});
 await wait(id,['processing']);await invoke('note_summary_cancel',{id});
 assert.equal((await invoke('note_summary_get',{id})).status,'cancelled');
 await invoke('summary_settings_set',{settings:{...config,enabled:false}});
 await cdp.evaluate(`Array.from(document.querySelectorAll('button')).find(b=>b.textContent.trim().toLowerCase()==='dictation').click()`);
 await new Promise(r=>setTimeout(r,300));
 await cdp.evaluate(`Array.from(document.querySelectorAll('button')).find(b=>b.textContent.trim().toLowerCase()==='notes').click()`);
 await new Promise(r=>setTimeout(r,1200));
 await cdp.evaluate(`Array.from(document.querySelectorAll('.notes-entry')).find(b=>b.querySelector('.notes-entry-title')?.textContent===${JSON.stringify(title)}).click()`);
 await new Promise(r=>setTimeout(r,900));
 await cdp.evaluate(`Array.from(document.querySelectorAll('[role="tab"]')).find(b=>b.textContent==='Notes').click()`);
 await new Promise(r=>setTimeout(r,900));
 const text=await cdp.evaluate('document.body.innerText');assert.ok(text.toLowerCase().includes('ai summary'));assert.ok(text.includes('User-edited closing remark.'));
 const source = await cdp.evaluate(`document.querySelector('.summary-pane a[href^="#summary-source-"]')?.textContent`);
 assert.equal(source, '00:00:00');
 await cdp.evaluate(`document.querySelector('.summary-pane a[href^="#summary-source-"]').click()`);
 await new Promise(r=>setTimeout(r,200));
 assert.equal(await cdp.evaluate(`document.querySelector('[role="tab"][aria-selected="true"]').textContent`), 'Transcript');
 assert.ok(await cdp.evaluate(`document.querySelector('[role="tabpanel"]:not([hidden]) .transcript-row-current') !== null`));
 await cdp.screenshot('target/summary-smoke/summary.png');
 await writeFile('target/summary-smoke/report.json',JSON.stringify({native:true,syntheticProvider:true,id,requests,tests:['automatic meeting enqueue','hierarchical long transcript','source references','editable separate summary','failed regeneration preserves edits','cancel','native Notes summary section'],text},null,2));
 console.log(JSON.stringify({id,requests,status:'passed'}));
}finally{server.close();await cdp.close();}
