#!/usr/bin/env node
/**
 * Renders a board of the approved design and the matching screen of the app at
 * one size, and saves the two side by side, so "does it match?" is a look rather
 * than an opinion formed by reading CSS.
 *
 *   node tools/visual-check/design-compare.mjs --url http://127.0.0.1:5197/ \
 *     --boards <folder with *.dc.html> --runtime <dc-runtime.js> \
 *     --out target/design-compare [--only Dictation,Notes] [--sizes 1280x800,1000x720] \
 *     [--port 9350] [--scheme light]
 *
 * The boards live in the design canvas, not in this repository, so their folder
 * and the canvas's runtime are arguments. A board loads `./support.js`; this
 * serves the runtime at that name, which is all the canvas's own page adds.
 *
 * The app side is the Vite dev server's fixtures, so every picture here is
 * "measured in headless Edge against dev fixtures, not in the Tauri window".
 * It uses a spare port: never the running app's 5183.
 */
import { createServer } from "node:http";
import { readFile, mkdir } from "node:fs/promises";
import { join, resolve } from "node:path";

import { killBrowser, launchEdge } from "./browser.mjs";
import { connectCdp } from "./cdp.mjs";
import { removeFixtureBanner } from "./snippets.mjs";

const delay = (ms) => new Promise((done) => setTimeout(done, ms));

function parse(argv) {
  const out = { sizes: "1280x800", port: 9350, scheme: "light", only: null, settle: 1800 };
  for (let i = 0; i < argv.length; i += 2) {
    const key = argv[i].replace(/^--/, "");
    if (!(key in out) && !["url", "boards", "runtime", "out"].includes(key)) throw new Error(`Unknown argument ${argv[i]}`);
    out[key] = argv[i + 1];
  }
  for (const needed of ["url", "boards", "runtime", "out"]) {
    if (!out[needed]) throw new Error(`--${needed} is required`);
  }
  return out;
}

/**
 * How to reach each board's screen in the fixtures: the text of what to press,
 * in order. A step is `[selector, text]`.
 */
const SETTINGS = (tile) => [["button.nav-pill", "Settings"], ["button.sheet-tile", tile]];
const SCREENS = {
  Dictation: [],
  Meetings: [["button", "Meetings"]],
  Notes: [["button", "Notes"], ["first", ".notes-entry[title^='Weekly sync']"]],
  NotesEmpty: [["button", "Notes"]],
  Import: [["button", "Notes"], ["button", "Import audio or video"]],
  Settings: [["button.nav-pill", "Settings"]],
  Shortcuts: SETTINGS("Shortcuts"),
  MeetingSettings: SETTINGS("Meetings"),
  Vocabulary: SETTINGS("Vocabulary"),
  Transcription: SETTINGS("Transcription"),
  Intelligence: SETTINGS("Cleanup & summaries"),
  LocalModels: SETTINGS("Local text models"),
  Providers: SETTINGS("Providers"),
  Calendar: SETTINGS("Calendar"),
  LocalApi: SETTINGS("Local API"),
  Cli: SETTINGS("CLI"),
  Mcp: SETTINGS("MCP"),
  Diagnostics: SETTINGS("Diagnostics"),
};

