import {
  forwardRef,
  useCallback,
  useEffect,
  useImperativeHandle,
  useMemo,
  useRef,
  useState,
  type CSSProperties,
  type ReactNode,
} from "react";

import { formatClock } from "../lib/clock";
import { recordingSrc } from "../lib/ipc";
import { SKIP_SECONDS, markFractions, skipTarget } from "../lib/playerControls";
import { IconFilled } from "./Icon";

/**
 * Where a player is: its time, the end of the interval it is playing to (or
 * `null`), whether it is playing, and its speed.
 */
export type PlayerPosition = { time: number; end: number | null; playing: boolean; rate: number };

export type RecordingPlayerHandle = {
  playInterval: (start: number, end: number | null) => void;
  /**
   * `position` and `resume` hand playback from one player to the next. A note
   * mounts a new player when a wide window moves it into the transcript
   * column, or back, and when a wide window's Replay tab is opened or left
   * (`NoteWorkspace.tsx`). `position` is read from the old player before it
   * goes, and the new one resumes from it.
   */
  position: () => PlayerPosition | null;
  resume: (from: PlayerPosition) => void;
  /**
   * The recording's time now, read from the element rather than from the
   * last `timeupdate` (four a second), for a transcript row highlighting word
   * by word on every animation frame (KNEG-408). `null` with no player.
   */
  time: () => number | null;
  pause: () => void;
};

type RecordingPlayerProps = {
  noteId: number;
  onCurrentTime: (seconds: number) => void;
  onPlayingChange: (playing: boolean) => void;
  onReadyChange?: (ready: boolean) => void;
  /** A line above the scrubber, for whoever holds the player to fill (KNEG-448). */
  header?: ReactNode;
  /**
   * Times, in seconds, to tick on the scrubber: where the transcript's
   * sections start (KNEG-448). Placed once the recording's length is known.
   */
  marks?: readonly number[];
};

const NO_MARKS: readonly number[] = [];

// Moved to `lib/clock.ts`, where a search hit's time reads it too (KNEG-406),
// and re-exported for the pages that already import it from here.
export { formatClock };

function mediaErrorMessage(error: MediaError | null): string {
  switch (error?.code) {
    case MediaError.MEDIA_ERR_ABORTED:
      return "Playback was aborted.";
    case MediaError.MEDIA_ERR_NETWORK:
      return "A network error interrupted the recording.";
    case MediaError.MEDIA_ERR_DECODE:
      return "The recording could not be decoded.";
    case MediaError.MEDIA_ERR_SRC_NOT_SUPPORTED:
      return "The recording source is not supported.";
    default:
      return "No further detail is available.";
  }
}

