import type { AutosaveStatus } from "./useAutosave.js";

/**
 * The quiet line shown next to a note's title while it saves, or `null` when
 * there is nothing to say.
 *
 * `dirty` and `saving` read the same to someone watching the title bar: both
 * mean an edit hasn't landed yet, and the difference (waiting out the debounce
 * versus a request in flight) is not something a person needs to see twice.
 * `idle` says nothing because there is nothing pending. `failed` also returns
 * `null` here -- it already has its own alert, next to a Retry button
 * (`NoteWorkspace.tsx`'s `role="alert"` paragraph), and showing both would be
 * the same failure said twice in two voices.
 */
export function saveStatusText(status: AutosaveStatus): string | null {
  switch (status) {
    case "dirty":
    case "saving":
      return "Saving…";
    case "saved":
      return "Saved";
    case "idle":
    case "failed":
      return null;
  }
}

/**
 * The same line for a whole note: its title and personal notes, and its AI
 * summary, which saves on its own (`SummaryPane.tsx`).
 *
 * The line used to follow the first alone, so it read "Saved" while a summary
 * edit was still waiting to land, or had failed (KNEG-398). Now it says
 * "Saved" only when nothing in either is pending or failed.
 *
 * A summary that failed reads "Summary not saved" here, not `null`: its alert
 * and Retry are in the summary pane, below the note and often scrolled out of
 * view, while the note's own failure has its banner at the top of the page. So
 * a failed note still leaves this line empty, as `saveStatusText` explains.
 */
export function workspaceSaveText(note: AutosaveStatus, summary: AutosaveStatus): string | null {
  if (summary === "failed") return "Summary not saved";
  if (note === "failed") return null;
  const pending = (status: AutosaveStatus) => status === "dirty" || status === "saving";
  if (pending(note) || pending(summary)) return saveStatusText("saving");
  if (note === "saved" || summary === "saved") return saveStatusText("saved");
  return null;
}