/** Serves the boards, the runtime under the name a board asks for, and the shots. */
function serve({ boards, runtime, out }) {
  const types = { ".html": "text/html; charset=utf-8", ".js": "text/javascript", ".png": "image/png" };
  const server = createServer(async (request, response) => {
    try {
      const path = decodeURIComponent(new URL(request.url, "http://x").pathname);
      if (path === "/pair") {
        const query = new URL(request.url, "http://x").searchParams;
        response.writeHead(200, { "content-type": types[".html"] });
        response.end(`<!doctype html><meta charset="utf-8"><style>
          body{margin:0;background:#2a2a2e;color:#fff;font:14px Arial}
          .p{display:flex;gap:16px;padding:12px;align-items:flex-start}
          figure{margin:0}figcaption{padding:0 0 8px}img{display:block}
        </style><div class="p">
          <figure><figcaption>Approved design</figcaption><img src="/shots/${query.get("a")}"></figure>
          <figure><figcaption>App</figcaption><img src="/shots/${query.get("b")}"></figure>
        </div>`);
        return;
      }
      let file;
      if (path.endsWith("/support.js")) file = runtime;
      else if (path.startsWith("/shots/")) file = join(out, path.slice("/shots/".length));
      else file = join(boards, path);
      const body = await readFile(file);
      const type = types[file.slice(file.lastIndexOf("."))] ?? "application/octet-stream";
      response.writeHead(200, { "content-type": type });
      response.end(body);
    } catch {
      response.writeHead(404);
      response.end("not found");
    }
  });
  return new Promise((done) => server.listen(0, "127.0.0.1", () => done({ server, port: server.address().port })));
}

async function go(client, url, settle) {
  await client.send("Page.navigate", { url });
  await delay(settle);
}

async function press(client, [selector, text]) {
  if (selector === "first") {
    await client.evaluate(`(() => { const el = document.querySelector(${JSON.stringify(text)}); if (!el) throw new Error('no ' + ${JSON.stringify(text)}); el.click(); })()`);
  } else {
    await client.evaluate(`(() => {
      const wanted = ${JSON.stringify(text)}.toLowerCase();
      const hit = [...document.querySelectorAll(${JSON.stringify(selector)})]
        .find((item) => item.textContent.trim().toLowerCase() === wanted
          || item.querySelector?.('.sheet-tile-label')?.firstChild?.textContent.trim().toLowerCase() === wanted);
      if (!hit) throw new Error('Could not find ' + wanted);
      hit.click();
    })()`);
  }
  await delay(500);
}

const options = parse(process.argv.slice(2));
const out = resolve(options.out);
await mkdir(out, { recursive: true });
const { server, port } = await serve({ boards: resolve(options.boards), runtime: resolve(options.runtime), out });
const edge = await launchEdge({ port: Number(options.port), out, url: options.url, width: 1280, height: 800 });
const client = await connectCdp(edge.target.webSocketDebuggerUrl);
const failures = [];
try {
  await client.send("Page.enable");
  await client.send("Runtime.enable");
  await client.send("Emulation.setEmulatedMedia", { features: [{ name: "prefers-color-scheme", value: options.scheme }] });
  const names = options.only ? options.only.split(",") : Object.keys(SCREENS);
  const sizes = options.sizes.split(",").map((size) => size.split("x").map(Number));
  for (const name of names) {
    for (const [width, height] of sizes) {
      await client.send("Emulation.setDeviceMetricsOverride", { width, height, deviceScaleFactor: 1, mobile: false });
      const stem = `${name}-${width}x${height}`;
      await go(client, `http://127.0.0.1:${port}/${name}.dc.html`, Number(options.settle));
      await client.screenshot(join(out, `${stem}-board.png`));
      try {
        await go(client, options.url, Number(options.settle));
        await client.evaluate(removeFixtureBanner());
        for (const step of SCREENS[name] ?? []) await press(client, step);
        await delay(600);
        await client.screenshot(join(out, `${stem}-app.png`));
      } catch (error) {
        failures.push(`${stem}: ${error.message}`);
        continue;
      }
      await client.send("Emulation.setDeviceMetricsOverride", { width: width * 2 + 40, height: height + 50, deviceScaleFactor: 1, mobile: false });
      await go(client, `http://127.0.0.1:${port}/pair?a=${stem}-board.png&b=${stem}-app.png`, 400);
      await client.screenshot(join(out, `${stem}.png`));
      console.log(`${stem}.png`);
    }
  }
} finally {
  await client.close();
  await killBrowser(edge.child);
  server.close();
}
if (failures.length > 0) {
  console.error(failures.join("\n"));
  process.exitCode = 1;
}
