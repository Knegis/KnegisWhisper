import { TaskLoading } from "../components/TaskLoading";
/**
 * Dictation: the task, and what came of it.
 *
 * Top to bottom, the way the canvas draws it:
 *
 *   - **The status notice** (`lib/dictationNotice.ts`): whether dictation will
 *     work and which microphone it would use, or that it is recording.
 *   - **The title**, centred: the shortcut to press (`lib/readiness.ts`). When
 *     dictation will not work, the one problem; while recording, "Listening…"
 *     and the microphone's live level (`LevelMeter`, KNEG-371).
 *   - **The white card** (`ControlBar`): the microphone, and where speech
 *     becomes text -- an installed model on this computer or a cloud
 *     provider -- each chosen right here in its chip's menu (`ChipMenu`);
 *     on every route but a Parakeet model's, the languages, in a panel
 *     holding the picker Settings > Transcription shows, with a line saying
 *     what the route does with them (`languageNote`, KNEG-370; that page
 *     keeps its own controls for the route and the languages). Those two
 *     are held while a dictation is under way, because Rust reads them at
 *     Stop. Then the
 *     privacy line, which is never hidden and follows a switch as soon as it
 *     lands; the last result with a Copy button; and Tidy up -- a switch
 *     once cleanup has been set up in Cleanup & summaries (held during a
 *     dictation too, since it applies to the running session at once, and
 *     saying under itself when it cannot run), until then the way there.
 *   - **The picture tile** beside it: the state in a word, the orb breathing at
 *     the state's pace, and the page's button -- Start, which waits while a
 *     route switch or the Tidy up write is applied, or Discard and Stop.
 *   - **The history** under them (`components/DictationHistory.tsx`), with the
 *     way to the shortcut editor and the vocabulary at its head.
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
 * that knows. The title bar's Dictation pill carries no recording mark; the notice says it.
 *
 * Rendered by `App.tsx` straight into the card, with no `PageHeader`: the title
 * is the page's `h1`, and this page scrolls in its own `.split-main`.
 */

import { useCallback, useEffect, useRef, useState } from "react";

