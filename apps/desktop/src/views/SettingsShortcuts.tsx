/**
 * Settings > Shortcuts: the key bound to each action, and the evidence that
 * pressing it did something.
 *
 * The editor used to open the Dictation page. It moved here so Dictation could
 * lead with the task, and the evidence moved with it, because binding a key,
 * pressing it and reading the result are one loop: the first bench check ended
 * up unable to tell "the hotkey did nothing" from "the hotkey saved nothing",
 * and a screen that split the control from its evidence would bring that back.
 * So *Last fired* -- the key reached Rust and came back as an event -- and
 * *Last dictation result* -- what the dictation it started produced -- sit
 * directly under the editor. Dictation shows the last result as well; it is
 * the same event, read on both pages.
 *
 * What someone has to see stays on the screen: a conflict, a caution, each
 * row's own problem line, the portal's restart notice, and a modifier-only
 * default that does nothing on this computer. A problem line is red because
 * it is a failure; a binding that works and is worth a word -- a hooked chord,
 * a watched mouse button -- says so in one plain sentence instead, with the
 * hook that makes it work under its own Learn more. Why the shortcuts behave
 * the way they do -- the actions with nothing behind them, the
 * `Control+Super` default, the mouse buttons -- is folded under "About these
 * shortcuts".
 */

import { Fragment, useCallback, useEffect, useState } from "react";

import { Keycaps } from "../components/Keycaps";
import { LearnMore } from "../components/LearnMore";
import { Row } from "../components/Row";
import { ShortcutRecorder } from "../components/ShortcutRecorder";
import {
  getHotkeys,
  getSetting,
  onDictationOutcome,
  onHotkey,
  setHotkey,
  setHotkeyMode,
  setHotkeyPortal,
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
  registrationFor,
} from "../lib/labels";
import { useRefresh } from "../lib/useRefresh";

