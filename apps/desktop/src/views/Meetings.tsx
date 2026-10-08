/**
 * Meetings: the recording task, the meeting under way, and the meetings
 * behind it.
 *
 * Three parts, the way the canvas draws them:
 *
 *   - **Idle**, a task page like Dictation's. The hero says whether a meeting
 *     can be recorded (`lib/readiness.ts`) and, when it cannot, the one
 *     problem and the button that fixes it. Under it, a card while a meeting
 *     application is in a call, with a way to record that call. At the foot,
 *     the control bar: the microphone, chosen in its chip's menu and shared
 *     with dictation, whose footer runs a five-second "Test microphone"
 *     (KNEG-364) -- whether the other participants are captured, a choice
 *     kept on this page only, who transcribes in what language, each chosen in
 *     its chip's menu (`ChipMenu`), whether a summary is made, a toggle
 *     that is a link to Cleanup & summaries until a provider and model are
 *     chosen there, and the folder the note is saved to. Then where the audio
 *     goes, which is never hidden, the caveats, what the microphone check
 *     heard, the meeting's name, and Start.
 *     The name and the folder go to Rust with Start and stay with the meeting
 *     (KNEG-365); the page keeps neither for a meeting already running.
 *     While Settings > Meetings deletes each recording once its transcript
 *     is saved, a note beside Start and the list's foot say so, and the live
 *     view says it of a meeting started that way (KNEG-409).
 *   - **Live**, while a meeting is running: its state and a timer, which
 *     captures opened and where the audio goes, how loud each capture is
 *     ("You", and "Others" with system audio; `LevelMeter`, KNEG-371), the
 *     live transcript, and a bar with what is in effect -- the microphone the
 *     meeting actually opened, from Rust, not the one chosen now (KNEG-364)
 *     -- Discard, and Stop.
 *   - **The list panel**: the meeting under way, the meetings saved lately --
 *     each opens its note in Notes -- and any recording an earlier session
 *     left behind, with Recover.
 *
 * Rendered by `App.tsx` straight into the card, with no `PageHeader`: the
 * panel's title is the page's `h1` -- `SplitView`'s `heading` stands in for
 * it while the panel is collapsed -- and the intro sentence that used to sit
 * under a header is the hero's.
 *
 * The provider and the language are chosen here, in their chips' menus, and
 * saved as they are chosen -- the same `meetings.provider` and
 * `meetings.language` Settings > Meetings sets, which keeps its own controls
 * (and the menus' footers go there, for a language the menu does not list).
 * The Summary toggle likewise writes `reasoning.summaries`, whole, as
 * Cleanup & summaries does; Rust refuses it where the settings are incomplete,
 * and the page shows why.
 * Rust reads the language from the database when the meeting starts
 * (`start_live`); the provider is the one this page read back from
 * `meetings.provider` and passes to `meeting_start` itself. A choice is
 * written, then everything is read back, so the chip, the hero and the
 * privacy line follow what was saved rather than what was clicked. Start waits
 * while a write is in flight (`writing`): that is the save this page now has to
 * race, and a Start pressed before it landed would use the value the page was
 * still showing -- and, once it has, until the configuration review has
 * answered for the new values (`startHeld`). The hero's "Choose a provider" and the language fix open
 * those same menus rather than navigating; its other fixes still navigate.
 * The live view's chips stay static: a running meeting has its provider and
 * language already. The idle chips stay live while a meeting is finishing
 * (being saved after Stop): turning Summary on then applies to the meeting
 * being saved, because its summary is enqueued when the note is saved, while
 * a provider or language change applies to the next meeting.
 *
 * Two things it refuses to do, carried over from the screen it came out of.
 *
 * It does not decide which providers can be used -- that comes from Rust,
 * along with the reason each one cannot, and `selectProvider` only picks among
 * the usable ones.
 *
 * It does not report a meeting as recording both sources when it is recording
 * one. `Sources` comes back from `meeting_start` saying what actually opened,
 * and that is what the live view says (`recordedSources`, `lib/meetingView.ts`).
 *
 * Meeting state arrives from Rust by event as well as by read: a meeting can
 * be started from the notification window whether or not this page is open,
 * so it reads the live state when it mounts and follows the events after
 * that. (The meeting hotkey starts nothing yet -- `shortcuts.rs` dispatches
 * it as "Meeting capture is not built yet.") Detection has no event -- only
 * an offer does -- so while no meeting is running the page reads
 * `meeting_status` again every five seconds for the call card, and stops as
 * soon as one starts or the page is left.
 */

