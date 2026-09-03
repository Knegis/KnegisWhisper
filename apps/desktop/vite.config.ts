import process from "node:process";
import { fileURLToPath } from "node:url";
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// Tauri drives this dev server, so the port is fixed and failing loudly is
// better than silently moving to another port the Rust side is not watching.
const DEV_PORT = 5183;

// Tauri sets this for a debug build.
const isDebugBuild = Boolean(process.env["TAURI_ENV_DEBUG"]);

export default defineConfig({
  plugins: [react()],

  // Vue's esm-bundler build (Milkdown's component runtime) reads these at
  // build time and warns in dev when they are absent. Options API stays on
  // because Milkdown's components are not audited for it; the other two are
  // production-only diagnostics this application does not use.
  define: {
    __VUE_OPTIONS_API__: "true",
    __VUE_PROD_DEVTOOLS__: "false",
    __VUE_PROD_HYDRATION_MISMATCH_DETAILS__: "false",
  },

  resolve: {
    alias: [
      // The bare specifier only: `lodash-es/debounce.js` and the other
      // per-method paths still resolve to the package. The shim says why -- the
      // package index evaluates lodash.default.js, which throws under Tauri's
      // frozen Object.prototype and left the dev window blank on 2026-09-03.
      // Applied in build too, so dev and production load the same modules.
      {
        find: /^lodash-es$/,
        replacement: fileURLToPath(new URL("./src/shims/lodash-es.ts", import.meta.url)),
      },
    ],
  },

  // `src-tauri` is a Cargo project; watching it would restart Vite on every
  // Rust rebuild.
  clearScreen: false,
  server: {
    port: DEV_PORT,
    strictPort: true,
    host: "127.0.0.1",
    watch: { ignored: ["**/src-tauri/**"] },
  },

  // Match the webview's baseline rather than the developer's browser.
  build: {
    target: "es2022",
    minify: isDebugBuild ? false : "esbuild",
    sourcemap: isDebugBuild,
  },
});
