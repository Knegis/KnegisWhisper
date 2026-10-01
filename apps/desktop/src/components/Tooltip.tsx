/**
 * One tooltip for the window, drawn for every element that carries a `title`.
 *
 * The native bubble is Windows' own: white, square, and drawn by the OS with
 * no regard for the theme. This replaces it without touching the buttons. A
 * listener on the document watches the pointer enter, or keyboard focus land
 * on, anything with a `title`; it moves the text into `data-tooltip` so the
 * browser never shows its bubble, waits, and renders the text in a box under
 * the element. Leaving, pressing (with the pointer or from the keyboard),
 * scrolling or Escape hides it and puts the
 * attribute back, so accessible names and the markup stay as they were.
 *
 * Mounted once, in the main window only. The meeting panel and the overlay
 * are small always-on-top windows, and a box floated near their edge would be
 * cut off by the window itself; they keep the native tooltip.
 */
import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { placeTooltip, type Placement } from "../lib/tooltipPlacement";

const SHOW_DELAY_MS = 400;
const STASH = "data-tooltip";
const ANCHOR = "[title], [data-tooltip]";

type Active = { anchor: HTMLElement; text: string };

export function Tooltips() {
  const [active, setActive] = useState<Active | null>(null);
  const [placement, setPlacement] = useState<Placement | null>(null);
  const box = useRef<HTMLDivElement>(null);

  useEffect(() => {
    let timer: number | undefined;
    let current: HTMLElement | null = null;
    // Only the attribute moves; React owns the prop and sets a new value if it
    // changes, so the stash is restored only when nothing has taken its place.
    const stash = (element: HTMLElement) => {
      const title = element.getAttribute("title");
      if (title !== null) { element.setAttribute(STASH, title); element.removeAttribute("title"); }
      return element.getAttribute(STASH) ?? "";
    };
    const restore = (element: HTMLElement) => {
      const text = element.getAttribute(STASH);
      if (text === null) return;
      element.removeAttribute(STASH);
      if (!element.hasAttribute("title")) element.setAttribute("title", text);
    };
    const hide = () => {
      window.clearTimeout(timer); timer = undefined;
      if (current) { restore(current); current = null; }
      setActive(null);
    };
    const arm = (element: HTMLElement, delay: number) => {
      if (element === current) return;
      hide();
      const text = stash(element).trim();
      if (!text) { restore(element); return; }
      current = element;
      timer = window.setTimeout(() => { if (current === element && element.isConnected) setActive({ anchor: element, text }); }, delay);
    };
    const anchorOf = (target: EventTarget | null) => {
      const element = target instanceof Element ? target.closest(ANCHOR) : null;
      return element instanceof HTMLElement ? element : null;
    };
    const over = (event: PointerEvent) => { const anchor = anchorOf(event.target); if (anchor) arm(anchor, SHOW_DELAY_MS); };
    const out = (event: PointerEvent) => {
      if (current && !(event.relatedTarget instanceof Node && current.contains(event.relatedTarget))) hide();
    };
    // Keyboard users get it at once, and only when focus is visibly theirs; a
    // click that focuses a button already shows the button being pressed.
    const focusIn = (event: FocusEvent) => { const anchor = anchorOf(event.target); if (anchor && anchor.matches(":focus-visible")) arm(anchor, 0); };
    const focusOut = (event: FocusEvent) => { if (current && current === event.target) hide(); };
    const escape = (event: KeyboardEvent) => { if (event.key === "Escape") hide(); };
    document.addEventListener("pointerover", over);
    document.addEventListener("pointerout", out);
    document.addEventListener("pointerdown", hide, true);
    // A press from the keyboard (Enter or Space on a button) has no pointerdown, and a button that
    // names what it will do next -- Hide list, Show list -- changes its `title` as it is pressed: a
    // tooltip left up would say the old one.
    document.addEventListener("click", hide, true);
    document.addEventListener("focusin", focusIn);
    document.addEventListener("focusout", focusOut);
    document.addEventListener("keydown", escape);
    document.addEventListener("scroll", hide, true);
    window.addEventListener("resize", hide);
    window.addEventListener("blur", hide);
    return () => {
      hide();
      document.removeEventListener("pointerover", over);
      document.removeEventListener("pointerout", out);
      document.removeEventListener("pointerdown", hide, true);
      document.removeEventListener("click", hide, true);
      document.removeEventListener("focusin", focusIn);
      document.removeEventListener("focusout", focusOut);
      document.removeEventListener("keydown", escape);
      document.removeEventListener("scroll", hide, true);
      window.removeEventListener("resize", hide);
      window.removeEventListener("blur", hide);
    };
  }, []);

  useLayoutEffect(() => {
    if (!active || !box.current) { setPlacement(null); return; }
    const anchor = active.anchor.getBoundingClientRect();
    const tip = box.current.getBoundingClientRect();
    const placed = placeTooltip(anchor, tip, { width: window.innerWidth, height: window.innerHeight });
    // Snap to device pixels: the anchor's rectangle is fractional, and a hairline border laid on a
    // fraction of a pixel blurs, most visibly where the arrow meets the box at 150% scaling.
    const snap = (value: number) => Math.round(value * window.devicePixelRatio) / window.devicePixelRatio;
    setPlacement({ ...placed, left: snap(placed.left), top: snap(placed.top), arrowLeft: snap(placed.arrowLeft) });
  }, [active]);

  if (!active) return null;
  return createPortal(
    <div ref={box} role="tooltip" className="tooltip" data-side={placement?.side ?? "below"}
      style={placement ? { left: placement.left, top: placement.top } : { left: 0, top: 0, visibility: "hidden" }}>
      {active.text}
      <span className="tooltip-arrow" aria-hidden="true" style={{ left: placement?.arrowLeft ?? 0 }} />
    </div>,
    document.body,
  );
}
