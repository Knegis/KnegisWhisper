import { test } from "node:test";
import assert from "node:assert/strict";
import type { CandidateEvent, ProviderFreshness, UpcomingMeetings } from "../src/generated/ipc-types.js";
import { calendarChipText, calendarNoticeLabels, calendarProviderLabel, eventTitle, freshnessTime, providerFreshness, startCalendarChoice, upcomingEmpty, upcomingEmptyLabels, upcomingTime } from "../src/lib/upcoming.js";

// Production formats in the OS local zone. Construct every test date locally
// before serialising to UTC, so the assertions also pass outside Europe.
const now = new Date(2026, 10, 9, 9, 0);
const iso = (day: number, hour: number, minute = 0) => new Date(2026, 10, day, hour, minute).toISOString();
const event: CandidateEvent = { id: "a", provider: "google", calendarId: "work", title: "Planning", start: iso(9, 8), end: iso(9, 10), meetingLike: true };
const provider: ProviderFreshness = { provider: "microsoft", connected: true, selectedCalendars: 1, lastAttemptAt: null, lastSuccessAt: null, lastOutcome: null };
const data: UpcomingMeetings = { events: [], hasMore: false, providers: [provider] };

test("upcoming times use local Now, Today, Tomorrow and weekday dates", () => {
  assert.equal(upcomingTime(event, now), "Now");
  assert.equal(upcomingTime({ ...event, start: iso(9, 9, 30) }, now), "Today 09:30");
  assert.equal(upcomingTime({ ...event, start: iso(10, 14, 30), end: iso(10, 15) }, now), "Tomorrow 14:30");
  assert.equal(upcomingTime({ ...event, start: iso(11, 10), end: iso(11, 11) }, now), "Wed 11 Nov 10:00");
  // The end is exclusive: an ended event must not still read Now.
  assert.equal(upcomingTime({ ...event, end: now.toISOString() }, now), "Today 08:00");
  const midnight = new Date(2026, 11, 31, 23, 59);
  assert.equal(upcomingTime({ start: new Date(2027, 0, 1, 0, 5).toISOString(), end: new Date(2027, 0, 1, 1).toISOString() }, midnight), "Tomorrow 00:05");
});

test("freshness wording covers minute/hour boundaries and dates", () => {
  const ago = (seconds: number) => new Date(now.getTime() - seconds * 1000).toISOString();
  assert.equal(freshnessTime(ago(59), now), "just now");
  assert.equal(freshnessTime(ago(60), now), "1 min ago");
  assert.equal(freshnessTime(ago(180), now), "3 min ago");
  assert.equal(freshnessTime(ago(3600), now), "1 h ago");
  assert.equal(freshnessTime(iso(7, 9), now), "7 Nov 2026");
  assert.equal(freshnessTime(ago(-60), now), "just now");
  assert.deepEqual(providerFreshness(provider, now), { text: "Outlook: not synced yet", settings: false });
  const success = { ...provider, lastSuccessAt: ago(180) };
  assert.equal(providerFreshness(success, now).text, "Outlook synced 3 min ago");
  const failures: ProviderFreshness["lastOutcome"][] = [
    { state: "failed", problem: null, message: "safe" },
    { state: "completed", calendarErrors: ["calendar"], eventsUpserted: 1, eventsRemoved: 0 },
  ];
  for (const lastOutcome of failures) {
    assert.deepEqual(providerFreshness({ ...success, provider: "google", lastOutcome }, now), { text: "Google Calendar: couldn't sync, see Settings", settings: true });
  }
  assert.equal(providerFreshness({ ...success, lastOutcome: { state: "completed", calendarErrors: [], eventsUpserted: 0, eventsRemoved: 0 } }, now).text, "Outlook synced 3 min ago");
  assert.deepEqual(providerFreshness({ ...success, provider: "google", lastOutcome: { state: "token-unavailable", problem: null, message: "safe" } }, now), { text: "Google Calendar: reconnect in Settings", settings: true });
});

test("empty-state selection distinguishes connection, selection and empty week", () => {
  assert.equal(upcomingEmpty({ ...data, providers: [] }), "not-connected");
  assert.equal(upcomingEmpty({ ...data, providers: [{ ...provider, connected: false }] }), "not-connected");
  assert.equal(upcomingEmpty({ ...data, providers: [{ ...provider, selectedCalendars: 0 }] }), "no-calendars");
  assert.equal(upcomingEmpty(data), "no-events");
  assert.equal(upcomingEmpty({ ...data, events: [event] }), null);
  assert.equal(upcomingEmpty({ ...data, providers: [{ ...provider, selectedCalendars: 0 }, { ...provider, provider: "google" }] }), "no-events");
  assert.deepEqual(upcomingEmptyLabels, { "not-connected": "Connect a calendar to see upcoming meetings here.", "no-calendars": "Choose which calendars to use in Settings.", "no-events": "Nothing on your calendar this week." });
});

test("notice labels and chip text retain the deliberate product wording", () => {
  assert.deepEqual(calendarNoticeLabels, {
    overlap: "Several meetings overlap, so this recording isn't linked.",
    "choice-unavailable": "The event you chose isn't happening now, so this recording isn't linked.",
    "calendar-unreadable": "Your calendar couldn't be read, so this recording isn't linked.",
    "not-saved": "The calendar link couldn't be saved with the recording, so it isn't linked.",
  });
  assert.equal(calendarChipText(event, now), "For: Planning · Now");
  assert.equal(calendarChipText({ ...event, title: null }, now), "For: Untitled event · Now");
  assert.equal(eventTitle({ title: "  " }), "Untitled event");
  assert.equal(calendarProviderLabel("microsoft"), "Outlook");
  assert.equal(calendarProviderLabel("google"), "Google Calendar");
});

test("Start resolves fresh Auto, preserves explicit event/none, and omits empty candidates", () => {
  const candidates = { events: [event], auto: event.id };
  assert.deepEqual(startCalendarChoice(candidates, null), { mode: "event", eventId: "a" });
  assert.deepEqual(startCalendarChoice(candidates, { mode: "event", eventId: "solo" }), { mode: "event", eventId: "solo" });
  assert.deepEqual(startCalendarChoice(candidates, { mode: "none" }), { mode: "none" });
  assert.equal(startCalendarChoice({ events: [], auto: null }, null), undefined);
  assert.deepEqual(startCalendarChoice({ events: [], auto: null }, { mode: "none" }), { mode: "none" });
  assert.deepEqual(startCalendarChoice({ events: [event], auto: null }, null), { mode: "auto" });
});
