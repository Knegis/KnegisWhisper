import { test } from "node:test";
import assert from "node:assert/strict";
import { cleanupConfigured, cleanupFields, effectiveCleanup } from "../src/lib/cleanup.js";
import type { CleanupSettings, ReasoningProviderSummary } from "../src/generated/ipc-types.js";

const model = (id: string) => ({
  id,
  name: id,
  tokenParam: "max_tokens",
  supportsThinking: false,
  supportsVision: false,
});
const provider = (id: string, enterprise = false, ids = ["a-1", "a-2"]): ReasoningProviderSummary => ({
  id,
  name: id,
  credentialSlot: enterprise ? null : id,
  configured: true,
  enterprise,
  dispatch: { state: "ready" },
  models: ids.map(model),
});
const providers = [provider("openai", false, ["gpt-4o-mini", "gpt-4o"]), provider("bedrock", true, [])];
const none: CleanupSettings = { enabled: false, provider: null, model: null, selfHostedUrl: null, enterprise: false };

test("a stored provider and model are sent as they are, with the switch asked for", () => {
  const stored = { ...none, provider: "openai", model: "gpt-4o" };
  assert.deepEqual(effectiveCleanup(stored, providers, true), {
    enabled: true,
    provider: "openai",
    model: "gpt-4o",
    selfHostedUrl: null,
    enterprise: false,
  });
  assert.equal(effectiveCleanup({ ...stored, enabled: true }, providers, false).enabled, false);
});

test("a stored provider with no model sends the provider's first model", () => {
  const stored = { ...none, provider: "openai" };
  assert.equal(effectiveCleanup(stored, providers, true).model, "gpt-4o-mini");
});

test("a provider with no models, or one the list does not know, sends no model", () => {
  assert.equal(effectiveCleanup({ ...none, provider: "bedrock", enterprise: true }, providers, true).model, null);
  assert.equal(effectiveCleanup({ ...none, provider: "gone" }, providers, true).model, null);
});

test("an enterprise provider is sent as enterprise, whatever the stored flag says", () => {
  const sent = effectiveCleanup({ ...none, provider: "bedrock" }, providers, true);
  assert.equal(sent.enterprise, true);
  assert.equal(effectiveCleanup({ ...none, provider: "openai", enterprise: true }, providers, true).enterprise, false);
});

test("a self-hosted URL is trimmed, and with no stored model sends none rather than a provider's first", () => {
  const sent = effectiveCleanup({ ...none, provider: "openai", selfHostedUrl: " http://127.0.0.1:8080 " }, providers, true);
  assert.equal(sent.selfHostedUrl, "http://127.0.0.1:8080");
  assert.equal(sent.model, null);
});

test("a blank URL is no URL", () => {
  const sent = effectiveCleanup({ ...none, provider: "openai", selfHostedUrl: "   " }, providers, true);
  assert.equal(sent.selfHostedUrl, null);
  assert.equal(sent.model, "gpt-4o-mini");
});

test("drafts win over what is stored, field by field", () => {
  const stored = { ...none, provider: "openai", model: "gpt-4o", selfHostedUrl: "http://old" };
  const sent = effectiveCleanup(stored, providers, true, { provider: "bedrock", model: null, url: "" });
  // The draft provider and the emptied URL win; the unset model draft falls back to stored.
  assert.deepEqual(sent, { enabled: true, provider: "bedrock", model: "gpt-4o", selfHostedUrl: null, enterprise: true });
});

test("a draft model wins over the stored one", () => {
  const stored = { ...none, provider: "openai", model: "gpt-4o" };
  assert.equal(effectiveCleanup(stored, providers, true, { model: "gpt-4o-mini" }).model, "gpt-4o-mini");
});

test("nothing stored sends nothing but the switch", () => {
  assert.deepEqual(effectiveCleanup(none, providers, false), {
    enabled: false,
    provider: null,
    model: null,
    selfHostedUrl: null,
    enterprise: false,
  });
});

test("cleanupFields is what the page shows: the same fallbacks, as plain strings", () => {
  const f = cleanupFields({ ...none, provider: "openai" }, providers);
  assert.equal(f.provider, "openai");
  assert.equal(f.model, "gpt-4o-mini");
  assert.equal(f.url, "");
  assert.equal(f.selfHosted, false);
  assert.equal(f.chosen?.id, "openai");
});

test("configured means a provider or a URL is stored", () => {
  assert.equal(cleanupConfigured(none), false);
  assert.equal(cleanupConfigured({ ...none, provider: "" }), false);
  assert.equal(cleanupConfigured({ ...none, selfHostedUrl: "  " }), false);
  assert.equal(cleanupConfigured({ ...none, provider: "openai" }), true);
  assert.equal(cleanupConfigured({ ...none, selfHostedUrl: "http://x" }), true);
});
