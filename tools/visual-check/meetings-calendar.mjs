#!/usr/bin/env node
/** Calendar cache, Start's IPC choices and both recording surfaces, against
 * browser fixtures only. No browser runs as part of the worker's syntax check.
 * The orchestrator supplies a running dev server and executes this probe. */
import { mkdir, writeFile } from "node:fs/promises";
import { resolve } from "node:path";
import { parseShellArgs, shellUsage } from "./args.mjs";
import { launchEdge, killBrowser } from "./browser.mjs";
import { connectCdp } from "./cdp.mjs";
import { createReport, addCheck, failedChecks, renderMarkdown } from "./report.mjs";
import * as snippets from "./snippets.mjs";
import { isEntryPoint } from "../ci/entry.mjs";

const delay = ms => new Promise(done => setTimeout(done, ms));
const START = "Boolean(document.querySelector('.control-start:not(:disabled)'))";
const OVERLAP = "Several meetings overlap, so this recording isn't linked.";
const LINKED = "Linked to Platform standup";

async function waitFor(cdp, expression, description, timeout = 12_000) {
  const deadline = Date.now() + timeout;
  while (Date.now() < deadline) {
    try { if (await cdp.evaluate(expression)) return; } catch { /* document changing */ }
    await delay(50);
  }
  throw new Error(`Timed out waiting for ${description}`);
}

/** Local wall times survive the ISO conversion in exactly the same way as
 * production events. Only running events need to have time-relative labels. */
function fixtures() {
  const now = Date.now();
  const event = (id, title, provider = "google", meetingLike = true) => ({
    id, title, provider, meetingLike, calendarId: `${provider}-primary`,
    start: new Date(now - 10 * 60_000).toISOString(), end: new Date(now + 40 * 60_000).toISOString(),
  });
  const a = event("a", "Platform standup");
  const b = event("b", "Project review", "microsoft");
  const solo = event("solo", "Focus time", "google", false);
  const provider = (name, patch = {}) => ({
    provider: name, connected: true, selectedCalendars: 1,
    lastAttemptAt: new Date(now - 180_000).toISOString(), lastSuccessAt: new Date(now - 180_000).toISOString(),
    lastOutcome: { state: "completed", eventsUpserted: 3, eventsRemoved: 0, calendarErrors: [] }, ...patch,
  });
  const upcoming = { events: [a, b, { ...a, id: "untitled", title: null }], hasMore: true, providers: [provider("google"), provider("microsoft")] };
  const candidates = { events: [a], auto: a.id };
  const linked = { eventId: a.id, provider: a.provider, title: a.title, titleFromEvent: true, omittedParticipants: 0 };
  const plan = { title: a.title, folderId: null, deleteRecording: false, liveProviderId: "local", calendar: linked };
  const status = { state: "recording", sessionId: "visual-calendar-session", plan, calendarNotice: null, calendarChoices: [a, b], elapsedSeconds: 180, sources: { microphone: true, microphoneName: "Default microphone", systemAudio: false, systemAudioReason: null }, notificationExcluded: { state: "active", evidence: "stub" } };
  return { a, b, solo, upcoming, candidates, provider, status };
}

/** The normal invoke stub logs Start arguments. This small extension makes
 * live edits emit plan-changed without a state transition, like Rust does,
 * and gives sync a local empty result. No dev fixture needs to be modified. */
const installCalendarStub = (syncError = null) => `(() => {
  const original = window.__TAURI_INTERNALS__.invoke.bind(window.__TAURI_INTERNALS__);
  window.__TAURI_INTERNALS__.invoke = async (cmd, args, options) => {
    if (cmd === 'calendar_sync_if_stale') {
      window.__chipStub.log.push({ cmd, args });
      if (${JSON.stringify(syncError)}) throw new Error(${JSON.stringify(syncError)});
      return [];
    }
    const result = await original(cmd, args, options);
    if (cmd === 'meeting_set_calendar') {
      const stub = window.__chipStub;
      const status = stub.merge.meeting_status;
      const event = status.calendarChoices.find(event => event.id === args.choice.eventId);
      status.plan = { ...status.plan, calendar: args.choice.mode === 'none' ? null : {
        eventId: event.id, title: event.title, provider: event.provider, titleFromEvent: true, omittedParticipants: 0
      } };
      status.calendarNotice = null;
      await import('/node_modules/@tauri-apps/api/event.js').then(m => m.emit('meeting-plan-changed'));
    }
    return result;
  };
  return true;
})()`;

