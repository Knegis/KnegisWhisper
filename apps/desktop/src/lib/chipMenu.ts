/**
 * What a chip's menu holds, and how the keyboard moves through it
 * (`components/ChipMenu.tsx` draws it).
 *
 * Pure, with no React, so `tests/chipMenu.test.ts` imports it directly. Every
 * menu on a task page takes the same `MenuOption`, whoever builds the list
 * (ruling R1 of KNEG-385's plan), so the component has one shape to draw.
 *
 * A greyed row -- `disabled`, with its `reason` -- is still a row the arrows
 * land on. Skipping it would hide the one sentence that says why it cannot
 * be chosen ("needs a key") from anyone who does not see it greyed; so it
 * takes focus, says `aria-disabled`, and `chosenOption` refuses it.
 */

export type MenuOption = {
  /** What the caller is told was chosen. Unique within one menu. */
  id: string;
  label: string;
  /** The heading this row sits under, or `null` for none. See `menuGroups`. */
  group: string | null;
  /** The value in effect: ticked, `aria-selected`, and where the menu opens. */
  current: boolean;
  disabled: boolean;
  /** Why a greyed row cannot be chosen; read out with it. */
  reason: string | null;
  /** A muted second line under the label. */
  detail?: string;
};

export type MenuGroup = { group: string | null; items: { option: MenuOption; index: number }[] };

/**
 * The row a key moves focus to, from row `from` (`-1` for none), or `null`
 * for a key that does not move or a menu with no rows. ArrowDown and ArrowUp
 * move one row and stop at the ends rather than wrapping, as a select-only
 * listbox does; Home and End go to the first and the last row.
 *
 * With `footer`, there is one more row after the options -- the menu's
 * "More in Settings…" -- at index `options.length`. It is not an option, but
 * it is reached the same way, because Tab closes the menu and an arrow is
 * the only key left that can get there.
 */
export function nextIndex(options: readonly MenuOption[], from: number, key: string, footer = false): number | null {
  const last = options.length + (footer ? 1 : 0) - 1;
  if (last < 0) return null;
  const inside = from >= 0 && from <= last;
  switch (key) {
    case "ArrowDown":
      return inside ? Math.min(from + 1, last) : 0;
    case "ArrowUp":
      return inside ? Math.max(from - 1, 0) : last;
    case "Home":
      return 0;
    case "End":
      return last;
    default:
      return null;
  }
}

/** Where the menu opens: on the value in effect, or on the first row when none is. */
export function openIndex(options: readonly MenuOption[]): number {
  return Math.max(0, options.findIndex((option) => option.current));
}

/**
 * The option at `index`, if it can be chosen: not greyed, and an option
 * rather than the footer. The current option can be chosen again; a caller
 * whose current row is a stand-in -- the fallback a saved, unusable value
 * gave way to -- needs that to save it.
 */
export function chosenOption(options: readonly MenuOption[], index: number): MenuOption | null {
  const option = options[index];
  return option !== undefined && !option.disabled ? option : null;
}

/**
 * The options as runs under one heading each, in the order given, every
 * option keeping its index in the flat list. Runs rather than gathering
 * every row of a group together, so the rows on screen are in the order the
 * arrows walk them; a caller lists a group's rows together, and one that
 * names a group again after another has started gets a second heading.
 */
export function menuGroups(options: readonly MenuOption[]): MenuGroup[] {
  const groups: MenuGroup[] = [];
  options.forEach((option, index) => {
    const run = groups[groups.length - 1];
    if (run !== undefined && run.group === option.group) run.items.push({ option, index });
    else groups.push({ group: option.group, items: [{ option, index }] });
  });
  return groups;
}
