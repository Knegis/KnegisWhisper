import { TitleBarSearch } from "./TitleBarSearch";
import { useEffect, useState } from "react";
import { getCurrentWindow, type Window as AppWindow } from "@tauri-apps/api/window";

export function TitleBar({ sidebarExpanded, onToggleSidebar, onOpenNote }: { sidebarExpanded: boolean; onToggleSidebar: () => void; onOpenNote: (id: number) => void }) {
  const [maximized, setMaximized] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const native = "__TAURI_INTERNALS__" in window;
  useEffect(() => {
    if (!native) return;
    const appWindow = getCurrentWindow();
    let disposed = false;
    let revision = 0;
    const stops: (() => void)[] = [];
    const refresh = async () => {
      const current = ++revision;
      try {
        const value = await appWindow.isMaximized();
        if (!disposed && current === revision) setMaximized(value);
      } catch (e) { if (!disposed) setError(String(e)); }
    };
    void Promise.all([
      appWindow.onResized(() => { void refresh(); }),
      appWindow.onFocusChanged(() => { void refresh(); }),
    ].map(async subscription => {
      const stop = await subscription;
      if (disposed) stop(); else stops.push(stop);
    })).then(refresh).catch(e => { if (!disposed) setError(String(e)); });
    return () => { disposed = true; stops.forEach(stop => stop()); };
  }, [native]);
  const act = async (action: (appWindow: AppWindow) => Promise<void>) => {
    setError(null);
    try { await action(getCurrentWindow()); }
    catch (e) { setError(String(e)); }
  };
  return <>
    <header className="app-titlebar" aria-label="Application title bar">
      <button
        type="button"
        className="sidebar-toggle"
        aria-label={sidebarExpanded ? "Retract sidebar" : "Expand sidebar"}
        title={sidebarExpanded ? "Retract sidebar" : "Expand sidebar"}
        aria-expanded={sidebarExpanded}
        aria-controls="sidebar-navigation"
        onClick={onToggleSidebar}
      >
        <svg
          width="20"
          height="20"
          viewBox="0 0 24 24"
          fill="none"
          xmlns="http://www.w3.org/2000/svg"
          stroke="currentColor"
          strokeWidth="1.5"
          aria-hidden="true"
          focusable="false"
        >
          <path d="M3 3H21V21H3V3Z" strokeLinecap="square" strokeLinejoin="miter" />
          <path d="M9.5 21V3" strokeLinecap="round" strokeLinejoin="round" />
          <path
            d={sidebarExpanded ? "M7.25 10L5.5 12L7.25 14" : "M5.5 10L7.25 12L5.5 14"}
            strokeLinecap="square"
            strokeLinejoin="miter"
          />
        </svg>
      </button>

      <TitleBarSearch onOpenNote={onOpenNote} />
      <div className="app-titlebar-drag" data-tauri-drag-region>
        <span>KnegisWhisper</span>
      </div>
      <div className="app-titlebar-buttons">
        <button type="button" disabled={!native} aria-label="Minimize window" title="Minimize" onClick={() => void act(w => w.minimize())}><span className="window-icon window-icon-minimize" aria-hidden="true" /></button>
        <button type="button" disabled={!native} aria-label={maximized ? "Restore window" : "Maximize window"} title={maximized ? "Restore" : "Maximize"} onClick={() => void act(w => w.toggleMaximize())}><span className={`window-icon ${maximized ? "window-icon-restore" : "window-icon-maximize"}`} aria-hidden="true" /></button>
        <button type="button" disabled={!native} className="window-close" aria-label="Close window" title="Close" onClick={() => void act(w => w.close())}><span className="window-icon window-icon-close" aria-hidden="true" /></button>
      </div>
    </header>
    {error && <div className="window-control-error" role="alert">{error}<button onClick={() => setError(null)} aria-label="Dismiss window error">Dismiss</button></div>}
  </>;
}
