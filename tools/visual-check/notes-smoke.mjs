// Native test only: launch with KW_AUTOMATION_PROFILE=<repo>/target/notes-smoke/profile,
// an isolated WEBVIEW2_USER_DATA_FOLDER, Vite 5183 and CDP 9337.
// Creates synthetic notes/audio and a temporary save-failure trigger in that profile.
import assert from 'node:assert/strict';
import { DatabaseSync } from 'node:sqlite';
import { mkdir, writeFile } from 'node:fs/promises';
import { resolve } from 'node:path';
import { connectCdp } from './cdp.mjs';

if (!process.argv.includes('--isolated-profile')) throw Error('Requires --isolated-profile and a disposable native instance.');
const out = resolve('target/notes-smoke');
const db = new DatabaseSync(resolve(out, 'profile/knegiswhisper.db'));
const targets = await (await fetch('http://127.0.0.1:9337/json/list')).json();
const cdp = await connectCdp(targets.find(t => t.url === 'http://127.0.0.1:5183/').webSocketDebuggerUrl);
const invoke = (command, args = {}) => cdp.evaluate(`window.__TAURI_INTERNALS__.invoke(${JSON.stringify(command)},${JSON.stringify(args)})`);
const pause = ms => new Promise(r => setTimeout(r, ms));
const waitFor = async expression => {
  for (let i = 0; i < 80; i++) { if (await cdp.evaluate(expression)) return; await pause(100); }
  throw Error(`Timed out: ${expression}`);
};
const click = async selector => { await cdp.evaluate(`document.querySelector(${JSON.stringify(selector)}).click()`); };
const tab = async value => { await click(`[role="tab"][id$="-${value}"]`); await waitFor(`document.querySelector('[role="tab"][id$="-${value}"]').getAttribute('aria-selected') === 'true'`); };
const setTitle = async value => cdp.evaluate(`(() => { const input = document.querySelector('input[aria-label="Title"]'); Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set.call(input, ${JSON.stringify(value)}); input.dispatchEvent(new Event('input',{bubbles:true})); })()`);
const open = async id => {
  const note = await invoke('notes_get', { id });
  await cdp.evaluate(`Array.from(document.querySelectorAll('.notes-entry')).find(b=>b.querySelector('.notes-entry-title')?.textContent===${JSON.stringify(note.title)})?.click()`);
  await waitFor(`!!document.querySelector('#note-detail-${id} input[aria-label="Title"]:enabled')`);
};
const results = [];
try {
  const name = `Notes smoke ${Date.now()}`;
  const transcript = JSON.stringify([{ text: '# **beslut** [länk](url)', speaker: 'Åsa', timestamp: 0, endTimestamp: 20 }, { text: 'Later statement', timestamp: 20, endTimestamp: 40 }]);
  const id = await invoke('notes_create', { request: { title: name, content: '', noteType: 'meeting', transcript } });
  // Prove that IPC and the database under test name the same synthetic note before writing fixtures.
  assert.equal(db.prepare('SELECT title FROM notes WHERE id=?').get(id)?.title, name);
  const legacy = await invoke('notes_create', { request: { title: `${name} legacy`, content: `# ${name} legacy\n\nExisting body` } });
  const wave = Buffer.alloc(44 + 16000 * 2 * 40);
  wave.write('RIFF'); wave.writeUInt32LE(wave.length - 8, 4); wave.write('WAVEfmt ', 8);
  wave.writeUInt32LE(16, 16); wave.writeUInt16LE(1, 20); wave.writeUInt16LE(1, 22);
  wave.writeUInt32LE(16000, 24); wave.writeUInt32LE(32000, 28); wave.writeUInt16LE(2, 32); wave.writeUInt16LE(16, 34);
  wave.write('data', 36); wave.writeUInt32LE(wave.length - 44, 40);
  await mkdir(resolve(out, 'profile/meetings'), { recursive: true });
  await writeFile(resolve(out, `profile/meetings/synthetic-${id}.wav`), wave);
  db.prepare('INSERT INTO note_recordings(note_id,storage_key,duration_seconds) VALUES (?,?,40)').run(id, `synthetic-${id}.wav`);
  db.prepare('UPDATE notes SET recording_details=? WHERE id=?').run('Synthetic recording. Microphone silent; system audio silent.', id);
  await cdp.evaluate(`Array.from(document.querySelectorAll('button')).find(b=>b.textContent.trim().toLowerCase()==='dictation').click()`);
  await pause(200);
  await cdp.evaluate(`Array.from(document.querySelectorAll('button')).find(b=>b.textContent.trim().toLowerCase()==='notes').click()`);
  await waitFor(`document.querySelectorAll('.notes-entry').length >= 2`);
  await open(legacy);
  assert.equal(await cdp.evaluate(`document.querySelector('.note-prose .ProseMirror').innerText.trim()`), 'Existing body');
  await pause(1000);
  assert.equal((await invoke('notes_get', { id: legacy })).content, `# ${name} legacy\n\nExisting body`);
  await setTitle(`${name} renamed`); await tab('transcript');
  assert.equal((await invoke('notes_get', { id: legacy })).content.trim(), 'Existing body');
  results.push('legacy H1 hidden without open-time writes; explicit title edit persists body independently');
  await open(id);
  assert.deepEqual(await cdp.evaluate(`Array.from(document.querySelectorAll('[role="tab"]')).map(b=>b.textContent)`), ['Notes', 'Transcript', 'Replay']);
  await waitFor(`!!document.querySelector('.start-from-transcript')`); await click('.start-from-transcript');
  await waitFor(`document.querySelector('.note-prose .ProseMirror').innerText.includes('beslut')`);
  assert.ok(await cdp.evaluate(`document.querySelector('.note-prose .ProseMirror').innerText.includes('# **beslut** [länk](url)')`));
  await tab('transcript');
  assert.equal((await invoke('notes_get', { id })).transcript, transcript);
  results.push('literal transcript copy into separate notes; three-tab navigation');
  await tab('replay');
  assert.equal(await cdp.evaluate(`document.querySelector('#note-${id}-panel-replay .transcript') === null`), true);
  await waitFor(`document.querySelector('audio')?.readyState >= 1`);
  assert.equal(await cdp.evaluate(`document.querySelectorAll('audio').length`), 1);
  await click('.recording-player > button');
  await waitFor(`!document.querySelector('audio').paused`);
  await tab('notes');
  assert.equal(await cdp.evaluate(`document.querySelector('audio').paused`), false);
  assert.equal(await cdp.evaluate(`document.querySelector('.note-mini-player').hidden`), false);
  // Waited for rather than read after a fixed 500 ms: playback of a freshly written WAV was
  // measured starting 110-323 ms after Play, and the first cold run failed (KNEG-377). Eight
  // seconds before `waitFor` gives up.
  await waitFor(`document.querySelector('audio').currentTime > 0`);
  await click('.recording-player > button');
  results.push('one native audio instance plays across tabs with mini-player');
  db.exec(`CREATE TRIGGER notes_smoke_fail BEFORE UPDATE ON notes BEGIN SELECT RAISE(FAIL, 'Synthetic save failure'); END`);
  await setTitle(`${name} retry`); await click('[role="tab"][id$="-transcript"]');
  await waitFor(`document.body.innerText.includes('Synthetic save failure')`);
  assert.equal(await cdp.evaluate(`document.querySelector('[role="tab"][aria-selected="true"]').textContent`), 'Notes');
  db.exec('DROP TRIGGER notes_smoke_fail');
  await tab('transcript');
  assert.equal((await invoke('notes_get', { id })).title, `${name} retry`);
  results.push('failed save blocks tab switch; retry retains edits');
  await tab('notes');
  await cdp.screenshot(resolve(out, 'notes.png'));
  await tab('replay'); await cdp.screenshot(resolve(out, 'replay.png'));
  await open(legacy); await tab('replay');
  assert.ok(await cdp.evaluate(`document.body.innerText.includes('This note has no recording attached')`));
  results.push('missing recording and transcript empty states');
  await tab('notes');
  await setTitle('   '); await tab('transcript');
  assert.equal((await invoke('notes_get', { id: legacy })).title, 'Untitled note');
  await cdp.evaluate(`document.querySelector('[role="tab"][aria-selected="true"]').dispatchEvent(new KeyboardEvent('keydown',{key:'End',bubbles:true}))`);
  await waitFor(`document.querySelector('[role="tab"][aria-selected="true"]').textContent==='Replay'`);
  assert.equal(await cdp.evaluate(`document.activeElement.textContent`), 'Replay');
  results.push('blank title normalization and keyboard tab focus');
  await open(id); await tab('notes');
  await cdp.send('Emulation.setDeviceMetricsOverride', { width: 720, height: 520, deviceScaleFactor: 1, mobile: false });
  await pause(200);
  assert.ok(await cdp.evaluate(`document.documentElement.scrollWidth <= window.innerWidth`));
  assert.ok(await cdp.evaluate(`Array.from(document.querySelectorAll('[role="tab"]')).every(e=>{const r=e.getBoundingClientRect(),p=document.querySelector('.notes-main').getBoundingClientRect();return r.left>=p.left && r.right<=p.right;})`));
  await cdp.screenshot(resolve(out, 'notes-narrow.png'));
  await cdp.send('Emulation.clearDeviceMetricsOverride');
  results.push('720px native WebView viewport has no document overflow');
  if (process.argv.includes('--close-window')) {
    db.exec(`CREATE TRIGGER notes_smoke_fail BEFORE UPDATE ON notes BEGIN SELECT RAISE(FAIL, 'Synthetic close failure'); END`);
    await setTitle(`${name} close`);
    await cdp.evaluate(`(async()=>{const w=await import('/node_modules/@tauri-apps/api/window.js');await w.getCurrentWindow().close()})()`);
    await waitFor(`document.body.innerText.includes('Synthetic close failure')`);
    db.exec('DROP TRIGGER notes_smoke_fail');
    await cdp.evaluate(`(()=>{const e=document.querySelector('.note-prose .ProseMirror');e.focus();const r=document.createRange();r.selectNodeContents(e);r.collapse(false);const s=window.getSelection();s.removeAllRanges();s.addRange(r)})()`);
    await cdp.send('Input.insertText', { text: ' Immediate close persistence.' });
    await cdp.evaluate(`(async()=>{const w=await import('/node_modules/@tauri-apps/api/window.js');await w.getCurrentWindow().close()})()`).catch(e => {
      if (!String(e).includes('CDP connection closed')) throw e;
    });
    for (let i = 0; i < 40; i++) {
      const saved = db.prepare('SELECT title,content FROM notes WHERE id=?').get(id);
      if (saved.title === `${name} close` && saved.content.includes('Immediate close persistence.')) break;
      await pause(100);
    }
    const saved = db.prepare('SELECT title,content FROM notes WHERE id=?').get(id);
    assert.equal(saved.title, `${name} close`);
    assert.ok(saved.content.includes('Immediate close persistence.'));
    results.push('failed native close keeps window open; immediate retry close saves title and body');
  }
  await writeFile(resolve(out, 'report.json'), JSON.stringify({ native: true, syntheticAudio: true, id, legacy, results }, null, 2));
  console.log(JSON.stringify({ status: 'passed', results }));
} finally {
  db.exec('DROP TRIGGER IF EXISTS notes_smoke_fail');
  db.close(); await cdp.close();
}
