/**
 * A refusal, told apart from a failure (KNEG-414).
 *
 * KNEG-343 folded every error on the settings screens under one visible line,
 * "That didn't work.", with the error's own text under Details, because that
 * text is usually Rust's: ids, ports, paths. A refusal is different. It is a
 * sentence written for the person, and what to do is the whole of it -- "Not
 * now: a meeting is being transcribed on this computer. Try again when it's
 * finished." -- so folding it hid the one line that mattered.
 *
 * Rust starts every such sentence with "Not now: " (`local_model.rs`
 * `ServerUsers`, `local_llm.rs`), and its tests pin that; `FailureLine` shows
 * a sentence that starts so as the line itself. Keep the two in step.
 *
 * Pure -- no React -- so it is tested (`tests/refusal.test.ts`).
 */

const NOT_NOW = "Not now: ";

/** The sentence to show in view when `detail` is a refusal, or `null`. */
export function refusalText(detail: string | null | undefined): string | null {
  return detail?.startsWith(NOT_NOW) ? detail : null;
}