import { Chip } from "../components/Chip";
import { ChipMenu } from "../components/ChipMenu";
import { ControlBar } from "../components/ControlBar";
import { DictationHistory } from "../components/DictationHistory";
import { Icon } from "../components/Icon";
import { Keycaps } from "../components/Keycaps";
import { LearnMore } from "../components/LearnMore";
import { LevelMeter } from "../components/LevelMeter";
import { LocalLanguagePicker } from "../components/LocalLanguagePicker";
import { LocalServerHealth } from "../components/LocalServerHealth";
import { SetupGuide } from "../components/SetupGuide";
import { StatusNotice } from "../components/StatusNotice";
import { cleanupConfigured, effectiveCleanup } from "../lib/cleanup";
import {
  cleanupLabel,
  destinationName,
  languageLabel,
  languageNote,
  microphoneMenu,
  privacyLines,
} from "../lib/controlBar";
import { timeOfDay } from "../lib/dayGroups";
import { dictationStatus } from "../lib/dictationNotice";
import {
  languagesApply,
  localEngine,
  routeBusyLabel,
  routeHeld,
  routeMenu,
  type RouteChoice,
} from "../lib/dictationRoute";
import {
  cancelDictation,
  getDictationRoute,
  getDictationState,
  getHistory,
  dismissSetupGuide,
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
  isSetupGuidePending,
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
import { acceleratorLabel, acceleratorText, outcomeLabel, pressableAccelerators, sessionStateLabel } from "../lib/labels";
import { lastDictation, type InsertedDictation } from "../lib/lastDictation";
import { microphoneState } from "../lib/microphone";
import type { ViewId } from "../lib/navigation";
import { dictationReadiness } from "../lib/readiness";
import { chooseRoute } from "../lib/routeChoice";
import { guideShown } from "../lib/setupGuide";
import { useRefresh } from "../lib/useRefresh";

/**
 * The sentence that was this page's `PageHeader` intro (`lib/pages.ts`), now
 * the line under the title, whatever it is asking you to do.
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
      setupPending,
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
      // Whether the first-run guide is still to be shown (KNEG-329). A failed
      // read shows no guide: the page works without it.
      isSetupGuidePending().catch(() => false),
    ]);
    // Every route but a Parakeet model's has languages to choose -- that model
    // uses its own -- so the languages are asked for unless it is in use.
    const languages = languagesApply(localEngine(route, sherpa)) ? await getLocalDictationLanguages() : [];
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
      setupPending,
    };
  }, []);

  const { data, error, reload, guard } = useRefresh(load);

  /** Live session state, which arrives by event between reads. */
  const [session, setSession] = useState<SessionState | null>(null);
  const [lastOutcome, setLastOutcome] = useState<LastOutcome | null>(null);
  const [lastInserted, setLastInserted] = useState<InsertedDictation | null>(null);
  /** The setup guide was closed in this visit; the stored answer is read on the next. */
  const [guideClosed, setGuideClosed] = useState(false);
  const [guideWasShown, setGuideWasShown] = useState(false);
  useEffect(() => {
    if (data?.setupPending) setGuideWasShown(true);
  }, [data?.setupPending]);
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

  if (!data) return <TaskLoading task="dictation" error={error} />;

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
    ? pressableAccelerators(dictationBinding, hotkeys.registrations).map((a) => acceleratorLabel(a).text)
    : [];
  // The microphone chip and its menu, including one chosen earlier and
  // unplugged since (`microphoneMenu` says why it is listed as itself).
  const microphone = microphoneMenu(devices, device);
  const readiness = dictationReadiness({ route, hotkeys, canInsert, providers });
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
  // The setup guide's "Use <service> for dictation": the same write the route
  // chip makes for that service, found by what it writes rather than by the
  // menu's row id.
  const useService = (providerId: string) => {
    for (const [id, choice] of routes.choices) {
      if (choice.kind === "cloud" && choice.provider === providerId) {
        chooseRouteRow(id);
        return;
      }
    }
  };
  // Once shown in this visit, keep the guide until it is closed (KNEG-329):
  // the first history row ends setupPending, but its result needs to stay
  // readable here. Closing is written where the settings gate looks for it.
  const showGuide = guideShown(data.setupPending, guideWasShown, guideClosed);
  const closeGuide = () => {
    setGuideClosed(true);
    void guard(dismissSetupGuide);
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
  // The switch's word, and "needs setup" where it cannot run: the switch alone
  // says on or off, which is what is stored, not what will happen.
  const tidyLabel = cleanupLabel(cleanup) === "Tidy up: needs setup" ? "Tidy up: needs setup" : "Tidy up";
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

  // The title. Readiness answers "will starting work?", which means nothing
  // once a dictation has started -- "Ready" over a recording, or over text
  // being typed, reads as a claim about the session -- so while one is under
  // way the page speaks for the session instead.
  const elapsed = clock(recordingSince === null ? 0 : (now - recordingSince) / 1000);
  const status = dictationStatus({
    recording,
    busyLabel: mic.busy ? sessionStateLabel(state) : null,
    ready: readiness.state === "ready",
    microphone: microphone.label,
    microphoneMissing: microphone.missing !== null,
    elapsed,
  });

  const hero = recording ? (
    <>
      <h1 id="dictation-title" className="page-title title-ink">Listening…</h1>
      <div className="hero-meter"><LevelMeter session="dictation" source="microphone" segments={9} className="level-wave" /></div>
      <p className="page-sub">Speak now, then press Stop and transcribe. The overlay is visible while you record.</p>
    </>
  ) : mic.busy ? (
    <>
      <h1 id="dictation-title" className="page-title title-ink">Your words are on their way</h1>
      <p className="page-sub">{INTRO}</p>
    </>
  ) : readiness.state === "ready" ? (
    <>
      <h1 id="dictation-title" className="page-title page-title-keys">
        <span className="title-ink">Press</span>{" "}
        <Keycaps accelerators={pressable} />{" "}
        <span className="title-ink">to dictate</span>
      </h1>
      <p className="page-sub">{INTRO}</p>
      {readiness.caution ? <p className="hero-note">{readiness.caution}</p> : null}
    </>
  ) : (
    <>
      {/* A sentence rather than three words, so it steps down a size. */}
      <h1 id="dictation-title" className="page-title page-title-long title-ink">{readiness.problem}</h1>
      <p className="page-sub">{INTRO}</p>
      {readiness.detail ? <LearnMore>{readiness.detail}</LearnMore> : null}
    </>
  );

  return (
    <div className="split-main">
      <div className="task-page task-arrival">
        <div className="task-notice">
          <StatusNotice tone={status.tone} {...status.notice} />
        </div>
        <section data-surface="dictation.task" className="task" aria-labelledby="dictation-title">
          <div className="task-hero" data-tone={status.tone}>{hero}</div>

          {showGuide ? (
            <SetupGuide
              route={route}
              providers={providers}
              choices={routes.choices}
              pressable={pressable}
              tried={lastInserted?.text ?? null}
              canInsert={canInsert}
              onOpenTranscription={() => onGoTo("transcription")}
              onUseService={useService}
              onKeySaved={reload}
              onClose={closeGuide}
            />
          ) : null}

          <div className="task-grid">
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
                {/* Local whisper and cloud alike (KNEG-370): not under a
                    Parakeet model, which uses its own languages, and
                    Settings disables the picker under one. The picker
                    writes, validates and announces the choice itself, and
                    the page re-reads on that announcement -- the route too,
                    so the note says what is now sent. It starts from what
                    this page read, so the panel opens on its first control
                    rather than on a picker still loading. */}
                {languagesApply(engine) ? (
                  <ChipMenu icon="language" name="Language" label={languageLabel(languages)} held={held}>
                    <LocalLanguagePicker initial={languages} note={languageNote(engine, route, languages)} />
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
                      last dictation{last.at ? ` · ${timeOfDay(last.at)}` : ""}
                    </span>
                    <button
                      type="button"
                      className="control-copy"
                      onClick={() => {
                        void copyLast(last.text);
                      }}
                    >
                      <Icon name="copy" size={16} />
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
                  {last.raw ? <p className="control-note"><em>Before cleanup:</em> {last.raw}</p> : null}
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
                <label className="switch-row control-tidy">
                  <span className="control-tidy-label"><Icon name="tidy" size={16} />{tidyLabel}</span>
                  {/* `aria-disabled` while held, as the chips are, so a keyboard
                      user on it keeps focus when the hold starts. */}
                  <input
                    type="checkbox"
                    role="switch"
                    checked={reasoning.settings.enabled}
                    onChange={toggleCleanup}
                    aria-disabled={held !== null ? true : undefined}
                    aria-description={held ?? undefined}
                    {...(held === null ? {} : { title: held })}
                  />
                </label>
              ) : (
                <Chip icon="tidy" label={cleanupLabel(cleanup)} chevron onClick={() => onGoTo("intelligence")} />
              )}
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

          <div className="picture-tile tile-lavender" data-tone={status.tone}>
            <div className="tile-orb" aria-hidden="true" />
            <span className="tile-label"><span className="tile-label-dot" aria-hidden="true" />{status.tile}</span>
            <div className="tile-actions">
              {/* The fix takes the filled button only when Start cannot be
                  pressed; otherwise Start keeps it and this stays plain. */}
              {readiness.state === "needs-setup" && !recording && !mic.busy ? (
                <button
                  type="button"
                  className={startBlocked ? "tile-cta primary" : "glass-plain"}
                  onClick={() => onGoTo(readiness.fix.view)}
                >
                  {readiness.fix.label}
                </button>
              ) : null}
              {recording ? (
                <button
                  type="button"
                  className="glass-plain"
                  onClick={() => {
                    void guard(cancelDictation);
                  }}
                >
                  <Icon name="discard" size={16} />
                  Discard
                </button>
              ) : null}
              {mic.busy && !recording ? (
                <button type="button" className="tile-cta primary" data-busy="true" disabled>
                  {sessionStateLabel(state)}
                </button>
              ) : (
                <button
                  type="button"
                  className={recording ? "glass-action glass-action-signal control-start" : "glass-action control-start"}
                  disabled={mic.disabled || startBlocked || startWaits}
                  onClick={() => {
                    void guard(toggleDictation);
                  }}
                >
                  {recording ? "Stop and transcribe" : "Start dictation"}
                  <span className="glass-action-end" aria-hidden="true">
                    {recording ? <span className="glass-action-stop" /> : <Icon name="microphone" size={18} />}
                  </span>
                </button>
              )}
            </div>
          </div>
          </div>

          {/* The whisper server's health, so on its route alone: under a
              Parakeet model it describes a server dictation is not using.
              Plain: nothing while it works, and no model id or port
              (KNEG-357). Diagnostics shows the whole sentence. */}
          {engine === "whisper" ? (
            <div className="task-health">
              <LocalServerHealth plain />
            </div>
          ) : null}
        </section>

        <DictationHistory
          history={history}
          historyOn={historyOn}
          keepRaw={keepRaw}
          shortcut={shortcut}
          onGoTo={onGoTo}
          guard={guard}
          reload={reload}
        />
      </div>
    </div>
  );
}
