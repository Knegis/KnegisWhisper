#!/usr/bin/env node
/**
 * The dev pre-bundle must not evaluate a module that writes an inherited
 * `Object.prototype` property at load.
 *
 * Why this gate exists (2026-09-03, KNEG-152): the webview runs every script
 * after Tauri's `freezePrototype` has done `Object.freeze(Object.prototype)`
 * (`tauri.conf.json` > `app.security.freezePrototype`). Once frozen, every
 * inherited property of a plain object -- `toString`, `constructor`, `valueOf`,
 * `hasOwnProperty` -- is non-writable, and in strict mode an assignment such as
 * `source.toString = fn` on a plain `{}` throws a TypeError. lodash-es's
 * `lodash.default.js` does exactly that while building the `_` wrapper, and
 * Vite's esbuild pre-bundle kept it beside the three named functions Milkdown
 * imports, so `npm run tauri:dev` opened to a blank white window: the module
 * graph threw before React mounted. The production bundle was unaffected --
 * Rollup drops the module -- so `vite build`, `tsc` and a plain browser all
 * passed while the application did not start.
 *
 * What it measures: it runs Vite's dependency optimizer for `apps/desktop` into
 * a scratch cache directory, exactly as the dev server would, and then reads
 * every emitted chunk for the esbuild module banners of the offenders below.
 * A hit is a module that will be evaluated in the dev webview.
 *
 * What it does not measure: any other library that mutates an inherited
 * prototype property. That class can only be found by running the frontend
 * with the prototype frozen, which is why `apps/desktop/src/main.tsx` freezes
 * it in a plain-browser dev session too -- so `npm run dev` fails the same way
 * the webview does instead of hiding it.
 */

import { existsSync, mkdtempSync, readdirSync, readFileSync, rmSync } from "node:fs";
import { createRequire } from "node:module";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { isEntryPoint } from "../ci/entry.mjs";

/**
 * Modules that write an inherited `Object.prototype` property while loading.
 *
 * Each entry names the module as `<package>/<file>` and the reason it is
 * forbidden, so a reader can check the claim rather than trust it. esbuild
 * marks every module in a pre-bundled chunk with a banner line, `// <path>`,
 * where the path is relative to the output directory -- `// node_modules/...`
 * when the cache sits under the project, `// ../../../<...>/node_modules/...`
 * when it sits in a scratch directory -- so the match is on the banner line's
 * suffix, never on its prefix. The first draft of this gate matched the prefix
 * and passed against a pre-bundle that contained the offender.
 * Add an entry only with a reason.
 */
export const FORBIDDEN_MODULES = [
  {
    module: "lodash-es/lodash.default.js",
    reason:
      "builds the `_` wrapper by copying every method, including `toString`, onto a " +
      "plain object; throws under Object.freeze(Object.prototype)",
  },
];

/**
 * Code that must not appear in any pre-bundled chunk, as a source-text regex.
 *
 * For an offender that cannot be left out of the bundle and is carried as a
 * patch instead (`apps/desktop/patches/`, applied by patch-package on install).
 * The rule fires when the patch has stopped applying -- a new upstream version,
 * a skipped postinstall -- and the unpatched line is back in what the dev
 * webview will run. `patch-package` itself fails `npm ci` loudly in that case;
 * this is the second line of defence for a tree where install was not re-run.
 */
export const FORBIDDEN_PATTERNS = [
  {
    pattern: /this\.toString = \(remark\) => remark\.stringify\(this\.build\(\)\);/,
    module: "@milkdown/transformer/lib/index.js",
    reason:
      "the serializer assigns `this.toString` in its constructor, an inherited property " +
      "Object.freeze(Object.prototype) makes read-only; the patch defines it as an own " +
      "property instead, and the editor never mounts without it (measured 2026-09-03)",
  },
];

/**
 * The esbuild banner line for `module`, wherever the output directory was: a
 * line that is `// `, optionally any path ending in `/`, then the module path.
 */
export function bannerPattern(module) {
  const escaped = module.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  return new RegExp(`^\\/\\/ (?:.*\\/)?${escaped}[ \\t]*$`, "m");
}

