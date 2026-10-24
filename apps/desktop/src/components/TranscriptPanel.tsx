import { useCallback, useEffect, useId, useLayoutEffect, useMemo, useRef, useState, type Dispatch, type ReactNode, type SetStateAction } from "react";

import type { NoteRecording, TranscriptRow } from "../lib/ipc";
import { extendSelection, lineAt, lineStop, navigableLines, NO_SELECTION, stepLine, type LineSelection, type TranscriptSection } from "../lib/transcriptLayout";
import { useFollow } from "../lib/useFollow";
import { useLatest } from "../lib/useLatest";
import { useReveal } from "../lib/useReveal";
import { useTranscriptFind } from "../lib/useTranscriptFind";
import { wordAt, wordsMatchText } from "../lib/wordTiming";
import { Icon } from "./Icon";
import { TranscriptFindBar } from "./TranscriptFindBar";
import { TranscriptSections, type RenameTarget, type TranscriptActions } from "./TranscriptSections";

const CURRENT_WORD = "transcript-word-current";

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
   * request that stayed set would be applied again whenever the transcript
   * came back on screen, or new rows arrived after a speaker rename, and would
   * scroll to an old summary source nobody just asked for.
   */
  onSourceShown?: () => void;
  rows: TranscriptRow[];
  /** `rows` grouped into sections and turns (`transcriptSections`), computed once per rows by the owner. */
  sections: readonly TranscriptSection[];
  recording: NoteRecording | null;
  currentTime: number;
  playing: boolean;
  visible: boolean;
  wide?: boolean;
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
   * Follow off. The panel's side of it is `useFollow`.
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

/**
 * A note's transcript: its sections and turns, the playing line and word, the selection and the
 * lines' one tab stop, speaker renames, and -- in three hooks of their own (KNEG-455) -- bringing a
 * line into view (`useReveal`), Find (`useTranscriptFind`) and Follow (`useFollow`). They are
 * called in that order: the last two reveal through the first, and in a commit where both reveal a
 * line, Follow's, the last, wins.
 */
export function TranscriptPanel({
  readOnly = false, sourceRequest, onSourceShown, rows, sections, recording, currentTime, playing, visible, wide = false, playFrom, time,
  selection, onSelectionChange, suggestions, onRename, follow, onReleaseFollow, takeRecentre, currentIndex,
}: TranscriptPanelProps) {
  const idPrefix = useId();
  const asideRef = useRef<HTMLElement>(null);
  // The playing row's words, when it is drawn word by word.
  const wordsRef = useRef<HTMLSpanElement>(null);
  const [renaming, setRenaming] = useState<RenameTarget | null>(null);
  const renameOpen = useRef<RenameTarget | null>(null);
  const renameOrigin = useRef<number | null>(null);
  const returnRenameFocus = useRef(false);
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

  const { rowRefs, reveal, revealCount, lineInBand, appScroll } = useReveal(asideRef, places.rowsAt, collapsed, setCollapsed);

  // What the lines' handlers read, so the handlers -- and the one `actions` object holding them --
  // stay the same from render to render and never redraw a memoised line by themselves.
  const canPlay = recording !== null;
  const latest = useLatest({ readOnly, rows, sections, lines, collapsed, places, stopIndex, activeIndex, selection, canPlay, playFrom, onSelectionChange, onRename, follow, onReleaseFollow });

  const openRename = useCallback((target: RenameTarget, origin?: number) => {
    if (latest.current.readOnly) return;
    const turn = latest.current.sections.flatMap(section => section.turns).find(item => item.key === target.turnKey);
    const stop = latest.current.stopIndex;
    renameOrigin.current = origin ?? (turn?.rows.some(row => row.index === stop) ? stop : turn?.rows[0]?.index ?? null);
    renameOpen.current = target;
    appScroll.interrupted();
    latest.current.onReleaseFollow();
    setRenaming(target);
  }, [appScroll, latest]);

  // A rename closing gives focus back to the line it was opened from, or the next line when that
  // one has gone; read in the commit that closes it, with that commit's lines.
  useLayoutEffect(() => {
    if (renaming !== null || !returnRenameFocus.current) return;
    returnRenameFocus.current = false;
    const origin = renameOrigin.current;
    if (origin === null) return;
    const target = lines.includes(origin) ? origin : stepLine(lines, origin, "ArrowDown");
    if (target === null) return;
    rowRefs.current.get(target)?.focus({ preventScroll: true });
    reveal(target, "nearest");
  }, [renaming, reveal, rowRefs, lines]);

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
      if (extend && all.some(item => item.index === target && item.startSeconds !== null)) select((previous) => extendSelection(all, previous.anchor === null ? { rows: previous.rows, anchor: row.index } : previous, target));
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
    openRename,
    renameLine: (index) => {
      const turn = latest.current.sections.flatMap(section => section.turns).find(item => item.rows.some(row => row.index === index));
      if (turn?.speakerId == null) return;
      openRename({ turnKey: turn.key, speakerId: turn.speakerId, current: turn.speaker ?? turn.speakerId }, index);
    },
    closeRename: (speakerId) => {
      if (renameOpen.current?.speakerId !== speakerId) return;
      renameOpen.current = null;
      returnRenameFocus.current = true;
      setRenaming(null);
    },
    rename: (speakerId, name) => latest.current.onRename(speakerId, name),
  }), [latest, reveal, rowRefs, openRename]);
  const toggleSection = useCallback((key: string) => {
    // Browser scroll anchoring after this app layout change belongs to the section operation.
    appScroll.begin();
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
  }, [appScroll, latest]);

  // The owner selected the row (`NoteWorkspace`'s `openRow`); here it is only brought into view --
  // its section opened first -- and it takes the tab stop from whichever line had focus before.
  // The request is reported shown only once it has been.
  useEffect(() => {
    if (!sourceRequest || !visible) return;
    if (!rows.some((item) => item.index === sourceRequest.index)) return;
    setCursor(null);
    reveal(sourceRequest.index, "nearest", onSourceShown);
  }, [sourceRequest, rows, visible, onSourceShown, reveal]);

  const find = useTranscriptFind({ rows, visible, playing, follow, onReleaseFollow, asideRef, rowRefs, reveal });
  const findPlace = find.currentRow === null ? undefined : places.rowsAt.get(find.currentRow);

  useFollow({
    visible, wide, follow, onReleaseFollow, takeRecentre, activeIndex, currentIndex,
    sourcePending: Boolean(sourceRequest), renameOpen: renaming !== null,
    asideRef, reveal, revealCount, lineInBand, appScroll,
  });

  // The playing row's words, when it has words that spell it (KNEG-408). Every other row --
  // notes saved before, and every engine but local Whisper -- highlights as a row, as before. While
  // Find has matches in the playing row, the row draws them in place of its words (KNEG-448), so
  // there are none to light: the loop below stops, and starts again on the words drawn afresh once
  // Find closes or stops finding anything in the row.
  const activeFound = activeRow !== null && find.found.has(activeRow.index);
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
      <TranscriptHeading labelled={!find.finding}>
        {find.finding ? (
          <TranscriptFindBar inputRef={find.inputRef} query={find.query} onQuery={find.setQuery} status={find.status}
            canStep={find.canStep} onStep={find.step} onClose={find.close} />
        ) : (
          <button ref={find.buttonRef} type="button" className="transcript-find" aria-label="Find in transcript" title="Find in transcript"
            onClick={find.open}>
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
        found={find.found}
        findCurrent={find.current}
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
