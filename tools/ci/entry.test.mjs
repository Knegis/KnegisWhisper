import test from "node:test";
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdirSync, mkdtempSync, realpathSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { isEntryPoint } from "./entry.mjs";

const TOOLS = resolve(dirname(fileURLToPath(import.meta.url)), "..");

/** A temporary directory, removed when the test ends. */
function scratch(t) {
  const dir = mkdtempSync(join(tmpdir(), "kw-entry-"));
  t.after(() => rmSync(dir, { recursive: true, force: true }));
  return dir;
}

/**
 * A link to `target` at `path`. A junction on Windows, which needs no
 * administrator; elsewhere the type is ignored and it is a symbolic link.
 */
function link(target, path) {
  symlinkSync(target, path, "junction");
}

test("the script Node was asked to run is the entry point, and nothing else is", (t) => {
  const dir = scratch(t);
  const script = join(dir, "x.mjs");
  const other = join(dir, "y.mjs");
  writeFileSync(script, "");
  writeFileSync(other, "");
  // What the loader sets `import.meta.url` to: the real path.
  const url = pathToFileURL(realpathSync(script)).href;

  assert.equal(isEntryPoint(url, script), true);
  assert.equal(isEntryPoint(url, other), false);
  assert.equal(isEntryPoint(url, undefined), false);
  assert.equal(isEntryPoint(url, join(dir, "missing.mjs")), false);
});

test("a script reached through a linked folder is still the entry point", (t) => {
  const dir = scratch(t);
  const real = join(dir, "real");
  mkdirSync(real);
  writeFileSync(join(real, "x.mjs"), "");
  link(real, join(dir, "linked"));

  const url = pathToFileURL(realpathSync(join(real, "x.mjs"))).href;
  assert.equal(isEntryPoint(url, join(dir, "linked", "x.mjs")), true);
});

// End to end, through Node's own loader: the two pin tools guard the weekly
// pin check, where a run that does nothing and exits 0 is a green check that
// checked nothing. Run with no arguments, each must refuse and exit 1.
for (const [tool, said] of [
  ["sidecar-pin", "--repo is required"],
  ["model-pin", "exactly one of --url"],
]) {
  test(`${tool} runs when reached through a linked folder`, (t) => {
    const dir = scratch(t);
    const linked = join(dir, tool);
    link(join(TOOLS, tool), linked);

    const run = spawnSync(process.execPath, [join(linked, "pin.mjs")], { encoding: "utf8" });
    assert.equal(run.status, 1, `exit ${run.status}; stderr: ${run.stderr}`);
    assert.match(run.stderr, new RegExp(said.replace(/[-]/g, "\\-")));
  });
}
