/**
 * The notes library's drag payload: what a dragged note carries, and how a
 * drop reads it back.
 *
 * Pure so the parsing can be tested without a browser. The payload is the
 * note's id under a private type, which is what lets a drop target tell a
 * note apart from a file being dragged in from the desktop; the window's
 * file-drop guard looks for the `Files` type and ignores this one.
 */
export const NOTE_DRAG_TYPE = "application/x-knegiswhisper-note";

/** Whether a drag's declared types include a note. */
export function carriesNote(types: Iterable<string> | undefined): boolean {
  if (!types) return false;
  for (const type of types) if (type === NOTE_DRAG_TYPE) return true;
  return false;
}

/** The dragged note's id, or null when the payload is not a note. */
export function draggedNoteId(data: { getData(type: string): string }): number | null {
  const raw = data.getData(NOTE_DRAG_TYPE);
  return /^\d+$/.test(raw) ? Number(raw) : null;
}

/** Whether filing a note under `target` changes anything. */
export function folderChange(current: number | null, target: number | null): boolean {
  return current !== target;
}
