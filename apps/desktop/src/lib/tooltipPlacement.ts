/**
 * Where a tooltip goes, given the thing it describes.
 *
 * Pure so the edge cases can be tested without a screen. The tooltip sits
 * centred under its anchor with its arrow at the anchor's centre; it moves
 * above when the space below the anchor is too small; and it is pushed back
 * inside the window edges, in which case the arrow stays where the anchor is
 * rather than in the middle of the box. Below wins when neither side fits,
 * because the pointer is usually above what it hovers.
 */
export type Rect = { left: number; top: number; width: number; height: number };
export type Size = { width: number; height: number };
export type Placement = { left: number; top: number; side: "below" | "above"; arrowLeft: number };

/** Space between the anchor's edge and the tooltip box; the arrow lives in it. */
export const TOOLTIP_GAP = 8;
/** Smallest distance the box keeps from the window edge. */
export const TOOLTIP_MARGIN = 6;
/** The arrow stays this far inside the box, so it never sits on a rounded corner. */
const ARROW_INSET = 8;

export function placeTooltip(anchor: Rect, tip: Size, viewport: Size): Placement {
  const fitsBelow = anchor.top + anchor.height + TOOLTIP_GAP + tip.height + TOOLTIP_MARGIN <= viewport.height;
  const fitsAbove = anchor.top - TOOLTIP_GAP - tip.height - TOOLTIP_MARGIN >= 0;
  const side = fitsBelow || !fitsAbove ? "below" : "above";
  const top = side === "below" ? anchor.top + anchor.height + TOOLTIP_GAP : anchor.top - TOOLTIP_GAP - tip.height;
  const centre = anchor.left + anchor.width / 2;
  const left = clamp(centre - tip.width / 2, TOOLTIP_MARGIN, Math.max(TOOLTIP_MARGIN, viewport.width - TOOLTIP_MARGIN - tip.width));
  const arrowLeft = clamp(centre - left, ARROW_INSET, Math.max(ARROW_INSET, tip.width - ARROW_INSET));
  return { left, top, side, arrowLeft };
}

function clamp(value: number, low: number, high: number): number {
  return Math.min(high, Math.max(low, value));
}
