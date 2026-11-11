import { useEffect, useRef, useState, type KeyboardEvent, type MouseEvent } from "react";

import { NAV, SETTINGS_PANEL, childrenOf, isGroup, type ViewId } from "../lib/navigation";
import { PAGES } from "../lib/pages";
import { Icon } from "./Icon";
import { KnegisMark } from "./NavPills";

type SheetView = (typeof SETTINGS_PANEL)[number]["views"][number];

const FIRST_TILE: SheetView = SETTINGS_PANEL[0].views[0];

/** A tile's word is `NAV`'s: Meetings, CLI, MCP, where the screens' own titles are longer. */
const LABEL_OF = Object.fromEntries(
  NAV.filter(isGroup).flatMap((group) => childrenOf(group).map((child) => [child.id, child.label])),
) as Record<SheetView, string>;

function isSheetView(view: ViewId): view is SheetView {
  return SETTINGS_PANEL.some((section) => (section.views as readonly ViewId[]).includes(view));
}

/** Today's day of the month, for the calendar's tile: a real date, not a picture of one. */
const today = () => new Date().getDate();

/**
 * What each tile's picture draws. Decoration, so `aria-hidden` on the holder; the
 * words are the label under it. Plain markup, styled in `styles.css`, because
 * pictures here are CSS: generated gradients and shapes, no photographs. Nothing
 * here is a reading: the meeting's tile says "rec" with no time, since the sheet
 * knows of no recording, and the calendar's date is today's.
 */
function TileArt({ id }: { id: SheetView }) {
  switch (id) {
    case "shortcuts":
      return <span className="art-keys"><span><kbd>ctrl</kbd><kbd>shift</kbd></span><kbd className="art-key-solid">space</kbd></span>;
    case "meeting-settings":
      return <><span className="art-weather" /><span className="art-label">rec</span></>;
    case "vocabulary":
      return <span className="art-letters">Aa</span>;
    case "transcription":
      return <span className="art-orb" />;
    case "intelligence":
      return <span className="art-lines"><span /><span /><span /><span /></span>;
    case "local-llm":
      return <span className="art-sphere" />;
    case "providers":
      return <span className="art-streak" />;
    case "calendar":
      return <><span className="art-date">{today()}</span><span className="art-date-dot" /></>;
    case "api":
      return <span className="art-api"><span className="art-api-call">GET /</span><span className="art-api-host"><span />127.0.0.1</span></span>;
    case "cli":
      return <span className="art-prompt"><span>&gt;</span><span className="art-cursor" /></span>;
    case "mcp":
      return (
        <svg className="art-nodes" width="96" height="96" viewBox="0 0 96 96" fill="none">
          <path d="M24 70L48 26L72 70Z" stroke="#0e3b8d" strokeWidth="1.4" />
          <circle cx="48" cy="26" r="10" fill="#0e3b8d" />
          <circle cx="24" cy="70" r="10" fill="#fff" />
          <circle cx="72" cy="70" r="10" fill="#7781c0" />
        </svg>
      );
    case "diagnostics":
      return <span className="art-bars"><span /><span /><span /><span /><span /></span>;
    default: {
      const unhandled: never = id;
      return unhandled;
    }
  }
}

/**
 * Settings, as a frosted sheet over the window: every settings and developer
 * screen is one square picture tile, in `SETTINGS_PANEL`'s two groups. What a screen
 * is for is the tile's tooltip and accessible description, so a reader who has
 * never met "Local API" can still learn it before choosing it.
 *
 * A modal `<dialog>`, so focus is held inside it and Escape closes it without
 * any code here. The tiles are one tab stop: Tab reaches the grid on the tile
 * for the current screen (or the first tile), and the arrow keys, Home and End
 * move between tiles, so the Settings sheet is not twelve Tab presses deep.
 * Choosing a tile closes the sheet and goes there; the tile for the screen
 * already showing is marked, so the sheet also says where you are.
 *
 * Pressing outside the sheet closes it, and a press on one of the title bar's
 * controls -- which the sheet's margin can just show -- is passed on to that
 * control. The Settings pill is the exception: passing that press on would open
 * the sheet it just closed.
 */
