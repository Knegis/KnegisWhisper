#!/usr/bin/env node
/**
 * Settings-key reachability gate.
 *
 * Fails the build when the Rust side reads a setting that nothing in the
 * shipped application can change.
 *
 * **Why this exists.** Three defects in this repository have had one shape, and
 * all three were found by a person reading a screen rather than by any test:
 *
 *   1. Four hotkeys that registered and dispatched nowhere.
 *   2. `history.keepRawText`, hardcoded `false` at the only real call site,
 *      under a comment calling it opt-in (KNEG-98).
 *   3. `history.enabled`, honoured before every history row and settable only
 *      by editing SQLite by hand (KNEG-99).
 *
 * Each time the backend honoured a setting, the setting had a key, a default
 * and a passing unit test, and nothing a user could reach could change it. The
 * tests passed in every case -- which is exactly why the tests were no help: a
 * test sets the value directly, so it exercises the branch and says nothing
 * about whether anybody can get to it. All three are fixed; the fourth was not
 * prevented. This is the prevention (KNEG-104).
 *
 * **What it checks**, against `config/settings-keys.json`:
 *
 *   A. Every settings key reachable from a `kw_storage::settings` call in the
 *      desktop backend is declared. A key expression this scanner cannot
 *      resolve is a violation, never a skip -- see [`resolveKey`].
 *   B. Every declared key is really read or written somewhere. The manifest
 *      does not get to accumulate fiction either.
 *   C. Every declared key either names the `#[tauri::command]`s that write it
 *      or says, in a sentence, why no user writes it.
 *   D. Those commands exist, are registered in `generate_handler!`, and
 *      actually write that key. A manifest naming a command that does not
 *      touch the key would be a second place to write the same unchecked
 *      string, which is the failure mode rule 7 of the parity gate was added
 *      to fix.
 *   E. Every command that writes a key is declared as a writer of it, so the
 *      manifest cannot go stale in the other direction.
 *   F. A key claiming to be reachable resolves through `config/ui-surfaces.json`
 *      to a built component whose source **uses** the IPC wrapper for one of
 *      its writers — see [`usesWrapper`] for why importing it is not enough.
 *      `reachable: false` is allowed and must carry a note; those are printed
 *      as a backlog on success, exactly as the parity gate does with its
 *      unreachable integrations.
 *
 * **The generic writer does not count.** `settings_set(key, value)` writes any
 * key the renderer names, so counting it as a writer would make this gate pass
 * for every key ever added and prove nothing. Commands whose key argument comes
 * from the renderer are computed from source, not hardcoded, and refused as
 * writers.
 *
 * **What it does not check.** That a key changes behaviour, that the screen's
 * control is correct, or that anything is wired behind the setting -- the
 * hotkey keys are writable from Settings > Shortcuts and four of the five
 * actions still have no handler, which `commands::hotkeys` reports separately.
 * This gate answers one question: can a user change the value at all.
 *
 * Usage:
 *   node tools/settings-audit/scan.mjs [--json]
 * Exit:
 *   0 = every key is accounted for, 1 = one or more violations
 *
 * No dependencies, by design: this runs in the cheap contracts job on a bare
 * CI image with no Rust or Node toolchain beyond node itself.
 */

