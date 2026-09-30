import { createContext, useContext, type ReactNode } from "react";

/**
 * Whether the list panel is collapsed, on whichever page is showing.
 *
 * A context rather than a prop because the page that draws a panel is not the
 * component that owns the toggle: the title bar's "Hide list" button lives in
 * `App.tsx`, above every view, while Dictation, Meetings and Notes each render
 * their own `SplitView` from inside their own file. Threading a boolean
 * through every view's props to reach one attribute is what this saves. The
 * value is one piece of state for the whole window, so collapsing the panel on
 * one page leaves it collapsed on the next.
 */
export const PanelCollapsed = createContext(false);

/**
 * A list panel beside a main area, inside the card.
 *
 * `id="list-panel"` is what the title bar's toggle names in `aria-controls`.
 * Only one `SplitView` is ever mounted -- one page shows at a time -- so the
 * id is unique in the document.
 *
 * `hidden` rather than unmounting, so a collapsed panel keeps its scroll
 * position and whatever its page put in it. `styles.css` restates
 * `[hidden]` for `.list-panel`, because the panel's own `display: flex`
 * would otherwise outrank the browser's `display: none`.
 *
 * `heading` is for a task page -- Dictation, Meetings, Notes -- whose `h1` is
 * its panel's title. A hidden panel is out of the accessibility tree, and
 * the page would have no `h1` at all; so while the panel is collapsed the
 * main area carries one, visually hidden, with the same words. Only then, so
 * the page has exactly one either way. A settings screen passes none: its
 * `h1` is its `PageHeader`, and the Settings panel's title is an `h2`.
 */
export function SplitView({ label, heading, panel, children }: { label: string; heading?: string; panel: ReactNode; children: ReactNode }) {
  const collapsed = useContext(PanelCollapsed);
  return (
    <div className="split">
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
