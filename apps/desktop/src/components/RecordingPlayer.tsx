import {
  forwardRef,
  useCallback,
  useEffect,
  useImperativeHandle,
  useRef,
  useState,
} from "react";

import { recordingSrc } from "../lib/ipc";

export type RecordingPlayerHandle = {
  playInterval: (start: number, end: number | null) => void;
};

type RecordingPlayerProps = {
  noteId: number;
  onCurrentTime: (seconds: number) => void;
  onPlayingChange: (playing: boolean) => void;
};

export function formatClock(seconds: number): string {
  const total = Math.max(0, Math.floor(seconds));
  const hours = Math.floor(total / 3600);
  const minutes = Math.floor((total % 3600) / 60);
  const remaining = total % 60;
  return hours > 0
    ? `${hours}:${String(minutes).padStart(2, "0")}:${String(remaining).padStart(2, "0")}`
    : `${minutes}:${String(remaining).padStart(2, "0")}`;
}

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
  function RecordingPlayer({ noteId, onCurrentTime, onPlayingChange }, ref) {
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
      <div className="recording-player">
        {/* Browser dev fixtures intentionally have no recording server, so this
            error state is expected outside Tauri rather than hidden. */}
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
          onPointerDown={() => { scrubbingRef.current = true; }}
          onPointerUp={() => { scrubbingRef.current = false; }}
          onChange={(event) => scrub(Number(event.target.value))}
        />
        {error ? <p className="error">The recording could not be loaded. {error}</p> : null}
      </div>
    );
  },
);
