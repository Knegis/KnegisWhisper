#!/usr/bin/env node
/**
 * Tauri window capability coverage gate.
 *
 * Fails when the application creates a window that no Tauri capability covers.
 * KNEG-168 was that exact failure: `meeting` and `update` recorded microphone
 * and system audio for two minutes while their event listeners were denied and
 * their notification window rendered nothing. The frontend used
 * `void ...listen().then()`, so the rejected promise was discarded; every
 * surrounding layer logged success and nothing went red until 2026-09-04.
 *
 * **What it checks:**
 *
 *   A. Every label declared by `tauri.conf.json` or a Rust `pub const LABEL`
 *      is matched by a capability's `windows` list, unless the tracked
 *      exemption manifest names it.
 *   B. Every capability window pattern matches a label the application creates.
 *      A typo otherwise silently grants nothing.
 *   C. Every exemption names a real created window; the manifest cannot
 *      accumulate fiction.
 *   D. Every `WebviewWindowBuilder::new(` can be resolved as either the
 *      same-file `LABEL` const or a string literal that is already a known
 *      label. A builder the gate cannot resolve must fail, not become invisible.
 *   E. Every capability file is enabled by `tauri.conf.json`. Tauri's
 *      `security.capabilities` is an allow-list: unset or empty enables every
 *      file under `capabilities/`, anything else enables only the identifiers
 *      it names. KNEG-171 was a file that existed, satisfied rule A, and was
 *      never compiled into the ACL because the list named only `default` --
 *      so the KNEG-168 fix landed inert and the meeting window recorded with
 *      an empty indicator for a second time. Rule A and the success listing
 *      count only enabled capabilities as coverage.
 *
 * **What it does not check.** It checks that a window is *covered* by a
 * capability, not that the capability's permission set is sufficient.
 * Resolving `core:default` needs
 * `apps/desktop/src-tauri/gen/schemas/acl-manifests.json`, but `gen/` is
 * git-ignored and produced by the Rust build. The Contracts job runs on
 * ubuntu-latest without Rust, so only tracked files can be read here. Tauri
 * window patterns support more forms than this reader: it supports `*` only.
 * A future Tauri pattern this reader does not understand will produce a rule A
 * false alarm, which is the safe direction rather than silently missing a
 * window with no permissions.
 *
 * Usage:
 *   node tools/capability-audit/scan.mjs [--json]
 */

import { existsSync, readFileSync, readdirSync, statSync } from "node:fs";
import { dirname, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const REPO_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..", "..");
const TAURI_CONFIG = "apps/desktop/src-tauri/tauri.conf.json";
const RUST_SOURCE = "apps/desktop/src-tauri/src";
const CAPABILITIES = "apps/desktop/src-tauri/capabilities";

export const MANIFEST = "config/window-capabilities.json";

/** Return relative Rust source paths below `dir`, in stable order. */
export function rustFiles(root, dir = join(root, RUST_SOURCE)) {
  if (!existsSync(dir)) return [];
  const found = [];
  const walk = (current) => {
    for (const entry of readdirSync(current).sort()) {
      const path = join(current, entry);
      let stats;
      try {
        stats = statSync(path);
      } catch {
        continue; // a broken symlink cannot contain a builder
      }
      if (stats.isDirectory()) walk(path);
      else if (entry.endsWith(".rs")) found.push(relative(root, path).split("\\").join("/"));
    }
  };
  walk(dir);
  return found;
}

/**
 * Labels declared at the two sources Tauri windows can come from here.
 *
 * `source` deliberately records the source *kind*, not each Rust file: the
 * success count must show that both independent inventories were consulted.
 * The conventional Rust expression is intentionally narrow. Rule D protects
 * that choice by rejecting a builder that does not use the same convention.
 */
export function windowLabels(root = REPO_ROOT) {
  const labels = [];
  const config = join(root, TAURI_CONFIG);
  if (existsSync(config)) {
    const windows = JSON.parse(readFileSync(config, "utf8")).app?.windows ?? [];
    for (const window of windows) {
      if (typeof window?.label === "string") {
        labels.push({ label: window.label, source: "tauri.conf.json" });
      }
    }
  }

  for (const file of rustFiles(root)) {
    const source = readFileSync(join(root, file), "utf8");
    const match = /\bpub\s+const\s+LABEL\s*:\s*&str\s*=\s*"((?:\\.|[^"\\])*)"\s*;/.exec(source);
    if (match !== null) {
      labels.push({ label: JSON.parse(`"${match[1]}"`), source: "Rust pub const LABEL" });
    }
  }
  return labels;
}

