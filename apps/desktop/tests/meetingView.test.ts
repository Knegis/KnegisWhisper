import { test } from "node:test";
import assert from "node:assert/strict";
import {
  activeCall,
  discardQuestion,
  elapsedLabel,
  interruptedTitle,
  liveElapsed,
  meetingLanguageMenu,
  meetingIntro,
  meetingMicrophone,
  privacyProvider,
  reviewIsFor,
  recentMeta,
  recordedSources,
  recordingStartedAt,
  sameMeetingStart,
  summaryModelName,
  summaryPrivacyLine,
} from "../src/lib/meetingView.js";
import { timeOfDay } from "../src/lib/dayGroups.js";
import type { Detected, SummarySettings } from "../src/generated/ipc-types.js";

// The dev fixtures' pair (`lib/devFixtures.ts`): Teams open and idle, Zoom in a call.
const TEAMS: Detected = { provider: "meeting-detect:teams", name: "Microsoft Teams", evidence: "ms-teams.exe", inCall: false, signal: "none" };
const ZOOM: Detected = { provider: "meeting-detect:zoom", name: "Zoom", evidence: "CptHost.exe", inCall: true, signal: "call-process" };

test("activeCall is the first application in a call, and nothing when none is", () => {
  assert.equal(activeCall([TEAMS, ZOOM]), ZOOM);
  assert.equal(activeCall([]), null);
  assert.equal(activeCall([TEAMS]), null);
});

test("recordingStartedAt reads the UTC time Rust writes into a storage key", () => {
  // `meeting.rs` names a recording `%Y%m%d-%H%M%S` in UTC, then a UUID.
  assert.equal(recordingStartedAt("20260901-101500-3f2a.wav")?.getTime(), Date.UTC(2026, 8, 1, 10, 15, 0));
  // Keys written before the UUID was added still parse.
  assert.equal(recordingStartedAt("20260902-141530.wav")?.getTime(), Date.UTC(2026, 8, 2, 14, 15, 30));
});

test("recordingStartedAt is null for a key it cannot read", () => {
  assert.equal(recordingStartedAt("orphan.wav"), null);
  assert.equal(recordingStartedAt("20260901-101500-3f2a.mp3"), null);
  assert.equal(recordingStartedAt("2026091-101500.wav"), null);
  // Digits in the right places that are not a date.
  assert.equal(recordingStartedAt("20261301-101500.wav"), null);
  assert.equal(recordingStartedAt("20260231-101500.wav"), null);
  assert.equal(recordingStartedAt("20260901-251500.wav"), null);
});

test("elapsedLabel is hh:mm:ss, whole seconds, never negative", () => {
  assert.equal(elapsedLabel(0), "00:00:00");
  assert.equal(elapsedLabel(768), "00:12:48");
  assert.equal(elapsedLabel(3600), "01:00:00");
  assert.equal(elapsedLabel(768.9), "00:12:48");
  assert.equal(elapsedLabel(-5), "00:00:00");
});

test("liveElapsed counts from the start, however seldom the clock ticks", () => {
  const start = new Date(Date.UTC(2026, 8, 30, 10, 0, 0));
  const later = (ms: number) => new Date(start.getTime() + ms);
  assert.equal(liveElapsed(start, later(65_000), null), 65);
  // A window the system throttles in the background may tick once a minute
  // or less; ten minutes in, the clock still reads ten minutes, not the
  // number of ticks that arrived.
  assert.equal(liveElapsed(start, later(600_000), null), 600);
  assert.equal(liveElapsed(start, later(1_500), null), 1.5);
});

test("liveElapsed holds at the moment the meeting stalled", () => {
  const start = new Date(Date.UTC(2026, 8, 30, 10, 0, 0));
  const stalledAt = new Date(start.getTime() + 90_000);
  assert.equal(liveElapsed(start, new Date(start.getTime() + 300_000), stalledAt), 90);
  assert.equal(liveElapsed(start, new Date(start.getTime() + 90_000), stalledAt), 90);
});

test("liveElapsed is never negative", () => {
  const start = new Date(Date.UTC(2026, 8, 30, 10, 0, 0));
  assert.equal(liveElapsed(start, new Date(start.getTime() - 5_000), null), 0);
});

