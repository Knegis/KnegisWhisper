import { useEffect, useRef } from "react";

import { childrenOf, isGroup, NAV, SETTINGS_PANEL, srSuffixOf, type ViewId } from "../lib/navigation";
import { NAV_ICON } from "../lib/navIcons";
import { Icon } from "./Icon";

/**
 * Settings, as a frosted sheet of picture tiles over the window -- the way
 * oem.care's menu opens over its page. It replaces the Settings list panel:
 * every settings and developer screen is one tile, in `NAV`'s order, the
 * Settings group first and Developer under its own heading.
 *
 * A modal `<dialog>`, so focus is held inside it and Escape closes it without
 * any code here. Pressing outside the sheet's content closes it too. Choosing a
 * tile closes it and goes there; the tile for the screen already showing is
 * marked, so the sheet also says where you are.
 *
 * Each tile's picture is CSS (`.sheet-tile-art[data-art]` in `styles.css`):
 * generated gradients and shapes, no photographs.
 */
export function SettingsSheet({ open, view, onSelect, onClose }: { open: boolean; view: ViewId; onSelect: (id: ViewId) => void; onClose: () => void }) {
  const ref = useRef<HTMLDialogElement>(null);
  useEffect(() => {
    const dialog = ref.current;
    if (!dialog) return;
    if (open && !dialog.open) dialog.showModal();
    if (!open && dialog.open) dialog.close();
  }, [open]);
  const groups = NAV.filter(isGroup);
  return (
    <dialog
      ref={ref}
      className="settings-sheet"
      aria-labelledby="settings-sheet-title"
      onClose={onClose}
      onClick={(event) => { if (event.target === event.currentTarget) onClose(); }}
    >
      <div className="settings-sheet-body">
        <div className="settings-sheet-head">
          <h2 id="settings-sheet-title" className="settings-sheet-title">Settings</h2>
          <button type="button" className="sheet-close" onClick={onClose}>
            Close
            <span className="sheet-close-mark" aria-hidden="true"><Icon name="close" size={12} /></span>
          </button>
        </div>
        {SETTINGS_PANEL.map((section) => {
          const group = groups.find((entry) => entry.id === section.group);
          if (!group) return null;
          return (
            <section key={section.group} className="settings-sheet-section" aria-label={section.heading ?? group.label}>
              {section.heading ? <h3 className="settings-sheet-label">{section.heading.toLowerCase()}</h3> : null}
              <ul className="settings-sheet-grid">
                {childrenOf(group).map((child) => {
                  const suffix = srSuffixOf(child);
                  return (
                    <li key={child.id}>
                      <button
                        type="button"
                        className="sheet-tile"
                        aria-current={view === child.id ? "page" : undefined}
                        onClick={() => { onSelect(child.id); onClose(); }}
                      >
                        <span className="sheet-tile-art" data-art={child.id} aria-hidden="true">
                          <Icon name={NAV_ICON[child.id]} size={22} />
                        </span>
                        <span className="sheet-tile-label">
                          {child.label}
                          {suffix ? <span className="sr-only">{suffix}</span> : null}
                        </span>
                      </button>
                    </li>
                  );
                })}
              </ul>
            </section>
          );
        })}
      </div>
    </dialog>
  );
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
