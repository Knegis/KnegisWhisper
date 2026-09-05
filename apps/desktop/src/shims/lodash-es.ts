/**
 * What `lodash-es` means inside this application: three functions, imported
 * from their own modules.
 *
 * `vite.config.ts` aliases the bare specifier `lodash-es` here. Milkdown imports
 * `debounce`, `throttle` and `defaultsDeep` by name from the package index
 * (`lodash-es/lodash.js`), and that index re-exports `lodash.default.js`, which
 * builds the `_` wrapper at load by copying every method — `toString` included —
 * onto a plain object. Under Tauri's `freezePrototype`
 * (`Object.freeze(Object.prototype)`, applied before any script runs) that
 * assignment is a TypeError, the module graph dies before React mounts, and the
 * window is blank. Rollup drops the module from the production bundle; Vite's
 * esbuild pre-bundle for `npm run tauri:dev` did not, which is how the
 * application shipped every gate green and opened to nothing on 2026-09-03.
 *
 * Deliberately narrow: only what is imported today. A future Milkdown that
 * imports a fourth function fails loudly at build and in dev with "does not
 * provide an export named ...", which is the right failure — add the export
 * here from its per-method module, never from the index.
 * `tools/dev-bundle-audit/scan.mjs` is the gate that keeps the full build out.
 */

export { default as debounce } from "lodash-es/debounce.js";
export { default as defaultsDeep } from "lodash-es/defaultsDeep.js";
export { default as throttle } from "lodash-es/throttle.js";