test("recordedSources says which captures opened, and why the second did not", () => {
  assert.equal(recordedSources({ microphone: true, systemAudio: true, systemAudioReason: null }), "Your microphone and the other participants");
  assert.equal(recordedSources({ microphone: true, systemAudio: false, systemAudioReason: null }), "Your microphone only");
  assert.equal(
    recordedSources({ microphone: true, systemAudio: false, systemAudioReason: "system audio was not requested" }),
    "Your microphone only — system audio was not requested",
  );
  assert.equal(recordedSources({ microphone: false, systemAudio: false, systemAudioReason: null }), "Nothing is being recorded");
  assert.equal(recordedSources(null), null);
});

const SUMMARIES: SummarySettings = {
  enabled: true,
  provider: "openai",
  model: "gpt-4o-mini",
  selfHostedUrl: null,
  enterprise: false,
  language: "match",
  contextBudget: 8192,
};

test("summaryPrivacyLine names where an automatic summary sends the transcript", () => {
  assert.equal(summaryPrivacyLine(SUMMARIES), "The transcript is sent to OpenAI for a summary");
  assert.equal(
    summaryPrivacyLine({ ...SUMMARIES, provider: "bedrock", enterprise: true }),
    "The transcript is sent to AWS Bedrock for a summary",
  );
  assert.equal(
    summaryPrivacyLine({ ...SUMMARIES, provider: "self-hosted", selfHostedUrl: "https://llm.example/v1" }),
    "The transcript is sent to your self-hosted server for a summary",
  );
  // `resolve_chat` falls back to OpenAI for a blank provider once validation
  // has let it through, which it does when a self-hosted URL field exists.
  assert.equal(
    summaryPrivacyLine({ ...SUMMARIES, provider: "", selfHostedUrl: "" }),
    "The transcript is sent to OpenAI for a summary",
  );
});

test("summaryPrivacyLine says nothing when no summary leaves this computer", () => {
  assert.equal(summaryPrivacyLine(null), null);
  assert.equal(summaryPrivacyLine({ ...SUMMARIES, enabled: false }), null);
  assert.equal(summaryPrivacyLine({ ...SUMMARIES, provider: "local" }), null);
  assert.equal(summaryPrivacyLine({ ...SUMMARIES, provider: "lan" }), null);
  // Settings Rust refuses before anything is sent (`SummarySettings::validate`).
  assert.equal(summaryPrivacyLine({ ...SUMMARIES, model: "  " }), null);
  assert.equal(summaryPrivacyLine({ ...SUMMARIES, provider: "" }), null);
});

test("recentMeta is the day and the length, and leaves out what it does not know", () => {
  const now = new Date(2026, 8, 29, 12, 0, 0);
  const withBoth = recentMeta("2026-08-26T09:00:00Z", 1847, now);
  assert.match(withBoth, /^.+ · 31 min$/);
  assert.doesNotMatch(withBoth, /2026/);
  assert.doesNotMatch(recentMeta("2026-08-26T09:00:00Z", null, now), /·|min/);
  assert.equal(recentMeta(null, 2612, now), "44 min");
  assert.equal(recentMeta(null, 20, now), "1 min");
  assert.equal(recentMeta(null, null, now), "");
  // Another year says which.
  assert.match(recentMeta("2025-08-26 09:00:00", null, now), /2025/);
});

test("interruptedTitle is when the recording started, or a plain name when that is unreadable", () => {
  const now = new Date(2026, 8, 29, 12, 0, 0);
  const started = new Date(Date.UTC(2026, 8, 1, 10, 15, 0));
  const title = interruptedTitle("20260901-101500-3f2a.wav", now);
  assert.ok(title.startsWith("Recording from "), title);
  assert.ok(title.endsWith(`, ${timeOfDay(started)}`), title);
  assert.equal(interruptedTitle("orphan.wav", now), "Interrupted recording");
});

const DETECTS = { name: "Deepgram", languageDetection: true };
const NEEDS_LANGUAGE = { name: "OpenAI Realtime", languageDetection: false };

