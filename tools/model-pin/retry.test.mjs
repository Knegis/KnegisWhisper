/**
 * Tests for the pin tools' bounded retry (KNEG-413).
 *
 * `isTransient` is tested against errors Node actually throws, captured from
 * `fetch` against a local server that drops its sockets, not against objects
 * shaped like what a reader believed undici throws. The 2026-10-05 failure was
 * a socket closed before the response headers; a socket closed mid-body throws
 * differently, and both have to be recognised.
 *
 * Run: node --test tools/model-pin/retry.test.mjs
 */

import { test } from "node:test";
import assert from "node:assert/strict";

import { HttpError, describe, isTransient, retrying } from "./retry.mjs";
import { body, bytesOf, closedPort, dropBeforeHeaders, dropMidBody, scriptedServer } from "./test-server.mjs";

/* ------------------------------------------------------------- isTransient */

test("a socket closed before the response headers is transient (the 2026-10-05 failure)", async () => {
  const server = await scriptedServer({ "/f": [dropBeforeHeaders()] });
  try {
    const error = await fetch(server.url("/f")).then(
      () => assert.fail("the request should have failed"),
      (e) => e
    );
    assert.equal(isTransient(error), true, describe(error));
  } finally {
    await server.close();
  }
});

test("a socket closed mid-body is transient", async () => {
  const server = await scriptedServer({ "/f": [dropMidBody(bytesOf(256 * 1024))] });
  try {
    const response = await fetch(server.url("/f"));
    const error = await response.arrayBuffer().then(
      () => assert.fail("the body should have failed"),
      (e) => e
    );
    assert.equal(isTransient(error), true, describe(error));
  } finally {
    await server.close();
  }
});

test("a refused connection is not transient: nothing is listening there", async () => {
  const port = await closedPort();
  const error = await fetch(`http://127.0.0.1:${port}/f`).then(
    () => assert.fail("the request should have failed"),
    (e) => e
  );
  assert.equal(isTransient(error), false, describe(error));
});

test("server errors and rate limits are transient; other statuses are not", () => {
  for (const status of [408, 429, 500, 502, 503, 504]) {
    assert.equal(isTransient(new HttpError(status, "https://example.invalid/f")), true, String(status));
  }
  for (const status of [400, 401, 403, 404, 410, 501]) {
    assert.equal(isTransient(new HttpError(status, "https://example.invalid/f")), false, String(status));
  }
});

test("a disk-full or permission error from the same pipeline is not transient", () => {
  for (const code of ["ENOSPC", "EACCES", "EPERM"]) {
    const error = Object.assign(new Error(`write failed: ${code}`), { code });
    assert.equal(isTransient(error), false, code);
  }
});

test("a transient code is found anywhere down the cause chain and inside an AggregateError", () => {
  const socket = Object.assign(new Error("other side closed"), { code: "UND_ERR_SOCKET" });
  assert.equal(isTransient(new TypeError("terminated", { cause: socket })), true);
  const reset = Object.assign(new Error("read ECONNRESET"), { code: "ECONNRESET" });
  const aggregate = new AggregateError([new Error("first"), reset], "connect failed");
  assert.equal(isTransient(new TypeError("fetch failed", { cause: aggregate })), true);
});

test("a cause chain that loops back on itself is walked once, not forever", () => {
  const a = new Error("a");
  const b = new Error("b", { cause: a });
  a.cause = b;
  assert.equal(isTransient(a), false);
});

test("describe names the innermost cause and its code", () => {
  const socket = Object.assign(new Error("other side closed"), { code: "UND_ERR_SOCKET" });
  assert.equal(
    describe(new TypeError("fetch failed", { cause: socket })),
    "fetch failed (UND_ERR_SOCKET: other side closed)"
  );
  assert.equal(describe(new Error("plain")), "plain");
});

/* ---------------------------------------------------------------- retrying */

const transient = () => Object.assign(new Error("dropped"), { code: "UND_ERR_SOCKET" });

test("retrying returns the first success, after sleeping the stated delay", async () => {
  const slept = [];
  let calls = 0;
  const value = await retrying(
    async () => {
      calls += 1;
      if (calls === 1) throw transient();
      return "bytes";
    },
    { delays: [5000, 15000], sleep: async (ms) => slept.push(ms) }
  );
  assert.equal(value, "bytes");
  assert.equal(calls, 2);
  assert.deepEqual(slept, [5000]);
});

test("retrying gives up after one attempt per delay plus one, with the last error", async () => {
  const slept = [];
  let calls = 0;
  const last = await retrying(
    async () => {
      calls += 1;
      throw Object.assign(transient(), { attempt: calls });
    },
    { delays: [5000, 15000], sleep: async (ms) => slept.push(ms) }
  ).then(
    () => assert.fail("should have given up"),
    (e) => e
  );
  assert.equal(calls, 3);
  assert.equal(last.attempt, 3);
  assert.deepEqual(slept, [5000, 15000]);
});

test("retrying makes one attempt and no sleep for a permanent error", async () => {
  const slept = [];
  let calls = 0;
  const error = await retrying(
    async () => {
      calls += 1;
      throw new HttpError(404, "https://example.invalid/f");
    },
    { delays: [5000, 15000], sleep: async (ms) => slept.push(ms) }
  ).then(
    () => assert.fail("a 404 should not succeed"),
    (e) => e
  );
  assert.equal(calls, 1);
  assert.deepEqual(slept, []);
  assert.equal(error.status, 404);
});

test("retrying tells onRetry which attempt is next, of how many, after what, and how long it waits", async () => {
  const seen = [];
  let calls = 0;
  await retrying(
    async () => {
      calls += 1;
      if (calls < 3) throw transient();
      return null;
    },
    { delays: [1, 2], sleep: async () => {}, onRetry: (info) => seen.push(info) }
  );
  assert.deepEqual(
    seen.map(({ next, of, delay, error }) => ({ next, of, delay, message: error.message })),
    [
      { next: 2, of: 3, delay: 1, message: "dropped" },
      { next: 3, of: 3, delay: 2, message: "dropped" },
    ]
  );
});
