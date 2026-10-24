import { useEffect, useLayoutEffect, useRef, type RefObject } from "react";

import { keyReleasesFollow, type KeyPlace } from "./followScroll";
import { useLatest } from "./useLatest";
import { scrollParent, type AppScroll, type RevealLine } from "./useReveal";

type FollowOptions = {
  visible: boolean;
  /** Which layout the panel is in: the column beside a note, or the Transcript tab. Each has its own scroller. */
  wide: boolean;
  follow: boolean;
  onReleaseFollow: () => void;
  takeRecentre: () => boolean;
  /** The line playing now, or null while paused or between lines. */
  activeIndex: number | null;
  /** The line the player is at, playing or paused (`TranscriptPanel`'s prop of that name). */
  currentIndex: number | null;
  /** Whether a summary source or search hit is about to be shown, which wins over Follow. */
  sourcePending: boolean;
  /** Whether a speaker rename is open, which holds Follow off. */
  renameOpen: boolean;
  asideRef: RefObject<HTMLElement | null>;
  reveal: RevealLine;
  revealCount: () => number;
  lineInBand: (index: number) => boolean;
  appScroll: AppScroll;
};

/**
 * Follow (KNEG-448), held by `NoteWorkspace`: while it is on, the line that starts playing is
 * centred whenever it is out of the band, the transcript coming on screen brings back the line the
 * player is at, and the person moving the transcript turns it off (`lib/followScroll.ts`).
 *
 * The panel calls this after `useTranscriptFind`: in a commit where both reveal a line, the last
 * scroll wins, and that is Follow's, as it was when these effects were the panel's own.
 */