import { useCallback, useContext, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";

import { Chip } from "../components/Chip";
import { ChipMenu, type ChipMenuHandle } from "../components/ChipMenu";
import { ControlBar } from "../components/ControlBar";
import { FailureLine } from "../components/FailureLine";
import { Icon, IconFilled } from "../components/Icon";
import { LearnMore } from "../components/LearnMore";
import { LevelMeter } from "../components/LevelMeter";
import { PanelAction, PanelHeader, PanelItem, PanelSection } from "../components/Panel";
import { formatClock } from "../components/RecordingPlayer";
import { PanelCollapsed, SplitView } from "../components/SplitView";
import { TaskHero } from "../components/TaskHero";
import { languageLabel, privacyLines } from "../lib/controlBar";
import { timeOfDay } from "../lib/dayGroups";
import {
  cancelMeeting,
  checkMicrophone,
  flushMeetingPanelNotes,
  getFolders,
  getMeetingDeleteRecordings,
  getMeetingKeyterms,
  getMeetingLanguage,
  getMeetingProvider,
  getMeetingProviders,
  getInputDevice,
  getInputDevices,
  getLocalStatus,
  getMeetingStatus,
  getNoteSummaries,
  getRecoverableRecordings,
  getSherpaStatus,
  getSummarySettings,
  setMeetingLanguage,
  setMeetingProvider,
  onMeetingOutcome,
  onMeetingState,
  onMeetingTranscript,
  recoverMeetingRecording,
  setInputDevice,
  setSummarySettings,
  startMeeting,
  stopMeeting,
  type Detected,
  type MeetingOutcome,
  type MeetingState,
  type MicrophoneCheck,
  type RecoverableRecording,
  type Sources,
} from "../lib/ipc";
import {
  availabilityLabel,
  languageRequiredWarning,
  meetingIsLive,
  meetingOutcomeLabel,
  meetingStateLabel,
  systemAudioValue,
} from "../lib/labels";
import { emptyLiveTranscript, foldTranscriptEvent, liveRows, type LiveRow } from "../lib/liveTranscript";
import { providerMenu, selectProvider } from "../lib/meetingProvider";
import {
  activeCall,
  discardQuestion,
  elapsedLabel,
  folderMenu,
  interruptedTitle,
  liveElapsed,
  liveMicrophone,
  meetingLanguageMenu,
  meetingIntro,
  meetingMicrophone,
  MICROPHONE_CHECK_LISTENING,
  microphoneCheckLine,
  privacyProvider,
  recentMeta,
  recordedSources,
  sameMeetingStart,
  summaryPrivacyLine,
} from "../lib/meetingView";
import type { ViewId } from "../lib/navigation";
import { localProviderProblem, meetingReadiness } from "../lib/readiness";
import { summaryConfigured } from "../lib/summaryConfigured";
import { useMeetingReviewFor } from "../lib/useMeetingReview";
import { message, useRefresh } from "../lib/useRefresh";

/** How often detection is read again while no meeting is running. */
const DETECTION_POLL_MS = 5000;

/**
 * A label read as a sentence: capitalised, and closed with a full stop when
 * it does not already end in one. `availabilityLabel` writes for a line
 * beside a provider's name ("refused: ..."); the hero's Learn more reads it
 * on its own.
 */
function asSentence(label: string): string {
  const text = label.charAt(0).toUpperCase() + label.slice(1);
  return /[.!?]$/.test(text) ? text : `${text}.`;
}

/** "Recording from 1 Sep" as it reads inside a sentence. */
function lowerFirst(text: string): string {
  return text.charAt(0).toLowerCase() + text.slice(1);
}

export function Meetings({
  onGoTo,
  onOpenNote,
}: {
  /** Where the hero's fix, the chips and the panel's Calendar take you. */
  onGoTo: (id: ViewId) => void;
  /** Opens a Recent meeting's note in Notes. */
  onOpenNote: (id: number) => void;
}) {
  // Everything Start, Stop and the privacy line depend on, read together:
  // the summary settings are here, so while they load or fail the line has
  // nothing to go on and the page shows no control bar at all rather than a
  // line that might leave a summary out.
  // Whether recordings are deleted once transcribed is read with them too:
  // the page says so whenever it is on (KNEG-409), and a failure to read it
  // must not leave the page saying nothing.
  const load = useCallback(async () => {
    const [providers, status, provider, recoverable, language, keyterms, summaries, deleteRecordings] =
      await Promise.all([
        getMeetingProviders(),
        getMeetingStatus(),
        getMeetingProvider(),
        getRecoverableRecordings(),
        getMeetingLanguage(),
        getMeetingKeyterms(),
        getSummarySettings(),
        getMeetingDeleteRecordings(),
      ]);
    return { providers, status, provider, recoverable, language, keyterms, summaries, deleteRecordings };
  }, []);

  const { data, error, reload, guard } = useRefresh(load);

  // The Recent list, read on its own: it is only a list of notes, and a
  // failure to read it used to blank Start and Stop with it. Its error and
  // its retry stay in the panel. Retried through `guard`, which clears the
  // last error before reading again -- a plain `reload` keeps it even when
  // the read succeeds.
  const loadRecent = useCallback(() => getNoteSummaries({ noteType: "meeting", limit: 20 }), []);
  const recentRead = useRefresh(loadRecent);
  // What the two local providers need on this computer (KNEG-387), read on
  // their own for the same reason: a failure leaves both unknown, which holds
  // nothing back, and Start still says what went wrong.
  const loadEngines = useCallback(async () => {
    const [whisper, sherpa] = await Promise.all([getLocalStatus(), getSherpaStatus()]);
    return { whisper, sherpa };
  }, []);
  const engines = useRefresh(loadEngines).data ?? null;
  // The microphone meetings and dictation both record (KNEG-341), chosen in
  // the chip's menu here as on Dictation (KNEG-364), read on its own: a
  // failed read leaves the chip saying the default, which is what a meeting
  // falls back to anyway, with the failure in view under the bar. A choice is
  // written through this read's `guard`, so it is read back -- the chip names
  // what was saved -- and a failure to save it is in view the same way.
  const loadMicrophone = useCallback(async () => {
    const [devices, chosen] = await Promise.all([getInputDevices(), getInputDevice()]);
    return { devices, chosen };
  }, []);
  const microphoneRefresh = useRefresh(loadMicrophone);
  const microphoneRead = microphoneRefresh.data ?? null;
  const microphone = meetingMicrophone(microphoneRead?.devices ?? null, microphoneRead?.chosen ?? null);
  /**
   * The five-second "Test microphone" (KNEG-364): listening, what it heard,
   * or why it couldn't. `null` before the first. Start is held while it
   * listens, and a choice made meanwhile applies to the next check -- this
   * one keeps the microphone it opened, as a meeting does.
   */
  const [check, setCheck] = useState<
    | { state: "listening" }
    | { state: "done"; result: MicrophoneCheck }
    | { state: "failed"; error: string }
    | null
  >(null);
  const checking = check?.state === "listening";
  // The folders the Folder chip offers (KNEG-365), read on their own for the
  // same reason: a failure leaves only "No folder", which is where a note
  // goes anyway, and nothing else on the page waits for them.
  const folderRead = useRefresh(getFolders);
  const folders = folderRead.data ?? [];
  const reloadFolders = folderRead.reload;
  const recentGuard = recentRead.guard;
  const reloadRecent = useCallback(() => recentGuard(async () => {}), [recentGuard]);
  // Every read that follows a meeting ending or a recording being recovered
  // reads both, since either can add a note to Recent -- and the folders,
  // since an outcome can say the chosen one was deleted, and the chip should
  // stop naming it then.
  const reloadAll = useCallback(async () => {
    await Promise.all([reload(), reloadRecent(), reloadFolders()]);
  }, [reload, reloadRecent, reloadFolders]);

  const [state, setState] = useState<MeetingState | null>(null);
  const [wantSystem, setWantSystem] = useState(true);
  /**
   * What the next meeting started here is saved as (KNEG-365): its name, and
   * the folder chosen in the Folder chip. Kept on this page only, like
   * `wantSystem`, and handed to Rust at Start, which keeps them with the
   * meeting however it ends. The name clears once a start from here works.
   */
  const [meetingName, setMeetingName] = useState("");
  const [folderId, setFolderId] = useState<number | null>(null);
  const [sources, setSources] = useState<Sources | null>(null);
  const [transcript, setTranscript] = useState(emptyLiveTranscript);
  const [outcome, setOutcome] = useState<MeetingOutcome | null>(null);
  const [notesFlushFailed, setNotesFlushFailed] = useState(false);
  /** A provider or language write is in flight: Start waits for it. */
  const [writing, setWriting] = useState(false);
  const providerMenuRef = useRef<ChipMenuHandle>(null);
  const languageMenuRef = useRef<ChipMenuHandle>(null);
  const [recoveringKey, setRecoveringKey] = useState<string | null>(null);
  const [recoveryError, setRecoveryError] = useState<string | null>(null);
  const [recoveryOutcome, setRecoveryOutcome] = useState<MeetingOutcome | null>(null);
  /** Detection as last read, by the load or by the poll -- whichever was later. */
  const [detected, setDetected] = useState<readonly Detected[] | null>(null);
  /** The detection poll's own failure, which the next poll that works clears. */
  const [pollError, setPollError] = useState<string | null>(null);
  /** When the running meeting started, from Rust's `elapsedSeconds`; `null` with none running. */
  const [startedAt, setStartedAt] = useState<Date | null>(null);
  /** When this page saw the meeting stall, which is where its clock stops. */
  const [stalledAt, setStalledAt] = useState<Date | null>(null);
  /** The clock's "now", moved on once a second while a meeting records. */
  const [clockNow, setClockNow] = useState(() => new Date());
  const liveHeading = useRef<HTMLHeadingElement>(null);
  const checkGeneration = useRef(0);
  useEffect(() => () => { checkGeneration.current++; }, []);


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
    void onMeetingState((next) => {
      if (disposed) return;
      setState(next);
      if (next === "idle") { setSources(null); setNotesFlushFailed(false); }
      // A new meeting's own outcome has not happened yet, so the last one's
      // "Saved as note ..." must not sit under it. Start clears it too, but a
      // meeting started from the notification window never passes through
      // this page's Start. The same for what the last microphone check heard;
      // a result still in flight is invalidated.
      if (next === "recording") {
        setOutcome(null);
        checkGeneration.current++; setCheck(null);
      }
      // The state event carries the state and nothing else, and the sentence
      // explaining it lives on `MeetingStatus`. Reloading is how it gets here
      // -- the same thing the outcome listener below does, for the same reason.
      // Both failure states, because both put a sentence on screen and a state
      // that arrived without one would render the caveat empty. And
      // `recording`, because a meeting started from the notification window
      // while this page is open brings its provider, its sources and its
      // running time only on `MeetingStatus` too -- without the read, the
      // live view would name this page's provider rather than the one
      // actually in use, and that is where it says whether the audio leaves
      // this computer.
      if (
        next === "recording" ||
        next === "recording-without-transcript" ||
        next === "stalled"
      ) {
        void reload();
      }
    }).then(keep);
    // Folded rather than replaced: `foldTranscriptEvent` keeps a line open
    // while each partial extends it and closes it when one does not, so the
    // live view reads as lines of a conversation rather than as the latest
    // few seconds per source.
    void onMeetingTranscript((event) => {
      if (disposed) return;
      setTranscript((prev) => foldTranscriptEvent(prev, event));
    }).then(keep);
    void onMeetingOutcome((result) => {
      if (disposed) return;
      setOutcome(result);
      setTranscript(emptyLiveTranscript());
      void reloadAll();
    }).then(keep);
    return () => {
      disposed = true;
      unlisteners.forEach((fn) => fn());
    };
  }, [reload, reloadAll]);

  // Every read of the status resets what is counted from it: detection, and
  // when the meeting started, which the clock below is read against. Only a
  // meeting that is running has a start: `elapsedSeconds` is 0 otherwise, and
  // a start taken from it would be the time of the read.
  const loadedStatus = data?.status ?? null;
  useEffect(() => {
    if (loadedStatus === null) return;
    setDetected(loadedStatus.detected);
    setStartedAt(
      meetingIsLive(loadedStatus.state)
        ? new Date(Date.now() - loadedStatus.elapsedSeconds * 1000)
        : null,
    );
  }, [loadedStatus]);

  const currentState: MeetingState | null = state ?? loadedStatus?.state ?? null;
  const idle = currentState === "idle";
  // Kept here rather than asked for every second: the runtime owns the
  // clock, and an IPC round trip a second for a timer is arithmetic's job.
  // Each tick reads the time since the start (`liveElapsed`) rather than
  // adding a second, so a window throttled in the background, whose ticks
  // arrive seldom, still shows the right time when they do. Stopped while
  // stalled, at the moment this page saw the stall: nothing is being
  // recorded then, and a clock still running would say otherwise.
  const ticking = currentState === "recording" || currentState === "recording-without-transcript";
  const stalledNow = currentState === "stalled";
  useEffect(() => {
    if (!ticking) return undefined;
    setClockNow(new Date());
    const timer = window.setInterval(() => setClockNow(new Date()), 1000);
    return () => window.clearInterval(timer);
  }, [ticking]);
  useEffect(() => {
    setStalledAt(stalledNow ? new Date() : null);
  }, [stalledNow]);
  const elapsed = startedAt === null ? 0 : liveElapsed(startedAt, clockNow, stalledAt);

  // Detection is only read, never pushed -- only an offer has an event -- so
  // the call card would otherwise go on saying Zoom is in a call long after
  // the call ended. Only `detected` is taken from the poll: the state comes
  // from events, and a second writer of it would race them. A failed read
  // is shown until one succeeds, in its own state rather than the page's
  // `error`: clearing that on the next good poll would also clear a Start
  // or Stop failure nobody had read yet.
  useEffect(() => {
    if (!idle) return undefined;
    let disposed = false;
    const timer = window.setInterval(() => {
      getMeetingStatus()
        .then((next) => {
          if (disposed) return;
          setDetected(next.detected);
          setPollError(null);
        })
        .catch((cause: unknown) => { if (!disposed) setPollError(message(cause)); });
    }, DETECTION_POLL_MS);
    return () => {
      disposed = true;
      window.clearInterval(timer);
      // The poll stops while a meeting runs; what it last said is not news
      // by the time it starts again.
      setPollError(null);
    };
  }, [idle]);

  const rows = useMemo(() => liveRows(transcript), [transcript]);

  // Computed defensively before `data` is known to have loaded, and reused
  // below once it has, so the review hook can sit above the loading guard
  // rather than after it -- a hook cannot be called only on some renders.
  const usable = (data?.providers ?? []).filter(
    (p) => p.availability.state === "available",
  );
  // No `chosen`: the menu saves a choice and the page reads it back, so what
  // is shown is what was saved. The saved choice wins over
  // the first usable provider, exactly as on Settings > Meetings, so the two
  // screens name the same provider, and this page passes it to `meeting_start`
  // itself -- Rust does not read `meetings.provider` at start. The
  // notification's Record button reads the same key but not through this
  // fallback: it passes the saved id as it is, or `local` when nothing is
  // saved (`RECORD_WITH`), which is why a meeting it started is named from
  // `status.provider` below. `lib/meetingProvider.ts` says why the fallback
  // never writes (KNEG-317).
  const { selected, savedUnusable } = selectProvider(
    null,
    data?.provider ?? null,
    usable.map((p) => p.id),
  );
  // The saved language, not a draft: the review has to describe the language
  // `start_live` will read back from the database.
  const { review, current: reviewCurrent } = useMeetingReviewFor(
    selected,
    data?.language ?? "",
    (data?.keyterms ?? []).join("\n"),
  );
  // Whether the list panel is showing, for the one sentence that points into it.
  const panelCollapsed = useContext(PanelCollapsed);

  if (!data) {
    return (
      <SplitView label="Meetings list" heading="Meetings" panel={<PanelHeader title="Meetings" />}>
        <section data-surface="meetings.record" className="task" aria-label="Meeting">
          {error ? <p className="error">{error}</p> : <p className="muted">Loading…</p>}
        </section>
      </SplitView>
    );
  }

  const { providers, status, recoverable, summaries } = data;
  const recent = recentRead.data;
  const current = state ?? status.state;
  const now = new Date();
  // A meeting whose provider died is still recording, so every control that
  // asks "is a session running" must keep saying yes: Stop and Discard have to
  // stay on screen, and Start has to stay off it. This was the whole shape of
  // the bug `stop` had -- a screen with no working button on it.
  const transcriptEnded = current === "recording-without-transcript";
  const stalled = current === "stalled";
  // Asked of `meetingIsLive` rather than compared here. A state added to the
  // union does not break a `===`, so when `stalled` arrived this screen kept
  // compiling and would have offered "Start recording" for a session that was
  // still held with a WAV still on disk.
  const recording = meetingIsLive(current);
  const languageWarning = review ? languageRequiredWarning(review) : null;
  const selectedProvider = providers.find((p) => p.id === selected) ?? null;
  const savedProvider = savedUnusable
    ? (providers.find((p) => p.id === savedUnusable) ?? null)
    : null;
  const readiness = meetingReadiness({
    usable,
    selected,
    // Named and with its reason, so the hero can say which choice can't be
    // used and why, rather than the page quietly using another.
    // A provider this version does not offer has only its id for a name, so
    // it goes unnamed in view and the id goes to the detail (KNEG-354).
    savedUnusable: savedUnusable
      ? {
          name: savedProvider?.name ?? null,
          reason: savedProvider
            ? asSentence(availabilityLabel(savedProvider.availability))
            : `It was saved as “${savedUnusable}”, which this version of the app doesn't offer.`,
        }
      : null,
    languageWarning,
    localProblem: localProviderProblem(selected, engines?.whisper ?? null, engines?.sherpa ?? null),
    systemAudio: status.systemAudio,
  });
  // Where readiness holds Start back, the hero's fix is the one thing to do
  // next and takes the page's single filled button.
  const startBlocked = readiness.state === "needs-setup";
  // What holds Start and "Record this call" off, the same for both: setup is
  // missing, no provider, a provider or language write in flight, or the
  // review still describing the previous configuration -- its answer arrives
  // after the re-read, and until it does `readiness` cannot be trusted to
  // know that the new provider needs a language. And the microphone check
  // while it listens: five seconds, and a meeting would open the microphone
  // under it (KNEG-364).
  const startHeld = startBlocked || selected === null || writing || !reviewCurrent || checking;
  // While a meeting is running, the provider it is running with -- which the
  // notification's Record button may have picked rather than this page.
  const liveProvider =
    recording && status.provider !== null
      ? (providers.find((p) => p.id === status.provider) ?? null)
      : null;
  const transcriber = liveProvider ?? selectedProvider;
  const systemAudioAvailable = status.systemAudio.state === "available";
  const liveSources = sources ?? status.sources;
  // The running meeting's name, from Rust rather than this page: a meeting
  // started from the notification window has none, and one started here may
  // have been named before the page was last mounted (KNEG-365).
  const liveTitle = status.plan?.title ?? "Meeting";
  const language = languageLabel(data.language.trim() === "" ? [] : [data.language.trim()]);
  // The privacy fact, never hidden: whether this meeting's audio leaves this
  // computer, from `audioLeavesDevice` -- the field Rust sets for exactly that
  // question -- and, when an automatic summary will send the transcript
  // somewhere, where (`summaryPrivacyLine`). With no provider nothing can
  // start, so nothing is sent.
  const summaryLine = summaryPrivacyLine(summaries);
  // Not the fallback while Start waits on an unusable saved provider: nothing
  // is sent until somebody chooses (`privacyProvider`).
  const sentTo = privacyProvider(liveProvider, selectedProvider, savedUnusable !== null);
  const privacy = [
    ...privacyLines(
      sentTo
        ? {
            isLocal: !sentTo.audioLeavesDevice,
            leavesDevice: sentTo.audioLeavesDevice,
            destination: sentTo.name,
          }
        : { isLocal: false, leavesDevice: false, destination: "", problem: "not set up" },
      null,
    ),
    ...(summaryLine === null ? [] : [summaryLine]),
  ];
  // The card is for a call nobody is recording yet. While one is being
  // recorded or transcribed there is nothing it could offer.
  const call = current === "idle" ? activeCall(detected ?? status.detected) : null;

  // The two menus. A choice is written and then everything is read back
  // (`guard`), so `selected`, the review, the hero and the privacy line all
  // follow the saved value. Choosing what is already saved writes nothing --
  // except the fallback shown when the saved provider cannot be used: that one
  // is not what is saved, so choosing it is the write that replaces it.
  const providerOptions = providerMenu(providers, data.provider, selected);
  const languageOptions = meetingLanguageMenu(data.language, selectedProvider);
  // Its `folderId` is the folder Start sends: the one the chip names, so a
  // folder deleted in Notes since it was chosen here is no folder.
  const folderChoice = folderMenu(folders, folderId);
  const choose = (write: () => Promise<void>) => {
    // One write at a time, as Dictation's switch does: a second choice made
    // while the first is in flight is ignored, so `writing` clears only when
    // the one write it stands for has landed.
    if (writing) return;
    setWriting(true);
    void guard(write).finally(() => setWriting(false));
  };

  // The microphone chip's choice: written, then the microphone read again,
  // through that read's own `guard`. Shared with dictation (KNEG-341), so
  // this changes its microphone too, as the chip's title says.
  const chooseMicrophone = (id: string) => {
    checkGeneration.current++; setCheck(null);
    void microphoneRefresh.guard(() => setInputDevice(id || null));
  };
  // "Test microphone": Rust opens the saved microphone for five seconds and
  // sends its level as `microphone-check` meanwhile (KNEG-364). Its refusals
  // -- a meeting under way, a dictation recording, a check already running --
  // start "Not now: ", and `FailureLine` shows them as the line itself.
  const testMicrophone = () => {
    if (checking) return;
    const generation = ++checkGeneration.current;
    setCheck({ state: "listening" });
    checkMicrophone()
      .then((result) => { if (generation === checkGeneration.current) setCheck({ state: "done", result }); })
      .catch((cause: unknown) => { if (generation === checkGeneration.current) setCheck({ state: "failed", error: message(cause) }); });
  };

  const start = (withSystem: boolean) => {
    if (startHeld) return;
    void guard(async () => {
      setOutcome(null);
      setCheck(null);
      // The outcome event clears the last meeting's lines; this makes sure a
      // new meeting never opens on them should that event not have arrived.
      setTranscript(emptyLiveTranscript());
      // Rust tidies the name (`note_title`); a blank one is no name.
      const title = meetingName.trim() === "" ? null : meetingName;
      setSources(await startMeeting(selected, withSystem, { title, folderId: folderChoice.folderId }));
      // That meeting has the name now; the next one starts unnamed. The
      // folder stays chosen, as the chips do.
      setMeetingName("");
    });
  };
  // Stop and Discard first ask the meeting window to save what was typed in
  // it, so the note Stop saves has the last edit; a flush that fails says so
  // and offers "Stop recording anyway" and "Discard anyway" rather than
  // leaving the meeting recording with no way to end it (KNEG-418).
  const stop = () => {
    void guard(async () => {
      try { await flushMeetingPanelNotes(); }
      catch (cause) { setNotesFlushFailed(true); throw cause; }
      setNotesFlushFailed(false);
      setOutcome(await stopMeeting());
    });
  };
  // Discard asks first, with what is about to be lost (KNEG-394): it sat
  // beside Stop and save and deleted the recording on one click. True only
  // for a yes about the meeting still running; saying no changes nothing, and
  // the meeting goes on recording.
  const confirmDiscard = async (question: (elapsedSeconds: number) => string): Promise<boolean> => {
    const before = await getMeetingStatus();
    if (!meetingIsLive(before.state)) return false;
    const began = Date.now() - before.elapsedSeconds * 1000;
    // The page's clock, which stops at a stall, when it has one; Rust's
    // count before the page has read a start, so the length is never
    // "under a minute" for a meeting it has not timed yet.
    if (!window.confirm(question(startedAt === null ? before.elapsedSeconds : elapsed))) return false;
    // The question holds the page, so the meeting may have changed while it
    // was open: stopped from its own window, or stopped and another one
    // started. Nothing is discarded unless the meeting asked about is still
    // running -- otherwise the page shows what happened instead.
    const after = await getMeetingStatus();
    return meetingIsLive(after.state) && sameMeetingStart(began, after, Date.now());
  };
  const discard = () => {
    void guard(async () => {
      if (!(await confirmDiscard((seconds) => discardQuestion(seconds)))) return;
      try { await flushMeetingPanelNotes(); }
      catch (cause) { setNotesFlushFailed(true); throw cause; }
      setNotesFlushFailed(false);
      await cancelMeeting();
    });
  };
  const stopAnyway = () => {
    void guard(async () => { setOutcome(await stopMeeting()); setNotesFlushFailed(false); });
  };
  // Discard without asking the meeting window again: it already didn't
  // answer. `cancel` still keeps the notes Rust holds (`notes.take()`,
  // `meeting.rs`), so what is lost is an edit the window hadn't sent -- the
  // question says so (KNEG-418).
  const discardAnyway = () => {
    void guard(async () => {
      if (!(await confirmDiscard((seconds) => discardQuestion(seconds, true)))) return;
      await cancelMeeting();
      setNotesFlushFailed(false);
    });
  };
  const recover = (candidate: RecoverableRecording) => {
    setRecoveringKey(candidate.storageKey);
    setRecoveryError(null);
    setRecoveryOutcome(null);
    void recoverMeetingRecording(candidate.storageKey)
      .then(async (result) => {
        setRecoveryOutcome(result);
        await reloadAll();
      })
      .catch((cause: unknown) => setRecoveryError(String(cause)))
      .finally(() => setRecoveringKey(null));
  };

  // Visible wherever they apply, live or idle: each is something that failed
  // or something to do now.
  const caveats = (
    <>
      {stalled ? (
        /*
         * A different sentence from the one below, because a different thing
         * has happened: there the recording continues and the transcript
         * does not, here neither does. The one instruction that matters is
         * the same in both — press Stop — because that is what turns what
         * was captured into a note.
         */
        <p className="caveat warn">
          <strong>The recording stopped unexpectedly.</strong>{" "}
          {status.transcriptFailure ??
            "The recording thread ended without saying why."}{" "}
          Nothing further is being captured. Stop will still transcribe what
          was recorded before it stopped, and Discard will throw it away.
        </p>
      ) : null}
      {transcriptEnded ? (
        /*
         * The provider's own sentence, which is why `MeetingError::
         * ProviderFault` carries which of the two sockets died: "the
         * connection failed" would send someone looking at their network.
         *
         * Marked rather than red-texted, the same split `.caveat.warn` was
         * introduced for: nothing here has failed in a way that needs
         * repairing right now. The recording is running and the note will
         * still be produced -- from the whole-file pass, which reads the
         * complete audio and does not depend on the provider that died.
         */
        <p className="caveat warn">
          <strong>The live transcript stopped.</strong>{" "}
          {status.transcriptFailure ??
            "The transcription task ended without saying why."}{" "}
          Recording continues, and Stop will still transcribe the whole
          recording — the saved note comes from that pass, not from the
          lines below.
        </p>
      ) : null}
      {recording && notesFlushFailed && <p className="caveat">The meeting window didn't confirm it saved your latest notes. Stop recording anyway saves the meeting; Discard anyway deletes the recording. Either way, notes the app already has are kept, and your most recent edit may be missing.</p>}
      {outcome ? <OutcomeCaveat outcome={outcome} /> : null}
    </>
  );

  const panel = (
    <>
      <PanelHeader title="Meetings" />
      <PanelSection label={null}>
        <PanelAction icon="calendar" label="Calendar" onClick={() => onGoTo("calendar")} />
      </PanelSection>
      {recording ? (
        <PanelSection label="Now">
          <PanelItem
            title={liveTitle}
            tooltip={liveTitle}
            current
            meta={
              // Stalled is not recording -- nothing is being captured -- so
              // not the recording dot: the hollow square Interrupted uses.
              <span className={stalled ? "panel-rec panel-rec-stalled" : "panel-rec"}>
                <span className={stalled ? "mark-hollow" : "rec-dot"} aria-hidden="true" />
                {stalled ? "Stopped unexpectedly" : "Recording"} · {elapsedLabel(elapsed)}
              </span>
            }
            onClick={() => liveHeading.current?.focus()}
          />
        </PanelSection>
      ) : null}
      {/* Recent's own states, since it is read on its own: a failure to
          list notes says so here and offers to read again, and leaves the
          recording controls beside it alone. */}
      {recentRead.error !== null ? (
        <div className="panel-note panel-note-error" role="alert">
          <p className="error">{`Couldn't load recent meetings: ${recentRead.error}`}</p>
          <button type="button" className="panel-inline" onClick={() => { void reloadRecent(); }}>
            Try again
          </button>
        </div>
      ) : null}
      {recent === null ? (
        recentRead.error === null ? <p className="panel-note">Loading recent meetings…</p> : null
      ) : recent.length === 0 ? (
        <p className="panel-note">No meetings saved yet.</p>
      ) : (
        <PanelSection label="Recent">
          {recent.map((note) => {
            const meta = recentMeta(note.createdAt, note.audioDurationSeconds, now);
            return (
              <PanelItem
                key={note.id}
                title={note.title}
                tooltip={note.title}
                {...(meta === "" ? {} : { meta })}
                onClick={() => onOpenNote(note.id)}
              />
            );
          })}
        </PanelSection>
      )}
      {recoverable.length === 0 ? null : (
        <PanelSection label="Interrupted">
          {recoverable.map((candidate) => {
            // The name it was given before Start, from the plan Rust kept
            // beside it (KNEG-365); otherwise when it started.
            const title = candidate.title ?? interruptedTitle(candidate.storageKey, now);
            const busy = recoveringKey === candidate.storageKey;
            return (
              // Not a `PanelItem`: that is a button, and Recover is a button
              // inside the row. The storage key, which this row used to show
              // as its label, is the title's tooltip.
              <div className="panel-row" key={candidate.storageKey}>
                <span className="panel-item-title" title={candidate.storageKey}>{title}</span>
                <span className="panel-item-meta panel-row-meta">
                  <span className="mark-hollow" aria-hidden="true" />
                  Not transcribed{candidate.hasNotes ? " · Typed notes found" : ""}
                  <button
                    type="button"
                    className="panel-inline"
                    aria-label={`${busy ? "Recovering" : "Recover"} ${candidate.title ?? lowerFirst(title)}`}
                    disabled={recording || current === "finishing" || recoveringKey !== null}
                    onClick={() => recover(candidate)}
                  >
                    {busy ? "Recovering…" : "Recover"}
                  </button>
                </span>
              </div>
            );
          })}
        </PanelSection>
      )}
      <div className="panel-foot">
        <Icon name="lock" size={14} />
        <span>{data.deleteRecordings ? "Recordings are deleted after transcribing." : "Recordings are saved on this computer."}</span>
      </div>
    </>
  );

  if (recording) {
    const sub = [
      startedAt === null ? null : `Started ${timeOfDay(startedAt)}`,
      recordedSources(liveSources),
      transcriber === null
        ? null
        : transcriber.audioLeavesDevice
          ? `Sent to ${transcriber.name}`
          : "Transcribed on this computer",
      // This meeting's own choice, taken at its Start (KNEG-409): the
      // setting may have changed since, and applies to the next one.
      status.plan?.deleteRecording ? "Recording deleted after saving" : null,
    ].filter((part): part is string => part !== null);
    // The microphone this meeting opened, as Rust reports it, and not the
    // setting: a choice made now applies to the next recording (KNEG-364).
    const openedMicrophone = liveMicrophone(liveSources);
    return (
      <SplitView label="Meetings list" heading="Meetings" panel={panel}>
        <section data-surface="meetings.record" className="meeting-live" aria-label="Meeting">
          <header className="live-head">
            {/* Stalled draws the hollow `--text` square and drops the
                recording red: the round dot says something is being
                recorded, and a stalled meeting records nothing. */}
            <p className={stalled ? "live-status live-status-stalled" : "live-status"}>
              <span className={stalled ? "mark-hollow" : "rec-dot"} aria-hidden="true" />
              {/* Only the state is a live region: the clock beside it ticks
                  every second. */}
              <span role="status">{meetingStateLabel(current)}</span>
              <span className="live-clock">{elapsedLabel(elapsed)}</span>
            </p>
            <h2 className="live-title" ref={liveHeading} tabIndex={-1}>{liveTitle}</h2>
            <p className="live-sub">{sub.join(" · ")}</p>
            {/* How loud each capture is (KNEG-371): you, and the others only
                when system audio opened -- the sub-line above says which.
                Decoration, labels included: `LevelMeter` says why. */}
            <div className="live-levels" aria-hidden="true">
              <span className="live-level">
                <span className="live-level-label">You</span>
                <LevelMeter session="meeting" source="microphone" />
              </span>
              {liveSources?.systemAudio ? (
                <span className="live-level">
                  <span className="live-level-label">Others</span>
                  <LevelMeter session="meeting" source="system" />
                </span>
              ) : null}
            </div>
          </header>
          {error || stalled || transcriptEnded || notesFlushFailed || outcome ? (
            <div className="live-caveats">
              {error && !notesFlushFailed ? <p className="error">{error}</p> : null}
              {caveats}
            </div>
          ) : null}
          <LiveTranscript rows={rows} ended={transcriptEnded || stalled} />
          {/* Nothing here can be changed while the meeting runs -- Settings >
              Meetings disables its fields too -- so the chips are statements
              rather than controls. */}
          <div className="live-bar">
            <div className="live-chips" role="group" aria-label="In effect for this meeting">
              <Chip icon="microphone" label={openedMicrophone.label} title={openedMicrophone.title} srText={openedMicrophone.title} />
              {liveSources?.systemAudio ? <Chip icon="others" label="Other participants" /> : null}
              <Chip icon="computer" label={transcriber ? transcriber.name : "Not set up"} />
              <Chip icon="language" label={language} />
            </div>
            <ul className="control-privacy">
              {privacy.map((line) => (
                <li key={line}>
                  <Icon name="lock" size={14} />
                  {line}
                </li>
              ))}
            </ul>
            <div className="control-spacer" />
            {/* Never held back by readiness: a meeting that is running has to
                be stoppable whatever the settings now say. */}
            <div className="live-actions">
              {notesFlushFailed ? (
                <>
                  <button type="button" onClick={stopAnyway}>Stop recording anyway</button>
                  <button type="button" onClick={discardAnyway}>Discard anyway</button>
                </>
              ) : null}
              <button type="button" className="secondary" onClick={discard}>
                <Icon name="discard" size={16} />
                Discard
              </button>
              <button type="button" className="primary" onClick={stop}>
                <IconFilled name="stop" size={14} />
                Stop and save
              </button>
            </div>
          </div>
        </section>
      </SplitView>
    );
  }

  // The hero. Readiness answers "will starting work?", which means nothing
  // while a recording is being transcribed -- "Ready" above "Transcribing"
  // would be two answers on one screen -- so then the hero speaks for that.
  const hero =
    current === "finishing" ? (
      <TaskHero
        icon="meetings"
        status={{ tone: "busy", label: meetingStateLabel(current) }}
        heading="Finishing your meeting"
        sub="Finalizing the transcript and saving your notes. Speaker labels or recovery from an interrupted transcript may take longer."
      />
    ) : readiness.state === "ready" ? (
      <TaskHero
        icon="meetings"
        status={{ tone: "ready", label: "Ready" }}
        heading="Record a meeting"
        sub={
          <>
            <p>{meetingIntro(summaries.enabled)}</p>
            {readiness.caution ? <p className="hero-note">{readiness.caution}</p> : null}
          </>
        }
      />
    ) : (
      <TaskHero
        icon="meetings"
        status={{ tone: "setup", label: "Needs setup" }}
        heading="Record a meeting"
        sub={
          <>
            <p>{meetingIntro(summaries.enabled)}</p>
            <p className="hero-note">{readiness.problem}</p>
          </>
        }
      >
        {/* Needs setup always holds Start back (see Start's comment), so the
            fix is the one thing to do next and takes the filled button. */}
        <button
          type="button"
          className="primary"
          onClick={() => {
            const view = readiness.fix.view;
            // The two choices this page makes itself open their menus; the
            // rest are somewhere else.
            if (view === "provider-menu") providerMenuRef.current?.open();
            else if (view === "language-menu") languageMenuRef.current?.open();
            else onGoTo(view);
          }}
        >
          {readiness.fix.label}
        </button>
        {readiness.detail ? <LearnMore>{readiness.detail}</LearnMore> : null}
      </TaskHero>
    );

  return (
    <SplitView label="Meetings list" heading="Meetings" panel={panel}>
      <section data-surface="meetings.record" className="task" aria-label="Meeting">
        <div className="task-hero">
          {hero}
          {call === null ? null : (
            <div className="call-card">
              <span className="call-card-icon">
                <Icon name="call" size={20} />
              </span>
              <span className="call-card-text">
                <span className="call-card-title">{call.name} is in a call</span>
                <span className="call-card-detail">Detected just now. Nothing is recorded until you start.</span>
              </span>
              {/* Held back exactly as Start is (ruling R9), and recording
                  what the "Other participants" chip below says, as Start
                  does (R21): the card is a quicker way to the same start,
                  not a second choice about who is recorded. Both are pinned
                  by tools/visual-check/chip-menus.mjs check 10. */}
              <button
                type="button"
                className="secondary"
                disabled={startHeld}
                onClick={() => start(wantSystem && systemAudioAvailable)}
              >
                Record this call
              </button>
            </div>
          )}
        </div>

        {/* The lines heard during the meeting, kept until it is saved: they
            went at Stop, while the meeting was still being finished, and the
            outcome event clears them (KNEG-377). */}
        {current === "finishing" && rows.length > 0 ? <LiveTranscript rows={rows} ended finishing /> : null}

        <div className="task-bar">
          <ControlBar
            strip={
              <>
                {/* Chosen here as on Dictation, one choice for both tasks
                    (KNEG-341, KNEG-364): the rows sit under a heading that
                    says so, and the `title` says choosing here changes
                    dictation's too. The footer runs the five-second test;
                    it is left off while one listens, so it is never a press
                    that does nothing. */}
                <ChipMenu
                  icon="microphone"
                  name="Microphone"
                  label={microphone.label}
                  title={microphone.title}
                  options={microphone.options}
                  onChoose={chooseMicrophone}
                  {...(checking ? {} : { footer: { label: "Test microphone", onClick: testMicrophone } })}
                />
                {/* Unpressed, not just disabled, where system audio cannot be
                    captured: a pressed "Other participants" beside a caution
                    saying they cannot be recorded would be two answers on one
                    screen. The request itself is kept, so the chip comes back
                    pressed where the capture does. */}
                <Chip
                  icon="others"
                  label="Other participants"
                  pressed={wantSystem && systemAudioAvailable}
                  disabled={!systemAudioAvailable}
                  title={
                    status.systemAudio.state === "available"
                      ? "Record what the other participants say"
                      : "Can't be recorded on this computer"
                  }
                  onClick={() => setWantSystem((want) => !want)}
                />
                {/* Named as Dictation's are: "Deepgram" or "Swedish" alone
                    says nothing about which setting it is the value of.
                    The label stays whole inside the name (WCAG 2.5.3). */}
                <ChipMenu
                  ref={providerMenuRef}
                  icon="computer"
                  name="Meeting transcription"
                  label={transcriber ? transcriber.name : "Not set up"}
                  options={providerOptions}
                  onChoose={(id) => {
                    if (id !== data.provider) choose(() => setMeetingProvider(id));
                  }}
                  footer={{ label: "More in Settings…", onClick: () => onGoTo("meeting-settings") }}
                />
                <ChipMenu
                  ref={languageMenuRef}
                  icon="language"
                  name="Language"
                  label={language}
                  options={languageOptions}
                  onChoose={(id) => {
                    if (id !== data.language.trim()) choose(() => setMeetingLanguage(id));
                  }}
                  footer={{ label: "Another language in Settings…", onClick: () => onGoTo("meeting-settings") }}
                />
                {/* The summary, written here like the provider and the language
                    (Settings > Cleanup & summaries keeps its own switch). Where
                    the provider and model are not chosen yet there is nothing
                    to turn on -- Rust refuses it -- so the chip says so and
                    goes where they are chosen. */}
                {summaryConfigured(summaries) ? (
                  <Chip
                    icon="notes"
                    label="Summary"
                    pressed={summaries.enabled}
                    title={
                      summaries.enabled
                        ? "A summary is made of each meeting when it ends"
                        : "Make a summary of each meeting when it ends"
                    }
                    onClick={() => choose(() => setSummarySettings({ ...summaries, enabled: !summaries.enabled }))}
                  />
                ) : (
                  <Chip
                    icon="notes"
                    label="Summary: needs setup"
                    chevron
                    title="Choose a summary provider and model in Cleanup & summaries"
                    onClick={() => onGoTo("intelligence")}
                  />
                )}
                {/* Where the note goes (KNEG-365). Page state, not a
                    setting: it applies to the next meeting started here and
                    goes to Rust with it, so it holds however that meeting
                    ends. */}
                <ChipMenu
                  icon="folder"
                  name="Folder"
                  label={folderChoice.label}
                  options={folderChoice.options}
                  onChoose={(id) => setFolderId(id === "" ? null : Number(id))}
                />
              </>
            }
            privacy={privacy}
          >
            {error ? <p className="error">{error}</p> : null}
            {pollError ? (
              <p className="error">{`Couldn't check whether a meeting app is in a call: ${pollError}`}</p>
            ) : null}
            {/* The microphone chip's own read and write (KNEG-364): a choice
                that couldn't be saved, or a list that couldn't be read, in
                view, with what the software said under Details. */}
            {microphoneRefresh.error !== null ? (
              <FailureLine text="The microphone choice couldn't be read or saved." detail={microphoneRefresh.error} />
            ) : null}
            {microphone.missing !== null ? (
              <p className="control-note">
                Not connected, so meetings and dictation use the system default
                until it is plugged back in.
              </p>
            ) : null}
            {/* The microphone check (KNEG-364). One live region from
                "Listening" to what it heard, so the result is read out when
                it replaces the line; the meter beside the line is decoration
                (`LevelMeter`). Heard nothing is a caveat: something to look
                at before a meeting, not a failure. A refusal or a failure is
                `FailureLine`'s, outside the region, as an alert. */}
            {check !== null && check.state !== "failed" ? (
              <div className="microphone-check" role="status">
                {check.state === "listening" ? (
                  <>
                    <p className="control-note">{MICROPHONE_CHECK_LISTENING}</p>
                    <LevelMeter session="microphone-check" source="microphone" />
                  </>
                ) : (
                  <p className={check.result.heard ? "control-note" : "caveat"}>{microphoneCheckLine(check.result)}</p>
                )}
              </div>
            ) : null}
            {check?.state === "failed" ? (
              <FailureLine text="The microphone couldn't be tested." detail={check.error} />
            ) : null}
            {/* Why the chip is off, said beside it in every state -- the
                hero's caution is only there while the page is Ready, and a
                switched-off control with no reason on screen is the button
                that silently does nothing. The failure is in view; why, which
                names the platform's mechanism (about 70 words on macOS, and
                PulseAudio, PipeWire and ALSA on Linux), is folded with the
                device it would capture (KNEG-354). */}
            {status.systemAudio.state === "available" ? null : (
              <p className="control-note">Other participants can't be recorded on this computer.</p>
            )}
            {/* Believed, not tested, said in view rather than only in the fold
                below: the one place a colleague would learn it (KNEG-351). */}
            {status.systemAudio.state === "available" && status.systemAudio.unverified ? (
              <p className="control-note">
                Recording the other participants is available on this operating system but has never been tested here.
              </p>
            ) : null}
            <LearnMore
              summary={
                status.systemAudio.state === "available"
                  ? "How the other participants are recorded"
                  : "Why they can't be recorded"
              }
            >
              <p>
                {status.systemAudio.state === "unsupported"
                  ? status.systemAudio.reason
                  : `System audio: ${systemAudioValue(status.systemAudio)} — ${
                      status.systemAudio.state === "available"
                        ? `${status.systemAudio.mechanism} — ${status.systemAudio.device}`
                        : `${status.systemAudio.mechanism}. ${asSentence(status.systemAudio.reason)}`
                    }`}
              </p>
            </LearnMore>
            {/* Said in view while it is on (KNEG-409): the switch is on
                Settings > Meetings, and a note with no recording would
                otherwise be the first anyone heard of it. */}
            {data.deleteRecordings ? (
              <p className="control-note">Recordings are deleted once each meeting is transcribed.</p>
            ) : null}
            {caveats}

            {/* The Interrupted rows are in the list panel; what recovering
                one did, and the sentence that changes what you get back,
                stay here. What these are and how Recover works is folded. */}
            {/* With the panel collapsed, "the list" is nowhere on screen, so
                the sentence also says where the list is. */}
            {recoverable.length > 0 ? (
              <>
                <p className="caveat">
                  A recording from an earlier session was interrupted. Recover
                  it from Interrupted in the list
                  {panelCollapsed ? " (Show list in the title bar)" : ""}; the
                  audio may end abruptly.
                </p>
                <LearnMore>
                  <p>
                    These recordings are not attached to a saved note. Recover
                    uses the local whole-recording transcriber and saves any
                    notes typed during the meeting.
                  </p>
                </LearnMore>
              </>
            ) : null}
            {recoveringKey !== null ? (
              <p className="control-note" role="status">
                Recovering{" "}
                {recoverable.find((candidate) => candidate.storageKey === recoveringKey)?.title ??
                  `the ${lowerFirst(interruptedTitle(recoveringKey, now))}`}
                …
              </p>
            ) : null}
            {recoveryOutcome && <OutcomeCaveat outcome={recoveryOutcome} />}
            {recoveryError && <p className="error" role="alert">{recoveryError}</p>}

            <div className="control-row">
              {/* The meeting's name, the note's title (KNEG-365); left
                  blank, the note is named for when the meeting started.
                  Rust caps a title at 200 characters. */}
              <input
                type="text"
                className="meeting-name"
                value={meetingName}
                maxLength={200}
                placeholder="Name this meeting (optional)"
                aria-label="Meeting name (optional)"
                onChange={(event) => setMeetingName(event.target.value)}
              />
              {/* Off whenever the hero says Needs setup, not only when no
                  provider is selected: with a language the provider requires
                  left blank, `start_live` refuses (`resolve_provider_config`,
                  src-tauri/src/meeting.rs), so the button would lead straight
                  into an error, and with the saved provider unusable it would
                  record with one nobody chose. The hero says what is wrong,
                  and its fix -- not this -- is the filled button then.
                  Records what the "Other participants" chip says, as the
                  call card does (R21): where system audio can't be captured
                  the chip reads unpressed, so Start doesn't ask for it
                  (KNEG-412). */}
              <button
                type="button"
                className={startBlocked ? "control-start" : "primary control-start"}
                disabled={startHeld || current === "finishing"}
                onClick={() => start(wantSystem && systemAudioAvailable)}
              >
                <span className="start-dot" aria-hidden="true" />
                Start recording
              </button>
            </div>
          </ControlBar>
        </div>
      </section>
    </SplitView>
  );
}

