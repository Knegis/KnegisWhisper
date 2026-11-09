import { getKnownVoicesStatus } from "../lib/ipc";
import type { KnownVoicesStatus } from "../generated/ipc-types";
import { Fragment, memo, useEffect, useId, useRef, useState, type FormEvent, type RefObject } from "react";

import { scrollBehavior } from "../lib/followScroll";
import type { TranscriptRow } from "../lib/ipc";
import { canSaveSpeakerName, matchingSuggestions, normaliseSpeakerName, renameHeading, speakerIsNamed } from "../lib/speakerNames";
import { sameMatchesOver, textParts, type FindMatch } from "../lib/transcriptFind";
import { sameProps, sameSelectionOver, type LineKey, type TranscriptSection, type TranscriptTurn } from "../lib/transcriptLayout";
import { message } from "../lib/useRefresh";
import { wordsMatchText } from "../lib/wordTiming";
import { Icon } from "./Icon";
import { formatClock } from "./RecordingPlayer";

/**
 * A note's transcript as the M6 canvas draws it (KNEG-448): time sections, each a
 * collapsible header over its speaker turns, each turn a head -- a round marker,
 * who, and when -- over its lines at the column's full width. `TranscriptPanel`
 * owns the state, the header and the effects; this file only draws.
 *
 * A transcript can run to well over a thousand lines, and playback renders the
 * panel four times a second. So `TranscriptSections`, `Section`, `Turn` and
 * `Line` are memoised, the panel hands `activeIndex` and the tab stop only to
 * the section and turn that hold them (null to the rest), and every handler
 * arrives in one `TranscriptActions` object the panel makes once. A
 * `timeupdate` that does not move the playing line redraws only the panel
 * around this; one that does redraws the two sections, two turns and two lines
 * it moved between. The selection is the note's one set of row indexes, so a
 * section or a turn compares it by its own lines only (`sameSelectionOver`): a
 * click redraws the turns whose lines it selected or deselected, not the rest.
 * Find's matches are the same (`sameMatchesOver`): one map of each row's
 * matches, whose arrays stay the same while a row's matches do
 * (`regroupMatches`), so a new query redraws only the lines it changed, and a
 * step between matches only the two lines it moved between.
 */

/** A speaker being renamed, and the turn whose name was pressed: the form opens under that turn. */
export type RenameTarget = { turnKey: string; speakerId: string; current: string };

/**
 * What a line or a turn can ask of the panel. Made once by `TranscriptPanel` from
 * callbacks that read its latest props and state through refs, so the object is
 * the same on every render and never redraws a memoised line by itself.
 */
export type TranscriptActions = {
  knownVoices?: (speakerId: string) => Promise<KnownVoicesStatus>;
  /** Keeps the panel's map from row index to its line, which its scrolls go through. */
  register: (index: number, element: HTMLLIElement | null) => void;
  /** The playing line's `.transcript-text`, whose words the panel lights frame by frame (KNEG-408). */
  wordsRef: RefObject<HTMLSpanElement | null>;
  /**
   * A click, Enter or Space on a line. With `extend` (Shift held), the lines from the anchor to it
   * are selected and nothing plays; otherwise it alone is selected, and plays from its start to
   * the end, with Follow on, when the note has a recording.
   */
  choose: (row: TranscriptRow, extend: boolean) => void;
  /** A click, Enter or Space on a word: its line alone selected, and played from the word with Follow on. */
  playWord: (row: TranscriptRow, seconds: number) => void;
  /**
   * An arrow key, Home or End on a line: focus moves to another line, brought into view, and
   * Follow lets go; with `extend`, the selection goes with it.
   */
  move: (row: TranscriptRow, key: LineKey, extend: boolean) => void;
  /** Escape on a line: the selection cleared. False when there was none, so the key goes on. */
  clear: () => boolean;
  /** A line, or a word in it, took focus, and so holds the lines' one tab stop. */
  focused: (index: number) => void;
  openRename: (target: RenameTarget) => void;
  renameLine: (index: number) => void;
  closeRename: (speakerId: string) => void;
  rename: (speakerId: string, name: string, remember?: boolean, replaceId?: number | null) => Promise<void>;
};

