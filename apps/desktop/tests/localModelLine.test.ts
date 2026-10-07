/**
 * What Dictation says about the local model server (KNEG-357).
 *
 * The page showed the server's own sentence -- "Local model base is ready on
 * port 8178." -- to colleagues who are not technical. These pin the plain
 * version: nothing while it works, failures in plain view, and the model id,
 * port, error text and stderr tail only behind Learn more. Diagnostics keeps
 * the whole sentence.
 */
import assert from "node:assert/strict";
import test from "node:test";

import { localModelLine } from "../src/lib/localModelLine.js";

test("a ready server says nothing: the privacy line already says this computer", () => {
  assert.equal(localModelLine({ state: "ready", model: "base", port: 8178 }), null);
});

test("a stopped server says nothing: the next dictation starts it", () => {
  assert.equal(localModelLine({ state: "stopped" }), null);
});

test("starting says so, without the model id", () => {
  for (const runtime of [
    { state: "checking", model: "kb-whisper-base" },
    { state: "loading", model: "kb-whisper-base" },
  ] as const) {
    const line = localModelLine(runtime);
    assert.ok(line);
    assert.equal(line.text, "Starting the local model…");
    assert.equal(line.problem, false);
    assert.equal(line.more, null);
  }
});

test("a server that could not start is a visible failure naming the screen, with the error folded", () => {
  const line = localModelLine({ state: "failed", error: "the model file is missing at C:\\models\\ggml-base.bin" });
  assert.ok(line);
  assert.equal(line.problem, true);
  assert.match(line.text, /could not start/);
  assert.match(line.text, /Settings › Transcription/);
  assert.doesNotMatch(line.text, /ggml|C:\\/);
  assert.equal(line.more, "the model file is missing at C:\\models\\ggml-base.bin");
});

test("a server that went away is a visible failure that says it recovers, with the report folded", () => {
  const line = localModelLine({ state: "exited", report: "exit code 0xc0000005 (access violation)" });
  assert.ok(line);
  assert.equal(line.problem, true);
  assert.match(line.text, /stopped unexpectedly/);
  assert.match(line.text, /next dictation/);
  assert.doesNotMatch(line.text, /0xc0000005/);
  assert.equal(line.more, "exit code 0xc0000005 (access violation)");
});

test("an exit with nothing to report folds nothing", () => {
  const line = localModelLine({ state: "exited", report: "  " });
  assert.ok(line);
  assert.equal(line.more, null);
});

test("no sentence carries a port or a raw id", () => {
  const runtimes = [
    { state: "checking", model: "base" },
    { state: "loading", model: "base" },
    { state: "failed", error: "port 8178 refused" },
    { state: "exited", report: "port 8178" },
  ] as const;
  for (const runtime of runtimes) {
    const line = localModelLine(runtime);
    assert.ok(line);
    assert.doesNotMatch(line.text, /8178|\bbase\b|port/, `${runtime.state}: ${line.text}`);
  }
});
