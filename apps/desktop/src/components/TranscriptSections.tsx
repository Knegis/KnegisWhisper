import { Fragment, memo, useEffect, useId, useRef, useState, type FormEvent, type RefObject } from "react";

import type { TranscriptRow } from "../lib/ipc";
import { normaliseSpeakerName, renameHeading, suggestionsFor } from "../lib/speakerNames";
import type { TranscriptSection, TranscriptTurn } from "../lib/transcriptLayout";
import { message } from "../lib/useRefresh";
import { wordsMatchText } from "../lib/wordTiming";
import { Icon } from "./Icon";
import { formatClock } from "./RecordingPlayer";

/**
 * A note's transcript as the M6 canvas draws it (KNEG-448): time sections, each a
 * collapsible header over its speaker turns, each turn a head -- a square marker,
 * who, and when -- over its lines at the column's full width. `TranscriptPanel`
 * owns the state, the header and the effects; this file only draws.
 *
 * A transcript can run to well over a thousand lines, and playback renders the
 * panel four times a second. So `TranscriptSections`, `Section`, `Turn` and
 * `Line` are memoised, the panel hands `activeIndex` only to the section and
 * turn that hold the playing line (null to the rest), and every handler arrives
 * in one `TranscriptActions` object the panel makes once. A `timeupdate` that
 * does not move the playing line redraws only the panel around this; one that
 * does redraws the two sections, two turns and two lines it moved between.
 */

/** Scrolls jump rather than glide when the person asked for less motion. */
export function scrollBehavior(): ScrollBehavior {
  return typeof matchMedia === "function" && matchMedia("(prefers-reduced-motion: reduce)").matches ? "auto" : "smooth";
}

/** A speaker being renamed, and the turn whose name was pressed: the form opens under that turn. */
export type RenameTarget = { turnKey: string; speakerId: string; current: string };

/**
 * What a line or a turn can ask of the panel. Made once by `TranscriptPanel` from
 * callbacks that read its latest props and state through refs, so the object is
 * the same on every render and never redraws a memoised line by itself.
 */
export type TranscriptActions = {
  /** Keeps the panel's map from row index to its line, which its scrolls go through. */
  register: (index: number, element: HTMLLIElement | null) => void;
  /** The playing line's `.transcript-text`, whose words the panel lights frame by frame (KNEG-408). */
  wordsRef: RefObject<HTMLSpanElement | null>;
  select: (row: TranscriptRow, shift: boolean) => void;
  play: (start: number, end: number | null) => void;
  seek: (seconds: number) => void;
  openRename: (target: RenameTarget) => void;
  closeRename: (speakerId: string) => void;
  rename: (speakerId: string, name: string) => Promise<void>;
};

