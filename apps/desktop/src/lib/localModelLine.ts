/**
 * What Dictation says about the local model server, in plain words (KNEG-357).
 *
 * Dictation showed the server's own sentence, "Local model base is ready on
 * port 8178.", to colleagues who are not technical. The copy rules put model
 * ids, ports and error text behind Learn more, and keep failures, and anything
 * the user must do, in plain view. So:
 *
 * - **Ready, or stopped:** nothing. The privacy line already says "this
 *   computer", and a stopped server is started by the next dictation
 *   (KNEG-387), so there is nothing to do.
 * - **Starting:** one line saying so, while Start waits on it.
 * - **Could not start, or went away:** a failure, in plain view, naming what
 *   to do and where. The error text or exit report is `more`.
 *
 * Diagnostics keeps the whole sentence (`LocalStatus.runtimeMessage`).
 */
import type { LocalRuntime } from "./ipc.js";

export type LocalModelLine = {
  text: string;
  /** A failure, drawn as one. */
  problem: boolean;
  /** The mechanism, for Learn more. */
  more: string | null;
};

export function localModelLine(runtime: LocalRuntime): LocalModelLine | null {
  switch (runtime.state) {
    case "stopped":
    case "ready":
      return null;
    case "checking":
    case "loading":
      return { text: "Starting the local model…", problem: false, more: null };
    case "failed":
      return {
        text: "The local model could not start. Try dictating again, or choose another model in Settings › Transcription.",
        problem: true,
        more: runtime.error,
      };
    case "exited":
      return {
        text: "The local model stopped unexpectedly. The next dictation starts it again.",
        problem: true,
        more: runtime.report.trim() === "" ? null : runtime.report,
      };
  }
}
