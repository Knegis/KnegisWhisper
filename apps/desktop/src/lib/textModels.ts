/**
 * Settings > Local text models, in parts (KNEG-573).
 *
 * The page listed every model the registry has, thirty-five cards in registry
 * order with no way to narrow them, and the one a person had installed was
 * wherever its family happened to fall: about seven screens down, on the
 * machine this was measured on. The parts put first what a person has, then
 * what to get, and fold the catalogue behind a search.
 *
 * - **installed**: every installed model, the loaded one first.
 * - **recommended**: recommended models that are not installed. The flag is
 *   the registry's, one per family. It is not sized to this computer: its
 *   memory and processor are not read, so nothing here says a model will run
 *   well, and the page must not either.
 * - **families**: the rest, under their family's name, narrowed by `query`
 *   on the model's name or its family's. A family nothing is left of is
 *   dropped.
 *
 * Every model is in exactly one part when `query` is empty.
 *
 * Pure, with no React, for `tests/textModels.test.ts`. The family's display
 * name is passed in, because it comes from the generated registry and this
 * file's test should not need it.
 */

import type { LocalLlmModelStatus } from "../generated/ipc-types.js";

type Model = Pick<LocalLlmModelStatus, "id" | "name" | "provider" | "recommended" | "integrity">;

export const isInstalled = (model: Pick<LocalLlmModelStatus, "integrity">) => model.integrity.state !== "not-installed";

export function textModelSections<M extends Model>(
  models: readonly M[],
  runningModel: string | null,
  query: string,
  familyName: (id: string) => string,
): { installed: M[]; recommended: M[]; families: { id: string; name: string; models: M[] }[]; rest: number } {
  const installed = models.filter(isInstalled);
  const others = models.filter((m) => !isInstalled(m) && !m.recommended);
  const needle = query.trim().toLocaleLowerCase();
  const families: { id: string; name: string; models: M[] }[] = [];
  for (const model of others) {
    const name = familyName(model.provider);
    if (needle !== "" && !model.name.toLocaleLowerCase().includes(needle) && !name.toLocaleLowerCase().includes(needle)) continue;
    const family = families.find((f) => f.id === model.provider);
    if (family) family.models.push(model);
    else families.push({ id: model.provider, name, models: [model] });
  }
  return {
    installed: [...installed.filter((m) => m.id === runningModel), ...installed.filter((m) => m.id !== runningModel)],
    recommended: models.filter((m) => !isInstalled(m) && m.recommended),
    families,
    rest: others.length,
  };
}
