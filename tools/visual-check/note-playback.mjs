#!/usr/bin/env node
/**
 * Measures, against dev fixtures, that a note's recording keeps playing, from
 * where it was and at the speed it was, when the note moves its one player
 * (KNEG-412). A note has one `<audio>` and three places to draw it
 * (`NoteWorkspace.tsx`): the foot of the transcript column in a wide window,
 * Replay's full player, and the mini player at the foot of the note. A tab
 * switch, a crossing of the 1280px width, or hiding the column mounts the
 * player afresh in another place, and the old one hands its position to the
 * new one (`RecordingPlayer`'s `position` and `resume`).
 *
 * The fixtures serve a playable two-minute WAV for every note with a
 * recording (`lib/fixtureWav.ts`, `installDevFixtures`); before KNEG-412 the
 * player could only show its error here, so none of this was measured.
 *
 * Each check starts from a fresh load at 1440x900 on Notes, fixture note 101
 * open and playing from the transcript column at 1.5x. Play is pressed with a
 * real pointer press, so the page has the sticky user activation a person's
 * click gives it, and the later moves' `play()` calls are allowed as they
 * would be in the app. Edge runs with `--mute-audio`.
 */
import { mkdir, writeFile } from "node:fs/promises";
import { resolve } from "node:path";
import { parseShellArgs, shellUsage } from "./args.mjs";
import { launchEdge, killBrowser } from "./browser.mjs";
import { connectCdp } from "./cdp.mjs";
import { createReport, addCheck, failedChecks, renderMarkdown } from "./report.mjs";
import * as snippets from "./snippets.mjs";
import { isEntryPoint } from "../ci/entry.mjs";

const delay = (ms) => new Promise((resolveDelay) => setTimeout(resolveDelay, ms));
const isMain = isEntryPoint(import.meta.url);

const EDGE_ARGS = ["--mute-audio"];
const RAIL_ITEM = "nav.rail .rail-item";
const NOTE_OPEN = "Boolean(document.querySelector('.note-detail input[aria-label=\"Title\"]:enabled'))";
const TITLE = "document.querySelector('.note-detail input[aria-label=\"Title\"]')?.value ?? null";
// Fixture note 101 (`apps/desktop/src/lib/devFixtures.ts`), the note Notes opens first.
const NOTE_101 = "Weekly sync — platform";
const WIDE = { width: 1440, height: 900 };
const NARROW = { width: 1000, height: 900 };
const RATE = 1.5;
/** How far into the recording playback is before the first move, so a resume from 0 shows. */
const LEAD_SECONDS = 2;
/** How far behind the position before a move the new player may start. */
const BEHIND_SECONDS = 0.25;
/** The longest a move may leave the note with nothing playing. */
const SILENCE_MS = 1000;
/** How long after a move the samples and the final state are read. */
const SETTLE_MS = 1500;
const SLOT = { side: ".note-side-player", full: ".note-full-player", mini: ".note-mini-player" };

/** Playing, here: not paused and with data to play -- not merely asked to play. */
const playing = (audio) => !audio.paused && audio.readyState >= 3;
const round = (seconds) => Math.round(seconds * 100) / 100;

async function waitFor(cdp, expression, description, timeout = 8_000) {
  const deadline = Date.now() + timeout;
  while (Date.now() < deadline) {
    try {
      if (await cdp.evaluate(expression)) return;
    } catch {
      // Between two documents during a reload there is no context to evaluate in.
    }
    await delay(60);
  }
  throw new Error(`Timed out waiting for ${description}`);
}

async function reload(cdp, timeout = 15_000) {
  let off = () => {};
  let timer;
  const loaded = new Promise((resolveLoad, reject) => {
    timer = setTimeout(() => reject(new Error("Timed out waiting for the reload")), timeout);
    off = cdp.on("Page.loadEventFired", () => resolveLoad());
  });
  try {
    await cdp.send("Page.reload", { ignoreCache: true });
    await loaded;
  } finally {
    clearTimeout(timer);
    off();
  }
}

