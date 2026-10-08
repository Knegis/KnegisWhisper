import { Fragment, useEffect, useId, useMemo, useRef, useState, type FormEvent } from "react";

import type { NoteRecording, TranscriptRow } from "../lib/ipc";
import { normaliseSpeakerName, renameHeading, suggestionsFor } from "../lib/speakerNames";
import { message } from "../lib/useRefresh";
import { wordAt, wordsMatchText } from "../lib/wordTiming";
import { formatClock } from "./RecordingPlayer";

const CURRENT_WORD = "transcript-word-current";

/** Scrolls jump rather than glide when the person asked for less motion. */
function scrollBehavior(): ScrollBehavior {
  return typeof matchMedia === "function" && matchMedia("(prefers-reduced-motion: reduce)").matches ? "auto" : "smooth";
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

type RenameTarget = { speakerId: string; current: string };

function RenameForm({ target, suggestions, onRename, onClose }: {
  target: RenameTarget;
  suggestions: readonly string[];
  onRename: TranscriptPanelProps["onRename"];
  onClose: (speakerId: string) => void;
}) {
  const [value, setValue] = useState(target.current);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const datalistId = useId();
  const heading = renameHeading(target.current);

  const submitName = async (name: string) => {
    if (saving) return;
    setSaving(true);
    setError(null);
    try {
      await onRename(target.speakerId, name);
      onClose(target.speakerId);
    } catch (cause) {
      setError(message(cause));
    } finally {
      setSaving(false);
    }
  };

  const onSubmit = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    void submitName(normaliseSpeakerName(value));
  };

  return <form className="transcript-rename" aria-label={heading} onSubmit={onSubmit}>
    <label htmlFor={`${datalistId}-name`}>{heading}</label>
    <input id={`${datalistId}-name`} type="text" list={datalistId} autoFocus maxLength={100}
      value={value} onChange={event => setValue(event.target.value)} onKeyDown={event => {
        if (event.key === "Escape") { event.preventDefault(); onClose(target.speakerId); }
      }} />
    <datalist id={datalistId}>
      {suggestionsFor(target.current, suggestions).map(name => <option key={name} value={name} />)}
    </datalist>
    <button type="submit" disabled={saving || normaliseSpeakerName(value) === normaliseSpeakerName(target.current)}>Save</button>
    <button type="button" disabled={saving} onClick={() => { void submitName(""); }}>Remove name</button>
    <button type="button" disabled={saving} onClick={() => onClose(target.speakerId)}>Cancel</button>
    {error && <p className="error" role="alert">{error}</p>}
  </form>;
}

export function TranscriptPanel({ readOnly = false, sourceRequest, onSourceShown, rows, recording, currentTime, playing, visible, playInterval, time, seek, suggestions, onRename }: TranscriptPanelProps) {
  const rowRefs = useRef(new Map<number, HTMLLIElement>());
  // The playing row's words, when it is drawn word by word.
  const wordsRef = useRef<HTMLSpanElement>(null);
  const [selected, setSelected] = useState<Set<number>>(() => new Set());
  const [anchor, setAnchor] = useState<number | null>(null);
  const [renaming, setRenaming] = useState<RenameTarget | null>(null);
  const closeRename = (speakerId: string) => setRenaming(previous => previous?.speakerId === speakerId ? null : previous);

  useEffect(() => {
    if (!sourceRequest || !visible) return;
    const row = rows.find(item => item.index === sourceRequest.index);
    if (!row) return;
    setSelected(new Set([row.index])); setAnchor(row.index);
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
      {!readOnly && renaming && (
        // Changing the target must re-seed the field and clear any error from the previous speaker.
        <RenameForm key={renaming.speakerId} target={renaming} suggestions={suggestions} onRename={onRename} onClose={closeRename} />
      )}
      <ol
        className="transcript-rows"
        role="listbox"
        aria-multiselectable="true"
      >
        {rows.map((row) => {
          const playable = row.startSeconds !== null;
          const isSelected = selected.has(row.index);
          const isActive = activeRow?.index === row.index;
          const speakerId = row.speakerId;
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
              {/* Who and when on one line, what was said under it. Play sits at the end of the
                  first line, so a row's text starts at the same x whether it can be played or not. */}
              <span className="transcript-row-head">
                {speakerId !== null ? (
                  <button type="button" className="transcript-speaker" disabled={readOnly} aria-label={`Rename ${row.speaker ?? speakerId}`}
                    onClick={event => {
                      // The label sits inside a selectable option; its click must not toggle the row.
                      event.stopPropagation();
                      setRenaming({ speakerId, current: row.speaker ?? speakerId });
                    }}
                    onKeyDown={event => {
                      // The option selects on Enter and Space, so keep those keys on this control.
                      if (event.key === "Enter" || event.key === " ") event.stopPropagation();
                    }}>{row.speaker ?? speakerId}</button>
                ) : row.speaker ? <span className="transcript-speaker">{row.speaker}</span> : null}
                <span className="transcript-clock">
                  {row.startSeconds !== null ? formatClock(row.startSeconds) : "—"}
                </span>
                {recording && playable ? (
                  <button
                    type="button"
                    className="transcript-play"
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
              </span>
              {recording && wordsMatchText(row.words, row.text) && row.words.length > 0 ? (
                // Word by word while it plays. A click on a word goes there, and only there: the
                // row's own click would select or deselect it.
                <span className="transcript-text" ref={isActive ? wordsRef : undefined}>
                  {row.words.map((word, at) => (
                    <Fragment key={at}>
                      {at > 0 ? " " : null}
                      <span className="transcript-word" role="button" aria-label={`Seek to ${word.text}`} tabIndex={at === 0 ? 0 : -1}
                        onFocus={event => { const siblings = event.currentTarget.closest(".transcript-text")?.querySelectorAll<HTMLElement>(".transcript-word"); siblings?.forEach(element => { element.tabIndex = element === event.currentTarget ? 0 : -1; }); }}
                        onKeyDown={event => {
                          if (event.key === "Enter" || event.key === " ") { event.preventDefault(); event.stopPropagation(); seek(word.start); }
                          else if (["ArrowLeft", "ArrowRight", "Home", "End"].includes(event.key)) {
                            event.preventDefault(); event.stopPropagation();
                            const siblings = [...(event.currentTarget.closest(".transcript-text")?.querySelectorAll<HTMLElement>(".transcript-word") ?? [])];
                            const next = event.key === "Home" ? 0 : event.key === "End" ? siblings.length - 1 : Math.max(0, Math.min(siblings.length - 1, at + (event.key === "ArrowRight" ? 1 : -1)));
                            siblings[next]?.focus();
                          }
                        }} onClick={(event) => { event.stopPropagation(); seek(word.start); }}>{word.text}</span>
                    </Fragment>
                  ))}
                </span>
              ) : (
                <span className="transcript-text">{row.text}</span>
              )}
            </li>
          );
        })}
      </ol>
    </aside>
  );
}
