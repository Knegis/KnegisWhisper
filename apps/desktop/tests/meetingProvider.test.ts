import { test } from "node:test";
import assert from "node:assert/strict";
import { selectProvider } from "../src/lib/meetingProvider.js";

const usable = ["openai-realtime", "local"];

test("nothing chosen and nothing saved falls back to the first usable provider", () => {
  assert.deepEqual(selectProvider(null, null, usable), {
    selected: "openai-realtime",
    savedUnusable: null,
  });
});

test("the saved provider wins over the first usable one", () => {
  assert.deepEqual(selectProvider(null, "local", usable), {
    selected: "local",
    savedUnusable: null,
  });
});

test("a choice made on screen wins over the saved provider until it is saved", () => {
  assert.deepEqual(selectProvider("openai-realtime", "local", usable), {
    selected: "openai-realtime",
    savedUnusable: null,
  });
});

test("a saved provider that cannot be used right now is named, and the first usable one is selected", () => {
  assert.deepEqual(selectProvider(null, "local", ["openai-realtime"]), {
    selected: "openai-realtime",
    savedUnusable: "local",
  });
});

test("a choice that stopped being usable falls through to the saved provider", () => {
  assert.deepEqual(selectProvider("openai-realtime", "local", ["local"]), {
    selected: "local",
    savedUnusable: null,
  });
});

test("no usable provider selects nothing but still names the saved one", () => {
  assert.deepEqual(selectProvider(null, "local", []), {
    selected: null,
    savedUnusable: "local",
  });
});
