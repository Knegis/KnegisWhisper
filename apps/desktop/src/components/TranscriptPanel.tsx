import { useCallback, useEffect, useId, useLayoutEffect, useMemo, useRef, useState, type Dispatch, type ReactNode, type SetStateAction } from "react";

import { followBand, inBand, keyReleasesFollow, releasesFollow, type KeyPlace, type ScrollInput } from "../lib/followScroll";
import type { NoteRecording, TranscriptRow } from "../lib/ipc";
import { extendSelection, lineAt, lineStop, navigableLines, NO_SELECTION, stepLine, type LineSelection, type TranscriptSection } from "../lib/transcriptLayout";
import { wordAt, wordsMatchText } from "../lib/wordTiming";
import { Icon } from "./Icon";
import { scrollBehavior, TranscriptSections, type RenameTarget, type TranscriptActions } from "./TranscriptSections";

const CURRENT_WORD = "transcript-word-current";

/**
 * The transcript's label, in the machine voice, at the top of whatever scrolls it: the column
 * beside a note in a wide window, or `.notes-main` under the Transcript tab (KNEG-448). The panel
 * draws it with its Find button; `NoteWorkspace` draws it alone over the column's loading, error
 * and empty lines, so the column is labelled the same before its rows arrive.
 */
export function TranscriptHeading({ children }: { children?: ReactNode }) {
  return <div className="transcript-head">
    <h2 className="transcript-label">Transcript</h2>
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

/** A line to bring into view once it can be (`reveal`), and what to do once it has been. */
type Reveal = { index: number; block: ScrollLogicalPosition; then?: (() => void) | undefined };

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
   * when it is closed. Nothing moves while the recording is paused.
   */
  follow: boolean;
  /**
   * Turns Follow off: the person scrolled the transcript (a wheel, a touch, the scrollbar, a key
   * that scrolls), moved between lines with the keys, or closed the section that is playing.
   */
  onReleaseFollow: () => void;
  /** True once after the card's chip turned Follow on, when the line the player is at is to be centred. */
  takeRecentre: () => boolean;
  /**
   * The line the player is at, playing or paused: the last to have started (`lastStartedLine`),
   * which the chip and the transcript coming on screen bring back when nothing is playing.
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
  // The lines' one tab stop: the cursor, else the selection's anchor, else the playing line, else
  // the first -- whichever is first on a timed line in an open section.
  const stopIndex = useMemo(() => lineStop(lines, [cursor, selection.anchor, activeIndex]), [lines, cursor, selection.anchor, activeIndex]);
  const stopPlace = stopIndex === null ? undefined : places.rowsAt.get(stopIndex);

  // What the lines' handlers and the panel's listeners read, so the handlers -- and the one
  // `actions` object holding them -- stay the same from render to render and never redraw a
  // memoised line by themselves. Declared before every effect that reads it, so it is current there.
  const canPlay = recording !== null;
  const sourcePending = Boolean(sourceRequest);
  const latest = useRef({ rows, sections, lines, collapsed, places, stopIndex, activeIndex, currentIndex, sourcePending, selection, canPlay, playFrom, onSelectionChange, onRename, follow, onReleaseFollow });
  useLayoutEffect(() => {
    latest.current = { rows, sections, lines, collapsed, places, stopIndex, activeIndex, currentIndex, sourcePending, selection, canPlay, playFrom, onSelectionChange, onRename, follow, onReleaseFollow };
  });

  // One way to bring a line into view (KNEG-448), for Follow, a summary source or search hit, and
  // the arrow keys: a closed section holding it is opened first, and the scroll waits for the
  // commit that draws it -- `scrollIntoView` on a hidden line does nothing. `then` runs once it
  // has scrolled; a reveal that replaces one still waiting runs that one's `then` too, so a source
  // request overtaken by Follow is still reported done rather than left to be shown again later.
  // The lines' scroll margins (`styles.css`) keep it clear of the sticky head and the player card,
  // so `center` centres it on the band between them.
  const pendingReveal = useRef<Reveal | null>(null);
  const flushReveal = useCallback(() => {
    const pending = pendingReveal.current;
    if (pending === null) return;
    const line = rowRefs.current.get(pending.index);
    if (line !== undefined && line.getClientRects().length === 0) return;
    pendingReveal.current = null;
    line?.scrollIntoView({ block: pending.block, behavior: scrollBehavior() });
    pending.then?.();
  }, []);
  const reveal = useCallback((index: number, block: ScrollLogicalPosition, then?: () => void) => {
    const overtaken = pendingReveal.current?.then;
    pendingReveal.current = { index, block, then: overtaken === undefined ? then : () => { overtaken(); then?.(); } };
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
    const scroller = scrollParent(aside);
    if (scroller === null) return true;
    const top = scroller.getBoundingClientRect().top + scroller.clientTop;
    const head = aside.querySelector<HTMLElement>(":scope > .transcript-head")?.offsetHeight ?? 0;
    const card = parseFloat(getComputedStyle(aside).getPropertyValue("--player-h")) || 0;
    const rect = line.getBoundingClientRect();
    return inBand({ top: rect.top, bottom: rect.bottom }, followBand({ top, bottom: top + scroller.clientHeight }, head, card));
  }, []);

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
  useLayoutEffect(() => {
    shownBefore.current = visible;
    if (!visible) return;
    let frame = requestAnimationFrame(() => {
      frame = requestAnimationFrame(() => {
        const { follow: following, sourcePending: showing, activeIndex: playingLine, currentIndex: at } = latest.current;
        const target = playingLine ?? at;
        if (following && !showing && target !== null && !lineInBand(target)) reveal(target, "center");
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
    const touch = () => letGo("touch");
    // A press on the scrollbar lands on the scroller itself, to the right of its content box.
    const press = (event: PointerEvent) => {
      if (event.target === scroller && event.offsetX >= scroller.clientWidth) letGo("scrollbar");
    };
    const key = (event: KeyboardEvent) => {
      const target = event.target;
      const place: KeyPlace = target === document.body || target === scroller ? "page"
        : target instanceof HTMLElement && scroller.contains(target) && !target.isContentEditable && !target.matches("input, textarea, select") ? "inside"
        : "elsewhere";
      if (keyReleasesFollow(event.key, place, event.defaultPrevented)) letGo("key");
    };
    scroller.addEventListener("wheel", wheel, { passive: true });
    scroller.addEventListener("touchmove", touch, { passive: true });
    scroller.addEventListener("pointerdown", press);
    document.addEventListener("keydown", key);
    return () => {
      scroller.removeEventListener("wheel", wheel);
      scroller.removeEventListener("touchmove", touch);
      scroller.removeEventListener("pointerdown", press);
      document.removeEventListener("keydown", key);
    };
  }, [visible]);

  // The playing row's words, when it has words that spell it (KNEG-408). Every other row --
  // notes saved before, and every engine but local Whisper -- highlights as a row, as before.
  const activeWords = useMemo(
    () => (activeRow !== null && wordsMatchText(activeRow.words, activeRow.text) ? activeRow.words : null),
    [activeRow],
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
      <TranscriptHeading>
        {/* Find in the transcript arrives later in KNEG-448; the button holds its place until then. */}
        <button type="button" className="transcript-find" aria-label="Find in transcript" title="Find in transcript" disabled>
          <Icon name="search" size={16} />
        </button>
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
        readOnly={readOnly}
        canPlay={canPlay}
        suggestions={suggestions}
        actions={actions}
      />
    </aside>
  );
}
