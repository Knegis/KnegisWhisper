/** Launches the isolated Edge instance used for fixture-based visual checks. */
import { existsSync } from "node:fs";
import { mkdir } from "node:fs/promises";
import { spawn, execFile } from "node:child_process";
import { promisify } from "node:util";
import { join } from "node:path";

const execFileAsync = promisify(execFile);
const delay = (ms) => new Promise((resolveDelay) => setTimeout(resolveDelay, ms));

export function findEdge(explicit) {
  if (explicit) {
    if (!existsSync(explicit)) throw new Error(`Edge was not found at ${explicit}`);
    return explicit;
  }
  const candidates = [
    process.env["ProgramFiles(x86)"]
      && join(
        process.env["ProgramFiles(x86)"],
        "Microsoft",
        "Edge",
        "Application",
        "msedge.exe",
      ),
    process.env.ProgramFiles
      && join(
        process.env.ProgramFiles,
        "Microsoft",
        "Edge",
        "Application",
        "msedge.exe",
      ),
  ].filter(Boolean);
  const found = candidates.find(existsSync);
  if (!found) throw new Error("Could not find msedge.exe; pass --edge <path>.");
  return found;
}

export async function jsonAt(port, path) {
  const response = await fetch(`http://127.0.0.1:${port}${path}`);
  if (!response.ok) throw new Error(`Debug endpoint returned ${response.status}`);
  return response.json();
}

/**
 * `extraArgs` go on Edge's command line before the URL, for a check that needs
 * the browser itself set up differently: `note-playback.mjs` passes
 * `--mute-audio`, so a run that plays a recording makes no sound.
 */
export async function launchEdge({
  edge,
  port,
  out,
  url,
  width = 1000,
  height = 720,
  extraArgs = [],
  hideScrollbars = true,
}) {
  await mkdir(out, { recursive: true });
  const executable = findEdge(edge);
  const child = spawn(
    executable,
    [
      "--headless=new",
      `--remote-debugging-port=${port}`,
      `--user-data-dir=${join(out, "edge-profile")}`,
      "--no-first-run",
      "--no-default-browser-check",
      "--disable-extensions",
      ...(hideScrollbars ? ["--hide-scrollbars"] : []),
      `--window-size=${width},${height}`,
      ...extraArgs,
      url,
    ],
    { stdio: "ignore", windowsHide: true },
  );
  const deadline = Date.now() + 10_000;
  while (Date.now() < deadline) {
    try {
      const targets = await jsonAt(port, "/json/list");
      const target = targets.find(
        (item) => item.type === "page" && item.url.startsWith(url),
      );
      if (target) {
        return {
          child,
          target,
          version: await jsonAt(port, "/json/version"),
        };
      }
    } catch {
      // The endpoint appears after Edge has started its debugging server.
    }
    await delay(100);
  }
  await killBrowser(child);
  throw new Error("Headless Edge did not expose a page target within 10 seconds.");
}

export async function killBrowser(child) {
  if (!child || child.exitCode !== null) return;
  child.kill();
  if (process.platform === "win32" && child.pid) {
    try {
      await execFileAsync(
        "taskkill",
        ["/PID", String(child.pid), "/T", "/F"],
        { windowsHide: true },
      );
    } catch {
      // The child may already have exited after the normal close.
    }
  }
}
