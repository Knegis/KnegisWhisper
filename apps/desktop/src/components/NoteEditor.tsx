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
  initialTitle: string;
  onChange: (markdown: string, title: string) => void;
  summary?: boolean;
};

export function NoteEditor({
  noteId,
  initialContent,
  initialTitle,
  onChange,
  summary = false,
}: NoteEditorProps) {
  const rootRef = useRef<HTMLDivElement>(null);
  const initialRef = useRef(initialContent);
  const onChangeRef = useRef(onChange);
  const generationRef = useRef(0);

  onChangeRef.current = onChange;

  useEffect(() => {
    const root = rootRef.current;
    if (root === null) return;

    initialRef.current = initialContent;
    const generation = generationRef.current + 1;
    generationRef.current = generation;
    let superseded = false;
    let initializing = true;
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
            // Keep Crepe's left/left-start alignment and 16px row spacing.
            middleware: [{ name: "rowSpacing", fn: ({ x }) => ({ x: x - 16 }) }],
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
        api.markdownUpdated((ctx, markdown, previous) => {
          if (initializing || superseded || markdown === previous) return;
          const first = ctx.get(editorViewCtx).state.doc.firstChild;
          const title = first?.type.name === "heading" && first.attrs.level === 1
            ? first.textContent.trim() || "Untitled note" : "Untitled note";
          onChangeRef.current(markdown, title);
        });
      });
    const created = builder.create().then(() => {
      if (superseded) return;
      builder.editor.action(ctx => {
        const view = ctx.get(editorViewCtx);
        stopMarginDrop = installEditorMarginDrop(view, root.closest<HTMLElement>(".notes-main") ?? root);
        stopGutter = installEditorGutter(view, root.closest<HTMLElement>(".notes-main") ?? root);
        const first = view.state.doc.firstChild;
        if (!summary && (first?.type.name !== "heading" || first.attrs.level !== 1)) {
          const headingType = view.state.schema.nodes.heading;
          if (!headingType) throw new Error("The note editor requires heading support.");
          const heading = headingType.create({ level: 1 }, view.state.schema.text(initialTitle.trim() || "Untitled note"));
          view.dispatch(view.state.tr.insert(0, heading));
        }
        initializing = false;
        const title = view.state.doc.firstChild?.textContent.trim() || "Untitled note";
        const markdown = ctx.get(serializerCtx)(view.state.doc);
        if (!summary && (markdown !== initialContent || title !== initialTitle)) onChangeRef.current(markdown, title);
      });
    });

    return () => {
      superseded = true;
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
