import type { MouseEvent, RefObject } from "react";

import { Icon } from "./Icon";

type TranscriptFindBarProps = {
  /** The field, which `TranscriptPanel` focuses as the bar opens and keeps focus in while it steps. */
  inputRef: RefObject<HTMLInputElement | null>;
  query: string;
  onQuery: (query: string) => void;
  /** The count, "2/5" and "Match 2 of 5" (`findStatus`); null while there is no query. */
  status: { shown: string; spoken: string } | null;
  /** Whether there is a match to step to: Previous and Next are disabled while there is none. */
  canStep: boolean;
  /** Next (1) or previous (-1) match, wrapping: Enter and Shift+Enter in the field, or the buttons. */
  onStep: (direction: 1 | -1) => void;
  /** Escape anywhere in the bar, or Close: the query cleared and focus back on the Find button. */
  onClose: () => void;
};

/** A press on a step button leaves focus where it is -- in the field -- as a find bar's buttons do. */
function keepFocus(event: MouseEvent<HTMLButtonElement>) {
  event.preventDefault();
}

/**
 * Find in this transcript (KNEG-448), in the transcript's head in place of its label while open:
 * a field with a search icon and the count, then Previous, Next and Close. The count is shown as
 * "2/5" and hidden from assistive technology, which hears "Match 2 of 5" from a status instead.
 * Typing goes to a match and Enter moves on; `TranscriptPanel` does the moving, which never selects
 * a line or plays. There is no key to open it from elsewhere: the head's search button does.
 */
export function TranscriptFindBar({ inputRef, query, onQuery, status, canStep, onStep, onClose }: TranscriptFindBarProps) {
  return (
    <div className="transcript-findbar" role="search" onKeyDown={(event) => {
      if (event.key !== "Escape") return;
      event.preventDefault();
      event.stopPropagation();
      onClose();
    }}>
      <label className="transcript-find-field">
        <Icon name="search" size={14} />
        <input ref={inputRef} className="transcript-find-input" type="text" aria-label="Find in transcript" value={query}
          spellCheck={false} autoComplete="off" onChange={(event) => onQuery(event.target.value)}
          onKeyDown={(event) => {
            // Enter that ends a composition (an IME) is the composition's, not a step.
            if (event.key !== "Enter" || event.nativeEvent.isComposing) return;
            event.preventDefault();
            onStep(event.shiftKey ? -1 : 1);
          }} />
        {status === null ? null : <span className="transcript-find-count" aria-hidden="true">{status.shown}</span>}
      </label>
      <span className="sr-only" role="status">{status?.spoken ?? ""}</span>
      <button type="button" className="transcript-find-step" aria-label="Previous match" title="Previous match" disabled={!canStep}
        onMouseDown={keepFocus} onClick={() => onStep(-1)}>
        <Icon name="chevronUp" size={16} />
      </button>
      <button type="button" className="transcript-find-step" aria-label="Next match" title="Next match" disabled={!canStep}
        onMouseDown={keepFocus} onClick={() => onStep(1)}>
        <Icon name="chevronDown" size={16} />
      </button>
      <button type="button" className="transcript-find-step" aria-label="Close find" title="Close find" onClick={onClose}>
        <Icon name="close" size={16} />
      </button>
    </div>
  );
}
