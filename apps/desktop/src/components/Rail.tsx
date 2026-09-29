import { Fragment, type RefObject } from "react";

import { NAV, RAIL, railOf, type RailId, type ViewId } from "../lib/navigation";
import { NavIcon } from "./NavIcon";

/** A rail entry's label: `NAV`'s own, so the rail and the tree cannot disagree. */
function labelOf(id: RailId): string {
  return NAV.find((entry) => entry.id === id)?.label ?? id;
}

/**
 * The rail: the three jobs, a divider, and Settings -- an icon over a label for
 * each, always visible. It never collapses; the title bar's toggle hides the
 * list panel beside the page, not this.
 *
 * Exactly one entry reads as current, and it is `railOf(view)`: a settings or
 * developer screen marks Settings, and an inner view marks its parent.
 *
 * `Settings` is not a screen of its own, so pressing it opens `lastSettings`
 * -- the settings screen visited last, or `SETTINGS_HOME` before any has been.
 * It is read when the button is pressed, not when the rail renders: `App.tsx`
 * updates it after each navigation, so a value captured at render would be
 * one screen behind on the render that arrived at a settings screen.
 */
export function Rail({
  view,
  lastSettings,
  onSelect,
}: {
  view: ViewId;
  lastSettings: RefObject<ViewId>;
  onSelect: (id: ViewId) => void;
}) {
  const current = railOf(view);
  return (
    <nav className="rail" aria-label="Main navigation">
      {RAIL.map((id) => (
        <Fragment key={id}>
          {id === "settings" ? <span className="rail-divider" aria-hidden="true" /> : null}
          <button
            type="button"
            className="rail-item"
            aria-current={current === id ? "page" : undefined}
            onClick={() => onSelect(id === "settings" ? lastSettings.current : id)}
          >
            <NavIcon id={id} size={22} />
            <span>{labelOf(id)}</span>
          </button>
        </Fragment>
      ))}
    </nav>
  );
}
