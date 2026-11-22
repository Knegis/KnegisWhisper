// Every screen at one size and theme: sideways overflow, one h1, what still moves, and a
// contact sheet to look at.
//
//   node tools/visual-check/screen-sweep.mjs <url> <out> <width> <height> [light|dark] [no-preference|reduce] [port]
import { launchEdge, killBrowser } from "./browser.mjs";
import { connectCdp } from "./cdp.mjs";
import { removeFixtureBanner } from "./snippets.mjs";
import { join, resolve } from "node:path";
import { mkdirSync, writeFileSync } from "node:fs";

const [url, outArg, widthArg, heightArg, scheme = "light", motion = "no-preference", portArg = "9378"] = process.argv.slice(2);
const out = resolve(outArg);
mkdirSync(out, { recursive: true });
const width = Number(widthArg);
const height = Number(heightArg);
const delay = (ms) => new Promise((d) => setTimeout(d, ms));
const edge = await launchEdge({ port: Number(portArg), out, url, width, height });
const c = await connectCdp(edge.target.webSocketDebuggerUrl);
const lines = [];
const shots = [];

const NAV = (label) => `[...document.querySelectorAll('button.nav-pill')].find(b => b.textContent.trim() === ${JSON.stringify(label)}).click(); true`;
const TILE = (label) => `[...document.querySelectorAll('button.sheet-tile')].find(b => b.querySelector('.sheet-tile-label')?.firstChild?.textContent.trim() === ${JSON.stringify(label)}).click(); true`;
const SCREENS = [
  ["Dictation", [NAV("Dictation")]],
  ["Meetings", [NAV("Meetings")]],
  ["Notes", [NAV("Notes")]],
  ["Import", [NAV("Notes"), `[...document.querySelectorAll('.list-panel button')].find(b => b.textContent.trim() === 'Import').click(); true`, `[...document.querySelectorAll('.notes-context-menu button')].find(b => b.textContent.trim() === 'Audio or video').click(); true`]],
  ["Sheet", [NAV("Settings")]],
  ...["Shortcuts", "Meetings", "Vocabulary", "Transcription", "Cleanup & summaries", "Local text models", "Providers", "Calendar", "Local API", "CLI", "MCP", "Diagnostics"].map((t) => [`S-${t}`, [NAV("Settings"), TILE(t)]]),
];

try {
  await c.send("Page.enable");
  await c.send("Runtime.enable");
  await c.send("Emulation.setEmulatedMedia", { features: [{ name: "prefers-color-scheme", value: scheme }, { name: "prefers-reduced-motion", value: motion }] });
  for (const [name, steps] of SCREENS) {
    await c.send("Emulation.setDeviceMetricsOverride", { width, height, deviceScaleFactor: 1, mobile: false });
    await c.send("Page.navigate", { url });
    await delay(900);
    await c.evaluate(removeFixtureBanner());
    for (const step of steps) {
      await c.evaluate(step);
      await delay(450);
    }
    await delay(500);
    const m = await c.evaluate(`(() => {
      const root = document.documentElement;
      const scroller = document.querySelector('dialog[open]') ?? document.querySelector('.split-main') ?? document.querySelector('.content');
      const box = scroller.getBoundingClientRect();
      const wide = [];
      for (const el of (document.querySelector('dialog[open]') ?? document.querySelector('.content, .split-main, main') ?? document.body).querySelectorAll('*')) {
        if (el.closest('pre, .terminal, svg, [hidden], .tile-weather, .tile-streak-art, .art-weather, .art-streak, .sr-only')) continue;
        const r = el.getBoundingClientRect();
        if (r.width > 0 && r.right > box.right + 1 && wide.length < 4) wide.push((el.className && typeof el.className === 'string' ? el.className.split(' ')[0] : el.tagName) + ':' + Math.round(r.right - box.right));
      }
      const endless = document.getAnimations().filter(a => a.playState === 'running' && a.effect?.getTiming?.().iterations === Infinity).map(a => a.animationName ?? 'x');
      const clipped = [...document.querySelectorAll('button, a, input, select, textarea')].filter(el => { const r = el.getBoundingClientRect(); return r.width > 0 && (el.scrollWidth > el.clientWidth + 2) && !['INPUT', 'TEXTAREA', 'SELECT'].includes(el.tagName); }).slice(0, 3).map(el => el.textContent.trim().slice(0, 24));
      return { sw: root.scrollWidth - innerWidth, inner: scroller.scrollWidth - scroller.clientWidth, h1: document.querySelectorAll('h1').length, scrollH: scroller.scrollHeight, wide, endless: [...new Set(endless)], clipped };
    })()`);
    lines.push(`${name.padEnd(24)} pageX=${m.sw} scrollX=${m.inner} h1=${m.h1} wide=[${m.wide.join(" ")}] clipped=[${m.clipped.join(" | ")}] moving=${m.endless.length ? m.endless.join(",") : "-"}`);
    const tall = Math.min(Math.max(height, m.scrollH + 110), 1500);
    await c.send("Emulation.setDeviceMetricsOverride", { width, height: tall, deviceScaleFactor: 1, mobile: false });
    await delay(350);
    const file = name.replace(/[^\w-]+/g, "_") + ".png";
    await c.screenshot(join(out, file));
    shots.push({ name, file, tall });
  }
  const cells = shots.map((s) => `<figure><img src="${s.file}" width="${Math.round(width * 0.5)}"><figcaption>${s.name}</figcaption></figure>`).join("");
  const sheet = join(out, "sheet.html");
  writeFileSync(sheet, `<!doctype html><meta charset="utf-8"><style>body{margin:0;padding:12px;background:#555;display:flex;flex-wrap:wrap;gap:10px;align-items:flex-start;font:11px sans-serif;color:#eee}figure{margin:0}figcaption{margin-top:3px}</style>${cells}`);
  await c.send("Emulation.setDeviceMetricsOverride", { width: Math.round((width * 0.5 + 10) * 6 + 24), height: 1700, deviceScaleFactor: 1, mobile: false });
  await c.send("Page.navigate", { url: "file:///" + sheet.replace(/\\/g, "/") });
  await delay(900);
  await c.screenshot(join(out, "sheet.png"));
} finally {
  console.log(lines.join("\n"));
  await c.close();
  await killBrowser(edge.child);
}
