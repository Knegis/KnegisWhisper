import { test } from "node:test";
import assert from "node:assert/strict";
import { textModelSections } from "../src/lib/textModels.js";

type State = "not-installed" | "unchanged-since-install";
const model = (id: string, provider: string, over: { recommended?: boolean; state?: State } = {}) => ({
  id, name: id, provider, recommended: over.recommended ?? false,
  integrity: { state: over.state ?? "not-installed" } as never,
});
const FAMILY: Record<string, string> = { qwen: "Qwen", gemma: "Gemma (Google)", mistral: "Mistral" };
const family = (id: string) => FAMILY[id] ?? id;

const models = [
  model("Qwen3 4B", "qwen"),
  model("Qwen3.5 9B", "qwen", { recommended: true }),
  model("Mistral 7B", "mistral", { state: "unchanged-since-install" }),
  model("Mistral Nemo 12B", "mistral", { recommended: true, state: "unchanged-since-install" }),
  model("Gemma 4 E2B", "gemma"),
  model("Gemma 4 E4B", "gemma"),
];

test("installed first, then recommended, then the rest by family (KNEG-573)", () => {
  const parts = textModelSections(models, null, "", family);
  assert.deepEqual(parts.installed.map((m) => m.id), ["Mistral 7B", "Mistral Nemo 12B"]);
  assert.deepEqual(parts.recommended.map((m) => m.id), ["Qwen3.5 9B"]);
  assert.deepEqual(parts.families.map((f) => [f.name, f.models.map((m) => m.id)]), [
    ["Qwen", ["Qwen3 4B"]],
    ["Gemma (Google)", ["Gemma 4 E2B", "Gemma 4 E4B"]],
  ]);
  assert.equal(parts.rest, 3);
  const count = parts.installed.length + parts.recommended.length + parts.families.reduce((n, f) => n + f.models.length, 0);
  assert.equal(count, models.length, "every model is in exactly one part");
});

test("the loaded model leads the installed ones", () => {
  const parts = textModelSections(models, "Mistral Nemo 12B", "", family);
  assert.deepEqual(parts.installed.map((m) => m.id), ["Mistral Nemo 12B", "Mistral 7B"]);
});

test("a recommended model that is installed is listed once, as installed", () => {
  const parts = textModelSections(models, null, "", family);
  assert.equal(parts.recommended.some((m) => m.id === "Mistral Nemo 12B"), false);
});

test("the search narrows the catalogue by model or family name, and nothing else", () => {
  const byModel = textModelSections(models, null, "e4b", family);
  assert.deepEqual(byModel.families.map((f) => f.models.map((m) => m.id)), [["Gemma 4 E4B"]]);
  const byFamily = textModelSections(models, null, "  GOOGLE ", family);
  assert.deepEqual(byFamily.families.map((f) => f.models.length), [2]);
  // What is installed and what is recommended do not answer to the search.
  assert.equal(byModel.installed.length, 2);
  assert.equal(byModel.recommended.length, 1);
  // `rest` is how many there are in all, for the fold's label.
  assert.equal(byModel.rest, 3);
  assert.deepEqual(textModelSections(models, null, "nothing like it", family).families, []);
});