export function SettingsSheet({ open, view, version, onSelect, onHide, onDismiss }: { open: boolean; view: ViewId; version: string | null; onSelect: (id: ViewId) => void; onHide: () => void; onDismiss: () => void }) {
  const ref = useRef<HTMLDialogElement>(null);
  const [active, setActive] = useState<SheetView>(FIRST_TILE);
  useEffect(() => {
    const dialog = ref.current;
    if (!dialog) return;
    if (open && !dialog.open) {
      setActive(isSheetView(view) ? view : FIRST_TILE);
      dialog.showModal();
    }
    if (!open && dialog.open) dialog.close();
  }, [open, view]);

  // Close synchronously before navigation can open the unsaved-drafts dialog.
  // The native close event is only an acknowledgement, never a second intent.
  const hide = () => { ref.current?.close(); onHide(); };
  const dismiss = () => { hide(); onDismiss(); };

  const onBackdrop = (event: MouseEvent<HTMLDialogElement>) => {
    if (event.target !== event.currentTarget) return;
    const { clientX, clientY } = event;
    // Closed first, so the title bar is no longer inert when it is asked
    // what sits under the pointer.
    hide();
    const control = document.elementFromPoint(clientX, clientY)?.closest<HTMLButtonElement>(".app-titlebar button");
    if (control && !control.disabled && control.getAttribute("aria-haspopup") !== "dialog") {
      // Explicit navigation/window actions own this click; do not also return.
      control.click();
    } else onDismiss();
  };

  const onGridKey = (event: KeyboardEvent<HTMLDivElement>) => {
    const from = (event.target as HTMLElement).closest<HTMLButtonElement>("button.sheet-tile");
    if (!from) return;
    const tiles = [...event.currentTarget.querySelectorAll<HTMLButtonElement>("button.sheet-tile")];
    const at = tiles.indexOf(from);
    let to: HTMLButtonElement | undefined;
    if (event.key === "ArrowRight") to = tiles[at + 1];
    else if (event.key === "ArrowLeft") to = tiles[at - 1];
    else if (event.key === "Home") to = tiles[0];
    else if (event.key === "End") to = tiles[tiles.length - 1];
    else if (event.key === "ArrowDown" || event.key === "ArrowUp") to = verticalNeighbour(tiles, from, event.key === "ArrowDown" ? 1 : -1);
    else return;
    event.preventDefault();
    if (!to) return;
    const id = to.dataset.view as SheetView | undefined;
    if (id) setActive(id);
    to.focus();
  };

  return (
    <dialog
      ref={ref}
      className="settings-sheet"
      aria-labelledby="settings-sheet-title"
      onCancel={event => { event.preventDefault(); dismiss(); }}
      onClick={onBackdrop}
    >
      <div className="settings-sheet-body" onKeyDown={onGridKey}>
        <div className="settings-sheet-head">
          <span />
          <KnegisMark />
          <button type="button" className="sheet-close" aria-label="Close settings" title="Close settings" onClick={dismiss}>
            <Icon name="close" size={18} />
          </button>
        </div>
        <div className="settings-sheet-groups">
          {SETTINGS_PANEL.map((section, group) => {
            const Heading = group === 0 ? "h2" : "h3";
            return (
              <section key={section.heading} className="settings-sheet-section" aria-labelledby={`sheet-${section.heading}`}>
                <Heading id={group === 0 ? "settings-sheet-title" : `sheet-${section.heading}`} className="settings-sheet-label">{section.heading}</Heading>
                <ul className="settings-sheet-grid">
                  {section.views.map((id, index) => {
                    const page = PAGES[id];
                    return (
                      <li key={id}>
                        <button
                          type="button"
                          className="sheet-tile"
                          data-view={id}
                          tabIndex={id === active ? 0 : -1}
                          title={page.intro}
                          aria-description={page.intro}
                          aria-current={view === id ? "page" : undefined}
                          style={{ animationDelay: `${(group * 8 + index) * 30}ms` }}
                          onFocus={() => setActive(id)}
                          onClick={() => { hide(); onSelect(id); }}
                        >
                          <span className="sheet-tile-art" data-art={id} aria-hidden="true"><TileArt id={id} /></span>
                          <span className="sheet-tile-label">{LABEL_OF[id]}<span className="sr-only">{id === "meeting-settings" ? " settings" : ""}</span></span>
                        </button>
                      </li>
                    );
                  })}
                </ul>
              </section>
            );
          })}
        </div>
        {version === null ? null : <p className="settings-sheet-version">KnegisWhisper {version}</p>}
      </div>
    </dialog>
  );
}

/**
 * The tile a row above or below `from` whose centre is nearest `from`'s,
 * across section boundaries: the grids are separate lists, but to a reader
 * they are one sheet. Rows are told apart by their top edge.
 */
function verticalNeighbour(tiles: HTMLButtonElement[], from: HTMLButtonElement, direction: 1 | -1): HTMLButtonElement | undefined {
  const origin = from.getBoundingClientRect();
  const centre = origin.left + origin.width / 2;
  const rows = tiles
    .map((tile) => ({ tile, rect: tile.getBoundingClientRect() }))
    .filter(({ rect }) => (direction === 1 ? rect.top > origin.top + 1 : rect.top < origin.top - 1));
  if (rows.length === 0) return undefined;
  const nearestTop = direction === 1 ? Math.min(...rows.map(({ rect }) => rect.top)) : Math.max(...rows.map(({ rect }) => rect.top));
  const row = rows.filter(({ rect }) => Math.abs(rect.top - nearestTop) < 2);
  row.sort((a, b) => Math.abs(a.rect.left + a.rect.width / 2 - centre) - Math.abs(b.rect.left + b.rect.width / 2 - centre));
  return row[0]?.tile;
}

/**
 * The way back to the sheet from a settings screen: a pill above the screen's
 * title, as the reference frames draw "← Settings".
 */
export function BackToSettings({ onOpen }: { onOpen: () => void }) {
  return (
    <button type="button" className="back-to-settings" aria-haspopup="dialog" onClick={onOpen}>
      <Icon name="back" size={16} />
      Settings
    </button>
  );
}
