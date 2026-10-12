import { useCallback, useEffect, useId, useLayoutEffect, useMemo, useRef, useState, type ReactNode } from "react";

import type { NoteRecording, TranscriptRow } from "../lib/ipc";
import type { TranscriptSection } from "../lib/transcriptLayout";
import { wordAt, wordsMatchText } from "../lib/wordTiming";
import { Icon } from "./Icon";
import { formatClock } from "./RecordingPlayer";
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
  sourceRequest?: { index: number; nonce: number } | null;
  /**
   * Called once `sourceRequest` has been shown (its row selected and scrolled
   * to), so the owner can clear it. A request that stayed set would be
   * applied again by a panel mounted afresh -- a wide window moving the
   * transcript into its column -- or by new rows after a speaker rename,
   * and would select an old summary source nobody just asked for.
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
  /** Go to a clicked word's start (`RecordingPlayerHandle.seek`). */
  seek: (seconds: number) => void;
  suggestions: readonly string[];
  onRename: (speakerId: string, name: string) => Promise<void>;
};

type Selection = { selected: ReadonlySet<number>; anchor: number | null };

/**
 * The click model as it has been: a click on a selected line clears the selection, Shift with an
 * anchor selects every timed line between the two, and anything else selects the one line and
 * makes it the anchor. Untimed lines are never selected.
 */
function nextSelection(previous: Selection, row: TranscriptRow, shift: boolean, rows: readonly TranscriptRow[]): Selection {
  if (!shift && previous.selected.has(row.index)) return { selected: new Set(), anchor: null };
  if (shift && previous.anchor !== null) {
    const from = Math.min(previous.anchor, row.index);
    const to = Math.max(previous.anchor, row.index);
    return {
      selected: new Set(rows.filter((candidate) => candidate.index >= from && candidate.index <= to && candidate.startSeconds !== null).map((candidate) => candidate.index)),
      anchor: previous.anchor,
    };
  }
  return { selected: new Set([row.index]), anchor: row.index };
}

export function TranscriptPanel({ readOnly = false, sourceRequest, onSourceShown, rows, sections, recording, currentTime, playing, visible, playInterval, time, seek, suggestions, onRename }: TranscriptPanelProps) {
  const idPrefix = useId();
  const rowRefs = useRef(new Map<number, HTMLLIElement>());
  // The playing row's words, when it is drawn word by word.
  const wordsRef = useRef<HTMLSpanElement>(null);
  const [selection, setSelection] = useState<Selection>(() => ({ selected: new Set(), anchor: null }));
  const [renaming, setRenaming] = useState<RenameTarget | null>(null);
  // Sections the reader closed, by key; every section starts open (KNEG-448).
  const [collapsed, setCollapsed] = useState<ReadonlySet<string>>(() => new Set());

  // What the lines' handlers read, so the handlers -- and the one `actions` object holding them --
  // stay the same from render to render and never redraw a memoised line by themselves.
  const latest = useRef({ rows, playInterval, seek, onRename });
  useLayoutEffect(() => { latest.current = { rows, playInterval, seek, onRename }; });
  const actions = useMemo<TranscriptActions>(() => ({
    register: (index, element) => {
      if (element === null) rowRefs.current.delete(index);
      else rowRefs.current.set(index, element);
    },
    wordsRef,
    select: (row, shift) => {
      if (row.startSeconds === null) return;
      setSelection((previous) => nextSelection(previous, row, shift, latest.current.rows));
    },
    play: (start, end) => latest.current.playInterval(start, end),
    seek: (seconds) => latest.current.seek(seconds),
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

  useEffect(() => {
    if (!sourceRequest || !visible) return;
    const row = rows.find(item => item.index === sourceRequest.index);
    if (!row) return;
    setSelection({ selected: new Set([row.index]), anchor: row.index });
    rowRefs.current.get(row.index)?.scrollIntoView({ block: "nearest", behavior: scrollBehavior() });
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

  // Where each line and turn sits, so only the section and turn holding the playing line, or the
  // open rename, are handed it.
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

  const playableSelected = useMemo(
    () => rows.filter((row) => selection.selected.has(row.index) && row.startSeconds !== null),
    [rows, selection.selected],
  );
  const selectionStart = playableSelected[0]?.startSeconds ?? null;
  const selectionEnd = playableSelected.at(-1)?.endSeconds ?? null;
  const shownEnd = selectionEnd ?? recording?.durationSeconds ?? selectionStart;

  return (
    <aside className="transcript" aria-label="Transcript">
      <TranscriptHeading>
        {/* Find in the transcript arrives later in KNEG-448; the button holds its place until then. */}
        <button type="button" className="transcript-find" aria-label="Find in transcript" title="Find in transcript" disabled>
          <Icon name="search" size={16} />
        </button>
      </TranscriptHeading>
      {recording && selectionStart !== null && shownEnd !== null ? (
        <button
          type="button"
          onClick={() => playInterval(selectionStart, selectionEnd)}
        >
          Play selection ({formatClock(selectionStart)}–{formatClock(shownEnd)})
        </button>
      ) : null}
      <TranscriptSections
        sections={sections}
        collapsed={collapsed}
        onToggle={toggleSection}
        activeIndex={activeIndex}
        activeSection={activePlace?.section ?? null}
        activeTurn={activePlace?.turn ?? null}
        renaming={renaming}
        renamingSection={renaming === null ? null : places.turnsAt.get(renaming.turnKey) ?? null}
        idPrefix={idPrefix}
        selected={selection.selected}
        readOnly={readOnly}
        canPlay={recording !== null}
        suggestions={suggestions}
        actions={actions}
      />
    </aside>
  );
}
