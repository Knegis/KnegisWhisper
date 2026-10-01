/**
 * Where a chip's menu goes (`components/ChipMenu.tsx`), given the chip.
 *
 * Pure so the edge cases can be tested without a screen, as `placeTooltip`
 * is, and with its shapes and its margin from the window edge. The menu
 * hangs from the chip's left edge, so its rows line up with the words that
 * opened it. It opens above the chip when it fits there, and below only when
 * it does not. Above first, because a chip sits at the top of the control bar
 * at the foot of the window: a short menu also fits below, but there it
 * covers the bar's own result and Start button, and menus on one strip would
 * open in different directions by their length. Dictation's four-row
 * microphone menu opened below at 1000x720 under a below-first rule, over
 * 146px of the bar (measured in headless Edge against dev fixtures, not in
 * the Tauri window). It is then pushed back inside the window's left and
 * right edges.
 *
 * Where it fits on neither side -- a long list in the 720x520 minimum window
 * -- it opens on the side with more room, and `maxHeight` is that room, so
 * the menu scrolls inside the window instead of leaving it. `maxHeight` is
 * the room on the chosen side in every case, which is at least the menu's
 * height whenever it fits.
 */
import { TOOLTIP_GAP, TOOLTIP_MARGIN, type Rect, type Size } from "./tooltipPlacement.js";

export type MenuPlacement = { left: number; top: number; side: "below" | "above"; maxHeight: number };

/**
 * Space between the chip and the menu: the tooltip's. A chip in the control
 * bar sits 7px inside the bar's top edge -- the strip's 6px padding and its
 * 1px border -- so anything less lays the menu's edge over the bar's.
 */
export const MENU_GAP = TOOLTIP_GAP;

export function placeMenu(anchor: Rect, menu: Size, viewport: Size): MenuPlacement {
  const below = anchor.top + anchor.height + MENU_GAP;
  const roomBelow = viewport.height - TOOLTIP_MARGIN - below;
  const roomAbove = anchor.top - MENU_GAP - TOOLTIP_MARGIN;
  const fitsBelow = menu.height <= roomBelow;
  const fitsAbove = menu.height <= roomAbove;
  const side = fitsAbove || (!fitsBelow && roomAbove >= roomBelow) ? "above" : "below";
  const maxHeight = Math.max(0, side === "below" ? roomBelow : roomAbove);
  const top = side === "below" ? below : anchor.top - MENU_GAP - Math.min(menu.height, maxHeight);
  const left = clamp(anchor.left, TOOLTIP_MARGIN, Math.max(TOOLTIP_MARGIN, viewport.width - TOOLTIP_MARGIN - menu.width));
  return { left, top, side, maxHeight };
}

function clamp(value: number, low: number, high: number): number {
  return Math.min(high, Math.max(low, value));
}
