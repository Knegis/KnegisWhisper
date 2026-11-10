import test from "node:test";
import assert from "node:assert/strict";
import { mkdtempSync, mkdirSync, writeFileSync, rmSync, readFileSync, existsSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { dependencyNotices } from "./frontend.mjs";

test("inventory contains only emitted package modules and preserves their notices", () => {
  const root = mkdtempSync(join(tmpdir(), "kw-notices-"));
  try {
    for (const name of ["shipped", "development"]) {
      const dir = join(root, "node_modules", name);
      mkdirSync(dir, {recursive:true});
      writeFileSync(join(dir, "package.json"), JSON.stringify({name,version:"1.0.0",license:"MIT"}));
      writeFileSync(join(dir, "LICENSE"), "Copyright " + name);
    }
    const plugin = dependencyNotices();
    plugin.configResolved({ root });
    const output = join(root, ".notice-build", "dependency-notices.json");
    mkdirSync(join(root, ".notice-build"));
    writeFileSync(output, "stale inventory");
    plugin.buildStart();
    assert.equal(existsSync(output), false);
    plugin.generateBundle.call({emitFile() { assert.fail("inventory must not be bundled"); }}, {}, {
      main: {type:"chunk",modules:{[join(root,"node_modules","shipped","index.js")]:{}}},
    });
    assert.equal(existsSync(output), false);
    plugin.writeBundle();
    assert.equal(existsSync(join(root, "dist", "dependency-notices.json")), false);
    const packages = JSON.parse(readFileSync(output, "utf8"));
    assert.equal(packages.length, 1);
    assert.equal(packages[0].name, "shipped");
    assert.deepEqual(packages[0].texts, ["Copyright shipped"]);
    plugin.renderError();
    assert.equal(existsSync(output), false);
  } finally { rmSync(root, {recursive:true,force:true}); }
});

test("a bundled package without a license stops the build", () => {
  const root = mkdtempSync(join(tmpdir(), "kw-notices-"));
  try {
    const dir = join(root, "node_modules", "missing");
    mkdirSync(dir, {recursive:true});
    writeFileSync(join(dir, "package.json"), JSON.stringify({name:"missing",version:"1"}));
    assert.throws(() => dependencyNotices().generateBundle.call({emitFile(){}}, {}, {
      main:{type:"chunk",modules:{[join(dir,"index.js")]:{}}},
    }), /Missing bundled license/);
  } finally { rmSync(root, {recursive:true,force:true}); }
});
