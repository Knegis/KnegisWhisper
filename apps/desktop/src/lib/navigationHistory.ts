import { DEFAULT_VIEW, type ViewId } from "./navigation.js";

export type Destination = { view: ViewId; noteId?: number };
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
  if (current.view === next.view && (current.noteId === next.noteId || next.noteId === undefined)) return state;
  // The first note displayed fills the existing Notes destination.
  if (current.view === "notes" && next.view === "notes" && current.noteId === undefined) {
    return { ...state, entries: state.entries.map((entry, index) => index === state.index ? next : entry) };
  }
  const entries = [...state.entries.slice(0, state.index + 1), next].slice(-100);
  return { entries, index: entries.length - 1 };
}
