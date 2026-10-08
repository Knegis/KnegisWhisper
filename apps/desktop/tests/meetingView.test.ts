import { test } from "node:test";
import assert from "node:assert/strict";
import {
  activeCall,
  discardQuestion,
  elapsedLabel,
  folderMenu,
  interruptedTitle,
  liveElapsed,
  liveMicrophone,
  meetingLanguageMenu,
  meetingIntro,
  meetingMicrophone,
  MICROPHONE_CHECK_LISTENING,
  microphoneCheckLine,
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
import type { Detected, Folder, MicrophoneCheck, SummarySettings } from "../src/generated/ipc-types.js";

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
  const mic = { microphoneName: "Microphone Array (Realtek(R) Audio)" };
  assert.equal(recordedSources({ ...mic, microphone: true, systemAudio: true, systemAudioReason: null }), "Your microphone and the other participants");
  assert.equal(recordedSources({ ...mic, microphone: true, systemAudio: false, systemAudioReason: null }), "Your microphone only");
  assert.equal(
    recordedSources({ ...mic, microphone: true, systemAudio: false, systemAudioReason: "system audio was not requested" }),
    "Your microphone only — system audio was not requested",
  );
  assert.equal(recordedSources({ microphone: false, microphoneName: null, systemAudio: false, systemAudioReason: null }), "Nothing is being recorded");
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

/* -------------- the microphone a meeting records (KNEG-341), chosen here (KNEG-364) */

const MICROPHONES = [
  { name: "Microphone Array (Realtek(R) Audio)", isDefault: true },
  { name: "Headset (Jabra Evolve2 65)", isDefault: false },
];
const SHARED = "Meetings and dictation use the same microphone. Choosing one here changes it for dictation too.";
const HEADING = "Used by meetings and dictation";

test("meetingMicrophone: the chip is a menu of Dictation's rows under one heading, and says the choice is shared", () => {
  const microphone = meetingMicrophone(MICROPHONES, null);
  assert.equal(microphone.label, "System default");
  assert.equal(microphone.title, SHARED);
  assert.equal(microphone.missing, null);
  assert.deepEqual(
    microphone.options.map((o) => [o.group, o.id, o.label, o.current]),
    [
      [HEADING, "", "System default", true],
      [HEADING, "Microphone Array (Realtek(R) Audio)", "Microphone Array (Realtek(R) Audio) (default)", false],
      [HEADING, "Headset (Jabra Evolve2 65)", "Headset (Jabra Evolve2 65)", false],
    ],
  );
  // Nothing says "chosen on Dictation" any more: it is chosen here too.
  assert.ok(!microphone.title.includes("chosen on Dictation"), microphone.title);
});

test("meetingMicrophone: a chosen microphone that is connected is named and current", () => {
  const microphone = meetingMicrophone(MICROPHONES, "Headset (Jabra Evolve2 65)");
  assert.equal(microphone.label, "Headset (Jabra Evolve2 65)");
  assert.equal(microphone.title, SHARED);
  assert.deepEqual(microphone.options.filter((o) => o.current).map((o) => o.id), ["Headset (Jabra Evolve2 65)"]);
});

test("meetingMicrophone: a chosen microphone that is unplugged is listed as itself, not connected", () => {
  const microphone = meetingMicrophone(MICROPHONES, "USB Mic");
  assert.equal(microphone.label, "USB Mic (not connected)");
  assert.equal(microphone.missing, "USB Mic");
  assert.deepEqual(microphone.options[0], {
    id: "USB Mic", label: "USB Mic (not connected)", group: HEADING, current: true, disabled: false, reason: null,
  });
});

test("meetingMicrophone: before the device list arrives, a choice is named rather than called unplugged", () => {
  const early = meetingMicrophone(null, "USB Mic");
  assert.equal(early.label, "USB Mic");
  assert.equal(early.missing, null);
  assert.deepEqual(early.options.map((o) => [o.id, o.current]), [["", false], ["USB Mic", true]]);
  assert.equal(meetingMicrophone(null, null).label, "System default");
});

test("liveMicrophone: the live chip names the microphone the meeting opened, not the one chosen now", () => {
  const sources = { microphone: true, microphoneName: "Headset (Jabra Evolve2 65)", systemAudio: false, systemAudioReason: null };
  assert.deepEqual(liveMicrophone(sources), {
    label: "Headset (Jabra Evolve2 65)",
    title: "This meeting is recording from Headset (Jabra Evolve2 65). A microphone chosen now is used from the next recording.",
  });
  // A device that gave no name, and sources not read yet, are said without inventing one.
  const unnamed = liveMicrophone({ ...sources, microphoneName: null });
  assert.equal(unnamed.label, "Microphone");
  assert.equal(unnamed.title, "This meeting is recording from your microphone. A microphone chosen now is used from the next recording.");
  assert.deepEqual(liveMicrophone(null), unnamed);
});

/* ------------------------------------------- the microphone check (KNEG-364) */

const check = (over: Partial<MicrophoneCheck>): MicrophoneCheck => ({
  device: "Headset (Jabra Evolve2 65)", fellBack: null, heard: true, peakLevel: 0.6, ...over,
});

test("microphoneCheckLine: heard, and heard nothing, each name the microphone tested", () => {
  assert.equal(microphoneCheckLine(check({})), "Heard you on Headset (Jabra Evolve2 65).");
  assert.equal(
    microphoneCheckLine(check({ heard: false, peakLevel: 0 })),
    "Heard nothing from Headset (Jabra Evolve2 65). Check it isn't muted, or choose another microphone.",
  );
});

test("microphoneCheckLine: a choice that wasn't connected says what was tested instead, first", () => {
  const fellBack = check({ device: "Microphone Array (Realtek(R) Audio)", fellBack: "USB Mic" });
  assert.equal(
    microphoneCheckLine(fellBack),
    "USB Mic isn't connected, so this tested Microphone Array (Realtek(R) Audio). Heard you on Microphone Array (Realtek(R) Audio).",
  );
  assert.equal(
    microphoneCheckLine({ ...fellBack, heard: false }),
    "USB Mic isn't connected, so this tested Microphone Array (Realtek(R) Audio). Heard nothing from Microphone Array (Realtek(R) Audio). Check it isn't muted, or choose another microphone.",
  );
});

test("microphoneCheckLine: a device with no name is the system default, which is what opens then", () => {
  assert.equal(microphoneCheckLine(check({ device: null })), "Heard you on the system default microphone.");
  assert.equal(
    microphoneCheckLine(check({ device: null, fellBack: "USB Mic", heard: false })),
    "USB Mic isn't connected, so this tested the system default microphone. Heard nothing from the system default microphone. Check it isn't muted, or choose another microphone.",
  );
});

test("the check's running line says how long, what to do, and that nothing is recorded", () => {
  assert.equal(MICROPHONE_CHECK_LISTENING, "Listening for 5 seconds — say something. Nothing is recorded.");
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

// KNEG-365: the Meetings page's Folder chip. Its menu is "No folder" -- the
// word Notes' own Move to folder uses -- then every folder in Notes' order;
// the chip says where the note will go.
const FOLDERS: Folder[] = [
  { id: 1, name: "Inbox", isDefault: true, sortOrder: 0 },
  { id: 2, name: "Customer calls", isDefault: false, sortOrder: 1 },
  { id: 3, name: "Internal", isDefault: false, sortOrder: 2 },
];

test("folderMenu offers no folder, then every folder, ticking the one chosen", () => {
  const menu = folderMenu(FOLDERS, 3);
  assert.equal(menu.label, "Save to Internal");
  // The folder Start sends: the one the chip names.
  assert.equal(menu.folderId, 3);
  assert.deepEqual(
    menu.options.map((option) => [option.id, option.label, option.current]),
    [["", "No folder", false], ["1", "Inbox", false], ["2", "Customer calls", false], ["3", "Internal", true]],
  );
  assert.ok(menu.options.every((option) => option.group === null && !option.disabled && option.reason === null));
});

test("folderMenu with no folder chosen saves to Notes", () => {
  const menu = folderMenu(FOLDERS, null);
  assert.equal(menu.label, "Save to Notes");
  assert.equal(menu.folderId, null);
  assert.deepEqual(menu.options.filter((option) => option.current).map((option) => option.id), [""]);
  // With no folders at all, the one row is still there to say where it goes.
  assert.deepEqual(folderMenu([], null), {
    label: "Save to Notes",
    folderId: null,
    options: [{ id: "", label: "No folder", group: null, current: true, disabled: false, reason: null }],
  });
});

test("folderMenu treats a chosen folder that is gone as no folder", () => {
  // Deleted in Notes while it was chosen here: the chip must not name it.
  const menu = folderMenu(FOLDERS.filter((folder) => folder.id !== 3), 3);
  assert.equal(menu.label, "Save to Notes");
  // And Start sends no folder, as the chip says, rather than the stale id.
  assert.equal(menu.folderId, null);
  assert.deepEqual(menu.options.filter((option) => option.current).map((option) => option.id), [""]);
});
