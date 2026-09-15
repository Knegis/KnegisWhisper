/**
 * Refuse files dropped onto the window, without touching drags inside the page.
 *
 * The main window sets `dragDropEnabled: false` so that the note editor's block
 * handle can drag at all (KNEG-258). While Tauri services drag and drop, wry
 * calls `SetAllowExternalDrop(false)` and then `RevokeDragDrop` on WebView2's
 * child windows and registers a file-drop-only `IDropTarget` in place of the
 * one Chromium uses, so an in-page drop never arrives: `dragstart` fires,
 * `drop` does not, and the block springs back. Measured, before and after, in
 * `docs/ENGINEERING_LOG.md`, 2026-09-15.
 *
 * Turning it off gives external file drops back to WebView2, whose default is
 * to navigate to the dropped file — replacing the running application with a
 * picture of a cat until it is restarted. Tauri had been swallowing those by
 * accident; this refuses them on purpose, which keeps the behaviour the
 * application already had.
 *
 * Nothing is lost by refusing them: Crepe's `imageBlock` has no drop path and
 * no Crepe feature this build mounts pulls `@milkdown/plugin-upload`, so the
 * editor could not have accepted a dropped file anyway. Accepting one is a
 * separate decision, and would start here.
 */

/**
 * Whether a drag carries files rather than page content.
 *
 * `DataTransfer.types` is the only thing readable during `dragover` — the items
 * themselves are protected until the drop — and a file drag always lists
 * "Files" there. A drag started inside the page carries "text/html" and
 * "text/plain" (`@milkdown/plugin-block` sets exactly those), so this is what
 * separates the drag to refuse from the drag the editor needs.
 */
export function carriesFiles(types: readonly string[] | undefined): boolean {
  return types?.includes("Files") === true;
}

/** The subset of the window that the guard uses, so a test can stand in for it. */
export type DropGuardTarget = {
  addEventListener: (type: string, listener: (event: DragEvent) => void) => void;
};

/**
 * Install the guard on `target`.
 *
 * Both events, deliberately: `dragover` decides whether a drop is allowed at
 * all and `drop` decides whether the browser performs its default with it.
 * Preventing one and not the other still navigates.
 *
 * The listeners are on the bubble phase, so anything inside the page — the
 * editor's own handle, ProseMirror's drop handling — has already seen the event
 * and can do what it likes with it first.
 */
export function installFileDropGuard(target: DropGuardTarget): void {
  const refuse = (event: DragEvent) => {
    if (!carriesFiles(event.dataTransfer?.types)) return;
    event.preventDefault();
  };
  target.addEventListener("dragover", refuse);
  target.addEventListener("drop", refuse);
}
