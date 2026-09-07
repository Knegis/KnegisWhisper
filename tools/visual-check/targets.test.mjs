/** Verifies stable fragment-based classification of WebView2 debug targets. */
import test from "node:test";
import assert from "node:assert/strict";
import {
  classifyTarget,
  listClassifiedTargets,
  pickTarget,
} from "./targets.mjs";

test("targets are classified by their fragment rather than their title", () => {
  assert.equal(classifyTarget({ url: "http://127.0.0.1:5183/" }), "main");
  assert.equal(
    classifyTarget({ url: "http://127.0.0.1:5183/#meeting" }),
    "meeting",
  );
  assert.equal(classifyTarget({ url: "bad url" }), "other");
});

test("pickTarget selects the requested page target", () => {
  const entries = [
    { type: "page", url: "http://127.0.0.1:5183/#overlay" },
    { type: "worker", url: "http://127.0.0.1:5183/#meeting" },
    { type: "page", url: "http://127.0.0.1:5183/#meeting" },
  ];
  assert.equal(pickTarget(entries, "meeting"), entries[2]);
  assert.equal(listClassifiedTargets(entries)[0].classification, "overlay");
});
