import { useCallback, useLayoutEffect, useMemo, useRef, useState, type Dispatch, type RefObject, type SetStateAction } from "react";

import { followBand, GlideWatch, inBand, RevealQueue, ScrollOwnership, scrollBehavior, type Band } from "./followScroll";
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

  // The glide under way, so one the browser stops short can be finished (KNEG-455 item 6). A key
  // typed into Find while a step's glide runs stops it where it is, its line still outside the
  // band, and no `scrollend` comes (measured in headless Edge, 2026-10-24: no app code scrolls then,
  // and what in the browser stops it is not traced). So each glide is watched a frame at a time
  // until the scroller holds still (`GlideWatch`); its line is then brought into the band at once
  // if it is not there. The person moving the transcript themselves (`interrupted`) ends the watch,
  // and so does a newer reveal.
  const glide = useRef<object | null>(null);
  const watchGlide = useCallback((index: number, block: ScrollLogicalPosition) => {
    const aside = asideRef.current;
    const scroller = aside === null ? null : scrollParent(aside);
    if (scroller === null) return;
    const watched = {};
    glide.current = watched;
    const watch = new GlideWatch(scroller.scrollTop);
    const frame = () => {
      if (glide.current !== watched) return;
      if (!watch.frame(scroller.scrollTop)) {
        requestAnimationFrame(frame);
        return;
      }
      glide.current = null;
      const line = rowRefs.current.get(index);
      if (line === undefined || line.getClientRects().length === 0 || lineInBand(index)) return;
      ownership.begin(performance.now());
      line.scrollIntoView({ block, behavior: "instant" });
    };
    requestAnimationFrame(frame);
  }, [asideRef, lineInBand, ownership]);

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
      const behavior = pending.behavior ?? scrollBehavior();
      ownership.begin(performance.now());
      line.scrollIntoView({ block: pending.block, behavior });
      if (behavior === "smooth") watchGlide(pending.index, pending.block);
      else glide.current = null;
    }
    pending.then?.();
  }, [ownership, queue, watchGlide]);

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

  const appScroll = useMemo<AppScroll>(() => ({
    begin: () => ownership.begin(performance.now()),
    scrolled: (now) => ownership.scroll(now),
    // A glide that ends by itself is still watched to its last frame: `scrollend` says nothing
    // about whether it got where it was going.
    ended: () => ownership.ended(),
    interrupted: () => {
      ownership.interrupted();
      glide.current = null;
    },
  }), [ownership]);

  return { rowRefs, reveal, revealCount, lineInBand, appScroll };
}
