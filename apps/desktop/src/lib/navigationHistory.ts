import { DEFAULT_VIEW, type ViewId } from "./navigation.js";

/**
 * A page, a note on Notes, and the transcript row a search hit opens it at
 * (KNEG-406). The row is kept with the entry, so Back and Forward show it
 * again.
 */
export type Destination = { view: ViewId; noteId?: number; transcriptRow?: number; summaryFocus?: boolean };
export type NavigationHistory = { entries: Destination[]; index: number };
export type NavigationAction = { type: "visit"; destination: Destination } | { type: "back" | "forward" };
export const defaultDestination: Destination = { view: DEFAULT_VIEW };
export const initialHistory: NavigationHistory = { entries: [defaultDestination], index: 0 };

export function navigationHistory(state: NavigationHistory, action: NavigationAction): NavigationHistory {
  if (action.type !== "visit") {
    const index = state.index + (action.type === "back" ? -1 : 1);
    return index < 0 || index >= state.entries.length ? state : { ...state, index };
  }
  const current = state.entries[state.index] ?? defaultDestination;
  const next = action.destination;
  // A transcript row in the note already open -- a search hit in its
  // transcript (KNEG-406) -- takes the current entry's place as a new object:
  // no Back step to the same note without the row, and choosing the same hit
  // again is still a new request for the page to show it.
  if (next.transcriptRow !== undefined && current.view === next.view && current.noteId === next.noteId) {
    return { ...state, entries: state.entries.map((entry, index) => index === state.index ? { ...next } : entry) };
  }
  if (current.view === next.view && (current.noteId === next.noteId || next.noteId === undefined)) return state;
  // The first note displayed fills the existing Notes destination.
  if (current.view === "notes" && next.view === "notes" && current.noteId === undefined) {
    return { ...state, entries: state.entries.map((entry, index) => index === state.index ? next : entry) };
  }
  const entries = [...state.entries.slice(0, state.index + 1), next].slice(-100);
  return { entries, index: entries.length - 1 };
}