/**
 * The `security.capabilities` allow-list from `tauri.conf.json`, or `null` when
 * it is unset or empty, which Tauri reads as "every file in `capabilities/`".
 * Entries are identifier strings or inline capability objects.
 */
export function securityCapabilities(root = REPO_ROOT) {
  const config = join(root, TAURI_CONFIG);
  if (!existsSync(config)) return null;
  const entries = JSON.parse(readFileSync(config, "utf8")).app?.security?.capabilities;
  return Array.isArray(entries) && entries.length > 0 ? entries : null;
}

/**
 * Mark each capability file `enabled` according to the allow-list, and append
 * any inline capability the list carries, since Tauri compiles those too.
 * A `null` list enables everything.
 */
export function markEnabled(files, entries) {
  if (entries === null) return files.map((capability) => ({ ...capability, enabled: true }));
  const named = new Set(entries.filter((entry) => typeof entry === "string"));
  const marked = files.map((capability) => ({ ...capability, enabled: named.has(capability.identifier) }));
  for (const entry of entries) {
    if (entry === null || typeof entry !== "object" || typeof entry.identifier !== "string") continue;
    marked.push({
      file: `${TAURI_CONFIG} (inline security.capabilities)`,
      identifier: entry.identifier,
      windows: Array.isArray(entry.windows) ? entry.windows : [],
      permissions: Array.isArray(entry.permissions) ? entry.permissions : [],
      enabled: true,
    });
  }
  return marked;
}

/**
 * Capability JSON files, retaining only the fields this source-level gate needs,
 * each marked `enabled` by whether `security.capabilities` lets Tauri load it.
 */
export function capabilityFiles(root = REPO_ROOT) {
  const directory = join(root, CAPABILITIES);
  if (!existsSync(directory)) return [];
  const files = readdirSync(directory)
    .filter((file) => file.endsWith(".json"))
    .sort()
    .map((name) => {
      const file = `${CAPABILITIES}/${name}`;
      const parsed = JSON.parse(readFileSync(join(directory, name), "utf8"));
      return {
        file,
        identifier: parsed.identifier ?? name,
        windows: Array.isArray(parsed.windows) ? parsed.windows : [],
        permissions: Array.isArray(parsed.permissions) ? parsed.permissions : [],
      };
    });
  return markEnabled(files, securityCapabilities(root));
}

/** A capability counts as coverage only if Tauri compiles it. Fixtures without the field are enabled. */
export function isEnabled(capability) {
  return capability.enabled !== false;
}

/** Whether `label` matches a Tauri window pattern using this gate's `*` subset. */
export function matchesWindowPattern(pattern, label) {
  if (typeof pattern !== "string" || typeof label !== "string") return false;
  const escaped = pattern
    .split("*")
    .map((part) => part.replace(/[|\\{}()[\]^$+?.]/g, "\\$&"))
    .join(".*");
  return new RegExp(`^${escaped}$`).test(label);
}

/** Remove Rust comments while preserving offsets and strings for builder parsing. */
export function stripRustComments(source) {
  let result = "";
  let index = 0;
  let blockDepth = 0;
  let quote = null;
  while (index < source.length) {
    const current = source[index];
    const next = source[index + 1];
    if (quote !== null) {
      result += current;
      if (current === "\\") result += source[++index] ?? "";
      else if (current === quote) quote = null;
      index += 1;
      continue;
    }
    if (blockDepth > 0) {
      if (current === "/" && next === "*") {
        blockDepth += 1;
        result += "  ";
        index += 2;
      } else if (current === "*" && next === "/") {
        blockDepth -= 1;
        result += "  ";
        index += 2;
      } else {
        result += current === "\n" ? "\n" : " ";
        index += 1;
      }
      continue;
    }
    if (current === "/" && next === "/") {
      while (index < source.length && source[index] !== "\n") {
        result += " ";
        index += 1;
      }
    } else if (current === "/" && next === "*") {
      blockDepth = 1;
      result += "  ";
      index += 2;
    } else {
      result += current;
      if (current === '"' || current === "'") quote = current;
      index += 1;
    }
  }
  return result;
}

