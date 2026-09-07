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

import { LocalServerHealth } from "../components/LocalServerHealth";
import { Row } from "../components/Row";
import {
  cancelDictation,
  clearHistory,
  getDictationRoute,
  getDictationState,
  getHistory,
  getHistoryEnabled,
  getHotkeys,
  getInputDevices,
  getKeepRawText,
  isInsertionSupported,
  onDictationOutcome,
  onDictationState,
  onHotkey,
  setHistoryEnabled,
  setHotkey,
  setHotkeyMode,
  setInputDevice,
  setKeepRawText,
  toggleDictation,
  type Binding,
  type HotkeyMode,
  type ShortcutHookStatus,
  type UnhandledAction,
} from "../lib/ipc";
import {
  acceleratorLabel,
  acceleratorText,
  changeLabel,
  hotkeyModeLabel,
  outcomeLabel,
} from "../lib/labels";
import { useRefresh } from "../lib/useRefresh";

export function DictationView({
  hook,
}: {
  /**
   * Whether modifier-only shortcuts work here, from diagnostics.
   *
   * A prop rather than a second `getDiagnostics()` call: the shell already has
   * it, and two reads of the same fact are two chances to disagree.
   */
  hook: ShortcutHookStatus;
}) {
  const load = useCallback(async () => {
    const [
      hotkeys,
      session,
      devices,
      canInsert,
      route,
      history,
      historyOn,
      keepRaw,
    ] = await Promise.all([
      getHotkeys(),
      getDictationState(),
      getInputDevices(),
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
      canInsert,
      route,
      history,
      historyOn,
      keepRaw,
    };
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

  /**
   * Switch one action between toggling and being held.
   *
   * Takes effect immediately, like the accelerator does, and reports through the
   * same row: the backend re-registers everything and hands back the same
   * `HotkeyChange`, so "saved but nothing was registered" cannot happen silently
   * for a mode change any more than it can for a key.
   */
  const applyMode = (b: Binding, mode: HotkeyMode) => {
    void guard(async () => {
      const change = await setHotkeyMode(b.action, mode);
      setHotkeyResult((prev) => ({ ...prev, [b.action]: changeLabel(change) }));
      await reload();
    });
  };

  if (!data) {
    return (
      <div className="view">
        {error ? <p className="error">{error}</p> : <p className="muted">Loading…</p>}
      </div>
    );
  }

  const { hotkeys, devices, canInsert, route, history, historyOn, keepRaw } =
    data;
  // Keyed by action, because that is the shape of the fact: nothing handles the
  // chat agent, whatever it happens to be bound to.
  const unhandled = new Map<string, string>(
    hotkeys.unhandled.map((u: UnhandledAction) => [u.action, u.reason]),
  );
  const state = session ?? data.session;
  const recording = state === "recording";

  return (
    <div className="view">
      {route.isLocal ? <LocalServerHealth /> : null}
      {error ? <p className="error">{error}</p> : null}

      <section data-surface="dictation.shortcuts">
        <h2>Shortcuts</h2>
        {hotkeys.bindings.map((b) => (
          <div key={b.action} className="row">
            <span className="row-label">{b.action}</span>
            <span className="row-value">
              {/*
                First, and ahead of the accelerator, because it outranks anything
                the accelerator could say. Four of these five actions used to
                register a real system shortcut and then do nothing whatsoever
                when pressed — and the row showed a tidy `Control+Shift+Space`
                with no problem beside it, which is the interface agreeing that
                it worked.
              */}
              {unhandled.has(b.action) ? (
                <em className="row-detail warn">{unhandled.get(b.action)}</em>
              ) : null}
              {b.accelerators.length === 0 ? (
                <em className="row-detail">none configured</em>
              ) : (
                b.accelerators.map((a, i) => {
                  const label = acceleratorLabel(a, hook);
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
              {/*
                Toggle or hold, per action. The shipped default is Toggle for
                every one of them and deliberately stays that way: holding is
                one of the answers to the prefix problem the caution below
                describes (KNEG-115), and making it the default would settle
                that question by accident. The option exists; the decision does
                not come from here.
              */}
              <select
                aria-label={`mode for ${b.action}`}
                value={b.mode}
                onChange={(e) => applyMode(b, e.target.value as HotkeyMode)}
              >
                {(["toggle", "hold"] as const).map((m) => (
                  <option key={m} value={m}>
                    {hotkeyModeLabel(m).text}
                  </option>
                ))}
              </select>
              <em className="row-detail">{hotkeyModeLabel(b.mode).detail}</em>
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
        {/*
          A third category, warn-coloured rather than filed under the caveats
          below, because it is the only thing on this screen that says a hotkey
          which *works* will also do something you did not ask for. Binding
          `Control+Alt` on a Nordic or German layout starts a dictation every
          time you type `@` — the keys Windows sends for AltGr are the chord,
          and nothing downstream can tell the two apart. See KNEG-37.
        */}
        {hotkeys.cautions.map((c, i) => (
          <p className="caveat warn" key={`caution-${i}`}>
            <strong>{c.action}:</strong> {c.reason}
          </p>
        ))}
        {lastHotkey ? <Row label="Last fired" value={lastHotkey} /> : null}
        {hotkeys.unhandled.length > 0 ? (
          <p className="caveat">
            {hotkeys.unhandled.length} of {hotkeys.bindings.length} actions have
            nothing behind them yet. Their shortcuts are still saved, and they
            are deliberately not registered with the system — so those keys stay
            free for the applications that do something with them.
          </p>
        ) : null}
        <p className="caveat">
          A shortcut takes effect as soon as you set it. The dictation default,{" "}
          <code>Control+Super</code>, is two modifiers and no key, which no
          cross-platform shortcut API can register.{" "}
          {hook.state === "available"
            ? `It works here anyway, through ${hook.mechanism} — and unlike a registered shortcut it does not take the keys away from anything else, so Windows still does whatever it does with them.`
            : "It therefore does nothing here and has to be changed. Try Control+Shift+D."}
        </p>
        <p className="caveat">
          The same hooks watch the two side buttons on a mouse: type{" "}
          <code>Mouse4</code> or <code>Mouse5</code> as the accelerator. Left,
          right and middle are deliberately refused — the click is not
          swallowed, so binding one would start this action on every click in
          every application while the click still went through.
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
                discovered. Both sentences name the other control, so whichever
                state the switch is in the user is told where the other half is.
                Neither counts anything: the list below is capped at twenty, so
                a number here would be a number about the wrong set. */}
            <em className="row-detail">
              {historyOn
                ? "Switching this off stops new entries and deletes nothing — Clear history is the control that removes what is already saved."
                : "Nothing new is being saved. Switching this off never deletes anything — what was saved before is still in the list until Clear history removes it."}
            </em>
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
                only proof cleanup did anything on that dictation. */}
            <em className="row-detail">
              {!historyOn
                ? "No entries are being saved, so there is nothing for a second version to go on."
                : keepRaw
                  ? "Two copies of what you said are stored on rows cleanup altered."
                  : "Only the final text is stored. Switch this on to see what cleanup changed."}
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
