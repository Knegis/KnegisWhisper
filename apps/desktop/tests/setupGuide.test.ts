import { test } from "node:test";
import assert from "node:assert/strict";
import { closeLabel, routeStep, stepsDone } from "../src/lib/setupGuide.js";

const providers = [
  { id: "openai", name: "OpenAI", configured: false },
  { id: "mistral", name: "Mistral", configured: true },
];

test("the first step is done only when the route would work (KNEG-329)", () => {
  // A new profile: OpenAI, no key. The page must not read as ready, and neither does the guide.
  assert.deepEqual(routeStep({ isLocal: false, destination: "openai", problem: null }, providers),
    { done: false, line: "OpenAI is chosen, and it has no key yet." });
  assert.deepEqual(routeStep({ isLocal: false, destination: "mistral", problem: null }, providers),
    { done: true, line: "At Mistral. Your audio is sent to Mistral." });
  assert.deepEqual(routeStep({ isLocal: true, destination: "local model", problem: null }, providers),
    { done: true, line: "On this computer. Your audio stays here." });
  assert.deepEqual(routeStep({ isLocal: true, destination: "local model", problem: "no model is installed" }, providers),
    { done: false, line: "Nothing is chosen yet." });
  assert.equal(routeStep({ isLocal: false, destination: "my own server", problem: null }, providers).done, true);
});

test("the guide says where audio goes whenever it leaves this computer", () => {
  const cloud = routeStep({ isLocal: false, destination: "mistral", problem: null }, providers);
  assert.match(cloud.line, /is sent to Mistral/);
  const local = routeStep({ isLocal: true, destination: "local model", problem: null }, providers);
  assert.match(local.line, /stays here/);
});

test("it counts what is done, and only calls itself done at three", () => {
  const ready = { done: true, line: "" };
  const not = { done: false, line: "" };
  assert.equal(stepsDone(not, false, false), 0);
  assert.equal(stepsDone(ready, true, false), 2);
  assert.equal(stepsDone(ready, true, true), 3);
  assert.equal(closeLabel(0), "Skip setup");
  assert.equal(closeLabel(2), "Skip setup");
  assert.equal(closeLabel(3), "Done");
});
