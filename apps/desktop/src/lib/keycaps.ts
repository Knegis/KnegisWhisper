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
