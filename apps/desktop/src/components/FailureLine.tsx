import type { ReactNode } from "react";
import { refusalText } from "../lib/refusal";
import { LearnMore } from "./LearnMore";

/**
 * A failure in plain words, with what the software said folded under Details
 * (KNEG-343).
 *
 * The settings screens' copy rule keeps a failure in view and puts an error's
 * own text -- a Rust message, an id, a path -- one click away rather than in
 * the sentence. Before this, the settings screens printed `String(e)` as the
 * whole of the failure. `children` is for an action that belongs beside the
 * sentence, such as Retry.
 *
 * A refusal is the exception (KNEG-414): a sentence Rust wrote for the
 * person, saying what to do, is shown as the line itself instead of behind
 * "That didn't work." See `lib/refusal.ts`.
 */
export function FailureLine({
  text,
  detail = null,
  children,
}: {
  text: string;
  detail?: string | null;
  children?: ReactNode;
}) {
  const refusal = refusalText(detail);
  if (refusal !== null) {
    return (
      <div className="failure-line" role="alert">
        <p className="error">
          {refusal}
          {children ? <> {children}</> : null}
        </p>
      </div>
    );
  }
  return (
    <div className="failure-line" role="alert">
      <p className="error">
        {text}
        {children ? <> {children}</> : null}
      </p>
      {detail ? (
        <LearnMore summary="Details">
          <p>{detail}</p>
        </LearnMore>
      ) : null}
    </div>
  );
}
