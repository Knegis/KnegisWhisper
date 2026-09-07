import { readdirSync, rmSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { spawnSync } from "node:child_process";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const output = join(root, ".test-build");
// Only this fixed output directory is removed; no shell or user-supplied path.
if (dirname(output) !== root) throw new Error("Test output escaped the desktop directory");
rmSync(output, { recursive: true, force: true });

function run(args) {
  const result = spawnSync(process.execPath, args, { cwd: root, stdio: "inherit" });
  if (result.error) throw result.error;
  if (result.status !== 0) process.exit(result.status ?? 1);
}

run([join(root, "node_modules/typescript/bin/tsc"), "-p", "tsconfig.test.json"]);

function testsIn(directory) {
  return readdirSync(directory, { withFileTypes: true }).flatMap((entry) => {
    const path = join(directory, entry.name);
    return entry.isDirectory() ? testsIn(path) : entry.name.endsWith(".test.js") ? [path] : [];
  });
}

const tests = testsIn(output).sort();
if (tests.length === 0) throw new Error("No compiled TypeScript tests found");
run(["--test", ...tests]);