async function viewport(cdp, { width, height }) {
  await cdp.send("Emulation.setDeviceMetricsOverride", { width, height, deviceScaleFactor: 1, mobile: false });
}

/** A real pointer press at a measured point, as a person's click is. */
async function press(cdp, { x, y }) {
  await cdp.send("Input.dispatchMouseEvent", { type: "mouseMoved", x, y });
  await cdp.send("Input.dispatchMouseEvent", { type: "mousePressed", x, y, button: "left", clickCount: 1 });
  await cdp.send("Input.dispatchMouseEvent", { type: "mouseReleased", x, y, button: "left", clickCount: 1 });
}

/** Presses the element's centre, which must be on screen for the press to reach it. */
async function pressText(cdp, selector, text) {
  const point = await cdp.evaluate(snippets.centerOfText(selector, text));
  const size = await cdp.evaluate("({ width: innerWidth, height: innerHeight })");
  if (point.x < 0 || point.y < 0 || point.x > size.width || point.y > size.height) {
    throw new Error(`${selector} "${text}" is off screen at ${JSON.stringify(point)}`);
  }
  await press(cdp, point);
}

const playingIn = (slot, from = 0) => `(() => {
  const audio = document.querySelector(${JSON.stringify(`${SLOT[slot]} audio`)});
  return Boolean(audio) && !audio.paused && audio.readyState >= 3 && audio.currentTime >= ${from};
})()`;

/** A setup step that did not get as far as the move it was for, with the player as it was then. */
class SetupError extends Error {
  constructor(message, player) {
    super(message);
    this.player = player;
  }
}

/**
 * A fresh load at 1440x900 on Notes with note 101 open and its recording
 * playing from the transcript column at 1.5x, at least `LEAD_SECONDS` in.
 */
async function playingNote(cdp) {
  await viewport(cdp, WIDE);
  await reload(cdp);
  await waitFor(cdp, "Boolean(document.querySelector('nav.rail') && document.querySelector('.card .split-main, .card .content'))", "the fixture shell");
  await cdp.evaluate(snippets.removeFixtureBanner());
  await cdp.evaluate(snippets.clickText(RAIL_ITEM, "Notes"));
  await waitFor(cdp, NOTE_OPEN, "a note to open");
  const title = await cdp.evaluate(TITLE);
  if (title !== NOTE_101) throw new SetupError(`Notes opened "${title}", not fixture note 101`, null);
  await waitFor(cdp, `Boolean(document.querySelector(${JSON.stringify(`${SLOT.side} audio`)}))`, "the player in the transcript column");
  // Its metadata, or the player's own error line saying it could not be loaded.
  await waitFor(
    cdp,
    `document.querySelector(${JSON.stringify(`${SLOT.side} audio`)}).readyState >= 1 || Boolean(document.querySelector('.recording-player .error'))`,
    "the recording's metadata or the player's error",
  );
  const loaded = await cdp.evaluate(snippets.playerState());
  if (loaded.errors.length) throw new SetupError("the player could not load the recording", loaded);
  await cdp.evaluate(snippets.choosePlayerSpeed(SLOT.side, String(RATE)));
  await pressText(cdp, `${SLOT.side} .recording-player button`, "Play");
  try {
    await waitFor(cdp, `${playingIn("side", LEAD_SECONDS)} || Boolean(document.querySelector('.recording-player .error'))`, "the recording to play", 10_000);
  } catch (error) {
    throw new SetupError(error.message, await cdp.evaluate(snippets.playerState()));
  }
  const started = await cdp.evaluate(snippets.playerState());
  if (started.errors.length) throw new SetupError("the player refused to play", started);
  return { loaded, started };
}

/** One sample as a line a person can read. */
const traceLine = (sample, at) => `${Math.round(sample.t - at)}ms: ${sample.audios.length === 0 ? "no audio"
  : sample.audios.map((audio) => `${audio.slot} ${audio.paused ? "paused" : "playing"} rs${audio.readyState} ${audio.currentTime.toFixed(2)}s x${audio.playbackRate}`).join(", ")}`;