export async function runMeetingsCalendar(options) {
  const out = resolve(options.out);
  await mkdir(out, { recursive: true });
  let browser;
  let cdp;
  try {
    browser = await launchEdge({ ...options, out });
    cdp = await connectCdp(browser.target.webSocketDebuggerUrl);
    await cdp.send("Page.enable");
    const report = createReport({ url: options.url, edgeVersion: browser.version.Browser ?? browser.version, fixtureBannerRemoved: true, timestamp: new Date().toISOString(), measurementScope: "headless browser fixtures; native window behavior is not measured" });
    let id = 0;
    let pass;
    const assert = (ok, message) => { if (!ok) throw new Error(message); };
    const text = selector => cdp.evaluate(`document.querySelector(${JSON.stringify(selector)})?.textContent ?? ''`);
    const hasText = async (selector, expected) => {
      await waitFor(cdp, `(document.querySelector(${JSON.stringify(selector)})?.textContent ?? '').includes(${JSON.stringify(expected)})`, expected);
    };
    const click = (selector, label) => cdp.evaluate(snippets.clickText(selector, label));
    const emit = event => cdp.evaluate(`import('/node_modules/@tauri-apps/api/event.js').then(m => m.emit(${JSON.stringify(event)})).then(() => true)`);
    const calls = cmd => cdp.evaluate(snippets.stubCalls(cmd));
    const shot = async (name, selector) => {
      if (selector) await cdp.evaluate(`document.querySelector(${JSON.stringify(selector)}).scrollIntoView({ block: 'start' }); true`);
      await delay(150);
      const filename = `${pass.name}-${name}.png`;
      await cdp.screenshot(resolve(out, filename));
      return filename;
    };
    const check = async (claim, run) => {
      const screenshots = [];
      try {
        const measured = await run(screenshots);
        addCheck(report, { id: String(++id), claim: `${pass.name}: ${claim}`, expectation: "all text, IPC and layout assertions pass", measured: measured ?? { assertionsPassed: true }, matches: true, screenshots });
      } catch (cause) {
        screenshots.push(await shot(`failure-${id + 1}`).catch(() => ""));
        addCheck(report, { id: String(++id), claim: `${pass.name}: ${claim}`, expectation: "all text, IPC and layout assertions pass", measured: { error: String(cause) }, matches: false, screenshots });
      }
    };
    const open = async (fixture, { panel = false, width = pass.width, height = pass.height, upcomingDelay = 0, upcomingError = null, candidatesError = null, syncError = null } = {}) => {
      await cdp.send("Emulation.setDeviceMetricsOverride", { width, height, deviceScaleFactor: 1, mobile: false });
      await cdp.send("Emulation.setEmulatedMedia", { features: [{ name: "prefers-color-scheme", value: pass.scheme }] });
      await cdp.send("Page.navigate", { url: "about:blank" });
      await cdp.send("Page.navigate", { url: options.url.replace(/#.*$/, "") + (panel ? "#meeting" : "") });
      await waitFor(cdp, panel ? "Boolean(document.querySelector('.notify'))" : snippets.SHELL_READY, "fixture document");
      await cdp.evaluate(snippets.removeFixtureBanner());
      // Ensure Meetings mounts only after its independent reads are stubbed.
      if (!panel) await click("button.nav-pill", "Dictation");
      await cdp.evaluate(snippets.installInvokeStub());
      await cdp.evaluate(snippets.stubInvoke({
        delay: { calendar_upcoming: upcomingDelay }, reject: { calendar_upcoming: upcomingError, meeting_calendar_candidates: candidatesError },
        merge: { calendar_upcoming: { ...fixture.upcoming, providers: fixture.upcoming.providers.map(provider => ({ ...provider, lastSuccessAt: provider.lastSuccessAt ? new Date(Date.now() - 180_000).toISOString() : null })) }, meeting_calendar_candidates: fixture.candidates, ...(fixture.live ? { meeting_status: fixture.live } : {}) },
      }));
      await cdp.evaluate(installCalendarStub(syncError));
      if (panel) {
        // Match floating-windows.mjs: #meeting at the real panel size, with
        // capture exclusion stubbed active and the fixture state event.
        await cdp.evaluate("kw.meetingState('recording'); true");
        await waitFor(cdp, "Boolean(document.querySelector('.notify-panel'))", "recording panel");
      } else {
        await click("button.nav-pill", "Meetings");
        await waitFor(cdp, fixture.live ? "Boolean(document.querySelector('.meeting-live'))" : START, "Meetings ready");
      }
    };

    for (pass of [
      { name: "light-1280x800", width: 1280, height: 800, scheme: "light" },
      { name: "light-720x520", width: 720, height: 520, scheme: "light" },
      { name: "dark-1280x800", width: 1280, height: 800, scheme: "dark" },
    ]) {
      const f = fixtures();
      const failure = { state: "failed", problem: null, message: "Safe fixture failure" };
      const states = [
        ["events", f.upcoming, ["Platform standup", "Untitled event", "Google Calendar", "Outlook", "Now", "and more this week"]],
        ["not-connected", { ...f.upcoming, events: [], providers: [] }, ["Connect a calendar to see upcoming meetings here.", "Calendar"]],
        ["no-calendars", { ...f.upcoming, events: [], providers: [f.provider("google", { selectedCalendars: 0 })] }, ["Choose which calendars to use in Settings."]],
        ["empty-week", { ...f.upcoming, events: [], hasMore: false }, ["Nothing on your calendar this week."]],
        ["partial-failure", { ...f.upcoming, providers: [f.provider("google", { lastOutcome: { state: "completed", eventsUpserted: 2, eventsRemoved: 0, calendarErrors: ["primary"] } }), f.provider("microsoft")] }, ["Platform standup", "Google Calendar: couldn't sync, see Settings", "Outlook synced 3 min ago"]],
        ["failure", { ...f.upcoming, providers: [f.provider("google", { lastOutcome: failure })] }, ["Google Calendar: couldn't sync, see Settings"]],
        ["freshness", f.upcoming, ["Google Calendar synced 3 min ago", "Outlook synced 3 min ago"]],
        ["token", { ...f.upcoming, providers: [f.provider("google", { lastOutcome: { state: "token-unavailable", problem: null, message: "Safe fixture" } })] }, ["Google Calendar: reconnect in Settings"]],
        ["never-synced", { ...f.upcoming, providers: [f.provider("microsoft", { lastOutcome: null, lastSuccessAt: null })] }, ["Outlook: not synced yet"]],
      ];
      for (const [name, upcoming, labels] of states) await check(`upcoming ${name}`, async shots => {
        await open({ ...f, upcoming });
        for (const label of labels) await hasText(".upcoming", label);
        assert((await calls("calendar_upcoming")).every(call => call.limit === 5), "Upcoming must ask for five");
        assert((await calls("calendar_sync_if_stale")).length === 1, "one sync per visit");
        assert(await cdp.evaluate("!document.querySelector('.upcoming-rows button')"), "rows are informational");
        shots.push(await shot(`upcoming-${name}`, ".upcoming"));
        return { labels };
      });
      await check("upcoming loading stays independent of Start and Recent", async shots => {
        await open(f, { upcomingDelay: 5000 });
        assert(await cdp.evaluate("document.querySelectorAll('.upcoming .task-placeholder').length === 3"), "three loading rows");
        assert(await cdp.evaluate("Boolean(document.querySelector('.recent:not(.upcoming)'))"), "Recent renders during upcoming read");
        shots.push(await shot("upcoming-loading", ".upcoming"));
      });
      await check("upcoming read failure has its own working Retry", async shots => {
        await open(f, { upcomingError: "Cache read failed" });
        await hasText(".upcoming", "Couldn't load upcoming meetings: Cache read failed");
        shots.push(await shot("upcoming-read-error", ".upcoming"));
        await cdp.evaluate(snippets.stubInvoke({ reject: { calendar_upcoming: null } }));
        await click(".upcoming button", "Retry");
        await hasText(".upcoming", "Platform standup");
        assert(!(await text(".upcoming")).includes("Cache read failed"), "Retry clears the read error");
      });
      await check("sync command failure belongs to freshness, leaving cached rows visible", async shots => {
        await open(f, { syncError: "Sync command failed" });
        await hasText(".upcoming", "Couldn't sync calendars: Sync command failed");
        await hasText(".upcoming", "Platform standup");
        shots.push(await shot("sync-error", ".upcoming"));
      });

      for (const mode of ["auto", "fresh-auto", "chosen", "none", "empty", "overlap", "solo", "solo-chosen"]) await check(`idle ${mode} and Start calendar arguments`, async shots => {
        const candidates = mode === "empty" ? { events: [], auto: null } : mode.startsWith("solo") ? { events: [f.solo], auto: null }
          : ["chosen", "overlap"].includes(mode) ? { events: [f.a, f.b], auto: null } : f.candidates;
        await open({ ...f, candidates });
        await waitFor(cdp, `(window.__chipStub.log.filter(x => x.cmd === 'meeting_calendar_candidates').length > 0)`, "candidate read");
        if (["auto", "fresh-auto", "none"].includes(mode)) {
          await hasText(".calendar-choice", "For: Platform standup · Now");
          assert(await cdp.evaluate("document.querySelector('input.meeting-name').placeholder === 'Platform standup'"), "event title placeholder");
          if (mode === "none") { await click(".calendar-choice button", "Don't link"); await hasText(".calendar-choice", "Not linked"); }
        } else if (mode !== "empty") {
          await hasText(".calendar-choice", "Not linked");
          await click(".calendar-choice button", "Link to an event");
          await hasText(".calendar-picker", mode.startsWith("solo") ? "no invitees" : "Project review");
          assert(await cdp.evaluate("!document.querySelector('.calendar-picker [aria-pressed=true]')"), "ambiguous and solo events never preselected");
          shots.push(await shot(`picker-${mode}`, ".tile-form"));
          if (mode === "chosen") await cdp.evaluate("[...document.querySelectorAll('.calendar-picker button')].find(b => b.textContent.includes('Project review')).click(); true");
          if (mode === "solo-chosen") await cdp.evaluate("[...document.querySelectorAll('.calendar-picker button')].find(b => b.textContent.includes('Focus time')).click(); true");
        }
        shots.push(await shot(`idle-${mode}`, ".tile-form"));
        if (mode === "fresh-auto") await cdp.evaluate(snippets.stubInvoke({ merge: { meeting_calendar_candidates: { events: [f.b], auto: f.b.id } } }));
        await cdp.evaluate(snippets.clearStubCalls());
        await cdp.evaluate(snippets.clickSelector(".control-start"));
        await waitFor(cdp, "Boolean(document.querySelector('.meeting-live'))", "Start finishes");
        const starts = await calls("meeting_start");
        assert(starts.length === 1, "exactly one Start");
        const expected = mode === "empty" ? undefined : mode === "none" ? { mode: "none" }
          : ["solo", "overlap"].includes(mode) ? { mode: "auto" } : { mode: "event", eventId: mode === "solo-chosen" ? f.solo.id : ["chosen", "fresh-auto"].includes(mode) ? f.b.id : f.a.id };
        assert(JSON.stringify(starts[0].calendar) === JSON.stringify(expected), `Start calendar: ${JSON.stringify(starts)}`);
        assert((await calls("meeting_calendar_candidates")).length === 1, "Start rereads candidates");
        return { starts, expected: expected ?? "omitted" };
      });
      await check("a failed candidate read leaves recording available", async shots => {
        await open(f, { candidatesError: "Candidate cache failed" });
        await hasText(".tile-form", "Couldn't read calendar choices: Candidate cache failed");
        shots.push(await shot("candidate-error", ".tile-form"));
        await cdp.evaluate(snippets.clearStubCalls());
        await cdp.evaluate(snippets.clickSelector(".control-start"));
        await waitFor(cdp, "Boolean(document.querySelector('.meeting-live'))", "recording without calendar context");
        const starts = await calls("meeting_start");
        assert(starts.length === 1 && !("calendar" in starts[0]), "Rust receives its default Auto after a failed optional cache read");
        return { starts };
      });

      for (const panel of [false, true]) for (const linked of [true, false]) await check(`${panel ? "panel" : "live view"} ${linked ? "linked" : "overlap"}`, async shots => {
        const live = { ...f.status, plan: { ...f.status.plan, calendar: linked ? f.status.plan.calendar : null }, calendarNotice: linked ? null : "overlap" };
        await open({ ...f, live }, panel ? { panel: true, width: 440, height: 560 } : {});
        const surface = panel ? ".notify-panel" : ".meeting-live";
        await hasText(surface, linked ? LINKED : OVERLAP);
        if (!linked) await hasText(surface, "Link to…");
        shots.push(await shot(`${panel ? "panel" : "live"}-${linked ? "linked" : "overlap"}`, surface));
        if (panel) {
          await cdp.send("Emulation.setDeviceMetricsOverride", { width: 392, height: 280, deviceScaleFactor: 1, mobile: false });
          assert(await cdp.evaluate("document.querySelector('.notify-tabs').getBoundingClientRect().bottom <= innerHeight"), "tabs fit beneath the original calendar line at minimum size");
          shots.push(await shot(`panel-minimum-${linked ? "linked" : "overlap"}`));
          await cdp.send("Emulation.setDeviceMetricsOverride", { width: 440, height: 560, deviceScaleFactor: 1, mobile: false });
        }
        const before = (await calls("meeting_status")).length;
        if (linked) {
          await cdp.evaluate(snippets.stubInvoke({ reject: { meeting_set_calendar: "Save refused" } }));
          await click(`${surface} .recording-calendar button`, "Remove");
          await hasText(surface, "Couldn't change the calendar link");
          await hasText(surface, LINKED);
          await cdp.evaluate(snippets.stubInvoke({ reject: { meeting_set_calendar: null } }));
          await click(`${surface} .recording-calendar button`, "Remove");
          await hasText(surface, "Link to…");
          assert(!(await text(surface)).includes(LINKED), "Remove reads the changed plan");
        } else {
          await click(`${surface} .recording-calendar button`, "Link to…");
          await hasText(`${surface} .calendar-picker`, "Project review");
          shots.push(await shot(`${panel ? "panel" : "live"}-overlap-picker`, surface));
          await cdp.evaluate(`[...document.querySelectorAll('${surface} .calendar-picker button')].find(b => b.textContent.includes('Platform standup')).click(); true`);
          await hasText(surface, LINKED);
        }
        assert((await calls("meeting_status")).length > before, "plan-changed rereads status without a state transition");
        const edits = await calls("meeting_set_calendar");
        assert(edits.every(edit => edit.sessionId === live.sessionId), "edit uses backend session id");
        assert(edits.at(-1).choice.mode === (linked ? "none" : "event"), "edit mode");
        if (!linked) assert(edits.at(-1).choice.eventId === f.a.id && !("event_id" in edits.at(-1).choice), "camelCase edit id");
        if (panel) {
          await cdp.send("Emulation.setDeviceMetricsOverride", { width: 392, height: 280, deviceScaleFactor: 1, mobile: false });
          const fits = await cdp.evaluate(`(() => {
            const panel = document.querySelector('.notify-panel'); const tabs = panel.querySelector('.notify-tabs').getBoundingClientRect();
            return { tabsBottom: tabs.bottom, height: innerHeight, width: panel.scrollWidth, clientWidth: panel.clientWidth };
          })()`);
          assert(fits.tabsBottom <= fits.height && fits.width <= fits.clientWidth, `minimum panel fits: ${JSON.stringify(fits)}`);
          shots.push(await shot(`panel-minimum-${linked ? "removed" : "linked"}`));
        }
        return { edits, statusReadsBefore: before, statusReadsAfter: (await calls("meeting_status")).length };
      });
      await check("calendar-changed refreshes both local reads without another sync", async () => {
        await open(f);
        await hasText(".upcoming", "Platform standup");
        const before = { upcoming: (await calls("calendar_upcoming")).length, candidates: (await calls("meeting_calendar_candidates")).length };
        await cdp.evaluate(snippets.stubInvoke({ merge: { calendar_upcoming: { ...f.upcoming, events: [f.b] }, meeting_calendar_candidates: { events: [f.b], auto: f.b.id } } }));
        await emit("calendar-changed");
        await hasText(".calendar-choice", "For: Project review");
        assert((await calls("calendar_upcoming")).length > before.upcoming && (await calls("meeting_calendar_candidates")).length > before.candidates, "both caches reread");
        assert((await calls("calendar_sync_if_stale")).length === 1, "no event-driven sync");
      });
    }
    await writeFile(resolve(out, "report.json"), `${JSON.stringify(report, null, 2)}\n`);
    const markdown = renderMarkdown(report);
    await writeFile(resolve(out, "report.md"), markdown);
    process.stdout.write(markdown);
    return report;
  } finally {
    try { await cdp?.close(); } finally { await killBrowser(browser?.child); }
  }
}

if (isEntryPoint(import.meta.url)) {
  try {
    const options = parseShellArgs(process.argv.slice(2));
    if (options.help) process.stdout.write(`${shellUsage().replace("shell-title-bar.mjs", "meetings-calendar.mjs")}\n`);
    else if (failedChecks(await runMeetingsCalendar(options)).length) process.exitCode = 1;
  } catch (cause) { console.error(`meetings-calendar failed: ${String(cause)}`); process.exitCode = 1; }
}
