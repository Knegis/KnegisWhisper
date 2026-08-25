#!/usr/bin/env node
// Fails if two workspace binaries share a name.
//
// Cargo puts every binary in one `target/<profile>/` directory, so two crates
// producing a binary with the same name silently overwrite each other and
// whichever linked last wins. There is no error and, depending on build order,
// no warning either.
//
// This is here because it happened. `apps/desktop/src-tauri` and
// `tools/knegiswhisper` both produced `knegiswhisper`, so
// `CARGO_BIN_EXE_knegiswhisper` in the CLI's end-to-end tests resolved to the
// desktop application — which the tests then launched fifteen times with
// command-line arguments it ignored, leaving a stray GUI window on screen and a
// locked executable that broke the next build. The tests had been passing.
//
// Deliberately a source check rather than `cargo metadata`: it runs in
// milliseconds with no toolchain, so it can sit in the same CI step as the other
// contract gates and fail before anything is compiled.

import { readFileSync, existsSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = join(dirname(fileURLToPath(import.meta.url)), "..", "..");

/** Members listed in the workspace root manifest. */
function members() {
  const root = readFileSync(join(ROOT, "Cargo.toml"), "utf8");
  const block = root.match(/members\s*=\s*\[([^\]]*)\]/s);
  if (!block) throw new Error("no [workspace] members array in Cargo.toml");
  return [...block[1].matchAll(/"([^"]+)"/g)].map((m) => m[1]);
}

/**
 * The binaries a member produces: every `[[bin]] name`, or the package name
 * when the crate has `src/main.rs` and declares no `[[bin]]`.
 */
function binariesOf(member) {
  const manifestPath = join(ROOT, member, "Cargo.toml");
  const manifest = readFileSync(manifestPath, "utf8");

  const declared = [...manifest.matchAll(/\[\[bin\]\]([\s\S]*?)(?=\n\[|$)/g)]
    .map((section) => section[1].match(/name\s*=\s*"([^"]+)"/))
    .filter(Boolean)
    .map((m) => m[1]);

  if (declared.length > 0) return declared;

  // An implicit binary exists only when src/main.rs does.
  if (!existsSync(join(ROOT, member, "src", "main.rs"))) return [];

  const name = manifest.match(/^\s*name\s*=\s*"([^"]+)"/m);
  return name ? [name[1]] : [];
}

const owners = new Map();
for (const member of members()) {
  for (const binary of binariesOf(member)) {
    if (!owners.has(binary)) owners.set(binary, []);
    owners.get(binary).push(member);
  }
}

const collisions = [...owners.entries()].filter(([, crates]) => crates.length > 1);

if (collisions.length > 0) {
  console.error("binary name collision — these would overwrite each other in target/:\n");
  for (const [binary, crates] of collisions) {
    console.error(`  ${binary}`);
    for (const crate of crates) console.error(`    ${crate}`);
  }
  console.error(
    "\nRename one. `CARGO_BIN_EXE_<name>` and anything invoking the binary by\n" +
      "path will silently get whichever crate linked most recently.",
  );
  process.exit(1);
}

const listed = [...owners.entries()]
  .map(([binary, [crate]]) => `${binary} (${crate})`)
  .sort()
  .join(", ");
console.log(`binary names OK — ${owners.size} distinct: ${listed}`);
