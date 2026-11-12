/**
 * Whether a key press belongs to a select whose list is open.
 *
 * The lists are drawn in the page (`appearance: base-select` on the select and its
 * `::picker(select)`, in styles.css), so their keys reach the page's own handlers. The
 * platform's popup never sent them. A menu or form that closes on Escape asks this first,
 * so Escape closes the list and leaves the menu open. While the list is open, focus is on
 * one of its options, so this looks up from the target to its select.
 *
 * False where `:open` is unknown: there the list is the platform's, and it keeps its keys.
 */
export function fromOpenList(target: EventTarget | null): boolean {
  const select = target instanceof Element ? target.closest("select") : null;
  if (!select) return false;
  try {
    return select.matches(":open");
  } catch {
    return false;
  }
}