export function SettingsShortcuts({
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
    const [hotkeys, portalPreference] = await Promise.all([
      getHotkeys(),
      getSetting("hotkey.usePortal"),
    ]);
    return { hotkeys, portalPreference: portalPreference === true };
  }, []);

  const { data, error, reload, guard } = useRefresh(load);

  useEffect(() => {
    if (!data?.hotkeys.portalEnabled) return;
    const timer = window.setInterval(() => void reload(), 3000);
    return () => window.clearInterval(timer);
  }, [data?.hotkeys.portalEnabled, reload]);

  const [lastHotkey, setLastHotkey] = useState<string | null>(null);
  const [lastOutcome, setLastOutcome] = useState<string | null>(null);
  const [draftKeys, setDraftKeys] = useState<Record<string, string>>({});
  const [captureBusy, setCaptureBusy] = useState(false);
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

  // The other half of the loop: what the dictation that key started came to.
  // No re-read after it, unlike Dictation's copy of this listener -- nothing
  // this screen loads changes when a dictation ends.
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    void onDictationOutcome((outcome) => {
      setLastOutcome(outcomeLabel(outcome));
    }).then((fn) => {
      unlisten = fn;
    });
    return () => unlisten?.();
  }, []);

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

  const { hotkeys } = data;
  // Keyed by action, because that is the shape of the fact: nothing handles the
  // chat agent, whatever it happens to be bound to.
  const unhandled = new Map<string, string>(
    hotkeys.unhandled.map((u: UnhandledAction) => [u.action, u.reason]),
  );
  // The modifier-only shortcuts bound now. Without the hook they do nothing,
  // and once dictation is rebound away from `Control+Super` there are none, so
  // the caveat below goes with them rather than outliving its cause (KNEG-351).
  // Once each: two actions can share one.
  const modifierOnly = [...new Set(hotkeys.bindings.flatMap((b) =>
    b.accelerators.filter((a) => a.kind === "modifier-only").map((a) => a.accelerator),
  ))];

  return (
    <div className="view">
      {error ? <p className="error">{error}</p> : null}

      <section data-surface="settings.shortcuts" aria-label="Shortcuts">
        {hotkeys.portalSupported && (
          <div className="field">
            <label>
              <input type="checkbox" checked={data.portalPreference}
                onChange={(event) => { const enabled = event.target.checked; void guard(async () => {
                  await setHotkeyPortal(enabled); await reload();
                }); }} />
              Use desktop portal for shortcuts
            </label>
            <p className="muted">Opt-in preview. Restart the application after changing this setting. The desktop may ask you to approve or change the shortcut.</p>
            {data.portalPreference !== hotkeys.portalEnabled && <p role="status">Restart required to apply this change.</p>}
          </div>
        )}
        {hotkeys.portalEnabled && hotkeys.registrations.map((report) => (
          <div key={report.action} className="row-detail">
            {report.results.map((result, index) => <p key={index}>
              {result.accelerator}: {result.state === "portal"
                ? `Desktop shortcut: ${result.granted ?? "see desktop shortcut settings"}`
                : result.state === "registered" ? "Registered using the fallback shortcut service."
                : result.state === "hooked" ? "Keyboard hook active."
                : result.reason}
            </p>)}
          </div>
        ))}
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
                  // The registration as well as the platform's hook: a hook
                  // that failed to start leaves the platform "available" and
                  // the chord dead (KNEG-353).
                  const label = acceleratorLabel(a, hook, registrationFor(hotkeys.registrations, b.action, a));
                  // No "works" note for an action nothing handles: its
                  // accelerators are not offered to the platform at all --
                  // not registered and not hooked (`register_all`,
                  // src-tauri/src/shortcuts.rs) -- so the sentence would be
                  // false, and the red line above already says why.
                  const note = unhandled.has(b.action) ? null : label.note;
                  // A fragment, not a wrapping span: each line becomes its
                  // own item in `.row-value`'s column, so the key and the
                  // sentence about it get the column's gap between them
                  // instead of running together ("Control+SuperUses only…").
                  // Red only for a failure; a binding that works and is worth
                  // a word says so plainly, with the mechanism folded.
                  // Keycaps for an accelerator that parsed, as Dictation
                  // draws it; one that didn't is what was typed, so it stays
                  // text beside the reason (KNEG-331).
                  return (
                    <Fragment key={`${b.action}-${i}`}>
                      {a.kind === "invalid" ? <code>{label.text}</code> : <span><Keycaps accelerators={[label.text]} /></span>}
                      {label.problem ? (
                        <em className="row-detail warn">{label.problem}</em>
                      ) : null}
                      {note ? (
                        <>
                          <em className="row-detail">{note.text}</em>
                          <LearnMore>
                            <p>{note.more}</p>
                          </LearnMore>
                        </>
                      ) : null}
                    </Fragment>
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
                disabled={captureBusy}
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
              <ShortcutRecorder disabled={captureBusy || hotkeys.portalEnabled}
                onBusy={setCaptureBusy}
                onCaptured={value => {
                  setDraftKeys(prev => ({ ...prev, [b.action]: value }));
                  setHotkeyResult(prev => ({ ...prev, [b.action]: "Shortcut recorded. Press Set to save." }));
                }}
                onError={message => setHotkeyResult(prev => ({ ...prev, [b.action]: message }))} />
              {/* Never disabled: a control that silently does nothing is the defect. */}
              <button type="button" disabled={captureBusy} onClick={() => applyHotkey(b)}>
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
                disabled={captureBusy}
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
          A third category, warn-coloured rather than folded under "About these
          shortcuts" below, because it is the only thing on this screen that
          says a hotkey which *works* will also do something you did not ask
          for. Binding `Control+Alt` on a Nordic or German layout starts a
          dictation every time you type `@` — the keys Windows sends for AltGr
          are the chord, and nothing downstream can tell the two apart. See
          KNEG-37.
        */}
        {hotkeys.cautions.map((c, i) => (
          <p className="caveat warn" key={`caution-${i}`}>
            <strong>{c.action}:</strong> {c.reason}
          </p>
        ))}
        {/*
          The failure half of the `Control+Super` paragraph folded below, kept
          out of the fold because it is a failure: without the hook the
          default does nothing, and that has to be read before anyone relies on
          it. The explanation of why stays in the fold.
        */}
        {hook.state === "available" || modifierOnly.length === 0 ? null : (
          <p className="caveat">
            {modifierOnly.map((accelerator, index) => (
              <Fragment key={`${index}-${accelerator}`}>{index > 0 ? ", " : null}<Keycaps accelerators={[accelerator]} /></Fragment>
            ))}{" "}
            {modifierOnly.length === 1 ? "does" : "do"} nothing on this computer
            and {modifierOnly.length === 1 ? "has" : "have"} to be changed to a
            shortcut with a key in it, such as <Keycaps accelerators={["Control+Shift+D"]} />.
          </p>
        )}
        {/*
          The evidence, directly under the controls it is evidence for: the key
          reached Rust and came back as an event, and the dictation it started
          ended the way this says. Together they are the loop the first bench
          check could not close.
        */}
        {lastHotkey ? <Row label="Last fired" value={lastHotkey} /> : null}
        {lastOutcome ? (
          <Row label="Last dictation result" value={lastOutcome} />
        ) : null}
        <LearnMore summary="About these shortcuts">
          {/* The verb follows the count -- "1 of 5 actions has", "2 of 5
              actions have" -- and the sentence after it is worded so that it
              reads the same for one action as for several. */}
          {hotkeys.unhandled.length > 0 ? (
            <p>
              {`${hotkeys.unhandled.length} of ${hotkeys.bindings.length} ${
                hotkeys.bindings.length === 1 ? "action" : "actions"
              } ${hotkeys.unhandled.length === 1 ? "has" : "have"} nothing behind ${
                hotkeys.unhandled.length === 1 ? "it" : "them"
              } yet.`}{" "}
              A shortcut for an action with nothing behind it is still saved,
              and deliberately not registered with the system — so its keys
              stay free for the applications that do something with them.
            </p>
          ) : null}
          <p>
            A shortcut takes effect as soon as you set it. The dictation default,{" "}
            <Keycaps accelerators={["Control+Super"]} />, is two modifiers and no key, which no
            cross-platform shortcut API can register.
            {hook.state === "available"
              ? ` It works here anyway, through ${hook.mechanism} — and unlike a registered shortcut it does not take the keys away from anything else, so Windows still does whatever it does with them.`
              : null}
          </p>
          <p>
            The same hooks watch the two side buttons on a mouse: type{" "}
            <code>Mouse4</code> or <code>Mouse5</code> as the accelerator. Left,
            right and middle are deliberately refused — the click is not
            swallowed, so binding one would start this action on every click in
            every application while the click still went through.
          </p>
        </LearnMore>
      </section>
    </div>
  );
}
