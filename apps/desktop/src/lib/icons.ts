/**
 * The icon set, drawn for the Kit -- the design before the current one, which
 * kept it unchanged: one 24-unit stroked path per name, plus the three filled
 * marks (`FILLED`) that read as fills rather than outlines because they *are*
 * the thing rather than a control for it -- `stop`, `play` and `pause` mark a
 * state, not an action to take on one.
 *
 * Every `d` here is copied verbatim from the approved canvas (`Kit.dc.html`,
 * `Settings.dc.html`, and the Meetings and Notes artboards for the handful
 * that live there instead) -- see
 * `.superpowers/sdd/nifty-tumbling-giraffe/icons-from-canvas.md` for the
 * source table. Six are not on the canvas. `pause` (two solid bars, in
 * `stop`'s and `play`'s 24-unit frame) and `chevronUp` (`chevronDown`
 * mirrored vertically) were drawn for the recording player's pause state and
 * the find bar's collapse control (KNEG-448), not copied from the Kit canvas.
 * `restore` is drawn for the title
 * bar, which needs a fourth window-state icon, in the same stroke and corner
 * style as the other three. `panelRight` is `panel` mirrored, its rule at
 * 14.75 rather than 9.25, for the note's transcript column toggle, which
 * hides a column on the right as `panel` hides the list on the left.
 * `expandAll` and `collapseAll` are the Notes
 * library's two folder buttons, carried over path for path from the inline
 * SVGs the library drew before it moved into the list panel; here they take
 * the set's square caps and miter joins like every other entry.
 *
 * A plain `d` string rather than a component per icon, so `Icon.tsx` is the
 * only place a `<path>` is ever written and `tests/icons.test.ts` can check
 * every entry the same way without rendering anything.
 */
export type IconName =
  | "dictation"
  | "meetings"
  | "notes"
  | "settings"
  | "search"
  | "microphone"
  | "computer"
  | "language"
  | "others"
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
  | "cli"
  | "api"
  | "mcp"
  | "diagnostics"
  | "discard"
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

