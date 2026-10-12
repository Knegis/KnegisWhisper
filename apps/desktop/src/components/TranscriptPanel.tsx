import { useCallback, useDeferredValue, useEffect, useId, useLayoutEffect, useMemo, useRef, useState, type Dispatch, type ReactNode, type SetStateAction } from "react";

import { followBand, inBand, keyReleasesFollow, releasesFollow, type Band, type KeyPlace, type ScrollInput } from "../lib/followScroll";
import type { NoteRecording, TranscriptRow } from "../lib/ipc";
import { findMatches, findStatus, firstMatchWhere, matchFrom, normaliseQuery, regroupMatches, rowOrder, sameMatches, stepMatch, type FindMatch } from "../lib/transcriptFind";
import { extendSelection, lineAt, lineStop, navigableLines, NO_SELECTION, stepLine, type LineSelection, type TranscriptSection } from "../lib/transcriptLayout";
import { wordAt, wordsMatchText } from "../lib/wordTiming";
import { Icon } from "./Icon";
import { TranscriptFindBar } from "./TranscriptFindBar";
import { scrollBehavior, TranscriptSections, type RenameTarget, type TranscriptActions } from "./TranscriptSections";

const CURRENT_WORD = "transcript-word-current";
/** What Find has while it is closed or finds nothing: one list and one map, so nothing redraws for a new empty one. */
const NO_MATCHES: readonly FindMatch[] = [];
const NOTHING_FOUND: ReadonlyMap<number, readonly FindMatch[]> = new Map();

/**
 * The transcript's label, in the machine voice, at the top of whatever scrolls it: the column
 * beside a note in a wide window, or `.notes-main` under the Transcript tab (KNEG-448). The panel
 * draws it with its Find button, and without the label while Find is open in its place;
 * `NoteWorkspace` draws it alone over the column's loading, error and empty lines, so the column
 * is labelled the same before its rows arrive.
 */
export function TranscriptHeading({ children, labelled = true }: { children?: ReactNode; labelled?: boolean }) {
  return <div className="transcript-head">
    {labelled ? <h2 className="transcript-label">Transcript</h2> : null}
    {children}
  </div>;
}

/**
 * What scrolls `element`: its nearest ancestor that scrolls its overflow -- `.note-side-body`
 * beside a note in a wide window, `.notes-main` under the Transcript tab below it.
 */
