import process from "node:process";
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// Tauri drives this dev server, so the port is fixed and failing loudly is
// better than silently moving to another port the Rust side is not watching.
const DEV_PORT = 5183;

// Tauri sets this for a debug build.
const isDebugBuild = Boolean(process.env["TAURI_ENV_DEBUG"]);

export default defineConfig({
  plugins: [react()],

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