export function RenameForm({ target, suggestions, onRename, onClose }: {
  target: RenameTarget;
  suggestions: readonly string[];
  onRename: TranscriptActions["rename"];
  onClose: (speakerId: string) => void;
}) {
  const [value, setValue] = useState(target.current);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const datalistId = useId();
  const formRef = useRef<HTMLFormElement>(null);
  const heading = renameHeading(target.current);

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

  return <form ref={formRef} className="transcript-rename" aria-label={heading} onSubmit={onSubmit}>
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

type LineProps = {
  row: TranscriptRow;
  selected: boolean;
  active: boolean;
  /** Whether the note has a recording to play: each timed line gets Play, and its words when it has them. */
  canPlay: boolean;
  actions: TranscriptActions;
};

/**
 * One line: an option of its turn's listbox, selected by a click, Enter or Space (Shift extends),
 * each timed line a tab stop. An untimed line is a disabled option with no tab stop. Play floats
 * at the end of its first line, so the text below it runs the full width.
 */
export const Line = memo(function Line({ row, selected, active, canPlay, actions }: LineProps) {
  const playable = row.startSeconds !== null;
  const className = ["transcript-line", playable ? null : "transcript-row-muted", selected || active ? "transcript-row-current" : null]
    .filter(Boolean).join(" ");
  return (
    <li
      ref={(element) => { actions.register(row.index, element); }}
      className={className}
      role="option"
      tabIndex={playable ? 0 : undefined}
      aria-selected={playable ? selected : false}
      aria-disabled={playable ? undefined : true}
      aria-current={active ? "true" : undefined}
      onClick={(event) => actions.select(row, event.shiftKey)}
      onKeyDown={(event) => {
        if (!playable || (event.key !== "Enter" && event.key !== " ")) return;
        event.preventDefault();
        actions.select(row, event.shiftKey);
      }}
    >
      {canPlay && playable ? (
        <button
          type="button"
          className="transcript-play"
          aria-label={`Play transcript row ${row.index + 1}`}
          onClick={(event) => {
            event.stopPropagation();
            if (row.startSeconds !== null) actions.play(row.startSeconds, row.endSeconds);
          }}
        >
          Play
        </button>
      ) : null}
      {canPlay && wordsMatchText(row.words, row.text) && row.words.length > 0 ? (
        // Word by word while it plays. A click on a word goes there, and only there: the
        // line's own click would select or deselect it.
        <span className="transcript-text" ref={active ? actions.wordsRef : undefined}>
          {row.words.map((word, at) => (
            <Fragment key={at}>
              {at > 0 ? " " : null}
              <span className="transcript-word" role="button" aria-label={`Seek to ${word.text}`} tabIndex={at === 0 ? 0 : -1}
                onFocus={event => { const siblings = event.currentTarget.closest(".transcript-text")?.querySelectorAll<HTMLElement>(".transcript-word"); siblings?.forEach(element => { element.tabIndex = element === event.currentTarget ? 0 : -1; }); }}
                onKeyDown={event => {
                  if (event.key === "Enter" || event.key === " ") { event.preventDefault(); event.stopPropagation(); actions.seek(word.start); }
                  else if (["ArrowLeft", "ArrowRight", "Home", "End"].includes(event.key)) {
                    event.preventDefault(); event.stopPropagation();
                    const siblings = [...(event.currentTarget.closest(".transcript-text")?.querySelectorAll<HTMLElement>(".transcript-word") ?? [])];
                    const next = event.key === "Home" ? 0 : event.key === "End" ? siblings.length - 1 : Math.max(0, Math.min(siblings.length - 1, at + (event.key === "ArrowRight" ? 1 : -1)));
                    siblings[next]?.focus();
                  }
                }} onClick={(event) => { event.stopPropagation(); actions.seek(word.start); }}>{word.text}</span>
            </Fragment>
          ))}
        </span>
      ) : (
        <span className="transcript-text">{row.text}</span>
      )}
    </li>
  );
});

/** What every turn is drawn with, whichever section it is in. */
type Shared = {
  idPrefix: string;
  selected: ReadonlySet<number>;
  readOnly: boolean;
  canPlay: boolean;
  suggestions: readonly string[];
  actions: TranscriptActions;
};

type TurnProps = Shared & {
  turn: TranscriptTurn;
  /** The playing line's index when it is in this turn; null otherwise. */
  activeIndex: number | null;
  /** The rename when its name was pressed in this turn; null otherwise. */
  renaming: RenameTarget | null;
};

/**
 * One speaker's run of lines: a 6px square (`--band` while one of them plays), the name -- the
 * button that renames that speaker, or plain text when there is no label to rename -- and when
 * the turn starts. A turn with no speaker shows only the square and the time. The rename form
 * opens between the head and the lines. The lines are a listbox named by who and when.
 */
export const Turn = memo(function Turn({ turn, activeIndex, renaming, idPrefix, selected, readOnly, canPlay, suggestions, actions }: TurnProps) {
  const nameId = `${idPrefix}-${turn.key}-name`;
  const timeId = `${idPrefix}-${turn.key}-time`;
  const speakerId = turn.speakerId;
  const name = turn.speaker ?? speakerId;
  return (
    <div className={activeIndex === null ? "transcript-turn" : "transcript-turn transcript-turn-playing"}>
      <div className="transcript-turn-head">
        <span className="transcript-turn-marker" aria-hidden="true" />
        {speakerId !== null ? (
          <button type="button" className="transcript-speaker" disabled={readOnly} aria-label={`Rename ${name ?? speakerId}`}
            onClick={() => actions.openRename({ turnKey: turn.key, speakerId, current: name ?? speakerId })}>
            <span id={nameId}>{name}</span>
          </button>
        ) : name !== null ? <span className="transcript-speaker" id={nameId}>{name}</span> : null}
        <span className="transcript-clock" id={timeId}>{turn.startSeconds !== null ? formatClock(turn.startSeconds) : "—"}</span>
      </div>
      {!readOnly && renaming ? (
        // Changing the target must re-seed the field and clear any error from the previous speaker.
        <RenameForm key={renaming.speakerId} target={renaming} suggestions={suggestions} onRename={actions.rename} onClose={actions.closeRename} />
      ) : null}
      <ol className="transcript-rows" role="listbox" aria-multiselectable="true" aria-labelledby={name !== null ? `${nameId} ${timeId}` : timeId}>
        {turn.rows.map((row) => (
          <Line key={row.index} row={row} selected={selected.has(row.index)} active={activeIndex === row.index} canPlay={canPlay} actions={actions} />
        ))}
      </ol>
    </div>
  );
});

type Placed = {
  activeIndex: number | null;
  activeTurn: string | null;
  renaming: RenameTarget | null;
};

/** A section's turns, each told whether it holds the playing line or the open rename. */
function turnsOf(turns: readonly TranscriptTurn[], { activeIndex, activeTurn, renaming }: Placed, shared: Shared) {
  return turns.map((turn) => (
    <Turn key={turn.key} turn={turn} {...shared}
      activeIndex={turn.key === activeTurn ? activeIndex : null}
      renaming={renaming !== null && renaming.turnKey === turn.key ? renaming : null} />
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
 * place in the document.
 */
export const Section = memo(function Section({ section, open, onToggle, activeIndex, activeTurn, renaming, ...shared }: SectionProps) {
  const bodyId = `${shared.idPrefix}-${section.key}`;
  const titleId = `${bodyId}-title`;
  return (
    <section className={activeIndex === null ? "transcript-section" : "transcript-section transcript-section-playing"} aria-labelledby={titleId}>
      <h3 className="transcript-section-head">
        <button type="button" className="transcript-section-toggle" aria-expanded={open} aria-controls={bodyId} onClick={() => onToggle(section.key)}>
          <Icon name={open ? "chevronDown" : "chevronRight"} size={12} />
          <span className="transcript-section-title" id={titleId}>{section.title}</span>
          <span className="transcript-section-count">{section.lineCount} {section.lineCount === 1 ? "line" : "lines"}</span>
        </button>
      </h3>
      <div id={bodyId} className="transcript-section-body" hidden={!open}>
        {turnsOf(section.turns, { activeIndex, activeTurn, renaming }, shared)}
      </div>
    </section>
  );
});

type SectionsProps = Shared & {
  sections: readonly TranscriptSection[];
  collapsed: ReadonlySet<string>;
  onToggle: (key: string) => void;
  activeIndex: number | null;
  activeSection: string | null;
  activeTurn: string | null;
  renaming: RenameTarget | null;
  renamingSection: string | null;
};

/**
 * The sections, or -- when the whole transcript is one section -- its turns alone, with no
 * header over them.
 */
export const TranscriptSections = memo(function TranscriptSections({
  sections, collapsed, onToggle, activeIndex, activeSection, activeTurn, renaming, renamingSection, ...shared
}: SectionsProps) {
  const only = sections.length === 1 ? sections[0] : undefined;
  if (only !== undefined) return <>{turnsOf(only.turns, { activeIndex, activeTurn, renaming }, shared)}</>;
  return <>
    {sections.map((section) => (
      <Section key={section.key} section={section} open={!collapsed.has(section.key)} onToggle={onToggle} {...shared}
        activeIndex={section.key === activeSection ? activeIndex : null}
        activeTurn={section.key === activeSection ? activeTurn : null}
        renaming={section.key === renamingSection ? renaming : null} />
    ))}
  </>;
});
