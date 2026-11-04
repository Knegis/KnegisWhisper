import { NAV, RAIL, railOf, type RailId, type ViewId } from "../lib/navigation";

/** A pill's label: `NAV`'s own, so the pills and the tree cannot disagree. */
function labelOf(id: RailId): string {
  return NAV.find((entry) => entry.id === id)?.label ?? id;
}

/** The three jobs, in `RAIL`'s order; Settings is drawn on its own, at the other end of the bar. */
const JOBS = RAIL.filter((id): id is Exclude<RailId, "settings"> => id !== "settings");

/**
 * The three jobs as pills in the title bar: Dictation, Meetings, Notes.
 *
 * They replace the rail. Exactly one reads as current when a job's screen is
 * showing -- `railOf(view)`, so Notes > Upload marks Notes -- and none does on
 * a settings screen, where the Settings pill is the marked one.
 *
 * While a meeting records, the Meetings pill carries the pulsing signal dot
 * from anywhere in the window, so leaving the page does not hide that the
 * microphone is open. Its name says so too: the dot alone says nothing to a
 * screen reader.
 */
export function JobPills({ view, onSelect, meetingRecording }: { view: ViewId; onSelect: (id: ViewId) => void; meetingRecording: boolean }) {
  const current = railOf(view);
  return (
    <nav className="nav-pills" aria-label="Main navigation">
      {JOBS.map((id) => {
        const recording = meetingRecording && id === "meetings";
        return (
          <button
            key={id}
            type="button"
            className="nav-pill"
            aria-current={current === id ? "page" : undefined}
            aria-label={recording ? `${labelOf(id)}, recording` : undefined}
            onClick={() => onSelect(id)}
          >
            {recording ? <span className="nav-pill-dot" aria-hidden="true" /> : null}
            {labelOf(id)}
          </button>
        );
      })}
    </nav>
  );
}

/**
 * The Settings pill. It opens the Settings sheet rather than a screen: every
 * settings and developer screen is a tile there. It reads as current while one
 * of those screens is showing, as the rail's Settings entry did.
 */
export function SettingsPill({ view, open, onOpen }: { view: ViewId; open: boolean; onOpen: () => void }) {
  return (
    <button
      type="button"
      className="nav-pill"
      aria-haspopup="dialog"
      aria-expanded={open}
      aria-current={railOf(view) === "settings" ? "page" : undefined}
      onClick={onOpen}
    >
      {labelOf("settings")}
    </button>
  );
}

/**
 * KnegisWhisper's mark: the owner's frog in a cowboy hat. Drawn as a mask over the text colour, so it
 * follows the theme. The file is their drawing as an alpha mask, trimmed to its ink, with its lines
 * thickened slightly so the eye and the hat still read at 40px on a 100% display.
 */
export function KnegisMark({ size = 40 }: { size?: number }) {
  return <span className="knegis-mark" role="img" aria-label="KnegisWhisper" style={{ width: size, height: size }} />;
}