export const RecordingPlayer = forwardRef<RecordingPlayerHandle, RecordingPlayerProps>(
  function RecordingPlayer({ noteId, onCurrentTime, onPlayingChange, onReadyChange, header = null, marks = NO_MARKS }, ref) {
    const audioRef = useRef<HTMLAudioElement>(null);
    const intervalEndRef = useRef<number | null>(null);
    const frameRef = useRef<number | null>(null);
    const scrubbingRef = useRef(false);
    const [currentTime, setCurrentTime] = useState(0);
    const [duration, setDuration] = useState(0);
    const [playing, setPlaying] = useState(false);
    const [speed, setSpeed] = useState("1");
    const readyRef = useRef(false);
    const resumeRef = useRef<PlayerPosition | null>(null);
    const [ready, setReady] = useState(false);
    const [error, setError] = useState<string | null>(null);

    const clearIntervalPlayback = useCallback(() => {
      intervalEndRef.current = null;
      if (frameRef.current !== null) {
        cancelAnimationFrame(frameRef.current);
        frameRef.current = null;
      }
    }, []);

    const checkInterval = useCallback(() => {
      const audio = audioRef.current;
      const end = intervalEndRef.current;
      if (audio === null || end === null) return;
      if (audio.paused) {
        clearIntervalPlayback();
        return;
      }
      if (audio.currentTime >= end) {
        audio.currentTime = end;
        audio.pause();
        clearIntervalPlayback();
        return;
      }
      frameRef.current = requestAnimationFrame(checkInterval);
    }, [clearIntervalPlayback]);

    const beginIntervalCheck = useCallback(() => {
      if (frameRef.current === null) {
        frameRef.current = requestAnimationFrame(checkInterval);
      }
    }, [checkInterval]);

    const play = useCallback(async (): Promise<boolean> => {
      const audio = audioRef.current;
      if (audio === null || !readyRef.current) return false;
      try {
        await audio.play();
        setError(null);
        return true;
      } catch (cause) {
        setError(cause instanceof Error ? cause.message : "The recording could not be played.");
        return false;
      }
    }, []);

    // Go to `seconds`, playing on if it was playing: the −15 / +15 buttons. A selection being
    // played still stops at its end when `seconds` is inside it; a jump at or past that end clears
    // it, so playback runs on from there. (A word clicked in the transcript plays from the word
    // through `playInterval` since KNEG-448, so the handle no longer seeks.)
    const seekTo = useCallback((seconds: number) => {
      const audio = audioRef.current;
      if (audio === null || !readyRef.current) return;
      const end = intervalEndRef.current;
      if (end !== null && seconds >= end) clearIntervalPlayback();
      audio.currentTime = seconds;
      setCurrentTime(seconds);
      onCurrentTime(seconds);
    }, [clearIntervalPlayback, onCurrentTime]);

    // From the element's own time and length rather than the last `timeupdate`, which is up to a
    // quarter of a second old; before the metadata is in, only the start limits a skip.
    const skip = (delta: number) => {
      const audio = audioRef.current;
      if (audio === null) return;
      seekTo(skipTarget(audio.currentTime, delta, audio.duration));
    };

    // Recomputed only when the marks or the length change, not on every `timeupdate`.
    const markAt = useMemo(() => markFractions(marks, duration), [marks, duration]);

    useImperativeHandle(
      ref,
      () => ({
        playInterval: (start, end) => {
          const audio = audioRef.current;
          if (audio === null || !readyRef.current) return;
          clearIntervalPlayback();
          audio.currentTime = Math.min(Math.max(start, 0), audio.duration);
          setCurrentTime(start);
          onCurrentTime(start);
          intervalEndRef.current = end;
          void play().then((started) => {
            if (started) beginIntervalCheck();
            else clearIntervalPlayback();
          });
        },
        position: () => {
          const audio = audioRef.current;
          if (audio === null) return null;
          return { time: audio.currentTime, end: intervalEndRef.current, playing: !audio.paused, rate: audio.playbackRate };
        },
        resume: ({ time, end, playing: wasPlaying, rate }) => {
          const audio = audioRef.current;
          if (audio === null) return;
          clearIntervalPlayback();
          audio.playbackRate = rate;
          setSpeed(String(rate));
          if (!readyRef.current) { resumeRef.current = { time, end, playing: wasPlaying, rate }; return; }
          // A player that has only just mounted has not read its file's metadata yet. A seek made
          // before then holds only where the engine keeps a default start position, so it is made
          // again once the metadata is in; playback cannot begin before that.
          audio.currentTime = time;
          if (audio.readyState < HTMLMediaElement.HAVE_METADATA) {
            audio.addEventListener("loadedmetadata", () => { audio.currentTime = time; }, { once: true });
          }
          setCurrentTime(time);
          onCurrentTime(time);
          if (!wasPlaying) return;
          intervalEndRef.current = end;
          void play().then((started) => {
            if (started) beginIntervalCheck();
            else clearIntervalPlayback();
          });
        },
        time: () => audioRef.current?.currentTime ?? null,
        pause: () => { audioRef.current?.pause(); clearIntervalPlayback(); },
      }),
      [beginIntervalCheck, clearIntervalPlayback, onCurrentTime, play],
    );

    useEffect(() => {
      const audio = audioRef.current;
      if (audio === null) return;
      const updateTime = () => {
        if (scrubbingRef.current) return;
        setCurrentTime(audio.currentTime);
        onCurrentTime(audio.currentTime);
      };
      const updateDuration = () => {
        const usable = !audio.error && Number.isFinite(audio.duration) && audio.duration > 0;
        readyRef.current = usable; setReady(usable); onReadyChange?.(usable);
        setDuration(usable ? audio.duration : 0);
        const from = resumeRef.current;
        if (usable && from) {
          resumeRef.current = null;
          audio.currentTime = Math.min(Math.max(from.time, 0), audio.duration);
          setCurrentTime(audio.currentTime); onCurrentTime(audio.currentTime);
          intervalEndRef.current = from.end;
          if (from.playing) void play().then(started => { if (started) beginIntervalCheck(); });
        }
      };
      const handlePlay = () => {
        setPlaying(true);
        onPlayingChange(true);
      };
      const handlePause = () => {
        setPlaying(false);
        onPlayingChange(false);
      };
      const handleEnded = () => clearIntervalPlayback();
      const handleEmptied = () => {
        readyRef.current = false; setReady(false); onReadyChange?.(false);
        setDuration(0); setCurrentTime(0); onCurrentTime(0);
      };
      const handleError = () => {
        readyRef.current = false; setReady(false); onReadyChange?.(false); resumeRef.current = null;
        audio.pause(); clearIntervalPlayback();
        setCurrentTime(0); setDuration(0); onCurrentTime(0);
        setError(mediaErrorMessage(audio.error));
      };
      updateDuration();
      audio.addEventListener("timeupdate", updateTime);
      audio.addEventListener("loadedmetadata", updateDuration);
      audio.addEventListener("durationchange", updateDuration);
      audio.addEventListener("play", handlePlay);
      audio.addEventListener("pause", handlePause);
      audio.addEventListener("ended", handleEnded);
      audio.addEventListener("error", handleError);
      audio.addEventListener("emptied", handleEmptied);
      return () => {
        clearIntervalPlayback();
        audio.pause();
        audio.removeEventListener("timeupdate", updateTime);
        audio.removeEventListener("loadedmetadata", updateDuration);
        audio.removeEventListener("durationchange", updateDuration);
        audio.removeEventListener("play", handlePlay);
        audio.removeEventListener("pause", handlePause);
        audio.removeEventListener("ended", handleEnded);
        audio.removeEventListener("error", handleError);
        audio.removeEventListener("emptied", handleEmptied);
      };
    }, [beginIntervalCheck, clearIntervalPlayback, onCurrentTime, onPlayingChange, onReadyChange, play]);

    const scrub = (next: number) => {
      const audio = audioRef.current;
      if (audio === null || !readyRef.current) return;
      clearIntervalPlayback();
      audio.currentTime = next;
      setCurrentTime(next);
      onCurrentTime(next);
    };

    // A card as the M6 canvas draws it (KNEG-448): whatever `header` the holder gives it, the
    // scrubber with a tick where each section starts, and one row of controls under it. The same
    // in all three places a note draws its one player (`NoteWorkspace.tsx`).
    return (
      <div className="recording-player" role="group" aria-label="Recording player">
        {/* In browser dev the fixtures answer this URL with a WAV as long as
            the note's recording for every note that has one
            (`lib/devFixtures.ts`' `servePlayableRecordings`, KNEG-412), so the
            player plays there too; an id they have no recording for fails as
            Rust's 404 would. */}
        <audio ref={audioRef} preload="metadata" src={recordingSrc(noteId)} className="recording-audio" />
        {header}
        <div className="recording-scrubber">
          {/* Under the range, so the thumb and the played part are drawn over a tick. */}
          {markAt.length > 0 ? (
            <div className="recording-marks" aria-hidden="true">
              {markAt.map((at) => (
                <span key={at} className="recording-mark" style={{ "--at": at } as CSSProperties} />
              ))}
            </div>
          ) : null}
          <input
            disabled={!ready}
            aria-label="Recording position"
            type="range"
            min="0"
            max={duration || 0}
            step="0.01"
            value={Math.min(currentTime, duration || currentTime)}
            // The fraction played, which `styles.css` draws in `--band` along the track.
            style={{ "--played": duration > 0 ? Math.min(currentTime / duration, 1) : 0 } as CSSProperties}
            onPointerDown={() => { scrubbingRef.current = true; }}
            onPointerUp={() => { scrubbingRef.current = false; }}
            onChange={(event) => scrub(Number(event.target.value))}
          />
        </div>
        <div className="recording-controls">
          <button
            type="button"
            disabled={!ready}
            className="recording-play"
            aria-label={playing ? "Pause" : "Play"}
            title={playing ? "Pause" : "Play"}
            onClick={() => {
              const audio = audioRef.current;
              if (audio === null) return;
              if (audio.paused) {
                clearIntervalPlayback();
                void play();
              } else {
                audio.pause();
              }
            }}
          >
            <IconFilled name={playing ? "pause" : "play"} size={16} />
          </button>
          <button type="button" disabled={!ready} className="recording-skip" aria-label={`Back ${SKIP_SECONDS} seconds`} title={`Back ${SKIP_SECONDS} seconds`} onClick={() => skip(-SKIP_SECONDS)}>
            −{SKIP_SECONDS}
          </button>
          <button type="button" disabled={!ready} className="recording-skip" aria-label={`Forward ${SKIP_SECONDS} seconds`} title={`Forward ${SKIP_SECONDS} seconds`} onClick={() => skip(SKIP_SECONDS)}>
            +{SKIP_SECONDS}
          </button>
          <span className="recording-clock">{formatClock(currentTime)}<span className="recording-clock-total"> / {formatClock(duration)}</span></span>
          <label className="recording-speed">
            <span className="sr-only">Playback speed</span>
            <select
              value={speed}
              onChange={(event) => {
                const next = Number(event.target.value);
                setSpeed(event.target.value);
                if (audioRef.current !== null) audioRef.current.playbackRate = next;
              }}
            >
              {["0.75", "1", "1.25", "1.5", "2"].map((rate) => (
                <option key={rate} value={rate}>{rate}×</option>
              ))}
            </select>
          </label>
        </div>
        {error ? <p className="error">{ready ? "The recording could not be played." : "The recording could not be loaded."} {error}</p> : null}
      </div>
    );
  },
);
