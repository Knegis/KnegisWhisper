/**
 * Make a plain-browser dev session fail the way the webview fails.
 *
 * Tauri runs every script after `Object.freeze(Object.prototype)`
 * (`tauri.conf.json` > `app.security.freezePrototype`). A library that assigns
 * an inherited property on a plain object at load — `obj.toString = fn`,
 * `Child.prototype.constructor = Child` — throws there and nowhere else, so an
 * `npm run dev` session in Edge or Chrome, and the dev fixtures with it, could
 * pass a bundle that left the real window blank. That is what happened on
 * 2026-09-03 with lodash-es's full build (see `src/shims/lodash-es.ts`).
 *
 * This module is imported first in `main.tsx`, so it evaluates before the rest
 * of the graph. It does nothing inside Tauri (the freeze has already happened)
 * and nothing in a production bundle (`import.meta.env.DEV` is statically
 * false, so the whole module is dropped).
 */

if (import.meta.env.DEV && !("__TAURI_INTERNALS__" in window)) {
  Object.freeze(Object.prototype);
}
