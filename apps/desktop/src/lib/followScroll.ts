/**
 * Follow (KNEG-448): while it is on, a note's transcript keeps the playing line in view; the
 * person scrolling the transcript turns it off, and the player card's Follow chip turns it back on
 * -- the owner's choice of 2026-10-11.
 *
 * Every scroll counts unless it belongs to the app's reveal operation. Input interrupts that
 * operation immediately; scrollend or 150 ms without a scroll ends it too (KNEG-454).
 */

/** Fallback for browsers without scrollend, including reveals that move nothing. */
export const APP_SCROLL_IDLE_MS = 150;

/**
 * Ownership of one app reveal. A clock is supplied by the caller, so interrupted glides are
 * testable. It ends one of two ways, named apart so a reveal can tell them apart (KNEG-455):
 * `ended` when the scroll itself stops (`scrollend`), `interrupted` when the person does something
 * that moves the transcript themselves.
 */
export class ScrollOwnership {
  private until = Number.NEGATIVE_INFINITY;
  begin(now: number): void { this.until = now + APP_SCROLL_IDLE_MS; }
  scroll(now: number): boolean {
    if (now >= this.until) return false;
    this.until = now + APP_SCROLL_IDLE_MS;
    return true;
  }
  ended(): void { this.until = Number.NEGATIVE_INFINITY; }
  interrupted(): void { this.until = Number.NEGATIVE_INFINITY; }
}

/** Scrolls jump rather than glide when the person asked for less motion. */
export function scrollBehavior(): ScrollBehavior {
  return typeof matchMedia === "function" && matchMedia("(prefers-reduced-motion: reduce)").matches ? "auto" : "smooth";
}

/**
 * A line to bring into view once it can be (`useReveal`), and what to do once it has been; with
 * `behavior`, scrolled that way rather than as `scrollBehavior` says.
 */
export type Reveal = { index: number; block: ScrollLogicalPosition; then?: (() => void) | undefined; behavior?: ScrollBehavior | undefined };

/**
 * The reveal waiting to be shown: one at a time, the latest asked for. One that replaces another
 * still waiting runs that one's `then` before its own once it is shown, so a request overtaken --
 * a summary source by Follow -- is still reported done rather than left to be shown again later.
 */
export class RevealQueue {
  private pending: Reveal | null = null;
  private asked = 0;

  /** How many reveals have been asked for, so a deferred one can tell that something else was shown first. */
  get count(): number { return this.asked; }

  push(reveal: Reveal): void {
    this.asked += 1;
    const overtaken = this.pending?.then;
    const then = reveal.then;
    this.pending = { ...reveal, then: overtaken === undefined ? then : () => { overtaken(); then?.(); } };
  }

  /** The reveal waiting, taken off the queue, once `ready` says its line can be scrolled to; else null, and it waits. */
  take(ready: (index: number) => boolean): Reveal | null {
    const pending = this.pending;
    if (pending === null || !ready(pending.index)) return null;
    this.pending = null;
    return pending;
  }
}

/** The keys that scroll a scroller when nothing else takes them. Left and Right do not move the transcript, which never scrolls sideways. */
const SCROLL_KEYS: ReadonlySet<string> = new Set(["PageUp", "PageDown", "Home", "End", "ArrowUp", "ArrowDown", " "]);

/**
 * Where a key was pressed, for `keyReleasesFollow`: on the transcript's scroller itself, or with
 * focus on nothing (`<body>`) when the last press was in the scroller, which is then the one the
 * key scrolls (`page`); on a button, a line or a word inside the scroller (`inside`); or anywhere
 * else -- a text field, a select, the note beside the column, `<body>` after a press elsewhere
 * (`elsewhere`).
 */
export type KeyPlace = "page" | "inside" | "elsewhere";

/**
 * Whether a key scrolls the transcript, and so turns Follow off. `handled` is the event's
 * `defaultPrevented`: a line's arrow keys, Home and End move between lines instead, and let go
 * themselves (`TranscriptPanel`'s `move`). On the page every scrolling key does; from inside the
 * scroller every one but Space, which presses the button that has focus.
 */
export function keyReleasesFollow(key: string, place: KeyPlace, handled: boolean): boolean {
  if (handled || !SCROLL_KEYS.has(key)) return false;
  return place === "page" || (place === "inside" && key !== " ");
}

/**
 * How far above the scroller's foot the band ends beyond the card's own height: the lines'
 * `scroll-margin-bottom` (`styles.css`), so a line scrolled to the band's foot is not behind the card.
 */
export const CARD_CLEARANCE = 24;

/** A vertical extent in viewport pixels. */
export type Band = { top: number; bottom: number };

/**
 * The part of the transcript's scroller the playing line is kept in: the scroller's box inside its
 * border, less the sticky head at the top and the player card -- its height plus
 * `CARD_CLEARANCE` -- at the bottom. Both layouts: the column's card floats over its scroller's
 * foot, and the mini player is held to the foot of `.notes-main`.
 */
export function followBand(scroller: Band, headHeight: number, cardHeight: number): Band {
  return { top: scroller.top + headHeight, bottom: scroller.bottom - cardHeight - CARD_CLEARANCE };
}

/** A pixel either side that still counts as inside: a line scrolled to an edge can measure a fraction past it. */
const EDGE = 1;

/** Whether a line from `line.top` to `line.bottom` is wholly inside `band`. */
export function inBand(line: Band, band: Band): boolean {
  return line.top >= band.top - EDGE && line.bottom <= band.bottom + EDGE;
}
