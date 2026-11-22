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
 * The button that shows and hides the list panel. There is one at a time, so the
 * press removes the button pressed and focus moves to the one that replaces it
 * rather than dropping to the page:
 *
 * - `place="panel"`: inside the panel while it is open, among the page's own
 *   actions. Drawn only then; the hidden panel stays mounted and would otherwise
 *   keep a second one.
 * - With the panel closed, on the page, so it never goes away along with what it
 *   would bring back. An open note draws it on its date line (`NoteWorkspace`),
 *   where it scrolls away with the note (the owner's ask, 2026-11-22). Where the
 *   page has no such line -- Trash, no note open -- `SplitView` draws it over
 *   the page's corner, which is where every page had it before.
 */
export function ListToggle({ place = "page" }: { place?: "panel" | "page" }) {
  const { collapsed, toggle } = useContext(PanelCollapsed);
  const label = collapsed ? "Show list" : "Hide list";
  if (place === "panel" && collapsed) return null;
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
 * `toggleInMain` says the main area draws the closed panel's toggle itself.
 */
export function SplitView({ label, panel, toggleInMain = false, children }: { label: string; panel: ReactNode; toggleInMain?: boolean; children: ReactNode }) {
  const { collapsed } = useContext(PanelCollapsed);
  return (
    <div className="split" data-list-collapsed={collapsed}>
      {collapsed && !toggleInMain ? <ListToggle /> : null}
      <aside id="list-panel" className="list-panel" aria-label={label} hidden={collapsed}>
        {panel}
      </aside>
      <div className="split-main">{children}</div>
    </div>
  );
}
