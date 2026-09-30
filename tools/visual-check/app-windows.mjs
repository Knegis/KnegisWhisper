#!/usr/bin/env node
/** Exercises the running Tauri application's windows through WebView2 CDP. */
import { existsSync } from "node:fs";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { parseAppArgs, appUsage } from "./args.mjs";
import { jsonAt } from "./browser.mjs";
import { connectCdp } from "./cdp.mjs";
import { listClassifiedTargets, pickTarget } from "./targets.mjs";
import { createReport, addEvent, renderMarkdown } from "./report.mjs";
import { windowByTitle } from "./win32.mjs";
import * as snippets from "./snippets.mjs";

const delay = (ms) => new Promise((resolveDelay) => setTimeout(resolveDelay, ms));
const isMain = process.argv[1]
  && resolve(process.argv[1]) === fileURLToPath(import.meta.url);
const title = "KnegisWhisper meeting";

function remoteDebuggingFailure(port, cause) {
  const guidance = [
    `WebView2 debugging port ${port} did not answer.`,
    "Launch the application from apps/desktop after setting",
    `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=${port};`,
    "in PowerShell:",
    `$env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS='--remote-debugging-port=${port}';`,
    "npm run tauri:dev.",
    cause?.message ?? "",
  ].join(" ");
  return new Error(guidance);
}

async function targets(port) {
  try {
    return await jsonAt(port, "/json/list");
  } catch (error) {
    throw remoteDebuggingFailure(port, error);
  }
}

async function targetClient(port, wanted) {
  const target = pickTarget(await targets(port), wanted);
  if (!target?.webSocketDebuggerUrl) {
    throw new Error(
      `No ${wanted} WebView target is currently exposed on port ${port}.`,
    );
  }
  const cdp = await connectCdp(target.webSocketDebuggerUrl);
  await cdp.send("Page.enable");
  return { cdp, target };
}

async function status(cdp) {
  return cdp.evaluate(snippets.invoke("meeting_status"));
}

async function screenshot(cdp, out, name) {
  await cdp.screenshot(resolve(out, name));
  return name;
}

async function loadReport(out, header) {
  const path = resolve(out, "report.json");
  if (!existsSync(path)) return createReport(header);
  return JSON.parse(await readFile(path, "utf8"));
}

async function saveReport(out, report) {
  const markdown = renderMarkdown(report);
  await writeFile(resolve(out, "report.json"), `${JSON.stringify(report, null, 2)}\n`);
  await writeFile(resolve(out, "report.md"), markdown);
  return markdown;
}

async function recordEvent(ctx, type, value) {
  addEvent(ctx.report, { timestamp: new Date().toISOString(), type, value });
}

async function pollMeeting(cdp, wanted, timeoutSeconds, onTick = undefined) {
  const deadline = Date.now() + timeoutSeconds * 1000;
  let last;
  while (Date.now() < deadline) {
    last = await status(cdp);
    await onTick?.(last);
    if (last?.state === wanted) return last;
    await delay(500);
  }
  throw new Error([
    `Timed out after ${timeoutSeconds}s waiting for meeting state ${wanted};`,
    `last state was ${last?.state ?? "unknown"}.`,
  ].join(" "));
}

async function commandStatus(ctx) {
  const all = listClassifiedTargets(await targets(ctx.options.port)).map(
    ({ url, title: targetTitle, type, classification }) => ({
      url,
      title: targetTitle,
      type,
      classification,
    }),
  );
  const { cdp } = await targetClient(ctx.options.port, "main");
  try {
    const meeting = await status(cdp);
    const window = await windowByTitle(title);
    await recordEvent(ctx, "status", {
      targets: all,
      meetingStatus: meeting,
      window,
    });
    return { targets: all, meetingStatus: meeting, window };
  } finally {
    await cdp.close();
  }
}

