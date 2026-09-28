import type { Readiness } from "../lib/readiness";
import { LearnMore } from "./LearnMore";

/**
 * The machine-voice line that opens Dictation (Task 6) and Meetings (Task 7)
 * while no session is under way -- both pages leave it out once one has
 * started, since "will starting work?" is no longer the question:
 * "Ready", with an optional caution underneath that never blocks anything, or
 * "Needs setup", with the one problem, the one button that fixes it, and the
 * detail -- when there is one -- folded under a {@link LearnMore} disclosure
 * rather than shown open.
 *
 * Generic in the same `T` `Readiness<T>` is generic in, so a page's own
 * `onGoTo: (id: ViewId) => void` type-checks as `onFix` once that page's own
 * fix targets exist, with no cast.
 */
export function StatusLine<T extends string>({
  readiness,
  onFix,
}: {
  readiness: Readiness<T>;
  onFix: (view: T) => void;
}) {
  if (readiness.state === "ready") {
    return (
      <>
        <p className="status-line" role="status">
          Ready
        </p>
        {readiness.caution ? <p className="caveat">{readiness.caution}</p> : null}
      </>
    );
  }

  return (
    <div className="caveat status-needs-setup" role="status">
      <p className="status-line">Needs setup</p>
      <p>{readiness.problem}</p>
      <button type="button" onClick={() => onFix(readiness.fix.view)}>
        {readiness.fix.label}
      </button>
      {readiness.detail ? <LearnMore>{readiness.detail}</LearnMore> : null}
    </div>
  );
}