/** Top-level arguments following an already-consumed opening parenthesis. */
export function builderArguments(source, start) {
  const args = [];
  let argument = "";
  let depth = 0;
  let quote = null;
  for (let index = start; index < source.length; index += 1) {
    const current = source[index];
    if (quote !== null) {
      argument += current;
      if (current === "\\") argument += source[++index] ?? "";
      else if (current === quote) quote = null;
      continue;
    }
    if (current === '"' || current === "'") {
      quote = current;
      argument += current;
    } else if (current === "(" || current === "[" || current === "{") {
      depth += 1;
      argument += current;
    } else if (current === ")" || current === "]" || current === "}") {
      if (current === ")" && depth === 0) {
        args.push(argument.trim());
        return args;
      }
      depth -= 1;
      argument += current;
    } else if (current === "," && depth === 0) {
      args.push(argument.trim());
      argument = "";
    } else {
      argument += current;
    }
  }
  return args;
}

/**
 * Each real WebviewWindowBuilder construction and the safely recognised form
 * of its second argument. Comments are stripped first: a code example in a
 * Rust comment is not a created window, and counting it would make rule D
 * noisy enough that a real unresolved builder could be ignored.
 */
export function builderSites(root = REPO_ROOT) {
  const sites = [];
  for (const file of rustFiles(root)) {
    const original = readFileSync(join(root, file), "utf8");
    const source = stripRustComments(original);
    const hasLabel = /\bpub\s+const\s+LABEL\s*:\s*&str\s*=\s*"(?:\\.|[^"\\])*"\s*;/.test(source);
    for (const match of source.matchAll(/\bWebviewWindowBuilder::new\s*\(/g)) {
      const args = builderArguments(source, match.index + match[0].length);
      const raw = args[1]?.trim() ?? "";
      const literal = /^"((?:\\.|[^"\\])*)"$/.exec(raw);
      const arg = literal === null ? raw : JSON.parse(`"${literal[1]}"`);
      const kind = raw === "LABEL" && hasLabel ? "same-file LABEL" : literal !== null ? "literal" : "unresolved";
      sites.push({
        file,
        line: source.slice(0, match.index).split("\n").length,
        arg,
        kind,
      });
    }
  }
  return sites;
}

