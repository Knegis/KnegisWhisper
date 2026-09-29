/**
 * Stored times, and lists grouped by the day they fall on.
 *
 * `parseStoredTime` moved here from `views/Notes.tsx`, where it was the body
 * of `noteDate`, so that Dictation's history and Notes read a stored time the
 * same way. Two shapes reach it: SQLite's `CURRENT_TIMESTAMP` form,
 * `YYYY-MM-DD HH:MM:SS`, which is UTC and says nothing about it, and an ISO
 * string, which carries its own offset. Reading the first as local time would
 * put a dictation made at 23:30 in Stockholm under the wrong day.
 *
 * Pure, and with no React: `tests/dayGroups.test.ts` imports this directly.
 */

/**
 * A stored time as a `Date`, or `null` when there is none or it will not
 * parse. `null` rather than an Invalid Date, so a caller cannot format one by
 * accident and show "Invalid Date".
 */
export function parseStoredTime(s: string | null): Date | null {
  if (!s) return null;
  const date = new Date(s.includes("T") ? s : s.replace(" ", "T") + "Z");
  return Number.isNaN(date.getTime()) ? null : date;
}

/** The local calendar day, as a key two dates can be compared by. */
function dayKey(d: Date): string {
  return `${d.getFullYear()}-${d.getMonth()}-${d.getDate()}`;
}

/**
 * `items` in runs by local day: "Today", "Yesterday", then a short date --
 * with the year when it is not this one, so 20 September last year and this
 * one are not two groups under the same name. Items with no readable time go
 * to "Earlier", which always comes last.
 *
 * Groups come in the order their first item appears, and items keep their
 * input order within a group, so a list already sorted newest first stays
 * that way. `now` is a parameter so the grouping can be tested without a
 * clock.
 */
export function groupByDay<T>(
  items: readonly T[],
  at: (t: T) => string | null,
  now: Date,
): { label: string; items: T[] }[] {
  const today = dayKey(now);
  const yesterday = dayKey(new Date(now.getFullYear(), now.getMonth(), now.getDate() - 1));
  const groups = new Map<string, { label: string; items: T[] }>();
  const earlier: T[] = [];
  for (const item of items) {
    const date = parseStoredTime(at(item));
    if (date === null) {
      earlier.push(item);
      continue;
    }
    const key = dayKey(date);
    let group = groups.get(key);
    if (group === undefined) {
      const label =
        key === today
          ? "Today"
          : key === yesterday
            ? "Yesterday"
            : date.toLocaleDateString(
                undefined,
                date.getFullYear() === now.getFullYear()
                  ? { day: "numeric", month: "short" }
                  : { day: "numeric", month: "short", year: "numeric" },
              );
      group = { label, items: [] };
      groups.set(key, group);
    }
    group.items.push(item);
  }
  const out = [...groups.values()];
  if (earlier.length > 0) out.push({ label: "Earlier", items: earlier });
  return out;
}

/**
 * The local time of day as `HH:MM`, on a 24-hour clock whatever the locale --
 * the canvas' history rows and "Last dictation" header read "18:04", and a
 * locale's "6:04 PM" would not fit the machine voice either sits in.
 */
export function timeOfDay(d: Date): string {
  return `${String(d.getHours()).padStart(2, "0")}:${String(d.getMinutes()).padStart(2, "0")}`;
}
