/**
 * Settings › Calendar in plain words (KNEG-343).
 *
 * What a person must do (reconnect) stays in `text` or `detail`; the token
 * mechanics and the provider's own error go to `more`, for Learn more.
 */
import { test } from "node:test";
import assert from "node:assert/strict";
import { connectOutcomeLabel, syncOutcomeLabel, tokenStatusLabel } from "../src/lib/labels.js";
import {
  calendarCleanupLabel,
  calendarClientLabel,
  calendarLeftoverLabel,
  calendarProblemLabel,
  calendarTroubleshootingLabel,
} from "../src/lib/calendarLabels.js";
import type { CalendarProblem } from "../src/lib/ipc.js";

test("a connection says connected or not, and keeps the token mechanics for Learn more", () => {
  const active = tokenStatusLabel({ state: "active" });
  assert.equal(active.text, "Connected");
  assert.equal(active.detail, "");
  assert.match(active.more ?? "", /access token/);

  assert.equal(tokenStatusLabel({ state: "needs-refresh" }).text, "Connected");
  assert.deepEqual(tokenStatusLabel({ state: "missing" }), { text: "Not connected", detail: "", more: null });
});

test("every CalendarProblem kind has a non-empty plain label", () => {
  // This record must grow when the generated union grows. A representative
  // array alone would keep passing if a new failure silently lost its label.
  const kinds = {
    cancelled: true, "timed-out": true, "access-denied": true,
    "admin-consent-required": true, "app-blocked": true,
    "client-misconfigured": true, "client-rejected": true,
    "reconnect-required": true, network: true, "provider-unavailable": true,
    storage: true, "invalid-response": true,
  } satisfies Record<CalendarProblem["kind"], true>;
  for (const kind of Object.keys(kinds) as CalendarProblem["kind"][]) {
    const label = calendarProblemLabel({ kind });
    assert.ok(label.trim().length > 0, kind);
    assert.equal(syncOutcomeLabel({ state: "token-unavailable", problem: { kind }, message: "fallback" }).text,
      kind === "reconnect-required" ? "Couldn't sync. Reconnect the account." : label);
  }
});

test("client wording distinguishes this build, an override and missing clients", () => {
  for (const provider of ["google", "microsoft"] as const) {
    assert.equal(calendarClientLabel(provider, "app"), "Using KnegisWhisper's built-in client");
    assert.equal(calendarClientLabel(provider, "custom"), "Using your own client");
  }
  assert.equal(calendarClientLabel("google", "missing"), "This build has no built-in Google Calendar connection. Add your own client.");
  assert.equal(calendarClientLabel("microsoft", "missing"), "This build has no built-in Outlook connection. Add your own client.");
});

test("sign-in troubleshooting names possibilities for each provider", () => {
  assert.equal(calendarTroubleshootingLabel("google"), "If Google says it hasn't verified this app, you can continue past that warning. If it says access is blocked, your organisation doesn't allow this app.");
  assert.equal(calendarTroubleshootingLabel("microsoft"), "A work or school account may need an administrator to approve KnegisWhisper first.");
});

test("leftovers and credential cleanup have plain labels and singular wording", () => {
  assert.equal(calendarLeftoverLabel("openwhispr"), "Imported from OpenWhispr");
  assert.equal(calendarLeftoverLabel("earlier"), "Earlier connection");
  assert.equal(calendarCleanupLabel(1), "1 old sign-in couldn't be removed from the credential store");
  assert.equal(calendarCleanupLabel(2), "2 old sign-ins couldn't be removed from the credential store");
});

test("a failed renewal says to reconnect, with the provider's error under Learn more", () => {
  const failed = tokenStatusLabel({ state: "refresh-failed", message: "invalid_grant: Token has been expired or revoked." });
  assert.equal(failed.text, "Not connected");
  assert.equal(failed.detail, "Reconnect the account.");
  assert.doesNotMatch(failed.detail, /invalid_grant/);
  assert.match(failed.more ?? "", /invalid_grant/);
});

test("connecting says who, or that access was declined, with the reason behind it", () => {
  assert.deepEqual(connectOutcomeLabel({ state: "connected", accountEmail: "anna@example.com", calendars: 2 }), {
    text: "Connected as anna@example.com (2 calendars).",
    more: null,
  });
  assert.deepEqual(connectOutcomeLabel({ state: "denied", reason: "access_denied" }), {
    text: "Not connected: access was declined.",
    more: "access_denied",
  });
});

test("a sync says what changed, or what to do, with the errors under Learn more", () => {
  assert.deepEqual(syncOutcomeLabel({ state: "completed", eventsUpserted: 3, eventsRemoved: 1, calendarErrors: [] }), {
    text: "Synced: 3 events updated, 1 removed.",
    more: null,
  });
  const partly = syncOutcomeLabel({ state: "completed", eventsUpserted: 0, eventsRemoved: 0, calendarErrors: ["Team: 403 Forbidden"] });
  assert.equal(partly.text, "Synced: 0 events updated, 0 removed. 1 calendar couldn't be synced.");
  assert.equal(partly.more, "Team: 403 Forbidden");

  const token = syncOutcomeLabel({ state: "token-unavailable", message: "invalid_grant", problem: null });
  assert.equal(token.text, "Couldn't sync. Reconnect the account.");
  assert.equal(token.more, "invalid_grant");

  assert.deepEqual(syncOutcomeLabel({ state: "failed", message: "connection reset", problem: null }), {
    text: "Couldn't sync.",
    more: "connection reset",
  });
});


test("classified calendar problems give short actionable wording", () => {
  const cases = [
    ["cancelled", "Connection cancelled."],
    ["timed-out", "Sign-in timed out. Try again."],
    ["access-denied", "Calendar access was declined."],
    ["admin-consent-required", "Ask your administrator to approve calendar access."],
    ["app-blocked", "Your organisation has blocked this app."],
    ["client-misconfigured", "Check the calendar client configuration."],
    ["client-rejected", "The provider rejected this calendar client."],
    ["reconnect-required", "Reconnect the account."],
    ["network", "Could not reach the calendar provider. Try again."],
    ["provider-unavailable", "The calendar provider is unavailable. Try again."],
    ["storage", "Could not save or read calendar data."],
    ["invalid-response", "The calendar provider returned an invalid response."],
  ] as const;
  for (const [kind, text] of cases) {
    assert.deepEqual(connectOutcomeLabel({ state: "failed", problem: { kind } }), {
      text: kind === "reconnect-required" ? "The previous connection could not be kept. Reconnect the account." : text,
      more: null,
    });
    assert.deepEqual(syncOutcomeLabel({ state: "failed", problem: { kind }, message: "safe fallback" }), { text, more: null });
  }
  assert.deepEqual(tokenStatusLabel({ state: "reconnect-required" }), {
    text: "Not connected", detail: "Reconnect the account.", more: null,
  });
  assert.equal(syncOutcomeLabel({ state: "token-unavailable", problem: { kind: "reconnect-required" }, message: "Reconnect." }).text,
    "Couldn't sync. Reconnect the account.");
});