/** Return every coverage, stale-config, stale-exemption, opaque-builder, and disabled-file violation. */
export function violations({ labels = [], capabilities = [], builders = [], exempt = {} }) {
  const found = [];
  const known = [...new Set(labels.map(({ label }) => label))];
  const enabled = capabilities.filter(isEnabled);

  // Rule A. KNEG-168: a label absent from every enabled capability receives no events.
  for (const label of known) {
    if (Object.hasOwn(exempt, label)) continue;
    if (enabled.some((capability) => capability.windows.some((pattern) => matchesWindowPattern(pattern, label)))) {
      continue;
    }
    const sources = [...new Set(labels.filter((item) => item.label === label).map((item) => item.source))].join(", ");
    found.push({
      rule: "A. created window is covered by no capability",
      subject: label,
      detail: `declared by ${sources} and matched by no enabled capability windows list. Add it to a tracked capability that security.capabilities enables, or exempt it in ${MANIFEST} with a reason if it intentionally has no Tauri permissions.`,
    });
  }

  // Rule E. KNEG-171: a capability file the allow-list does not name is never compiled into the ACL.
  for (const capability of capabilities) {
    if (isEnabled(capability)) continue;
    found.push({
      rule: "E. capability file is not enabled by security.capabilities",
      subject: capability.identifier,
      detail: `${capability.file} exists, but tauri.conf.json security.capabilities does not name ${JSON.stringify(capability.identifier)}, so Tauri never loads it and the windows it lists (${capability.windows.join(", ") || "none"}) get nothing from it. Add the identifier to security.capabilities, or remove that list so every file in ${CAPABILITIES} is enabled.`,
    });
  }

  // Rule B. A pattern matching no created window is a typo or stale grant.
  for (const capability of capabilities) {
    for (const pattern of capability.windows) {
      if (known.some((label) => matchesWindowPattern(pattern, label))) continue;
      found.push({
        rule: "B. capability names no created window",
        subject: `${capability.identifier}:${pattern}`,
        detail: `${capability.file} grants window pattern ${JSON.stringify(pattern)}, but no declared window matches it. Correct the label or remove the stale capability entry.`,
      });
    }
  }

  // Rule C. Exemptions are explicit decisions and must remain attached to reality.
  for (const [label, reason] of Object.entries(exempt)) {
    if (known.includes(label)) continue;
    found.push({
      rule: "C. exemption names no created window",
      subject: label,
      detail: `${MANIFEST} exempts it (${JSON.stringify(reason)}), but no declared window has that label. Remove the stale exemption or restore the window it documents.`,
    });
  }

  // Rule D. Unknown construction syntax would leave rule A blind to a window.
  for (const builder of builders) {
    if (builder.kind === "same-file LABEL") continue;
    if (builder.kind === "literal" && known.includes(builder.arg)) continue;
    found.push({
      rule: "D. window construction has no resolvable label",
      subject: `${builder.file}:${builder.line}`,
      detail: `WebviewWindowBuilder::new uses ${builder.arg === "" ? "no second argument" : JSON.stringify(builder.arg)}. Use the same-file pub const LABEL convention or a string literal matching a known label so rule A can cover this window.`,
    });
  }
  return found;
}

/** Read all tracked inputs. A missing manifest means no exemptions. */
export function collect(root = REPO_ROOT) {
  const manifest = join(root, MANIFEST);
  const exempt = existsSync(manifest) ? (JSON.parse(readFileSync(manifest, "utf8")).exempt ?? {}) : {};
  return {
    labels: windowLabels(root),
    capabilities: capabilityFiles(root),
    builders: builderSites(root),
    exempt,
  };
}

function main() {
  const input = collect();
  const found = violations(input);
  if (process.argv.includes("--json")) {
    console.log(JSON.stringify({ ...input, violations: found }, null, 2));
    process.exit(found.length === 0 ? 0 : 1);
  }
  if (found.length > 0) {
    console.error(`window capabilities FAILED — ${found.length} violation(s)\n`);
    const grouped = {};
    for (const violation of found) (grouped[violation.rule] ??= []).push(violation);
    for (const [rule, entries] of Object.entries(grouped)) {
      console.error(`  ${rule} (${entries.length})`);
      for (const violation of entries) console.error(`    ${violation.subject}: ${violation.detail}`);
      console.error("");
    }
    process.exit(1);
  }

  const sources = new Set(input.labels.map(({ source }) => source));
  const enabled = input.capabilities.filter(isEnabled);
  console.log(
    `window capabilities OK — ${input.labels.length} label(s) from ${sources.size} source(s), ` +
      `${input.capabilities.length} capability file(s) read, ${enabled.length} enabled by security.capabilities`
  );
  for (const label of [...new Set(input.labels.map(({ label }) => label))]) {
    const covering = enabled
      .filter((capability) => capability.windows.some((pattern) => matchesWindowPattern(pattern, label)))
      .map((capability) => capability.identifier);
    if (Object.hasOwn(input.exempt, label)) console.log(`  ${label.padEnd(16)} exempt — ${input.exempt[label]}`);
    else console.log(`  ${label.padEnd(16)} ${covering.join(", ")}`);
  }
  process.exit(0);
}

if (resolve(process.argv[1] ?? "") === fileURLToPath(import.meta.url)) {
  main();
}
