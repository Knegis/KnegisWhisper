/**
 * Dictation: the task, and what came of it.
 *
 * The page leads with whether dictation will work (`lib/readiness.ts`), the
 * one button that starts it, and the shortcut, microphone and route it will
 * use -- each with a way to the place it is changed. The shortcut editor is
 * on Settings > Shortcuts.
 *
 * That split had to keep the loop this page used to hold on one screen: bind
 * a key, press it, read the result. Putting the evidence that dictation worked
 * on a different page from the control that started it is how the first bench
 * check ended up unable to tell "the hotkey did nothing" from "the hotkey saved
 * nothing". So the evidence travels with each half rather than staying behind
 * with one of them: this page shows the last result under the button that
 * started it, and Settings > Shortcuts shows *Last fired* and *Last dictation
 * result* under the editor.
 *
 * Dictation state arrives from Rust by event rather than being tracked here: the
 * hotkey works while this window is hidden, so the window cannot be the thing
 * that knows.
 */

import { useCallback, useEffect, useState } from "react";

import { LearnMore } from "../components/LearnMore";
import { LocalServerHealth } from "../components/LocalServerHealth";
import { Row } from "../components/Row";
import { StatusLine } from "../components/StatusLine";
import {
  cancelDictation,
  clearHistory,
  getDictationRoute,
  getDictationState,
  getHistory,
  getHistoryEnabled,
  getHotkeys,
  getInputDevice,
  getInputDevices,
  getKeepRawText,
  isInsertionSupported,
  onDictationOutcome,
  onDictationState,
  setHistoryEnabled,
  setInputDevice,
  setKeepRawText,
  toggleDictation,
  type SessionState,
} from "../lib/ipc";
import { acceleratorText, outcomeLabel, sessionStateLabel } from "../lib/labels";
import { microphoneState } from "../lib/microphone";
import type { ViewId } from "../lib/navigation";
import { dictationReadiness } from "../lib/readiness";
import { useRefresh } from "../lib/useRefresh";

