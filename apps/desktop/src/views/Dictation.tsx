/**
 * Dictation: the task, and what came of it.
 *
 * Three parts, the way the canvas draws them:
 *
 *   - **The hero**, centred: whether dictation will work (`lib/readiness.ts`)
 *     and, when it will, the shortcut to press. When it will not, the one
 *     problem and the button that fixes it; while recording, "Listening…" and
 *     a timer.
 *   - **The control bar** at the foot: the microphone, and where speech
 *     becomes text -- an installed model on this computer or a cloud
 *     provider -- each chosen right here in its chip's menu (`ChipMenu`);
 *     on the local whisper route, the languages, in a panel holding the
 *     picker Settings > Transcription shows (that page keeps its own
 *     controls for the route and the languages). Those two are held while a
 *     dictation is under way, because Rust reads them at Stop. Then the
 *     privacy line, which is never hidden and follows a switch as soon as it
 *     lands; the last result with a Copy button; the Tidy up chip -- a switch
 *     once cleanup has been set up in Cleanup & summaries (held during a
 *     dictation too, since it applies to the running session at once, and
 *     saying under itself when it cannot run), until then the way there; and
 *     Start, which waits while a route switch or the Tidy up write is applied.
 *   - **The history**, in the list panel (`components/DictationHistoryPanel.tsx`),
 *     with the way to the shortcut editor and the vocabulary at its top.
 *
 * The loop this page used to hold on one screen -- bind a key, press it, read
 * the result -- still has to hold across the two screens it is split over.
 * Putting the evidence that dictation worked on a different page from the
 * control that started it is how the first bench check ended up unable to
 * tell "the hotkey did nothing" from "the hotkey saved nothing". So the
 * evidence travels with each half: this page shows the last result above the
 * button that started it, and Settings > Shortcuts shows *Last fired* and
 * *Last dictation result* under the editor.
 *
 * Dictation state arrives from Rust by event rather than being tracked here: the
 * hotkey works while this window is hidden, so the window cannot be the thing
 * that knows. The rail carries no recording mark; the hero says it.
 *
 * Rendered by `App.tsx` straight into the card, with no `PageHeader`: the
 * panel's title is the page's `h1` -- `SplitView`'s `heading` stands in for
 * it while the panel is collapsed -- and the intro sentence that used to sit
 * under a header is the hero's own line under the shortcut.
 */

import { Fragment, useCallback, useEffect, useRef, useState, type ReactNode } from "react";

import { Chip } from "../components/Chip";
import { ChipMenu } from "../components/ChipMenu";
import { ControlBar } from "../components/ControlBar";
import { DictationHistoryPanel } from "../components/DictationHistoryPanel";
import { Icon, IconFilled } from "../components/Icon";
import { LearnMore } from "../components/LearnMore";
import { LocalLanguagePicker } from "../components/LocalLanguagePicker";
import { LocalServerHealth } from "../components/LocalServerHealth";
import { PanelHeader } from "../components/Panel";
import { SplitView } from "../components/SplitView";
import { TaskHero } from "../components/TaskHero";
import { cleanupConfigured, effectiveCleanup } from "../lib/cleanup";
import { cleanupLabel, destinationName, languageLabel, microphoneMenu, privacyLines } from "../lib/controlBar";
import { timeOfDay } from "../lib/dayGroups";
import { localEngine, routeBusyLabel, routeHeld, routeMenu, type RouteChoice } from "../lib/dictationRoute";
import {
  cancelDictation,
  getDictationRoute,
  getDictationState,
  getHistory,
  getHistoryEnabled,
  getHotkeys,
  getInputDevice,
  getInputDevices,
  getKeepRawText,
  getLocalDictationLanguages,
  getLocalStatus,
  getProviders,
  getReasoningStatus,
  getSherpaStatus,
  isInsertionSupported,
  onDictationOutcome,
  onDictationState,
  onLocalDictationLanguages,
  setCleanup,
  setInputDevice,
  toggleDictation,
  type DictationOutcome,
  type SessionState,
} from "../lib/ipc";
import { acceleratorText, outcomeLabel, pressableAccelerators, sessionStateLabel } from "../lib/labels";
import { lastDictation, type InsertedDictation } from "../lib/lastDictation";
import { microphoneState } from "../lib/microphone";
import type { ViewId } from "../lib/navigation";
import { dictationReadiness } from "../lib/readiness";
import { chooseRoute } from "../lib/routeChoice";
import { useRefresh } from "../lib/useRefresh";

