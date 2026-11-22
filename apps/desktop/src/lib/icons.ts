/**
 * The icon set: one 24-unit path per name, in two families that `Icon.tsx`
 * tells apart by name.
 *
 * `ICONS` are soft line: stroked, with round caps and joins (`.icon` in
 * `styles.css`) and corners drawn as arcs in the paths themselves, because a
 * round join alone leaves a box looking like a box. They are the things you
 * press and what a row or chip is about.
 *
 * `FILLED` are soft solids, for what *is* the thing rather than a control for
 * it: `stop`, `play` and `pause` mark a state. `folder` and `trash` were solid
 * on the same reasoning, as places in a list, until the owner asked for each
 * in line (2026-11-13 and 2026-11-20). `folder` is now `folderPlus`'s outline
 * without the plus. `trash` is a plain bin: a lid, its handle and a body. It
 * is not `discard`, which is the action and stands a little narrower; Trash is
 * where deleted notes are, and where a note can be dragged.
 *
 * The glyphs began as the Kit's (the design before this one) and took their
 * rounding with oem.care's language; `git log` on this file has that history.
 * A few were drawn for a need of their own: `pause` and `chevronUp` for the
 * recording player and the find bar (KNEG-448), `restore` as the title bar's
 * fourth window state, and `panelRight`, `panel` mirrored with its rule at
 * 14.75, for the note's transcript column. `expandAll` and `collapseAll` are
 * the Notes library's two folder buttons, a pair of chevrons opening and
 * closing.
 *
 * A plain `d` string rather than a component per icon, so `Icon.tsx` is the
 * only place a `<path>` is ever written and `tests/icons.test.ts` can check
 * every entry the same way without rendering anything.
 */
export type LineIconName =
  | "dictation"
  | "meetings"
  | "notes"
  | "settings"
  | "search"
  | "microphone"
  | "computer"
  | "language"
  | "others"
  | "speakers"
  | "tidy"
  | "shortcut"
  | "vocabulary"
  | "transcription"
  | "textModel"
  | "key"
  | "calendar"
  | "folder"
  | "folderPlus"
  | "import"
  | "newNote"
  | "copy"
  | "lock"
  | "globe"
  | "cli"
  | "api"
  | "mcp"
  | "diagnostics"
  | "discard"
  | "trash"
  | "close"
  | "plus"
  | "check"
  | "chevronDown"
  | "chevronRight"
  | "chevronUp"
  | "back"
  | "forward"
  | "panel"
  | "panelRight"
  | "more"
  | "minimise"
  | "maximise"
  | "restore"
  | "export"
  | "refresh"
  | "clock"
  | "call"
  | "expandAll"
  | "collapseAll";

export type FilledIconName = keyof typeof FILLED;

/** Any icon `Icon` can draw, whichever family it belongs to. */
export type IconName = LineIconName | FilledIconName;

