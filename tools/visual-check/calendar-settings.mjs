#!/usr/bin/env node
/**
 * Calendar Settings against npm run dev fixtures, in headless Edge. Each case
 * starts from a fresh document with installInvokeStub / stubInvoke; only the
 * invoke boundary varies. Both providers are captured at 1280x800 and 720x520.
 * Reports describe UI and command arguments, never a typed secret or sign-in
 * URL. This does not exercise a real provider, credential store or browser flow.
 */
import { mkdir, writeFile } from "node:fs/promises";
import { randomBytes } from "node:crypto";
import { resolve } from "node:path";
import { parseShellArgs, shellUsage } from "./args.mjs";
import { launchEdge, killBrowser } from "./browser.mjs";
import { connectCdp } from "./cdp.mjs";
import { createReport, addCheck, failedChecks, renderMarkdown } from "./report.mjs";
import * as snippets from "./snippets.mjs";
import { isEntryPoint } from "../ci/entry.mjs";

const delay = ms => new Promise(done => setTimeout(done, ms));
const SURFACE = '[data-surface="integrations.calendar"]';
const PROVIDERS = ["google", "microsoft"];
const section = provider => `${SURFACE} [data-calendar-provider="${provider}"]`;
const GOOGLE_HELP = "If Google says it hasn't verified this app, you can continue past that warning. If it says access is blocked, your organisation doesn't allow this app.";
const OUTLOOK_HELP = "A work or school account may need an administrator to approve KnegisWhisper first.";
const RECONNECT = "A connection imported from OpenWhispr, or made with a different client, has to be made again here.";
const LINK = "https://example.invalid/calendar-sign-in";
const own = 'Using your own client';
const builtIn = "Using KnegisWhisper's built-in client";
const missing = provider => `This build has no built-in ${provider === "google" ? "Google Calendar" : "Outlook"} connection. Add your own client.`;

function providerStatus(overrides = {}) {
  return {
    clientIdConfigured: true, customClientId: null, clientSource: "app",
    customGoogleSecretConfigured: false, appClientAvailable: true,
    operation: "idle", pendingSignIn: null, cleanupPending: 0, leftover: [], account: null,
    ...overrides,
  };
}
function account(token = { state: "active" }, outcome = null) {
  return {
    email: "calendar@example.com", token,
    calendars: [{ id: "primary", summary: "Work", isPrimary: true, isSelected: true, eventCount: 3 }],
    lastSync: outcome ? {
      at: "2026-11-10T08:00:00Z", account: "calendar@example.com", snapshotVersion: 1,
      lastSuccessAt: null, outcome,
    } : null,
  };
}
const statusFor = (overrides = {}) => Object.fromEntries(PROVIDERS.map(provider => [provider, providerStatus(overrides)]));