/**
 * The sentence that was this page's `PageHeader` intro (`lib/pages.ts`), now
 * the hero's line under whatever it is asking you to do.
 */
const INTRO = "Speak, and your words are typed where your cursor is.";

/** How long "Copied" stands in for the Copy button's label. */
const COPIED_MS = 2000;

/** Seconds as `mm:ss`, the canvas' recording clock. */
function clock(seconds: number): string {
  const s = Math.max(0, Math.floor(seconds));
  return `${String(Math.floor(s / 60)).padStart(2, "0")}:${String(s % 60).padStart(2, "0")}`;
}

/**
 * A shortcut as keycaps: `Control+Shift+D` is three, and a second
 * accelerator on the same action is joined with "or". Built from
 * `acceleratorText`, the same string Settings > Shortcuts and the panel show.
 */
function keycaps(shortcut: string): ReactNode {
  return shortcut.split(", ").map((accelerator, i) => (
    <Fragment key={accelerator}>
      {i > 0 ? " or " : null}
      <span className="keycaps">
        {accelerator.split("+").map((key) => (
          <kbd key={key}>{key}</kbd>
        ))}
      </span>
    </Fragment>
  ));
}

/**
 * What the last dictation event said: its sentence under the result, and
 * whether it failed. The text of an insert is not kept here but in its own
 * state (`InsertedDictation`), so a later Discard or failure replaces this
 * without taking the last inserted text with it.
 */
type LastOutcome = {
  label: string;
  failed: boolean;
  /**
   * The history row the outcome wrote, for `inserted` and `failed`; `null`
   * when history is off or the outcome writes none. A `failed` outcome
   * carries no text, but its row does when only the paste failed.
   */
  historyId: number | null;
  at: Date;
};

