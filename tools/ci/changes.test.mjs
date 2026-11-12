import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { classifyChanges } from "./changes.mjs";

test("documentation only runs contracts", () => {
  assert.deepEqual(classifyChanges(["README.md", "docs/SECURITY.md"]), { frontend: false, windows: false, notices: false });
});
test("frontend inputs also compile the Windows desktop", () => {
  for (const path of ["apps/desktop/src/views/Notes.tsx", "apps/desktop/package-lock.json", "apps/desktop/vite.config.ts"]) {
    assert.deepEqual(classifyChanges([path]), { frontend: true, windows: true, notices: true });
  }
});
test("native security, platform and sidecar changes run Windows", () => {
  for (const path of ["crates/security/src/file_acl.rs", "crates/platform/src/keyhook.rs", "crates/sidecar/src/process.rs", "apps/desktop/src-tauri/tauri.conf.json"]) {
    assert.deepEqual(classifyChanges([path]), { frontend: false, windows: true, notices: true });
  }
});
test("shared build inputs and workflows run both", () => {
  for (const path of ["Cargo.lock", "Cargo.toml", "config/settings-keys.json", ".cargo/config.toml", ".github/workflows/ci.yml", "tools/ci/changes.mjs", "rust-toolchain.toml"]) {
    assert.deepEqual(classifyChanges([path]), { frontend: true, windows: true, notices: true });
  }
});

test("vendored dependency changes cannot bypass the Windows build", () => {
  for (const path of ["vendor/cpal/Cargo.toml", "vendor/cpal/build.rs", "vendor/cpal/src/host/wasapi/device.rs"]) {
    assert.deepEqual(classifyChanges([path]), { frontend: true, windows: true, notices: true });
  }
});

test("standalone license and notice inputs run notices without a desktop build", () => {
  for (const path of ["NOTICE", "LICENSE", "LICENSE.txt", "COPYING", ".gitattributes"]) {
    assert.deepEqual(classifyChanges([path]), { frontend: false, windows: false, notices: true });
  }
});

test("bundled notices, fonts and generator inputs cannot bypass freshness", () => {
  for (const path of ["apps/desktop/src-tauri/resources/THIRD_PARTY_NOTICES.txt",
    "apps/desktop/src/fonts/OFL-Geist.txt", "tools/licenses/frontend.mjs", "tools/licenses/assets/sources.json"]) {
    assert.equal(classifyChanges([path]).notices, true);
  }
});

test("CI wires notices selection into the Windows job and preserves manual runs", () => {
  const workflow = readFileSync(new URL("../../.github/workflows/ci.yml", import.meta.url), "utf8");
  assert.match(workflow, /notices: \$\{\{ steps\.paths\.outputs\.notices \}\}/);
  const job = workflow.split("\n  notices:\n")[1];
  assert.ok(job, "notices job exists");
  assert.match(job, /needs: \[changes, contracts\]/);
  assert.match(job, /if: github\.event_name == 'workflow_dispatch' \|\| needs\.changes\.outputs\.notices == 'true'/);
});
