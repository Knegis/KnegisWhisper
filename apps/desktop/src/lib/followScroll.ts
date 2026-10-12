/**
 * Follow (KNEG-448): while it is on, a note's transcript keeps the playing line in view; the
 * person scrolling the transcript turns it off, and the player card's Follow chip turns it back on
 * -- the owner's choice of 2026-10-11.
 *
 * What counts as the person scrolling is an input event -- a wheel, a touch moving, a press on
 * the scrollbar, a key that scrolls -- never a `scroll` event, which the transcript's own
 * `scrollIntoView` fires too. The decisions are here, pure, so they are testable without a DOM;
 * `TranscriptPanel` listens for the events and measures the boxes.
 */

/**
 * How long after Follow turns on a wheel is still taken for the flick before it -- a touchpad
 * goes on sending wheel events for a while after the fingers lift -- rather than a new scroll.
 */
export const FOLLOW_INERTIA_MS = 400;

/** The person's inputs that scroll the transcript. */
export type ScrollInput = "wheel" | "touch" | "scrollbar" | "key";

/** Whether `input`, `sinceOn` ms after Follow last turned on, turns it off. */
export function releasesFollow(input: ScrollInput, sinceOn: number): boolean {
  return input !== "wheel" || sinceOn >= FOLLOW_INERTIA_MS;
}

/** The keys that scroll a scroller when nothing else takes them. Left and Right do not move the transcript, which never scrolls sideways. */
const SCROLL_KEYS: ReadonlySet<string> = new Set(["PageUp", "PageDown", "Home", "End", "ArrowUp", "ArrowDown", " "]);

/**
 * Where a key was pressed, for `keyReleasesFollow`: with focus on nothing (`<body>`) or on the
 * transcript's scroller itself (`page`); on a button, a line or a word inside the scroller
 * (`inside`); or anywhere else -- a text field, a select, the note beside the column (`elsewhere`).
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
