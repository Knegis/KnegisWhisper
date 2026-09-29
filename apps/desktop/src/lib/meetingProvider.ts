/**
 * Which meeting provider the two meeting screens should show as selected:
 * Settings > Meetings, in its select, and the Meetings page, as the provider
 * that will transcribe. The Meetings page passes no `chosen` -- it has no
 * select -- so the two agree on everything but a choice being made right now.
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
