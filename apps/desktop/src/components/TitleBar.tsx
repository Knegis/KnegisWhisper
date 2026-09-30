import { TitleBarSearch } from "./TitleBarSearch";
import { Icon } from "./Icon";
import { useEffect, useState } from "react";
import { getCurrentWindow, type Window as AppWindow } from "@tauri-apps/api/window";

/**
 * The main window's own title bar: the list-panel toggle, back and forward, the
 * search field centred between two drag regions, and the window buttons.
 *
 * The toggle shows and hides the list panel beside the page (`SplitView`'s
 * `#list-panel`), never the rail -- the rail is always drawn. It is the only
 * such toggle: Notes' library is that list panel now, and its own toggle and
 * the title-bar slot it was portalled into have gone.
 *
 * On a page with no panel (`hasListPanel` false -- Notes > Upload, the card
 * while it loads or after it failed to, and Dictation, Meetings or Notes
 * while the error boundary stands in for the page) the toggle is not drawn
 * at all, rather than drawn pointing its `aria-controls` at an element that
 * is not in the document. The collapsed
 * state it flips belongs to the whole window and outlives the page, so the
 * next page with a panel still opens the way it was left.
 */
export function TitleBar({ panelCollapsed, hasListPanel, onTogglePanel, onOpenNote, canGoBack, canGoForward, onBack, onForward }: { panelCollapsed: boolean; hasListPanel: boolean; onTogglePanel: () => void; onOpenNote: (id: number) => void; canGoBack: boolean; canGoForward: boolean; onBack: () => void; onForward: () => void }) {
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
  const panelLabel = panelCollapsed ? "Show list" : "Hide list";
  return <>
    <header className="app-titlebar" aria-label="Application title bar">
      {hasListPanel ? (
        <button
          type="button"
          className="titlebar-button"
          aria-label={panelLabel}
          title={panelLabel}
          aria-expanded={!panelCollapsed}
          aria-controls="list-panel"
          onClick={onTogglePanel}
        >
          <Icon name="panel" />
        </button>
      ) : null}
      <div className="titlebar-history" role="group" aria-label="Navigation history">
        <button type="button" className="titlebar-button" aria-label="Go back" title="Go back" disabled={!canGoBack} onClick={onBack}>
          <Icon name="back" />
        </button>
        <button type="button" className="titlebar-button" aria-label="Go forward" title="Go forward" disabled={!canGoForward} onClick={onForward}>
          <Icon name="forward" />
        </button>
      </div>
      <div className="app-titlebar-drag" data-tauri-drag-region />
      <TitleBarSearch onOpenNote={onOpenNote} />
      <div className="app-titlebar-drag" data-tauri-drag-region />
      <div className="app-titlebar-buttons">
        <button type="button" disabled={!native} aria-label="Minimize window" title="Minimize" onClick={() => void act(w => w.minimize())}><Icon name="minimise" size={16} /></button>
        <button type="button" disabled={!native} aria-label={maximized ? "Restore window" : "Maximize window"} title={maximized ? "Restore" : "Maximize"} onClick={() => void act(w => w.toggleMaximize())}><Icon name={maximized ? "restore" : "maximise"} size={16} /></button>
        <button type="button" disabled={!native} className="window-close" aria-label="Close window" title="Close" onClick={() => void act(w => w.close())}><Icon name="close" size={16} /></button>
      </div>
    </header>
    {error && <div className="window-control-error" role="alert">{error}<button onClick={() => setError(null)} aria-label="Dismiss window error">Dismiss</button></div>}
  </>;
}
