/**
 * Tests for the model pin measurer.
 *
 * Three things are worth asserting without a network: that the argument parser
 * agrees with the documented command lines, that hashing a file on disk
 * produces the digest and the size the rest of the world would produce for the
 * same bytes, and that the reader for the Rust `PINNED` table refuses to guess.
 *
 * The last is the one that earns its keep. `pinnedTargets` reads a Rust literal
 * from a Node tool, and the failure mode of a loose reader is not a crash — it
 * is `--verify` checking zero models and printing a pass. So the negative cases
 * are asserted: a table that moved, a table whose declared arity no longer
 * matches its rows.
 *
 * Run: node --test tools/model-pin/pin.test.mjs
 */

import { test } from "node:test";
import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { mkdtempSync, writeFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

import { args, measureFile, parseWhisperPins, pinnedTargets } from "./pin.mjs";

/* -------------------------------------------------------------------- args */

test("args parses --flag value pairs", () => {
  assert.deepEqual(
    args(["--url", "https://example.invalid/m.bin", "--id", "tiny"]),
    { url: "https://example.invalid/m.bin", id: "tiny" }
  );
});

test("args parses a bare switch as true", () => {
  assert.deepEqual(args(["--verify"]), { verify: true });
});

test("args treats a flag followed by another flag as a switch", () => {
  // `--verify --only tiny` must not swallow `--only` as --verify's value.
  assert.deepEqual(args(["--verify", "--only", "tiny"]), { verify: true, only: "tiny" });
});

test("args keeps hyphenated names verbatim so --expect-sha256 is reachable", () => {
  const parsed = args(["--expect-sha256", "ab".repeat(32), "--expect-size", "77691713"]);
  assert.equal(parsed["expect-sha256"], "ab".repeat(32));
  assert.equal(parsed["expect-size"], "77691713");
});

test("args ignores positional tokens", () => {
  assert.deepEqual(args(["measure", "--file", "m.bin", "trailing"]), { file: "m.bin" });
});

test("args lets a later flag win, rather than silently keeping the first", () => {
  assert.deepEqual(args(["--id", "a", "--id", "b"]), { id: "b" });
});

test("args accepts a URL that itself starts with no dashes but has query separators", () => {
  const url = "https://example.invalid/a.bin?download=true&x=1";
  assert.deepEqual(args(["--url", url]), { url });
});

/* --------------------------------------------------------------- measuring */

test("measureFile reproduces a known digest and the exact byte size", async () => {
  const dir = mkdtempSync(join(tmpdir(), "model-pin-test-"));
  try {
    const path = join(dir, "fixture.bin");
    // Not text: a model is binary, and a hash of bytes must not go through any
    // encoding on the way in.
    const bytes = Buffer.alloc(200_003);
    for (let i = 0; i < bytes.length; i += 1) bytes[i] = (i * 31 + 7) % 256;
    writeFileSync(path, bytes);

    const expected = createHash("sha256").update(bytes).digest("hex");
    const measured = await measureFile(path, "fixture");

    assert.equal(measured.sha256, expected);
    assert.equal(measured.sizeBytes, 200_003);
    assert.equal(measured.resolvedFrom, null);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("measureFile hashes an empty file rather than skipping it", async () => {
  const dir = mkdtempSync(join(tmpdir(), "model-pin-test-"));
  try {
    const path = join(dir, "empty.bin");
    writeFileSync(path, Buffer.alloc(0));
    const measured = await measureFile(path, "empty");
    // The digest of nothing, which is a real answer and not an error.
    assert.equal(
      measured.sha256,
      "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
    assert.equal(measured.sizeBytes, 0);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("measureFile streams a file larger than one chunk without changing the answer", async () => {
  const dir = mkdtempSync(join(tmpdir(), "model-pin-test-"));
  try {
    // Larger than the 64 KiB default highWaterMark, so the loop runs many
    // times. A hash that is only ever fed one chunk proves nothing about the
    // path a real model takes.
    const path = join(dir, "multi.bin");
    const bytes = Buffer.alloc(1_000_000, 0xab);
    writeFileSync(path, bytes);
    const measured = await measureFile(path, "multi");
    assert.equal(measured.sha256, createHash("sha256").update(bytes).digest("hex"));
    assert.equal(measured.sizeBytes, 1_000_000);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

/* ------------------------------------------------------- the Rust PINNED table */

const REAL_SHAPE = `
const PINNED: [(&str, &str, u64); 1] = [(
    "tiny",
    "be07e048e1e599ad46341c8d2a135645097a538221678b7acdd1b1919c6e1b21",
    77_691_713,
)];
`;

test("parseWhisperPins reads the committed table's shape", () => {
  assert.deepEqual(parseWhisperPins(REAL_SHAPE), [
    {
      id: "tiny",
      sha256: "be07e048e1e599ad46341c8d2a135645097a538221678b7acdd1b1919c6e1b21",
      sizeBytes: 77_691_713,
    },
  ]);
});

test("parseWhisperPins reads a multi-row table written on one line each", () => {
  const source = `
const PINNED: [(&str, &str, u64); 2] = [
    ("tiny", "${"a".repeat(64)}", 77_691_713),
    ("base", "${"b".repeat(64)}", 147_951_465),
];
`;
  const rows = parseWhisperPins(source);
  assert.deepEqual(
    rows.map((r) => [r.id, r.sizeBytes]),
    [
      ["tiny", 77_691_713],
      ["base", 147_951_465],
    ]
  );
});

test("parseWhisperPins lowercases a digest written in upper case", () => {
  const source = `const PINNED: [(&str, &str, u64); 1] = [("tiny", "${"A".repeat(64)}", 1)];`;
  assert.equal(parseWhisperPins(source)[0].sha256, "a".repeat(64));
});

test("parseWhisperPins throws when the table is gone", () => {
  assert.throws(() => parseWhisperPins("// no table here\n"), /no `const PINNED/);
});

test("parseWhisperPins throws when the declared arity disagrees with the rows", () => {
  // The checksum that matters: a table saying it holds three entries while
  // only two parse means a row is not a plain tuple, and verifying the two
  // that happened to parse would report a pass over an unchecked model.
  const source = `
const PINNED: [(&str, &str, u64); 3] = [
    ("tiny", "${"a".repeat(64)}", 1),
    ("base", "${"b".repeat(64)}", 2),
    ("small", SMALL_DIGEST, 3),
];
`;
  assert.throws(() => parseWhisperPins(source), /declares 3 entries and 2 parsed/);
});

test("parseWhisperPins rejects a digest that is not 64 hex characters", () => {
  // Caught by the arity checksum rather than by a separate rule: a short
  // digest fails to match the tuple pattern, the count comes up short, and
  // the parse throws instead of returning a table missing a row.
  const source = `const PINNED: [(&str, &str, u64); 1] = [("tiny", "deadbeef", 1)];`;
  assert.throws(() => parseWhisperPins(source), /declares 1 entries and 0 parsed/);
});

/* --------------------------------------------------------- pinned targets */

test("pinnedTargets joins the Rust pins to the registry for their URLs", () => {
  const reg = {
    localAsr: {
      whisper: [
        {
          id: "tiny",
          downloadUrl: "https://example.invalid/ggml-tiny.bin",
          // Rounded, as the real registry's are. The target must not use it.
          expectedSizeBytes: 78_000_000,
        },
      ],
      sherpa: [],
    },
  };
  const targets = pinnedTargets(reg, REAL_SHAPE);
  assert.equal(targets.length, 1);
  assert.equal(targets[0].kind, "whisper");
  assert.equal(targets[0].url, "https://example.invalid/ggml-tiny.bin");
  assert.equal(targets[0].sizeBytes, 77_691_713);
});

test("pinnedTargets takes sherpa pins from the registry and skips the unpinned", () => {
  const reg = {
    localAsr: {
      whisper: [{ id: "tiny", downloadUrl: "https://example.invalid/ggml-tiny.bin" }],
      sherpa: [
        {
          id: "pinned-model",
          downloadUrl: "https://example.invalid/pinned.tar.bz2",
          sha256: "c".repeat(64),
          expectedSizeBytes: 487_170_055,
        },
        {
          id: "unpinned-model",
          downloadUrl: "https://example.invalid/unpinned.tar.bz2",
          sha256: null,
          expectedSizeBytes: null,
        },
      ],
    },
  };
  const targets = pinnedTargets(reg, REAL_SHAPE);
  assert.deepEqual(
    targets.map((t) => `${t.kind}/${t.id}`),
    ["whisper/tiny", "sherpa/pinned-model"]
  );
  assert.equal(targets[1].sizeBytes, 487_170_055);
});

test("the committed registry and PINNED table actually join", () => {
  // Not a unit test of anything — an assertion that the two real files this
  // tool reads still line up, so `--verify` would have something to measure.
  // It is the check that would have caught the pins moving.
  const targets = pinnedTargets();
  assert.ok(targets.length > 0, "no committed model pins were discovered");
  for (const t of targets) {
    assert.match(t.sha256, /^[0-9a-f]{64}$/, `${t.id} has no usable digest`);
    assert.ok(t.url?.startsWith("https://"), `${t.id} has no https URL`);
  }
});
