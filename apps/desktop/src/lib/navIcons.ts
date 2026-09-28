/**
 * The rail's icons, keyed by the same ids `lib/navigation.ts` uses.
 *
 * Centralised here rather than living inside `components/NavIcon.tsx`, so
 * that `tests/navIcons.test.ts` can check every leaf, group and inner-view id
 * has an entry without importing a component. Two shapes, because the icon
 * set mixes two kinds of asset:
 *
 *   - `SUPPLIED` names a CSS mask class (`nav-icon-<name>`) for icons that
 *     ship as separate SVG files under `nav-icon-*` in `styles.css`.
 *   - `PATHS` is an inline `<path>` `d` attribute for the handful of icons
 *     drawn directly, with no separate asset.
 *
 * An id appears in at most one of the two -- `NavIcon` checks `SUPPLIED`
 * first and falls back to `PATHS`, so a later addition to the wrong map would
 * silently never render, and the test above would catch neither.
 */
export const SUPPLIED: Record<string, string> = {
  dictation: "voice", notes: "text", "notes-upload": "upload", developer: "code", cli: "terminal",
  providers: "git-branch", intelligence: "flare", calendar: "calendar-arrow-up",
  transcription: "align-left", diagnostics: "square-wave",
  meetings: "user-plus", "meeting-settings": "user-plus", "local-llm": "tv",
};

export const PATHS: Record<string, string> = {
  vocabulary: "M12 5v15M3 4c4-1 6 0 9 1 3-1 5-2 9-1v15c-4-1-6 0-9 1-3-1-5-2-9-1V4Z",
  shortcuts: "M3 7h18v10H3zM7 11h.01M11 11h.01M15 11h.01M8 14h8",
  settings: "M4 7h6m4 0h6M4 17h10m4 0h2M10 4v6m4 4v6",
  api: "m9 6-6 6 6 6m6-12 6 6-6 6",
  mcp: "M4 4h6v6H4V4Zm10 10h6v6h-6v-6ZM7 10v7h7m3-3V7h-7",
};
