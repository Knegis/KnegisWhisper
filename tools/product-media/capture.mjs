/** Capture real native WebView footage from an already isolated demo profile. */
import { mkdir, writeFile } from "node:fs/promises";
import { resolve, join } from "node:path";
import { connectCdp } from "../visual-check/cdp.mjs";
const port = Number(process.argv[2] ?? 9471);
const out = resolve(process.argv[3] ?? "target/product-media");
const assets = resolve("docs/assets");
await mkdir(out, { recursive: true });
await mkdir(assets, { recursive: true });
const targets = await (await fetch(`http://127.0.0.1:${port}/json/list`)).json();
const target = targets.find(t => t.type === "page" && /:\d+\/$/.test(t.url));
if (!target) throw new Error("Expected the isolated native app's main WebView");
const c = await connectCdp(target.webSocketDebuggerUrl);
const wait = ms => new Promise(r => setTimeout(r, ms));
const nav = label => c.evaluate(`[...document.querySelectorAll('button.nav-pill')].find(b=>b.textContent.trim()===${JSON.stringify(label)}).click();true`);
async function frames(name, seconds, beforeFrame) {
  const folder = join(out, name); await mkdir(folder, { recursive: true });
  for (let i = 0; i < seconds * 5; i++) {
    const started = Date.now();
    if (beforeFrame) await beforeFrame(i);
    const shot = await c.send("Page.captureScreenshot", { format: "jpeg", quality: 88, captureBeyondViewport: false });
    await writeFile(join(folder, `${String(i).padStart(4, "0")}.jpg`), Buffer.from(shot.data, "base64"));
    await wait(Math.max(0, 200 - (Date.now() - started)));
  }
}
try {
  await c.send("Page.enable");
  if (!await c.evaluate("!!window.__TAURI_INTERNALS__")) throw new Error("Browser fixtures are not permitted");
  await nav("Notes"); await wait(800);
  const note = await c.evaluate("window.__TAURI_INTERNALS__.invoke('notes_get',{id:1})");
  const text = await c.evaluate("document.body.innerText") + (note?.content ?? "");
  if (!text.includes("Fictional example for the product demonstration.") || !text.includes("A calmer way to capture ideas")) {
    throw new Error("Refusing capture: expected the fictional demo note, not a personal profile");
  }
  await c.send("Page.reload"); await wait(900);
  await nav("Dictation"); await wait(500);
  await c.screenshot(join(assets, "dictation.png"));
  await frames("dictation", 10);
  await nav("Meetings"); await wait(700);
  await c.screenshot(join(assets, "meetings.png"));
  await frames("meetings", 8);
  await nav("Notes"); await wait(800);
  await c.evaluate("[...document.querySelectorAll('button')].find(b=>b.textContent.trim()==='Notes'&&!b.classList.contains('nav-pill'))?.click();true");
  await wait(400);
  await c.screenshot(join(assets, "notes.png"));
  await frames("notes", 5);
  // Use the real editor input path, without replacing or drawing over app UI.
  await c.evaluate("(()=>{const e=document.querySelector('[contenteditable=true]');e.focus();const s=getSelection();s.selectAllChildren(e.querySelector('p'));s.collapseToEnd();return true})()");
  const words = [" ", "Keep ", "the ", "next ", "step ", "in ", "view."];
  await frames("edit", 3, async i => { if (i < words.length) await c.send("Input.insertText", { text: words[i] }); });
  await c.evaluate("[...document.querySelectorAll('button')].find(b=>b.textContent.trim()==='Replay').click();true");
  await wait(500);
  await c.send("Runtime.evaluate", { expression: "(()=>{const a=document.querySelector('audio');if(a)a.muted=true;document.querySelector('button[aria-label=Play]').click();return true})()", userGesture: true });
  await frames("replay", 4);
  const replay = await c.evaluate("({currentTime:document.querySelector('audio')?.currentTime,error:document.querySelector('audio')?.error?.message})");
  if (!(replay.currentTime > 1) || replay.error) throw new Error("Native example playback did not advance");
  await c.evaluate("document.querySelector('audio')?.pause();true");
  await writeFile(join(out, "capture.json"), JSON.stringify({
    source: "Native Windows debug WebView; fictional note and Windows TTS recording",
    capturedAt: new Date().toISOString(), fps: 5,
    scenes: { dictation: 10, meetings: 8, notes: 5, edit: 3, replay: 4 },
    caveat: "UI demonstration; no microphone transcription or installed-release validation is claimed."
  }, null, 2));
} finally { await c.close(); }
console.log(`Captured native footage in ${out}`);
