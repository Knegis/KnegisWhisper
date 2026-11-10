import test from "node:test";
import assert from "node:assert/strict";
import { mkdtempSync, mkdirSync, writeFileSync, rmSync } from "node:fs";
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
    let output;
    dependencyNotices().generateBundle.call({emitFile: x => { output = x; }}, {}, {
      main: {type:"chunk",modules:{[join(root,"node_modules","shipped","index.js")]:{}}},
    });
    const packages = JSON.parse(output.source);
    assert.equal(packages.length, 1);
    assert.equal(packages[0].name, "shipped");
    assert.deepEqual(packages[0].texts, ["Copyright shipped"]);
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