function scrollParent(element: Element): HTMLElement | null {
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
function visibleBand(aside: HTMLElement): Band | null {
  const scroller = scrollParent(aside);
  if (scroller === null) return null;
  const top = scroller.getBoundingClientRect().top + scroller.clientTop;
  const head = aside.querySelector<HTMLElement>(":scope > .transcript-head")?.offsetHeight ?? 0;
  const card = parseFloat(getComputedStyle(aside).getPropertyValue("--player-h")) || 0;
  return followBand({ top, bottom: top + scroller.clientHeight }, head, card);
}

/**
 * A line to bring into view once it can be (`reveal`), and what to do once it has been; with
 * `behavior`, scrolled that way rather than as `scrollBehavior` says.
 */
type Reveal = { index: number; block: ScrollLogicalPosition; then?: (() => void) | undefined; behavior?: ScrollBehavior | undefined };

/** The Find match gone to last (KNEG-448): its ordinal in `matches`, the list of the query it was found for. */
type FindPosition = { matches: readonly FindMatch[]; ordinal: number };

type TranscriptPanelProps = {
  readOnly?: boolean;
  /**
   * A row to show: scrolled to, once. The owner has already selected it
   * (`NoteWorkspace`'s `openRow`); the panel only brings it into view, opening
   * its section first when it is closed.
   */
  sourceRequest?: { index: number; nonce: number } | null;
  /**
   * Called once `sourceRequest` has been shown, so the owner can clear it. A
   * request that stayed set would be applied again by a panel mounted afresh
   * -- a wide window moving the transcript into its column -- or by new rows
   * after a speaker rename, and would scroll to an old summary source nobody
   * just asked for.
   */
  onSourceShown?: () => void;
  rows: TranscriptRow[];
  /** `rows` grouped into sections and turns (`transcriptSections`), computed once per rows by the owner. */
  sections: readonly TranscriptSection[];
  recording: NoteRecording | null;
  currentTime: number;
  playing: boolean;
  visible: boolean;
  /** Plays from `start` to the end with Follow on (`NoteWorkspace`'s `playFrom`): a line or a word pressed. */
  playFrom: (start: number) => void;
  /** The player's time now (`RecordingPlayerHandle.time`), read every animation frame while a row with words plays. */
  time: () => number | null;
  /**
   * The note's selected lines (KNEG-448). Held by `NoteWorkspace`, whose player card offers to
   * play them and to clear them; a line's click, Shift-click, Shift+Arrow and Escape change them
   * through `onSelectionChange`.
   */
  selection: LineSelection;
  onSelectionChange: Dispatch<SetStateAction<LineSelection>>;
  suggestions: readonly string[];
  onRename: (speakerId: string, name: string) => Promise<void>;
  /**
   * Follow (KNEG-448), held by `NoteWorkspace`, whose player card has its chip: while it is on, the
   * line that starts playing is centred in view whenever it is not already, opening its section
   * when it is closed. Nothing follows while the recording is paused; the chip turning it on
   * (`takeRecentre`), or the transcript coming on screen while it is on, still brings back the
   * line the player is at (`currentIndex`). The chip is a toggle: pressed while on, it turns
   * Follow off.
   */
  follow: boolean;
  /**
   * Turns Follow off: the person scrolled the transcript (a wheel, a touch anywhere but the player
   * card, the scrollbar, a key that scrolls), moved between lines with the keys, or closed the
   * section that is playing.
   */
  onReleaseFollow: () => void;
  /** True once after the card's chip turned Follow on, when the line the player is at is to be centred. */
  takeRecentre: () => boolean;
  /**
   * The line the player is at, playing or paused: the last to have started (`lastStartedLine`),
   * which the chip and the transcript coming on screen bring back when nothing is playing, and
   * which holds the lines' tab stop when no line has had focus and nothing is selected.
   */
  currentIndex: number | null;
};

export function TranscriptPanel({
  readOnly = false, sourceRequest, onSourceShown, rows, sections, recording, currentTime, playing, visible, playFrom, time,
  selection, onSelectionChange, suggestions, onRename, follow, onReleaseFollow, takeRecentre, currentIndex,
}: TranscriptPanelProps) {
  const idPrefix = useId();
  const asideRef = useRef<HTMLElement>(null);
  const rowRefs = useRef(new Map<number, HTMLLIElement>());
  // The playing row's words, when it is drawn word by word.
  const wordsRef = useRef<HTMLSpanElement>(null);
  const [renaming, setRenaming] = useState<RenameTarget | null>(null);
  // Sections the reader closed, by key; every section starts open (KNEG-448).
  const [collapsed, setCollapsed] = useState<ReadonlySet<string>>(() => new Set());
  // The line that holds the lines' one tab stop ahead of the selection (`lineStop`): the line that
  // last took focus, or the one the stop moved to when its section was closed. A summary source or
  // a search hit clears it, so the stop follows the line they select.
  const [cursor, setCursor] = useState<number | null>(null);
  // The lines the arrow keys move between: the timed lines of the open sections.
  const lines = useMemo(() => navigableLines(sections, collapsed), [sections, collapsed]);

  // Today's playing-row predicate (`lineAt`), and only while the recording plays.
  const activeRow = useMemo(() => (playing ? lineAt(rows, currentTime) : null), [currentTime, playing, rows]);
  const activeIndex = activeRow?.index ?? null;

  // Where each line and turn sits, so only the section and turn holding the playing line, the tab
  // stop or the open rename are handed it, and so a line's section can be opened to show it.
  const places = useMemo(() => {
    const rowsAt = new Map<number, { section: string; turn: string }>();
    const turnsAt = new Map<string, string>();
    for (const section of sections) {
      for (const turn of section.turns) {
        turnsAt.set(turn.key, section.key);
        for (const row of turn.rows) rowsAt.set(row.index, { section: section.key, turn: turn.key });
      }
    }
    return { rowsAt, turnsAt };
  }, [sections]);
  const activePlace = activeIndex === null ? undefined : places.rowsAt.get(activeIndex);
  // The lines' one tab stop: the cursor, else the selection's anchor, else the line the player is
  // at (`currentIndex`), else the first -- whichever is first on a timed line in an open section.
  // The line the player is at holds through the gaps between lines and while paused, so someone
  // tabbing in then lands where the recording is rather than on line 1, and the stop does not
  // move -- redrawing the first section and turn -- at every gap.
  const stopIndex = useMemo(() => lineStop(lines, [cursor, selection.anchor, currentIndex]), [lines, cursor, selection.anchor, currentIndex]);
  const stopPlace = stopIndex === null ? undefined : places.rowsAt.get(stopIndex);

  // What the lines' handlers and the panel's listeners read, so the handlers -- and the one
  // `actions` object holding them -- stay the same from render to render and never redraw a
  // memoised line by themselves. Declared before every effect that reads it, so it is current there.
  const canPlay = recording !== null;
  const sourcePending = Boolean(sourceRequest);
  const latest = useRef({ rows, sections, lines, collapsed, places, stopIndex, activeIndex, currentIndex, sourcePending, selection, canPlay, playFrom, onSelectionChange, onRename, follow, onReleaseFollow, playing });
  useLayoutEffect(() => {
    latest.current = { rows, sections, lines, collapsed, places, stopIndex, activeIndex, currentIndex, sourcePending, selection, canPlay, playFrom, onSelectionChange, onRename, follow, onReleaseFollow, playing };
  });

  // One way to bring a line into view (KNEG-448), for Follow, a summary source or search hit, and
  // the arrow keys: a closed section holding it is opened first, and the scroll waits for the
  // commit that draws it -- `scrollIntoView` on a hidden line does nothing. `then` runs once it
  // has scrolled; a reveal that replaces one still waiting runs that one's `then` too, so a source
  // request overtaken by Follow is still reported done rather than left to be shown again later.
  // The lines' scroll margins (`styles.css`) keep it clear of the sticky head and the player card,
  // so `center` centres it on the band between them.
  const pendingReveal = useRef<Reveal | null>(null);
  // How many reveals have been asked for, so a deferred one can tell that something else was shown first.
  const reveals = useRef(0);
  const flushReveal = useCallback(() => {
    const pending = pendingReveal.current;
    if (pending === null) return;
    const line = rowRefs.current.get(pending.index);
    if (line !== undefined && line.getClientRects().length === 0) return;
    pendingReveal.current = null;
    line?.scrollIntoView({ block: pending.block, behavior: pending.behavior ?? scrollBehavior() });
    pending.then?.();
  }, []);
  const reveal = useCallback((index: number, block: ScrollLogicalPosition, then?: () => void, behavior?: ScrollBehavior) => {
    reveals.current += 1;
    const overtaken = pendingReveal.current?.then;
    pendingReveal.current = { index, block, then: overtaken === undefined ? then : () => { overtaken(); then?.(); }, behavior };
    const section = latest.current.places.rowsAt.get(index)?.section;
    if (section !== undefined && latest.current.collapsed.has(section)) {
      setCollapsed((previous) => {
        const next = new Set(previous);
        next.delete(section);
        return next;
      });
    } else {
      flushReveal();
    }
  }, [flushReveal]);
  // After every commit: a check of one ref, unless a reveal is waiting on the section it opened.
  useLayoutEffect(() => { flushReveal(); });

  const actions = useMemo<TranscriptActions>(() => ({
    register: (index, element) => {
      if (element === null) rowRefs.current.delete(index);
      else rowRefs.current.set(index, element);
    },
    wordsRef,
    choose: (row, extend) => {
      const { rows: all, canPlay: hasRecording, playFrom: play, onSelectionChange: select } = latest.current;
      if (row.startSeconds === null) return;
      if (extend) {
        select((previous) => extendSelection(all, previous, row.index));
        return;
      }
      select({ rows: new Set([row.index]), anchor: row.index });
      // To the end: a line is where to start listening, not a clip (the owner's ruling, 2026-10-11).
      if (hasRecording) play(row.startSeconds);
    },
    playWord: (row, seconds) => {
      const { canPlay: hasRecording, playFrom: play, onSelectionChange: select } = latest.current;
      select({ rows: new Set([row.index]), anchor: row.index });
      if (hasRecording) play(seconds);
    },
    move: (row, key, extend) => {
      const { rows: all, lines: open, onSelectionChange: select, follow: following, onReleaseFollow: release } = latest.current;
      const target = stepLine(open, row.index, key);
      if (target === null) return;
      // Shift with nothing selected extends from the line focus is leaving.
      if (extend) select((previous) => extendSelection(all, previous.anchor === null ? { rows: previous.rows, anchor: row.index } : previous, target));
      // Going through the lines by key is the person moving the transcript: Follow lets go of it.
      if (following) release();
      const element = rowRefs.current.get(target);
      if (!element) return;
      element.focus({ preventScroll: true });
      reveal(target, "nearest");
    },
    clear: () => {
      const { selection: now, onSelectionChange: select } = latest.current;
      if (now.rows.size === 0 && now.anchor === null) return false;
      select(NO_SELECTION);
      return true;
    },
    focused: (index) => setCursor(index),
    openRename: (target) => setRenaming(target),
    closeRename: (speakerId) => setRenaming((previous) => previous?.speakerId === speakerId ? null : previous),
    rename: (speakerId, name) => latest.current.onRename(speakerId, name),
  }), [reveal]);
  const toggleSection = useCallback((key: string) => {
    const { sections: all, collapsed: closed, places: at, stopIndex: stop, activeIndex: playingLine, follow: following, onReleaseFollow: release } = latest.current;
    if (!closed.has(key)) {
      // Closing the section that is playing puts it away: Follow lets go rather than open it again.
      if (following && playingLine !== null && at.rowsAt.get(playingLine)?.section === key) release();
      // The tab stop leaves the closed section for the nearest line after it, or before it when
      // none follows, rather than the transcript's first line. Focus stays on the toggle.
      if (stop !== null && at.rowsAt.get(stop)?.section === key) {
        setCursor(stepLine(navigableLines(all, new Set([...closed, key])), stop, "ArrowDown"));
      }
    }
    setCollapsed((previous) => {
      const next = new Set(previous);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return next;
    });
  }, []);

  // The owner selected the row (`NoteWorkspace`'s `openRow`); here it is only brought into view --
  // its section opened first -- and it takes the tab stop from whichever line had focus before.
  // The request is reported shown only once it has been.
  useEffect(() => {
    if (!sourceRequest || !visible) return;
    if (!rows.some((item) => item.index === sourceRequest.index)) return;
    setCursor(null);
    reveal(sourceRequest.index, "nearest", onSourceShown);
  }, [sourceRequest, rows, visible, onSourceShown, reveal]);

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
  }, []);

  // Find in this transcript (KNEG-448): the head's search button opens a bar in its label's place.
  // The query is searched a moment after it is typed (`useDeferredValue`), so typing stays quick in
  // a long transcript, and the lines are drawn with what it found, row by row.
  const [finding, setFinding] = useState(false);
  const [query, setQuery] = useState("");
  const searched = useDeferredValue(query);
  // The matches drawn now, kept when a new query finds the same ("MÖTE" after "möte", a space
  // typed after a word), so nothing is redrawn and the current match stays.
  const drawnMatches = useRef(NO_MATCHES);
  const matches = useMemo(() => {
    const next = finding ? findMatches(rows, searched) : NO_MATCHES;
    return sameMatches(drawnMatches.current, next) ? drawnMatches.current : next;
  }, [finding, rows, searched]);
  useLayoutEffect(() => { drawnMatches.current = matches; }, [matches]);
  // The same by row, so a new query keeps a line's matches -- and so the line -- when they are the
  // same (`regroupMatches`), and a playback render keeps them all.
  const drawnFound = useRef(NOTHING_FOUND);
  const found = useMemo(() => regroupMatches(drawnFound.current, matches), [matches]);
  useLayoutEffect(() => { drawnFound.current = found; }, [found]);
  const order = useMemo(() => rowOrder(rows), [rows]);
  // The match gone to last. The current match is it while it is one of these matches; for a new
  // query's matches, the first from it on (`matchFrom`), which the effect below then goes to --
  // worked out here, so the line it is in is drawn with it at once rather than first without it.
  // Until a query has gone to a match there is none.
  const [position, setPosition] = useState<FindPosition | null>(null);
  const was = position === null ? undefined : position.matches[position.ordinal];
  const current = position !== null && position.matches === matches ? position.ordinal
    : was !== undefined ? matchFrom(matches, order, was) : null;
  const currentMatch = current === null ? undefined : matches[current];
  const findPlace = currentMatch === undefined ? undefined : places.rowsAt.get(currentMatch.row);
  const findInput = useRef<HTMLInputElement>(null);
  const findButton = useRef<HTMLButtonElement>(null);
  // Set as Find closes, so the Find button takes focus back once it is drawn again.
  const refocusFind = useRef(false);

  // Goes to match `ordinal` of `list`: its line brought to the middle of the band, its section
  // opened first. Nothing is selected or played, and focus stays where it is, in the field. While
  // something plays, going to a match is the person moving the transcript, so Follow lets go.
  const goToMatch = useCallback((list: readonly FindMatch[], ordinal: number, behavior?: ScrollBehavior) => {
    const match = list[ordinal];
    if (match === undefined) return;
    setPosition({ matches: list, ordinal });
    const { playing: now, follow: following, onReleaseFollow: release } = latest.current;
    if (now && following) release();
    reveal(match.row, "center", undefined, behavior);
  }, [reveal]);

  // A new query goes to its first match at or after where the reader is: the match that was
  // current, when there was one, so a letter more or less keeps to the same place; otherwise the
  // line at the top of the visible band (the first match whose line reaches below the band's top,
  // or whose section's header does while the section is closed), and the first match when none is
  // that far on. A letter added that still finds the same match moves nothing. The line jumps
  // there rather than gliding: a key typed while the glide runs stops it where it is -- in headless
  // Edge, typing "möte" left the glide to its first match 10px from where it began, with no other
  // scroll asked for. Believed, not traced: the browser bringing the field's caret into view.
  useLayoutEffect(() => {
    if (matches.length === 0) return;
    let start: number | null;
    if (was !== undefined) {
      start = current;
    } else {
      const aside = asideRef.current;
      const band = aside === null ? null : visibleBand(aside);
      start = band === null ? 0 : firstMatchWhere(matches, (match) => {
        const line = rowRefs.current.get(match.row);
        const box = line === undefined ? null : line.getClientRects().length > 0 ? line : line.closest(".transcript-section");
        return box !== null && box.getBoundingClientRect().bottom > band.top;
      }) ?? 0;
    }
    const next = start === null ? undefined : matches[start];
    if (start === null || next === undefined) return;
    if (was !== undefined && next.row === was.row && next.start === was.start) setPosition({ matches, ordinal: start });
    else goToMatch(matches, start, "instant");
    // `was` and `current` are read as they are when the matches change; they changing alone moves nothing.
  }, [matches, goToMatch]);

  // Enter, Shift+Enter, Previous and Next: the next or the previous match, round from the last to the first.
  const stepFind = (direction: 1 | -1) => {
    const next = stepMatch(current, matches.length, direction);
    if (next !== null) goToMatch(matches, next);
  };
  const closeFind = () => {
    refocusFind.current = true;
    setFinding(false);
    setQuery("");
    setPosition(null);
  };
  // The field takes focus as the bar opens; the Find button takes it back as the bar closes.
  useLayoutEffect(() => {
    if (finding) {
      findInput.current?.focus({ preventScroll: true });
    } else if (refocusFind.current) {
      refocusFind.current = false;
      findButton.current?.focus({ preventScroll: true });
    }
  }, [finding]);
  // Nothing while there is no query; "No matches" while it finds nothing.
  const findCount = normaliseQuery(searched) === "" ? null : findStatus(current, matches.length);

  // Whether the panel was on screen at the last commit, so the effect below can leave the
  // transcript's coming on screen to the one after it.
  const shownBefore = useRef(false);

  // Following (KNEG-448). When the playing line changes or Follow turns on -- not on every render
  // -- the playing line is centred if it is out of the band; nothing moves while paused. A press of
  // the chip, taken once, centres the line the player is at, playing or not, in view or not.
  useLayoutEffect(() => {
    if (!visible || !follow) return;
    if (takeRecentre()) {
      const target = activeIndex ?? currentIndex;
      if (target !== null) reveal(target, "center");
      return;
    }
    if (shownBefore.current && activeIndex !== null && !lineInBand(activeIndex)) reveal(activeIndex, "center");
    // `currentIndex` is read as it is when one of these changes; it changing alone moves nothing.
  }, [activeIndex, follow, visible, takeRecentre, reveal, lineInBand]);

  // The transcript coming on screen -- the Transcript tab opened, or the column mounted afresh as
  // the layout switches -- brings back the line the player is at, playing or paused, when it is out
  // of view; not when a summary source or a search hit is about to be shown instead. Two frames
  // on, once the card that appears with it has its height (`--player-h`, written by a
  // ResizeObserver after layout): the scroll margins centre the line against that height.
  //
  // Whether a source is to be shown is read now, in the commit that brought the transcript on
  // screen: the source effect shows it, and clears the request, long before those two frames, and
  // read then the request would be gone and the line the player is at would take its place
  // (KNEG-406's "selected and in view"). Anything shown in the meantime wins the same way.
  useLayoutEffect(() => {
    shownBefore.current = visible;
    if (!visible || latest.current.sourcePending) return;
    const revealsThen = reveals.current;
    let frame = requestAnimationFrame(() => {
      frame = requestAnimationFrame(() => {
        const { follow: following, sourcePending: showing, activeIndex: playingLine, currentIndex: at } = latest.current;
        const target = playingLine ?? at;
        if (!following || showing || reveals.current !== revealsThen || target === null) return;
        if (!lineInBand(target)) reveal(target, "center");
      });
    });
    return () => cancelAnimationFrame(frame);
  }, [visible, reveal, lineInBand]);

  // When Follow last turned on, for the wheel's grace (`FOLLOW_INERTIA_MS`): never, until it does.
  const followOnAt = useRef(Number.NEGATIVE_INFINITY);
  const followWas = useRef(follow);
  useLayoutEffect(() => {
    if (follow && !followWas.current) followOnAt.current = performance.now();
    followWas.current = follow;
  }, [follow]);

  // What counts as the person scrolling the transcript (`lib/followScroll.ts`): input events on
  // whatever scrolls it, while it is on screen. Not `scroll` events, which Follow's own scrolls
  // fire too. Taken off when the panel hides, unmounts, or remounts in the other layout.
  useEffect(() => {
    const aside = asideRef.current;
    const scroller = visible && aside !== null ? scrollParent(aside) : null;
    if (scroller === null) return;
    const letGo = (input: ScrollInput) => {
      const { follow: following, onReleaseFollow: release } = latest.current;
      if (following && releasesFollow(input, performance.now() - followOnAt.current)) release();
    };
    const wheel = () => letGo("wheel");
    // A touch on the player card scrubs or presses it rather than scrolling the transcript: below
    // 1280px the mini card is inside `.notes-main`, the scroller listened on here (KNEG-448).
    const touch = (event: TouchEvent) => {
      if (event.target instanceof Element && event.target.closest(".recording-player") !== null) return;
      letGo("touch");
    };
    // A press on the scrollbar lands on the scroller itself, to the right of its content box.
    const press = (event: PointerEvent) => {
      if (event.target === scroller && event.offsetX >= scroller.clientWidth) letGo("scrollbar");
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
      if (keyReleasesFollow(event.key, place, event.defaultPrevented)) letGo("key");
    };
    scroller.addEventListener("wheel", wheel, { passive: true });
    scroller.addEventListener("touchmove", touch, { passive: true });
    scroller.addEventListener("pointerdown", press);
    document.addEventListener("pointerdown", pressedAnywhere, { capture: true, passive: true });
    document.addEventListener("keydown", key);
    return () => {
      scroller.removeEventListener("wheel", wheel);
      scroller.removeEventListener("touchmove", touch);
      scroller.removeEventListener("pointerdown", press);
      document.removeEventListener("pointerdown", pressedAnywhere, { capture: true });
      document.removeEventListener("keydown", key);
    };
  }, [visible]);

  // The playing row's words, when it has words that spell it (KNEG-408). Every other row --
  // notes saved before, and every engine but local Whisper -- highlights as a row, as before. While
  // Find has matches in the playing row, the row draws them in place of its words (KNEG-448), so
  // there are none to light: the loop below stops, and starts again on the words drawn afresh once
  // Find closes or stops finding anything in the row.
  const activeFound = activeRow !== null && found.has(activeRow.index);
  const activeWords = useMemo(
    () => (activeRow !== null && !activeFound && wordsMatchText(activeRow.words, activeRow.text) ? activeRow.words : null),
    [activeRow, activeFound],
  );

  // Which word is being said moves faster than `timeupdate` (four a second) and the row's React
  // render, so while a row with words plays and the panel shows, each animation frame reads the
  // player's time and moves one class between two spans, with no render. Nothing runs otherwise.
  useEffect(() => {
    const container = wordsRef.current;
    if (!playing || !visible || activeWords === null || container === null) return;
    const spans = container.querySelectorAll<HTMLElement>(".transcript-word");
    let lit: number | null = null;
    let frame = 0;
    const step = () => {
      const now = time();
      const next = now === null ? null : wordAt(activeWords, now);
      if (next !== lit) {
        if (lit !== null) spans[lit]?.classList.remove(CURRENT_WORD);
        if (next !== null) spans[next]?.classList.add(CURRENT_WORD);
        lit = next;
      }
      frame = requestAnimationFrame(step);
    };
    frame = requestAnimationFrame(step);
    return () => {
      cancelAnimationFrame(frame);
      if (lit !== null) spans[lit]?.classList.remove(CURRENT_WORD);
    };
  }, [activeWords, playing, visible, time]);

  return (
    <aside ref={asideRef} className="transcript" aria-label="Transcript">
      <TranscriptHeading labelled={!finding}>
        {finding ? (
          <TranscriptFindBar inputRef={findInput} query={query} onQuery={setQuery} status={findCount}
            canStep={matches.length > 0} onStep={stepFind} onClose={closeFind} />
        ) : (
          <button ref={findButton} type="button" className="transcript-find" aria-label="Find in transcript" title="Find in transcript"
            onClick={() => setFinding(true)}>
            <Icon name="search" size={16} />
          </button>
        )}
      </TranscriptHeading>
      <TranscriptSections
        sections={sections}
        collapsed={collapsed}
        onToggle={toggleSection}
        activeIndex={activeIndex}
        activeSection={activePlace?.section ?? null}
        activeTurn={activePlace?.turn ?? null}
        stopIndex={stopIndex}
        stopSection={stopPlace?.section ?? null}
        stopTurn={stopPlace?.turn ?? null}
        renaming={renaming}
        renamingSection={renaming === null ? null : places.turnsAt.get(renaming.turnKey) ?? null}
        idPrefix={idPrefix}
        selected={selection.rows}
        found={found}
        findCurrent={current}
        findSection={findPlace?.section ?? null}
        findTurn={findPlace?.turn ?? null}
        readOnly={readOnly}
        canPlay={canPlay}
        suggestions={suggestions}
        actions={actions}
      />
    </aside>
  );
}
