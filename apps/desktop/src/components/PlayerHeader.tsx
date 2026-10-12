import { Icon } from "./Icon";

type PlayerHeaderProps = {
  /** Who is speaking at the player's time (`lastStartedLine`'s speaker); null when nobody is known. */
  speaker: string | null;
  playing: boolean;
  /** The selection's "a–b" (`selectionInterval`), while two or more lines are selected; null otherwise. */
  selection: string | null;
  onPlaySelection: () => void;
  onClearSelection: () => void;
};

/**
 * The line across the top of the player card while the transcript is on screen (KNEG-448): who
 * is speaking -- a 6px square, `--band` while the recording plays and muted while it is paused,
 * and the name -- or, while lines are selected, "Play selection (a–b)" and a button that clears
 * the selection. A fixed 24px row whichever it shows, and when it shows nothing, so the card does
 * not change height as playback passes a line nobody is known to have said.
 */
export function PlayerHeader({ speaker, playing, selection, onPlaySelection, onClearSelection }: PlayerHeaderProps) {
  return (
    <div className="player-header">
      {selection !== null ? (
        <>
          <button type="button" className="player-header-play" onClick={onPlaySelection}>
            Play selection <span className="player-header-range">({selection})</span>
          </button>
          <button type="button" className="player-header-clear" aria-label="Clear selection" title="Clear selection" onClick={onClearSelection}>
            <Icon name="close" size={14} />
          </button>
        </>
      ) : speaker !== null ? (
        <>
          <span className={playing ? "player-header-marker player-header-marker-playing" : "player-header-marker"} aria-hidden="true" />
          <span className="player-header-speaker">{speaker}</span>
        </>
      ) : null}
    </div>
  );
}
