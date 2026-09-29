import type { Readiness } from "../lib/readiness";
import { LearnMore } from "./LearnMore";

/**
 * The machine-voice line that opens Dictation and Meetings while no session
 * is under way -- both pages leave it out once one has started, since "will
 * starting work?" is no longer the question:
 * "Ready", with an optional caution underneath that never blocks anything, or
 * "Needs setup", with the one problem, the one button that fixes it, and the
 * detail -- when there is one -- folded under a {@link LearnMore} disclosure
 * rather than shown open.
 *
 * `fixIsPrimary` is the page's to decide, because only the page knows whether
 * its Start still works. Where readiness disables Start, the fix is the one
 * thing to do next and takes the page's single `button.primary`, and Start
 * gives it up -- otherwise the one filled button on the screen was the one
 * that could not be pressed. Where Start still works (Dictation with no
 * shortcut: the button needs none), Start keeps it and the fix stays plain.
 * Either way there is at most one primary on the screen.
 *
 * Generic in the same `T` `Readiness<T>` is generic in, so a page's own
 * `onGoTo: (id: ViewId) => void` type-checks as `onFix` with no cast.
 */
export function StatusLine<T extends string>({
  readiness,
  onFix,
  fixIsPrimary,
}: {
  readiness: Readiness<T>;
  onFix: (view: T) => void;
  fixIsPrimary: boolean;
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
      <button
        type="button"
        className={fixIsPrimary ? "primary" : undefined}
        onClick={() => onFix(readiness.fix.view)}
      >
        {readiness.fix.label}
      </button>
      {readiness.detail ? <LearnMore>{readiness.detail}</LearnMore> : null}
    </div>
  );
}
