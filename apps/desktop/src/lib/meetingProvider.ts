/**
 * Which meeting provider the two meeting screens should show as selected:
 * Settings > Meetings, in its select, and the Meetings page, as the provider
 * that will transcribe. The Meetings page passes no `chosen` -- its chip's
 * menu saves the choice and the page reads it back -- so the two agree on
 * everything but a choice being made right now.
 *
 * Three inputs, in order of authority: what the person chose on this visit to
 * the screen, what was saved on an earlier one (`meetings.provider`), and the
 * providers that can be used right now. The saved choice used to be nothing --
 * the select fell back to the first usable provider on every mount, so
 * somebody who picked the local provider saw OpenAI Realtime again after the
 * next start, which reads as the setting having been lost.
 *
 * A saved provider that is not usable at the moment (the local one with no
 * model server running, a cloud one whose key was removed) is *named* rather
 * than silently replaced, so the screen can say why the select shows something
 * else. Nothing here writes: the fallback must never overwrite the saved
 * choice, or one start without the model server would forget it.
 */
import type { MenuOption } from "./chipMenu.js";
import type { MeetingProviderSummary } from "./ipc.js";
import { availabilityLabel } from "./labels.js";

export interface ProviderSelection {
  /** The provider id to show selected, or `null` when nothing is usable. */
  selected: string | null;
  /** The saved provider, when it exists and cannot be used right now. */
  savedUnusable: string | null;
}

export function selectProvider(
  chosen: string | null,
  saved: string | null,
  usable: readonly string[],
): ProviderSelection {
  const candidates = [chosen, saved].filter(
    (id): id is string => id !== null && id !== "" && usable.includes(id),
  );
  const selected = candidates[0] ?? usable[0] ?? null;
  const savedUnusable = saved !== null && saved !== "" && !usable.includes(saved) ? saved : null;
  return { selected, savedUnusable };
}

/**
 * The Meetings page's provider menu: where the audio goes first, because that
 * is the question the choice answers. "On this computer" and "Cloud", each
 * group's rows together, as `ChipMenu` requires.
 *
 * A provider that is not `available` is greyed with the reason Rust gave
 * (`availabilityLabel`), so the menu says why a row cannot be chosen rather
 * than leaving it out -- except `declined`, which is a refusal and not
 * something to act on, and is left out unless it is the saved provider.
 *
 * A saved provider that cannot be used is listed too, greyed, and `selected`
 * -- the fallback `selectProvider` picked, which Start would use -- is the
 * current row. The page lets that row be chosen again, which writes it, so a
 * saved choice that cannot be used is replaced only by somebody choosing;
 * nothing here, and nothing in `selectProvider`, writes it.
 */
export function providerMenu(
  providers: readonly MeetingProviderSummary[],
  saved: string | null,
  selected: string | null,
): MenuOption[] {
  const option = (p: MeetingProviderSummary): MenuOption => {
    const usable = p.availability.state === "available";
    return {
      id: p.id,
      label: p.name,
      group: p.audioLeavesDevice ? "Cloud" : "On this computer",
      current: usable && p.id === selected,
      disabled: !usable,
      reason: usable ? null : availabilityLabel(p.availability),
    };
  };
  const listed = providers
    .filter((p) => p.availability.state !== "declined" || p.id === saved)
    .map(option);
  return [
    ...listed.filter((o) => o.group === "On this computer"),
    ...listed.filter((o) => o.group === "Cloud"),
  ];
}
