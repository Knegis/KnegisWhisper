import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import App from "./App";
import "./styles.css";

// Outside a Tauri webview there is no `invoke`, so `Shell` never gets past its
// first read and every screen renders an error instead of itself. In dev only,
// and only when no Tauri host is present, stand in fixture data so the UI can
// be worked on without a Rust build. `import.meta.env.DEV` is statically false
// in a bundled build, so the dynamic import is dropped rather than shipped.
if (import.meta.env.DEV && !("__TAURI_INTERNALS__" in window)) {
  const { installDevFixtures } = await import("./lib/devFixtures");
  installDevFixtures();
}

const container = document.getElementById("root");
if (!container) {
  throw new Error("#root is missing from index.html");
}

createRoot(container).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