async function commandStart(ctx) {
  const main = await targetClient(ctx.options.port, "main");
  try {
    // The provider select lives on Settings > Meetings since KNEG-334, not on
    // the Meetings task page that starts the recording, so both routes visit
    // Settings first: the rail's Settings, then the panel's item. The item
    // reads "Meetings" on screen and "Meetings settings" in its text -- a
    // visually hidden suffix, so it is not named the same as the rail's
    // entry -- and `clickText` compares `textContent`, so it is found by the
    // full name. The rail's own "Meetings" is a different selector below. A
    // collapsed panel does not matter: `clickText` calls `click()`, which
    // fires on an element that is not displayed.
    await main.cdp.evaluate(snippets.clickText("nav.rail .rail-item", "Settings"));
    await delay(100);
    await main.cdp.evaluate(snippets.clickText(".list-panel .panel-item", "Meetings settings"));
    await delay(100);
    if (ctx.options.via === "invoke") {
      // Resolve the displayed label to the live option value instead of
      // guessing an IPC provider identifier that a product owner can rename.
      const provider = await main.cdp.evaluate(
        snippets.meetingProviderValue(ctx.options.providerLabel),
      );
      await main.cdp.evaluate(snippets.clickText("nav.rail .rail-item", "Meetings"));
      await delay(100);
      await main.cdp.evaluate(
        snippets.invoke("meeting_start", {
          provider,
          systemAudio: ctx.options.systemAudio,
        }),
      );
    } else {
      await main.cdp.evaluate(
        snippets.selectMeetingProvider(ctx.options.providerLabel),
      );
      await delay(100);
      await main.cdp.evaluate(snippets.clickText("nav.rail .rail-item", "Meetings"));
      await delay(100);
      await main.cdp.evaluate(
        snippets.startMeetingRecording(ctx.options.systemAudio),
      );
    }
    const live = await pollMeeting(main.cdp, "recording", ctx.options.timeout);
    const meeting = await targetClient(ctx.options.port, "meeting");
    try {
      const panel = await meeting.cdp.evaluate(snippets.meetingSnapshot());
      const shot = await screenshot(meeting.cdp, ctx.out, "app-01-recording.png");
      const window = await windowByTitle(title);
      await recordEvent(ctx, "recording", {
        status: live,
        panel,
        window,
        screenshots: [shot],
      });
      return { status: live, panel, window, screenshot: shot };
    } finally {
      await meeting.cdp.close();
    }
  } finally {
    await main.cdp.close();
  }
}

async function commandWatch(ctx, seconds = ctx.options.seconds) {
  const meeting = await targetClient(ctx.options.port, "meeting");
  try {
    const started = Date.now();
    let previous;
    let nextWindow = started;
    let liveChanges = 0;
    const transitions = [];
    const liveValues = [];
    const liveNodeCounts = [];
    const liveExtendsPrevious = {
      extendsPrevious: 0,
      doesNotExtendPrevious: 0,
    };

    while (Date.now() - started < seconds * 1000) {
      const panel = await meeting.cdp.evaluate(snippets.meetingSnapshot());
      const timestamp = new Date().toISOString();
      liveNodeCounts.push({ timestamp, liveNodeCount: panel.liveNodeCount });

      if (previous) {
        for (const key of Object.keys(panel)) {
          if (panel[key] === previous[key]) continue;
          const transition = {
            timestamp,
            field: key,
            from: previous[key],
            to: panel[key],
          };
          transitions.push(transition);
          if (key !== "live") continue;

          liveValues.push(panel.live);
          liveChanges += 1;
          if (typeof previous.live === "string" && typeof panel.live === "string") {
            if (panel.live.startsWith(previous.live)) {
              liveExtendsPrevious.extendsPrevious += 1;
            } else {
              liveExtendsPrevious.doesNotExtendPrevious += 1;
            }
          }
          if (liveChanges <= 5) {
            transition.screenshot = await screenshot(
              meeting.cdp,
              ctx.out,
              `app-02-caption-${liveChanges}.png`,
            );
          }
        }
      }
      previous = panel;

      if (Date.now() >= nextWindow) {
        await recordEvent(ctx, "meeting-window", await windowByTitle(title));
        nextWindow += 2_000;
      }
      await delay(500);
    }

    const observed = {
      seconds,
      transitions,
      liveChanges,
      liveValues,
      liveNodeCounts,
      liveExtendsPrevious,
      finalPanel: previous,
    };
    await recordEvent(ctx, "watch", observed);
    return observed;
  } finally {
    await meeting.cdp.close();
  }
}

