// Every floating window state at its real window size, on a contact sheet: the overlay,
// the floating microphone, the meeting offer, finishing card and recording panel, the
// update card and the languages window.
//
//   node tools/visual-check/floating-windows.mjs <url> <out> [light|dark]
//
// Headless Edge against dev fixtures, so this says nothing about transparency, focus or
// dragging in the real windows. Capture exclusion is stubbed on to show transcript rows.
// The real microphone window is also clipped to a rectangle a fixture can't apply, so
// what is drawn here is compared with that rectangle in persistent_overlay.rs.
import { launchEdge, killBrowser } from "./browser.mjs";
import { connectCdp } from "./cdp.mjs";
import { removeFixtureBanner } from "./snippets.mjs";
import { join, resolve } from "node:path";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";

const [url, outArg, scheme = "light"] = process.argv.slice(2);
const out = resolve(outArg);
mkdirSync(out, { recursive: true });
const delay = (ms) => new Promise((d) => setTimeout(d, ms));
const edge = await launchEdge({ port: 9374, out, url, width: 1000, height: 800 });
const c = await connectCdp(edge.target.webSocketDebuggerUrl);
const shots = [];
const results = [];
const check = (name, ok, detail = "") => results.push(`${ok ? "PASS" : "FAIL"}  ${name}${detail ? "  " + detail : ""}`);

async function open(hash, width, height) {
  await c.send("Emulation.setDeviceMetricsOverride", { width, height, deviceScaleFactor: 2, mobile: false });
  await c.send("Page.navigate", { url: "about:blank" });
  await delay(100);
  await c.send("Page.navigate", { url: url + hash });
  await delay(1200);
  await c.evaluate(removeFixtureBanner());
  await c.send("Emulation.setDefaultBackgroundColorOverride", { color: scheme === "dark" ? { r: 40, g: 42, b: 48, a: 1 } : { r: 236, g: 234, b: 240, a: 1 } });
  await c.evaluate(`document.documentElement.style.background = 'transparent'; document.body.style.background = 'transparent'; true`);
}
async function shot(name, width, height) {
  await delay(450);
  await c.screenshot(join(out, `${name}.png`));
  shots.push({ name, width, height });
}
const overlayRust = readFileSync(new URL("../../apps/desktop/src-tauri/src/persistent_overlay.rs", import.meta.url), "utf8");
const nativeRect = (name) => overlayRust.match(new RegExp(`const ${name}: \\[f64; 4\\] = \\[([^\\]]+)\\]`))[1].split(",").map(Number);
const pillRect = nativeRect("PILL_RECT");
const inside = ([l, t, r, b], [ol, ot, or, ob]) => l >= ol && t >= ot && r <= or && b <= ob;
async function checkInsideNativeRegion(state, vertical) {
  const clip = vertical ? [pillRect[1], pillRect[0], pillRect[3], pillRect[2]] : pillRect;
  const box = (s) => `(() => { const e = document.querySelector('${s}'); if (!e) return null; const b = e.getBoundingClientRect(); return [b.left, b.top, b.right, b.bottom]; })()`;
  const pill = await c.evaluate(box(".floating-microphone"));
  const arrow = await c.evaluate(box(".microphone-language-toggle"));
  check(`the ${state} microphone is drawn inside the window's clip`, inside(pill, clip), JSON.stringify({ pill, clip }));
  check(`the ${state} arrow is inside the pill`, inside(arrow, pill), JSON.stringify({ arrow, pill }));
  for (const content of [".microphone-glyph", ".microphone-level", ".microphone-dots"]) {
    const b = await c.evaluate(box(content));
    if (b) check(`the ${state} ${content} clears the arrow`, b[2] <= arrow[0] || b[3] <= arrow[1], JSON.stringify({ content: b, arrow }));
  }
}
const emit = (event, payload) => c.evaluate(`import('/node_modules/@tauri-apps/api/event.js').then(m => m.emit(${JSON.stringify(event)}, ${JSON.stringify(payload)})).then(() => true)`);

