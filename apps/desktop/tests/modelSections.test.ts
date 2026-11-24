import { test } from "node:test";
import assert from "node:assert/strict";
import { localModelSections, type LocalModelRow } from "../src/lib/localModels.js";
import { cloudServices } from "../src/lib/cloudServices.js";

const row = (id: string, over: Partial<LocalModelRow> = {}): LocalModelRow => ({
  id, name: id, engine: "whisper", usedFor: "dictation", languages: "many", sizeMb: 100,
  installed: false, recommended: false, inUse: false, loadedButGoesTo: null, hint: null, verifiedBy: null,
  ...over,
});

test("the list comes in three parts, and every row is in exactly one (KNEG-573)", () => {
  const rows = [
    row("tiny"),
    row("base", { recommended: true }),
    row("small", { installed: true }),
    row("medium", { installed: true, inUse: true }),
    row("large"),
  ];
  const parts = localModelSections(rows);
  assert.deepEqual(parts.installed.map((r) => r.id), ["medium", "small"]);
  assert.deepEqual(parts.recommended.map((r) => r.id), ["base"]);
  assert.deepEqual(parts.more.map((r) => r.id), ["tiny", "large"]);
  assert.equal(parts.installed.length + parts.recommended.length + parts.more.length, rows.length);
});

test("the model in use leads the installed ones; the rest keep the order they came in", () => {
  const parts = localModelSections([
    row("a", { installed: true }),
    row("b", { installed: true }),
    row("c", { installed: true, inUse: true }),
  ]);
  assert.deepEqual(parts.installed.map((r) => r.id), ["c", "a", "b"]);
});

test("a recommended model that is installed is listed once, as installed", () => {
  const parts = localModelSections([row("base", { recommended: true, installed: true })]);
  assert.deepEqual(parts.installed.map((r) => r.id), ["base"]);
  assert.deepEqual(parts.recommended, []);
});

test("a model that is loaded while dictation goes elsewhere is installed, not in use", () => {
  const parts = localModelSections([
    row("a", { installed: true, loadedButGoesTo: "OpenAI" }),
    row("b", { installed: true }),
  ]);
  assert.deepEqual(parts.installed.map((r) => r.id), ["a", "b"]);
});

const ready = { state: "ready" } as const;
const provider = (id: string, configured: boolean, dispatch: { state: "ready" } | { state: "needs-client"; reason: string } = ready) =>
  ({ id, configured, dispatch });

test("a cloud service gets a card when it has a key, or dictation is set to it", () => {
  const providers = [provider("openai", true), provider("groq", false), provider("mistral", false)];
  const local = cloudServices(providers, { isLocal: true, destination: "local model" });
  assert.deepEqual(local.cards.map((p) => p.id), ["openai"]);
  assert.deepEqual(local.addable.map((p) => p.id), ["groq", "mistral"]);
  assert.equal(local.inUse, null);

  // Dictation set to a service with no key: it keeps its card, where the key goes.
  const keyless = cloudServices(providers, { isLocal: false, destination: "groq" });
  assert.deepEqual(keyless.cards.map((p) => p.id), ["openai", "groq"]);
  assert.deepEqual(keyless.addable.map((p) => p.id), ["mistral"]);
  assert.equal(keyless.inUse, "groq");
});

test("a service with no client is neither a card nor offered a key", () => {
  const providers = [provider("openai", false), provider("tinfoil", true, { state: "needs-client", reason: "refused" })];
  const { cards, addable } = cloudServices(providers, { isLocal: true, destination: "local model" });
  assert.deepEqual(cards, []);
  assert.deepEqual(addable.map((p) => p.id), ["openai"]);
});

test("a local route names no service as in use, whatever the destination says", () => {
  assert.equal(cloudServices([provider("openai", true)], { isLocal: true, destination: "openai" }).inUse, null);
  assert.equal(cloudServices([provider("openai", true)], { isLocal: false, destination: "nowhere" }).inUse, null);
});
