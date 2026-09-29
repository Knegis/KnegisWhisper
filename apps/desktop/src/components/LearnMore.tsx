import type { ReactNode } from "react";

/**
 * The rest of the explanation, folded under a disclosure so a screen for a
 * colleague who isn't technical leads with the intro sentence and lets
 * anyone go looking for more.
 *
 * `open` unfolds it when a page has a reason to -- Dictation's history
 * options, while history is off. It sets the attribute rather than holding
 * state, so the person can still fold it: React only touches `open` again
 * when the value passed here changes.
 */
export function LearnMore({
  summary = "Learn more",
  open,
  children,
}: {
  summary?: string;
  open?: boolean;
  children: ReactNode;
}) {
  return (
    <details className="learn-more" open={open}>
      <summary>{summary}</summary>
      <div className="learn-more-body">{children}</div>
    </details>
  );
}
