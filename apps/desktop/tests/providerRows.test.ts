import { test } from "node:test";
import assert from "node:assert/strict";
import { filterRows, keyRowShown, providerRows, type ProviderRow } from "../src/lib/providerRows.js";
import type { CredentialSlotStatus, Dispatch, ProviderSummary } from "../src/generated/ipc-types.js";

/*
 * Shaped as `credential_slots` returns them (`commands.rs`), with slot ids,
 * names, kinds, `usedFor` and `feature` copied from
 * `config/provider-registry.json`'s `credentials` array. Only `stored` is
 * invented, and Groq's `usedFor` is narrowed where a test says so.
 */
const slot = (over: Partial<CredentialSlotStatus> & Pick<CredentialSlotStatus, "slot" | "name">): CredentialSlotStatus => ({
  kind: "provider",
  usedFor: [],
  feature: null,
  stored: false,
  ...over,
});

const openai = slot({ slot: "openai", name: "OpenAI", usedFor: ["transcription", "reasoning"], stored: true });
const anthropic = slot({ slot: "anthropic", name: "Anthropic", usedFor: ["reasoning"] });
// Narrowed to speech alone: a slot the registry also uses for reasoning keeps its + (see below).
const groq = slot({ slot: "groq", name: "Groq", usedFor: ["transcription"] });
const tinfoil = slot({ slot: "tinfoil", name: "Tinfoil", usedFor: ["transcription", "reasoning"] });
const noteFormatting = slot({ slot: "note-formatting-custom", name: "Note formatting", kind: "custom-endpoint", feature: "note-formatting" });
const translation = slot({ slot: "translation-custom", name: "Translation", kind: "custom-endpoint", feature: "translation", stored: true });
const bedrock = slot({ slot: "bedrock", name: "AWS Bedrock", kind: "enterprise", feature: "bedrock", stored: true });
const azure = slot({ slot: "azure", name: "Azure OpenAI", kind: "enterprise", feature: "azure" });
const assemblyai = slot({ slot: "assemblyai", name: "AssemblyAI" });
const deepgram = slot({ slot: "deepgram", name: "Deepgram", stored: true });

/* Shaped as `providers` returns them: the registry's transcription providers and what the router would do. */
const summary = (id: string, dispatch: Dispatch): ProviderSummary => ({
  id,
  name: id,
  credentialSlot: id,
  configured: false,
  modelCount: 1,
  models: ["m"],
  dispatch,
});
const transcription: ProviderSummary[] = [
  summary("openai", { state: "ready" }),
  summary("groq", { state: "needs-client", reason: "no Groq transcription client is built yet" }),
  summary("tinfoil", { state: "needs-client", reason: "an unattested request would present as Tinfoil" }),
];

const slots = [openai, anthropic, groq, tinfoil, noteFormatting, translation, bedrock, azure, assemblyai, deepgram];

const bySlot = (rows: readonly ProviderRow[], id: string) => rows.find((r) => r.slot === id);

test("a stored key used for speech and text is in use, with both purposes and its letter", () => {
  const { inUse } = providerRows(slots, transcription);
  assert.deepEqual(bySlot(inUse, "openai"), {
    slot: "openai",
    name: "OpenAI",
    letter: "O",
    purposes: ["Speech to text", "Cleanup and summaries"],
    stored: true,
    unsupported: false,
  });
});

test("an unstored key whose speech client is missing is available and marked unsupported", () => {
  const { available, inUse } = providerRows(slots, transcription);
  assert.equal(bySlot(inUse, "groq"), undefined);
  assert.deepEqual(bySlot(available, "groq"), {
    slot: "groq",
    name: "Groq",
    letter: "G",
    purposes: ["Speech to text"],
    stored: false,
    unsupported: true,
  });
});

test("a missing speech client does not mark a key unsupported while the key has another use", () => {
  // Tinfoil's transcription is refused and its reasoning is not, so a key for
  // it still does something: the + stays, and Settings > Providers still stores it.
  const { available } = providerRows(slots, transcription);
  assert.equal(bySlot(available, "tinfoil")?.unsupported, false);
});

test("custom endpoints and enterprise connectors get their own lists, stored or not", () => {
  const { inUse, available, custom, enterprise } = providerRows(slots, transcription);
  assert.deepEqual(custom.map((r) => r.slot), ["note-formatting-custom", "translation-custom"]);
  assert.deepEqual(enterprise.map((r) => r.slot), ["bedrock", "azure"]);
  for (const id of ["note-formatting-custom", "translation-custom", "bedrock", "azure"]) {
    assert.equal(bySlot(inUse, id), undefined, id);
    assert.equal(bySlot(available, id), undefined, id);
  }
});

test("each list keeps the order the slots arrived in", () => {
  const { inUse, available } = providerRows(slots, transcription);
  assert.deepEqual(inUse.map((r) => r.slot), ["openai", "deepgram"]);
  assert.deepEqual(available.map((r) => r.slot), ["anthropic", "groq", "tinfoil", "assemblyai"]);
});

test("an empty usedFor gives no purposes, and a use the page has no words for is left out", () => {
  const odd = slot({ slot: "odd", name: "odd", usedFor: ["reasoning", "something-new"] });
  const { inUse, available } = providerRows([deepgram, odd], []);
  assert.deepEqual(bySlot(inUse, "deepgram")?.purposes, []);
  assert.deepEqual(bySlot(available, "odd")?.purposes, ["Cleanup and summaries"]);
  assert.equal(bySlot(available, "odd")?.letter, "O");
});

test("the speech filter keeps only rows used for speech to text", () => {
  const { inUse, available } = providerRows(slots, transcription);
  const all = [...inUse, ...available];
  assert.deepEqual(filterRows(all, "speech", "").map((r) => r.slot), ["openai", "groq", "tinfoil"]);
});

test("the text filter keeps only rows used for cleanup and summaries", () => {
  const { inUse, available } = providerRows(slots, transcription);
  const all = [...inUse, ...available];
  assert.deepEqual(filterRows(all, "text", "").map((r) => r.slot), ["openai", "anthropic", "tinfoil"]);
});

test("all keeps every row, and the query matches the name whatever its case", () => {
  const { inUse, available } = providerRows(slots, transcription);
  const all = [...inUse, ...available];
  assert.equal(filterRows(all, "all", "").length, all.length);
  assert.deepEqual(filterRows(all, "all", "oPeNaI").map((r) => r.slot), ["openai"]);
  assert.deepEqual(filterRows(all, "all", "  gram ").map((r) => r.slot), ["deepgram"]);
  assert.deepEqual(filterRows(all, "speech", "anthropic"), []);
});

test("a key row is shown under a filter and a search only while one of its lists keeps it", () => {
  // KNEG-377: a typed key stayed in the hidden row's form and came back with
  // it. The page closes the form when this says the row went out of view.
  const lists = providerRows(slots, transcription);
  assert.equal(keyRowShown(lists, "anthropic", "all", ""), true);
  assert.equal(keyRowShown(lists, "anthropic", "speech", ""), false);
  assert.equal(keyRowShown(lists, "anthropic", "all", "deep"), false);
  assert.equal(keyRowShown(lists, "translation-custom", "all", "trans"), true);
  // Enterprise rows have no key form, so they are never "shown" as one.
  assert.equal(keyRowShown(lists, "azure", "all", ""), false);
});
