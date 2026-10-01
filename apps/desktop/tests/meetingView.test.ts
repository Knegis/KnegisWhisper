import { test } from "node:test";
import assert from "node:assert/strict";
import {
  activeCall,
  elapsedLabel,
  interruptedTitle,
  liveElapsed,
  meetingLanguageMenu,
  recentMeta,
  recordedSources,
  recordingStartedAt,
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

test("meetingLanguageMenu with no provider greys nothing", () => {
  assert.equal(meetingLanguageMenu("en", null).every((o) => !o.disabled), true);
});

test("meetingLanguageMenu lists a saved value it does not know as itself, so the tick is never missing", () => {
  const menu = meetingLanguageMenu("sv-SE", DETECTS);
  assert.deepEqual(menu.filter((o) => o.current).map((o) => [o.id, o.label]), [["sv-SE", "sv-SE"]]);
  assert.equal(menu.length, 10);
});
