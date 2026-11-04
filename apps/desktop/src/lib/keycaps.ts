/**
 * The keys of each accelerator, as `components/Keycaps.tsx` draws them: one
 * group per accelerator, one cap per key (KNEG-331).
 *
 * The accelerators come one by one, not as `acceleratorText`'s ", "-joined
 * string, because the comma is a key as well as that string's separator.
 * Within one accelerator `+` only separates: the comma key is written
 * `Control+,`, and nothing spells a key as a bare `+` (`classify` in
 * crates/platform/src/hotkey.rs).
 */
export function keycapGroups(accelerators: readonly string[]): string[][] {
  return accelerators.map((accelerator) => accelerator.split("+"));
}

/**
 * What a key is called on this keyboard. The accelerator grammar's word is
 * "Super" (`crates/platform/src/hotkey.rs`), which is right where it is stored
 * and typed, but a Windows keyboard says "Win" on that key. For showing a key
 * only: what is saved stays the grammar's. `platform` is `navigator.platform`,
 * a parameter so this can be tested without a browser.
 */
export function keyName(key: string, platform: string): string {
  if (!platform.startsWith("Win")) return key;
  switch (key) {
    case "Super":
      return "Win";
    case "LeftSuper":
      return "Left Win";
    case "RightSuper":
      return "Right Win";
    default:
      return key;
  }
}

/** A shortcut in words -- `acceleratorText`'s string -- with the same names as the caps. */
export function shortcutText(text: string, platform: string): string {
  return text.replace(/\b(?:Left|Right)?Super\b/g, (key) => keyName(key, platform));
}