try {
  await c.send("Page.enable");
  await c.send("Runtime.enable");
  await c.send("Emulation.setEmulatedMedia", { features: [{ name: "prefers-color-scheme", value: scheme }] });

  // The dictation overlay.
  await open("#overlay", 280, 96);
  await shot("overlay-ready", 280, 96);
  await c.evaluate(`kw.dictationState('recording'); kw.levels(); true`);
  await shot("overlay-recording", 280, 96);
  await c.evaluate(`kw.dictationState('transcribing'); true`);
  await shot("overlay-transcribing", 280, 96);
  await c.evaluate(`kw.dictationState('inserting'); true`);
  await shot("overlay-inserting", 280, 96);
  const overlayBox = await c.evaluate(`(() => { const o = document.querySelector('.overlay'); return { w: o.scrollWidth, cw: o.clientWidth, r: getComputedStyle(o).borderRadius }; })()`);
  check("overlay holds its content", overlayBox.w <= overlayBox.cw, JSON.stringify(overlayBox));

  // The floating microphone.
  await open("#overlay", 96, 96);
  await c.evaluate(`kw.dictationState('idle'); kw.overlayFloating(); true`);
  await shot("mic-rest", 96, 96);
  await c.send("Input.dispatchMouseEvent", { type: "mouseMoved", x: 48, y: 48 });
  await shot("mic-hover", 96, 96);
  await c.send("Input.dispatchMouseEvent", { type: "mouseMoved", x: 70, y: 48 });
  const reach = await c.evaluate(`(() => { const t = document.querySelector('.microphone-language-toggle'); const r = t.getBoundingClientRect(); const s = getComputedStyle(t); return { w: r.width, h: r.height, pe: s.pointerEvents, op: s.opacity, under: document.elementFromPoint(70, 48)?.closest('button')?.className ?? null }; })()`);
  await checkInsideNativeRegion("hovered", false);
  check("the language toggle stays reachable from the pill, within the page", reach.pe === "auto" && String(reach.under).includes("microphone-language-toggle"), JSON.stringify(reach));
  await c.send("Input.dispatchMouseEvent", { type: "mouseMoved", x: 2, y: 2 });
  await c.evaluate(`kw.dictationState('recording'); kw.levels(); true`);
  await shot("mic-recording", 96, 96);
  await checkInsideNativeRegion("recording", false);
  await c.evaluate(`kw.dictationState('transcribing'); true`);
  await shot("mic-busy", 96, 96);
  await checkInsideNativeRegion("busy", false);
  await c.evaluate(`kw.dictationState('idle'); kw.overlayFloating(true, true); true`);
  await c.send("Input.dispatchMouseEvent", { type: "mouseMoved", x: 48, y: 48 });
  await shot("mic-vertical-hover", 96, 96);
  await checkInsideNativeRegion("upright hovered", true);
  await c.send("Input.dispatchMouseEvent", { type: "mouseMoved", x: 2, y: 2 });
  await c.evaluate(`kw.dictationState('recording'); kw.levels(); true`);
  await shot("mic-vertical-recording", 96, 96);
  await checkInsideNativeRegion("upright recording", true);
  await open("#overlay", 280, 360);
  await c.evaluate(`kw.dictationState('idle'); kw.overlayFloating(); true`);
  await delay(400);
  await c.evaluate(`kw.dictationFailed(); true`);
  await shot("mic-error", 280, 360);

  // The meeting window.
  await open("#meeting", 392, 102);
  await c.evaluate(`kw.offer(); true`);
  await shot("offer-call", 392, 102);
  const fit = await c.evaluate(`(() => { const n = document.querySelector('.notify'); const a = n.querySelector('.notify-actions').getBoundingClientRect(); return { sh: n.scrollHeight, ch: n.clientHeight, actionsBottom: a.bottom, win: innerHeight }; })()`);
  check("the offer's buttons fit its window", fit.sh <= fit.ch && fit.actionsBottom <= fit.win, JSON.stringify(fit));
  await emit("meeting-offer", { provider: "meeting-detect:teams", name: "Microsoft Teams", evidence: "ms-teams.exe", signal: "microphone" });
  await shot("offer-microphone", 392, 102);
  await open("#meeting", 392, 102);
  await c.evaluate(`kw.meetingState('finishing'); kw.meetingProgress({ revision: 9, progress: { operationId: 'a', kind: 'finishing', stage: 'transcribing', elapsedSeconds: 43 } }); true`).catch(() => null);
  await shot("finishing", 392, 102);
  await open("#meeting", 440, 560);
  await c.evaluate(`(() => { const t = window.__TAURI_INTERNALS__; const orig = t.invoke.bind(t); t.invoke = async (cmd, args, opts) => { const r = await orig(cmd, args, opts); return cmd.startsWith('meeting_status') || cmd === 'get_meeting_status' ? { ...r, notificationExcluded: { state: 'active', evidence: 'stub' } } : r; }; return true; })()`);
  await c.evaluate(`kw.meetingState('recording'); true`);
  await delay(700);
  await shot("panel-notes", 440, 560);
  await c.evaluate(`[...document.querySelectorAll('.notify-tabs [role=tab]')].find(b => b.textContent === 'Transcript').click(); kw.transcript("Let's start with the release.", true); kw.transcript("The localisation pass needs two more days.", true, { speaker: 'Speaker 2' }); kw.transcript("Then Thursday is the earliest", false, { speaker: 'Speaker 3' }); true`);
  await delay(500);
  await shot("panel-transcript", 440, 560);

  // The update card.
  await open("#update", 392, 104);
  await emit("update-check-result", { state: "update-available", current: "0.2.0", latest: "0.2.1", url: "https://github.com/Knegis/KnegisWhisper/releases/tag/v0.2.1" });
  await shot("update", 392, 104);
  const updateFit = await c.evaluate(`(() => { const n = document.querySelector('.notify'); return { sh: n.scrollHeight, ch: n.clientHeight }; })()`);
  check("the update card fits its window", updateFit.sh <= updateFit.ch, JSON.stringify(updateFit));
  const updateButtons = await c.evaluate(`(() => {
    const n = document.querySelector('.notify');
    const card = n.getBoundingClientRect();
    return [...n.querySelectorAll('button')].map(button => {
      const b = button.getBoundingClientRect();
      const style = getComputedStyle(button);
      return { text: button.textContent.trim(), visible: b.width > 0 && b.height > 0 && style.visibility === 'visible' && style.display !== 'none' && Number(style.opacity) > 0,
        inside: b.left >= card.left && b.top >= card.top && b.right <= card.right && b.bottom <= card.bottom };
    });
  })()`);
  check("both update buttons are visible inside the card", updateButtons.length === 2 && ["Open release page", "Dismiss"].every(text => updateButtons.some(b => b.text === text && b.visible && b.inside)), JSON.stringify(updateButtons));

  // The languages window.
  await open("#dictation-languages", 280, 272);
  await shot("languages", 280, 272);
  const langFit = await c.evaluate(`(() => { const m = document.querySelector('.microphone-language-menu'); const d = m.querySelector('.language-done').getBoundingClientRect(); return { sh: m.scrollHeight, ch: m.clientHeight, doneBottom: d.bottom, win: innerHeight }; })()`);
  check("Done fits the languages window", langFit.sh <= langFit.ch && langFit.doneBottom <= langFit.win, JSON.stringify(langFit));

  // The sheet.
  const cells = shots.map((s) => `<figure><img src="${s.name}.png" width="${s.width}" height="${s.height}"><figcaption>${s.name}</figcaption></figure>`).join("");
  const sheet = join(out, "sheet.html");
  writeFileSync(sheet, `<!doctype html><meta charset="utf-8"><style>body{margin:0;padding:24px;background:${scheme === "dark" ? "#282a30" : "#eceaf0"};display:flex;flex-wrap:wrap;gap:22px;align-items:flex-start;font:12px sans-serif;color:${scheme === "dark" ? "#ccc" : "#444"}}figure{margin:0}figcaption{margin-top:6px}</style>${cells}`);
  await c.send("Emulation.setDeviceMetricsOverride", { width: 1500, height: 1300, deviceScaleFactor: 1, mobile: false });
  await c.send("Page.navigate", { url: "file:///" + sheet.replace(/\\/g, "/") });
  await delay(900);
  await c.screenshot(join(out, "sheet.png"));
} finally {
  console.log(results.join("\n"));
  await c.close();
  await killBrowser(edge.child);
}
