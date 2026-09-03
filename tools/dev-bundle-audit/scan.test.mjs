/**
 * Self-tests for the dev pre-bundle audit.
 *
 * Same argument as the other gates' self-tests: a rule that stops matching
 * still reports green, so each rule is shown to fire against a constructed
 * input before it is trusted against the tree. The first draft of this gate
 * was exactly that failure -- it matched `// node_modules/...` and the scratch
 * cache wrote `// ../../../.../node_modules/...` -- so the banner test below
 * pins every path shape esbuild has been seen to write.
 */

import assert from "node:assert/strict";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";

import {
  bannerPattern,
  FORBIDDEN_MODULES,
  FORBIDDEN_PATTERNS,
  readChunks,
  scanChunks,
} from "./scan.mjs";

const lodash = "lodash-es/lodash.default.js";
const transformerBanner = "// node_modules/@milkdown/transformer/lib/index.js";
const unpatchedLine = "    this.toString = (remark) => remark.stringify(this.build());";
const patchedLine =
  '    Object.defineProperty(this, "toString", { configurable: true, writable: true, ' +
  "value: (remark) => remark.stringify(this.build()) });";

test("the lodash-es full build is a forbidden module, with a reason", () => {
  const entry = FORBIDDEN_MODULES.find((m) => m.module === lodash);
  assert.ok(entry, "lodash.default.js must stay on the list; it is the measured offender");
  assert.ok(entry.reason.length > 20, "an entry without a reason cannot be re-checked");
});

test("a chunk carrying the lodash.default banner is reported with its chunk name", () => {
  const hits = scanChunks([
    { name: "chunk-AAAA.js", text: "// node_modules/lodash-es/throttle.js\nexport {};" },
    { name: "chunk-BBBB.js", text: `x;\n// node_modules/${lodash}\nvar wrapperLodash;` },
  ]);
  assert.deepEqual(
    hits.map((h) => [h.chunk, h.module]),
    [["chunk-BBBB.js", lodash]],
  );
});

test("the banner is matched wherever the output directory was", () => {
  for (const banner of [
    `// node_modules/${lodash}`,
    `// ../../../Users/someone/repo/apps/desktop/node_modules/${lodash}`,
    `// C:/Users/someone/repo/apps/desktop/node_modules/${lodash}`,
    `// ${lodash}`,
  ]) {
    const hits = scanChunks([{ name: "c.js", text: `var a;\n${banner}\nvar b;` }]);
    assert.equal(hits.length, 1, banner);
    assert.equal(hits[0].banner, banner);
  }
});

test("a different module that merely ends the same way, or a mention in code, is not a hit", () => {
  assert.deepEqual(scanChunks([{ name: "c.js", text: `// node_modules/not-${lodash}` }]), []);
  assert.deepEqual(
    scanChunks([{ name: "c.js", text: `var s = "${lodash}";` }]),
    [],
    "a string inside code is not a module banner",
  );
  assert.ok(bannerPattern("a/b.js").test("// x/a/b.js"));
  assert.ok(!bannerPattern("a/b.js").test("// x/a/b.jsx"));
  assert.ok(!bannerPattern("a/b.js").test("// x/a/bXjs"), "the dot is literal");
});

test("the unpatched Milkdown serializer line is a hit, and the patched form is not", () => {
  const hit = scanChunks([{ name: "c.js", text: `${transformerBanner}\n${unpatchedLine}\n` }]);
  assert.equal(hit.length, 1);
  assert.equal(hit[0].module, "@milkdown/transformer/lib/index.js");
  assert.deepEqual(scanChunks([{ name: "c.js", text: `${transformerBanner}\n${patchedLine}\n` }]), []);
  assert.ok(FORBIDDEN_PATTERNS.every((p) => p.reason.length > 20));
});

test("per-method lodash modules are not hits", () => {
  const hits = scanChunks([
    {
      name: "chunk-KUDINAM5.js",
      text:
        "// node_modules/lodash-es/debounce.js\n// node_modules/lodash-es/throttle.js\n" +
        "// node_modules/lodash-es/defaultsDeep.js\nexport { debounce_default, throttle_default };",
    },
  ]);
  assert.deepEqual(hits, []);
});

test("both lists are injectable, so a second offender is one entry away", () => {
  const hits = scanChunks(
    [{ name: "c.js", text: "// node_modules/example/proto-writer.js\nobj.valueOf = f;" }],
    [{ module: "example/proto-writer.js", reason: "test" }],
    [{ pattern: /obj\.valueOf = f;/, module: "example/proto-writer.js", reason: "test" }],
  );
  assert.equal(hits.length, 2);
});

test("readChunks reads only .js files, sorted, and an absent directory reads as nothing", () => {
  const dir = mkdtempSync(join(tmpdir(), "kw-dev-bundle-audit-test-"));
  try {
    writeFileSync(join(dir, "b.js"), "bee");
    writeFileSync(join(dir, "a.js"), "ay");
    writeFileSync(join(dir, "a.js.map"), "{}");
    writeFileSync(join(dir, "_metadata.json"), "{}");
    assert.deepEqual(
      readChunks(dir).map((c) => [c.name, c.text]),
      [
        ["a.js", "ay"],
        ["b.js", "bee"],
      ],
    );
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
  assert.deepEqual(readChunks(join(dir, "gone")), []);
});
