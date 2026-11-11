/**
 * Calendar setup and failures in plain words. These helpers only read the
 * generated vocabulary: they never inspect credentials or contact a provider.
 * A build may supply its own client; a saved custom client overrides it.
 */
import type { CalendarProblem, CalendarProvider, ProviderStatus } from "./ipc.js";

/** Safe wording for every classified calendar failure, shared by connect and sync. */
export function calendarProblemLabel(problem: CalendarProblem): string {
  switch (problem.kind) {
    case "cancelled": return "Connection cancelled.";
    case "timed-out": return "Sign-in timed out. Try again.";
    case "access-denied": return "Calendar access was declined.";
    case "admin-consent-required": return "Ask your administrator to approve calendar access.";
    case "app-blocked": return "Your organisation has blocked this app.";
    case "client-misconfigured": return "Check the calendar client configuration.";
    case "client-rejected": return "The provider rejected this calendar client.";
    case "reconnect-required": return "Reconnect the account.";
    case "network": return "Could not reach the calendar provider. Try again.";
    case "provider-unavailable": return "The calendar provider is unavailable. Try again.";
    case "storage": return "Could not save or read calendar data.";
    case "invalid-response": return "The calendar provider returned an invalid response.";
  }
}

export function calendarClientLabel(provider: CalendarProvider, source: ProviderStatus["clientSource"]): string {
  switch (source) {
    case "app": return "Using KnegisWhisper's built-in client";
    case "custom": return "Using your own client";
    case "missing": return `This build has no built-in ${provider === "google" ? "Google Calendar" : "Outlook"} connection. Add your own client.`;
  }
}

/** Provider warnings are possibilities, not a diagnosis of the person's account. */
export function calendarTroubleshootingLabel(provider: CalendarProvider): string {
  return provider === "google"
    ? "If Google says it hasn't verified this app, you can continue past that warning. If it says access is blocked, your organisation doesn't allow this app."
    : "A work or school account may need an administrator to approve KnegisWhisper first.";
}

export function calendarLeftoverLabel(origin: ProviderStatus["leftover"][number]["origin"]): string {
  switch (origin) {
    case "openwhispr": return "Imported from OpenWhispr";
    case "earlier": return "Earlier connection";
  }
}

export function calendarCleanupLabel(count: number): string {
  return `${count} old sign-in${count === 1 ? "" : "s"} couldn't be removed from the credential store`;
}
