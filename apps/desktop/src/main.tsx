// First on purpose: it must evaluate before anything that follows it. See
// the module for why a browser dev session freezes Object.prototype.
import "./lib/devPrototypeFreeze";

import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import App from "./App";
import { installFileDropGuard } from "./lib/fileDrops";
import { currentSurface } from "./lib/surface";
import "./styles.css";

// This has to precede the first render: an attribute-less frame would let the
// shell's `var(--bg)` paint a transparent panel window before its panel does.
document.documentElement.dataset.window = currentSurface();

// Outside a Tauri webview there is no `invoke`, so `Shell` never gets past its
// first read and every screen renders an error instead of itself. In dev only,
// and only when no Tauri host is present, stand in fixture data so the UI can
// be worked on without a Rust build. `import.meta.env.DEV` is statically false
// in a bundled build, so the dynamic import is dropped rather than shipped.
if (import.meta.env.DEV && !("__TAURI_INTERNALS__" in window)) {
  const { installDevFixtures } = await import("./lib/devFixtures");
  installDevFixtures();
  const { installSharingFixtures } = await import("./lib/sharingFixtures");
  installSharingFixtures();
}

installFileDropGuard(window);

const container = document.getElementById("root");
if (!container) {
  throw new Error("#root is missing from index.html");
}

createRoot(container).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