/** The sentence over the live lines, while they are still arriving. */
const LIVE_NOTE =
  "Lines appear as they are heard, and the last one may still change. Local streaming models save the finalized transcript when you stop. Other models process the recording afterwards.";

/** The same, once nothing more will arrive. */
const ENDED_NOTE =
  "These are the last lines the provider sent before the live transcription stopped. Nothing further will appear here, and the transcript that gets saved comes from one pass over the whole recording when the meeting ends.";

/** The same, after Stop and before the meeting is saved. */
const FINISHING_NOTE =
  "What was heard while the meeting ran. These lines stay until the meeting is saved to Notes.";

/**
 * How a meeting ended, with the note's id and the recording's path behind
 * Learn more (KNEG-354).
 */
function OutcomeCaveat({ outcome }: { outcome: MeetingOutcome }) {
  const { text, more } = meetingOutcomeLabel(outcome);
  return (
    <>
      <p className="caveat">{text}</p>
      {more ? (
        <LearnMore>
          <p>{more}</p>
        </LearnMore>
      ) : null}
    </>
  );
}

/**
 * The live lines, as `foldTranscriptEvent` built them: time, speaker, text.
 * A line still open is muted and ends in the `--band` caret.
 *
 * It follows the newest line while it is scrolled to the bottom, and stops
 * following once scrolled up, so a line being read is not pulled away -- the
 * rule the meeting window's transcript tab keeps (`MeetingPanel.tsx`).
 */
