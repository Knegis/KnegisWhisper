/**
 * The icon for every id `lib/navigation.ts` produces: the three top-level
 * leaves and `settings`, which the rail draws; `developer`, the other group
 * header; every child under the two groups, which the Settings panel draws;
 * and `notes-upload`, the one inner view, which neither lists but which a
 * `NavIconId` has to cover because it is a `ViewId`.
 *
 * `NavIconId` rather than a plain `string` key, so a leaf, group or inner-view
 * id added to `navigation.ts` without a matching entry here is a compile
 * error instead of an icon that silently never renders --
 * `tests/navIcons.test.ts` checks the same thing at the value's end, that
 * every mapped name is actually a key of `ICONS`.
 *
 * Replaces the old `SUPPLIED`/`PATHS` split -- separate CSS-mask and inline
 * `<path>` maps that existed only because some icons shipped as SVG files
 * under `src/assets/navigation` and some did not. Every icon is now a `d`
 * string in `lib/icons.ts`, so one map is enough.
 */
import type { NavEntry, ViewId } from "./navigation.js";
import type { IconName } from "./icons.js";

export type NavIconId = NavEntry["id"] | ViewId;

export const NAV_ICON: Record<NavIconId, IconName> = {
  dictation: "dictation",
  meetings: "meetings",
  "meeting-settings": "meetings",
  notes: "notes",
  "notes-upload": "import",
  settings: "settings",
  developer: "cli",
  shortcuts: "shortcut",
  vocabulary: "vocabulary",
  transcription: "transcription",
  intelligence: "tidy",
  "local-llm": "textModel",
  providers: "key",
  calendar: "calendar",
  api: "api",
  cli: "cli",
  mcp: "mcp",
  diagnostics: "diagnostics",
};
