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
};

export function NoteEditor({
  noteId,
  initialContent,
  onChange,
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
    const builder = new CrepeBuilder({ root, defaultValue: initialRef.current })
      // The full Crepe class would also add CodeMirror and KaTeX. Those create
      // runtime styles and font assets that production's self-only CSP blocks.
      .addFeature(blockEdit)
      .addFeature(toolbar)
      .addFeature(placeholder, { text: "Write the note as the conversation develops." })
      .addFeature(cursor)
      .addFeature(listItem)
      .addFeature(linkTooltip)
      .addFeature(table)
      .addFeature(imageBlock)
      .on((api) => {
        api.markdownUpdated((_ctx, markdown, previous) => {
          if (markdown !== previous) onChangeRef.current(markdown);
        });
      });
    const created = builder.create();

    return () => {
      superseded = true;
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
  }, [noteId]);

  return (
    <div className="note-editor">
      <div ref={rootRef} />
    </div>
  );
}