function LiveTranscript({ rows, ended, finishing = false }: { rows: readonly LiveRow[]; ended: boolean; finishing?: boolean }) {
  const list = useRef<HTMLOListElement>(null);
  const following = useRef(true);
  useLayoutEffect(() => {
    const element = list.current;
    if (element !== null && following.current) element.scrollTop = element.scrollHeight;
  }, [rows]);
  return (
    <div className="live-transcript" role="region" aria-label="Live transcript">
      <h3 className="live-label">Live transcript</h3>
      <p className="live-note">{finishing ? FINISHING_NOTE : ended ? ENDED_NOTE : LIVE_NOTE}</p>
      <ol
        className="live-rows"
        ref={list}
        onScroll={() => {
          const element = list.current;
          if (element !== null) following.current = element.scrollHeight - element.scrollTop - element.clientHeight <= 8;
        }}
      >
        {rows.map((row) => {
          const speaker = row.speaker ?? (row.source === "mic" ? "You" : "Others");
          return (
            <li key={row.id} className={row.committed ? "live-row" : "live-row live-row-open"}>
              <span className="live-time">{formatClock(row.startSeconds)}</span>
              <span className={speaker === "You" ? "live-speaker live-speaker-you" : "live-speaker"} title={speaker}>
                {speaker}
              </span>
              <span className="live-text" dir="auto">
                {row.text}
                {row.committed ? null : <span className="live-caret" aria-hidden="true" />}
              </span>
            </li>
          );
        })}
      </ol>
    </div>
  );
}
