#!/usr/bin/env node
/**
 * Forbidden-domain scanner.
 *
 * Fails the build if a vendor service host or identifier appears anywhere it
 * could reach a user: source, config, built assets, or CI workflows.
 *
 * The workflow coverage is not incidental. The upstream audit found that
 * OpenWhispr's CI still defaults `VITE_AUTH_URL` to `https://auth.openwhispr.com`
 * while the main process reads it with no guard — a vendor host that reaches the
 * shipped binary and is invisible to any `src/`-only grep. The scanner also
 * blocks `OpenWhispr/whisper.cpp` and the vendor's release-asset paths, because
 * depending on a vendor's GitHub org for binaries is a vendor dependency even
 * when no vendor domain appears in the code.
 *
 * Usage:
 *   node tools/domain-scanner/scan.mjs [--root <dir>] [--json]
 * Exit:
 *   0 = clean, 1 = violations found
 *
 * No dependencies, by design: this must run on a bare CI image.
 */

import { readFileSync, readdirSync, statSync } from "node:fs";
import { join, relative, resolve, sep, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const DEFAULT_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..", "..");

/** Directories never worth walking. `dist` is handled via builtAssetPaths. */
const PRUNE = new Set([".git", "node_modules", "target", ".vite", "gen"]);

/**
 * Convert a glob to a RegExp.
 *
 * Supports `**` (any depth, including none), `*` (within one segment), `?`, and
 * `{a,b}` alternation. Deliberately small — the patterns in
 * config/forbidden-domains.json are simple, and a full glob engine would be a
 * dependency we do not need.
 */
export function globToRegExp(glob) {
  let out = "";
  for (let i = 0; i < glob.length; i += 1) {
    const c = glob[i];
    if (c === "*") {
      if (glob[i + 1] === "*") {
        // `**/` should also match zero directories.
        if (glob[i + 2] === "/") {
          out += "(?:.*/)?";
          i += 2;
        } else {
          out += ".*";
          i += 1;
        }
      } else {
        out += "[^/]*";
      }
    } else if (c === "?") {
      out += "[^/]";
    } else if (c === "{") {
      const end = glob.indexOf("}", i);
      if (end === -1) {
        out += "\\{";
      } else {
        const alts = glob.slice(i + 1, end).split(",");
        out += `(?:${alts.map((a) => a.replace(/[.+^${}()|[\]\\]/g, "\\$&")).join("|")})`;
        i = end;
      }
    } else if (".+^$()|[]\\".includes(c)) {
      out += `\\${c}`;
    } else {
      out += c;
    }
  }
  return new RegExp(`^${out}$`);
}

function walk(dir, root, acc) {
  let entries;
  try {
    entries = readdirSync(dir, { withFileTypes: true });
  } catch {
    return acc;
  }
  for (const entry of entries) {
    const full = join(dir, entry.name);
    if (entry.isDirectory()) {
      if (!PRUNE.has(entry.name)) walk(full, root, acc);
    } else if (entry.isFile()) {
      acc.push(relative(root, full).split(sep).join("/"));
    }
  }
  return acc;
}

/** Files larger than this are almost certainly binary; skip them. */
const MAX_BYTES = 4 * 1024 * 1024;

/**
 * Inline, per-line suppression.
 *
 * Preferred over allowlisting a whole file whenever only a few lines
 * legitimately need a forbidden string — notably the runtime blocklist itself,
 * which has to name the hosts it blocks. Blinding the scanner to an entire file
 * is worst exactly where it matters most, so this keeps the exemption at the
 * line, in view of anyone reading the code.
 *
 * Two forms, both requiring a reason — the bare marker does not suppress:
 *
 *   `domain-scanner:allow <reason>`             suppresses this line
 *   `domain-scanner:allow-next-line <reason>`   suppresses the following line
 *
 * The next-line form exists so an exemption can sit on its own comment line
 * instead of being wedged into the middle of an expression.
 */
const SUPPRESS_NEXT = /domain-scanner:allow-next-line\s+(\S.*)$/;
const SUPPRESS_LINE = /domain-scanner:allow(?!-next-line)\s+(\S.*)$/;

function isProbablyText(path, root) {
  try {
    if (statSync(join(root, path)).size > MAX_BYTES) return false;
  } catch {
    return false;
  }
  return true;
}

/**
 * Run the scan.
 *
 * @param {{root?: string, config?: object}} options
 * @returns {{violations: Array, scanned: number, patterns: number}}
 */
export function scan({ root = DEFAULT_ROOT, config } = {}) {
  const cfg =
    config ?? JSON.parse(readFileSync(join(root, "config/forbidden-domains.json"), "utf8"));

  const scanMatchers = [...(cfg.scanPaths ?? []), ...(cfg.builtAssetPaths ?? [])].map(
    globToRegExp
  );
  const allowMatchers = (cfg.allowlist ?? []).map((a) => ({
    re: globToRegExp(a.path),
    reason: a.reason,
  }));
  const patterns = (cfg.forbidden ?? []).map((f) => ({
    re: new RegExp(f.pattern, "i"),
    source: f.pattern,
    reason: f.reason,
  }));

  const all = walk(root, root, []);
  const violations = [];
  let scanned = 0;
  let suppressed = 0;

  for (const rel of all) {
    if (!scanMatchers.some((m) => m.test(rel))) continue;
    if (allowMatchers.some((a) => a.re.test(rel))) continue;
    if (!isProbablyText(rel, root)) continue;

    let text;
    try {
      text = readFileSync(join(root, rel), "utf8");
    } catch {
      continue;
    }
    scanned += 1;

    const lines = text.split(/\r?\n/);
    let suppressNext = false;
    for (const [idx, line] of lines.entries()) {
      const skip = suppressNext || SUPPRESS_LINE.test(line);
      suppressNext = SUPPRESS_NEXT.test(line);
      if (skip) {
        suppressed += 1;
        continue;
      }
      for (const p of patterns) {
        if (p.re.test(line)) {
          violations.push({
            file: rel,
            line: idx + 1,
            pattern: p.source,
            reason: p.reason,
            // Truncated so a long minified line does not flood the log.
            excerpt: line.trim().slice(0, 160),
          });
        }
      }
    }
  }

  return { violations, scanned, suppressed, patterns: patterns.length };
}

// --- CLI --------------------------------------------------------------------
const isMain = process.argv[1] && resolve(process.argv[1]) === resolve(fileURLToPath(import.meta.url));
if (isMain) {
  const args = process.argv.slice(2);
  const rootIdx = args.indexOf("--root");
  const root = rootIdx === -1 ? DEFAULT_ROOT : resolve(args[rootIdx + 1] ?? DEFAULT_ROOT);
  const asJson = args.includes("--json");

  const result = scan({ root });

  if (asJson) {
    console.log(JSON.stringify(result, null, 2));
    process.exit(result.violations.length === 0 ? 0 : 1);
  }

  if (result.violations.length === 0) {
    console.log(
      `domain scan OK — ${result.scanned} files scanned against ${result.patterns} patterns` +
        (result.suppressed ? `, ${result.suppressed} line(s) suppressed inline` : "")
    );
    process.exit(0);
  }

  console.error(
    `domain scan FAILED — ${result.violations.length} violation(s) in ${result.scanned} files scanned\n`
  );
  for (const v of result.violations) {
    console.error(`  ${v.file}:${v.line}`);
    console.error(`    pattern: ${v.pattern}`);
    console.error(`    reason:  ${v.reason}`);
    console.error(`    line:    ${v.excerpt}`);
    console.error("");
  }
  console.error(
    "If this is legitimate documentation or a test fixture, add the path to the\n" +
      "allowlist in config/forbidden-domains.json with a reason. Do not widen a\n" +
      "pattern to make a real dependency pass."
  );
  process.exit(1);
}
