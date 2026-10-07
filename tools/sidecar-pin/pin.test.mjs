/**
 * Tests for the sidecar pin measurer's download (KNEG-413).
 *
 * This tool had no tests: importing it ran the CLI, so nothing could reach a
 * function without starting a measurement. The retry that the 2026-10-05
 * failure called for is tested here against a local server that drops its
 * connections, the same way `tools/model-pin/pin.test.mjs` tests its own.
 *
 * Run: node --test tools/sidecar-pin/pin.test.mjs
 */

import { test } from "node:test";
import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { existsSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

import { download } from "./pin.mjs";
import {
  body,
  bytesOf,
  dropBeforeHeaders,
  dropMidBody,
  scriptedServer,
  status,
} from "../model-pin/test-server.mjs";

const BYTES = bytesOf(256 * 1024);
const DIGEST = createHash("sha256").update(BYTES).digest("hex");
const quick = { sleep: async () => {} };
const tempFor = (label) => join(tmpdir(), `sidecar-pin-${process.pid}-${label}`);

async function withServer(routes, run) {
  const server = await scriptedServer(routes);
  try {
    await run(server);
  } finally {
    await server.close();
  }
}

test("importing the tool runs nothing: the CLI waits for it to be the entry point", () => {
  // Reaching this line is the assertion. Before KNEG-413 the import above ran
  // the measuring CLI with no arguments, which exits the process.
  assert.equal(typeof download, "function");
});

test("download retries a connection closed before the headers, the 2026-10-05 failure", async () => {
  await withServer({ "/a.zip": [dropBeforeHeaders(), body(BYTES)] }, async (server) => {
    const measured = await download(server.url("/a.zip"), "pre-headers", quick);
    try {
      assert.equal(measured.sha256, DIGEST);
      assert.equal(measured.sizeBytes, BYTES.length);
      assert.equal(server.requests("/a.zip"), 2);
    } finally {
      rmSync(measured.path, { force: true });
    }
  });
});

test("download retries a body cut off halfway, and hashes only the whole one", async () => {
  await withServer({ "/a.zip": [dropMidBody(BYTES), body(BYTES)] }, async (server) => {
    const measured = await download(server.url("/a.zip"), "mid-body", quick);
    try {
      assert.equal(measured.sha256, DIGEST);
      assert.equal(measured.sizeBytes, BYTES.length);
    } finally {
      rmSync(measured.path, { force: true });
    }
  });
});

test("download does not retry a 404, throws rather than exiting, and leaves no file", async () => {
  await withServer({ "/a.zip": [status(404)] }, async (server) => {
    const error = await download(server.url("/a.zip"), "missing", quick).then(
      () => assert.fail("a 404 is not a download"),
      (e) => e
    );
    assert.equal(server.requests("/a.zip"), 1);
    assert.equal(error.status, 404);
    assert.equal(existsSync(tempFor("missing")), false);
  });
});

test("download gives up after three attempts and leaves no partial file", async () => {
  await withServer({ "/a.zip": [dropMidBody(BYTES)] }, async (server) => {
    await download(server.url("/a.zip"), "always-drops", quick).then(
      () => assert.fail("every attempt was cut off"),
      () => {}
    );
    assert.equal(server.requests("/a.zip"), 3);
    assert.equal(existsSync(tempFor("always-drops")), false);
  });
});
