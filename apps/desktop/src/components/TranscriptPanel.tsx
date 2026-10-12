import { useCallback, useEffect, useId, useLayoutEffect, useMemo, useRef, useState, type Dispatch, type ReactNode, type SetStateAction } from "react";

import type { NoteRecording, TranscriptRow } from "../lib/ipc";
import { extendSelection, lineStop, navigableLines, NO_SELECTION, stepLine, type LineSelection, type TranscriptSection } from "../lib/transcriptLayout";
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

type TranscriptPanelProps = {
  readOnly?: boolean;
  /**
   * A row to show: scrolled to, once. The owner has already selected it
   * (`NoteWorkspace`'s `openRow`); the panel only brings it into view.
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
  playInterval: (start: number, end: number | null) => void;
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
};

export function TranscriptPanel({
  readOnly = false, sourceRequest, onSourceShown, rows, sections, recording, currentTime, playing, visible, playInterval, time,
  selection, onSelectionChange, suggestions, onRename,
}: TranscriptPanelProps) {
  const idPrefix = useId();
  const rowRefs = useRef(new Map<number, HTMLLIElement>());
  // The playing row's words, when it is drawn word by word.
  const wordsRef = useRef<HTMLSpanElement>(null);
  const [renaming, setRenaming] = useState<RenameTarget | null>(null);
  // Sections the reader closed, by key; every section starts open (KNEG-448).
  const [collapsed, setCollapsed] = useState<ReadonlySet<string>>(() => new Set());
  // The line that last took focus, which holds the lines' one tab stop until a summary source or
  // a search hit selects another (`lineStop`).
  const [cursor, setCursor] = useState<number | null>(null);
  // The lines the arrow keys move between: the timed lines of the open sections.
  const lines = useMemo(() => navigableLines(sections, collapsed), [sections, collapsed]);

  // What the lines' handlers read, so the handlers -- and the one `actions` object holding them --
  // stay the same from render to render and never redraw a memoised line by themselves.
  const canPlay = recording !== null;
  const latest = useRef({ rows, lines, selection, canPlay, playInterval, onSelectionChange, onRename });
  useLayoutEffect(() => { latest.current = { rows, lines, selection, canPlay, playInterval, onSelectionChange, onRename }; });
  const actions = useMemo<TranscriptActions>(() => ({
    register: (index, element) => {
      if (element === null) rowRefs.current.delete(index);
      else rowRefs.current.set(index, element);
    },
    wordsRef,
    choose: (row, extend) => {
      const { rows: all, canPlay: hasRecording, playInterval: play, onSelectionChange: select } = latest.current;
      if (row.startSeconds === null) return;
      if (extend) {
        select((previous) => extendSelection(all, previous, row.index));
        return;
      }
      select({ rows: new Set([row.index]), anchor: row.index });
      // To the end: a line is where to start listening, not a clip (the owner's ruling, 2026-10-11).
      if (hasRecording) play(row.startSeconds, null);
    },
    playWord: (row, seconds) => {
      const { canPlay: hasRecording, playInterval: play, onSelectionChange: select } = latest.current;
      select({ rows: new Set([row.index]), anchor: row.index });
      if (hasRecording) play(seconds, null);
    },
    move: (row, key, extend) => {
      const { rows: all, lines: open, onSelectionChange: select } = latest.current;
      const target = stepLine(open, row.index, key);
      if (target === null) return;
      // Shift with nothing selected extends from the line focus is leaving.
      if (extend) select((previous) => extendSelection(all, previous.anchor === null ? { rows: previous.rows, anchor: row.index } : previous, target));
      const element = rowRefs.current.get(target);
      if (!element) return;
      // Scrolled by the lines' scroll margins (`styles.css`), clear of the sticky head and the card.
      element.focus({ preventScroll: true });
      element.scrollIntoView({ block: "nearest" });
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
  }), []);
  const toggleSection = useCallback((key: string) => setCollapsed((previous) => {
    const next = new Set(previous);
    if (next.has(key)) next.delete(key);
    else next.add(key);
    return next;
  }), []);

  // The owner selected the row (`NoteWorkspace`'s `openRow`); here it is only brought into view,
  // and it takes the tab stop from whichever line had focus before.
  useEffect(() => {
    if (!sourceRequest || !visible) return;
    if (!rows.some((item) => item.index === sourceRequest.index)) return;
    setCursor(null);
    rowRefs.current.get(sourceRequest.index)?.scrollIntoView({ block: "nearest", behavior: scrollBehavior() });
    onSourceShown?.();
  }, [sourceRequest, rows, visible, onSourceShown]);

  const activeRow = useMemo(() => {
    if (!playing) return null;
    return rows.find(
      (row) =>
        row.startSeconds !== null &&
        row.startSeconds <= currentTime &&
        (row.endSeconds === null || currentTime < row.endSeconds),
    ) ?? null;
  }, [currentTime, playing, rows]);

  // Lines and the rename form carry scroll margins (`styles.css`), so this and the source scroll
  // above stop clear of the sticky header and of the player card floating over the column's foot.
  useEffect(() => {
    if (visible && activeRow !== null) {
      rowRefs.current.get(activeRow.index)?.scrollIntoView({ block: "nearest" });
    }
  }, [activeRow, visible]);

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

  // Where each line and turn sits, so only the section and turn holding the playing line, the tab
  // stop or the open rename are handed it.
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
  const activeIndex = activeRow?.index ?? null;
  const activePlace = activeIndex === null ? undefined : places.rowsAt.get(activeIndex);
  // The lines' one tab stop: the line focus was last on, else the selection's anchor, else the
  // playing line, else the first -- whichever is first on a timed line in an open section.
  const stopIndex = useMemo(() => lineStop(lines, [cursor, selection.anchor, activeIndex]), [lines, cursor, selection.anchor, activeIndex]);
  const stopPlace = stopIndex === null ? undefined : places.rowsAt.get(stopIndex);

  return (
    <aside className="transcript" aria-label="Transcript">
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
