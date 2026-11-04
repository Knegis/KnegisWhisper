import { useEffect, useRef, useState } from "react";

import { groupByDay } from "../lib/dayGroups";
import { historyRow } from "../lib/historyRow";
import { shortcutText } from "../lib/keycaps";
import {
  clearHistory,
  setHistoryEnabled,
  setKeepRawText,
  type Transcription,
} from "../lib/ipc";
import type { ViewId } from "../lib/navigation";
import { Icon } from "./Icon";

/** How long "Copied" stands in for a row's Copy. */
const COPIED_MS = 2000;

/**
 * Dictation's history, under the page: the way to the shortcut and the
 * vocabulary, the history grouped by day, and the history options.
 *
 * This is the `dictation.history` surface (`config/ui-surfaces.json`): the two
 * switches and Clear history call their IPC wrappers from this file, because
 * `tools/settings-audit` checks that the component a surface names is the one
 * that writes `history.enabled` and `history.keepRawText`.
 *
 * # Rows
 *
 * A row is its text over its time, with a Copy button that shows under the
 * pointer and on keyboard focus: copying is the only thing a past dictation is
 * for, once it has been typed somewhere. The button answers "Copied" for two
 * seconds, and a copy that fails says "Couldn't copy" and keeps saying it until
 * the next press, because a failure stays visible, so an answering button stays
 * shown. Both are also said in a polite live region.
 *
 * What a row says comes from `lib/historyRow.ts`. A failed row keeps its text
 * when it has one -- a dictation whose paste failed is stored with the cleaned
 * text on purpose, so it can be copied from here -- and the failure's reason goes
 * in the meta line beside the time. Only a row with no text at all -- the request
 * failed before anything came back -- reads "Didn't work", muted, beside the
 * signal dot, and its Copy takes the reason, which is the text a bug report
 * needs. A row cleanup changed says " · Cleaned up", and its tooltip carries the
 * text before cleanup and the provider that transcribed it.
 *
 * # History off
 *
 * The section says so, and opens the options, since switching it back on is the
 * likely next press -- and leaves them open when it is, rather than folding them
 * under the switch just pressed. Anything saved before it was switched off is
 * still listed: it is still stored, and a list that went blank would read as
 * deleted when it is not.
 *
 * There is no search button here. The title bar's search looks through notes
 * only, so a search affordance over dictation history would promise something
 * nothing does.
 */
