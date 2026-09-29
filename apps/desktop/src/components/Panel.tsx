import type { ReactNode } from "react";

import type { IconName } from "../lib/icons";
import { Icon } from "./Icon";

/**
 * The pieces a list panel is built from: a header, labelled sections, and the
 * rows inside them. `SplitView` places the panel; these fill it.
 *
 * Shared rather than written per page, because every page that has a panel
 * draws the same four things -- the Settings panel today, Dictation's history,
 * Meetings' recent list and the Notes library once they move into one -- and
 * a row that looked one way in one panel and another way in the next would
 * read as two kinds of thing where there is one.
 *
 * # Two kinds of row
 *
 * - **`PanelItem` is a place.** It selects something the main area then shows
 *   -- a settings screen, a note -- and exactly one of them is `current`, so it
 *   carries `aria-current="page"` and the `--selected` fill. With an `icon` it
 *   is a single 36px line (the Settings rows); without one it is a title over
 *   an optional `meta` line (a note's title over its time).
 * - **`PanelAction` is a command.** It does something -- starts a note, opens
 *   the importer -- and is never current. It has an icon, a label, and an
 *   optional trailing `hint` in the machine voice for a key or a count.
 */

/** The panel's title, and any buttons that belong beside it. */
export function PanelHeader({ title, children }: { title: string; children?: ReactNode }) {
  return (
    <div className="panel-head">
      {/* `h1`, as on the canvas: on a task page this is the page's title. */}
      <h1 className="panel-title">{title}</h1>
      {children}
    </div>
  );
}

/**
 * A run of rows under an optional heading. `null` draws no heading, for the
 * section directly under a `PanelHeader` whose title already names it.
 */
export function PanelSection({ label, children }: { label: string | null; children: ReactNode }) {
  return (
    <div className="panel-section">
      {label === null ? null : <h2 className="panel-label">{label}</h2>}
      <div className="panel-list">{children}</div>
    </div>
  );
}

/**
 * A row that selects something.
 *
 * `srSuffix` is text a screen reader hears after `title` and nobody sees --
 * `lib/navigation.ts`' `srSuffixOf`, which is how the Settings panel's
 * "Meetings" reads as "Meetings settings" beside the rail's own "Meetings".
 * It sits inside the title's span so the accessible name keeps the visible
 * label at its start (WCAG 2.5.3, label in name).
 */
export function PanelItem({
  title,
  srSuffix = null,
  meta,
  current = false,
  muted = false,
  icon,
  onClick,
}: {
  title: string;
  srSuffix?: string | null;
  meta?: ReactNode;
  current?: boolean;
  muted?: boolean;
  icon?: IconName;
  onClick: () => void;
}) {
  const className = [
    "panel-item",
    icon === undefined ? null : "panel-item-icon",
    muted ? "panel-item-muted" : null,
  ].filter((name) => name !== null).join(" ");
  const text = (
    <>
      <span className="panel-item-title">
        {title}
        {srSuffix === null ? null : <span className="sr-only">{srSuffix}</span>}
      </span>
      {meta === undefined ? null : <span className="panel-item-meta">{meta}</span>}
    </>
  );
  return (
    <button type="button" className={className} aria-current={current ? "page" : undefined} onClick={onClick}>
      {icon === undefined ? text : <><Icon name={icon} /><span className="panel-item-body">{text}</span></>}
    </button>
  );
}

/** A row that does something rather than selecting something. */
export function PanelAction({
  icon,
  label,
  hint,
  onClick,
  disabled = false,
}: {
  icon: IconName;
  label: string;
  hint?: string;
  onClick: () => void;
  disabled?: boolean;
}) {
  return (
    <button type="button" className="panel-action" onClick={onClick} disabled={disabled}>
      <Icon name={icon} />
      <span className="panel-action-label">{label}</span>
      {hint === undefined ? null : <span className="panel-action-hint">{hint}</span>}
    </button>
  );
}