/**
 * Does `act` -- a move of the player -- and measures it from the sampler's
 * record: the position before, the first sample with the expected slot
 * playing, the longest stretch with nothing playing, how many `audio`
 * elements were ever on the page at once, and the player `SETTLE_MS` later.
 */
async function move(cdp, expected, act) {
  const before = await cdp.evaluate(snippets.playerState());
  const from = before.audios.find((audio) => playing(audio)) ?? null;
  const at = await cdp.evaluate("performance.now()");
  await act();
  await delay(SETTLE_MS);
  const samples = await cdp.evaluate(snippets.playbackSamples());
  const after = await cdp.evaluate(snippets.playerState());

  const anyPlaying = (sample) => sample.audios.some(playing);
  const later = samples.filter((sample) => sample.t >= at);
  let lastPlaying = samples.filter((sample) => sample.t < at && anyPlaying(sample)).at(-1)?.t ?? at;
  let longestSilence = 0;
  for (const sample of later) {
    if (!anyPlaying(sample)) continue;
    longestSilence = Math.max(longestSilence, sample.t - lastPlaying);
    lastPlaying = sample.t;
  }
  longestSilence = Math.max(longestSilence, (later.at(-1)?.t ?? at) - lastPlaying);
  const firstIndex = later.findIndex((sample) => sample.audios.some((audio) => audio.slot === expected && playing(audio)));
  const first = firstIndex === -1 ? null : later[firstIndex];
  const firstAudio = first?.audios.find((audio) => audio.slot === expected && playing(audio)) ?? null;
  const mostAudios = Math.max(0, ...later.map((sample) => sample.audios.length));
  const final = after.audios;
  const traceEnd = firstIndex === -1 ? 12 : firstIndex + 3;
  const measured = {
    expected,
    from: from && { slot: from.slot, time: round(from.currentTime), rate: from.playbackRate },
    firstPlaying: first && { afterMs: Math.round(first.t - at), time: round(firstAudio.currentTime) },
    longestSilenceMs: Math.round(longestSilence),
    mostAudios,
    samples: later.length,
    after: {
      audios: final.map((audio) => ({ slot: audio.slot, hidden: audio.hidden, paused: audio.paused, time: round(audio.currentTime), rate: audio.playbackRate, blob: audio.blob })),
      errors: after.errors,
      clocks: after.clocks,
    },
    trace: later.slice(0, traceEnd).map((sample) => traceLine(sample, at)),
  };
  const matches = from !== null
    && firstAudio !== null && firstAudio.currentTime >= from.currentTime - BEHIND_SECONDS
    && longestSilence < SILENCE_MS
    && mostAudios <= 1
    && final.length === 1 && final[0].slot === expected && !final[0].hidden
    && !final[0].paused && final[0].playbackRate === RATE
    && after.errors.length === 0;
  return { measured, matches };
}

const MOVE_EXPECTATION = [
  `after each move, within ${SETTLE_MS}ms: exactly one audio, in the expected slot and not hidden, not paused, at ${RATE}x;`,
  `the first sample with that slot playing is at or after the position before the move minus ${BEHIND_SECONDS}s;`,
  `under ${SILENCE_MS}ms with nothing playing; never two audio elements; no player error`,
].join(" ");

const SETUP_NOTE = "Each check starts from a fresh load at 1440x900 on Notes with note 101 playing from the transcript column at 1.5x, Play pressed with a real CDP pointer press. \"Playing\" is not paused with readyState >= 3. Samples are a 50ms timer in the page.";