export function RenameForm({ target, suggestions, onRename, onClose, loadStatus }: {
  loadStatus?: TranscriptActions["knownVoices"];
  target: RenameTarget;
  suggestions: readonly string[];
  onRename: TranscriptActions["rename"];
  onClose: (speakerId: string) => void;
}) {
  const [known, setKnown] = useState<KnownVoicesStatus | null>(null);
  const [remember, setRemember] = useState(false);
  const [replaceId, setReplaceId] = useState<number | null>(null);
  useEffect(() => { let active = true; void (loadStatus ? loadStatus(target.speakerId) : getKnownVoicesStatus()).then(v => { if(active) setKnown(v); }).catch(() => {}); return () => { active = false; }; }, [loadStatus, target.speakerId]);
  const named = speakerIsNamed(target.current, target.speakerId);
  // A name is there to edit. "Speaker 1" isn't one: it would only have to be cleared first.
  const [value, setValue] = useState(named ? target.current : "");
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const fieldId = useId();
  const formRef = useRef<HTMLFormElement>(null);
  const fieldRef = useRef<HTMLInputElement>(null);
  const heading = renameHeading(target.current);
  const typed = normaliseSpeakerName(value).toLocaleLowerCase();
  const shown = matchingSuggestions(target.current, value, suggestions);

  // Opened under a turn near the foot of the column, the form would be behind the player card:
  // brought into view, its scroll margins (`styles.css`) clear the card and the sticky header.
  useEffect(() => {
    formRef.current?.scrollIntoView({ block: "nearest", behavior: scrollBehavior() });
  }, []);

  const submitName = async (name: string) => {
    if (saving) return;
    setSaving(true);
    setError(null);
    try {
      await onRename(target.speakerId, name, remember && !!name.trim(), replaceId);
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

  // Escape on the form, not the field: it closes from a suggestion or a button too.
  return <form ref={formRef} className="transcript-rename" aria-label={heading} onSubmit={onSubmit} onKeyDown={event => {
    if (event.key === "Escape") { event.preventDefault(); onClose(target.speakerId); }
  }}>
    <label className="section-label" htmlFor={fieldId}>name this speaker</label>
    <input id={fieldId} ref={fieldRef} type="text" autoFocus autoComplete="off" maxLength={100} placeholder={named ? undefined : target.current}
      value={value} onChange={event => setValue(event.target.value)} />
    {shown.length > 0 ? <div className="transcript-rename-suggestions" role="group" aria-label="Suggested names">
      {/* Fills the field and hands focus back to it, so Enter saves; nothing is saved by the press. */}
      {shown.map(name => <button key={name} type="button" className="language-pill" title={name} disabled={saving}
        aria-pressed={name.toLocaleLowerCase() === typed} onClick={() => { setValue(name); fieldRef.current?.focus(); }}>{name}</button>)}
    </div> : null}
    {known?.enabled && known.ready ? <div>
      <label><input type="checkbox" checked={remember} disabled={saving || !!known.enrollmentReason} onChange={e => setRemember(e.target.checked)} />Remember this voice on this computer</label>
      {known.enrollmentReason ? <p>{known.enrollmentReason}</p> : null}
      {remember ? <label>Voice profile <select disabled={saving} value={replaceId ?? "new"} onChange={e => setReplaceId(e.target.value === "new" ? null : Number(e.target.value))}>
        <option value="new">Create a new voice</option>
        {known.voices.map(v => <option key={v.id} value={v.id}>Replace sample for {v.displayName} (#{v.id})</option>)}
      </select></label> : null}
    </div> : null}
    <div className="transcript-rename-actions">
      {named ? <button type="button" disabled={saving} onClick={() => { void submitName(""); }}>Remove name</button> : null}
      <button type="button" disabled={saving} onClick={() => onClose(target.speakerId)}>Cancel</button>
      <button type="submit" className="primary" disabled={saving || (!canSaveSpeakerName(value, target) && !(remember && value.trim()))}>Save</button>
    </div>
    {error && <p className="error" role="alert">{error}</p>}
  </form>;
}

type LineProps = {
  row: TranscriptRow;
  readOnly: boolean;
  selected: boolean;
  active: boolean;
  /** Whether this line holds the lines' one tab stop (`lineStop`); its words then hold one too. */
  stop: boolean;
  /** Whether the note has a recording to play: a timed line then plays when pressed, and its words when it has them. */
  canPlay: boolean;
  /** Find's matches in this line, while Find is open and finds any here (KNEG-448); the same array while they are the same. */
  found: readonly FindMatch[] | undefined;
  /** The current Find match's ordinal when it is in this line; null otherwise. */
  currentMatch: number | null;
  actions: TranscriptActions;
};

/** The keys that move between lines; inside a line's words, Home and End move between words instead. */
const LINE_KEYS: ReadonlySet<string> = new Set<LineKey>(["ArrowUp", "ArrowDown", "Home", "End"]);
const WORD_KEYS: ReadonlySet<string> = new Set(["ArrowLeft", "ArrowRight", "Home", "End"]);

/**
 * Whether the page's text selection reaches into `element`: text dragged over in it, or a word in
 * it double-clicked. The click that ends either is someone copying a quote, not asking to listen.
 */
function selectsText(element: Element): boolean {
  const selection = window.getSelection();
  if (selection === null || selection.isCollapsed) return false;
  for (let at = 0; at < selection.rangeCount; at += 1) {
    if (selection.getRangeAt(at).intersectsNode(element)) return true;
  }
  return false;
}

/**
 * One line: an option of its turn's listbox (KNEG-448). A click, Enter or Space selects it alone and
 * plays from its start to the end; with Shift, the lines from the anchor to it are selected and
 * nothing plays. Pressed again, it plays from its start again rather than deselecting: Escape on a
 * line clears the selection, as the card's Clear does while two or more lines are selected. A
 * click that ends a text selection in the line -- a drag over its text, or the second click of a
 * double-click on a word -- does nothing, so a quote can be copied without starting playback. The
 * lines are one tab stop between them, plus one of its words when the line holding it is drawn
 * word by word, and the arrow keys, Home and End move from line to line.
 * An untimed line is a disabled option that is never focused, selected or played. While Find
 * has matches in it, the line draws its text with them marked, in place of its words.
 */
export const Line = memo(function Line({ row, readOnly, selected, active, stop, canPlay, found, currentMatch, actions }: LineProps) {
  const timed = row.startSeconds !== null;
  const navigable = timed || row.speakerId !== null;
  // Which word takes Tab while this line holds the stop: the one the arrow keys between words
  // last left focus on, the first until then.
  const [wordStop, setWordStop] = useState(0);
  const className = [
    "transcript-line",
    timed ? null : "transcript-row-muted",
    canPlay && timed ? "transcript-line-playable" : null,
    selected ? "transcript-line-selected" : null,
    active ? "transcript-line-playing" : null,
    // Unstyled; the visual checks find the selected and the playing line by it.
    selected || active ? "transcript-row-current" : null,
  ].filter(Boolean).join(" ");
  return (
    <li
      ref={(element) => { actions.register(row.index, element); }}
      className={className}
      role="option"
      tabIndex={navigable ? (stop ? 0 : -1) : undefined}
      aria-keyshortcuts={!readOnly && row.speakerId !== null ? "F2" : undefined}
      aria-description={!readOnly && row.speakerId !== null ? "Press F2 to rename this speaker." : undefined}
      aria-selected={timed ? selected : false}
      aria-disabled={navigable ? undefined : true}
      aria-current={active ? "true" : undefined}
      onFocus={() => { if (navigable) actions.focused(row.index); }}
      // A Shift-press would also drag a text selection over every line up to this one.
      onMouseDown={(event) => { if (timed && event.shiftKey) event.preventDefault(); }}
      onClick={(event) => {
        if (!timed) return;
        // A Shift-click plays nothing, so it may extend the selection whatever text is selected.
        if (!event.shiftKey && selectsText(event.currentTarget)) return;
        // Focus, which the Shift-press above kept from moving.
        if (event.shiftKey) event.currentTarget.focus({ preventScroll: true });
        actions.choose(row, event.shiftKey);
      }}
      onKeyDown={(event) => {
        if (!navigable) return;
        if (event.key === "F2" && !event.altKey && !event.ctrlKey && !event.metaKey) {
          event.preventDefault(); event.stopPropagation();
          if (!readOnly) actions.renameLine(row.index);
        } else if (timed && (event.key === "Enter" || event.key === " ")) {
          event.preventDefault();
          actions.choose(row, event.shiftKey);
        } else if (LINE_KEYS.has(event.key)) {
          event.preventDefault();
          actions.move(row, event.key as LineKey, event.shiftKey);
        } else if (event.key === "Escape" && actions.clear()) {
          event.preventDefault();
          event.stopPropagation();
        }
      }}
    >
      {found !== undefined && found.length > 0 ? (
        // Find's matches, marked in the line's text (KNEG-448): the current one filled, the rest
        // washed and underlined. Its words are not drawn while it has matches, so the panel lights
        // none of them (`activeWords`), and draws them again once Find closes or stops matching here.
        <span className="transcript-text">
          {textParts(row.text, found).map((part, at) => part.ordinal === null ? <Fragment key={at}>{part.text}</Fragment> : (
            <mark key={at} className={part.ordinal === currentMatch ? "transcript-match transcript-match-current" : "transcript-match"}>{part.text}</mark>
          ))}
        </span>
      ) : canPlay && wordsMatchText(row.words, row.text) && row.words.length > 0 ? (
        // Word by word while it plays. A click on a word selects its line and plays from the word;
        // with Shift it is a Shift-click on the line.
        <span className="transcript-text" ref={active ? actions.wordsRef : undefined}>
          {row.words.map((word, at) => (
            <Fragment key={at}>
              {at > 0 ? " " : null}
              <span className="transcript-word" role="button" aria-label={`Play from ${word.text}`} tabIndex={stop && at === wordStop ? 0 : -1}
                onFocus={() => setWordStop(at)}
                onKeyDown={event => {
                  if ((event.key === "Enter" || event.key === " ") && !event.shiftKey) {
                    event.preventDefault(); event.stopPropagation();
                    actions.playWord(row, word.start);
                  } else if (WORD_KEYS.has(event.key)) {
                    event.preventDefault(); event.stopPropagation();
                    const siblings = [...(event.currentTarget.closest(".transcript-text")?.querySelectorAll<HTMLElement>(".transcript-word") ?? [])];
                    const next = event.key === "Home" ? 0 : event.key === "End" ? siblings.length - 1 : Math.max(0, Math.min(siblings.length - 1, at + (event.key === "ArrowRight" ? 1 : -1)));
                    siblings[next]?.focus();
                  }
                }}
                onClick={(event) => {
                  if (event.shiftKey) return;
                  event.stopPropagation();
                  if (selectsText(event.currentTarget)) return;
                  actions.playWord(row, word.start);
                }}>{word.text}</span>
            </Fragment>
          ))}
        </span>
      ) : (
        <span className="transcript-text">{row.text}</span>
      )}
    </li>
  );
});

/**
 * Whether a turn or a section would draw the same: every prop either side has identical
 * (`sameProps`) but `selected`, the note's whole selection, which only has to select the same of
 * `turns`' lines, the ones being drawn, and `found`, Find's matches in every row, which only has
 * to give those lines the same matches.
 */
function sameDrawing<P extends Pick<Shared, "selected" | "found">>(previous: P, next: P, turns: readonly TranscriptTurn[]): boolean {
  return sameProps(previous, next, ["selected", "found"]) && turns.every((turn) => sameSelectionOver(turn.rows, previous.selected, next.selected)
    && sameMatchesOver(turn.rows, previous.found, next.found));
}

/** What every turn is drawn with, whichever section it is in. */
type Shared = {
  idPrefix: string;
  /** The note's selected rows, all of them: a turn or section compares only its own (`sameDrawing`). */
  selected: ReadonlySet<number>;
  /**
   * Find's matches by row, every row that has any (`regroupMatches`; empty while Find is closed):
   * a turn or a section compares only its own lines' (`sameDrawing`).
   */
  found: ReadonlyMap<number, readonly FindMatch[]>;
  readOnly: boolean;
  canPlay: boolean;
  suggestions: readonly string[];
  actions: TranscriptActions;
};

type TurnProps = Shared & {
  turn: TranscriptTurn;
  /** The playing line's index when it is in this turn; null otherwise. */
  activeIndex: number | null;
  /** The index of the line holding the tab stop when it is in this turn; null otherwise. */
  stopIndex: number | null;
  /** The rename when its name was pressed in this turn; null otherwise. */
  renaming: RenameTarget | null;
  /** The current Find match's ordinal when it is in one of this turn's lines; null otherwise. */
  findCurrent: number | null;
};

/** Whether `found`, a line's matches, holds the match numbered `ordinal`. */
function holds(found: readonly FindMatch[] | undefined, ordinal: number | null): boolean {
  return ordinal !== null && found !== undefined && found.some((match) => match.ordinal === ordinal);
}

/**
 * One speaker's run of lines: a 6px dot (`--band` while one of them plays), the name -- the
 * button that renames that speaker, or plain text when there is no label to rename -- and when
 * the turn starts. A turn with no speaker shows only the dot and the time. The rename form
 * opens between the head and the lines. The lines are a listbox named by who and when.
 */
export const Turn = memo(function Turn({ turn, activeIndex, stopIndex, renaming, findCurrent, idPrefix, selected, found, readOnly, canPlay, suggestions, actions }: TurnProps) {
  const nameId = `${idPrefix}-${turn.key}-name`;
  const timeId = `${idPrefix}-${turn.key}-time`;
  const speakerId = turn.speakerId;
  const name = turn.speaker ?? speakerId;
  return (
    <div className={activeIndex === null ? "transcript-turn" : "transcript-turn transcript-turn-playing"}>
      <div className="transcript-turn-head">
        <span className="transcript-turn-marker" aria-hidden="true" />
        {speakerId !== null ? (
          <button type="button" tabIndex={-1} className="transcript-speaker" disabled={readOnly} aria-label={`Rename ${name ?? speakerId}`}
            onClick={() => actions.openRename({ turnKey: turn.key, speakerId, current: name ?? speakerId })}>
            <span id={nameId}>{name}</span>
          </button>
        ) : name !== null ? <span className="transcript-speaker" id={nameId}>{name}</span> : null}
        <span className="transcript-clock" id={timeId}>{turn.startSeconds !== null ? formatClock(turn.startSeconds) : "—"}</span>
      </div>
      {!readOnly && renaming ? (
        // Changing the target must re-seed the field and clear any error from the previous speaker.
        <RenameForm key={renaming.speakerId} target={renaming} suggestions={suggestions} onRename={actions.rename} onClose={actions.closeRename} loadStatus={actions.knownVoices} />
      ) : null}
      <ol className="transcript-rows" role="listbox" aria-multiselectable="true" aria-labelledby={name !== null ? `${nameId} ${timeId}` : timeId}>
        {turn.rows.map((row) => {
          const matches = found.get(row.index);
          return <Line key={row.index} row={row} readOnly={readOnly} selected={selected.has(row.index)} active={activeIndex === row.index}
            stop={stopIndex === row.index} canPlay={canPlay} found={matches} currentMatch={holds(matches, findCurrent) ? findCurrent : null}
            actions={actions} />;
        })}
      </ol>
    </div>
  );
}, (previous, next) => sameDrawing(previous, next, [next.turn]));

type Placed = {
  activeIndex: number | null;
  activeTurn: string | null;
  stopIndex: number | null;
  stopTurn: string | null;
  renaming: RenameTarget | null;
  /** The current Find match's ordinal, and the turn it is in. */
  findCurrent: number | null;
  findTurn: string | null;
};

/** A section's turns, each told whether it holds the playing line, the tab stop, the open rename or the current match. */
function turnsOf(turns: readonly TranscriptTurn[], { activeIndex, activeTurn, stopIndex, stopTurn, renaming, findCurrent, findTurn }: Placed, shared: Shared) {
  return turns.map((turn) => (
    <Turn key={turn.key} turn={turn} {...shared}
      activeIndex={turn.key === activeTurn ? activeIndex : null}
      stopIndex={turn.key === stopTurn ? stopIndex : null}
      renaming={renaming !== null && renaming.turnKey === turn.key ? renaming : null}
      findCurrent={turn.key === findTurn ? findCurrent : null} />
  ));
}

type SectionProps = Shared & Placed & {
  section: TranscriptSection;
  open: boolean;
  onToggle: (key: string) => void;
};

/**
 * A time section: a 36px header that opens and closes it -- the chevron, the range it covers, in
 * the machine voice and in `--band-text` while it holds the playing line, and how many lines it
 * has -- over its turns. Closed, the turns stay rendered and `hidden`, so the lines keep their
 * place in the document. A group named by its range, not a region: a labelled `<section>` is a
 * landmark, and a two-hour meeting would add thirteen of them to the page's landmarks.
 */
export const Section = memo(function Section({ section, open, onToggle, activeIndex, activeTurn, stopIndex, stopTurn, renaming, findCurrent, findTurn, ...shared }: SectionProps) {
  const bodyId = `${shared.idPrefix}-${section.key}`;
  const titleId = `${bodyId}-title`;
  return (
    <section className={activeIndex === null ? "transcript-section" : "transcript-section transcript-section-playing"} role="group" aria-labelledby={titleId}>
      <h3 className="transcript-section-head">
        <button type="button" className="transcript-section-toggle" aria-expanded={open} aria-controls={bodyId} onClick={(event) => {
          // WKWebView does not focus a pressed button: focus left on a line this closes would be
          // left on a hidden line. Taken here, it stays on the toggle (KNEG-448).
          event.currentTarget.focus();
          onToggle(section.key);
        }}>
          <Icon name={open ? "chevronDown" : "chevronRight"} size={12} />
          <span className="transcript-section-title" id={titleId}>{section.title}</span>
          <span className="transcript-section-count">{section.lineCount} {section.lineCount === 1 ? "line" : "lines"}</span>
        </button>
      </h3>
      <div id={bodyId} className="transcript-section-body" hidden={!open}>
        {turnsOf(section.turns, { activeIndex, activeTurn, stopIndex, stopTurn, renaming, findCurrent, findTurn }, shared)}
      </div>
    </section>
  );
}, (previous, next) => sameDrawing(previous, next, next.section.turns));

type SectionsProps = Shared & {
  sections: readonly TranscriptSection[];
  collapsed: ReadonlySet<string>;
  onToggle: (key: string) => void;
  activeIndex: number | null;
  activeSection: string | null;
  activeTurn: string | null;
  stopIndex: number | null;
  stopSection: string | null;
  stopTurn: string | null;
  renaming: RenameTarget | null;
  renamingSection: string | null;
  /** The current Find match's ordinal, and the section and turn it is in; null while there is none. */
  findCurrent: number | null;
  findSection: string | null;
  findTurn: string | null;
};

/**
 * The sections, or -- when the whole transcript is one section -- its turns alone, with no
 * header over them.
 */
export const TranscriptSections = memo(function TranscriptSections({
  sections, collapsed, onToggle, activeIndex, activeSection, activeTurn, stopIndex, stopSection, stopTurn, renaming, renamingSection,
  findCurrent, findSection, findTurn, ...shared
}: SectionsProps) {
  const only = sections.length === 1 ? sections[0] : undefined;
  if (only !== undefined) return <>{turnsOf(only.turns, { activeIndex, activeTurn, stopIndex, stopTurn, renaming, findCurrent, findTurn }, shared)}</>;
  return <>
    {sections.map((section) => (
      <Section key={section.key} section={section} open={!collapsed.has(section.key)} onToggle={onToggle} {...shared}
        activeIndex={section.key === activeSection ? activeIndex : null}
        activeTurn={section.key === activeSection ? activeTurn : null}
        stopIndex={section.key === stopSection ? stopIndex : null}
        stopTurn={section.key === stopSection ? stopTurn : null}
        renaming={section.key === renamingSection ? renaming : null}
        findCurrent={section.key === findSection ? findCurrent : null}
        findTurn={section.key === findSection ? findTurn : null} />
    ))}
  </>;
});