export const ICONS: Record<LineIconName, string> = {
  dictation: "M3.75 10v4M7.5 7v10M11.25 4v16M15 8v8M18.75 10.5v3",
  meetings:
    "M12 8a3 3 0 1 1-6 0 3 3 0 0 1 6 0zM3.75 19.25v-1a3.5 3.5 0 0 1 3.5-3.5h3.5a3.5 3.5 0 0 1 3.5 3.5v1M15.25 5.25a3 3 0 0 1 0 5.5M17.75 14.9a3.5 3.5 0 0 1 2.5 3.35v1",
  notes:
    "M7.75 3.75h6.5l4 4v10.5a2 2 0 0 1-2 2h-8.5a2 2 0 0 1-2-2V5.75a2 2 0 0 1 2-2zM14.25 3.75v4h4M9 12.25h6M9 15.75h6",
  settings: "M4 7.25h9M17 7.25h3M15 5v4.5M4 16.75h3M11 16.75h9M9 14.5V19",
  search: "M16.5 10.5a6 6 0 1 1-12 0 6 6 0 0 1 12 0zM15 15l5.25 5.25",
  microphone:
    "M9.25 6.5a2.75 2.75 0 0 1 5.5 0v5a2.75 2.75 0 0 1-5.5 0zM6 11.25a6 6 0 0 0 12 0M12 17.25v3",
  computer:
    "M7.75 5.75h8.5a2 2 0 0 1 2 2v5a2 2 0 0 1-2 2h-8.5a2 2 0 0 1-2-2v-5a2 2 0 0 1 2-2zM3 18.25h18",
  language: "M3.75 5.75h8M7.75 5.75v12.5M12.75 18.25l3.5-9 3.5 9M14.05 15h4.4",
  others:
    "M4.75 10.25a1 1 0 0 1 1-1h2.5l4.5-4v13.5l-4.5-4h-2.5a1 1 0 0 1-1-1zM16 9a4 4 0 0 1 0 6M18.5 6.5a7.5 7.5 0 0 1 0 11",
  speakers:
    "M5.75 4.75h12.5a2 2 0 0 1 2 2v7.5a2 2 0 0 1-2 2h-6.5l-4 3.5v-3.5h-2a2 2 0 0 1-2-2v-7.5a2 2 0 0 1 2-2zM8.5 9h7M8.5 12.25h4.5",
  tidy: "M12 3.5l1.9 6.6 6.6 1.9-6.6 1.9-1.9 6.6-1.9-6.6-6.6-1.9 6.6-1.9z",
  shortcut:
    "M6.25 6.75h11.5a2.5 2.5 0 0 1 2.5 2.5v5.5a2.5 2.5 0 0 1-2.5 2.5h-11.5a2.5 2.5 0 0 1-2.5-2.5v-5.5a2.5 2.5 0 0 1 2.5-2.5zM7.5 10.25h.01M11 10.25h.01M14.5 10.25h.01M8 13.75h8",
  vocabulary: "M4 18.25l3.5-9 3.5 9M5.3 15h4.4M14 9.75h6M14 13.75h6M14 17.75h6",
  transcription: "M4 9v6M7.5 6.5v11M11 10v4M14.5 8.25h5.5M14.5 12h5.5M14.5 15.75h3.5",
  textModel:
    "M8.75 6.75h6.5a2 2 0 0 1 2 2v6.5a2 2 0 0 1-2 2h-6.5a2 2 0 0 1-2-2v-6.5a2 2 0 0 1 2-2zM10.75 10h2.5a0.75 0.75 0 0 1 0.75 0.75v2.5a0.75 0.75 0 0 1-0.75 0.75h-2.5a0.75 0.75 0 0 1-0.75-0.75v-2.5a0.75 0.75 0 0 1 0.75-0.75zM9.5 3.5v3.25M14.5 3.5v3.25M9.5 17.25v3.25M14.5 17.25v3.25M3.5 9.5h3.25M3.5 14.5h3.25M17.25 9.5h3.25M17.25 14.5h3.25",
  key: "M11.75 15.5a3.75 3.75 0 1 1-7.5 0 3.75 3.75 0 0 1 7.5 0zM10.75 12.75l8.5-8.5M16.25 7.25l2.5 2.5M13.75 9.75l2 2",
  calendar:
    "M7.25 5.75h9.5a2.5 2.5 0 0 1 2.5 2.5v8.5a2.5 2.5 0 0 1-2.5 2.5h-9.5a2.5 2.5 0 0 1-2.5-2.5v-8.5a2.5 2.5 0 0 1 2.5-2.5zM4.75 9.75h14.5M8.5 3.5v4M15.5 3.5v4",
  folder:
    "M3.75 7.75a2 2 0 0 1 2-2h3.4a2 2 0 0 1 1.5.7l1 1.1a2 2 0 0 0 1.5.7h5.1a2 2 0 0 1 2 2v6.5a2 2 0 0 1-2 2H5.75a2 2 0 0 1-2-2z",
  folderPlus:
    "M3.75 7.75a2 2 0 0 1 2-2h3.4a2 2 0 0 1 1.5.7l1 1.1a2 2 0 0 0 1.5.7h5.1a2 2 0 0 1 2 2v6.5a2 2 0 0 1-2 2H5.75a2 2 0 0 1-2-2zM12 10.75v5M9.5 13.25h5",
  import:
    "M12 15.25V4.75M7.5 9.25l4.5-4.5 4.5 4.5M4.75 14.75v2.5a2 2 0 0 0 2 2h10.5a2 2 0 0 0 2-2v-2.5",
  newNote:
    "M11.25 4.75h-4.5a2 2 0 0 0-2 2v10.5a2 2 0 0 0 2 2h10.5a2 2 0 0 0 2-2v-4.5M16.75 3.75l3.5 3.5-8 8h-3.5v-3.5z",
  copy: "M10.75 8.75h6.5a2 2 0 0 1 2 2v6.5a2 2 0 0 1-2 2h-6.5a2 2 0 0 1-2-2v-6.5a2 2 0 0 1 2-2zM15.25 8.75v-2a2 2 0 0 0-2-2H6.75a2 2 0 0 0-2 2v6.5a2 2 0 0 0 2 2h2",
  globe: "M20.25 12a8.25 8.25 0 1 1-16.5 0 8.25 8.25 0 0 1 16.5 0zM3.75 12h16.5M12 3.75a4 8.25 0 0 1 0 16.5 4 8.25 0 0 1 0-16.5z",
  lock: "M7.75 10.75h8.5a2 2 0 0 1 2 2v5.5a2 2 0 0 1-2 2h-8.5a2 2 0 0 1-2-2v-5.5a2 2 0 0 1 2-2zM8.75 10.75V7.5a3.25 3.25 0 0 1 6.5 0v3.25",
  cli: "M6.25 4.75h11.5a2.5 2.5 0 0 1 2.5 2.5v9.5a2.5 2.5 0 0 1-2.5 2.5h-11.5a2.5 2.5 0 0 1-2.5-2.5v-9.5a2.5 2.5 0 0 1 2.5-2.5zM7.5 9.25l2.75 2.75-2.75 2.75M12.5 14.75h4",
  api: "M8.75 3.75v4.5M15.25 3.75v4.5M6.25 8.25h11.5v3.5a5.75 5.75 0 0 1-11.5 0zM12 17.5v2.75",
  mcp: "M6 4.75h2.5a1.25 1.25 0 0 1 1.25 1.25v2.5a1.25 1.25 0 0 1-1.25 1.25h-2.5a1.25 1.25 0 0 1-1.25-1.25v-2.5a1.25 1.25 0 0 1 1.25-1.25zM15.5 14.25h2.5a1.25 1.25 0 0 1 1.25 1.25v2.5a1.25 1.25 0 0 1-1.25 1.25h-2.5a1.25 1.25 0 0 1-1.25-1.25v-2.5a1.25 1.25 0 0 1 1.25-1.25zM9.75 7.25h2.5a2 2 0 0 1 2 2v5",
  diagnostics: "M3.5 12h4l2.25-6 4.5 12 2.25-6h4",
  discard:
    "M4.75 6.75h14.5M9.25 6.75V5.25a1 1 0 0 1 1-1h3.5a1 1 0 0 1 1 1v1.5M6.75 6.75l.9 11.1a1.5 1.5 0 0 0 1.5 1.4h5.7a1.5 1.5 0 0 0 1.5-1.4l.9-11.1",
  trash:
    "M4.5 7h15M9.5 7V5.5a1.25 1.25 0 0 1 1.25-1.25h2.5a1.25 1.25 0 0 1 1.25 1.25V7M6.25 7l.85 10.9a2 2 0 0 0 2 1.85h5.8a2 2 0 0 0 2-1.85L17.75 7",
  close: "M6 6l12 12M18 6L6 18",
  plus: "M12 5v14M5 12h14",
  check: "M5 12.5l4.5 4.5L19 7.5",
  chevronDown: "M7 10l5 5 5-5",
  chevronRight: "M10 7l5 5-5 5",
  chevronUp: "M7 14l5-5 5 5",
  back: "M19.5 12h-15M10.5 6l-6 6 6 6",
  forward: "M4.5 12h15M13.5 6l6 6-6 6",
  panel:
    "M6.25 4.75h11.5a2.5 2.5 0 0 1 2.5 2.5v9.5a2.5 2.5 0 0 1-2.5 2.5h-11.5a2.5 2.5 0 0 1-2.5-2.5v-9.5a2.5 2.5 0 0 1 2.5-2.5zM9.25 4.75v14.5",
  panelRight:
    "M6.25 4.75h11.5a2.5 2.5 0 0 1 2.5 2.5v9.5a2.5 2.5 0 0 1-2.5 2.5h-11.5a2.5 2.5 0 0 1-2.5-2.5v-9.5a2.5 2.5 0 0 1 2.5-2.5zM14.75 4.75v14.5",
  more: "M5.75 12h.01M12 12h.01M18.25 12h.01",
  minimise: "M5 12h14",
  maximise: "M7.75 5.75h8.5a2 2 0 0 1 2 2v8.5a2 2 0 0 1-2 2h-8.5a2 2 0 0 1-2-2v-8.5a2 2 0 0 1 2-2z",
  restore:
    "M10 8.25h7a1.75 1.75 0 0 1 1.75 1.75v7a1.75 1.75 0 0 1-1.75 1.75h-7a1.75 1.75 0 0 1-1.75-1.75v-7a1.75 1.75 0 0 1 1.75-1.75zM5.25 15.75v-9a1.5 1.5 0 0 1 1.5-1.5h9",
  export:
    "M12 3.75v10.5M7.75 8l4.25-4.25L16.25 8M5.75 12.75v4.5a2 2 0 0 0 2 2h8.5a2 2 0 0 0 2-2v-4.5",
  refresh: "M19.25 12a7.25 7.25 0 1 1-2.1-5.1M19.25 4.75v4.5h-4.5",
  clock: "M20.25 12a8.25 8.25 0 1 1-16.5 0 8.25 8.25 0 0 1 16.5 0zM12 7.5V12l3 2",
  call: "M6 6.75h7a2.25 2.25 0 0 1 2.25 2.25v6a2.25 2.25 0 0 1-2.25 2.25h-7a2.25 2.25 0 0 1-2.25-2.25v-6a2.25 2.25 0 0 1 2.25-2.25zM15.25 10.5l5-3v9l-5-3",
  expandAll: "M8 9.5l4-4 4 4M8 14.5l4 4 4-4",
  collapseAll: "M8 5l4 4 4-4M8 19l4-4 4 4",
};