export function useFollow({
  visible, wide, follow, onReleaseFollow, takeRecentre, activeIndex, currentIndex, sourcePending, renameOpen,
  asideRef, reveal, revealCount, lineInBand, appScroll,
}: FollowOptions) {
  const latest = useLatest({ follow, onReleaseFollow, activeIndex, currentIndex, sourcePending });
  // Whether the panel was on screen at the commit this effect last ran in. It runs whenever
  // `visible` changes, so that is the previous commit's `visible`: the transcript has just come on
  // screen when it was false, and the effect below brings the line back then instead.
  const wasVisible = useRef(false);

  // When the playing line changes or Follow turns on -- not on every render -- the playing line is
  // centred if it is out of the band; nothing moves while paused. A press of the chip, taken once,
  // centres the line the player is at, playing or not, in view or not.
  useLayoutEffect(() => {
    const shownBefore = wasVisible.current;
    wasVisible.current = visible;
    if (!visible || !follow) return;
    if (renameOpen) { onReleaseFollow(); return; }
    if (takeRecentre()) {
      const target = activeIndex ?? currentIndex;
      if (target !== null) reveal(target, "center");
      return;
    }
    if (shownBefore && activeIndex !== null && !lineInBand(activeIndex)) reveal(activeIndex, "center");
    // `currentIndex` is read as it is when one of these changes; it changing alone moves nothing.
  }, [activeIndex, follow, visible, takeRecentre, reveal, lineInBand, renameOpen, onReleaseFollow]);

  // The transcript coming on screen -- the Transcript tab opened, or the layout switched so it is
  // in the column -- brings back the line the player is at, playing or paused, when it is out of
  // view; not when a summary source or a search hit is about to be shown instead. Two frames on,
  // once the card that appears with it has its height (`--player-h`, written by a ResizeObserver
  // after layout): the scroll margins centre the line against that height.
  //
  // Whether a source is to be shown is this commit's `sourcePending`: the source effect shows it,
  // and clears the request, long before those two frames, and read then the request would be gone
  // and the line the player is at would take its place (KNEG-406's "selected and in view").
  // Anything shown in the meantime wins the same way. Declared after the effect above, so a line
  // that effect reveals counts as shown before this one takes its count.
  useLayoutEffect(() => {
    if (!visible || sourcePending) return;
    const revealsThen = revealCount();
    let frame = requestAnimationFrame(() => {
      frame = requestAnimationFrame(() => {
        const { follow: following, sourcePending: showing, activeIndex: playingLine, currentIndex: at } = latest.current;
        const target = playingLine ?? at;
        if (!following || showing || renameOpen || revealCount() !== revealsThen || target === null) return;
        if (!lineInBand(target)) reveal(target, "center");
      });
    });
    return () => cancelAnimationFrame(frame);
    // `sourcePending` is read as it is when one of these changes; it changing alone moves nothing.
  }, [visible, reveal, revealCount, lineInBand, renameOpen]);

  // What counts as the person scrolling the transcript (`lib/followScroll.ts`): input events on
  // whatever scrolls it, while it is on screen. Not `scroll` events, which Follow's own scrolls
  // fire too. Taken off when the panel hides or unmounts, and moved to the other scroller when the
  // layout switches.
  useEffect(() => {
    const aside = asideRef.current;
    const scroller = visible && aside !== null ? scrollParent(aside) : null;
    if (scroller === null) return;
    const letGo = () => {
      appScroll.interrupted();
      const { follow: following, onReleaseFollow: release } = latest.current;
      if (following) release();
    };
    const scroll = () => { if (!appScroll.scrolled(performance.now())) letGo(); };
    const scrollEnd = () => appScroll.ended();
    const wheel = () => letGo();
    // A touch on the player card scrubs or presses it rather than scrolling the transcript: below
    // 1280px the mini card is inside `.notes-main`, the scroller listened on here (KNEG-448).
    const touch = (event: TouchEvent) => {
      if (event.target instanceof Element && event.target.closest(".recording-player") !== null) return;
      letGo();
    };
    // A press on the scrollbar lands on the scroller itself, to the right of its content box.
    const press = (event: PointerEvent) => {
      // Middle-button autoscroll, overlay scrollbars and selection dragging all start with a
      // pointer press. Player controls are not a request to move the transcript.
      if (event.target instanceof Element && !event.target.closest(".recording-player")) {
        appScroll.interrupted();
        if (event.button === 1 || (event.target === scroller && event.offsetX >= scroller.clientWidth)) letGo();
      }
    };
    // Whether the last press anywhere landed in the transcript's scroller. With focus on nothing
    // (`<body>`), a key scrolls the scroller pressed last, as Chromium picks it: after a press in
    // the note column beside a wide transcript, PageDown scrolls the note, and Follow stays on.
    let pressedHere = false;
    const pressedAnywhere = (event: PointerEvent) => {
      pressedHere = event.target instanceof Node && scroller.contains(event.target);
    };
    const key = (event: KeyboardEvent) => {
      const target = event.target;
      const place: KeyPlace = target === scroller || (target === document.body && pressedHere) ? "page"
        : target instanceof HTMLElement && target !== document.body && scroller.contains(target) && !target.isContentEditable && !target.matches("input, textarea, select") ? "inside"
        : "elsewhere";
      if (keyReleasesFollow(event.key, place, event.defaultPrevented)) letGo();
    };
    const focused = (event: FocusEvent) => {
      if (event.target instanceof Element && event.target.matches("button, input, textarea, select") && !event.target.closest(".recording-player")) appScroll.interrupted();
    };
    scroller.addEventListener("scroll", scroll, { passive: true });
    scroller.addEventListener("scrollend", scrollEnd);
    scroller.addEventListener("focusin", focused);
    scroller.addEventListener("wheel", wheel, { passive: true });
    scroller.addEventListener("touchmove", touch, { passive: true });
    scroller.addEventListener("pointerdown", press);
    document.addEventListener("pointerdown", pressedAnywhere, { capture: true, passive: true });
    document.addEventListener("keydown", key);
    return () => {
      appScroll.interrupted();
      scroller.removeEventListener("scroll", scroll);
      scroller.removeEventListener("scrollend", scrollEnd);
      scroller.removeEventListener("focusin", focused);
      scroller.removeEventListener("wheel", wheel);
      scroller.removeEventListener("touchmove", touch);
      scroller.removeEventListener("pointerdown", press);
      document.removeEventListener("pointerdown", pressedAnywhere, { capture: true });
      document.removeEventListener("keydown", key);
    };
    // `asideRef`, `appScroll` and `latest` never change; the scroller does, with the layout.
  }, [asideRef, appScroll, latest, visible, wide]);
}
