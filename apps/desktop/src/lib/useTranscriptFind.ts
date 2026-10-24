import { useCallback, useDeferredValue, useEffect, useLayoutEffect, useMemo, useRef, useState, type RefObject } from "react";

import type { TranscriptRow } from "./ipc";
import { findMatches, findStatus, firstMatchWhere, matchFrom, normaliseQuery, regroupMatches, rowOrder, sameMatches, stepMatch, type FindMatch } from "./transcriptFind";
import { useLatest } from "./useLatest";
import { visibleBand, type RevealLine } from "./useReveal";

/** What Find has while it is closed or finds nothing: one list and one map, so nothing redraws for a new empty one. */
const NO_MATCHES: readonly FindMatch[] = [];
const NOTHING_FOUND: ReadonlyMap<number, readonly FindMatch[]> = new Map();

/** The Find match gone to last (KNEG-448): its ordinal in `matches`, the list of the query it was found for. */
type FindPosition = { matches: readonly FindMatch[]; ordinal: number };

type TranscriptFindOptions = {
  rows: TranscriptRow[];
  visible: boolean;
  playing: boolean;
  follow: boolean;
  onReleaseFollow: () => void;
  asideRef: RefObject<HTMLElement | null>;
  /** The lines' elements by row index (`useReveal`'s `rowRefs`). */
  rowRefs: RefObject<Map<number, HTMLLIElement>>;
  reveal: RevealLine;
};

/**
 * Find in this transcript (KNEG-448): the head's search button opens a bar in its label's place.
 * The query is searched a moment after it is typed (`useDeferredValue`), so typing stays quick in
 * a long transcript, and the lines are drawn with what it found, row by row. Going to a match
 * brings its line to the middle of the band, its section opened first; nothing is selected or
 * played, and focus stays in the field.
 */
export function useTranscriptFind({ rows, visible, playing, follow, onReleaseFollow, asideRef, rowRefs, reveal }: TranscriptFindOptions) {
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
  const inputRef = useRef<HTMLInputElement>(null);
  const buttonRef = useRef<HTMLButtonElement>(null);
  // Set as Find closes, so the Find button takes focus back once it is drawn again.
  const refocusButton = useRef(false);
  const latest = useLatest({ playing, follow, onReleaseFollow });

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
  }, [latest, reveal]);

  // A new query goes to its first match at or after where the reader is: the match that was
  // current, when there was one, so a letter more or less keeps to the same place; otherwise the
  // line at the top of the visible band (the first match whose line reaches below the band's top,
  // or whose section's header does while the section is closed), and the first match when none is
  // that far on. A letter added that still finds the same match moves nothing. The line jumps
  // there rather than gliding: a key typed while the glide runs stops it where it is -- in headless
  // Edge, typing "möte" left the glide to its first match 10px from where it began. No app code
  // scrolls then (every scroll and focus call logged, 2026-10-24), and it is not the field's caret
  // being brought into view alone: a glide set off by script, outside any key, went on through a
  // typed key. What in the browser stops it is not traced. `useReveal` finishes a glide stopped
  // short, so a step's glide (Enter, Previous, Next) reaches its line all the same (KNEG-455 item 6).
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
  const step = (direction: 1 | -1) => {
    const next = stepMatch(current, matches.length, direction);
    if (next !== null) goToMatch(matches, next);
  };
  const open = () => setFinding(true);
  const close = () => {
    refocusButton.current = true;
    setFinding(false);
    setQuery("");
    setPosition(null);
  };
  // The field takes focus as the bar opens; the Find button takes it back as the bar closes.
  useLayoutEffect(() => {
    if (finding) {
      inputRef.current?.focus({ preventScroll: true });
    } else if (refocusButton.current) {
      refocusButton.current = false;
      buttonRef.current?.focus({ preventScroll: true });
    }
  }, [finding]);
  // Ctrl+F or Cmd+F, anywhere in the note workspace while the transcript is on screen, but in an
  // editable field or a dialog, which keep their own; inside Find it selects the query.
  useEffect(() => {
    if (!visible) return;
    const workspace = asideRef.current?.closest(".note-detail");
    if (!workspace) return;
    const findShortcut = (event: Event) => {
      const key = event as KeyboardEvent;
      if (key.defaultPrevented || key.isComposing || key.altKey || key.shiftKey || !(key.ctrlKey || key.metaKey) || key.key.toLowerCase() !== "f") return;
      const target = key.target instanceof Element ? key.target : null;
      if (target?.closest("dialog, [role=dialog]")) return;
      if (target !== inputRef.current && target?.closest("input, textarea, select, [contenteditable=true], [role=textbox]")) return;
      key.preventDefault();
      if (finding) { inputRef.current?.focus(); inputRef.current?.select(); }
      else setFinding(true);
    };
    workspace.addEventListener("keydown", findShortcut);
    return () => workspace.removeEventListener("keydown", findShortcut);
  }, [asideRef, visible, finding]);
  // Nothing while there is no query; "No matches" while it finds nothing.
  const status = normaliseQuery(searched) === "" ? null : findStatus(current, matches.length);

  return {
    finding, open, close, step, query, setQuery, status,
    canStep: matches.length > 0,
    /** The matches by row, for the lines to draw. */
    found,
    /** The current match's ordinal in this query's matches, and the row it is in. */
    current,
    currentRow: currentMatch?.row ?? null,
    inputRef, buttonRef,
  };
}
