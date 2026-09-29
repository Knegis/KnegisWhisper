import type { ReactNode } from "react";

/**
 * The rest of the explanation, folded under a disclosure so a screen for a
 * colleague who isn't technical leads with the intro sentence and lets
 * anyone go looking for more.
 */
export function LearnMore({
  summary = "Learn more",
  children,
}: {
  summary?: string;
  children: ReactNode;
}) {
  return (
    <details className="learn-more">
      <summary>{summary}</summary>
      <div className="learn-more-body">{children}</div>
    </details>
  );
}
