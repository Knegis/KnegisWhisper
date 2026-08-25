/**
 * Shortcuts, dictation, and what came of it.
 *
 * The three belong on one screen because they are one loop: bind a key, press
 * it, read the result. Splitting them would put the evidence that dictation
 * worked on a different page from the control that started it, which is how the
 * first bench check ended up unable to tell "the hotkey did nothing" from "the
 * hotkey saved nothing".
 *
 * Dictation state arrives from Rust by event rather than being tracked here: the
 * hotkey works while this window is hidden, so the window cannot be the thing
 * that knows.
 */

import { useCallback, useEffect, useState } from "react";

import { Row } from "../components/Row";
import {
  cancelDictation,
  clearHistory,
  getDictationRoute,
  getDictationState,
  getHistory,
  getHotkeys,
  getInputDevices,
  isInsertionSupported,
  onDictationOutcome,
  onDictationState,
  onHotkey,
  setHotkey,
  setInputDevice,
  toggleDictation,
  type Binding,
} from "../lib/ipc";
import {
  acceleratorLabel,
  acceleratorText,
  changeLabel,
  outcomeLabel,
} from "../lib/labels";
import { useRefresh } from "../lib/useRefresh";

export function DictationView() {
  const load = useCallback(async () => {
    const [hotkeys, session, devices, canInsert, route, history] =
      await Promise.all([
        getHotkeys(),
        getDictationState(),
        getInputDevices(),
        isInsertionSupported(),
        getDictationRoute(),
        getHistory(20),
      ]);
    return { hotkeys, session, devices, canInsert, route, history };
  }, []);

  const { data, error, reload, guard } = useRefresh(load);

  /** Live session state, which arrives by event between reads. */
  const [session, setSession] = useState<string | null>(null);
  const [lastHotkey, setLastHotkey] = useState<string | null>(null);
  const [lastOutcome, setLastOutcome] = useState<string | null>(null);
  const [draftKeys, setDraftKeys] = useState<Record<string, string>>({});
  /** Per-action outcome of the last Set press, so the control is never silent. */
  const [hotkeyResult, setHotkeyResult] = useState<Record<string, string>>({});

  // Proves the hotkey path end to end: the shortcut fires in Rust and the event
  // arrives here.
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    void onHotkey((fired) => {
      setLastHotkey(
        `${fired.action} (${fired.accelerator}) at ${new Date().toLocaleTimeString()}`,
      );
    }).then((fn) => {
      unlisten = fn;
    });
    return () => unlisten?.();
  }, []);

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

  /**
   * Save one action's accelerator and say what happened.
   *
   * Every path reports something. An empty field, a refusal by the platform, and
   * a working shortcut are three different outcomes and the row names which one
   * it was — an earlier version of this control had a fourth, silence, which is
   * the only unacceptable one.
   */
  const applyHotkey = (b: Binding) => {
    const typed = (draftKeys[b.action] ?? acceleratorText(b)).trim();
    if (!typed) {
      setHotkeyResult((prev) => ({
        ...prev,
        [b.action]: "Type an accelerator first, for example Control+Shift+D.",
      }));
      return;
    }
    void guard(async () => {
      // A single comma-separated string, matching `hotkey_set`: one action can
      // have several accelerators.
      const change = await setHotkey(b.action, typed);
      setHotkeyResult((prev) => ({ ...prev, [b.action]: changeLabel(change) }));
      // Drop the draft so the field goes back to mirroring what is stored.
      setDraftKeys((prev) => {
        const next = { ...prev };
        delete next[b.action];
        return next;
      });
    });
  };

  if (!data) {
    return (
      <div className="view">
        {error ? <p className="error">{error}</p> : <p className="muted">Loading…</p>}
      </div>
    );
  }

  const { hotkeys, devices, canInsert, route, history } = data;
  const state = session ?? data.session;
  const recording = state === "recording";

  return (
    <div className="view">
      {error ? <p className="error">{error}</p> : null}

      <section>
        <h2>Shortcuts</h2>
        {hotkeys.bindings.map((b) => (
          <div key={b.action} className="row">
            <span className="row-label">{b.action}</span>
            <span className="row-value">
              {b.accelerators.length === 0 ? (
                <em className="row-detail">none configured</em>
              ) : (
                b.accelerators.map((a, i) => {
                  const label = acceleratorLabel(a);
                  return (
                    <span key={`${b.action}-${i}`}>
                      <code>{label.text}</code>
                      {label.problem ? (
                        <em className="row-detail warn">{label.problem}</em>
                      ) : null}
                    </span>
                  );
                })
              )}
              {/*
                Prefilled with what is actually configured, not a placeholder. A
                placeholder reading `Control+Shift+D` is indistinguishable from a typed
                value, so the field looked filled, the button was disabled for being
                empty, and pressing it did nothing and said nothing. That cost a real
                bench check.
              */}
              <input
                aria-label={`accelerator for ${b.action}`}
                value={draftKeys[b.action] ?? acceleratorText(b)}
                onChange={(e) => {
                  setDraftKeys((prev) => ({
                    ...prev,
                    [b.action]: e.target.value,
                  }));
                }}
                onKeyDown={(e) => {
                  if (e.key === "Enter") applyHotkey(b);
                }}
              />
              {/* Never disabled: a control that silently does nothing is the defect. */}
              <button type="button" onClick={() => applyHotkey(b)}>
                Set
              </button>
              {hotkeyResult[b.action] ? (
                <em className="row-detail">{hotkeyResult[b.action]}</em>
              ) : null}
            </span>
          </div>
        ))}
        {hotkeys.conflicts.length > 0 ? (
          <p className="caveat">
            {hotkeys.conflicts
              .map(
                (c) => `${c.accelerator} is claimed by ${c.actions.join(" and ")}`,
              )
              .join("; ")}
            .
          </p>
        ) : null}
        {lastHotkey ? <Row label="Last fired" value={lastHotkey} /> : null}
        <p className="caveat">
          A shortcut takes effect as soon as you set it. The dictation default,
          <code>Control+Super</code>, is two modifiers and no key — no
          cross-platform shortcut API can register that, so it has to be changed
          before the hotkey will do anything. Try <code>Control+Shift+D</code>.
        </p>
      </section>

      <section>
        <h2>Dictation</h2>
        <Row
          label="Session"
          value={state}
          detail={recording ? "the overlay is visible" : undefined}
        />
        <Row
          label="Text insertion"
          value={
            canInsert === null
              ? "checking…"
              : canInsert
                ? "Supported"
                : "Not supported here"
          }
          detail={
            canInsert === false
              ? "dictations are transcribed and kept in history, but cannot be typed at the cursor"
              : undefined
          }
        />
        <Row
          label="Route"
          value={route.problem ? "Not usable" : route.destination}
          detail={
            route.problem
              ? route.problem
              : route.leavesDevice
                ? "audio leaves this machine"
                : "audio stays on this machine"
          }
        />
        {lastOutcome ? <Row label="Last result" value={lastOutcome} /> : null}
        <div className="row">
          <span className="row-label">Microphone</span>
          <span className="row-value">
            <select
              onChange={(e) => {
                void guard(() => setInputDevice(e.target.value || null));
              }}
            >
              <option value="">System default</option>
              {devices.map((d) => (
                <option key={d.name} value={d.name}>
                  {d.name}
                  {d.isDefault ? " (default)" : ""}
                </option>
              ))}
            </select>
          </span>
        </div>
        <button
          type="button"
          onClick={() => {
            void guard(toggleDictation);
          }}
        >
          {recording ? "Stop and transcribe" : "Start dictation"}
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
      </section>

      <section>
        <h2>History</h2>
        {history.length === 0 ? (
          <p className="muted">Nothing dictated yet.</p>
        ) : (
          <>
            <ul className="purposes">
              {history.map((h) => (
                <li key={h.id}>
                  <code>{h.status}</code> {h.text || h.errorMessage || "(empty)"}
                  {h.provider ? (
                    <em className="row-detail">{h.provider}</em>
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