test("meetingLanguageMenu lists Detect language first, then the eight languages, and ticks the current one", () => {
  const menu = meetingLanguageMenu("sv", DETECTS);
  assert.equal(menu.length, 9);
  assert.deepEqual(menu[0], { id: "", label: "Detect language", group: null, current: false, disabled: false, reason: null });
  assert.deepEqual(menu.filter((o) => o.current).map((o) => o.id), ["sv"]);
  assert.equal(menu.find((o) => o.id === "sv")?.label, "Swedish");
});

test("meetingLanguageMenu ticks Detect language when no language is saved, ignoring blanks", () => {
  assert.deepEqual(meetingLanguageMenu("  ", DETECTS).filter((o) => o.current).map((o) => o.id), [""]);
});

test("meetingLanguageMenu greys Detect language, with the reason, for a provider that cannot detect", () => {
  const detect = meetingLanguageMenu("en", NEEDS_LANGUAGE)[0];
  assert.equal(detect?.disabled, true);
  assert.equal(detect?.reason, "OpenAI Realtime can't detect the language. Choose one.");
  assert.equal(meetingLanguageMenu("en", DETECTS)[0]?.disabled, false);
});

test("reviewIsFor is true only for the inputs the verdict was asked about", () => {
  const asked = { provider: "meeting:deepgram", language: "sv", keytermsText: "a\nb" };
  assert.equal(reviewIsFor(asked, "meeting:deepgram", "sv", "a\nb"), true);
  assert.equal(reviewIsFor(asked, "meeting:openai-realtime", "sv", "a\nb"), false);
  assert.equal(reviewIsFor(asked, "meeting:deepgram", "", "a\nb"), false);
  assert.equal(reviewIsFor(asked, "meeting:deepgram", "sv", "a"), false);
});

test("reviewIsFor: no verdict yet is stale, and no provider needs none", () => {
  assert.equal(reviewIsFor(null, "meeting:deepgram", "sv", ""), false);
  assert.equal(reviewIsFor(null, null, "sv", ""), true);
});

test("meetingLanguageMenu with no provider greys nothing", () => {
  assert.equal(meetingLanguageMenu("en", null).every((o) => !o.disabled), true);
});

test("meetingLanguageMenu lists a saved value it does not know as itself, so the tick is never missing", () => {
  const menu = meetingLanguageMenu("sv-SE", DETECTS);
  assert.deepEqual(menu.filter((o) => o.current).map((o) => [o.id, o.label]), [["sv-SE", "sv-SE"]]);
  assert.equal(menu.length, 10);
});

/* ------------------------------- the microphone a meeting records (KNEG-341) */

const MICROPHONES = [
  { name: "Microphone Array (Realtek(R) Audio)", isDefault: true },
  { name: "Headset (Jabra Evolve2 65)", isDefault: false },
];

test("meetingMicrophone: nothing chosen is the system default", () => {
  assert.deepEqual(meetingMicrophone(MICROPHONES, null), {
    label: "Default microphone",
    title: "Meetings record the system default microphone. Dictation and meetings use the same microphone, chosen on Dictation.",
  });
});

test("meetingMicrophone: a chosen microphone that is connected is named", () => {
  assert.deepEqual(meetingMicrophone(MICROPHONES, "Headset (Jabra Evolve2 65)"), {
    label: "Headset (Jabra Evolve2 65)",
    title: "Meetings record Headset (Jabra Evolve2 65). Dictation and meetings use the same microphone, chosen on Dictation.",
  });
});

test("meetingMicrophone: a chosen microphone that is unplugged says the default is recorded instead", () => {
  assert.deepEqual(meetingMicrophone(MICROPHONES, "USB Mic"), {
    label: "USB Mic (not connected)",
    title: "USB Mic isn't connected, so meetings record the system default microphone until it is.",
  });
});

test("meetingMicrophone: before the device list arrives, a choice is named rather than called unplugged", () => {
  assert.equal(meetingMicrophone(null, "USB Mic").label, "USB Mic");
  assert.equal(meetingMicrophone(null, null).label, "Default microphone");
});

test("the hero promises a summary only while automatic summaries are on", () => {
  // KNEG-377: R19's rule had no committed test. With them off, a summary is
  // something to ask for in Notes, not something you get.
  assert.equal(meetingIntro(true), "You get a transcript and a summary, saved to Notes.");
  assert.equal(meetingIntro(false), "You get a transcript, saved to Notes.");
});

