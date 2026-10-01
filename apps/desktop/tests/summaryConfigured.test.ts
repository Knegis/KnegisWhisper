import { test } from "node:test";
import assert from "node:assert/strict";
import { summaryConfigured } from "../src/lib/summaryConfigured.js";
import type { SummarySettings } from "../src/generated/ipc-types.js";

const base: SummarySettings = {
  enabled: false,
  provider: "openai",
  model: "gpt-4o-mini",
  selfHostedUrl: null,
  enterprise: false,
  language: "match",
  contextBudget: 8192,
};

test("a provider and a model is configured, whether or not summaries are on", () => {
  assert.equal(summaryConfigured(base), true);
  assert.equal(summaryConfigured({ ...base, enabled: true }), true);
});

test("the default settings are not configured", () => {
  assert.equal(summaryConfigured({ ...base, provider: "", model: "" }), false);
});

test("no model is not configured, even with a provider", () => {
  assert.equal(summaryConfigured({ ...base, model: "" }), false);
  assert.equal(summaryConfigured({ ...base, model: "   " }), false);
});

test("no provider and no URL is not configured", () => {
  assert.equal(summaryConfigured({ ...base, provider: "" }), false);
  assert.equal(summaryConfigured({ ...base, provider: "  " }), false);
});

test("a self-hosted URL stands in for the provider", () => {
  assert.equal(
    summaryConfigured({ ...base, provider: "", selfHostedUrl: "https://llm.example.com/v1" }),
    true,
  );
});

test("Rust's check is a URL that is present, even if blank", () => {
  // validate() asks only `self_hosted_url.is_none()`; mirrored as it is.
  assert.equal(summaryConfigured({ ...base, provider: "", selfHostedUrl: "" }), true);
});

test("a language Rust refuses is not configured", () => {
  assert.equal(summaryConfigured({ ...base, language: "fr" }), false);
  assert.equal(summaryConfigured({ ...base, language: "sv" }), true);
  assert.equal(summaryConfigured({ ...base, language: "en" }), true);
});

test("a context budget outside 4096..=262144 is not configured", () => {
  assert.equal(summaryConfigured({ ...base, contextBudget: 4095 }), false);
  assert.equal(summaryConfigured({ ...base, contextBudget: 4096 }), true);
  assert.equal(summaryConfigured({ ...base, contextBudget: 262144 }), true);
  assert.equal(summaryConfigured({ ...base, contextBudget: 262145 }), false);
});
