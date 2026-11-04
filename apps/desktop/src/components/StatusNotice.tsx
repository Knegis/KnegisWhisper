/**
 * The small rounded notice at a task page's top left: a dot, a label, and what
 * is true right now.
 *
 * The dot's shape says the state as well as its colour, so the state never rests
 * on colour alone: filled on mint for ready, the pulsing signal dot for
 * recording, a hollow ring for needs setup, and no dot for work under way.
 *
 * Only `value` is a live region. `extra` is a clock that ticks every second, and
 * a status that announced each tick would talk over the person dictating.
 */
export function StatusNotice({
  tone,
  label,
  value,
  extra,
}: {
  tone: "ready" | "recording" | "busy" | "setup";
  label: string;
  value: string;
  extra?: string;
}) {
  return (
    <div className={`notice notice-${tone}`}>
      {tone === "busy" ? null : <span className="notice-dot" aria-hidden="true" />}
      <span className="notice-body">
        <span className="notice-label">{label}</span>
        <span className="notice-value">
          {extra === undefined ? null : <span className="notice-extra">{extra}</span>}
          {extra === undefined ? null : " · "}
          <span role="status">{value}</span>
        </span>
      </span>
    </div>
  );
}
