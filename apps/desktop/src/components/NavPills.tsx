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
 */
export function JobPills({ view, onSelect }: { view: ViewId; onSelect: (id: ViewId) => void }) {
  const current = railOf(view);
  return (
    <nav className="nav-pills" aria-label="Main navigation">
      {JOBS.map((id) => (
        <button
          key={id}
          type="button"
          className="nav-pill"
          aria-current={current === id ? "page" : undefined}
          onClick={() => onSelect(id)}
        >
          {labelOf(id)}
        </button>
      ))}
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
 * KnegisWhisper's mark: a diamond of sixteen dots, the dot-matrix motif of the
 * design language in the application's own shape. Columns of 1, 2, 3, 4, 3, 2
 * and 1 dots, each centred on the middle row.
 */
const MARK_DOTS = [1, 2, 3, 4, 3, 2, 1].flatMap((count, column) =>
  Array.from({ length: count }, (_, row) => ({ cx: 3 + column * 4, cy: 15 - (count - 1) * 2 + row * 4 })),
);

export function DotMark({ size = 30 }: { size?: number }) {
  return (
    <svg className="dot-mark" role="img" aria-label="KnegisWhisper" width={size} height={size} viewBox="0 0 30 30">
      <g fill="currentColor">
        {MARK_DOTS.map(({ cx, cy }) => <circle key={`${cx}-${cy}`} cx={cx} cy={cy} r={1.7} />)}
      </g>
    </svg>
  );
}
