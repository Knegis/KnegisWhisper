import type { EditorView } from "@milkdown/kit/prose/view";

/** Extend internal block drops to the writing surface's surrounding margins. */
export function installEditorMarginDrop(view: EditorView, surface: HTMLElement): () => void {
  const forward = (event: DragEvent) => {
    if (!view.dragging || !event.dataTransfer || event.dataTransfer.types.includes("Files")) return;
    if (!(event.target instanceof Node) || view.dom.contains(event.target)) return;
    const surfaceRect = surface.getBoundingClientRect();
    if (event.clientX < surfaceRect.left || event.clientX >= surfaceRect.right
      || event.clientY < surfaceRect.top || event.clientY >= surfaceRect.bottom) return;
    // Handles can float over the margin. Inspect the writing surface by
    // coordinates rather than requiring the native target to be its child.
    if (event.target instanceof Element && !event.target.closest(".milkdown-block-handle")
      && event.target.closest(".transcript, button, input, select, a")) return;
    const rect = view.dom.getBoundingClientRect();
    if (rect.width < 2 || rect.height < 2) return;
    // Accept the actual browser drag before forwarding it. Cancelling only a
    // synthetic event does not establish a native drop target in the gutter.
    event.preventDefault();
    event.stopPropagation();
    const forwarded = new DragEvent(event.type, {
      bubbles: true, cancelable: true, dataTransfer: event.dataTransfer,
      clientX: rect.left + rect.width / 2,
      clientY: Math.max(rect.top + 1, Math.min(event.clientY, rect.bottom - 1)),
      ctrlKey: event.ctrlKey, altKey: event.altKey, metaKey: event.metaKey, shiftKey: event.shiftKey,
    });
    view.dom.dispatchEvent(forwarded);
  };
  let cleanupTimer: ReturnType<typeof setTimeout> | undefined;
  const finish = () => {
    const dragging = view.dragging;
    clearTimeout(cleanupTimer);
    // Some browsers send dragend before drop. Preserve the active slice until
    // the drop handler has had an opportunity to move it.
    cleanupTimer = setTimeout(() => {
      if (!view.isDestroyed && view.dragging === dragging) {
        view.dragging = null;
        view.dom.dataset.dragging = "false";
      }
    }, 50);
  };
  document.addEventListener("dragover", forward, true);
  document.addEventListener("drop", forward, true);
  document.addEventListener("dragend", finish, true);
  return () => {
    clearTimeout(cleanupTimer);
    document.removeEventListener("dragover", forward, true);
    document.removeEventListener("drop", forward, true);
    document.removeEventListener("dragend", finish, true);
  };
}
