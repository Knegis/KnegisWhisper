import test from "node:test";
import assert from "node:assert/strict";
import { classifyChanges } from "./changes.mjs";

test("documentation only runs contracts", () => {
  assert.deepEqual(classifyChanges(["README.md", "docs/SECURITY.md"]), { frontend: false, windows: false });
});
test("frontend inputs also compile the Windows desktop", () => {
  for (const path of ["apps/desktop/src/views/Notes.tsx", "apps/desktop/package-lock.json", "apps/desktop/vite.config.ts"]) {
    assert.deepEqual(classifyChanges([path]), { frontend: true, windows: true });
  }
});
test("native security, platform and sidecar changes run Windows", () => {
  for (const path of ["crates/security/src/file_acl.rs", "crates/platform/src/keyhook.rs", "crates/sidecar/src/process.rs", "apps/desktop/src-tauri/tauri.conf.json"]) {
    assert.deepEqual(classifyChanges([path]), { frontend: false, windows: true });
  }
});
test("shared build inputs and workflows run both", () => {
  for (const path of ["Cargo.lock", "Cargo.toml", "config/settings-keys.json", ".cargo/config.toml", ".github/workflows/ci.yml", "tools/ci/changes.mjs", "rust-toolchain.toml"]) {
    assert.deepEqual(classifyChanges([path]), { frontend: true, windows: true });
  }
});
