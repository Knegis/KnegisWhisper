#!/usr/bin/env node
/** Runs repeatable rail and responsive-layout measurements against dev fixtures. */
import { mkdir, writeFile } from "node:fs/promises";
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { parseShellArgs, shellUsage } from "./args.mjs";
import { launchEdge, killBrowser } from "./browser.mjs";
import { connectCdp } from "./cdp.mjs";
import { createReport, addCheck, renderMarkdown } from "./report.mjs";
import * as snippets from "./snippets.mjs";

const delay = (ms) => new Promise((resolveDelay) => setTimeout(resolveDelay, ms));
const isMain = process.argv[1]
  && resolve(process.argv[1]) === fileURLToPath(import.meta.url);

export function clipRail({ rect, scrollX = 0, scrollY = 0 }) {
  return {
    x: rect.left + scrollX,
    y: rect.top + scrollY,
    width: rect.width,
    height: rect.height,
  };
}

const screenshot = async (cdp, out, name, options) => {
  await cdp.screenshot(resolve(out, name), options);
  return name;
};

const numberEqual = (a, b, epsilon = 0.5) =>
  Math.abs(Number.parseFloat(a) - Number.parseFloat(b)) <= epsilon;

async function waitFor(cdp, expression, description, timeout = 8_000) {
  const deadline = Date.now() + timeout;
  while (Date.now() < deadline) {
    if (await cdp.evaluate(expression)) return;
    await delay(100);
  }
  throw new Error(`Timed out waiting for ${description}`);
}

async function click(cdp, selector, text) {
  return cdp.evaluate(snippets.clickText(selector, text));
}

async function geometry(cdp) {
  return cdp.evaluate(snippets.railGeometry());
}

async function viewport(cdp, width, height) {
  await cdp.send("Emulation.setDeviceMetricsOverride", {
    width,
    height,
    deviceScaleFactor: 1,
    mobile: false,
  });
  await delay(150);
}