export async function runNotePlayback(options) {
  const out = resolve(options.out);
  await mkdir(out, { recursive: true });
  let browser;
  let cdp;
  try {
    browser = await launchEdge({ ...options, out, extraArgs: EDGE_ARGS });
    cdp = await connectCdp(browser.target.webSocketDebuggerUrl);
    await cdp.send("Page.enable");
    const report = createReport({
      url: options.url,
      edgeVersion: browser.version.Browser ?? browser.version,
      edgeArgs: EDGE_ARGS,
      fixtureBannerRemoved: true,
      timestamp: new Date().toISOString(),
      measurementScope: "measured in headless Edge against dev fixtures, not in the Tauri window",
    });

    /** Runs one check; a setup or move that throws is the check measured false, with what was on the page. */
    const check = async ({ id, claim, expectation }, run) => {
      try {
        const result = await run();
        addCheck(report, { id, claim, expectation, note: SETUP_NOTE, ...result });
      } catch (error) {
        let player = error instanceof SetupError ? error.player : null;
        if (player === null) {
          try { player = await cdp.evaluate(snippets.playerState()); } catch { /* no page to read */ }
        }
        addCheck(report, { id, claim, expectation, measured: { failedAt: error.message, player }, matches: false, note: SETUP_NOTE });
      }
    };

    // 1. Notes -> Replay -> Notes: the column's player to Replay's and back.
    await check({
      id: "1",
      claim: "At 1440x900, Notes to Replay and back to Notes moves the playing recording from the transcript column to Replay's full player and back, still playing from where it was at 1.5x.",
      expectation: `Replay: slot full; Notes again: slot side. ${MOVE_EXPECTATION}`,
    }, async () => {
      const setup = await playingNote(cdp);
      await cdp.evaluate(snippets.startPlaybackSampler());
      const toReplay = await move(cdp, "full", () => pressText(cdp, '.note-detail [role="tab"]', "Replay"));
      const shot = "01-replay-playing-1440x900.png";
      await cdp.screenshot(resolve(out, shot));
      const toNotes = await move(cdp, "side", () => pressText(cdp, '.note-detail [role="tab"]', "Notes"));
      const started = setup.started.audios[0];
      return {
        measured: {
          recording: { duration: started?.duration ?? null, blob: started?.blob ?? null, clocks: setup.started.clocks },
          toReplay: toReplay.measured,
          toNotes: toNotes.measured,
        },
        matches: toReplay.matches && toNotes.matches,
        screenshots: [shot],
      };
    });

    // 2. On Notes, across the width and back: the column's player to the mini player and back.
    await check({
      id: "2",
      claim: "On Notes, narrowing the window from 1440 to 1000 and widening it again moves the playing recording from the transcript column to the mini player and back, still playing from where it was at 1.5x.",
      expectation: `1000x900: slot mini; 1440x900 again: slot side. ${MOVE_EXPECTATION}`,
    }, async () => {
      await playingNote(cdp);
      await cdp.evaluate(snippets.startPlaybackSampler());
      const narrow = await move(cdp, "mini", () => viewport(cdp, NARROW));
      const shot = "02-mini-player-1000x900.png";
      await cdp.screenshot(resolve(out, shot));
      const wide = await move(cdp, "side", () => viewport(cdp, WIDE));
      return { measured: { narrow: narrow.measured, wide: wide.measured }, matches: narrow.matches && wide.matches, screenshots: [shot] };
    });

    // 3. On Replay, across the width both ways: the full player stays where it is.
    await check({
      id: "3",
      claim: "On Replay, crossing the width both ways keeps the same audio element in Replay's full player, playing, its time never going back.",
      expectation: [
        "with the full player's audio tagged data-probe=kept after Replay is pressed: in every 50ms sample while the window goes to 1000x900 and back to 1440x900,",
        `exactly one audio, the tagged one, in slot full, not paused, at ${RATE}x, its time never lower than the sample before;`,
        "the note one grid track at 1000 and two at 1440 again (the width really crossed); more than 1s of the recording played; no player error",
      ].join(" "),
    }, async () => {
      await playingNote(cdp);
      await pressText(cdp, '.note-detail [role="tab"]', "Replay");
      await waitFor(cdp, playingIn("full"), "the recording to play in Replay");
      await cdp.evaluate(`(() => { document.querySelector(${JSON.stringify(`${SLOT.full} audio`)}).dataset.probe = 'kept'; return true; })()`);
      const at = await cdp.evaluate(snippets.startPlaybackSampler());
      await viewport(cdp, NARROW);
      await delay(1200);
      const narrowGrid = await cdp.evaluate(snippets.notesGrid());
      await viewport(cdp, WIDE);
      await delay(1200);
      const wideGrid = await cdp.evaluate(snippets.notesGrid());
      const samples = await cdp.evaluate(snippets.playbackSamples());
      const after = await cdp.evaluate(snippets.playerState());
      const kept = (sample) => sample.audios.length === 1 && sample.audios[0].probe === "kept"
        && sample.audios[0].slot === "full" && !sample.audios[0].paused && sample.audios[0].playbackRate === RATE;
      const times = samples.map((sample) => sample.audios[0]?.currentTime ?? null);
      const wentBack = times.findIndex((time, index) => index > 0 && (time === null || times[index - 1] === null || time < times[index - 1]));
      const first = times[0] ?? null;
      const last = times.at(-1) ?? null;
      const notKept = samples.filter((sample) => !kept(sample));
      return {
        measured: {
          samples: samples.length,
          notKept: notKept.length,
          firstNotKept: notKept[0] ? traceLine(notKept[0], at) : null,
          wentBackAt: wentBack === -1 ? null : traceLine(samples[wentBack], at),
          time: { first: first === null ? null : round(first), last: last === null ? null : round(last) },
          grid: { narrow: narrowGrid.tracks, wide: wideGrid.tracks },
          after: { audios: after.audios, errors: after.errors },
        },
        matches: samples.length > 0 && notKept.length === 0 && wentBack === -1
          && narrowGrid.tracks === 1 && wideGrid.tracks === 2
          && first !== null && last !== null && last - first > 1
          && after.errors.length === 0,
      };
    });

    // 4. Hide transcript, then Show transcript, while playing.
    await check({
      id: "4",
      claim: "At 1440x900 on Notes, Hide transcript and then Show transcript move the playing recording from the transcript column to the mini player and back, still playing from where it was at 1.5x.",
      expectation: `hidden: slot mini; shown: slot side. ${MOVE_EXPECTATION}`,
    }, async () => {
      await playingNote(cdp);
      await cdp.evaluate(snippets.startPlaybackSampler());
      const hidden = await move(cdp, "mini", () => cdp.evaluate(snippets.pressTranscriptToggle()));
      const shot = "04-column-hidden-1440x900.png";
      await cdp.screenshot(resolve(out, shot));
      const shown = await move(cdp, "side", () => cdp.evaluate(snippets.pressTranscriptToggle()));
      return { measured: { hidden: hidden.measured, shown: shown.measured }, matches: hidden.matches && shown.matches, screenshots: [shot] };
    });

    const markdown = renderMarkdown(report);
    await writeFile(resolve(out, "report.json"), `${JSON.stringify(report, null, 2)}\n`);
    await writeFile(resolve(out, "report.md"), markdown);
    process.stdout.write(markdown);
    return report;
  } finally {
    try {
      await cdp?.close();
    } finally {
      await killBrowser(browser?.child);
    }
  }
}

if (isMain) {
  try {
    const options = parseShellArgs(process.argv.slice(2));
    if (options.help) process.stdout.write(`${shellUsage().replace("shell-rail.mjs", "note-playback.mjs")}\n`);
    else {
      const failed = failedChecks(await runNotePlayback(options));
      // The run completed; a check measured false still fails it, so a wrapper or CI step sees it.
      if (failed.length) {
        console.error(`note-playback: ${failed.length} check(s) false: ${failed.join(", ")}`);
        process.exitCode = 1;
      }
    }
  } catch (error) {
    console.error(`note-playback failed: ${error.message}`);
    process.exitCode = 1;
  }
}
