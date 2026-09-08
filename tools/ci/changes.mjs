import { execFileSync } from "node:child_process";
import { appendFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { resolve } from "node:path";

export function classifyChanges(paths) {
  const shared = paths.some(p => /^(?:config|tools|scripts|vendor|\.github|\.cargo)\//.test(p)
    || /^(?:Cargo\.(?:toml|lock)|rust-toolchain(?:\.toml)?|rustfmt\.toml|clippy\.toml|package(?:-lock)?\.json)$/.test(p));
  const frontend = shared || paths.some(p => p.startsWith("apps/desktop/") && !p.startsWith("apps/desktop/src-tauri/"));
  const windows = shared || frontend || paths.some(p => p.startsWith("crates/") || p.startsWith("apps/desktop/src-tauri/") || p.endsWith(".rs"));
  return { frontend, windows };
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const { KW_BASE_SHA: base, KW_HEAD_SHA: head, GITHUB_OUTPUT: output } = process.env;
  if (!base || !head || !output) throw new Error("Base, head and output are required");
  if (![base, head].every(sha => /^[a-f0-9]{40}$/.test(sha))) throw new Error("Expected commit SHAs");
  const paths = execFileSync("git", ["diff", "--no-renames", "--name-only", "-z", `${base}...${head}`], { encoding: "utf8" }).split("\0").filter(Boolean);
  const result = classifyChanges(paths);
  for (const [key, value] of Object.entries(result)) appendFileSync(output, `${key}=${value}\n`);
  console.log(result);
}
