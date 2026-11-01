import type { ReactNode } from "react";

import type { IconName } from "../lib/icons";
import { Icon } from "./Icon";

/**
 * The centre of a task page: the icon on a breathing orb of colour, a
 * one-line status in the pixel voice on a pill, the heading that says what to
 * do in the Dusk gradient, and a sentence under it.
 *
 * The orb is the design language's "breathe" picture, and its pace is the
 * state: slow at rest, quicker and warmer while recording, quicker again
 * while busy, and still while something needs setting up. It is decoration
 * (`aria-hidden`); the status says the same thing in words.
 *
 * Four tones. The three that mark a state are each a shape as well as a
 * colour, so none of them rests on colour alone:
 *
 *   - **ready** -- a filled dot;
 *   - **recording** -- the pink signal dot, pulsing, followed by `extra`
 *     (the page's timer);
 *   - **setup** -- a hollow ring. Not orange: the redesign has no warning
 *     colour (decision 6);
 *   - **busy** -- no mark, only the label: work under way that is neither
 *     ready nor recording, such as Dictation's "Transcribing…" and a
 *     meeting's. A ready mark there read as "Ready" beside a label that
 *     said otherwise.
 *
 * The label's own text is `--text` except while recording, where it takes
 * `--rec`.
 *
 * Only the label is a live region. `extra` ticks every second while
 * recording, and a status that announced each tick would talk over the
 * person dictating.
 *
 * The heading is an `h2`: the page's `h1` is its list panel's title, or,
 * while the panel is collapsed, the visually hidden one `SplitView` draws in
 * its place.
 * `meter` is for a recording's live sound level (`LevelMeter`, KNEG-371),
 * under the heading. `children` is for the setup state's fix button and its
 * Learn more.
 */
export function TaskHero({
  icon,
  status,
  heading,
  meter,
  sub,
  children,
}: {
  icon: IconName;
  status: { tone: "ready" | "recording" | "setup" | "busy"; label: string; extra?: string };
  heading: ReactNode;
  meter?: ReactNode;
  sub: ReactNode;
  children?: ReactNode;
}) {
  return (
    <div className="hero" data-tone={status.tone}>
      <span className="hero-icon">
        <span className="hero-orb" aria-hidden="true" />
        <Icon name={icon} size={52} />
      </span>
      <p className={`hero-status hero-status-${status.tone}`}>
        {status.tone === "busy" ? null : <span className="hero-mark" aria-hidden="true" />}
        <span role="status">{status.label}</span>
        {status.extra === undefined ? null : <span className="hero-extra">{status.extra}</span>}
      </p>
      <h2 className="hero-heading">{heading}</h2>
      {meter === undefined ? null : <div className="hero-meter">{meter}</div>}
      <div className="hero-sub">{sub}</div>
      {children === undefined ? null : <div className="hero-actions">{children}</div>}
    </div>
  );
}
