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
import { meetingModelChoices } from "./meetingModel.js";

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
 *
 * With `windowed`, the local windowed provider's models follow under their own
 * heading, as Dictation's chip lists its models, so choosing one is one press.
 * The provider's own row is then Automatic. Nothing is listed while `windowed`
 * isn't known, or when that provider can't be used, because a model of a
 * provider that can't start is no choice at all. Choosing a row is
 * `providerChoice`'s to write.
 */
export function providerMenu(
  providers: readonly MeetingProviderSummary[],
  saved: string | null,
  selected: string | null,
  windowed: WindowedModels | null = null,
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
  const windowedRow = listed.find((o) => o.id === WINDOWED_PROVIDER && !o.disabled);
  const models = windowed !== null && windowedRow !== undefined ? windowedRows(windowed, windowedRow.current) : null;
  return [
    ...listed
      .filter((o) => o.group === "On this computer")
      .map((o) =>
        o.id === WINDOWED_PROVIDER && models !== null
          ? { ...o, detail: "Automatic model", current: o.current && windowed?.chosen === null }
          : o,
      ),
    ...(models ?? []),
    ...listed.filter((o) => o.group === "Cloud"),
  ];
}

const WINDOWED_PROVIDER = "local";
/** A model row's id: this, then the model's. No provider's id starts with it. */
const WINDOWED_MODEL_ROW = "local:";

export interface WindowedModels {
  models: readonly { id: string; name: string; sizeMb: number | null; installed: boolean }[];
  /** `meetings.localModel`: the model chosen, or `null` for Automatic. */
  chosen: string | null;
}

function windowedRows(windowed: WindowedModels, providerIsCurrent: boolean): MenuOption[] {
  return meetingModelChoices(windowed.models, windowed.chosen).map((choice) => ({
    id: `${WINDOWED_MODEL_ROW}${choice.id}`,
    label: choice.label,
    group: "Local windowed models",
    current: providerIsCurrent && windowed.chosen === choice.id,
    disabled: !choice.installed,
    reason: choice.installed ? null : "not installed",
  }));
}

/** What a choice in the provider menu writes. `model` absent leaves the model alone. */
export type ProviderChoice = { provider?: string; model?: string | null };

/**
 * What choosing row `id` has to write, or `null` when it is already saved. A
 * model row sets the model and the provider that uses it; the windowed
 * provider's own row is Automatic, so it clears any model. Any other provider
 * leaves the model alone, so going to the cloud and back keeps the choice.
 *
 * `savedModel` is `undefined` while the model isn't known, when the menu lists
 * no model rows and the provider's row claims nothing about the model.
 */
export function providerChoice(
  id: string,
  savedProvider: string | null,
  savedModel: string | null | undefined,
): ProviderChoice | null {
  const isModelRow = id.startsWith(WINDOWED_MODEL_ROW);
  const provider = isModelRow ? WINDOWED_PROVIDER : id;
  const model = isModelRow
    ? id.slice(WINDOWED_MODEL_ROW.length)
    : id === WINDOWED_PROVIDER && savedModel !== undefined
      ? null
      : undefined;
  const choice: ProviderChoice = {
    ...(provider !== savedProvider ? { provider } : {}),
    ...(model !== undefined && model !== savedModel ? { model } : {}),
  };
  return Object.keys(choice).length > 0 ? choice : null;
}
