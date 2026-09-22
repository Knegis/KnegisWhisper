import { useEffect, useMemo, useRef, useState } from "react";

import type { NoteRecording, TranscriptRow } from "../lib/ipc";
import { formatClock } from "./RecordingPlayer";

type TranscriptPanelProps = {
  sourceRequest?: { index: number; nonce: number } | null;
  rows: TranscriptRow[];
  recording: NoteRecording | null;
  currentTime: number;
  playing: boolean;
  visible: boolean;
  playInterval: (start: number, end: number | null) => void;
};

export function TranscriptPanel({ sourceRequest, rows, recording, currentTime, playing, visible, playInterval }: TranscriptPanelProps) {
  const rowRefs = useRef(new Map<number, HTMLLIElement>());
  const [selected, setSelected] = useState<Set<number>>(() => new Set());
  const [anchor, setAnchor] = useState<number | null>(null);

  useEffect(() => {
    if (!sourceRequest || !visible) return;
    const row = rows.find(item => item.index === sourceRequest.index);
    if (!row) return;
    setSelected(new Set([row.index])); setAnchor(row.index);
    rowRefs.current.get(row.index)?.scrollIntoView({ block: "nearest", behavior: "smooth" });
  }, [sourceRequest, rows, visible]);

  const activeRow = useMemo(() => {
    if (!playing) return null;
    return rows.find(
      (row) =>
        row.startSeconds !== null &&
        row.startSeconds <= currentTime &&
        (row.endSeconds === null || currentTime < row.endSeconds),
    ) ?? null;
  }, [currentTime, playing, rows]);

  useEffect(() => {
    if (visible && activeRow !== null) {
      rowRefs.current.get(activeRow.index)?.scrollIntoView({ block: "nearest" });
    }
  }, [activeRow, visible]);

  const playableSelected = rows.filter((row) => selected.has(row.index) && row.startSeconds !== null);
  const selectionStart = playableSelected[0]?.startSeconds ?? null;
  const selectionEnd = playableSelected.at(-1)?.endSeconds ?? null;
  const shownEnd = selectionEnd ?? recording?.durationSeconds ?? selectionStart;

  const selectRow = (row: TranscriptRow, shift: boolean) => {
    const start = row.startSeconds;
    if (start === null) return;
    if (!shift && selected.has(row.index)) {
      setSelected(new Set());
      setAnchor(null);
      return;
    }
    if (shift && anchor !== null) {
      const from = Math.min(anchor, row.index);
      const to = Math.max(anchor, row.index);
      setSelected(
        new Set(
          rows
            .filter(
              (candidate) =>
                candidate.index >= from &&
                candidate.index <= to &&
                candidate.startSeconds !== null,
            )
            .map((candidate) => candidate.index),
        ),
      );
      return;
    }
    setSelected(new Set([row.index]));
    setAnchor(row.index);
  };

  return (
    <aside className="transcript" aria-label="Transcript">
      {recording && selectionStart !== null && shownEnd !== null ? (
        <button
          type="button"
          onClick={() => playInterval(selectionStart, selectionEnd)}
        >
          Play selection ({formatClock(selectionStart)}–{formatClock(shownEnd)})
        </button>
      ) : null}
      <ol
        className="transcript-rows"
        role="listbox"
        aria-multiselectable="true"
      >
        {rows.map((row) => {
          const playable = row.startSeconds !== null;
          const isSelected = selected.has(row.index);
          const isActive = activeRow?.index === row.index;
          return (
            <li
              key={row.index}
              ref={(element) => {
                if (element === null) rowRefs.current.delete(row.index);
                else rowRefs.current.set(row.index, element);
              }}
              className={`${playable ? "" : "transcript-row-muted"} ${
                isSelected || isActive ? "transcript-row-current" : ""
              }`}
              tabIndex={playable ? 0 : undefined}
              role={playable ? "option" : undefined}
              aria-selected={playable ? isSelected : undefined}
              aria-current={isActive ? "true" : undefined}
              onClick={(event) => selectRow(row, event.shiftKey)}
              onKeyDown={(event) => {
                if (!playable || (event.key !== "Enter" && event.key !== " ")) return;
                event.preventDefault();
                selectRow(row, event.shiftKey);
              }}
            >
              {recording && playable ? (
                <button
                  type="button"
                  aria-label={`Play transcript row ${row.index + 1}`}
                  onClick={(event) => {
                    event.stopPropagation();
                    if (row.startSeconds !== null) {
                      playInterval(row.startSeconds, row.endSeconds);
                    }
                  }}
                >
                  Play
                </button>
              ) : null}
              <span className="transcript-clock">
                {row.startSeconds !== null ? formatClock(row.startSeconds) : "—"}
              </span>
              {row.speaker ? <span className="transcript-speaker">{row.speaker}</span> : null}
              <span>{row.text}</span>
            </li>
          );
        })}
      </ol>
    </aside>
  );
}
