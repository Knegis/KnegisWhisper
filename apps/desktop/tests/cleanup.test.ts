import { test } from "node:test";
import assert from "node:assert/strict";
import { cleanupConfigured, cleanupFields, cleanupRouteLine, effectiveCleanup, reasoningDispatchLine } from "../src/lib/cleanup.js";
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

/* ------------------------------------------------- in plain words (KNEG-343) */

const ready = (destination: string, leavesDevice: boolean, caveat: string | null = null) =>
  ({ state: "ready", destination, model: "m-1", leavesDevice, caveat }) as const;

test("cleanup that is off says dictations are typed as they were transcribed", () => {
  assert.deepEqual(cleanupRouteLine({ state: "off" }), {
    value: "Off",
    detail: "Dictations are typed exactly as they were transcribed.",
    problem: false,
    more: null,
  });
});

test("a cloud route names the provider and says the text leaves this computer", () => {
  assert.deepEqual(cleanupRouteLine(ready("openai", true)), {
    value: "OpenAI · m-1",
    detail: "Text is sent to OpenAI to tidy it up.",
    problem: false,
    more: null,
  });
});

test("the model is named as the model menu names it, when the page knows the name", () => {
  const named = cleanupRouteLine(ready("openai", true), (id) => (id === "m-1" ? "Model One" : id));
  assert.equal(named.value, "OpenAI · Model One");
});

test("the local route says the text stays on this computer", () => {
  const line = cleanupRouteLine(ready("local model", false));
  assert.equal(line.value, "This computer · m-1");
  assert.equal(line.detail, "Text stays on this computer.");
});

test("a self-hosted route says your server, with its address under Details", () => {
  const away = cleanupRouteLine(ready("self-hosted at http://192.168.1.4:8080/v1", true));
  assert.equal(away.value, "Your server · m-1");
  assert.equal(away.detail, "Text is sent to your server to tidy it up.");
  assert.match(away.more ?? "", /192\.168\.1\.4:8080/);
  assert.equal(cleanupRouteLine(ready("self-hosted at http://127.0.0.1:11434/v1", false)).detail, "Text stays on this computer.");
});

test("a caveat says what to do for its kind of route, with Rust's sentence under Details", () => {
  const noKey = "no key is stored for openai. Cleanup will leave the transcript as it is until one is, on the Providers screen.";
  assert.deepEqual(cleanupRouteLine(ready("openai", true, noKey)), {
    value: "OpenAI · m-1",
    detail: "No key is saved for OpenAI. Add one on the Providers page; until then dictations are typed as transcribed.",
    problem: true,
    more: noKey,
  });
  assert.equal(
    cleanupRouteLine(ready("local model", false, "no local model is running, so …")).detail,
    "No local model is running. Start one under Local text models; until then dictations are typed as transcribed.",
  );
  assert.equal(
    cleanupRouteLine(ready("bedrock", true, "bedrock has no configuration stored, so …")).detail,
    "AWS Bedrock isn't set up yet. Set it up under Company accounts below; until then dictations are typed as transcribed.",
  );
});

test("a route that cannot be resolved is a problem, with the reason under Details", () => {
  assert.deepEqual(cleanupRouteLine({ state: "problem", reason: "no model is chosen" }), {
    value: "On, but it can't be used",
    detail: "Dictations are typed as transcribed until this is fixed.",
    problem: true,
    more: "no model is chosen",
  });
});

test("why a provider can or cannot be used, with Rust's reason under Details", () => {
  assert.deepEqual(reasoningDispatchLine({ state: "ready" }), { text: "Can be used", more: null });
  assert.deepEqual(reasoningDispatchLine({ state: "needs-config", reason: "bedrock needs …" }), {
    text: "Set up under Company accounts below",
    more: "bedrock needs …",
  });
  assert.deepEqual(reasoningDispatchLine({ state: "needs-sidecar", reason: "no local model is running." }), {
    text: "Start a local model first",
    more: "no local model is running.",
  });
  assert.deepEqual(reasoningDispatchLine({ state: "unresolvable", reason: "x" }), { text: "Can't be used", more: "x" });
});