export const ICONS: Record<IconName, string> = {
  dictation: "M3.75 10v4M7.5 7v10M11.25 4v16M15 8v8M18.75 10.5v3",
  meetings:
    "M12 8a3 3 0 1 1-6 0 3 3 0 0 1 6 0zM3.75 19.25v-2.5L6 14.5h6l2.25 2.25v2.5M15.25 5.25a3 3 0 0 1 0 5.5M17.5 14.25l2.75 2.5v2.5",
  notes: "M5.75 3.75h8.5l4 4v12.5H5.75zM14.25 3.75v4h4M9 12.25h6M9 15.75h6",
  settings: "M4 7.25h9M17 7.25h3M15 5v4.5M4 16.75h3M11 16.75h9M9 14.5V19",
  search: "M16.5 10.5a6 6 0 1 1-12 0 6 6 0 0 1 12 0zM15 15l5.25 5.25",
  microphone:
    "M9.25 6.5a2.75 2.75 0 0 1 5.5 0v5a2.75 2.75 0 0 1-5.5 0zM6 11.25a6 6 0 0 0 12 0M12 17.25v3",
  computer: "M5.75 5.75h12.5v9H5.75zM3 18.25h18",
  language: "M3.75 5.75h8M7.75 5.75v12.5M12.75 18.25l3.5-9 3.5 9M14.05 15h4.4",
  others: "M4.75 9.25h3.5l4.5-4v13.5l-4.5-4h-3.5zM16 9a4 4 0 0 1 0 6M18.5 6.5a7.5 7.5 0 0 1 0 11",
  tidy: "M12 3.5l1.9 6.6 6.6 1.9-6.6 1.9-1.9 6.6-1.9-6.6-6.6-1.9 6.6-1.9z",
  shortcut: "M3.75 6.75h16.5v10.5H3.75zM7.25 10.25h.5M11 10.25h.5M14.75 10.25h.5M8 13.75h8",
  vocabulary: "M4 18.25l3.5-9 3.5 9M5.3 15h4.4M14 9.75h6M14 13.75h6M14 17.75h6",
  transcription: "M4 9v6M7.5 6.5v11M11 10v4M14.5 8.25h5.5M14.5 12h5.5M14.5 15.75h3.5",
  textModel:
    "M6.75 6.75h10.5v10.5H6.75zM10 10h4v4h-4zM9.5 3.5v3.25M14.5 3.5v3.25M9.5 17.25v3.25M14.5 17.25v3.25M3.5 9.5h3.25M3.5 14.5h3.25M17.25 9.5h3.25M17.25 14.5h3.25",
  key: "M11.75 15.5a3.75 3.75 0 1 1-7.5 0 3.75 3.75 0 0 1 7.5 0zM10.75 12.75l8.5-8.5M16.25 7.25l2.5 2.5M13.75 9.75l2 2",
  calendar: "M4.75 5.75h14.5v13.5H4.75zM4.75 9.75h14.5M8.5 3.5v4M15.5 3.5v4",
  folder: "M3.75 5.75h6l2 2.5h8.5v10H3.75z",
  folderPlus: "M3.75 5.75h6l2 2.5h8.5v10H3.75zM12 11v5M9.5 13.5h5",
  import: "M12 15.25V4.75M7.5 9.25l4.5-4.5 4.5 4.5M4.75 14.75v4.5h14.5v-4.5",
  newNote: "M11.25 4.75h-6.5v14.5h14.5v-6.5M16.75 3.75l3.5 3.5-8 8h-3.5v-3.5z",
  copy: "M8.75 8.75h10.5v10.5H8.75zM15.25 8.75v-4H4.75v10.5h4",
  lock: "M5.75 10.75h12.5v9.5H5.75zM8.75 10.75V7.5a3.25 3.25 0 0 1 6.5 0v3.25",
  cli: "M3.75 4.75h16.5v14.5H3.75zM7.5 9.25l2.75 2.75-2.75 2.75M12.5 14.75h4",
  api: "M8.75 3.75v4.5M15.25 3.75v4.5M6.25 8.25h11.5v3.5a5.75 5.75 0 0 1-11.5 0zM12 17.5v2.75",
  mcp: "M4.75 4.75h5v5h-5zM14.25 14.25h5v5h-5zM9.75 7.25h4.5v7",
  diagnostics: "M3.5 12h4l2.25-6 4.5 12 2.25-6h4",
  discard: "M4.75 6.75h14.5M9.25 6.75V4.25h5.5v2.5M6.75 6.75l1 12.5h8.5l1-12.5",
  close: "M6 6l12 12M18 6L6 18",
  plus: "M12 5v14M5 12h14",
  check: "M5 12.5l4.5 4.5L19 7.5",
  chevronDown: "M7 10l5 5 5-5",
  chevronRight: "M10 7l5 5-5 5",
  chevronUp: "M7 14l5-5 5 5",
  back: "M19.5 12h-15M10.5 6l-6 6 6 6",
  forward: "M4.5 12h15M13.5 6l6 6-6 6",
  panel: "M3.75 4.75h16.5v14.5H3.75zM9.25 4.75v14.5",
  panelRight: "M3.75 4.75h16.5v14.5H3.75zM14.75 4.75v14.5",
  more: "M5.5 12h.5M11.75 12h.5M18 12h.5",
  minimise: "M5 12h14",
  maximise: "M5.75 5.75h12.5v12.5H5.75z",
  restore: "M8.25 8.25h10.5v10.5H8.25zM5.25 15.75V5.25h10.5",
  export: "M12 3.75v10.5M7.75 8l4.25-4.25L16.25 8M5.75 12.75v6.5h12.5v-6.5",
  refresh: "M19.25 12a7.25 7.25 0 1 1-2.1-5.1M19.25 4.75v4.5h-4.5",
  clock: "M20.25 12a8.25 8.25 0 1 1-16.5 0 8.25 8.25 0 0 1 16.5 0zM12 7.5V12l3 2",
  call: "M3.75 6.75h11.5v10.5H3.75zM15.25 10.5l5-3v9l-5-3",
  expandAll: "M18 2L6 2M18 22L6 22M12 14V19M12 19L15 16M12 19L9 16M12 10V5M12 5L15 8M12 5L9 8",
  collapseAll: "M18 2L6 2M18 22L6 22M12 5V10M12 10L15 7M12 10L9 7M12 19V14M12 14L15 17M12 14L9 17",
};

/**
 * Filled marks: `fill: currentColor; stroke: none` (`.icon-filled` in
 * `styles.css`), for the states that are a fill rather than a stroked
 * control -- a playing transport is a solid triangle, not an outlined one,
 * and the pause that replaces it is two solid bars.
 */
export const FILLED = {
  stop: "M5 5h14v14H5z",
  play: "M7.5 5.25v13.5L18.5 12z",
  pause: "M7 5h3.5v14H7zM13.5 5H17v14h-3.5z",
} as const;

/**
 * Per-icon stroke-width overrides, applied by `Icon` as a `style` prop.
 *
 * `more` is three dots drawn as zero-length strokes (`M5.5 12h.5`, and so on)
 * -- at the set's usual 1.5 they read as barely-there commas rather than
 * dots, so the canvas draws this one heavier.
 */
export const ICON_STROKE: Partial<Record<IconName, number>> = {
  more: 2.25,
};
