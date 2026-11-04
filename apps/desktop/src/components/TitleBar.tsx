import { TitleBarSearch } from "./TitleBarSearch";
import { Icon } from "./Icon";
import { DotMark, JobPills, SettingsPill } from "./NavPills";
import type { ViewId } from "../lib/navigation";
import { useEffect, useState } from "react";
import { getCurrentWindow, type Window as AppWindow } from "@tauri-apps/api/window";

/**
 * The main window's own title bar, which is also its navigation: on the left
 * back, forward and the three job pills; the KnegisWhisper dot mark centred on
 * the window; on the right the search field, the Settings pill and the window
 * buttons.
 *
 * What is not a control is the drag region. The bar and its two ends carry the
 * attribute as well as the mark, because Tauri starts a drag only when the
 * element pressed has it: the bar's own padding and the empty middle of each
 * end are the places a hand goes to move a window.
 *
 * The list panel's toggle is not here. Only Notes has a list panel now, so the
 * toggle sits with the panel (`SplitView`), and back, forward and the pills
 * stay in one place on every page.
 */
export function TitleBar({ view, onSelect, sheetOpen, onOpenSheet, onOpenNote, canGoBack, canGoForward, onBack, onForward, meetingRecording }: { view: ViewId; onSelect: (id: ViewId) => void; sheetOpen: boolean; onOpenSheet: () => void; onOpenNote: (id: number, transcriptRow?: number) => void; canGoBack: boolean; canGoForward: boolean; onBack: () => void; onForward: () => void; meetingRecording: boolean }) {
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
    <header className="app-titlebar" aria-label="Application title bar" data-tauri-drag-region>
      <div className="app-titlebar-start" data-tauri-drag-region>
        <div className="titlebar-history" role="group" aria-label="Navigation history">
          <button type="button" className="titlebar-button" aria-label="Go back" title="Go back" disabled={!canGoBack} onClick={onBack}>
            <Icon name="back" size={16} />
          </button>
          <button type="button" className="titlebar-button" aria-label="Go forward" title="Go forward" disabled={!canGoForward} onClick={onForward}>
            <Icon name="forward" size={16} />
          </button>
        </div>
        <JobPills view={view} onSelect={onSelect} meetingRecording={meetingRecording} />
      </div>
      <span className="app-titlebar-mark" data-tauri-drag-region><DotMark size={34} /></span>
      <div className="app-titlebar-end" data-tauri-drag-region>
        <TitleBarSearch onOpenNote={onOpenNote} />
        <SettingsPill view={view} open={sheetOpen} onOpen={onOpenSheet} />
        <div className="app-titlebar-buttons">
          <button type="button" disabled={!native} aria-label="Minimize window" title="Minimize" onClick={() => void act(w => w.minimize())}><Icon name="minimise" size={18} /></button>
          <button type="button" disabled={!native} aria-label={maximized ? "Restore window" : "Maximize window"} title={maximized ? "Restore" : "Maximize"} onClick={() => void act(w => w.toggleMaximize())}><Icon name={maximized ? "restore" : "maximise"} size={18} /></button>
          <button type="button" disabled={!native} className="window-close" aria-label="Close window" title="Close" onClick={() => void act(w => w.close())}><Icon name="close" size={18} /></button>
        </div>
      </div>
    </header>
    {error && <div className="window-control-error" role="alert">{error}<button onClick={() => setError(null)} aria-label="Dismiss window error">Dismiss</button></div>}
  </>;
}
