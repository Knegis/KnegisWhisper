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
 * The button that shows and hides the list panel. Inside the panel while it is
 * open, among the page's own actions, and over the corner of the page while it is
 * not, so it never goes away along with what it would bring back. Either way the
 * press removes the button pressed, so focus moves to the one that replaces it
 * rather than dropping to the page.
 */
export function ListToggle() {
  const { collapsed, toggle } = useContext(PanelCollapsed);
  const label = collapsed ? "Show list" : "Hide list";
  return (
    <button
      type="button"
      className="list-toggle"
      data-collapsed={collapsed}
      aria-label={label}
      title={label}
      aria-expanded={!collapsed}
      aria-controls="list-panel"
      onClick={() => {
        toggle();
        requestAnimationFrame(() => document.querySelector<HTMLElement>(".list-toggle")?.focus());
      }}
    >
      <Icon name="panel" size={16} />
    </button>
  );
}

/**
 * A list panel beside a main area, inside the card.
 *
 * `hidden` rather than unmounting, so a collapsed panel keeps its scroll
 * position and whatever its page put in it. `styles.css` restates
 * `[hidden]` for `.list-panel`, because the panel's own `display: flex`
 * would otherwise outrank the browser's `display: none`.
 *
 * `id="list-panel"` is what the toggle names in `aria-controls`. One page shows
 * at a time, so one `SplitView` is mounted and the id is unique in the document.
 * The panel draws its own toggle (`ListToggle`), wherever suits its actions.
 */
export function SplitView({ label, panel, children }: { label: string; panel: ReactNode; children: ReactNode }) {
  const { collapsed } = useContext(PanelCollapsed);
  return (
    <div className="split">
      {collapsed ? <ListToggle /> : null}
      <aside id="list-panel" className="list-panel" aria-label={label} hidden={collapsed}>
        {panel}
      </aside>
      <div className="split-main">{children}</div>
    </div>
  );
}
