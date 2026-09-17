import type { EditorView } from "@milkdown/kit/prose/view";
import { TextSelection } from "@milkdown/kit/prose/state";

/** Make the visible left gutter part of the editable element's native hit area. */
export function installEditorGutter(view: EditorView, surface: HTMLElement): () => void {
  const update = () => {
    if (view.isDestroyed) return;
    const rect = view.dom.getBoundingClientRect();
    const padding = Number.parseFloat(getComputedStyle(view.dom).paddingLeft) || 0;
    // Padding and negative margin cancel, preserving the text's position and
    // wrapping while extending the editable box to the writing surface edge.
    const gutter = Math.max(0, rect.left + padding - surface.getBoundingClientRect().left - 1);
    view.dom.style.setProperty("--note-editor-gutter", `${gutter}px`);
  };
  const observer = new ResizeObserver(update);
  const placeCursor = (event: MouseEvent) => {
    if (event.button !== 0 || view.isDestroyed || !view.editable) return;
    if (!(event.target instanceof Element) || event.target.closest("button, input, a, [contenteditable=false]")) return;
    // Browser caret placement in a list's outer gutter can select the whole
    // list boundary. Resolve the text row at this height explicitly instead.
    let row: { start: number; rect: DOMRect; distance: number } | undefined;
    view.state.doc.descendants((node, pos) => {
      if (!node.isTextblock) return;
      const dom = view.nodeDOM(pos);
      if (!(dom instanceof HTMLElement)) return false;
      const rect = dom.getBoundingClientRect();
      const distance = Math.max(rect.top - event.clientY, event.clientY - rect.bottom, 0);
      if (!row || distance < row.distance) row = { start: pos + 1, rect, distance };
      return false;
    });
    if (!row || event.clientX >= row.rect.left) return;
    event.preventDefault();
    const selection = event.shiftKey
      ? TextSelection.create(view.state.doc, view.state.selection.anchor, row.start)
      : TextSelection.create(view.state.doc, row.start);
    view.dispatch(view.state.tr.setSelection(selection));
    view.focus();
  };
  view.dom.addEventListener("mousedown", placeCursor, true);
  observer.observe(surface);
  if (view.dom.parentElement) observer.observe(view.dom.parentElement);
  update();
  return () => {
    observer.disconnect();
    view.dom.removeEventListener("mousedown", placeCursor, true);
    view.dom.style.removeProperty("--note-editor-gutter");
  };
}