/**
 * Scan pre-bundled chunks for forbidden module banners.
 *
 * Pure and injectable so `scan.test.mjs` can exercise the real rule against
 * constructed inputs: a gate that has only ever run green proves nothing.
 *
 * @param {{ name: string, text: string }[]} chunks
 * @param {typeof FORBIDDEN_MODULES} [forbidden]
 * @returns {{ chunk: string, banner: string, module: string, reason: string }[]}
 */
export function scanChunks(chunks, forbidden = FORBIDDEN_MODULES, patterns = FORBIDDEN_PATTERNS) {
  const hits = [];
  for (const chunk of chunks) {
    for (const entry of forbidden) {
      const found = chunk.text.match(bannerPattern(entry.module));
      if (found) {
        hits.push({
          chunk: chunk.name,
          banner: found[0].trim(),
          module: entry.module,
          reason: entry.reason,
        });
      }
    }
    for (const entry of patterns) {
      const found = chunk.text.match(entry.pattern);
      if (found) {
        hits.push({
          chunk: chunk.name,
          banner: found[0].trim(),
          module: entry.module,
          reason: entry.reason,
        });
      }
    }
  }
  return hits;
}

/** Every `.js` chunk in a Vite deps directory, as `{ name, text }`. */
export function readChunks(depsDir) {
  if (!existsSync(depsDir)) return [];
  return readdirSync(depsDir)
    .filter((f) => f.endsWith(".js"))
    .sort()
    .map((name) => ({ name, text: readFileSync(join(depsDir, name), "utf8") }));
}

/**
 * Run Vite's optimizer for `apps/desktop` into a scratch cache and return the
 * directory holding the emitted chunks. The caller removes it.
 */
async function optimizeInto(repoRoot) {
  const desktop = join(repoRoot, "apps", "desktop");
  // `vite` lives under apps/desktop, not under tools/, so resolve it from there
  // rather than relying on a node_modules that does not exist up this tree.
  const require = createRequire(join(desktop, "package.json"));
  const vite = await import(pathToFileURL(require.resolve("vite")).href);

  const cacheDir = mkdtempSync(join(tmpdir(), "kw-dev-bundle-audit-"));
  const config = await vite.resolveConfig(
    {
      root: desktop,
      configFile: join(desktop, "vite.config.ts"),
      cacheDir,
      logLevel: "error",
    },
    "serve",
    "development",
  );
  // `force` so a stale cache can never make this pass; `asCommand` so the
  // optimizer scans the entry points itself instead of waiting for requests.
  await vite.optimizeDeps(config, true, true);
  return { cacheDir, depsDir: join(cacheDir, "deps") };
}

async function main() {
  const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..", "..");
  const { cacheDir, depsDir } = await optimizeInto(repoRoot);
  try {
    const chunks = readChunks(depsDir);
    if (chunks.length === 0) {
      // The control: an optimizer that emitted nothing would pass a scan of
      // nothing, which is the failure mode a gate must never have.
      console.error("dev-bundle audit: the optimizer emitted no chunks, so nothing was measured");
      process.exit(1);
    }
    const hits = scanChunks(chunks);
    if (hits.length > 0) {
      console.error(
        `dev-bundle audit FAILED: ${hits.length} forbidden module(s) in the dev pre-bundle.\n` +
          "These are evaluated in the webview after Object.prototype is frozen, and one\n" +
          "TypeError at module load leaves the window blank.",
      );
      for (const h of hits) {
        console.error(`  ${h.chunk}\n    ${h.banner}\n    ${h.module}: ${h.reason}`);
      }
      process.exit(1);
    }
    // The counts that would make this vacuous are printed on success.
    console.log(
      `dev-bundle audit OK — ${chunks.length} pre-bundled chunk(s) scanned against ` +
        `${FORBIDDEN_MODULES.length} forbidden module(s) and ${FORBIDDEN_PATTERNS.length} ` +
        "forbidden code pattern(s)",
    );
  } finally {
    rmSync(cacheDir, { recursive: true, force: true });
  }
}

if (isEntryPoint(import.meta.url)) {
  main().catch((err) => {
    console.error(`dev-bundle audit: ${err?.stack ?? err}`);
    process.exit(1);
  });
}
