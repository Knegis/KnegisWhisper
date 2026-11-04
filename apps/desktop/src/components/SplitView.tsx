import { createContext, useContext, type ReactNode } from "react";

import { Icon } from "./Icon";

/**
 * Whether the list panel is collapsed, and how to change it.
 *
 * A context rather than props because the state belongs to the window while the
 * page that draws a panel renders its own `SplitView`: `App.tsx` holds it, so it
 * can collapse the panel when the window narrows past 900px, and threading it
 * through a view's props to reach one attribute is what this saves.
 */
export const PanelCollapsed = createContext<{ collapsed: boolean; toggle: () => void }>({ collapsed: false, toggle: () => {} });

/**
 * A list panel beside a main area, inside the card.
 *
 * `hidden` rather than unmounting, so a collapsed panel keeps its scroll
 * position and whatever its page put in it. `styles.css` restates
 * `[hidden]` for `.list-panel`, because the panel's own `display: flex`
 * would otherwise outrank the browser's `display: none`.
 *
 * The toggle names the panel in `aria-controls`, which is why the id is here:
 * one page shows at a time, so one `SplitView` is mounted and the id is unique.
 * It stays in the document either way and is laid out over the page while the
 * panel is hidden, so it never disappears along with what it would bring back.
 *
 * `heading` is for a page whose `h1` is its panel's title. A hidden panel is
 * out of the accessibility tree, and the page would have no `h1` at all; so
 * while the panel is collapsed the main area carries one, visually hidden,
 * with the same words. Only then, so the page has exactly one either way.
 */
export function SplitView({ label, heading, panel, children }: { label: string; heading?: string; panel: ReactNode; children: ReactNode }) {
  const { collapsed, toggle } = useContext(PanelCollapsed);
  const toggleLabel = collapsed ? "Show list" : "Hide list";
  return (
    <div className="split">
      <button
        type="button"
        className="list-toggle"
        data-collapsed={collapsed}
        aria-label={toggleLabel}
        title={toggleLabel}
        aria-expanded={!collapsed}
        aria-controls="list-panel"
        onClick={toggle}
      >
        <Icon name="panel" size={16} />
      </button>
      <aside id="list-panel" className="list-panel" aria-label={label} hidden={collapsed}>
        {panel}
      </aside>
      <div className="split-main">
        {collapsed && heading !== undefined ? <h1 className="sr-only">{heading}</h1> : null}
        {children}
      </div>
    </div>
  );
}