import { readFileSync, existsSync, readdirSync } from "node:fs";
import { join, dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const REPO_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..", "..");

/** The only tree that talks to `kw_storage::settings`, verified by rule A's own scan. */
export const BACKEND_DIR = "apps/desktop/src-tauri/src";

/** Where the renderer's IPC wrappers live. */
export const IPC_MODULE = "apps/desktop/src/lib/ipc.ts";

/* ------------------------------------------------------------------ parsing */

/**
 * Walk forward from the opening delimiter at `from` to its match.
 *
 * String literals are skipped so a brace or paren inside one cannot unbalance
 * the count. Rust lifetimes (`'a`) are deliberately not treated as quotes --
 * only `"` opens a string here, and a lifetime cannot contain a delimiter.
 *
 * Returns the index of the matching close, or -1 if the source runs out.
 */
export function matchDelimiter(src, from) {
  const open = src[from];
  const close = { "(": ")", "{": "}", "[": "]" }[open];
  if (!close) return -1;
  let depth = 0;
  for (let i = from; i < src.length; i += 1) {
    const c = src[i];
    if (c === '"') {
      i += 1;
      while (i < src.length && src[i] !== '"') i += src[i] === "\\" ? 2 : 1;
      continue;
    }
    if (c === "/" && src[i + 1] === "/") {
      while (i < src.length && src[i] !== "\n") i += 1;
      continue;
    }
    if (c === open) depth += 1;
    else if (c === close) {
      depth -= 1;
      if (depth === 0) return i;
    }
  }
  return -1;
}

/** Split an argument list on top-level commas. */
function splitArgs(args) {
  const parts = [];
  let depth = 0;
  let cur = "";
  for (let i = 0; i < args.length; i += 1) {
    const c = args[i];
    if (c === '"') {
      cur += c;
      i += 1;
      while (i < args.length && args[i] !== '"') {
        cur += args[i];
        if (args[i] === "\\") {
          cur += args[i + 1] ?? "";
          i += 1;
        }
        i += 1;
      }
      cur += '"';
      continue;
    }
    if ("([<".includes(c)) depth += 1;
    if (")]>".includes(c)) depth -= 1;
    if (c === "," && depth === 0) {
      parts.push(cur.trim());
      cur = "";
    } else cur += c;
  }
  if (cur.trim()) parts.push(cur.trim());
  return parts;
}

/**
 * Every function in one Rust source, with whether it is a Tauri command.
 *
 * A call site is attributed to the last function declared at or before it.
 * That is not a call graph and does not pretend to be: a key written only by a
 * helper the command calls reads as unwritten and fails the gate. That
 * direction is the safe one -- it is loud, and the fix is a sentence in the
 * manifest rather than a silent pass. Measured on the committed tree
 * (2026-08-31): every production write except calendar's OAuth bookkeeping and
 * `local_asr::clear_selection` sits directly inside a command, so nothing is
 * mis-attributed today.
 *
 * It also means test code needs no special handling for the writer rules. A
 * `#[cfg(test)]` function is never a `#[tauri::command]`, so a test that sets a
 * value directly -- the exact thing that made the three past defects invisible
 * -- cannot count as the way a user changes it.
 */
export function functions(source) {
  const lines = source.split("\n");
  const found = [];
  let offset = 0;
  for (let i = 0; i < lines.length; i += 1) {
    const sig = /^\s*(?:pub(?:\([^)]*\))?\s+)?(?:const\s+)?(?:async\s+)?fn\s+(\w+)/.exec(lines[i]);
    if (sig) {
      let isCommand = false;
      let j = i - 1;
      // Walk back over the attribute block: attributes, doc comments, blank
      // lines, and the closing line of a multi-line attribute.
      const attribute = (line) =>
        /^\s*(#\[|\/\/)/.test(line) || line.trim() === "" || /^\s*\)/.test(line);
      while (j >= 0 && attribute(lines[j])) {
        if (lines[j].includes("tauri::command")) isCommand = true;
        j -= 1;
      }
      found.push({ name: sig[1], at: offset, line: i + 1, isCommand });
    }
    offset += lines[i].length + 1;
  }
  return found;
}

/** `const NAME: &str = "value";`, from anywhere in the scanned sources. */
export function constants(sources) {
  const out = new Map();
  const re = /(?:pub\s+)?const\s+([A-Z][A-Z0-9_]*)\s*:\s*&(?:'static\s+)?str\s*=\s*"([^"]*)"\s*;/g;
  for (const source of Object.values(sources)) {
    let m;
    while ((m = re.exec(source)) !== null) out.set(m[1], m[2]);
  }
  return out;
}

/**
 * `fn name(..) -> &'static str` and every literal it can return.
 *
 * This is how the calendar keys are written: one small match per provider,
 * beside the runtime that uses them. Collecting the arms is enough -- the
 * function returns nothing else -- and it means adding a third provider adds
 * its keys to this gate's view without anyone remembering to.
 */
export function keyHelpers(sources) {
  const out = new Map();
  const re = /fn\s+(\w+)\s*\([^)]*\)\s*->\s*&'static\s+str\s*\{/g;
  for (const source of Object.values(sources)) {
    let m;
    while ((m = re.exec(source)) !== null) {
      const open = source.indexOf("{", m.index + m[0].length - 1);
      const close = matchDelimiter(source, open);
      if (close === -1) continue;
      const body = source.slice(open, close);
      const keys = [...body.matchAll(/"([^"]*)"/g)].map((s) => s[1]);
      if (keys.length > 0) out.set(m[1], keys);
    }
  }
  return out;
}

/** Every `settings::{get,get_or,set,remove}` call in the backend sources. */
export function callSites(sources) {
  const sites = [];
  for (const [file, source] of Object.entries(sources)) {
    const fns = functions(source);
    const re = /(?:kw_storage::)?settings::(get_or|get|set|remove)\s*(?:::<[^>]*>)?\s*\(/g;
    let m;
    while ((m = re.exec(source)) !== null) {
      // `settings::get` is a prefix of `settings::get_or`; the alternation is
      // ordered so the longer one wins, but a bare identifier boundary is still
      // needed for `settings::getter`-shaped names that do not exist today.
      const open = m.index + m[0].length - 1;
      const close = matchDelimiter(source, open);
      if (close === -1) continue;
      const args = splitArgs(source.slice(open + 1, close));
      let owner = null;
      for (const fn of fns) {
        if (fn.at <= m.index) owner = fn;
        else break;
      }
      sites.push({
        file,
        line: source.slice(0, m.index).split("\n").length,
        op: m[1],
        expression: (args[1] ?? "").replace(/\s+/g, " ").trim(),
        fn: owner?.name ?? "(top level)",
        isCommand: owner?.isCommand ?? false,
      });
    }
  }
  return sites;
}

/* --------------------------------------------------------------- resolution */

/**
 * Reduce a key argument to the form the manifest declares it under.
 *
 * A receiver is dropped from a method call so `&action.settings_key()` and
 * `&HotkeyAction::Dictation.settings_key()` are one expression rather than two
 * -- they produce the same family from the same function, and making the
 * manifest list every receiver would be listing call sites, not keys.
 */
export function normalise(expression) {
  let e = expression.trim();
  while (e.startsWith("&")) e = e.slice(1).trim();
  const method = /\.(\w+)\(\s*\)$/.exec(e);
  if (method) return `.${method[1]}()`;
  return e;
}

/**
 * The keys one argument expression can produce, or null when this scanner
 * cannot tell.
 *
 * Null is a violation, never a skip. A scanner that quietly ignores a shape it
 * does not understand reports success on exactly the key nobody can change,
 * which is the defect it was written to catch, one level up.
 */
export function resolveKey(expression, { constants, helpers, expressions }) {
  const e = normalise(expression);

  const literal = /^"([^"]*)"$/.exec(e);
  if (literal) return { kind: "literal", keys: [literal[1]] };

  const declared = expressions[e];
  if (declared) {
    if (declared.kind === "renderer-supplied") return { kind: "renderer-supplied", keys: [] };
    if (declared.kind === "derived") return { kind: "derived", keys: declared.keys ?? [] };
    return null;
  }

  const path = /^(?:\w+::)*([A-Z][A-Z0-9_]*)$/.exec(e);
  if (path) {
    const key = constants.get(path[1]);
    return key === undefined ? null : { kind: "constant", keys: [key] };
  }

  const call = /^(\w+)\s*\(/.exec(e);
  if (call && helpers.has(call[1])) return { kind: "helper", keys: helpers.get(call[1]) };

  return null;
}

/* --------------------------------------------------------------- the checks */

/** Command names inside `tauri::generate_handler![ ... ]`. */
export function registeredCommands(libSource) {
  const at = libSource.indexOf("generate_handler![");
  if (at === -1) return new Set();
  const open = libSource.indexOf("[", at);
  const close = matchDelimiter(libSource, open);
  const block = libSource.slice(open + 1, close === -1 ? undefined : close);
  return new Set([...block.matchAll(/(?:\w+::)*(\w+)\s*,/g)].map((m) => m[1]));
}

/**
 * The exported IPC wrapper that invokes a command, or null.
 *
 * Found by walking back from the `invoke("name")` to the nearest preceding
 * export, which is how the file is written throughout: one thin exported
 * function per command, in order.
 */
/**
 * Whether `component` uses `wrapper`, rather than merely importing it.
 *
 * **An import is a mention, and a mention was enough until 2026-08-31.** This
 * started life as `\\b${wrapper}\\b` against the whole file, which a screen that
 * imported the wrapper and never used it satisfied. It was found by mutating
 * KNEG-114's own fix: the call was replaced and the import left in place, and
 * the gate stayed green — the one mutation it was supposed to fail against.
 *
 * **The obvious tightening is wrong, and the tree said so.** Requiring an open
 * parenthesis after the name separates a call from an import list, and it also
 * reported `Integrations > API` as having no way in — because
 * `IntegrationsApi.tsx` calls `guard(api.running ? stopLocalApi : startLocalApi)`,
 * which is a real call by reference with no parenthesis anywhere near the name.
 * A gate that invents an unreachable setting is worse than one that misses a
 * reachable one, so the import statements are removed and the mention is then
 * taken at face value.
 *
 * What it still cannot tell: a name that appears only in a comment, or only in
 * a branch nothing reaches, counts as use. This gate answers "can a user change
 * the value at all", and a mention outside the imports is as close to that as
 * source text gets without a parser.
 *
 * @param {string} component  the screen's source
 * @param {string} wrapper    the exported IPC wrapper's name
 * @returns {boolean}
 */
export function usesWrapper(component, wrapper) {
  const body = component.replace(/^import[\s][\s\S]*?from\s+["'][^"']+["'];?/gm, "");
  return new RegExp(`\\b${wrapper}\\b`).test(body);
}

export function ipcWrapper(command, ipcSource) {
  const at = ipcSource.search(new RegExp(`invoke(?:<[^>]*>)?\\(\\s*"${command}"`));
  if (at === -1) return null;
  const before = ipcSource.slice(0, at);
  const exports = [...before.matchAll(/export\s+(?:async\s+)?function\s+(\w+)/g)];
  return exports.length > 0 ? exports[exports.length - 1][1] : null;
}

/**
 * Every violation in one set of inputs.
 *
 * Pure and injectable for the reason `verify-parity.mjs` says out loud: a gate
 * whose rules are only ever run against a passing tree proves that the tree is
 * clean, not that the rules work, and only the second claim survives a
 * refactor. `scan.test.mjs` mutates the real committed inputs one at a time.
 *
 * @param {object} input
 * @param {object} input.manifest    config/settings-keys.json
 * @param {Record<string, string>} input.backend  file name -> Rust source, for BACKEND_DIR
 * @param {string} input.lib         apps/desktop/src-tauri/src/lib.rs source
 * @param {object} input.surfaces    config/ui-surfaces.json
 * @param {string} input.ipc         apps/desktop/src/lib/ipc.ts source
 * @param {(relativePath: string) => string | null} input.readSource
 *   A committed file's contents, or null when there is no such file. Defaults
 *   to refusing everything, so a caller that forgets it fails loudly rather
 *   than passing a check it never ran.
 * @returns {{rule: string, key: string, detail: string}[]}
 */
export function violations({
  manifest,
  backend,
  lib,
  surfaces,
  ipc,
  readSource = () => null,
}) {
  const found = [];
  const fail = (rule, key, detail) => found.push({ rule, key, detail });

  const declared = manifest.keys ?? {};
  const expressions = manifest.expressions ?? {};
  const declaredSurfaces = surfaces.surfaces ?? {};

  const resolver = {
    constants: constants(backend),
    helpers: keyHelpers(backend),
    expressions,
  };

  // --- the source side ------------------------------------------------------
  const sites = callSites(backend);
  const seen = new Map(); // key -> ["file:line", ...]
  const writtenByCommand = new Map(); // command -> Set(key)
  const genericCommands = new Set();
  const usedExpressions = new Set();

  for (const site of sites) {
    usedExpressions.add(normalise(site.expression));
    const r = resolveKey(site.expression, resolver);
    if (!r) {
      fail(
        "unresolvable-key-expression",
        `${site.file}:${site.line}`,
        `settings::${site.op} is passed \`${site.expression}\`, which this scanner cannot ` +
          "resolve to a key. Declare it in the manifest's \"expressions\" block, or pass a " +
          "constant"
      );
      continue;
    }
    if (r.kind === "renderer-supplied") {
      if (site.isCommand) genericCommands.add(site.fn);
      continue;
    }
    for (const key of r.keys) {
      if (!seen.has(key)) seen.set(key, []);
      seen.get(key).push(`${site.file}:${site.line}`);
      if ((site.op === "set" || site.op === "remove") && site.isCommand) {
        if (!writtenByCommand.has(site.fn)) writtenByCommand.set(site.fn, new Set());
        writtenByCommand.get(site.fn).add(key);
      }
    }
  }

  // A declared `derived` family is produced by its expression even though no
  // single call site names its members.
  for (const [expression, spec] of Object.entries(expressions)) {
    if (!usedExpressions.has(expression)) {
      fail(
        "phantom-expression",
        expression,
        "is declared in the manifest but no settings call passes it. Remove it rather than " +
          "leaving a rule that matches nothing"
      );
      continue;
    }
    if (spec.kind === "derived") {
      for (const key of spec.keys ?? []) if (!seen.has(key)) seen.set(key, [expression]);
      const src = spec.derivedFrom?.file ? readSource(spec.derivedFrom.file) : null;
      if (!src) {
        fail(
          "phantom-derivation",
          expression,
          `claims its keys come from "${spec.derivedFrom?.file}", which does not exist`
        );
      } else {
        for (const [field, what] of [
          ["fn", "the function that builds the key"],
          ["test", "the test that pins the family"],
        ]) {
          const name = spec.derivedFrom[field];
          if (!name) {
            fail("phantom-derivation", expression, `derivedFrom is missing "${field}" -- ${what}`);
          } else if (!new RegExp(`fn\\s+${name}\\b`).test(src)) {
            fail(
              "phantom-derivation",
              expression,
              `derivedFrom names ${field} "${name}", which ${spec.derivedFrom.file} does not define`
            );
          }
        }
      }
    } else if (spec.kind !== "renderer-supplied") {
      fail("unknown-expression-kind", expression, `kind "${spec.kind}" is not a known kind`);
    }
    if (!spec.why) {
      fail("phantom-expression", expression, "must say why it is not an ordinary constant");
    }
  }

  // --- A: nothing in source is undeclared -----------------------------------
  for (const [key, where] of [...seen].sort()) {
    if (!declared[key]) {
      fail(
        "unlisted-key",
        key,
        `is read or written at ${where[0]} but config/settings-keys.json does not declare it`
      );
    }
  }

  const registered = registeredCommands(lib);
  const commandNames = new Set();
  for (const source of Object.values(backend)) {
    for (const fn of functions(source)) if (fn.isCommand) commandNames.add(fn.name);
  }

  // --- B through F: per-key invariants --------------------------------------
  for (const [key, entry] of Object.entries(declared)) {
    if (!seen.has(key)) {
      fail(
        "phantom-key",
        key,
        "is declared but no settings call in the backend reads or writes it. A manifest that " +
          "keeps a key nothing uses stops describing the application"
      );
      continue;
    }
    if (!entry.what) {
      fail("incomplete-key", key, 'must say what it controls in "what"');
    }

    const writers = entry.writtenBy ?? [];
    if (entry.internal) {
      if (writers.length > 0) {
        fail(
          "ambiguous-key",
          key,
          'declares both "internal" and "writtenBy"; a key is either user-settable or it is not'
        );
      }
      // A key called internal that a command does write is a stale declaration,
      // and the day someone adds the screen is the day this should say so.
      for (const [command, keys] of writtenByCommand) {
        if (keys.has(key)) {
          fail(
            "internal-key-has-a-writer",
            key,
            `is declared internal but the command "${command}" writes it`
          );
        }
      }
      continue;
    }

    if (writers.length === 0) {
      fail(
        "unwritable-key",
        key,
        'names no command that writes it and no "internal" reason. This is the defect the gate ' +
          "exists for: the backend honours a setting nothing can change"
      );
      continue;
    }

    for (const command of writers) {
      if (genericCommands.has(command)) {
        fail(
          "generic-writer",
          key,
          `names "${command}", which writes whatever key the renderer passes. A command that ` +
            "writes every key is evidence for none of them"
        );
        continue;
      }
      if (!commandNames.has(command)) {
        fail("phantom-command", key, `names "${command}", which is not a #[tauri::command]`);
        continue;
      }
      if (!registered.has(command)) {
        fail(
          "unregistered-command",
          key,
          `names "${command}", which generate_handler! does not register, so the renderer ` +
            "cannot call it"
        );
      }
      if (!writtenByCommand.get(command)?.has(key)) {
        fail(
          "command-does-not-write-the-key",
          key,
          `names "${command}" as a writer, but no settings::set or settings::remove in that ` +
            "command's body writes this key"
        );
      }
    }

    // --- F: reachability ----------------------------------------------------
    if (typeof entry.reachable !== "boolean") {
      fail(
        "missing-reachability",
        key,
        'must declare "reachable": true or false; absent is not the same as false'
      );
      continue;
    }
    if (!entry.reachable) {
      if (!entry.note) {
        fail("incomplete-reachability", key, '"reachable": false must carry a note saying why');
      }
      continue;
    }
    const surface = declaredSurfaces[entry.ui];
    if (!entry.ui || !surface) {
      fail(
        "unknown-ui-surface",
        key,
        `claims to be reachable through "${entry.ui}", which config/ui-surfaces.json does not ` +
          "declare"
      );
      continue;
    }
    if (!surface.built) {
      fail(
        "unbuilt-surface",
        key,
        `claims to be reachable through "${entry.ui}", which is not built`
      );
      continue;
    }
    const component = readSource(surface.component);
    if (component === null) {
      fail("unbuilt-surface", key, `names "${entry.ui}", whose component does not exist`);
      continue;
    }
    const reached = writers.some((command) => {
      const wrapper = ipcWrapper(command, ipc);
      return wrapper !== null && usesWrapper(component, wrapper);
    });
    if (!reached) {
      fail(
        "unreachable-writer",
        key,
        `claims to be reachable through "${entry.ui}", but ${surface.component} calls none of ` +
          `the IPC wrappers for ${writers.map((c) => `"${c}"`).join(", ")}`
      );
    }
  }

  // --- E: no writer goes undeclared -----------------------------------------
  for (const [command, keys] of writtenByCommand) {
    for (const key of keys) {
      const entry = declared[key];
      if (entry && !entry.internal && !(entry.writtenBy ?? []).includes(command)) {
        fail(
          "undeclared-writer",
          key,
          `is written by the command "${command}", which its writtenBy does not list`
        );
      }
    }
  }

  return found;
}

/* ---------------------------------------------------------------------- CLI */

function readIfPresent(relative) {
  const path = join(REPO_ROOT, relative);
  return existsSync(path) ? readFileSync(path, "utf8") : null;
}

/** Every `.rs` file in the desktop backend, keyed by file name. */
export function backendSources(root = REPO_ROOT) {
  const dir = join(root, BACKEND_DIR);
  const out = {};
  for (const name of readdirSync(dir)) {
    if (name.endsWith(".rs")) out[name] = readFileSync(join(dir, name), "utf8");
  }
  return out;
}

function main() {
  const manifest = JSON.parse(readFileSync(join(REPO_ROOT, "config/settings-keys.json"), "utf8"));
  const backend = backendSources();
  const found = violations({
    manifest,
    backend,
    lib: backend["lib.rs"] ?? "",
    surfaces: JSON.parse(readFileSync(join(REPO_ROOT, "config/ui-surfaces.json"), "utf8")),
    ipc: readFileSync(join(REPO_ROOT, IPC_MODULE), "utf8"),
    readSource: readIfPresent,
  });

  if (process.argv.includes("--json")) {
    console.log(JSON.stringify(found, null, 2));
    process.exit(found.length === 0 ? 0 : 1);
  }

  const keys = Object.keys(manifest.keys ?? {});
  if (found.length === 0) {
    const internal = keys.filter((k) => manifest.keys[k].internal);
    const unreachable = keys.filter((k) => manifest.keys[k].reachable === false);
    console.log(
      `settings keys OK — ${keys.length} declared, ` +
        `${keys.length - internal.length - unreachable.length} changeable from a built screen, ` +
        `${internal.length} written only by the backend`
    );
    // Printed on success rather than buried: these are honestly declared gaps,
    // and this list is what is left to build.
    if (unreachable.length > 0) {
      console.log(`\n${unreachable.length} setting(s) the backend honours and no screen changes:`);
      for (const k of unreachable) console.log(`  ${k.padEnd(28)} ${manifest.keys[k].note}`);
    }
    process.exit(0);
  }

  console.error(
    `settings keys FAILED — ${found.length} violation(s) across ${keys.length} keys\n`
  );
  const grouped = {};
  for (const v of found) (grouped[v.rule] ??= []).push(v);
  for (const [rule, vs] of Object.entries(grouped)) {
    console.error(`  ${rule} (${vs.length})`);
    for (const v of vs) console.error(`    ${v.key}: ${v.detail}`);
    console.error("");
  }
  process.exit(1);
}

if (resolve(process.argv[1] ?? "") === fileURLToPath(import.meta.url)) {
  main();
}
