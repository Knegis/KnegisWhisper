import type { ReactNode } from "react";

import type { IconName } from "../lib/icons";
import { Icon } from "./Icon";

/**
 * The centre of a task page: a large icon, a one-line status in the machine
 * voice, the heading that says what to do, and a sentence under it.
 *
 * Four tones. The three that mark a state are each a shape as well as a
 * colour, so none of them rests on colour alone:
 *
 *   - **ready** -- a filled `--band` square;
 *   - **recording** -- a round `--rec` dot, the one round mark the Kit
 *     allows, followed by `extra` (the page's timer);
 *   - **setup** -- a hollow `--text` square. Not orange: the redesign has no
 *     warning colour (decision 6), and the canvas' amber was the brown the
 *     theme removed;
 *   - **busy** -- no mark, only the label: work under way that is neither
 *     ready nor recording, such as Dictation's "Transcribing…" and a
 *     meeting's. The ready square there read as "Ready" beside a label that
 *     said otherwise.
 *
 * The label's own text is `--text` except while recording, where it takes
 * `--rec` as the canvas draws it. The canvas also paints "Ready" blue; that
 * is left out because `#3d55ff` on the dark card is 3.3:1, under the 4.5:1 a
 * 12px label needs, and the square already carries the blue.
 *
 * Only the label is a live region. `extra` ticks every second while
 * recording, and a status that announced each tick would talk over the
 * person dictating.
 *
 * The heading is an `h2`: the page's `h1` is its list panel's title, or,
 * while the panel is collapsed, the visually hidden one `SplitView` draws in
 * its place.
 * `children` is for the setup state's fix button and its Learn more.
 */
export function TaskHero({
  icon,
  status,
  heading,
  sub,
  children,
}: {
  icon: IconName;
  status: { tone: "ready" | "recording" | "setup" | "busy"; label: string; extra?: string };
  heading: ReactNode;
  sub: ReactNode;
  children?: ReactNode;
}) {
  return (
    <div className="hero">
      <span className="hero-icon">
        <Icon name={icon} size={60} />
      </span>
      <p className={`hero-status hero-status-${status.tone}`}>
        {status.tone === "busy" ? null : <span className="hero-mark" aria-hidden="true" />}
        <span role="status">{status.label}</span>
        {status.extra === undefined ? null : <span className="hero-extra">{status.extra}</span>}
      </p>
      <h2 className="hero-heading">{heading}</h2>
      <div className="hero-sub">{sub}</div>
      {children === undefined ? null : <div className="hero-actions">{children}</div>}
    </div>
  );
}