async function commandTheme(ctx) {
  const meeting = await targetClient(ctx.options.port, "meeting");
  try {
    await meeting.cdp.send("Emulation.setEmulatedMedia", {
      features: [{ name: "prefers-color-scheme", value: "light" }],
    });
    const light = await screenshot(meeting.cdp, ctx.out, "app-03-light.png");
    await meeting.cdp.send("Emulation.setEmulatedMedia", {
      features: [{ name: "prefers-color-scheme", value: "dark" }],
    });
    const dark = await screenshot(meeting.cdp, ctx.out, "app-03-dark.png");
    await meeting.cdp.send("Emulation.setEmulatedMedia", { features: [] });
    await recordEvent(ctx, "theme", { screenshots: [light, dark] });
    return { light, dark };
  } finally {
    await meeting.cdp.close();
  }
}

async function commandStop(ctx) {
  const main = await targetClient(ctx.options.port, "main");
  let finishingAt = null;
  let hiddenAt = null;
  let nextWindow = 0;
  try {
    if (ctx.options.via === "invoke") {
      await main.cdp.evaluate(snippets.invoke("meeting_stop"));
    } else {
      try {
        const meeting = await targetClient(ctx.options.port, "meeting");
        try {
          await meeting.cdp.evaluate(snippets.clickStop());
        } finally {
          await meeting.cdp.close();
        }
      } catch {
        await main.cdp.evaluate(snippets.clickStop());
      }
    }
    const idle = await pollMeeting(
      main.cdp,
      "idle",
      ctx.options.timeout,
      async (state) => {
        const now = new Date().toISOString();
        if (state.state === "finishing" && !finishingAt) finishingAt = now;
        if (Date.now() >= nextWindow) {
          const window = await windowByTitle(title);
          if (!window.visible && !hiddenAt) hiddenAt = now;
          nextWindow = Date.now() + 500;
        }
      },
    );
    const visibleAfter = await windowByTitle(title);
    if (!visibleAfter.visible && !hiddenAt) hiddenAt = new Date().toISOString();
    const meetingTargetExists = Boolean(
      pickTarget(await targets(ctx.options.port), "meeting"),
    );
    const shot = await screenshot(main.cdp, ctx.out, "app-04-main-after-stop.png");
    const observed = {
      idle,
      finishingAt,
      windowHiddenAt: hiddenAt,
      meetingTargetExistsAfterStop: meetingTargetExists,
      windowAfter: visibleAfter,
      screenshots: [shot],
    };
    await recordEvent(ctx, "stop", observed);
    return observed;
  } finally {
    await main.cdp.close();
  }
}

export async function runAppWindows(options) {
  const out = resolve(options.out);
  await mkdir(out, { recursive: true });
  const report = await loadReport(out, {
    port: options.port,
    timestamp: new Date().toISOString(),
    measurementScope: [
      "running application through WebView2 remote debugging;",
      "route measured on this machine on 2026-09-07",
    ].join(" "),
  });
  const ctx = { options, out, report };

  if (options.command === "status") await commandStatus(ctx);
  else if (options.command === "start") await commandStart(ctx);
  else if (options.command === "watch") await commandWatch(ctx);
  else if (options.command === "theme") await commandTheme(ctx);
  else if (options.command === "stop") await commandStop(ctx);
  else {
    await commandStatus(ctx);
    await commandStart(ctx);
    await commandWatch(ctx, 60);
    await commandTheme(ctx);
    await commandStop(ctx);
  }

  const markdown = await saveReport(out, report);
  process.stdout.write(markdown);
  return report;
}

if (isMain) {
  try {
    const options = parseAppArgs(process.argv.slice(2));
    if (options.help) process.stdout.write(`${appUsage()}\n`);
    else await runAppWindows(options);
  } catch (error) {
    console.error(`app-windows failed: ${error.message}`);
    process.exitCode = 1;
  }
}
