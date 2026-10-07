#!/usr/bin/env node
/**
 * Dialog-grant coverage gate for path-taking IPC commands.
 *
 * Fails the build when a registered `#[tauri::command]` takes a filesystem
 * path from the renderer and is not covered by `DialogGrants`.
 *
 * **Why this exists.** KNEG-80 found three import commands reading any path the
 * renderer handed them; `DialogGrants` closed it, and the doc comment on that
 * type states the closure as a fact — *"the only three commands that take a
 * path, so the surface is closed"*. That sentence was prose. Nothing checked it,
 * and two things downstream depended on it being true.
 *
 * KNEG-151 is what happens when it stops being true quietly. `media.rs`'s IPC
 * boundary test named the three commands in a `generate_handler!` it built
 * itself and again in its own assertion loop, so it proved those three
 * functions refuse an ungranted path and nothing about the application's import
 * surface. A fourth path-taking command was covered by neither list. The Rust
 * side of that fix shares one list — `commands::PATH_TAKING_COMMANDS` — which
 * closes the two directions a test can see: an entry added to the list is
 * driven over real IPC, and an entry dropped from `lib.rs`'s registration fails
 * `every_path_taking_command_is_registered`.
 *
 * **This gate is the direction neither test can see**: a command that takes a
 * renderer path and was never put in the list at all. No Rust test can find it,
 * because Rust has no reflection over `#[tauri::command]` signatures.
 *
 * **What it checks:**
 *
 *   A. Every registered command whose signature takes a renderer-supplied path
 *      appears in `PATH_TAKING_COMMANDS`. See [`pathParameters`] for what
 *      counts, and for the two directions it deliberately errs in.
 *   B. Every command in `PATH_TAKING_COMMANDS` is really registered, is really
 *      a `#[tauri::command]`, and really calls `authorise` in its body. A list
 *      naming a command that does not check the grant would be a second place
 *      claiming the surface is closed while it is open.
 *
 * **What it does not check.** That `authorise` is called *before* the path is
 * used — it reads for the call, not for its position; the four pure policy
 * tests in `media.rs` and the IPC boundary test cover the behaviour. That a
 * path arrives under a name this scanner recognises: a command taking
 * `name: String` and treating it as a filename is missed, and that is the
 * honest limit of reading signatures as text. And it says nothing about
 * `pick_import_file`, which *produces* grants rather than consuming them.
 *
 * Usage:
 *   node tools/command-audit/scan.mjs [--json]
 */

