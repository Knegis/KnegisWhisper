import { editorViewCtx, serializerCtx } from "@milkdown/kit/core";
import { CrepeBuilder } from "@milkdown/crepe/builder";
import { blockEdit } from "@milkdown/crepe/feature/block-edit";
import { cursor } from "@milkdown/crepe/feature/cursor";
import { imageBlock } from "@milkdown/crepe/feature/image-block";
import { linkTooltip } from "@milkdown/crepe/feature/link-tooltip";
import { listItem } from "@milkdown/crepe/feature/list-item";
import { placeholder } from "@milkdown/crepe/feature/placeholder";
import { table } from "@milkdown/crepe/feature/table";
import { toolbar } from "@milkdown/crepe/feature/toolbar";
import { useEffect, useRef } from "react";
import { installEditorMarginDrop } from "../lib/editorMarginDrop";
import { installEditorGutter } from "../lib/editorGutter";

import "@milkdown/crepe/theme/common/reset.css";
import "@milkdown/crepe/theme/common/prosemirror.css";
import "@milkdown/crepe/theme/common/block-edit.css";
import "@milkdown/crepe/theme/common/cursor.css";
import "@milkdown/crepe/theme/common/image-block.css";
import "@milkdown/crepe/theme/common/link-tooltip.css";
import "@milkdown/crepe/theme/common/list-item.css";
import "@milkdown/crepe/theme/common/placeholder.css";
import "@milkdown/crepe/theme/common/toolbar.css";
import "@milkdown/crepe/theme/common/table.css";
import "@milkdown/crepe/theme/frame.css";

type NoteEditorProps = {
  noteId: number;
  initialContent: string;
  onChange: (markdown: string) => void;
  summary?: boolean;
  readOnly?: boolean;
  legacyTitle?: string;
  onReady?: (markdown: string) => void;
};

export function NoteEditor({
  noteId,
  initialContent,
  onChange,
  summary = false,
  readOnly = false,
  legacyTitle,
  onReady,
}: NoteEditorProps) {
  const builderRef = useRef<CrepeBuilder | null>(null);
  const readOnlyRef = useRef(readOnly);
  readOnlyRef.current = readOnly;
  useEffect(() => { builderRef.current?.setReadonly(readOnly); }, [readOnly]);
  const rootRef = useRef<HTMLDivElement>(null);
  const initialRef = useRef(initialContent);
  const onChangeRef = useRef(onChange);
  const generationRef = useRef(0);
  const onReadyRef = useRef(onReady);
  onReadyRef.current = onReady;

  onChangeRef.current = onChange;

  useEffect(() => {
    const root = rootRef.current;
    if (root === null) return;

    initialRef.current = initialContent;
    const generation = generationRef.current + 1;
    generationRef.current = generation;
    let superseded = false;
    let initializing = true;
    let lastMarkdown = initialRef.current;
    let stopMarginDrop: (() => void) | undefined;
    let stopGutter: (() => void) | undefined;
    const builder = new CrepeBuilder({ root, defaultValue: initialRef.current })
      // The full Crepe class would also add CodeMirror and KaTeX. Those create
      // runtime styles and font assets that production's self-only CSP blocks.
      .addFeature(blockEdit, {
        blockHandle: {
          floatingUIOptions: {
            // Replace the default flip middleware: the Notes rail must not
            // move row controls to the opposite side of the document.
            // Keep Crepe's left/left-start alignment, 2px off the text: the note's
            // sheet has a 44px margin, which the scroller would clip anything past.
            middleware: [{ name: "rowSpacing", fn: ({ x }) => ({ x: x - 2 }) }],
          },
        },
      })
      .addFeature(toolbar)
      .addFeature(placeholder, { text: summary ? "Edit the meeting summary." : "Write the note as the conversation develops." })
      .addFeature(cursor)
      .addFeature(listItem)
      .addFeature(linkTooltip)
      .addFeature(table)
      .addFeature(imageBlock)
      .on((api) => {
        api.markdownUpdated((_ctx, markdown, previous) => {
          if (initializing || superseded || markdown === previous || markdown === lastMarkdown) return;
          lastMarkdown = markdown;
          onChangeRef.current(markdown);
        });
      });
    builderRef.current = builder;
    const created = builder.create().then(() => {
      if (superseded) return;
      builder.setReadonly(readOnlyRef.current);
      builder.editor.action(ctx => {
        const view = ctx.get(editorViewCtx);
        stopMarginDrop = installEditorMarginDrop(view, root.closest<HTMLElement>(".notes-main") ?? root);
        stopGutter = installEditorGutter(view, root.closest<HTMLElement>(".notes-main") ?? root);
        const first = view.state.doc.firstChild;
        if (legacyTitle !== undefined && first?.type.name === "heading" && first.attrs.level === 1 && first.textContent.trim() === legacyTitle.trim()) {
          view.dispatch(view.state.tr.delete(0, first.nodeSize).setMeta("addToHistory", false));
        }
        lastMarkdown = ctx.get(serializerCtx)(view.state.doc);
        onReadyRef.current?.(lastMarkdown);
        initializing = false;
      });
    });

    return () => {
      superseded = true;
      if (builderRef.current === builder) builderRef.current = null;
      stopMarginDrop?.();
      stopGutter?.();
      void created
        .catch(() => undefined)
        .then(async () => {
          await builder.destroy().catch(() => undefined);
          // StrictMode may have started the next editor while this one settled.
          // Only this effect may clear its root; the stale instance is destroyed.
          if (superseded && generationRef.current === generation) {
            root.replaceChildren();
          }
        });
    };
  }, [noteId, summary]);

  return (
    <div className="note-editor">
      <div ref={rootRef} />
    </div>
  );
}
