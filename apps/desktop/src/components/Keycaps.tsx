import { Fragment } from "react";

import { keycapGroups } from "../lib/keycaps";

/**
 * Shortcuts as keycaps: `Control+Shift+D` is three, and a second accelerator
 * on the same action is joined with "or". Dictation's hero and Settings ›
 * Shortcuts draw them the same way (KNEG-331).
 *
 * Only for accelerators that parsed. An invalid one is what was typed, and
 * Settings › Shortcuts shows it as text beside the reason it failed.
 */
export function Keycaps({ accelerators }: { accelerators: readonly string[] }) {
  return keycapGroups(accelerators).map((keys, i) => (
    <Fragment key={`${i}-${keys.join("+")}`}>
      {i > 0 ? " or " : null}
      <span className="keycaps">
        {keys.map((key, k) => (
          <kbd key={`${k}-${key}`}>{key}</kbd>
        ))}
      </span>
    </Fragment>
  ));
}