import { readFileSync, existsSync, readdirSync } from "node:fs";
import { join, dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { registeredCommands, matchDelimiter } from "../settings-audit/scan.mjs";
import { isEntryPoint } from "../ci/entry.mjs";

const REPO_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..", "..");

export const BACKEND_DIR = "apps/desktop/src-tauri/src";
export const LIB = `${BACKEND_DIR}/lib.rs`;
export const COMMANDS = `${BACKEND_DIR}/commands.rs`;

/** Parameter types that are injected by Tauri, never sent by the renderer. */
const INJECTED = /\b(State|AppHandle|Window|Webview|WebviewWindow|Runtime)\b/;

/**
 * Parameter names that mean "a place on this filesystem".
 *
 * Deliberately generous. A false positive costs one line in
 * `PATH_TAKING_COMMANDS` or one line of argument in a review; a false negative
 * is the defect this gate exists for.
 */
const PATH_NAME = /(^|_)(path|paths|file|files|dir|dirs|directory|dest|destination|target|location|filename)($|_)/i;

/** Types that can hold a path as a plain string. */
const PATH_TYPE = /^(Option<)?(String|PathBuf|&str|&Path)>?$/;

/** Every `.rs` file in the desktop backend, as `{ file, source }`. */
export function backendSources(root = REPO_ROOT) {
  const dir = join(root, BACKEND_DIR);
  const found = [];
  const walk = (at, prefix) => {
    for (const entry of readdirSync(at, { withFileTypes: true })) {
      const path = join(at, entry.name);
      if (entry.isDirectory()) walk(path, `${prefix}${entry.name}/`);
      else if (entry.name.endsWith(".rs"))
        found.push({ file: `${prefix}${entry.name}`, source: readFileSync(path, "utf8") });
    }
  };
  walk(dir, "");
  return found;
}

/**
 * Every `#[tauri::command]` in the backend, as
 * `{ name, params, body, file }`.
 *
 * The signature is taken from the attribute to the first `{` after the closing
 * parenthesis, so a generic bound or a multi-line return type does not truncate
 * it. `body` runs to the matching brace, which is what rule B reads.
 */
export function commands(sources) {
  const found = [];
  for (const { file, source } of sources) {
    const attribute = /#\[tauri::command[^\]]*\]/g;
    let hit;
    while ((hit = attribute.exec(source)) !== null) {
      const after = source.slice(hit.index + hit[0].length);
      const signature = /^\s*(?:pub(?:\([^)]*\))?\s+)?(?:async\s+)?fn\s+(\w+)\s*(?:<[^>]*>)?\s*\(/.exec(
        after
      );
      if (signature === null) continue;
      const open = hit.index + hit[0].length + signature[0].length - 1;
      const close = matchDelimiter(source, open);
      if (close === -1) continue;
      const params = source.slice(open + 1, close);
      const brace = source.indexOf("{", close);
      const end = brace === -1 ? -1 : matchDelimiter(source, brace);
      found.push({
        name: signature[1],
        params,
        body: brace === -1 || end === -1 ? "" : source.slice(brace, end),
        file,
      });
    }
  }
  return found;
}

/**
 * `text` with Rust comments removed, skipping the contents of string literals.
 *
 * **Not cosmetic — the first draft of this gate was wrong without it.** It
 * reported two path-taking commands where there are three, missing
 * `import_openwhispr`: the doc comment on `ImportRequest::path` reads
 * "…the source `transcriptions.db`, as […] returned it", and that comma split
 * the field list mid-sentence. The resulting "field" ended in `pub path`, so
 * the name test saw a blob rather than a name and the struct-following case —
 * the one this repository's threat model cares most about — silently did not
 * fire. The gate passed, for the wrong reason.
 */
export function stripComments(text) {
  let out = "";
  let quote = false;
  for (let i = 0; i < text.length; i += 1) {
    const c = text[i];
    if (quote) {
      out += c;
      if (c === "\\") {
        out += text[i + 1] ?? "";
        i += 1;
      } else if (c === '"') quote = false;
      continue;
    }
    if (c === '"') {
      quote = true;
      out += c;
      continue;
    }
    if (c === "/" && text[i + 1] === "/") {
      while (i < text.length && text[i] !== "\n") i += 1;
      out += "\n";
      continue;
    }
    if (c === "/" && text[i + 1] === "*") {
      const end = text.indexOf("*/", i + 2);
      i = end === -1 ? text.length : end + 1;
      out += " ";
      continue;
    }
    out += c;
  }
  return out;
}

/** Split a parameter list on top-level commas, so `State<'_, T>` survives. */
export function splitParams(params) {
  const out = [];
  let depth = 0;
  let current = "";
  for (const c of params) {
    if (c === "<" || c === "(" || c === "[") depth += 1;
    if (c === ">" || c === ")" || c === "]") depth -= 1;
    if (c === "," && depth === 0) {
      out.push(current);
      current = "";
      continue;
    }
    current += c;
  }
  out.push(current);
  return out.map((p) => p.trim()).filter((p) => p !== "");
}

/**
 * A `struct Name { … }`'s field names and types, or null.
 *
 * Needed because `import_openwhispr` does not take a path — it takes an
 * `ImportRequest`, which has one. A scanner that looked only at parameter names
 * would call the surface closed while missing the command this repository's
 * threat model cares most about.
 */
export function structFields(name, sources) {
  for (const { source: raw } of sources) {
    const source = stripComments(raw);
    const at = new RegExp(`\\bstruct\\s+${name}\\b[^{;]*\\{`).exec(source);
    if (at === null) continue;
    const open = at.index + at[0].length - 1;
    const close = matchDelimiter(source, open);
    if (close === -1) continue;
    return splitParams(source.slice(open + 1, close)).map(named).filter((f) => f !== null);
  }
  return null;
}

/**
 * One `pub? name: Type` declaration as `{ name, type }`, or null.
 *
 * Attributes are dropped first: a field can carry `#[serde(default)]` or
 * `#[ts(type = "string")]`, and both would otherwise end up inside the name.
 */
export function named(declaration) {
  const clean = declaration.replace(/#\[[^\]]*\]/g, "").replace(/\s+/g, " ").trim();
  const hit = /^(?:pub(?:\([^)]*\))?\s+)?(\w+)\s*:\s*(.+)$/.exec(clean);
  return hit === null ? null : { name: hit[1], type: hit[2].trim() };
}

/**
 * The renderer-supplied parameters of `command` that name a filesystem path.
 *
 * Injected parameters are dropped first: `State<'_, DialogGrants>` is not
 * something a renderer sends, and neither is `AppHandle`. What is left is the
 * IPC surface, and a path there is either a string named like a path or a
 * struct with such a field.
 */
export function pathParameters(command, sources) {
  const found = [];
  for (const param of splitParams(stripComments(command.params))) {
    const declared = named(param);
    if (declared === null) continue;
    const { name, type } = declared;
    if (INJECTED.test(type)) continue;

    if (PATH_TYPE.test(type) && PATH_NAME.test(name)) {
      found.push({ name, type, via: null });
      continue;
    }
    const fields = /^[A-Z]\w*$/.test(type) ? structFields(type, sources) : null;
    if (fields === null) continue;
    for (const field of fields) {
      if (PATH_TYPE.test(field.type) && PATH_NAME.test(field.name)) {
        found.push({ name, type, via: field.name });
      }
    }
  }
  return found;
}

/**
 * The names in `commands::PATH_TAKING_COMMANDS`.
 *
 * Read from the Rust source rather than duplicated here, for the reason this
 * whole gate exists: a second copy of a list is a second thing to keep in step.
 */
export function declaredList(commandsSource) {
  const at = commandsSource.indexOf("PATH_TAKING_COMMANDS");
  if (at === -1) return null;
  const open = commandsSource.indexOf("[", commandsSource.indexOf("=", at));
  if (open === -1) return null;
  const close = matchDelimiter(commandsSource, open);
  const block = commandsSource.slice(open + 1, close === -1 ? undefined : close);
  return [...block.matchAll(/\(\s*"(\w+)"\s*,/g)].map((m) => m[1]);
}

export function violations({ registered, declared, all, sources }) {
  const found = [];
  const byName = new Map(all.map((c) => [c.name, c]));

  // Rule A. The direction no Rust test can see.
  for (const name of registered) {
    const command = byName.get(name);
    if (command === undefined) continue;
    const paths = pathParameters(command, sources);
    if (paths.length === 0 || declared.includes(name)) continue;
    found.push({
      rule: "A. path-taking command is not in PATH_TAKING_COMMANDS",
      subject: name,
      detail:
        `${command.file} registers it and it takes ` +
        paths.map((p) => (p.via === null ? `\`${p.name}\`` : `\`${p.name}.${p.via}\``)).join(", ") +
        ` from the renderer. Add it to commands::PATH_TAKING_COMMANDS so the IPC ` +
        `boundary test drives it, and call DialogGrants::authorise on the path.`,
    });
  }

  // Rule B. The list does not get to claim a command it has not checked.
  for (const name of declared) {
    if (!registered.has(name)) {
      found.push({
        rule: "B. declared command is not registered",
        subject: name,
        detail: `PATH_TAKING_COMMANDS names it and lib.rs's generate_handler! does not.`,
      });
      continue;
    }
    const command = byName.get(name);
    if (command === undefined) {
      found.push({
        rule: "B. declared command is not a #[tauri::command]",
        subject: name,
        detail: `PATH_TAKING_COMMANDS names it and no #[tauri::command] declares it.`,
      });
      continue;
    }
    if (!/\bauthorise\s*\(/.test(command.body)) {
      found.push({
        rule: "B. declared command does not check the grant",
        subject: name,
        detail:
          `${command.file} declares it and its body never calls authorise(). ` +
          `The list claims this command's path is grant-checked and it is not.`,
      });
    }
  }

  return found;
}

export function collect(root = REPO_ROOT) {
  const sources = backendSources(root);
  const libPath = join(root, LIB);
  const commandsPath = join(root, COMMANDS);
  const registered = existsSync(libPath)
    ? registeredCommands(readFileSync(libPath, "utf8"))
    : new Set();
  const declared = existsSync(commandsPath)
    ? (declaredList(readFileSync(commandsPath, "utf8")) ?? [])
    : [];
  return { registered, declared, all: commands(sources), sources };
}

function main() {
  const state = collect();
  const found = violations(state);
  const taking = [...state.registered]
    .map((n) => state.all.find((c) => c.name === n))
    .filter((c) => c !== undefined)
    .filter((c) => pathParameters(c, state.sources).length > 0);

  if (process.argv.includes("--json")) {
    console.log(
      JSON.stringify(
        {
          registered: [...state.registered],
          declared: state.declared,
          pathTaking: taking.map((c) => c.name),
          violations: found,
        },
        null,
        2
      )
    );
    process.exit(found.length === 0 ? 0 : 1);
  }

  if (found.length > 0) {
    console.error(`command grants FAILED — ${found.length} violation(s)\n`);
    const grouped = {};
    for (const v of found) (grouped[v.rule] ??= []).push(v);
    for (const [rule, vs] of Object.entries(grouped)) {
      console.error(`  ${rule} (${vs.length})`);
      for (const v of vs) console.error(`    ${v.subject}: ${v.detail}`);
      console.error("");
    }
    process.exit(1);
  }

  // The counts are the point. "No violations" over zero parsed commands would
  // be this gate committing the failure it exists to catch, so the numbers that
  // would make it vacuous are printed every run.
  console.log(
    `command grants OK — ${state.registered.size} registered, ` +
      `${taking.length} take a renderer path, ${state.declared.length} declared`
  );
  for (const command of taking) {
    const paths = pathParameters(command, state.sources)
      .map((p) => (p.via === null ? p.name : `${p.name}.${p.via}`))
      .join(", ");
    console.log(`  ${command.name.padEnd(24)} ${paths.padEnd(14)} ${command.file}`);
  }
  process.exit(0);
}

if (isEntryPoint(import.meta.url)) {
  main();
}
