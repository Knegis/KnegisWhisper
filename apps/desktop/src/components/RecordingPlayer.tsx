import {
  forwardRef,
  useCallback,
  useEffect,
  useImperativeHandle,
  useRef,
  useState,
  type CSSProperties,
} from "react";

import { formatClock } from "../lib/clock";
import { recordingSrc } from "../lib/ipc";

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
  /**
   * Go to `seconds`, playing on if it was playing: a word of the transcript
   * clicked (KNEG-408). A row or selection being played still stops at its
   * end when `seconds` is inside it, and plays on when it is past it.
   */
  seek: (seconds: number) => void;
};

type RecordingPlayerProps = {
  noteId: number;
  onCurrentTime: (seconds: number) => void;
  onPlayingChange: (playing: boolean) => void;
  compact?: boolean;
};

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
  function RecordingPlayer({ noteId, onCurrentTime, onPlayingChange, compact = false }, ref) {
    const audioRef = useRef<HTMLAudioElement>(null);
    const intervalEndRef = useRef<number | null>(null);
    const frameRef = useRef<number | null>(null);
    const scrubbingRef = useRef(false);
    const [currentTime, setCurrentTime] = useState(0);
    const [duration, setDuration] = useState(0);
    const [playing, setPlaying] = useState(false);
    const [speed, setSpeed] = useState("1");
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
      if (audio === null) return false;
      try {
        await audio.play();
        return true;
      } catch (cause) {
        setError(cause instanceof Error ? cause.message : "The recording could not be played.");
        return false;
      }
    }, []);

    useImperativeHandle(
      ref,
      () => ({
        playInterval: (start, end) => {
          const audio = audioRef.current;
          if (audio === null) return;
          clearIntervalPlayback();
          audio.currentTime = start;
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
        seek: (seconds) => {
          const audio = audioRef.current;
          if (audio === null) return;
          const end = intervalEndRef.current;
          if (end !== null && seconds >= end) clearIntervalPlayback();
          audio.currentTime = seconds;
          setCurrentTime(seconds);
          onCurrentTime(seconds);
        },
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
      const updateDuration = () => setDuration(Number.isFinite(audio.duration) ? audio.duration : 0);
      const handlePlay = () => {
        setPlaying(true);
        onPlayingChange(true);
      };
      const handlePause = () => {
        setPlaying(false);
        onPlayingChange(false);
      };
      const handleEnded = () => clearIntervalPlayback();
      const handleError = () => setError(mediaErrorMessage(audio.error));
      audio.addEventListener("timeupdate", updateTime);
      audio.addEventListener("loadedmetadata", updateDuration);
      audio.addEventListener("durationchange", updateDuration);
      audio.addEventListener("play", handlePlay);
      audio.addEventListener("pause", handlePause);
      audio.addEventListener("ended", handleEnded);
      audio.addEventListener("error", handleError);
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
      };
    }, [clearIntervalPlayback, onCurrentTime, onPlayingChange]);

    const scrub = (next: number) => {
      const audio = audioRef.current;
      if (audio === null) return;
      clearIntervalPlayback();
      audio.currentTime = next;
      setCurrentTime(next);
      onCurrentTime(next);
    };

    return (
      <div className={`recording-player${compact ? " recording-player-compact" : ""}`} aria-label="Recording player">
        {/* In browser dev the fixtures answer this URL with a two-minute WAV
            for every note that has a recording (`lib/devFixtures.ts`'
            `servePlayableRecordings`, KNEG-412), so the player plays there
            too; an id they have no recording for fails as Rust's 404 would. */}
        <audio ref={audioRef} preload="metadata" src={recordingSrc(noteId)} className="recording-audio" />
        <button
          type="button"
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
          {playing ? "Pause" : "Play"}
        </button>
        <span className="recording-clock">{formatClock(currentTime)} / {formatClock(duration)}</span>
        <label>
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
        <input
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
        {error ? <p className="error">The recording could not be loaded. {error}</p> : null}
      </div>
    );
  },
);
