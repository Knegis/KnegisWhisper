/**
 * The meeting notification window's contents.
 *
 * Compact offers and finishing use 392×92; recording opens the 440×560 panel.
 * The panel hosts the typed-notes draft and reads it back after a webview reload.
 * Its editor and tabs opt out of Tauri's deep drag region, leaving the header
 * as the drag surface. The transcript tab shows live text only when capture
 * exclusion was read back as active.
 * Two jobs in one window:
 *
 * * **Offering to record**, when a meeting appears to be in progress. The offer
 *   says which application and which process gave it away, so somebody who
 *   thinks it is wrong can say why.
 * * **Saying it is recording**, for as long as it is. That is not decoration:
 *   the brief requires the local user get a visible indicator whenever a
 *   microphone is open, and this is a window recording a room.
 *
 * "Content-protected" means out of screen shares and never hidden from the
 * person at the keyboard — the same rule as `Overlay.tsx`.
 *
 * Rust owns whether the meeting records. This window asks and listens for that
 * state while keeping only the editor view, autosave and live transcript rows.
 *
 * Two states put the panel in its recording form, not one. A meeting whose
 * provider has died is still a meeting recording a room, so the indicator stays
 * up and the timer keeps counting -- what changes is the second line, because
 * the transcript is the half that stopped. Falling through to the empty panel
 * would take the indicator off screen at the exact moment there is more to say,
 * which is the thing the brief forbids.
 */