export function DictationView({
  onGoTo,
}: {
  /** Where the status line's fix and each Change button take you. */
  onGoTo: (id: ViewId) => void;
}) {
  const load = useCallback(async () => {
    const [
      hotkeys,
      session,
      devices,
      device,
      canInsert,
      route,
      history,
      historyOn,
      keepRaw,
    ] = await Promise.all([
      getHotkeys(),
      getDictationState(),
      getInputDevices(),
      getInputDevice(),
      isInsertionSupported(),
      getDictationRoute(),
      getHistory(20),
      getHistoryEnabled(),
      getKeepRawText(),
    ]);
    return {
      hotkeys,
      session,
      devices,
      device,
      canInsert,
      route,
      history,
      historyOn,
      keepRaw,
    };
  }, []);

  const { data, error, reload, guard } = useRefresh(load);

  /** Live session state, which arrives by event between reads. */
  const [session, setSession] = useState<SessionState | null>(null);
  const [lastOutcome, setLastOutcome] = useState<string | null>(null);

  useEffect(() => {
    const unlisteners: Array<() => void> = [];
    void onDictationState(setSession).then((fn) => unlisteners.push(fn));
    void onDictationOutcome((outcome) => {
      setLastOutcome(outcomeLabel(outcome));
      // History and the session both moved, so re-read rather than guessing.
      void reload();
    }).then((fn) => unlisteners.push(fn));
    return () => unlisteners.forEach((fn) => fn());
  }, [reload]);

  if (!data) {
    return (
      <div className="view">
        {error ? <p className="error">{error}</p> : <p className="muted">Loading…</p>}
      </div>
    );
  }

  const { hotkeys, devices, device, canInsert, route, history, historyOn, keepRaw } =
    data;
  const state = session ?? data.session;
  // The floating microphone's own reading of the session, reused rather than
  // re-derived, so the two buttons cannot disagree -- and so Start is off
  // while a dictation is still transcribing or being typed, which this button
  // alone used to allow (KNEG-342).
  const mic = microphoneState(state, false);
  const recording = mic.recording;
  const dictationBinding = hotkeys.bindings.find((b) => b.action === "dictation");
  const shortcut = dictationBinding ? acceleratorText(dictationBinding) : "";
  // A microphone chosen earlier and unplugged since. Listed as itself rather
  // than letting the select fall back to "System default", which would read as
  // a choice nobody made. The recording itself does fall back to the default
  // -- `Capture::start` in crates/audio-core/src/capture.rs asks for the device
  // by name and takes the default when it is gone -- so the row says that too.
  const missingDevice =
    device !== null && !devices.some((d) => d.name === device) ? device : null;

  return (
    <div className="view">
      {error ? <p className="error">{error}</p> : null}

      <section data-surface="dictation.task" aria-label="Dictation">
        {/* Only while no dictation is under way. Readiness answers "will
            starting work?", which means nothing once one has started --
            "Ready" over a recording, or over text being typed, reads as a
            claim about the session. The Status row speaks for it instead. */}
        {mic.recording || mic.busy ? null : (
          <StatusLine
            readiness={dictationReadiness({ route, hotkeys, canInsert })}
            onFix={onGoTo}
          />
        )}
        {/* Off on a broken route: the backend resolves the route only at Stop
            and cancels on failure (`toggle`, src-tauri/src/dictation.rs), so
            Start would record words that are then thrown away. A missing
            shortcut does not disable it -- the button needs none. */}
        <button
          type="button"
          className="primary"
          disabled={mic.disabled || (!recording && route.problem !== null)}
          onClick={() => {
            void guard(toggleDictation);
          }}
        >
          {recording
            ? "Stop and transcribe"
            : mic.busy
              ? sessionStateLabel(state)
              : "Start dictation"}
        </button>
        {recording ? (
          <button
            type="button"
            onClick={() => {
              void guard(cancelDictation);
            }}
          >
            Discard
          </button>
        ) : null}

        <div className="row">
          <span className="row-label">Shortcut</span>
          <span className="row-value controls">
            {shortcut ? <code>{shortcut}</code> : <span>none set</span>}
            <button type="button" onClick={() => onGoTo("shortcuts")}>
              Change
            </button>
          </span>
        </div>
        <div className="row">
          <span className="row-label">Microphone</span>
          <span className="row-value controls">
            {/* Controlled by what the backend reports, not by the last click:
                `guard` re-reads after the change, and `getInputDevice` is what
                brings the choice back. Uncontrolled, this read "System default"
                whatever had been chosen (KNEG-340). */}
            <select
              aria-label="Microphone"
              value={device ?? ""}
              onChange={(e) => {
                const next = e.target.value || null;
                void guard(() => setInputDevice(next));
              }}
            >
              {missingDevice !== null ? (
                <option value={missingDevice}>{missingDevice} (not connected)</option>
              ) : null}
              <option value="">System default</option>
              {devices.map((d) => (
                <option key={d.name} value={d.name}>
                  {d.name}
                  {d.isDefault ? " (default)" : ""}
                </option>
              ))}
            </select>
            {missingDevice !== null ? (
              <em className="row-detail">
                Not connected, so dictation uses the system default until it is
                plugged back in.
              </em>
            ) : null}
          </span>
        </div>
        {/* The privacy fact, so it is on the row rather than behind anything:
            whether this dictation's audio leaves this computer. `leavesDevice`
            decides the sentence rather than `isLocal`, because it is the field
            that answers that question; `isLocal` only names the engine. A
            route with a problem has no destination to describe -- the status
            line above says what to do, and carries the problem itself. */}
        <div className="row">
          <span className="row-label">Transcription</span>
          <span className="row-value controls">
            <span>
              {route.problem
                ? "Not set up"
                : route.isLocal
                  ? "On this computer"
                  : route.destination}
            </span>
            {route.problem ? null : (
              <em className="row-detail">
                {route.leavesDevice
                  ? `Audio is sent to ${route.destination}`
                  : "Audio stays on this computer"}
              </em>
            )}
            <button type="button" onClick={() => onGoTo("transcription")}>
              Change
            </button>
          </span>
        </div>
        <Row
          label="Status"
          value={sessionStateLabel(state)}
          detail={recording ? "the overlay is visible" : undefined}
        />
        {lastOutcome ? <Row label="Last result" value={lastOutcome} /> : null}
        {route.isLocal ? <LocalServerHealth /> : null}
      </section>

      <section data-surface="dictation.history">
        <h2>History</h2>

        {/* First, because it governs whether the other two controls in this
            section have anything to act on. Until 2026-08-31 the backend
            honoured `history.enabled` before every write and nothing in the
            application could set it: the only way to stop the app writing down
            everything dictated was to edit SQLite by hand. */}
        <div className="row">
          <span className="row-label">Save dictation history</span>
          <span className="row-value controls">
            <label>
              <input
                type="checkbox"
                checked={historyOn}
                onChange={(e) => {
                  const enabled = e.target.checked;
                  void guard(async () => {
                    await setHistoryEnabled(enabled);
                    await reload();
                  });
                }}
              />
              Add each dictation to the list below
            </label>
            {/* This and Clear history are a stop and a delete, and they sound
                alike enough that the difference has to be said rather than
                discovered -- so the one visible sentence says it in both
                states: switching this off deletes nothing. It is a retention
                fact, and a retention fact is never folded. Only the pointer to
                the control that does delete sits under Learn more. Neither
                sentence counts anything: the list below is capped at twenty, so
                a number here would be a number about the wrong set. */}
            <em className="row-detail">
              {historyOn
                ? "Switching this off stops new entries; it deletes nothing."
                : "Nothing new is being saved; what was saved before stays until you clear it."}
            </em>
            <LearnMore>
              <p>Clear history is the control that removes what is already saved.</p>
            </LearnMore>
          </span>
        </div>

        {/* The same shape `Settings > Meetings` uses for "Offer to record":
            `Row` takes a string value, and this needs a control. */}
        <div className="row">
          <span className="row-label">Keep the text before cleanup</span>
          <span className="row-value controls">
            <label>
              <input
                type="checkbox"
                checked={keepRaw}
                /* Disabled rather than hidden while history is off. It is a
                   choice about what a row contains, and with history off there
                   are no rows -- but the setting is still stored, so hiding it
                   would leave a value the user set and can no longer see. */
                disabled={!historyOn}
                onChange={(e) => {
                  const enabled = e.target.checked;
                  void guard(async () => {
                    await setKeepRawText(enabled);
                    await reload();
                  });
                }}
              />
              Store both versions when cleanup changes the text
            </label>
            {/* Said plainly because this is a privacy choice, not a
                preference: switching it on stores a second copy of what was
                said. The reason to want it is that a row carrying both is the
                only proof cleanup did anything on that dictation. One
                sentence in every state, like the switch above. */}
            <em className="row-detail">
              {!historyOn
                ? "No entries are being saved, so there is nothing for a second version to go on."
                : keepRaw
                  ? "Two copies of what you said are stored on rows cleanup altered."
                  : "Only the final text is stored; switch this on to see what cleanup changed."}
            </em>
          </span>
        </div>

        {history.length === 0 ? (
          <p className="muted">Nothing dictated yet.</p>
        ) : (
          <>
            {/* `list`, not `purposes`: a history entry is a status, the text
                that was dictated and where it went -- a row, not a pill. */}
            <ul className="list">
              {history.map((h) => (
                <li key={h.id}>
                  <code>{h.status}</code> {h.text || h.errorMessage || "(empty)"}
                  {h.provider ? (
                    <em className="row-detail">{h.provider}</em>
                  ) : null}
                  {/* Only ever present when cleanup changed the text and the
                      user asked to keep it -- see `raw_for_history`. So its
                      presence is the finding, and it is labelled rather than
                      shown as a bare second line. */}
                  {h.rawText ? (
                    <em className="row-detail">before cleanup: {h.rawText}</em>
                  ) : null}
                </li>
              ))}
            </ul>
            <button
              type="button"
              onClick={() => {
                void guard(clearHistory);
              }}
            >
              Clear history
            </button>
          </>
        )}
      </section>
    </div>
  );
}
