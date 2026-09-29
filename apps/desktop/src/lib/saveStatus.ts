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
