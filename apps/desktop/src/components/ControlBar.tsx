import type { ReactNode } from "react";

import { Icon } from "./Icon";

/**
 * The bar at the foot of a task page: a strip of chips, the privacy lines,
 * and a body below them for the page's result and its one primary button.
 *
 * `privacy` comes from `lib/controlBar.ts`' `privacyLines` and is drawn
 * whatever the page passes as `strip`, because the privacy line is never
 * hidden (the redesign's decision 5). When the strip is too narrow for chips
 * and sentences on one line -- the 720px minimum window -- the sentences wrap
 * onto a line of their own rather than being cut: a chip may ellipsise, since
 * its `title` carries the rest, but a sentence about where your audio goes
 * may not.
 *
 * `role="group"` with a label so a screen reader hears the strip as one set
 * of settings in effect, not as a stray run of buttons.
 */
export function ControlBar({
  strip,
  privacy,
  children,
}: {
  strip: ReactNode;
  privacy: readonly string[];
  children: ReactNode;
}) {
  return (
    <div className="control-bar">
      <div className="control-strip" role="group" aria-label="Settings in effect">
        <div className="control-chips">{strip}</div>
        <div className="control-spacer" />
        <ul className="control-privacy">
          {privacy.map((line) => (
            <li key={line}>
              <Icon name="lock" size={14} />
              {line}
            </li>
          ))}
        </ul>
      </div>
      <div className="control-body">{children}</div>
    </div>
  );
}
