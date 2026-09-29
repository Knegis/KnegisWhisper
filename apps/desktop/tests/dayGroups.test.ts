import { test } from "node:test";
import assert from "node:assert/strict";
import { groupByDay, parseStoredTime, timeOfDay } from "../src/lib/dayGroups.js";

// Stored strings are built from local dates, so the expectations hold in any
// time zone: SQLite's `YYYY-MM-DD HH:MM:SS` is UTC, and this is that form.
const stored = (d: Date) => d.toISOString().slice(0, 19).replace("T", " ");
const now = new Date(2026, 8, 29, 12, 0);
const short = (d: Date) => d.toLocaleDateString(undefined, { day: "numeric", month: "short" });

test("items fall under Today, Yesterday, or their own date", () => {
  const items = [new Date(2026, 8, 29, 9), new Date(2026, 8, 28, 23, 59), new Date(2026, 8, 20, 8)].map(stored);
  const groups = groupByDay(items, (s) => s, now);
  assert.deepEqual(groups.map((g) => g.label).slice(0, 2), ["Today", "Yesterday"]);
  assert.equal(groups.length, 3);
  assert.equal(groups[2]?.label, short(new Date(2026, 8, 20)));
});

test("an undated item goes to Earlier, after every dated group", () => {
  const items = [null, stored(new Date(2026, 8, 29, 9)), "not a date"];
  const groups = groupByDay(items, (s) => s, now);
  assert.deepEqual(groups.map((g) => g.label), ["Today", "Earlier"]);
  assert.deepEqual(groups[1]?.items, [null, "not a date"]);
});

test("input order is kept within a group", () => {
  const items = [
    { id: 3, at: stored(new Date(2026, 8, 29, 8)) },
    { id: 1, at: stored(new Date(2026, 8, 29, 11)) },
    { id: 2, at: stored(new Date(2026, 8, 29, 10)) },
  ];
  const groups = groupByDay(items, (item) => item.at, now);
  assert.equal(groups.length, 1);
  assert.deepEqual(groups[0]?.items.map((item) => item.id), [3, 1, 2]);
});

test("the same day and month in another year is its own group, and says the year", () => {
  const items = [new Date(2026, 8, 20, 8), new Date(2025, 8, 20, 8)].map(stored);
  const groups = groupByDay(items, (s) => s, now);
  assert.equal(groups.length, 2);
  assert.equal(groups[0]?.label, short(new Date(2026, 8, 20)));
  assert.equal(
    groups[1]?.label,
    new Date(2025, 8, 20).toLocaleDateString(undefined, { day: "numeric", month: "short", year: "numeric" }),
  );
});

test("parseStoredTime reads SQLite's UTC form and ISO strings, and refuses the rest", () => {
  assert.equal(parseStoredTime("2026-09-29 10:00:00")?.toISOString(), "2026-09-29T10:00:00.000Z");
  assert.equal(parseStoredTime("2026-08-26T14:12:03Z")?.toISOString(), "2026-08-26T14:12:03.000Z");
  assert.equal(parseStoredTime(null), null);
  assert.equal(parseStoredTime(""), null);
  assert.equal(parseStoredTime("not a date"), null);
});

test("timeOfDay is the local HH:MM, on a 24-hour clock", () => {
  assert.equal(timeOfDay(new Date(2026, 8, 29, 9, 5)), "09:05");
  assert.equal(timeOfDay(new Date(2026, 8, 29, 18, 40)), "18:40");
});
