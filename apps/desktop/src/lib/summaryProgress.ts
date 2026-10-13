import type { SummaryState } from "../generated/ipc-types.js";

/** Actual saved worker steps: N extracts followed by N-1 combinations. Never an ETA. */
export function summaryProgressText(state: Pick<SummaryState, "status" | "completed" | "total">): string | null {
  switch (state.status) {
    case "queued": return "Summary queued";
    case "processing": {
      if (state.total <= 0) return "Preparing transcript";
      const extracts = Math.floor((state.total + 1) / 2);
      if (state.completed >= state.total) return "Saving summary";
      if (state.completed < extracts) return `Summarising transcript · part ${state.completed + 1} of ${extracts}`;
      return `Combining summary · ${state.completed - extracts} of ${state.total - extracts} parts completed`;
    }
    case "failed": return "Summary failed";
    case "interrupted": return "Summary interrupted";
    case "cancelled": return "Summary cancelled";
    case "succeeded": return "Summary completed";
    default: return null;
  }
}