export function DictationHistory({
  history,
  historyOn,
  keepRaw,
  shortcut,
  onGoTo,
  guard,
  reload,
}: {
  history: readonly Transcription[];
  historyOn: boolean;
  keepRaw: boolean;
  /** `acceleratorText` of the dictation binding, or `""` when none is set. */
  shortcut: string;
  onGoTo: (id: ViewId) => void;
  guard: (work: () => Promise<unknown>) => Promise<void>;
  reload: () => Promise<void>;
}) {
  // Which row last answered a press, and how. One at a time: a second press
  // moves the notice rather than leaving two.
  const [notice, setNotice] = useState<{ id: number; text: string } | null>(null);
  const timer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  useEffect(() => () => clearTimeout(timer.current), []);

  // Only ever opened by history going off, never closed by it coming back on
  // (`LearnMore`'s rule): closing here would fold the options under the switch
  // just pressed and drop its focus.
  const [optionsOpen, setOptionsOpen] = useState(!historyOn);
  useEffect(() => {
    if (!historyOn) setOptionsOpen(true);
  }, [historyOn]);

  const copy = async (id: number, text: string) => {
    clearTimeout(timer.current);
    try {
      await navigator.clipboard.writeText(text);
      setNotice({ id, text: "Copied" });
      timer.current = setTimeout(() => setNotice(null), COPIED_MS);
    } catch {
      setNotice({ id, text: "Couldn't copy" });
    }
  };

  const groups = groupByDay(history, (h) => h.createdAt, new Date());

  return (
    <section data-surface="dictation.history" className="history" aria-labelledby="history-title">
      <div className="history-head">
        <div className="history-title">
          <h2 id="history-title" className="section-label">history</h2>
          <span className="history-privacy">
            <Icon name="lock" size={16} />
            History stays on this computer.
          </span>
        </div>
        <div className="history-actions">
          <button type="button" className="pill pill-with-key" onClick={() => onGoTo("shortcuts")}>
            Shortcut
            <span className="pill-key">{shortcut ? shortcutText(shortcut, navigator.platform) : "none set"}</span>
          </button>
          <button type="button" className="pill" onClick={() => onGoTo("vocabulary")}>
            Vocabulary
          </button>
          <button
            type="button"
            className="pill"
            aria-expanded={optionsOpen}
            aria-controls="history-options"
            onClick={() => setOptionsOpen((open) => !open)}
          >
            History options
            <span className="pill-plus" data-open={optionsOpen}><Icon name="plus" size={12} /></span>
          </button>
        </div>
      </div>

      {optionsOpen ? (
        <div id="history-options" className="history-options learn-more-body">
          {/* First, because it governs whether the other two controls have
              anything to act on. Until 2026-08-31 the backend honoured
              `history.enabled` before every write and nothing in the
              application could set it: the only way to stop the app writing
              down everything dictated was to edit SQLite by hand. */}
          <label className="switch-row">
            <span>Save dictation history</span>
            <input
              type="checkbox"
              role="switch"
              checked={historyOn}
              onChange={(e) => {
                const enabled = e.target.checked;
                void guard(async () => {
                  await setHistoryEnabled(enabled);
                  await reload();
                });
              }}
            />
          </label>
          {/* This and Clear history are a stop and a delete, and they sound
              alike enough that the difference has to be said rather than
              discovered -- so the sentence says it in both states: switching
              this off deletes nothing. Neither sentence counts anything: the
              list is capped at twenty, so a number here would be a number
              about the wrong set. */}
          <p>
            Adds each dictation to the list below.{" "}
            {historyOn
              ? "Switching this off stops new entries; it deletes nothing."
              : "Nothing new is being saved; what was saved before stays until you clear it."}
          </p>

          <label className="switch-row">
            <span>Keep the text before cleanup</span>
            <input
              type="checkbox"
              role="switch"
              checked={keepRaw}
              /* Disabled rather than hidden while history is off. It is a
                 choice about what a row contains, and with history off there
                 are no new rows -- but the setting is still stored, so hiding
                 it would leave a value the user set and can no longer see. */
              disabled={!historyOn}
              onChange={(e) => {
                const enabled = e.target.checked;
                void guard(async () => {
                  await setKeepRawText(enabled);
                  await reload();
                });
              }}
            />
          </label>
          {/* Said plainly because this is a privacy choice, not a
              preference: switching it on stores a second copy of what was
              said. The reason to want it is that a row carrying both is the
              only proof cleanup did anything on that dictation. */}
          <p>
            Stores both versions when cleanup changes the text.{" "}
            {!historyOn
              ? "No entries are being saved, so there is nothing for a second version to go on."
              : keepRaw
                ? "Two copies of what you said are stored on rows cleanup altered."
                : "Only the final text is stored; switch this on to see what cleanup changed."}
          </p>

          {history.length === 0 ? null : (
            <>
              <p>Clear history is the control that removes what is already saved.</p>
              <button
                type="button"
                className="pill"
                onClick={() => {
                  void guard(clearHistory);
                }}
              >
                Clear history
              </button>
            </>
          )}
        </div>
      ) : null}

      {historyOn ? null : <p className="history-note">History is off.</p>}
      {history.length === 0 ? (
        historyOn ? <p className="history-note">Nothing dictated yet.</p> : null
      ) : (
        groups.map((group) => (
          <div key={group.label} className="history-day">
            <h3 className="history-date">{group.label.toLowerCase()}</h3>
            <ul className="history-rows">
              {group.items.map((h) => {
                const row = historyRow(h);
                const answer = notice?.id === h.id ? notice.text : null;
                return (
                  <li key={h.id} className="history-row" {...(row.tooltip === null ? {} : { title: row.tooltip })}>
                    <div className="history-row-body">
                      <span className={row.muted ? "history-row-text history-row-muted" : "history-row-text"}>
                        {row.muted ? <span className="history-row-dot" aria-hidden="true" /> : null}
                        {row.title}
                      </span>
                      <span className="history-row-meta">
                        {row.time === null ? null : <span className="history-row-time">{row.time}</span>}
                        {row.time !== null && row.detail !== "" ? " · " : null}
                        {row.detail}
                      </span>
                    </div>
                    {row.copyDescription === null ? null : (
                      <button
                        type="button"
                        className="history-copy"
                        data-answering={answer !== null}
                        aria-description={row.copyDescription}
                        {...(row.tooltip === null ? {} : { title: row.tooltip })}
                        onClick={() => {
                          void copy(h.id, row.copyText);
                        }}
                      >
                        {answer ?? "Copy"}
                      </button>
                    )}
                  </li>
                );
              })}
            </ul>
          </div>
        ))
      )}
      {/* Always rendered, so the region exists before the notice arrives
          in it -- a live region added along with its text is often not
          read. */}
      <p className="sr-only" role="status">{notice?.text ?? ""}</p>
    </section>
  );
}