import { useEffect, useMemo, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";

import {
  dismissMeetingOffer,
  getMeetingStatus,
  getMeetingNotes,
  getMeetingProvider,
  onMeetingOffer,
  onMeetingOutcome,
  onMeetingState,
  onMeetingTranscript,
  saveMeetingNotificationPlacement,
  startMeeting,
  type MeetingState,
  type Offer,
  type Sources,
} from "./lib/ipc";
import { meetingIsLive, offerReason } from "./lib/labels";
import { MeetingPanel } from "./components/MeetingPanel";
import { emptyLiveTranscript, foldTranscriptEvent, liveRows, type LiveTranscript } from "./lib/liveTranscript";

/**
 * Which provider a detected application is recorded with before anybody has
 * chosen one under Settings > Meetings. Once somebody has, that choice is
 * used here too, so the offer and the screen agree (KNEG-317).
 */
const RECORD_WITH = "local";

export function MeetingNotification() {
  const [state, setState] = useState<MeetingState | null>(null);
  const [offer, setOffer] = useState<Offer | null>(null);
  const [sources, setSources] = useState<Sources | null>(null);
  const [elapsed, setElapsed] = useState(0);
  const [session, setSession] = useState(0);
  const [notes, setNotes] = useState<string | null>(null);
  const [transcript, setTranscript] = useState<LiveTranscript>(emptyLiveTranscript);
  const [notificationCaptureExcluded, setNotificationCaptureExcluded] =
    useState(false);
  const [problem, setProblem] = useState<string | null>(null);
  const [stateReadProblem, setStateReadProblem] = useState<string | null>(null);

  useEffect(() => {
    let disposed = false;
    let revision = 0;
    let currentState: MeetingState | null = null;
    let captureExcluded = false;
    let notesReady = false;
    const unlisteners: Array<() => void> = [];
    const keepListener = (fn: () => void) => {
      if (disposed) fn();
      else unlisteners.push(fn);
    };
    let placementTimer: number | undefined;
    const schedulePlacementSave = () => {
      if (placementTimer !== undefined) window.clearTimeout(placementTimer);
      placementTimer = window.setTimeout(() => {
        placementTimer = undefined;
        void saveMeetingNotificationPlacement().catch((err: unknown) => {
          console.warn("Could not save meeting notification placement", err);
        });
      }, 250);
    };
    const notification = getCurrentWindow();
    const enterState = (next: MeetingState) => {
      const entering = meetingIsLive(next) && (currentState === null || !meetingIsLive(currentState));
      currentState = next;
      setState(next);
      if (entering) {
        setSession((value) => value + 1);
        setNotes(null);
        setTranscript(emptyLiveTranscript());
        notesReady = false;
      }
    };
    void notification.onMoved(schedulePlacementSave).then(keepListener).catch((err: unknown) => {
      if (!disposed) console.warn("Could not listen for meeting notification moves", err);
    });
    void notification.onResized(schedulePlacementSave).then(keepListener).catch((err: unknown) => {
      if (!disposed) console.warn("Could not listen for meeting notification resizes", err);
    });
    // A hidden window survives between meetings. Refresh the runtime snapshot
    // on state transitions, and discard reads overtaken by a newer event/read.
    const refresh = () => {
      const requested = ++revision;
      void getMeetingStatus()
        .then((status) => {
          if (disposed || requested !== revision) return;
          enterState(status.state);
          if (meetingIsLive(status.state) && !notesReady) {
            void getMeetingNotes().then((draft) => {
              if (disposed || requested !== revision || !currentState || !meetingIsLive(currentState)) return;
              notesReady = true;
              setNotes(draft);
            }).catch((err: unknown) => {
              if (!disposed && requested === revision) setProblem(String(err));
            });
          }
          setOffer(status.offer);
          setSources(status.sources);
          setElapsed(status.elapsedSeconds);
          setStateReadProblem(null);
          const excluded = status.notificationExcluded?.state === "active";
          captureExcluded = excluded;
          setNotificationCaptureExcluded(excluded);
        })
        .catch((err: unknown) => {
          if (!disposed && requested === revision) setStateReadProblem(String(err));
        });
    };

    const subscriptions = [
      onMeetingState((next) => {
        enterState(next);
        if (!meetingIsLive(next)) {
          setSources(null);
          setElapsed(0);
        }
        refresh();
      }).then(keepListener),
      onMeetingOffer((next) => {
        // Invalidate any pending snapshot which may contain a withdrawn offer.
        ++revision;
        setOffer(next);
        refresh();
      }).then(keepListener),
      onMeetingTranscript((event) => {
        if (currentState === "recording" && captureExcluded) setTranscript((previous) => foldTranscriptEvent(previous, event));
      }).then(keepListener),
      onMeetingOutcome(() => {
        setTranscript(emptyLiveTranscript());
      }).then(keepListener),
    ];
    // Subscribe before reading so a transition between the two is not lost.
    void Promise.all(subscriptions).then(() => {
      if (!disposed) refresh();
    }).catch((err: unknown) => {
      if (!disposed) setStateReadProblem(String(err));
    });
    return () => {
      disposed = true;
      if (placementTimer !== undefined) window.clearTimeout(placementTimer);
      unlisteners.forEach((fn) => fn());
    };
  }, []);

  const transcriptEnded = state === "recording-without-transcript";
  const stalled = state === "stalled";
  // Asked of `meetingIsLive` rather than compared here: a state added to the
  // union does not break a `===`, so this window would have gone on compiling
  // and stopped offering Stop. See the note on that function.
  const recording = state !== null && meetingIsLive(state);
  const rows = useMemo(() => liveRows(transcript), [transcript]);

  // Counted here rather than asked for every second: the runtime owns the
  // session and the clock, and one command a second to render a timer would be
  // an IPC round trip for something arithmetic can do. Re-synced from
  // `getMeetingStatus` on every runtime state transition.
  useEffect(() => {
    if (!recording) return undefined;
    const timer = window.setInterval(() => setElapsed((s) => s + 1), 1000);
    return () => window.clearInterval(timer);
  }, [recording]);

  if (recording) {
    return <MeetingPanel session={session} elapsed={elapsed}
      detail={stalled ? "recording stopped — press Stop to keep what was captured" : transcriptEnded ? "live transcript stopped — still recording" : whoIsRecorded(sources)}
      alert={transcriptEnded || stalled} transcriptAllowed={notificationCaptureExcluded}
      rows={rows} initialNotes={notes} onProblem={setProblem} />;
  }

  if (state === "finishing") {
    return (
      <div className="notify" data-tauri-drag-region="deep">
        <div className="notify-header" title="Drag to move">
        <span className="indicator" aria-hidden="true" />
        <span className="notify-body">
          <strong>Transcribing</strong>
          <span className="notify-detail">
            the recording is being transcribed; this can take a while
          </span>
        </span>
        </div>
      </div>
    );
  }

  if (offer) {
    return (
      <div className="notify" data-tauri-drag-region="deep">
        <div className="notify-header" title="Drag to move">
        <span className="indicator" aria-hidden="true" />
        <span className="notify-body">
          <strong>{offer.name} meeting in progress</strong>
          <span className="notify-detail">{problem ?? offerReason(offer)}</span>
        </span>
        <button
          type="button"
          onClick={() => {
            setProblem(null);
            void getMeetingProvider()
              .then((saved) => startMeeting(saved ?? RECORD_WITH, true))
              .catch((err: unknown) => {
                // Shown here rather than swallowed. The most likely cause is no
                // local model server running, and being told that beats a button
                // that does nothing.
                setProblem(String(err));
              });
          }}
        >
          Record
        </button>
        <button
          type="button"
          onClick={() => {
            void dismissMeetingOffer();
          }}
        >
          Not now
        </button>
        </div>
      </div>
    );
  }

  if (stateReadProblem && state === null) {
    return (
      <div className="notify" data-tauri-drag-region="deep">
        <div className="notify-header" title="Drag to move">
        <span className="indicator" aria-hidden="true" />
        <span className="notify-body">
          <strong>State unknown</strong>
          <span className="notify-detail notify-alert">{stateReadProblem}</span>
        </span>
        </div>
      </div>
    );
  }

  // Nothing to say. The window is hidden in this state, so this is only what a
  // stray render would show — and an empty panel is better than a stale one.
  return <div className="notify notify-empty" data-tauri-drag-region="deep" />;
}

/** Elapsed seconds as `m:ss`, or `h:mm:ss` once it has been an hour. */
export function clock(seconds: number): string {
  const whole = Math.max(0, Math.floor(seconds));
  const s = whole % 60;
  const m = Math.floor(whole / 60) % 60;
  const h = Math.floor(whole / 3600);
  const pad = (n: number) => String(n).padStart(2, "0");
  return h > 0 ? `${h}:${pad(m)}:${pad(s)}` : `${m}:${pad(s)}`;
}

/** Which captures are live, in the fewest words that stay true. */
function whoIsRecorded(sources: Sources | null): string {
  if (!sources) return "starting…";
  if (sources.microphone && sources.systemAudio) return "you and everyone else";
  if (sources.microphone) return "you only";
  return "nothing is being captured";
}
