#!/usr/bin/env node
/**
 * Self-tests for the dialog-grant coverage gate.
 *
 * These exist for the reason the other gates' self-tests exist, recorded in
 * docs/ENGINEERING_LOG.md: **a check that matches nothing passes trivially.**
 * "The committed tree is clean" and "the rules work" are different claims, and
 * a green gate only ever demonstrates the first.
 *
 * That is not hypothetical here. **The first draft of this gate passed and was
 * wrong**: it reported two path-taking commands where there are three, missing
 * `import_openwhispr` because a comma inside a field's doc comment split the
 * struct's field list mid-sentence. It was caught by comparing the printed
 * count against a hand audit, not by the gate going red — it never went red.
 * `the_struct_following_case_is_found` is the regression, and it is the single
 * most load-bearing test in this file, because the struct case is the command
 * whose path reaches `kw_storage::import::from_openwhispr` and its `exists()`.
 *
 * Usage: node --test tools/command-audit/scan.test.mjs
 */

import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join, dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import {
  collect,
  violations,
  commands,
  backendSources,
  pathParameters,
  structFields,
  stripComments,
  splitParams,
  named,
  declaredList,
  COMMANDS,
} from "./scan.mjs";

const REPO_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..", "..");
const rules = (found) => found.map((v) => v.rule).sort();

test("the real tree is clean, and not vacuously so", () => {
  const state = collect(REPO_ROOT);

  // The control. Everything below about "no violations" is worth nothing if the
  // parsers came back empty, which is this gate's own failure mode.
  assert.ok(state.registered.size > 50, `only ${state.registered.size} commands parsed`);
  assert.ok(state.all.length > 50, `only ${state.all.length} #[tauri::command]s parsed`);
  assert.deepEqual(state.declared, [
    "media_import_audio",
    "media_import_video",
    "import_openwhispr",
  ]);

  assert.deepEqual(violations(state), []);
});

test("the struct-following case is found", () => {
  // The regression for the false negative described in this file's header.
  // `import_openwhispr` takes an `ImportRequest`, not a path, so a scanner that
  // reads only parameter names calls the surface closed while missing the one
  // command whose path is handed to a function that checks `exists()`.
  const sources = backendSources(REPO_ROOT);
  const all = commands(sources);

  const openwhispr = all.find((c) => c.name === "import_openwhispr");
  assert.ok(openwhispr !== undefined, "import_openwhispr is not being parsed at all");
  assert.deepEqual(pathParameters(openwhispr, sources), [
    { name: "request", type: "ImportRequest", via: "path" },
  ]);

  // And the whole surface is the three, which is what DialogGrants' doc claims
  // in prose. This is the assertion that turns that sentence into a check.
  const taking = [...collect(REPO_ROOT).registered]
    .map((n) => all.find((c) => c.name === n))
    .filter((c) => c !== undefined && pathParameters(c, sources).length > 0)
    .map((c) => c.name)
    .sort();
  assert.deepEqual(taking, ["import_openwhispr", "media_import_audio", "media_import_video"]);
});

test("KNEG-151 reconstructed: a registered path-taking command nobody listed", () => {
  const sources = backendSources(REPO_ROOT);
  const newcomer = {
    name: "media_import_subtitles",
    params: "app: AppHandle<R>, grants: State<'_, DialogGrants>, path: String",
    body: "{ runtime.import_local_file(&app, &path).await }",
    file: "media.rs",
  };
  const state = collect(REPO_ROOT);

  const found = violations({
    ...state,
    registered: new Set([...state.registered, newcomer.name]),
    all: [...state.all, newcomer],
    sources,
  });

  assert.deepEqual(rules(found), ["A. path-taking command is not in PATH_TAKING_COMMANDS"]);
  assert.equal(found[0].subject, "media_import_subtitles");
  assert.match(found[0].detail, /PATH_TAKING_COMMANDS/);
});

test("rule B: the list does not get to claim a command it has not checked", () => {
  const sources = backendSources(REPO_ROOT);
  const state = collect(REPO_ROOT);

  // Declared, registered, a real command -- and its body never authorises.
  const unchecked = violations({
    ...state,
    all: state.all.map((c) =>
      c.name === "media_import_audio" ? { ...c, body: "{ runtime.import(&path).await }" } : c
    ),
    sources,
  });
  assert.deepEqual(rules(unchecked), ["B. declared command does not check the grant"]);
  assert.equal(unchecked[0].subject, "media_import_audio");

  // Declared and not registered.
  const gone = violations({
    ...state,
    registered: new Set([...state.registered].filter((n) => n !== "import_openwhispr")),
    sources,
  });
  assert.deepEqual(rules(gone), ["B. declared command is not registered"]);

  // Declared and not a #[tauri::command] at all.
  const phantom = violations({
    ...state,
    registered: new Set([...state.registered, "media_import_ghost"]),
    declared: [...state.declared, "media_import_ghost"],
    sources,
  });
  assert.deepEqual(rules(phantom), ["B. declared command is not a #[tauri::command]"]);
});

test("a command that takes no renderer path is not flagged", () => {
  // The false-positive control, and a real example: `note_export_to_file`
  // writes a file, and the path comes from a save dialog it opens itself, never
  // from the renderer. A gate that flagged it would push a grant check onto a
  // command that has nothing to check.
  const sources = backendSources(REPO_ROOT);
  const all = commands(sources);
  const exportToFile = all.find((c) => c.name === "note_export_to_file");

  assert.ok(exportToFile !== undefined, "note_export_to_file is not being parsed");
  assert.deepEqual(pathParameters(exportToFile, sources), []);
});

test("stripComments survives the doc comment that broke the first draft", () => {
  const sources = backendSources(REPO_ROOT);
  const fields = structFields("ImportRequest", sources);
  assert.deepEqual(fields, [{ name: "path", type: "String" }]);

  // The exact shape: a comma inside a doc comment, which splitParams would
  // otherwise treat as a field boundary.
  const withComma = "/// the source `x.db`, as returned\n    pub path: String,";
  assert.deepEqual(splitParams(stripComments(withComma)).map(named).filter(Boolean), [
    { name: "path", type: "String" },
  ]);

  // A `//` inside a string literal is not a comment.
  assert.equal(stripComments('let u = "https://x/y"; // gone'), 'let u = "https://x/y"; \n');
  assert.equal(stripComments("a /* b */ c"), "a   c");
});

test("named drops attributes and visibility", () => {
  assert.deepEqual(named('#[serde(default)] pub path: String'), {
    name: "path",
    type: "String",
  });
  assert.deepEqual(named("pub(crate) dest: PathBuf"), { name: "dest", type: "PathBuf" });
  assert.deepEqual(named("state: State<'_, AppState>"), {
    name: "state",
    type: "State<'_, AppState>",
  });
  assert.equal(named("// not a field"), null);
});

test("declaredList reads the Rust const rather than a copy of it", () => {
  const source = readFileSync(join(REPO_ROOT, COMMANDS), "utf8");
  assert.deepEqual(declaredList(source), [
    "media_import_audio",
    "media_import_video",
    "import_openwhispr",
  ]);
  assert.equal(declaredList("no such const here"), null);
});
