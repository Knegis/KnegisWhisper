import { useEffect, useRef, useState, type KeyboardEvent, type MouseEvent } from "react";

import { SETTINGS_PANEL, type ViewId } from "../lib/navigation";
import { NAV_ICON } from "../lib/navIcons";
import { PAGES } from "../lib/pages";
import { Icon } from "./Icon";

type SheetView = (typeof SETTINGS_PANEL)[number]["views"][number];

const FIRST_TILE: SheetView = SETTINGS_PANEL[0].views[0];

function isSheetView(view: ViewId): view is SheetView {
  return SETTINGS_PANEL.some((section) => (section.views as readonly ViewId[]).includes(view));
}

/**
 * Settings, as a frosted sheet of tiles over the window -- the way oem.care's
 * menu opens over its page. It replaces the Settings list panel: every
 * settings and developer screen is one tile, in the four sections
 * `SETTINGS_PANEL` lays out, each tile carrying its screen's title and the
 * one-sentence intro that screen opens with, so a reader who has never met
 * "Local API" learns what it is before choosing it.
 *
 * A modal `<dialog>`, so focus is held inside it and Escape closes it without
 * any code here. The tiles are one tab stop: Tab reaches the grid on the tile
 * for the current screen (or the first tile), and the arrow keys, Home and End
 * move between tiles, so the Settings sheet is not twelve Tab presses deep.
 * Choosing a tile closes the sheet and goes there; the tile for the screen
 * already showing is marked, so the sheet also says where you are.
 *
 * Pressing outside the sheet closes it, and a press on one of the title bar's
 * controls -- visible under the backdrop, but inert while the dialog is modal
 * -- is passed on to that control, so pressing "Meetings" with the sheet open
 * goes to Meetings rather than only closing the sheet. The Settings pill is
 * the exception: passing that press on would open the sheet it just closed.
 *
 * Each tile's picture is CSS (`.sheet-tile-art[data-art]` in `styles.css`):
 * generated gradients and shapes, no photographs.
 */
export function SettingsSheet({ open, view, onSelect, onClose }: { open: boolean; view: ViewId; onSelect: (id: ViewId) => void; onClose: () => void }) {
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

  const onBackdrop = (event: MouseEvent<HTMLDialogElement>) => {
    if (event.target !== event.currentTarget) return;
    const { clientX, clientY } = event;
    // Closed first, so the title bar is no longer inert when it is asked
    // what sits under the pointer.
    event.currentTarget.close();
    const control = document.elementFromPoint(clientX, clientY)?.closest<HTMLButtonElement>(".app-titlebar button");
    if (control && !control.disabled && control.getAttribute("aria-haspopup") !== "dialog") control.click();
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
      onClose={onClose}
      onClick={onBackdrop}
    >
      <div className="settings-sheet-body" onKeyDown={onGridKey}>
        <div className="settings-sheet-head">
          <h2 id="settings-sheet-title" className="settings-sheet-title">Settings</h2>
          <button type="button" className="sheet-close" onClick={onClose}>
            Close
            <span className="sheet-close-mark" aria-hidden="true"><Icon name="close" size={12} /></span>
          </button>
        </div>
        {SETTINGS_PANEL.map((section) => (
          <section key={section.heading} className="settings-sheet-section" aria-labelledby={`sheet-${section.views[0]}`}>
            <h3 id={`sheet-${section.views[0]}`} className="settings-sheet-label">{section.heading}</h3>
            <ul className="settings-sheet-grid">
              {section.views.map((id) => {
                const page = PAGES[id];
                return (
                  <li key={id}>
                    <button
                      type="button"
                      className="sheet-tile"
                      data-view={id}
                      tabIndex={id === active ? 0 : -1}
                      aria-current={view === id ? "page" : undefined}
                      onFocus={() => setActive(id)}
                      onClick={() => { onSelect(id); onClose(); }}
                    >
                      <span className="sheet-tile-art" data-art={id} aria-hidden="true">
                        <Icon name={NAV_ICON[id]} size={20} />
                      </span>
                      <span className="sheet-tile-label">{page.title}</span>
                      <span className="sheet-tile-intro">{page.intro}</span>
                    </button>
                  </li>
                );
              })}
            </ul>
          </section>
        ))}
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
      <Icon name="back" size={14} />
      Settings
    </button>
  );
}
