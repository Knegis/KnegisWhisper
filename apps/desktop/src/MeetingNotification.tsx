/**
 * The meeting notification window's contents.
 *
 * 392×92, content-protected, bottom-right. Two jobs in one window:
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
 * Like the overlay, it renders Rust's state and keeps none of its own. Recording
 * is a fact the runtime owns; this window asks and listens.
 */

import { useEffect, useState } from "react";

import {
  dismissMeetingOffer,
  getMeetingStatus,
  onMeetingOffer,
  onMeetingState,
  startMeeting,
  stopMeeting,
  type MeetingState,
  type Offer,
  type Sources,
} from "./lib/ipc";

/** Which provider a detected application should be recorded with. */
const RECORD_WITH = "local";

export function MeetingNotification() {
  const [state, setState] = useState<MeetingState | null>(null);
  const [offer, setOffer] = useState<Offer | null>(null);
  const [sources, setSources] = useState<Sources | null>(null);
  const [elapsed, setElapsed] = useState(0);
  const [problem, setProblem] = useState<string | null>(null);

  useEffect(() => {
    // Read once as well as subscribing: the window is hidden until there is
    // something to say, so by the time it is shown the event that caused it may
    // already have been delivered.
    void getMeetingStatus().then((status) => {
      setState(status.state);
      setSources(status.sources);
      setElapsed(status.elapsedSeconds);
    });

    const unlisteners: Array<() => void> = [];
    void onMeetingState(setState).then((fn) => unlisteners.push(fn));
    void onMeetingOffer(setOffer).then((fn) => unlisteners.push(fn));
    return () => unlisteners.forEach((fn) => fn());
  }, []);

  const recording = state === "recording";

  // Counted here rather than asked for every second: the runtime owns the
  // session and the clock, and one command a second to render a timer would be
  // an IPC round trip for something arithmetic can do. Re-synced from
  // `getMeetingStatus` whenever the window is shown, so it cannot drift far.
  useEffect(() => {
    if (!recording) return undefined;
    const timer = window.setInterval(() => setElapsed((s) => s + 1), 1000);
    return () => window.clearInterval(timer);
  }, [recording]);

  if (recording) {
    return (
      <div className="notify notify-recording">
        <span className="indicator indicator-recording" aria-hidden="true" />
        <span className="notify-body">
          <strong>Recording {clock(elapsed)}</strong>
          <span className="notify-detail">{whoIsRecorded(sources)}</span>
        </span>
        <button
          type="button"
          onClick={() => {
            void stopMeeting().catch((err: unknown) => setProblem(String(err)));
          }}
        >
          Stop
        </button>
      </div>
    );
  }

  if (state === "finishing") {
    return (
      <div className="notify">
        <span className="indicator" aria-hidden="true" />
        <span className="notify-body">
          <strong>Transcribing</strong>
          <span className="notify-detail">
            the recording is being transcribed; this can take a while
          </span>
        </span>
      </div>
    );
  }

  if (offer) {
    return (
      <div className="notify">
        <span className="indicator" aria-hidden="true" />
        <span className="notify-body">
          <strong>{offer.name} meeting in progress</strong>
          <span className="notify-detail">
            {problem ?? `detected as ${offer.evidence}`}
          </span>
        </span>
        <button
          type="button"
          onClick={() => {
            setProblem(null);
            void startMeeting(RECORD_WITH, true).catch((err: unknown) => {
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
    );
  }

  // Nothing to say. The window is hidden in this state, so this is only what a
  // stray render would show — and an empty panel is better than a stale one.
  return <div className="notify notify-empty" />;
}

/** Elapsed seconds as `m:ss`, or `h:mm:ss` once it has been an hour. */
function clock(seconds: number): string {
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
