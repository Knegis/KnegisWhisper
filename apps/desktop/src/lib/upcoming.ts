/** Calendar presentation stays pure: the views supply cache snapshots and now,
 * so labels share one vocabulary without performing syncs or reading settings. */
import type { CalendarCandidates, CalendarLinkChoice, CalendarLinkNotice, CalendarProvider, CandidateEvent, ProviderFreshness, UpcomingMeetings } from "../generated/ipc-types.js";

export const calendarProviderLabel = (provider: CalendarProvider): string => provider === "google" ? "Google Calendar" : "Outlook";
export const eventTitle = (event: { title: string | null }): string => event.title?.trim() || "Untitled event";

/** Use local calendar days, rather than 24-hour offsets, so tomorrow also
 * works across daylight-saving changes. `now` is supplied by the caller. */
export function upcomingTime(event: { start: string; end: string }, now: Date): string {
  const start = new Date(event.start);
  if (start <= now && now < new Date(event.end)) return "Now";
  const day = (date: Date) => new Date(date.getFullYear(), date.getMonth(), date.getDate()).getTime();
  const tomorrow = new Date(now.getFullYear(), now.getMonth(), now.getDate() + 1);
  const time = start.toLocaleTimeString("en-GB", { hour: "2-digit", minute: "2-digit", hour12: false });
  const label = day(start) === day(now) ? "Today" : day(start) === day(tomorrow) ? "Tomorrow"
    : `${start.toLocaleDateString("en-GB", { weekday: "short" })} ${start.getDate()} ${start.toLocaleDateString("en-GB", { month: "short" })}`;
  return `${label} ${time}`;
}

export function freshnessTime(at: string, now: Date): string {
  const minutes = Math.max(0, Math.floor((now.getTime() - new Date(at).getTime()) / 60_000));
  if (minutes < 1) return "just now";
  if (minutes < 60) return `${minutes} min ago`;
  if (minutes < 1440) return `${Math.floor(minutes / 60)} h ago`;
  return new Date(at).toLocaleDateString("en-GB", { day: "numeric", month: "short", year: "numeric" });
}

/** Partial sync failures keep cached rows visible but must never claim a
 * successful sync. No provider error details need to enter these labels. */
export function providerFreshness(provider: ProviderFreshness, now: Date): { text: string; settings: boolean } {
  const name = calendarProviderLabel(provider.provider);
  const outcome = provider.lastOutcome;
  if (outcome?.state === "token-unavailable") return { text: `${name}: reconnect in Settings`, settings: true };
  if (outcome?.state === "failed" || (outcome?.state === "completed" && outcome.calendarErrors.length > 0)) {
    return { text: `${name}: couldn't sync, see Settings`, settings: true };
  }
  return { text: provider.lastSuccessAt ? `${name} synced ${freshnessTime(provider.lastSuccessAt, now)}` : `${name}: not synced yet`, settings: false };
}

export type UpcomingEmpty = "not-connected" | "no-calendars" | "no-events" | null;
export function upcomingEmpty(data: UpcomingMeetings): UpcomingEmpty {
  const connected = data.providers.filter(provider => provider.connected);
  if (connected.length === 0) return "not-connected";
  if (connected.every(provider => provider.selectedCalendars === 0)) return "no-calendars";
  return data.events.length === 0 ? "no-events" : null;
}
export const upcomingEmptyLabels = {
  "not-connected": "Connect a calendar to see upcoming meetings here.",
  "no-calendars": "Choose which calendars to use in Settings.",
  "no-events": "Nothing on your calendar this week.",
} as const;

export const calendarNoticeLabels: Record<CalendarLinkNotice, string> = {
  overlap: "Several meetings overlap, so this recording isn't linked.",
  "choice-unavailable": "The event you chose isn't happening now, so this recording isn't linked.",
  "calendar-unreadable": "Your calendar couldn't be read, so this recording isn't linked.",
  "not-saved": "The calendar link couldn't be saved with the recording, so it isn't linked.",
};

export const calendarChipText = (event: CandidateEvent, now: Date): string => `For: ${eventTitle(event)} · ${upcomingTime(event, now)}`;

/** An explicit refusal survives an empty cache; an automatic choice is resolved
 * from the fresh candidates immediately before Start. Rust validates event ids. */
export function startCalendarChoice(candidates: CalendarCandidates, choice: CalendarLinkChoice | null): CalendarLinkChoice | undefined {
  if (choice && choice.mode !== "auto") return choice;
  if (candidates.events.length === 0) return undefined;
  return candidates.auto ? { mode: "event", eventId: candidates.auto } : { mode: "auto" };
}