/**
 * Filled marks: `fill: currentColor; stroke: none` (`.icon-filled` in
 * `styles.css`), for what is a solid shape rather than a stroked control -- a
 * playing transport is a solid triangle and the pause that replaces it two
 * solid bars.
 */
export const FILLED = {
  stop: "M8.5 5h7a3.5 3.5 0 0 1 3.5 3.5v7a3.5 3.5 0 0 1-3.5 3.5h-7a3.5 3.5 0 0 1-3.5-3.5v-7a3.5 3.5 0 0 1 3.5-3.5z",
  play: "M9.01 5.83L17.3 10.71A1.5 1.5 0 0 1 17.3 13.29L9.01 18.17A1.5 1.5 0 0 1 6.75 16.88L6.75 7.12A1.5 1.5 0 0 1 9.01 5.83z",
  pause:
    "M8.75 5a1.75 1.75 0 0 1 1.75 1.75v10.5a1.75 1.75 0 0 1-3.5 0V6.75A1.75 1.75 0 0 1 8.75 5zM15.25 5A1.75 1.75 0 0 1 17 6.75v10.5a1.75 1.75 0 0 1-3.5 0V6.75A1.75 1.75 0 0 1 15.25 5z",
} as const;

/**
 * Per-icon stroke-width overrides, whatever the size.
 *
 * `more` is three dots drawn as near-zero-length strokes (`M5.75 12h.01`, and
 * so on), so the stroke is the dot's whole diameter: at the set's usual 1.5
 * they read as specks.
 */
export const ICON_STROKE: Partial<Record<LineIconName, number>> = {
  more: 3,
};

/** From this size up, `.icon`'s stroke scales with the drawing. */
const SCALES_FROM_PX = 18;
/** The weight on screen, in CSS pixels, that a smaller icon keeps. */
const SMALL_ICON_INK_PX = 1.15;

/**
 * The stroke-width `Icon` sets on a line icon, in the path's 24 units, or
 * `undefined` to leave it to the stylesheet.
 *
 * `.icon`'s 1.5 scales with the drawing, which is right from 18px up but would
 * leave a 12px chevron at 0.75px beside its 16px neighbours at 1px, reading as
 * a fainter set. Below 18px the width is worked out to hold the same weight on
 * screen. Returning `undefined` at larger sizes keeps a stylesheet override
 * working, such as the hero's thinner stroke, which an inline width would beat.
 */
export function iconStrokeWidth(name: LineIconName, size: number): number | undefined {
  const fixed = ICON_STROKE[name];
  if (fixed !== undefined) return fixed;
  if (size >= SCALES_FROM_PX) return undefined;
  return Math.round(((SMALL_ICON_INK_PX * 24) / size) * 100) / 100;
}
