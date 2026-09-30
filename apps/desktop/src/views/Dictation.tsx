/**
 * Dictation: the task, and what came of it.
 *
 * Three parts, the way the canvas draws them:
 *
 *   - **The hero**, centred: whether dictation will work (`lib/readiness.ts`)
 *     and, when it will, the shortcut to press. When it will not, the one
 *     problem and the button that fixes it; while recording, "Listening…" and
 *     a timer.
 *   - **The control bar** at the foot: the microphone, chosen right here in
 *     its own select; where speech becomes text and, on the local route, the
 *     language -- each a chip that goes to Settings > Transcription, where
 *     they are set; the privacy line, which is never hidden; the last result
 *     with a Copy button; the Tidy up chip, to Cleanup & summaries; and Start.
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
import { ControlBar } from "../components/ControlBar";
import { DictationHistoryPanel } from "../components/DictationHistoryPanel";
import { Icon, IconFilled } from "../components/Icon";
import { LearnMore } from "../components/LearnMore";
import { LocalServerHealth } from "../components/LocalServerHealth";
import { PanelHeader } from "../components/Panel";
import { SplitView } from "../components/SplitView";
import { TaskHero } from "../components/TaskHero";
import { cleanupLabel, destinationName, languageLabel, privacyLines } from "../lib/controlBar";
import { timeOfDay } from "../lib/dayGroups";
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
  getReasoningStatus,
  isInsertionSupported,
  onDictationOutcome,
  onDictationState,
  onLocalDictationLanguages,
  setInputDevice,
  toggleDictation,
  type DictationOutcome,
  type SessionState,
} from "../lib/ipc";
import { acceleratorText, outcomeLabel, sessionStateLabel } from "../lib/labels";
import { lastDictation, type InsertedDictation } from "../lib/lastDictation";
import { microphoneState } from "../lib/microphone";
import type { ViewId } from "../lib/navigation";
import { dictationReadiness } from "../lib/readiness";
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
    ]);
    // Only the local route has languages to choose, so only it is asked.
    const languages = route.isLocal ? await getLocalDictationLanguages() : [];
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
      cleanup: reasoning.route,
      languages,
    };
  }, []);

  const { data, error, reload, guard } = useRefresh(load);

  /** Live session state, which arrives by event between reads. */
  const [session, setSession] = useState<SessionState | null>(null);
  const [lastOutcome, setLastOutcome] = useState<LastOutcome | null>(null);
  const [lastInserted, setLastInserted] = useState<InsertedDictation | null>(null);

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

  const { hotkeys, devices, device, canInsert, route, history, historyOn, keepRaw, cleanup, languages } =
    data;
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
  // parse is Settings > Shortcuts' to report, not the hero's to invite.
  const pressable = dictationBinding
    ? acceleratorText({
        ...dictationBinding,
        accelerators: dictationBinding.accelerators.filter((a) => a.kind !== "invalid"),
      })
    : "";
  // A microphone chosen earlier and unplugged since. Listed as itself rather
  // than letting the select fall back to "System default", which would read as
  // a choice nobody made. The recording itself does fall back to the default
  // -- `Capture::start` in crates/audio-core/src/capture.rs asks for the device
  // by name and takes the default when it is gone -- so the bar says that too.
  const missingDevice =
    device !== null && !devices.some((d) => d.name === device) ? device : null;
  // The select's `title`: the selected option's own text, so the full name
  // is there under the pointer once the chip has cut it short.
  const deviceLabel =
    missingDevice !== null
      ? `${missingDevice} (not connected)`
      : device === null
        ? "System default"
        : `${device}${devices.some((d) => d.name === device && d.isDefault) ? " (default)" : ""}`;
  const readiness = dictationReadiness({ route, hotkeys, canInsert });
  // Off on a broken route: the backend resolves the route only at Stop and
  // cancels on failure (`toggle`, src-tauri/src/dictation.rs), so Start would
  // record words that are then thrown away. A missing shortcut does not
  // disable it -- the button needs none -- so only this case hands the
  // page's one filled button to the hero's fix.
  const startBlocked = !recording && route.problem !== null;
  // The route chip's label. A route with a problem has no destination to
  // name; the hero says what to do and carries the problem itself.
  const routeLabel = route.problem
    ? "Not set up"
    : route.isLocal
      ? "On this computer"
      : destinationName(route.destination);

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
                <span className="chip-select">
                  <Icon name="microphone" size={16} />
                  {/* Controlled by what the backend reports, not by the last
                      click: `guard` re-reads after the change, and
                      `getInputDevice` is what brings the choice back.
                      Uncontrolled, this read "System default" whatever had
                      been chosen (KNEG-340). */}
                  <select
                    className="chip"
                    aria-label="Microphone"
                    title={deviceLabel}
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
                </span>
                {/* Named for what they are the value of: on its own,
                    "OpenAI" or "Swedish" is a word with nothing to say which
                    setting it is. */}
                <Chip
                  icon="computer"
                  label={routeLabel}
                  name={`Transcription: ${routeLabel}`}
                  chevron
                  onClick={() => onGoTo("transcription")}
                />
                {route.isLocal ? (
                  <Chip
                    icon="language"
                    label={languageLabel(languages)}
                    name={`Language: ${languageLabel(languages)}`}
                    chevron
                    onClick={() => onGoTo("transcription")}
                  />
                ) : null}
              </>
            }
            privacy={privacyLines(route, cleanup)}
          >
            {error ? <p className="error">{error}</p> : null}
            {missingDevice !== null ? (
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
                      place they are is here and in the history. */}
                  {last.typed ? null : (
                    <p className="error control-note">
                      This could not be typed where your cursor was. Copy it from here.
                    </p>
                  )}
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
              {lastOutcome && !recording ? (
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
              <Chip icon="tidy" label={cleanupLabel(cleanup)} onClick={() => onGoTo("intelligence")} />
              <div className="control-spacer" />
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
              <button
                type="button"
                className={startBlocked ? "control-start" : "primary control-start"}
                disabled={mic.disabled || startBlocked}
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
          </ControlBar>
          {route.isLocal ? (
            <div className="task-health">
              <LocalServerHealth />
            </div>
          ) : null}
        </div>
      </section>
    </SplitView>
  );
}
