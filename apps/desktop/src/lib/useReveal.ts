import { useCallback, useLayoutEffect, useMemo, useRef, useState, type Dispatch, type RefObject, type SetStateAction } from "react";

import { followBand, inBand, RevealQueue, ScrollOwnership, scrollBehavior, type Band } from "./followScroll";
import { useLatest } from "./useLatest";

/**
 * What scrolls `element`: its nearest ancestor that scrolls its overflow -- `.note-side-body`
 * beside a note in a wide window, `.notes-main` under the Transcript tab below it.
 */
export function scrollParent(element: Element): HTMLElement | null {
  for (let parent = element.parentElement; parent !== null; parent = parent.parentElement) {
    const { overflowY } = getComputedStyle(parent);
    if (overflowY === "auto" || overflowY === "scroll") return parent;
  }
  return null;
}

/**
 * The band of `aside`'s scroller Follow keeps the playing line in (`followBand`): the scroller
 * inside its border, less the sticky head and the card with its clearance. Null when nothing
 * scrolls the panel.
 */
export function visibleBand(aside: HTMLElement): Band | null {
  const scroller = scrollParent(aside);
  if (scroller === null) return null;
  const top = scroller.getBoundingClientRect().top + scroller.clientTop;
  const head = aside.querySelector<HTMLElement>(":scope > .transcript-head")?.offsetHeight ?? 0;
  const card = parseFloat(getComputedStyle(aside).getPropertyValue("--player-h")) || 0;
  return followBand({ top, bottom: top + scroller.clientHeight }, head, card);
}

/**
 * The app's side of the transcript's scrolling (KNEG-454): a scroll it set off belongs to it until
 * the scroll ends or the person does something that moves the transcript themselves. `ended` is
 * for `scrollend`; `interrupted` for a wheel, a touch, a press, a scrolling key, focus moving to a
 * control, a rename opening, or the listeners coming off.
 */
export type AppScroll = {
  /** A scroll the app is about to set off: a reveal, or the anchoring after a section toggles. */
  begin: () => void;
  /** Whether a scroll seen now belongs to the app, renewing its hold if it does. */
  scrolled: (now: number) => boolean;
  ended: () => void;
  interrupted: () => void;
};

export type RevealLine = (index: number, block: ScrollLogicalPosition, then?: () => void, behavior?: ScrollBehavior) => void;

/**
 * One way to bring a transcript line into view (KNEG-448), for Follow, a summary source or search
 * hit, Find, and the arrow keys: a closed section holding it is opened first, and the scroll waits
 * for the commit that draws it -- `scrollIntoView` on a hidden line does nothing. `then` runs once
 * it has scrolled; a reveal that replaces one still waiting runs that one's `then` too
 * (`RevealQueue`), so a source request overtaken by Follow is still reported done rather than left
 * to be shown again later. The lines' scroll margins (`styles.css`) keep it clear of the sticky
 * head and the player card, so `center` centres it on the band between them.
 *
 * `rowRefs` is where the panel's lines register their elements. `collapsed` and `setCollapsed` are
 * the panel's closed sections, which a reveal opens.
 */
export function useReveal(
  asideRef: RefObject<HTMLElement | null>,
  rowsAt: ReadonlyMap<number, { section: string }>,
  collapsed: ReadonlySet<string>,
  setCollapsed: Dispatch<SetStateAction<ReadonlySet<string>>>,
) {
  const rowRefs = useRef(new Map<number, HTMLLIElement>());
  const [ownership] = useState(() => new ScrollOwnership());
  const [queue] = useState(() => new RevealQueue());
  const latest = useLatest({ rowsAt, collapsed });

  const flushReveal = useCallback(() => {
    const pending = queue.take((index) => {
      const line = rowRefs.current.get(index);
      // A line not drawn at all is gone, and its reveal is dropped; one drawn hidden -- its
      // section still closed, or the panel off screen -- waits for a later commit.
      return line === undefined || line.getClientRects().length > 0;
    });
    if (pending === null) return;
    const line = rowRefs.current.get(pending.index);
    if (line !== undefined) {
      ownership.begin(performance.now());
      line.scrollIntoView({ block: pending.block, behavior: pending.behavior ?? scrollBehavior() });
    }
    pending.then?.();
  }, [ownership, queue]);

  const reveal = useCallback<RevealLine>((index, block, then, behavior) => {
    queue.push({ index, block, then, behavior });
    const section = latest.current.rowsAt.get(index)?.section;
    if (section !== undefined && latest.current.collapsed.has(section)) {
      setCollapsed((previous) => {
        const next = new Set(previous);
        next.delete(section);
        return next;
      });
    } else {
      flushReveal();
    }
  }, [flushReveal, latest, queue, setCollapsed]);
  // After every commit: a check of the queue, unless a reveal is waiting on the section it opened or
  // on the panel coming on screen. Not narrowed to `collapsed`: the panel showing is a commit too.
  useLayoutEffect(() => { flushReveal(); });

  /** How many reveals have been asked for, so a deferred one can tell that something else was shown first. */
  const revealCount = useCallback(() => queue.count, [queue]);

  // Whether line `index` is drawn wholly inside the band Follow keeps the playing line in
  // (`followBand`): the scroller, less the sticky head and the card with its clearance.
  const lineInBand = useCallback((index: number): boolean => {
    const aside = asideRef.current;
    const line = rowRefs.current.get(index);
    if (!aside || !line || line.getClientRects().length === 0) return false;
    const band = visibleBand(aside);
    if (band === null) return true;
    const rect = line.getBoundingClientRect();
    return inBand({ top: rect.top, bottom: rect.bottom }, band);
  }, [asideRef]);

  const appScroll = useMemo<AppScroll>(() => ({
    begin: () => ownership.begin(performance.now()),
    scrolled: (now) => ownership.scroll(now),
    ended: () => ownership.ended(),
    interrupted: () => ownership.interrupted(),
  }), [ownership]);

  return { rowRefs, reveal, revealCount, lineInBand, appScroll };
}