test("while Start waits on a saved provider that can't be used, the privacy line names no provider", () => {
  // KNEG-354, item 1. The hero said the chosen provider can't be used while
  // the privacy line said audio is sent to the fallback: two answers. Nothing
  // is sent until somebody chooses.
  assert.equal(privacyProvider(null, "fallback", true), null);
  assert.equal(privacyProvider(null, "fallback", false), "fallback");
  assert.equal(privacyProvider(null, null, false), null);
  // A meeting that is running says what it is running with, whatever is saved.
  assert.equal(privacyProvider("running", "fallback", true), "running");
});

// KNEG-367: a summary names the model that wrote it. What is stored is the id
// the settings chose; a model the registry lists is named as the menus name
// it, and anything else -- a deployment name, a typed id -- is shown as stored.
test("a summary's model is named as the registry names it, or shown as stored", () => {
  assert.equal(summaryModelName("qwen3.5-9b-q4_k_m"), "Qwen3.5 9B");
  assert.equal(summaryModelName("claude-sonnet-5"), "Claude Sonnet 5");
  assert.equal(summaryModelName("my-azure-deployment"), "my-azure-deployment");
  assert.equal(summaryModelName(null), null);
});

// KNEG-394: Discard asks first, with what is about to be lost. The length is
// rounded as the saved outcome rounds it ("Saved as a note (38 min)."), and
// the notes sentence holds whatever was typed: Rust keeps notes as a note
// when there are any and has nothing to keep when there are none.
test("Discard's question names the recording's length and what happens to typed notes", () => {
  assert.equal(
    discardQuestion(38 * 60 + 10),
    "Discard this meeting? The recording so far (38 min) will be deleted and can't be recovered. Any notes you typed are kept as a separate note.",
  );
  assert.match(discardQuestion(90), /\(2 min\)/);
  assert.match(discardQuestion(42), /\(under a minute\)/);
  assert.match(discardQuestion(0), /\(under a minute\)/);
});

// KNEG-418: "Discard anyway" asks after the meeting window didn't confirm it
// saved the notes. Rust still keeps what it already holds (`cancel` takes
// `notes`), so the question says so, and says the newest edit may not be
// among it -- the one sentence that changes from Discard's.
test("Discard anyway's question says the latest edit may be missing", () => {
  assert.equal(
    discardQuestion(38 * 60 + 10, true),
    "Discard this meeting? The recording so far (38 min) will be deleted and can't be recovered. Notes the app already has are kept as a separate note; your latest edit may be missing.",
  );
  assert.match(discardQuestion(42, true), /\(under a minute\)/);
  // The default is Discard's own question, unchanged.
  assert.equal(discardQuestion(90, false), discardQuestion(90));
});

// KNEG-394's guard, out of `Meetings.tsx`: the meeting asked about is the one
// still running only when the start Rust reports after the question is within
// 2 s of the start it reported before. `elapsedSeconds` counts from the start,
// so the same meeting gives the same start on every read.
test("sameMeetingStart holds within 2 s either way, and not a millisecond past", () => {
  const began = 1_000_000;
  const tenMinutesIn = { elapsedSeconds: 600 };
  assert.equal(sameMeetingStart(began, tenMinutesIn, 1_600_000), true);
  // Read 2 s later than the count says: a start 2 s after the one asked about.
  assert.equal(sameMeetingStart(began, tenMinutesIn, 1_602_000), true);
  assert.equal(sameMeetingStart(began, tenMinutesIn, 1_602_001), false);
  // And 2 s earlier.
  assert.equal(sameMeetingStart(began, tenMinutesIn, 1_598_000), true);
  assert.equal(sameMeetingStart(began, tenMinutesIn, 1_597_999), false);
});

test("sameMeetingStart: a meeting started while the question was open is another meeting", () => {
  // Ten minutes in before the question, one second in after it.
  assert.equal(sameMeetingStart(1_000_000, { elapsedSeconds: 1 }, 1_600_000), false);
});

test("sameMeetingStart takes another tolerance", () => {
  assert.equal(sameMeetingStart(1_000_000, { elapsedSeconds: 600 }, 1_600_500, 500), true);
  assert.equal(sameMeetingStart(1_000_000, { elapsedSeconds: 600 }, 1_600_501, 500), false);
});
