/**
 * The Whisper model a meeting on the `local` provider uses, chosen on
 * Settings > Meetings (`meetings.localModel`).
 *
 * Automatic is the setting's absence, and is what meetings always did: the
 * model already loaded, otherwise the one last chosen for dictation, otherwise
 * the first one installed. A chosen model is loaded at Start and makes both the
 * live transcript and the saved note (`local_model.rs` `MeetingServer`).
 *
 * Chosen in two places, which write the same setting: the select on Settings >
 * Meetings, and the transcription chip's menu on the Meetings page, as Dictation's
 * chip lists its models (`providerMenu`).
 *
 * Pure, so the wording is tested (`tests/meetingModel.test.ts`). What can
 * actually be started is Rust's to say at Start; `localProviderProblem` says so
 * before it for a chosen model that isn't installed.
 */
import { bySize } from "./localModels.js";

/** The select's value for Automatic. The setting itself holds no model. */
export const AUTOMATIC_MODEL = "";

interface WhisperModel {
  id: string;
  name: string;
  sizeMb: number | null;
  installed: boolean;
}

export interface MeetingModelOption {
  id: string;
  label: string;
  disabled: boolean;
}

export interface MeetingModelChoice {
  id: string;
  /** The name and size for an installed model, the name alone for one that has gone. */
  label: string;
  installed: boolean;
}

/**
 * What can be listed: what's installed, smallest first as on Settings >
 * Transcription. Models that aren't installed are left out, with one
 * exception: a chosen model that has gone stays listed, so the control still
 * shows what is saved rather than quietly showing something else.
 */
export function meetingModelChoices(
  models: readonly WhisperModel[],
  chosen: string | null,
): MeetingModelChoice[] {
  const installed = models.filter((model) => model.installed).sort(bySize);
  const choices: MeetingModelChoice[] = installed.map((model) => ({
    id: model.id,
    label: model.sizeMb !== null ? `${model.name} · ${model.sizeMb} MB` : model.name,
    installed: true,
  }));
  if (chosen !== null && !installed.some((model) => model.id === chosen)) {
    const name = models.find((model) => model.id === chosen)?.name ?? chosen;
    choices.push({ id: chosen, label: name, installed: false });
  }
  return choices;
}

/** The select on Settings > Meetings: Automatic, then the choices, one that has gone greyed. */
export function meetingModelOptions(
  models: readonly WhisperModel[],
  chosen: string | null,
): MeetingModelOption[] {
  return [
    { id: AUTOMATIC_MODEL, label: "Automatic", disabled: false },
    ...meetingModelChoices(models, chosen).map((choice) => ({
      id: choice.id,
      label: choice.installed ? choice.label : `${choice.label} · not installed`,
      disabled: !choice.installed,
    })),
  ];
}

/**
 * What the transcription chip says: the model, when one is chosen for the
 * provider that uses it, since that is the choice its menu just made. The
 * provider's own name otherwise, Automatic included.
 */
export function transcriberLabel(
  provider: { id: string; name: string } | null,
  models: readonly WhisperModel[],
  chosen: string | null,
): string {
  if (provider === null) return "Not set up";
  if (provider.id !== "local" || chosen === null) return provider.name;
  return models.find((model) => model.id === chosen)?.name ?? chosen;
}

/** The sentence under the select: what the choice showing will actually do. */
export function meetingModelDetail(i: {
  models: readonly WhisperModel[];
  chosen: string | null;
  runningModel: string | null;
  /** The Parakeet model selected for dictation, by name, or `null` for none. */
  parakeet: string | null;
}): string {
  if (i.chosen !== null && !i.models.some((model) => model.id === i.chosen && model.installed)) {
    return "This model isn't installed. Install it under Settings > Transcription > Local, or choose another.";
  }
  const more = i.models.some((model) => !model.installed)
    ? " Install more under Settings > Transcription > Local."
    : "";
  if (i.chosen !== null) {
    return (
      "Loaded when a meeting starts, for the live transcript and the saved note. " +
      `Until the note is saved, dictation on a local Whisper model uses this one too.${more}`
    );
  }
  const running = i.models.find((model) => model.id === i.runningModel)?.name ?? i.runningModel;
  const uses =
    running !== null
      ? `Uses ${running}, the Whisper model loaded now.`
      : "Uses the Whisper model last chosen for dictation, or the first one installed.";
  // The saved note follows dictation's engine unless a model is chosen here,
  // and a selected Parakeet model outranks a running Whisper server.
  const note =
    i.parakeet !== null ? ` The saved note comes from ${i.parakeet}, the model chosen for dictation.` : "";
  return `${uses}${note}${more}`;
}