export async function runShellRail(options) {
  const out = resolve(options.out);
  await mkdir(out, { recursive: true });
  let browser;
  let cdp;

  try {
    browser = await launchEdge({ ...options, out });
    cdp = await connectCdp(browser.target.webSocketDebuggerUrl);
    await cdp.send("Page.enable");
    await viewport(cdp, 1000, 720);
    await waitFor(
      cdp,
      "Boolean(document.querySelector('nav.sidebar') && document.querySelector('.content'))",
      "the fixture shell",
    );

    const bannerResult = options.keepFixtureBanner
      ? {
          existed: Boolean(
            await cdp.evaluate(
              "document.querySelector('[data-dev-fixtures=\"banner\"]')",
            ),
          ),
          kept: true,
        }
      : {
          ...(await cdp.evaluate(snippets.removeFixtureBanner())),
          kept: false,
        };
    const initial = await geometry(cdp);
    const report = createReport({
      url: options.url,
      edgeVersion: browser.version.Browser ?? browser.version,
      remBasis: initial.rem,
      fixtureBannerRemoved: !options.keepFixtureBanner && bannerResult.existed,
      timestamp: new Date().toISOString(),
      measurementScope: "measured in headless Edge against dev fixtures, not in the Tauri window",
    });

    // The comparison after scrolling makes this a behaviour measurement, not
    // just an interpretation of the fixed CSS declaration.
    await click(cdp, "nav.sidebar button", "Settings");
    await click(cdp, "nav.sidebar .nav-child", "Providers");
    const beforeScroll = await geometry(cdp);
    await cdp.evaluate("document.scrollingElement.scrollTo(0, 600); true");
    await delay(100);
    const afterScroll = await geometry(cdp);
    const shot1 = await screenshot(cdp, out, "01-rail.png", {
      clip: clipRail(afterScroll),
      scale: 2,
    });
    addCheck(report, {
      id: "1",
      claim: "The rail is pinned to the left edge and stays put while content scrolls.",
      measured: {
        beforeScroll,
        afterScroll,
        unchanged: beforeScroll.rect.left === afterScroll.rect.left
          && beforeScroll.rect.top === afterScroll.rect.top,
      },
      expectation: [
        "fixed, left 0, top 0, shell padding-left equals rail width,",
        "and an unchanged rectangle after scroll",
      ].join(" "),
      matches: beforeScroll.position === "fixed"
        && beforeScroll.rect.left === 0
        && beforeScroll.rect.top === 0
        && numberEqual(beforeScroll.shellPaddingLeft, beforeScroll.rect.width)
        && beforeScroll.rect.left === afterScroll.rect.left
        && beforeScroll.rect.top === afterScroll.rect.top,
      screenshots: [shot1],
      note: [
        "Measured after navigating to the long Providers screen and setting",
        "document.scrollingElement to 600px.",
      ].join(" "),
    });
    await cdp.evaluate("window.scrollTo(0, 0); true");

    // Reset the state so the closed/open comparison covers both headers rather
    // than a remembered state.
    await click(cdp, "nav.sidebar button", "Settings");
    const groupsClosed = await cdp.evaluate(snippets.groupState());
    const shot2a = await screenshot(cdp, out, "02-groups-closed.png", {
      clip: clipRail(await geometry(cdp)),
      scale: 2,
    });
    await click(cdp, "nav.sidebar button", "Settings");
    await click(cdp, "nav.sidebar button", "Developer");
    const groupsOpen = await cdp.evaluate(snippets.groupState());
    const shot2b = await screenshot(cdp, out, "02-groups-open.png", {
      clip: clipRail(await geometry(cdp)),
      scale: 2,
    });
    addCheck(report, {
      id: "2",
      claim: "Settings and Developer show a trailing + when closed and - when open.",
      measured: { closed: groupsClosed, open: groupsOpen },
      expectation: [
        "pseudo-element content changes from + to -;",
        "character appearance is human-reviewed",
      ].join(" "),
      matches: null,
      screenshots: [shot2a, shot2b],
      note: [
        "The computed pseudo-element values and font availability are measured.",
        "A person judges whether the glyph is visibly a character rather than a box.",
      ].join(" "),
    });
    const groupLayoutMatches = [
      groupsClosed.settings,
      groupsClosed.developer,
      groupsOpen.settings,
      groupsOpen.developer,
    ].every((group) =>
      group.display === "flex" && group.justifyContent === "space-between"
    );
    addCheck(report, {
      id: "3",
      claim: "A collapsed group reads as a section, not a screen.",
      measured: {
        settings: groupsClosed.settings,
        developer: groupsClosed.developer,
      },
      expectation: "nav-group uses display:flex and justify-content:space-between",
      matches: groupLayoutMatches,
      screenshots: [shot2a, shot2b],
      note: "This check reuses the group-state clips.",
    });

    await click(cdp, "nav.sidebar button", "Developer");
    await click(cdp, "nav.sidebar .nav-child", "Providers");
    const current = await cdp.evaluate(snippets.railCurrent());
    const shot4 = await screenshot(cdp, out, "04-child-current.png", {
      clip: clipRail(await geometry(cdp)),
      scale: 2,
    });
    const rem = Number.parseFloat(initial.rem);
    const transparent = (value) =>
      value === "rgba(0, 0, 0, 0)" || value === "transparent";
    addCheck(report, {
      id: "4",
      claim: "Children are indented; exactly one filled mark.",
      measured: current,
      expectation: [
        `child left padding ${1.7 * rem}px,`,
        `top-level horizontal padding ${0.6 * rem}px,`,
        "one filled current leaf, Settings open with text colour and transparent fill",
      ].join(" "),
      matches: numberEqual(current.childPaddingLeft, 1.7 * rem)
        && numberEqual(current.topPaddingLeft, 0.6 * rem)
        && current.filled.length === 1
        && current.current?.text === "Providers"
        && current.current?.className.includes("nav-child")
        && current.settings?.color === current.settings?.text
        && transparent(current.settings?.background),
      screenshots: [shot4],
      note: "The count excludes transparent button backgrounds and the sidebar's own background.",
    });

    await viewport(cdp, 1000, 720);
    const content1000 = await cdp.evaluate(snippets.contentMeasure());
    const shot5a = await screenshot(cdp, out, "05-content-1000.png");
    await viewport(cdp, 1920, 1080);
    const content1920 = await cdp.evaluate(snippets.contentMeasure());
    const shot5b = await screenshot(cdp, out, "05-content-1920.png");
    addCheck(report, {
      id: "5",
      claim: "The content is wider.",
      measured: { at1000: content1000, at1920: content1920 },
      expectation: [
        "KNEG-161's quoted large-screen figure assumes a 16px rem;",
        "this application measures 68rem from its live root font size,",
        "so the 1920 width must equal computed max-width while the 1000",
        "width is smaller.",
      ].join(" "),
      matches: numberEqual(content1920.width, content1920.maxWidth)
        && content1000.width < Number.parseFloat(content1000.maxWidth),
      screenshots: [shot5a, shot5b],
      note: [
        "No historical pixel figure is used as an assertion;",
        "the live rem basis is recorded in the header.",
      ].join(" "),
    });

    await viewport(cdp, 1000, 720);
    await click(cdp, "nav.sidebar button", "Notes");
    await waitFor(
      cdp,
      [
        "Boolean([...document.querySelectorAll('button')]",
        ".find((b) => b.textContent.trim() === 'Open note'))",
      ].join(""),
      "an Open note button",
    );
    await click(cdp, ".content button", "Open note");
    await waitFor(
      cdp,
      "Boolean(document.querySelector('.note-workspace'))",
      "an opened note",
    );
    const grid1000 = await cdp.evaluate(snippets.notesGrid());
    const shot6a = await screenshot(cdp, out, "06-notes-1000.png");
    await viewport(cdp, 1400, 900);
    const grid1400 = await cdp.evaluate(snippets.notesGrid());
    const shot6b = await screenshot(cdp, out, "06-notes-1400.png");
    addCheck(report, {
      id: "6",
      claim: "Notes shows the transcript beside the editor above 80rem.",
      measured: { at1000: grid1000, at1400: grid1400 },
      expectation: "one computed grid track at 1000px and two at 1400px",
      matches: grid1000.tracks === 1 && grid1400.tracks === 2,
      screenshots: [shot6a, shot6b],
      note: "The fixture note was opened through its visible Open note button.",
    });

    await viewport(cdp, 1000, 720);
    const settingsOpen = await cdp.evaluate(
      [
        "[...document.querySelectorAll('button.nav-group')]",
        ".find((b) => b.textContent.trim() === 'Settings')",
        "?.getAttribute('aria-expanded') === 'true'",
      ].join(""),
    );
    if (settingsOpen) await click(cdp, "nav.sidebar button", "Settings");
    await click(cdp, "nav.sidebar button", "Settings");
    await click(cdp, "nav.sidebar .nav-child", "Intelligence");
    const arrivedAtIntelligence = await cdp.evaluate(snippets.sidewaysState());
    await click(cdp, "nav.sidebar button", "Settings");
    const sidewaysCollapsedBefore = await cdp.evaluate(snippets.sidewaysState());
    const providerButton = await cdp.evaluate(
      [
        "[...document.querySelectorAll('.content button')]",
        ".find((b) => /providers/i.test(b.textContent))",
        "?.textContent ?? null",
      ].join(""),
    );
    if (!providerButton) {
      throw new Error("Intelligence did not expose its Providers navigation button");
    }
    await cdp.evaluate(`(() => {
      [...document.querySelectorAll('.content button')]
        .find((button) => /providers/i.test(button.textContent))
        .click();
    })()`);
    await delay(100);
    const sidewaysOpen = await cdp.evaluate(snippets.sidewaysState());
    await click(cdp, "nav.sidebar button", "Settings");
    const sidewaysCollapsed = await cdp.evaluate(snippets.sidewaysState());
    const shot7 = await screenshot(cdp, out, "07-sideways.png", {
      clip: clipRail(await geometry(cdp)),
      scale: 2,
    });
    addCheck(report, {
      id: "7",
      claim: "Sideways navigation opens the group holding its target.",
      measured: {
        arrivedAtIntelligence,
        collapsedBeforeProvidersButton: sidewaysCollapsedBefore,
        afterProvidersButton: sidewaysOpen,
        afterCollapse: sidewaysCollapsed,
      },
      expectation: [
        "collapsing Settings keeps the same screen heading and clears the current",
        "rail item; pressing the Providers button opens Settings and marks Providers",
        "current; after the final collapse no rail item is current while Providers",
        "heads the content",
      ].join(" "),
      matches: sidewaysCollapsedBefore.expanded === "false"
        && sidewaysCollapsedBefore.current === null
        && sidewaysCollapsedBefore.heading === arrivedAtIntelligence.heading
        && arrivedAtIntelligence.current === "Intelligence"
        && sidewaysOpen.current === "Providers"
        && sidewaysOpen.expanded === "true"
        && sidewaysCollapsed.current === null
        && /Providers/i.test(sidewaysCollapsed.heading ?? ""),
      screenshots: [shot7],
      note: [
        "No dev-fixture switch was found that deliberately throws a screen render,",
        "so the error-boundary Diagnostics route is not measured.",
      ].join(" "),
    });

    await click(cdp, "nav.sidebar button", "Settings");
    await cdp.evaluate("document.querySelector('nav.sidebar button')?.focus(); true");
    const focused = [];
    for (let i = 0; i < 20; i += 1) {
      await cdp.send("Input.dispatchKeyEvent", {
        type: "rawKeyDown",
        key: "Tab",
        code: "Tab",
        windowsVirtualKeyCode: 9,
      });
      await cdp.send("Input.dispatchKeyEvent", {
        type: "keyUp",
        key: "Tab",
        code: "Tab",
        windowsVirtualKeyCode: 9,
      });
      const state = await cdp.evaluate(snippets.focusState());
      if (state) focused.push(state);
    }
    await cdp.evaluate(
      [
        "[...document.querySelectorAll('nav.sidebar button.nav-group')]",
        ".find((b) => b.textContent.trim() === 'Settings').focus(); true",
      ].join(""),
    );
    const shot8 = await screenshot(cdp, out, "08-focus.png", {
      clip: clipRail(await geometry(cdp)),
      scale: 2,
    });
    const leaf = await cdp.evaluate(`(() => {
      const current = document.querySelector('nav.sidebar [aria-current="page"]');
      return current
        ? {
            className: current.className,
            groupCurrent: document.querySelectorAll(
              'nav.sidebar .nav-group[aria-current]',
            ).length,
          }
        : null;
    })()`);
    addCheck(report, {
      id: "8",
      claim: "Keyboard focus is visible on every rail button; aria-current lands on the leaf.",
      measured: { focused, current: leaf },
      expectation: [
        "every recorded rail button has a non-none outline,",
        "current Providers is nav-child, and no nav-group has aria-current",
      ].join(" "),
      matches: focused.length > 0
        && focused.every(
          (item) => item.outlineStyle !== "none" && item.outlineWidth !== "0px",
        )
        && leaf?.className.includes("nav-child")
        && leaf?.groupCurrent === 0,
      screenshots: [shot8],
      note: [
        "Tab was dispatched up to 20 times from the first rail button;",
        "only states reached by those Tab presses inside the rail are recorded.",
      ].join(" "),
    });

    const markdown = renderMarkdown(report);
    await writeFile(resolve(out, "report.json"), `${JSON.stringify(report, null, 2)}\n`);
    await writeFile(resolve(out, "report.md"), markdown);
    process.stdout.write(markdown);
    return report;
  } finally {
    await cdp?.close();
    await killBrowser(browser?.child);
  }
}

if (isMain) {
  try {
    const options = parseShellArgs(process.argv.slice(2));
    if (options.help) process.stdout.write(`${shellUsage()}\n`);
    else await runShellRail(options);
  } catch (error) {
    console.error(`shell-rail failed: ${error.message}`);
    process.exitCode = 1;
  }
}