export function DictationView({
  onGoTo,
}: {
  /** Where the fix button, the chips and the panel's actions take you. */
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
      reasoning,
      providers,
      local,
      sherpa,
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
      getReasoningStatus(),
      // What the route menu lists, read as Settings > Transcription reads it.
      getProviders(),
      getLocalStatus(),
      getSherpaStatus(),
    ]);
    // Only local whisper has languages to choose -- a Parakeet model uses its
    // own -- so only it is asked.
    const languages = localEngine(route, sherpa) === "whisper" ? await getLocalDictationLanguages() : [];
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
      // The whole status, not only where cleanup goes: the Tidy up toggle
      // writes from what is stored (`effectiveCleanup`).
      reasoning,
      languages,
      providers,
      local,
      sherpa,
    };
  }, []);

  const { data, error, reload, guard } = useRefresh(load);

  /** Live session state, which arrives by event between reads. */
  const [session, setSession] = useState<SessionState | null>(null);
  const [lastOutcome, setLastOutcome] = useState<LastOutcome | null>(null);
  const [lastInserted, setLastInserted] = useState<InsertedDictation | null>(null);
  /**
   * The route switch being applied, from its choice until the re-read after
   * it. The chip says what is starting, and Start waits: the backend resolves
   * the route at Stop, so a dictation started mid-switch could end on a route
   * nobody chose -- the server up, the route not yet pointed at it. Stop and
   * Discard never wait on it: no switch can begin during a dictation (the
   * chips are held), and one begun before it, by the hotkey starting a
   * dictation mid-switch, must not keep the microphone open.
   */
  const [switching, setSwitching] = useState<RouteChoice | null>(null);
  /** The Tidy up write in flight; Start waits on it as on a switch. */
  const [tidying, setTidying] = useState(false);

  // `disposed` because `listen` resolves after the effect returns: a page left
  // before it did would otherwise keep its listeners for the rest of the
  // session -- the same guard Settings > Meetings uses for its one listener.
  useEffect(() => {
    let disposed = false;
    const unlisteners: Array<() => void> = [];
    const keep = (unlisten: () => void) => {
      if (disposed) unlisten();
      else unlisteners.push(unlisten);
    };
    void onDictationState((next) => {
      if (!disposed) setSession(next);
    }).then(keep);
    void onDictationOutcome((outcome: DictationOutcome) => {
      if (disposed) return;
      const at = new Date();
      setLastOutcome({
        label: outcomeLabel(outcome),
        failed: outcome.outcome === "failed",
        historyId:
          outcome.outcome === "inserted" || outcome.outcome === "failed" ? outcome.historyId : null,
        at,
      });
      if (outcome.outcome === "inserted") {
        setLastInserted({ text: outcome.text, historyId: outcome.historyId, at });
      }
      // History and the session both moved, so re-read rather than guessing.
      void reload();
    }).then(keep);
    // The floating microphone's language menu changes this too, while the
    // page is open; the chip re-reads rather than going stale.
    void onLocalDictationLanguages(() => {
      if (!disposed) void reload();
    }).then(keep);
    return () => {
      disposed = true;
      unlisteners.forEach((fn) => fn());
    };
  }, [reload]);

  const liveState = session ?? data?.session ?? null;
  const recordingNow = liveState === "recording";

  // A local timer from the moment this page saw the session go to
  // "recording". Local, because Rust sends no start time: a page opened
  // mid-recording counts from when it opened.
  const [recordingSince, setRecordingSince] = useState<number | null>(null);
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    if (!recordingNow) {
      setRecordingSince(null);
      return;
    }
    const start = Date.now();
    setRecordingSince(start);
    setNow(start);
    const timer = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(timer);
  }, [recordingNow]);

  // The Copy button's answer, and the text it was about. "Couldn't copy"
  // stays until the next press, because a failure stays visible; "Copied"
  // goes after two seconds. Either one lapses when a new dictation replaces
  // the text it was about.
  const [copied, setCopied] = useState<{ text: string; label: "Copied" | "Couldn't copy" } | null>(null);
  const copiedTimer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  useEffect(() => () => clearTimeout(copiedTimer.current), []);

  if (!data) {
    return (
      <SplitView label="Dictation history" heading="Dictation" panel={<PanelHeader title="Dictation" />}>
        <section data-surface="dictation.task" className="task" aria-label="Dictation">
          {error ? <p className="error">{error}</p> : <p className="muted">Loading…</p>}
        </section>
      </SplitView>
    );
  }

  const {
    hotkeys,
    devices,
    device,
    canInsert,
    route,
    history,
    historyOn,
    keepRaw,
    reasoning,
    languages,
    providers,
    local,
    sherpa,
  } = data;
  const cleanup = reasoning.route;
  const state = liveState ?? data.session;
  // The floating microphone's own reading of the session, reused rather than
  // re-derived, so the two buttons cannot disagree -- and so Start is off
  // while a dictation is still transcribing or being typed, which this button
  // alone used to allow (KNEG-342).
  const mic = microphoneState(state, false);
  const recording = mic.recording;
  const dictationBinding = hotkeys.bindings.find((b) => b.action === "dictation");
  const shortcut = dictationBinding ? acceleratorText(dictationBinding) : "";
  // Keycaps name only what can be pressed. An accelerator Rust could not
  // parse, or could not register, is Settings > Shortcuts' to report, not the
  // hero's to invite (`pressableAccelerators`).
  const pressable = dictationBinding
    ? acceleratorText({
        ...dictationBinding,
        accelerators: pressableAccelerators(dictationBinding, hotkeys.registrations),
      })
    : "";
  // The microphone chip and its menu, including one chosen earlier and
  // unplugged since (`microphoneMenu` says why it is listed as itself).
  const microphone = microphoneMenu(devices, device);
  const readiness = dictationReadiness({ route, hotkeys, canInsert });
  // Off on a broken route: the backend resolves the route only at Stop and
  // cancels on failure (`toggle`, src-tauri/src/dictation.rs), so Start would
  // record words that are then thrown away. A missing shortcut does not
  // disable it -- the button needs none -- so only this case hands the
  // page's one filled button to the hero's fix.
  const startBlocked = !recording && route.problem !== null;
  // Start, never Stop, waits for a route switch (see `switching`).
  const startWaits = !recording && (switching !== null || tidying);
  // The route and the language are held from Start until the dictation is
  // over (`routeHeld` says why); the chips keep saying what is in effect. The
  // microphone is not: it never was, as a select, before it became a menu.
  const held = routeHeld(state);
  // The route chip's menu (`lib/dictationRoute.ts`): the installed models
  // and the providers, which one the route resolves to, and what choosing
  // each writes. A choice is written and everything is read back (`guard`),
  // so the privacy line, the hero and the local server's health follow the
  // route as Rust resolves it; the row already in effect writes nothing.
  const routes = routeMenu(providers, local, sherpa, route);
  const engine = localEngine(route, sherpa);
  const chooseRouteRow = (id: string) => {
    const choice = routes.choices.get(id);
    // One switch at a time: a provider's is not `busy`, so its chip can be
    // reopened while the write is still out.
    if (choice === undefined || switching !== null || held !== null) return;
    setSwitching(choice);
    // A switch that fails part-way may already have stopped the route it
    // left, so the page is re-read before the error is shown: the chip and
    // the menu must not keep marking a route that is no longer running.
    void guard(async () => {
      try {
        await chooseRoute(choice);
      } catch (cause) {
        await reload();
        throw cause;
      }
    }).finally(() => setSwitching(null));
  };
  // Tidy up is a switch once a provider or a URL is stored; until then it is
  // the way to set one up. The switch writes the stored setting with only
  // `enabled` changed (`effectiveCleanup`), which Rust resolves first and
  // refuses if it cannot (the page's error line says so, and the chip stays as
  // it was), and applies to the running session at once -- so it is held, as
  // the route is, until a dictation is over: a text already on its way would
  // change destination after the privacy line was read. The privacy line
  // follows the write, as `guard` re-reads.
  const tidyConfigured = cleanupConfigured(reasoning.settings);
  const toggleCleanup = () => {
    if (tidying || held !== null) return;
    setTidying(true);
    void guard(() =>
      setCleanup(effectiveCleanup(reasoning.settings, reasoning.providers, !reasoning.settings.enabled)),
    ).finally(() => setTidying(false));
  };
  // Why a stored setup cannot run, for the line under the chip: a caveat on a
  // route that resolves (no key), or a setup that no longer resolves.
  const tidyCaveat =
    cleanup.state === "ready" ? cleanup.caveat : cleanup.state === "problem" ? cleanup.reason : null;
  // The route chip's label: the row in effect, by its own words. Failing
  // that, a route with a problem has no destination to name -- the hero says
  // what to do and carries the problem itself -- and a local route with no
  // model running, or a destination the menu does not list, is said plainly.
  const routeLabel =
    routes.current?.label ??
    (route.problem
      ? "Not set up"
      : route.isLocal
        ? "On this computer"
        : destinationName(route.destination));

  // The last thing dictated: this session's insert, a failure whose row kept
  // the text, or the newest history row with text -- whichever is newest
  // (`lib/lastDictation.ts` says how, and why the insert is its own state).
  const last = lastDictation(
    history,
    lastInserted,
    lastOutcome?.failed ? { historyId: lastOutcome.historyId, at: lastOutcome.at } : null,
  );
  const lastText = last?.text ?? null;
  // Said to a screen reader as well as on the button, whose changed label
  // it may not announce.
  const copyNotice = copied !== null && copied.text === lastText ? copied.label : null;

  const copyLast = async (text: string) => {
    clearTimeout(copiedTimer.current);
    try {
      await navigator.clipboard.writeText(text);
      setCopied({ text, label: "Copied" });
      copiedTimer.current = setTimeout(() => setCopied(null), COPIED_MS);
    } catch {
      setCopied({ text, label: "Couldn't copy" });
    }
  };

  // The hero. Readiness answers "will starting work?", which means nothing
  // once a dictation has started -- "Ready" over a recording, or over text
  // being typed, reads as a claim about the session -- so while one is under
  // way the hero speaks for the session instead.
  const hero = recording ? (
    <TaskHero
      icon="dictation"
      status={{
        tone: "recording",
        label: sessionStateLabel(state),
        extra: clock(recordingSince === null ? 0 : (now - recordingSince) / 1000),
      }}
      heading="Listening…"
      sub="Speak now, then press Stop and transcribe. The overlay is visible while you record."
    />
  ) : mic.busy ? (
    <TaskHero
      icon="dictation"
      status={{ tone: "busy", label: sessionStateLabel(state) }}
      heading="Your words are on their way"
      sub={INTRO}
    />
  ) : readiness.state === "ready" ? (
    <TaskHero
      icon="dictation"
      status={{ tone: "ready", label: "Ready" }}
      heading={<>Press {keycaps(pressable)} to dictate</>}
      sub={
        <>
          <p>{INTRO}</p>
          {readiness.caution ? <p className="hero-note">{readiness.caution}</p> : null}
        </>
      }
    />
  ) : (
    <TaskHero
      icon={readiness.fix.view === "shortcuts" ? "shortcut" : "transcription"}
      status={{ tone: "setup", label: "Needs setup" }}
      heading={readiness.problem}
      sub={INTRO}
    >
      {/* The fix takes the page's one filled button only when Start cannot
          be pressed; otherwise Start keeps it and this stays plain. */}
      <button
        type="button"
        className={startBlocked ? "primary" : undefined}
        onClick={() => onGoTo(readiness.fix.view)}
      >
        {readiness.fix.label}
      </button>
      {readiness.detail ? <LearnMore>{readiness.detail}</LearnMore> : null}
    </TaskHero>
  );

  return (
    <SplitView
      label="Dictation history"
      heading="Dictation"
      panel={
        <DictationHistoryPanel
          history={history}
          historyOn={historyOn}
          keepRaw={keepRaw}
          shortcut={shortcut}
          onGoTo={onGoTo}
          guard={guard}
          reload={reload}
        />
      }
    >
      <section data-surface="dictation.task" className="task" aria-label="Dictation">
        <div className="task-hero">{hero}</div>

        <div className="task-bar">
          <ControlBar
            strip={
              <>
                {/* What is current is what the backend reports, not the
                    last click: `guard` re-reads after the change, and
                    `getInputDevice` is what brings the choice back. The
                    select here before, while it was uncontrolled, read
                    "System default" whatever had been chosen (KNEG-340). */}
                <ChipMenu
                  icon="microphone"
                  name="Microphone"
                  label={microphone.label}
                  options={microphone.options}
                  onChoose={(id) => {
                    void guard(() => setInputDevice(id || null));
                  }}
                />
                {/* Named for what they are the value of: on its own,
                    "OpenAI" or "Swedish" is a word with nothing to say which
                    setting it is. Settings > Transcription keeps its own
                    controls for both, and installing a model is only there. */}
                <ChipMenu
                  icon="computer"
                  name="Transcription"
                  label={routeLabel}
                  busy={routeBusyLabel(switching)}
                  held={held}
                  options={routes.options}
                  onChoose={chooseRouteRow}
                  footer={{ label: "More in Settings…", onClick: () => onGoTo("transcription") }}
                />
                {/* Local whisper only: a Parakeet model uses its own
                    languages, and Settings disables the picker under one.
                    The picker writes, validates and announces the choice
                    itself, and the page re-reads on that announcement. It
                    starts from what this page read, so the panel opens on
                    its first control rather than on a picker still loading. */}
                {engine === "whisper" ? (
                  <ChipMenu icon="language" name="Language" label={languageLabel(languages)} held={held}>
                    <LocalLanguagePicker initial={languages} />
                  </ChipMenu>
                ) : null}
              </>
            }
            privacy={privacyLines(route, cleanup)}
          >
            {error ? <p className="error">{error}</p> : null}
            {microphone.missing !== null ? (
              <p className="control-note">
                Not connected, so dictation uses the system default until it is
                plugged back in.
              </p>
            ) : null}

            <div className="control-result">
              {recording ? (
                <p className="control-result-empty">Your words appear here when you stop.</p>
              ) : last !== null ? (
                <>
                  <div className="control-result-head">
                    <span className="control-result-label">
                      Last dictation{last.at ? ` · ${timeOfDay(last.at)}` : ""}
                    </span>
                    <button
                      type="button"
                      className="control-copy"
                      onClick={() => {
                        void copyLast(last.text);
                      }}
                    >
                      <Icon name="copy" size={14} />
                      {copyNotice ?? "Copy"}
                    </button>
                  </div>
                  <p className="control-result-text">{last.text}</p>
                  {/* A failure, so on screen: the words exist, and the one
                      place they are is here and in the history. When it is
                      the last outcome's own failure, the outcome's sentence
                      is the same event, so it is folded here rather than
                      drawn as a second red line below (KNEG-377). */}
                  {last.typed ? null : (
                    <p className="error control-note">
                      This could not be typed where your cursor was. Copy it from here.
                    </p>
                  )}
                  {last.fromFailure && lastOutcome?.failed ? (
                    <LearnMore>
                      <p>{lastOutcome.label}</p>
                    </LearnMore>
                  ) : null}
                  {/* Only ever present when cleanup changed the text and the
                      user asked to keep it -- see `raw_for_history`. So its
                      presence is the finding, and it is labelled rather than
                      shown as a bare second line. */}
                  {last.raw ? <p className="control-note">Before cleanup: {last.raw}</p> : null}
                </>
              ) : (
                <p className="control-result-empty">Nothing dictated yet.</p>
              )}
              {/* The last event's own sentence -- "Inserted 42 characters",
                  or why it failed -- under the text, because a failure is
                  never folded. */}
              {lastOutcome && !recording && !(last?.fromFailure && lastOutcome.failed) ? (
                <p className={lastOutcome.failed ? "error control-note" : "control-note"}>
                  {lastOutcome.label}
                </p>
              ) : null}
              {/* Always rendered, so the region exists before the notice
                  arrives in it -- a live region added along with its text is
                  often not read. */}
              <p className="sr-only" role="status">{copyNotice ?? ""}</p>
            </div>

            <div className="control-row">
              {tidyConfigured ? (
                <Chip
                  icon="tidy"
                  label={cleanupLabel(cleanup)}
                  pressed={reasoning.settings.enabled}
                  onClick={toggleCleanup}
                  held={held}
                />
              ) : (
                <Chip icon="tidy" label={cleanupLabel(cleanup)} chevron onClick={() => onGoTo("intelligence")} />
              )}
              <div className="control-spacer" />
              {recording ? (
                <button
                  type="button"
                  className="secondary"
                  onClick={() => {
                    void guard(cancelDictation);
                  }}
                >
                  <Icon name="discard" size={16} />
                  Discard
                </button>
              ) : null}
              <button
                type="button"
                className={startBlocked ? "control-start" : "primary control-start"}
                disabled={mic.disabled || startBlocked || startWaits}
                onClick={() => {
                  void guard(toggleDictation);
                }}
              >
                {recording ? <IconFilled name="stop" size={14} /> : <Icon name="microphone" size={16} />}
                {recording
                  ? "Stop and transcribe"
                  : mic.busy
                    ? sessionStateLabel(state)
                    : "Start dictation"}
              </button>
            </div>
            {/* Under the chip, and not only in its label: "needs setup" says
                that it cannot run, this says why and where it is fixed. */}
            {/* The live region is always rendered, so it exists before the
                caveat arrives in it -- one added along with its text is often
                not read. */}
            <div role="status">
              {tidyConfigured && tidyCaveat !== null ? (
                <p className="control-note">
                  Tidy up can't run yet: {tidyCaveat}{" "}
                  <button type="button" className="provider-link" onClick={() => onGoTo("intelligence")}>
                    Open Cleanup &amp; summaries
                  </button>
                </p>
              ) : null}
            </div>
          </ControlBar>
          {/* The whisper server's health, so on its route alone: under a
              Parakeet model it describes a server dictation is not using.
              Plain: nothing while it works, and no model id or port
              (KNEG-357). Diagnostics shows the whole sentence. */}
          {engine === "whisper" ? (
            <div className="task-health">
              <LocalServerHealth plain />
            </div>
          ) : null}
        </div>
      </section>
    </SplitView>
  );
}