async function waitFor(cdp, expression, description, timeout = 10_000) {
  const deadline = Date.now() + timeout;
  while (Date.now() < deadline) {
    try { if (await cdp.evaluate(expression)) return; } catch { /* Reload may replace the context. */ }
    await delay(60);
  }
  throw new Error(`Timed out waiting for ${description}`);
}
function assert(condition, description) {
  if (!condition) throw new Error(description);
}
async function reload(cdp) {
  let off = () => {};
  let timer;
  const loaded = new Promise((done, reject) => {
    timer = setTimeout(() => reject(new Error("Timed out waiting for reload")), 15_000);
    off = cdp.on("Page.loadEventFired", done);
  });
  try { await cdp.send("Page.reload", { ignoreCache: true }); await loaded; }
  finally { clearTimeout(timer); off(); }
}
async function status(cdp, providers) {
  await cdp.evaluate(snippets.stubInvoke({ merge: { calendar_status: providers } }));
}
async function prepared(cdp, width, height, providers) {
  await cdp.send("Emulation.setDeviceMetricsOverride", { width, height, deviceScaleFactor: 1, mobile: false });
  await reload(cdp);
  await waitFor(cdp, snippets.SHELL_READY, "the fixture shell");
  await cdp.evaluate(snippets.removeFixtureBanner());
  await cdp.evaluate(snippets.installInvokeStub());
  await status(cdp, providers);
  // Intercept only clipboard writing, leaving the browser's click and the
  // application's clipboard call intact. No host clipboard is changed.
  await cdp.evaluate(`(() => {
    window.__calendarCopied = [];
    Object.defineProperty(navigator.clipboard, 'writeText', {
      configurable: true, value: async text => { window.__calendarCopied.push(text); },
    });
    return true;
  })()`);
  await cdp.evaluate(snippets.openSettingsScreen("Calendar"));
  await waitFor(cdp, `Boolean(document.querySelector(${JSON.stringify(section("microsoft"))}))`, "both Calendar sections");
  await cdp.evaluate(snippets.clearStubCalls());
}
async function read(cdp, provider) {
  return cdp.evaluate(`(() => {
    const root = document.querySelector(${JSON.stringify(section(provider))});
    const advanced = [...root.querySelectorAll('details')].find(item => item.querySelector('summary').textContent === 'Advanced');
    const password = root.querySelector('input[type=password]');
    return {
      text: root.innerText, heading: root.querySelector('h2').textContent,
      advancedOpen: advanced.open,
      advancedLast: root.lastElementChild === advanced,
      advancedText: advanced.textContent,
      clientLines: [...root.querySelectorAll('p')]
        .filter(item => ${JSON.stringify([builtIn, own, missing(provider)])}.includes(item.textContent))
        .map(item => ({ text: item.textContent, inAdvanced: advanced.contains(item) })),
      buttons: [...root.querySelectorAll('button')].filter(item => item.getClientRects().length)
        .map(item => ({ text: item.textContent.trim(), disabled: item.disabled, primary: item.classList.contains('action-iris') })),
      clientId: root.querySelector('input[type=text]').value,
      password: password ? { type: password.type, empty: password.value === '', placeholder: password.placeholder } : null,
      calendars: root.querySelectorAll('.calendar-card').length,
      toggles: [...root.querySelectorAll('.calendar-card input')].map(item => ({ checked: item.checked, disabled: item.disabled })),
      overflow: document.documentElement.scrollWidth > innerWidth
        || [...document.querySelectorAll('.split-main, .content')].some(item => item.scrollWidth > item.clientWidth + 1),
    };
  })()`);
}
const button = (state, text) => state.buttons.find(item => item.text === text);
async function press(cdp, provider, text, selector = "button") {
  const query = `${section(provider)} ${selector}`;
  await cdp.evaluate(`(() => {
    const item = [...document.querySelectorAll(${JSON.stringify(query)})].find(item => item.textContent.trim() === ${JSON.stringify(text)});
    if (!item || item.disabled) throw new Error('Missing or disabled action: ' + ${JSON.stringify(text)});
    item.scrollIntoView({ block: 'center' });
    return true;
  })()`);
  const point = await cdp.evaluate(snippets.centerOfText(query, text));
  await cdp.send("Input.dispatchMouseEvent", { type: "mouseMoved", ...point });
  await cdp.send("Input.dispatchMouseEvent", { type: "mousePressed", ...point, button: "left", clickCount: 1 });
  await cdp.send("Input.dispatchMouseEvent", { type: "mouseReleased", ...point, button: "left", clickCount: 1 });
  await delay(120);
}
async function type(cdp, selector, text) {
  await cdp.evaluate(`(() => {
    const item = document.querySelector(${JSON.stringify(selector)});
    if (!item || item.disabled) throw new Error('Missing or disabled input');
    item.scrollIntoView({ block: 'center' }); item.focus(); item.select();
    return true;
  })()`);
  await cdp.send("Input.dispatchKeyEvent", { type: "rawKeyDown", key: "Backspace", code: "Backspace", windowsVirtualKeyCode: 8 });
  await cdp.send("Input.dispatchKeyEvent", { type: "keyUp", key: "Backspace", code: "Backspace", windowsVirtualKeyCode: 8 });
  if (text) await cdp.send("Input.insertText", { text });
  await delay(80);
}
async function shots(cdp, out, name, width, height, selectors = {}) {
  const files = [];
  for (const provider of PROVIDERS) {
    const selector = selectors[provider] ?? section(provider);
    await cdp.evaluate(`(() => { document.querySelector(${JSON.stringify(selector)}).scrollIntoView({ block: 'start' }); return true; })()`);
    await delay(100);
    const file = `${name}-${provider}-${width}x${height}.png`;
    await cdp.screenshot(resolve(out, file));
    files.push(file);
  }
  return files;
}
const calls = (cdp, command) => cdp.evaluate(snippets.stubCalls(command));
const waitText = (cdp, provider, text) => waitFor(cdp,
  `document.querySelector(${JSON.stringify(section(provider))})?.innerText.includes(${JSON.stringify(text)})`, text);

