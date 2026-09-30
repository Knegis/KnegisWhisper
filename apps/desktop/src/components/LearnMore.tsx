import { useLayoutEffect, useRef, type ReactNode } from "react";

/**
 * The rest of the explanation, folded under a disclosure so a screen for a
 * colleague who isn't technical leads with the intro sentence and lets
 * anyone go looking for more.
 *
 * `unfold` opens it when a page has a reason to -- Dictation's history
 * options, when history is off -- and only ever opens it. Whether it is open
 * is otherwise the person's: nothing here holds it or passes `open` to
 * React, which would fold it again the moment the reason went away. That is
 * what `open={!historyOn}` did, snapping the options shut under the pointer
 * as "Save dictation history" was ticked back on and dropping focus from the
 * checkbox inside. Before the first paint, so an options block that should
 * start open does not flash shut first.
 */
export function LearnMore({
  summary = "Learn more",
  unfold = false,
  children,
}: {
  summary?: string;
  unfold?: boolean;
  children: ReactNode;
}) {
  const details = useRef<HTMLDetailsElement>(null);
  useLayoutEffect(() => {
    if (unfold && details.current) details.current.open = true;
  }, [unfold]);
  return (
    <details ref={details} className="learn-more">
      <summary>{summary}</summary>
      <div className="learn-more-body">{children}</div>
    </details>
  );
}