export async function runCalendarSettings(options) {
  const out = resolve(options.out);
  await mkdir(out, { recursive: true });
  let browser;
  let cdp;
  try {
    browser = await launchEdge({ ...options, out });
    cdp = await connectCdp(browser.target.webSocketDebuggerUrl);
    await cdp.send("Page.enable");
    const report = createReport({
      url: options.url, edgeVersion: browser.version.Browser ?? browser.version,
      timestamp: new Date().toISOString(), fixtureBannerRemoved: true,
      measurementScope: "headless Edge against invoke-stubbed dev fixtures; no real OAuth or credential store",
    });
    for (const [width, height] of [[1280, 800], [720, 520]]) {
      const check = async (name, claim, work) => {
        const screenshots = [];
        try {
          const measured = await work(async (suffix = name, selectors = {}) => {
            screenshots.push(...await shots(cdp, out, suffix, width, height, selectors));
          });
          addCheck(report, { id: `${name}-${width}x${height}`, claim, expectation: claim, measured, screenshots, matches: true });
        } catch (error) {
          // Never dump stub logs or form values here: a draft may contain a
          // generated test secret. Password fields are masked in screenshots.
          const file = `${name}-failed-${width}x${height}.png`;
          try { await cdp.screenshot(resolve(out, file)); screenshots.push(file); } catch { /* No page. */ }
          addCheck(report, { id: `${name}-${width}x${height}`, claim, expectation: claim,
            measured: { failedAt: error.message }, screenshots, matches: false });
        }
      };
      for (const source of ["app", "custom", "missing"]) {
        await check(`client-${source}`, `${source} clients show the client-in-use line and the appropriate primary Connect action.`, async capture => {
          const providers = statusFor({
            clientSource: source, clientIdConfigured: source !== "missing",
            customClientId: source === "custom" ? "saved-custom-client" : null,
            customGoogleSecretConfigured: source === "custom", appClientAvailable: source !== "missing",
          });
          await prepared(cdp, width, height, providers);
          const measured = {};
          for (const provider of PROVIDERS) {
            const state = measured[provider] = await read(cdp, provider);
            assert(state.heading === (provider === "google" ? "google calendar" : "outlook"), "Provider heading");
            assert(provider !== "microsoft" || state.text.includes("Outlook.com, Hotmail and Microsoft 365"), "Outlook sub-line");
            assert(!state.text.includes("Microsoft Calendar"), "Outlook naming");
            const clientLine = source === "missing" ? missing(provider) : source === "app" ? builtIn : own;
            assert(state.clientLines.length === 1 && state.clientLines[0].text === clientLine
              && state.clientLines[0].inAdvanced === (source !== "missing"), "One client-in-use line in the appropriate location");
            assert(state.advancedText.includes(source === "missing"
              ? "This build needs your own desktop app client."
              : "Use your own desktop app client, or clear the Client ID to return to this build's built-in client."), "Advanced client instruction");
            const connect = button(state, "Connect");
            assert(connect?.primary && connect.disabled === (source === "missing"), "Primary Connect availability");
            assert(state.advancedOpen === (source === "missing"), "Advanced disclosure state");
            assert(state.advancedLast, "Advanced is last in the section");
            assert(!state.overflow, "No horizontal overflow");
            assert(!state.password || state.password.empty, "A saved secret is never filled into the input");
          }
          await capture();
          if (source === "custom") {
            await press(cdp, "google", "Advanced", "summary");
            const advanced = await read(cdp, "google");
            assert(advanced.password.placeholder === "Saved" && advanced.password.empty, "Saved custom secret indicator");
            await capture("custom-advanced", { google: `${section("google")} > details` });
          }
          return measured;
        });
      }

      await check("connecting", "Connect stays in flight with Cancel, sign-in link and provider troubleshooting, including a browser that did not open.", async capture => {
        const providers = statusFor();
        await prepared(cdp, width, height, providers);
        await cdp.evaluate(snippets.stubInvoke({ delay: { calendar_connect: 6000 }, merge: {
          calendar_connect: { state: "failed", problem: { kind: "cancelled" } },
        } }));
        await press(cdp, "google", "Connect");
        // Only the one-second local read can pick up this pending sign-in.
        providers.google = providerStatus({ operation: "connecting", pendingSignIn: { authorizeUrl: LINK, browserOpened: false } });
        providers.microsoft = providerStatus({ operation: "connecting", pendingSignIn: { authorizeUrl: LINK, browserOpened: true } });
        await status(cdp, providers);
        await waitText(cdp, "google", "Your browser didn't open. Copy the sign-in link into your browser.");
        const measured = {};
        for (const provider of PROVIDERS) {
          const state = measured[provider] = await read(cdp, provider);
          assert(state.text.includes("Finish signing in in your browser"), "Finish signing in state");
          assert(state.text.includes(provider === "google" ? GOOGLE_HELP : OUTLOOK_HELP), "Provider troubleshooting");
          assert(button(state, "Cancel") && button(state, "Copy sign-in link"), "Sign-in actions");
          assert(!button(state, "Connect"), "No competing Connect action");
          await press(cdp, provider, "Copy sign-in link");
          await waitText(cdp, provider, "Sign-in link copied.");
        }
        assert(await cdp.evaluate(`window.__calendarCopied.length === 2 && window.__calendarCopied.every(item => item === ${JSON.stringify(LINK)})`), "Both sign-in links reach clipboard.writeText");
        assert((await calls(cdp, "calendar_status")).length === 1, "Exactly one delayed status read while waiting");
        await capture();
        providers.google = providerStatus();
        providers.microsoft = providerStatus();
        await status(cdp, providers);
        await press(cdp, "google", "Cancel");
        const cancelled = await calls(cdp, "calendar_cancel_connect");
        assert(cancelled.length === 1 && cancelled[0].provider === "google", "Cancel invokes its wrapper while connect is in flight");
        await waitText(cdp, "google", "Connection cancelled.");
        await delay(1100);
        assert((await calls(cdp, "calendar_status")).length === 3, "No polling after cancellation");
        return { ...measured, clipboardWrites: 2, cancelCalls: cancelled.length };
      });

      await check("timeout", "A timed-out sign-in shows troubleshooting and Retry for Google Calendar and Outlook.", async capture => {
        await prepared(cdp, width, height, statusFor());
        await cdp.evaluate(snippets.stubInvoke({ merge: { calendar_connect: { state: "failed", problem: { kind: "timed-out" } } } }));
        const measured = {};
        for (const provider of PROVIDERS) {
          await press(cdp, provider, "Connect");
          await waitText(cdp, provider, "Sign-in timed out. Try again.");
          const state = measured[provider] = await read(cdp, provider);
          assert(state.text.includes(provider === "google" ? GOOGLE_HELP : OUTLOOK_HELP), "Timeout troubleshooting");
          assert(button(state, "Retry")?.primary && !button(state, "Retry").disabled, "Timeout Retry");
        }
        await capture();
        await press(cdp, "google", "Retry");
        await waitText(cdp, "google", "Sign-in timed out. Try again.");
        assert((await calls(cdp, "calendar_connect")).length === 3, "Retry starts another connect");
        return measured;
      });

      for (const trigger of ["token", "connect", "sync"]) {
        await check(`reconnect-${trigger}`, `Reconnect is offered when ${trigger} reports that the grant must be made again.`, async capture => {
          const providers = statusFor();
          if (trigger === "token") for (const provider of PROVIDERS) providers[provider].account = account({ state: "reconnect-required" });
          if (trigger === "sync") for (const provider of PROVIDERS) providers[provider].account = account();
          await prepared(cdp, width, height, providers);
          if (trigger !== "token") {
            const cmd = trigger === "connect" ? "calendar_connect" : "calendar_sync";
            await cdp.evaluate(snippets.stubInvoke({ merge: { [cmd]: { state: "failed", problem: { kind: "reconnect-required" }, message: "Reconnect." } } }));
            for (const provider of PROVIDERS) await press(cdp, provider, trigger === "connect" ? "Connect" : "Sync now");
          }
          const measured = {};
          for (const provider of PROVIDERS) {
            await waitText(cdp, provider, RECONNECT);
            const state = measured[provider] = await read(cdp, provider);
            assert(button(state, "Reconnect")?.primary, "Reconnect primary action");
            if (trigger !== "connect") assert(button(state, "Sync now").disabled, "A new grant is required before sync");
          }
          await capture();
          await cdp.evaluate(snippets.stubInvoke({ merge: { calendar_connect: { state: "failed", problem: { kind: "cancelled" } } } }));
          await cdp.evaluate(snippets.clearStubCalls());
          await press(cdp, "google", "Reconnect");
          await waitText(cdp, "google", "Connection cancelled.");
          assert((await calls(cdp, "calendar_connect"))[0]?.provider === "google", "Reconnect calls connect");
          return measured;
        });
      }

      await check("leftovers", "Imported and earlier connections show their email, origin and a Remove action that reloads status.", async capture => {
        const providers = statusFor({ leftover: [
          { email: "imported@example.com", origin: "openwhispr" }, { email: "earlier@example.com", origin: "earlier" },
        ] });
        await prepared(cdp, width, height, providers);
        const measured = {};
        for (const provider of PROVIDERS) {
          const state = measured[provider] = await read(cdp, provider);
          assert(state.text.includes("Imported from OpenWhispr") && state.text.includes("Earlier connection"), "Leftover origins");
          assert(state.text.includes("imported@example.com") && state.text.includes("earlier@example.com"), "Leftover emails");
          assert(state.buttons.filter(item => item.text === "Remove").length === 2, "Both leftovers have Remove");
          assert(state.advancedLast, "Leftover account actions precede Advanced");
        }
        await capture();
        providers.google = providerStatus();
        await status(cdp, providers);
        await press(cdp, "google", "Remove");
        await waitFor(cdp, `!document.querySelector(${JSON.stringify(section("google"))}).innerText.includes('imported@example.com')`, "leftover removal reload");
        const removed = await calls(cdp, "calendar_remove_leftover");
        assert(removed.length === 1 && removed[0].provider === "google" && removed[0].email === "imported@example.com", "Remove arguments");
        return measured;
      });

      await check("cleanup", "Credential cleanup names one or multiple old sign-ins and Retry reloads status.", async capture => {
        const providers = { google: providerStatus({ cleanupPending: 1 }), microsoft: providerStatus({ cleanupPending: 2 }) };
        await prepared(cdp, width, height, providers);
        const measured = {};
        for (const provider of PROVIDERS) {
          const state = measured[provider] = await read(cdp, provider);
          const sentence = provider === "google" ? "1 old sign-in couldn't be removed from the credential store" : "2 old sign-ins couldn't be removed from the credential store";
          assert(state.text.includes(sentence) && button(state, "Retry"), "Cleanup notice and Retry");
          assert(state.advancedLast, "Cleanup notice and action precede Advanced");
        }
        await capture();
        providers.google = providerStatus();
        await status(cdp, providers);
        await press(cdp, "google", "Retry");
        await waitFor(cdp, `!document.querySelector(${JSON.stringify(section("google"))}).innerText.includes('credential store')`, "cleanup retry reload");
        assert((await calls(cdp, "calendar_retry_cleanup"))[0]?.provider === "google", "Retry cleanup provider");
        return measured;
      });

      await check("advanced", "Advanced saves ID and secret together, discards drafts, keeps untouched secrets, removes them explicitly and returns to the app client when the custom ID is cleared.", async capture => {
        const providers = statusFor({ clientSource: "custom", customClientId: "saved-client", customGoogleSecretConfigured: true });
        providers.microsoft.customGoogleSecretConfigured = false;
        await prepared(cdp, width, height, providers);
        for (const provider of PROVIDERS) await press(cdp, provider, "Advanced", "summary");
        const fields = provider => `${section(provider)} input[type=text]`;
        const password = `${section("google")} input[type=password]`;
        const advancedSelectors = Object.fromEntries(PROVIDERS.map(provider => [provider, `${section(provider)} > details`]));
        assert((await read(cdp, "google")).password.empty, "Stored secret is absent from DOM");
        for (const provider of PROVIDERS) await type(cdp, fields(provider), "new-client");
        // Generate a throwaway password at runtime; no secret literal lives in
        // a fixture, snapshot, report or source file. Only comparison booleans
        // leave the page when the captured write is inspected below.
        await type(cdp, password, randomBytes(18).toString("base64url"));
        await cdp.evaluate(`window.__calendarTypedSecret = document.querySelector(${JSON.stringify(password)}).value; true`);
        assert((await calls(cdp, "calendar_set_custom_client")).length === 0, "Editing ID and then secret makes no separate writes");
        await capture("advanced-draft", advancedSelectors);
        // Each save reloads both providers. Publish only this provider's new
        // status so that reload cannot make the other provider's draft clean.
        for (const provider of PROVIDERS) {
          providers[provider].customClientId = "new-client";
          await status(cdp, providers);
          await press(cdp, provider, "Save changes");
        }
        await waitFor(cdp, `document.querySelector(${JSON.stringify(password)}).value === ''`, "saved password to clear");
        const savedTogether = await cdp.evaluate(`(() => {
          const writes = window.__chipStub.log.filter(item => item.cmd === 'calendar_set_custom_client');
          const google = writes.find(item => item.args.provider === 'google')?.args;
          const outlook = writes.find(item => item.args.provider === 'microsoft')?.args;
          return writes.length === 2 && google?.clientId === 'new-client'
            && google.googleSecret.kind === 'set' && google.googleSecret.value === window.__calendarTypedSecret
            && outlook?.clientId === 'new-client' && outlook.googleSecret.kind === 'keep';
        })()`);
        assert(savedTogether, "One combined write per provider, set for typed Google secret and keep for Outlook");
        await capture("advanced-save", advancedSelectors);
        await cdp.evaluate(snippets.clearStubCalls());
        for (const provider of PROVIDERS) await type(cdp, fields(provider), "discarded-client");
        await type(cdp, password, randomBytes(18).toString("base64url"));
        for (const provider of PROVIDERS) await press(cdp, provider, "Discard changes");
        for (const provider of PROVIDERS) assert((await read(cdp, provider)).clientId === "new-client", "Discard restores saved ID");
        assert((await read(cdp, "google")).password.empty && (await read(cdp, "google")).password.placeholder === "Saved", "Discard restores saved secret indicator");
        assert((await calls(cdp, "calendar_set_custom_client")).length === 0, "Discard does not write");
        await capture("advanced-discard", advancedSelectors);
        await type(cdp, fields("google"), "id-only-client");
        // No intervening save reloads status before this single-provider save.
        providers.google.customClientId = "id-only-client";
        await status(cdp, providers);
        await press(cdp, "google", "Save changes");
        const kept = await cdp.evaluate(`(() => {
          const writes = window.__chipStub.log.filter(item => item.cmd === 'calendar_set_custom_client');
          return writes.length === 1 && writes[0].args.clientId === 'id-only-client'
            && writes[0].args.googleSecret.kind === 'keep';
        })()`);
        assert(kept && (await read(cdp, "google")).password.placeholder === "Saved", "An ID-only Google edit keeps the saved secret");
        await capture("advanced-keep-secret", advancedSelectors);
        await cdp.evaluate(snippets.clearStubCalls());
        await press(cdp, "google", "Remove secret");
        await waitText(cdp, "google", "Secret will be removed when you save.");
        await press(cdp, "google", "Discard changes");
        assert((await read(cdp, "google")).password.placeholder === "Saved", "Discard undoes Remove secret");
        await press(cdp, "google", "Remove secret");
        providers.google.customGoogleSecretConfigured = false;
        await status(cdp, providers);
        await press(cdp, "google", "Save changes");
        // Project the log in the page so even an unexpected Set cannot leak a value.
        const cleared = await cdp.evaluate(`window.__chipStub.log.filter(item => item.cmd === 'calendar_set_custom_client').map(item => item.args.googleSecret.kind)`);
        assert(cleared.length === 1 && cleared[0] === "clear", "Explicit secret removal uses clear");
        await capture("advanced-remove-secret", advancedSelectors);
        await cdp.evaluate(snippets.clearStubCalls());
        for (const provider of PROVIDERS) await type(cdp, fields(provider), "");
        for (const provider of PROVIDERS) {
          providers[provider] = providerStatus();
          await status(cdp, providers);
          await press(cdp, provider, "Save changes");
        }
        const returnedToApp = await cdp.evaluate(`(() => {
          const writes = window.__chipStub.log.filter(item => item.cmd === 'calendar_set_custom_client');
          return writes.length === 2 && writes.every(item => item.args.clientId === null && item.args.googleSecret.kind === 'keep');
        })()`);
        assert(returnedToApp, "Clearing custom IDs uses null with keep");
        for (const provider of PROVIDERS) assert((await read(cdp, provider)).text.includes(builtIn), "Clearing the ID returns to the built-in client");
        await capture("advanced-return-to-app", advancedSelectors);
        return { savedTogether, discardedWithoutWrite: true, keptUntouchedSecret: kept, explicitSecretRemoval: true, returnedToApp };
      });

      await check("connected-busy", "Connected account, last sync and calendar toggles remain, and sync disables conflicting controls.", async capture => {
        const providers = statusFor({ account: account() });
        await prepared(cdp, width, height, providers);
        await cdp.evaluate(snippets.stubInvoke({ delay: { calendar_sync: 1800 } }));
        await press(cdp, "google", "Sync now");
        const busy = await read(cdp, "google");
        assert(busy.calendars === 1 && busy.toggles[0].disabled, "Busy sync disables calendar toggle");
        assert(button(busy, "Sync now").disabled && button(busy, "Disconnect").disabled, "Busy sync disables conflicting actions");
        await capture();
        await waitText(cdp, "google", "Synced: 12 events updated, 1 removed.");
        const idle = await read(cdp, "google");
        assert(idle.text.includes("calendar@example.com") && idle.text.includes("Last sync") && !idle.toggles[0].disabled, "Account and idle calendar toggle remain");
        return { busy, idle };
      });
    }
    const markdown = renderMarkdown(report);
    await writeFile(resolve(out, "report.json"), `${JSON.stringify(report, null, 2)}\n`);
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
    if (options.help) process.stdout.write(`${shellUsage().replace("shell-title-bar.mjs", "calendar-settings.mjs")}\n`);
    else {
      const failed = failedChecks(await runCalendarSettings(options));
      if (failed.length) {
        console.error(`calendar-settings: ${failed.length} check(s) false: ${failed.join(", ")}`);
        process.exitCode = 1;
      }
    }
  } catch (error) {
    console.error(`calendar-settings failed: ${error.message}`);
    process.exitCode = 1;
  }
}
