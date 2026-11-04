/**
 * The navigation tree, the title bar's pills drawn from it, and the `ViewId`
 * union derived from it.
 *
 * # Three jobs and Settings
 *
 * The pills answer one question: what is this application for. That is three
 * things -- `Dictation`, `Meetings`, `Notes` -- and everything else is
 * configuring one of those three or exposing the application to another
 * program. So the title bar carries four pills (`RAIL` below): the three jobs
 * at its left, and `Settings` at its right. A flat list of thirteen peers,
 * which is what this was before grouping, cannot say that; every screen read
 * as equally important because every screen was one indistinguishable row.
 *
 * `Settings` opens a sheet of tiles over the window (`SETTINGS_PANEL`), and
 * every screen under either of `NAV`'s two groups is a tile there, in one of
 * four sections. **Developer is a section inside that sheet, not a pill**: four
 * machine-facing screens are not a fifth job, and a pill of their own would
 * put them level with the three that are. The groups themselves are unchanged
 * -- `NAV` is still the source of their order and of every label, and
 * `config/ui-surfaces.json` still files the surfaces under the same names.
 * The pills replaced a rail, and the sheet a list panel; the constants keep
 * those older names.
 *
 * # Where each screen went, and why
 *
 * - **Calendar is Settings, not Developer.** It is an OAuth client-ID paste, a
 *   connect and a sync -- configuring an account, which is what the rest of
 *   Settings is.
 * - **Local text models is Settings, not Developer**, which is not the
 *   obvious answer: it is the screen that installs a llama.cpp build, which
 *   sounds machine-facing. It is filed here because `config/ui-surfaces.json`
 *   calls its surface `settings.intelligence.local` and files it directly
 *   under `Settings > Intelligence`. A sheet that disagreed with the config
 *   describing the same screen would be a second, quieter answer to "where
 *   does this live". It is also a user action -- install a model, pick one --
 *   rather than a developer one.
 * - **Vocabulary is Settings, not top-level.** It is a dictionary that
 *   Transcription and Meetings both read, not a job of its own the way
 *   Dictation, Meetings and Notes are -- a colleague reaches it to fix how a
 *   name gets spelled, which is configuration, not a destination.
 * - **Developer is the four machine-facing surfaces**: an HTTP API on
 *   loopback, a CLI binary, an MCP stdio server, and the diagnostics dump.
 *   What they have in common is that their audience is another program, which
 *   is why they sit under their own heading at the foot of the Settings sheet.
 * - **Import is an inner view of Notes, not a pill.** It has no job of
 *   its own -- it exists to put a recording into the writing space Notes
 *   already is -- so it is reached from Notes' own toolbar and empty state
 *   (`INNER_VIEWS` below) rather than sitting beside it in the title bar as a
 *   fourth job.
 *
 * # The Settings order
 *
 * `NAV`'s order is the settings for the three jobs first -- Shortcuts
 * (Dictation's), Meetings, and Vocabulary, which dictation and meetings both
 * read -- and the setup screens after them: Transcription, Cleanup &
 * summaries, Local text models, then Providers and Calendar. The sheet lays
 * them out in that order (`SETTINGS_PANEL`).
 *
 * # The ids are mostly unchanged
 *
 * Every destination the flat array had keeps the id it used there --
 * `meetings` is still the top-level Meetings, which now opens the recording task
 * rather than its configuration. `shortcuts` and `meeting-settings` are new,
 * because the shortcut editor and the meeting configuration had no screen of
 * their own until they moved out of Dictation and Meetings; the second is not
 * called `meetings` because that id is taken, and the unique-id test is the
 * only thing that would catch the clash. `notes-upload` is the one id that
 * left `NAV` entirely, for `INNER_VIEWS` below -- it is still a `ViewId` and
 * still the id `App.tsx`'s switch renders, but it is no longer a destination
 * the navigation lists on its own. The ids are internal -- the names that have to
 * stay stable are the surface ids in `config/ui-surfaces.json`, and those
 * live on the components, not here -- but keeping them identical means
 * `onGoToProviders` and the error boundary's reset target keep working
 * without a translation table between two sets of names for one screen.
 */

/**
 * The tree.
 *
 * `as const` with **no type annotation**, and both halves of that are
 * load-bearing. The literal types are what let `ViewId` below be exactly the
 * leaf ids -- excluding `settings` and `developer`, which are group
 * headers and not destinations. And a `readonly` tuple is what makes
 * `group.children[0]` a real element type: this project compiles with
 * `noUncheckedIndexedAccess`, under which indexing an *array* yields
 * `T | undefined`, but indexing a tuple at a known position does not.
 * Annotating this `readonly NavEntry[]` would quietly cost both.
 */
export const NAV = [
  { id: "dictation", label: "Dictation" },
  { id: "meetings", label: "Meetings" },
  { id: "notes", label: "Notes" },
  {
    id: "settings",
    label: "Settings",
    children: [
      { id: "shortcuts", label: "Shortcuts" },
      // Visually "Meetings", like the top-level entry above; to a screen
      // reader "Meetings settings", so the Meetings pill and this one's tile
      // in the Settings sheet -- both visible at once -- do not share a name.
      // The suffix follows the visible label, so the name still contains it
      // (WCAG 2.5.3, label in name).
      { id: "meeting-settings", label: "Meetings", srSuffix: " settings" },
      { id: "vocabulary", label: "Vocabulary" },
      { id: "transcription", label: "Transcription" },
      { id: "intelligence", label: "Cleanup & summaries" },
      { id: "local-llm", label: "Local text models" },
      { id: "providers", label: "Providers" },
      { id: "calendar", label: "Calendar" },
    ],
  },
  {
    id: "developer",
    label: "Developer",
    children: [
      { id: "api", label: "Local API" },
      // Named for what they are to someone who has not met the acronym;
      // the acronym stays where it is the thing you would search for.
      { id: "cli", label: "CLI" },
      { id: "mcp", label: "MCP" },
      { id: "diagnostics", label: "Diagnostics" },
    ],
  },
] as const;

/**
 * A screen reached from inside another screen rather than from the pills or
 * the Settings sheet.
 *
 * `notes-upload` is still a `ViewId` -- `App.tsx`'s switch still has a case
 * for it, and `PAGES` still carries its title -- but it is not one of `NAV`'s
 * destinations, so neither the pills nor the Settings sheet lists it. `parent`
 * is the pill that should read as current while an inner view is open;
 * `railOf` below is what reads it.
 */
export const INNER_VIEWS = [
  { id: "notes-upload", label: "Import audio or video", parent: "notes" },
] as const;

export type NavEntry = (typeof NAV)[number];

export type NavGroup = Extract<
  NavEntry,
  { readonly children: readonly unknown[] }
>;

/**
 * A group contributes its children; a leaf contributes itself.
 *
 * A conditional type distributes over a union when the checked type is a naked
 * type parameter, so applying this to the whole `NavEntry` union flattens the
 * tree in one step rather than needing a recursive walk.
 */
type LeavesOf<E> = E extends { readonly children: readonly (infer C)[] }
  ? C
  : E;

/**
 * Every screen the shell can show, derived rather than written down.
 *
 * This is the reason the tree is the single source: `App.tsx`'s view switch is
 * exhaustive over `ViewId`, so a nav entry added here without a branch there is
 * a compile error instead of a pill or tile that opens a blank window. The union
 * includes `INNER_VIEWS`' ids too -- `notes-upload` is a screen the switch must
 * render even though the navigation never lists it as a destination.
 */
export type ViewId = LeavesOf<NavEntry>["id"] | (typeof INNER_VIEWS)[number]["id"];

/**
 * Dictation, because that is what the application is for.
 *
 * `satisfies` rather than a `: ViewId` annotation so the constant keeps its
 * literal type while still being checked against the union -- a typo here is a
 * compile error either way, but the annotation would widen it to every leaf id.
 */
export const DEFAULT_VIEW = "dictation" satisfies ViewId;

/**
 * Whether an entry has children.
 *
 * `"children" in entry` rather than `entry.children !== undefined`: no entry
 * declares an optional `children`, so there is no property to test on a leaf.
 * That shape is deliberate -- this project compiles with
 * `exactOptionalPropertyTypes`, where an optional property and a property
 * holding `undefined` are different things, and a tree that never has the
 * second cannot confuse them.
 */
export function isGroup(entry: NavEntry): entry is NavGroup {
  return "children" in entry;
}

/** A destination: an entry that selects a screen rather than a group. */
export type NavLeaf = { readonly id: ViewId; readonly label: string };

/**
 * Text a screen reader hears after a leaf's `label` and nobody sees, or
 * `null` for the leaves that carry none -- every one but `meeting-settings`
 * today.
 *
 * Read with `in` rather than declared as an optional field on `NavLeaf`.
 * Adding `srSuffix?: string` there was tried, and it changes what
 * `NAV.flatMap` infers: a leaf without an optional property is not a strict
 * subtype of one with it, so the flattened list stopped being `NavLeaf[]` and
 * every caller that flattens the tree -- the navigation tests among them --
 * would need a type argument to say what it already meant.
 */
export function srSuffixOf(leaf: NavLeaf): string | null {
  return "srSuffix" in leaf && typeof leaf.srSuffix === "string" ? leaf.srSuffix : null;
}

/**
 * A group's children as one non-empty tuple type, rather than as the union of
 * the two groups' tuples.
 *
 * Narrowing with `isGroup` leaves `entry.children` typed as
 * `readonly [Providers, …] | readonly [LocalApi, …]`, and a method call on a
 * union of array types is not generally callable -- TypeScript unions the
 * `map` signatures and finds no signature compatible with all of them. Widening
 * once here, where the reason can be written down, is better than widening at
 * the call site with a cast.
 *
 * The return type keeps the non-empty shape on purpose: a group with no
 * children is a header that opens onto nothing, and it would not compile
 * against this signature.
 */
export function childrenOf(group: NavGroup): readonly [NavLeaf, ...NavLeaf[]] {
  return group.children;
}

/**
 * The group a screen sits inside, or `null` for a top-level destination.
 *
 * The shell no longer calls this. It was how arriving at a screen -- by a
 * sideways "go to Providers", or the error boundary's jump to Diagnostics --
 * opened the rail group holding it, and the pills that replaced the rail have
 * no groups to open: both groups' children are always tiles in the Settings
 * sheet, and `railOf` below is the question the shell asks instead. It stays as the
 * tree's lookup from a child to its group, which the navigation tests pin.
 */
export function groupOf(view: ViewId): string | null {
  for (const entry of NAV) {
    if (isGroup(entry) && childrenOf(entry).some((c) => c.id === view)) {
      return entry.id;
    }
  }
  return null;
}

/**
 * The small label over a page's title: the part of the application it belongs to,
 * in the pixel voice's lower case. `null` for a top-level destination, whose title
 * stands alone.
 */
export function eyebrowOf(view: ViewId): string | null {
  const group = NAV.find((entry) => isGroup(entry) && entry.id === groupOf(view));
  if (group !== undefined) return group.label.toLowerCase();
  const inner = INNER_VIEWS.find((entry) => entry.id === view);
  if (inner === undefined) return null;
  return NAV.find((entry) => entry.id === inner.parent)?.label.toLowerCase() ?? null;
}

/**
 * The title bar's pills: the three jobs, then `Settings`.
 *
 * Written out rather than derived from `NAV`, because it is not `NAV`'s
 * top level -- `developer` is a top-level group there and is not a pill
 * here. The labels are still `NAV`'s: `NavPills.tsx` reads them from the tree.
 * The name is the rail's, which these pills replaced.
 */
export const RAIL = ["dictation", "meetings", "notes", "settings"] as const;

export type RailId = (typeof RAIL)[number];

/**
 * The pill that reads as current while `view` is showing.
 *
 * A top-level leaf is its own pill. An inner view reads as its `parent` --
 * it is not in `NAV`, so comparing it against the pills directly would mark
 * nothing. And every child of either group reads as `settings`, Developer's
 * four included, because the Settings sheet is where all of them are listed.
 */
export function railOf(view: ViewId): RailId {
  for (const entry of NAV) {
    if (!isGroup(entry) && entry.id === view) return entry.id;
  }
  const inner = INNER_VIEWS.find((entry) => entry.id === view);
  // Not a top-level leaf and not an inner view, so a group child: `ViewId`
  // has no fourth kind.
  return inner ? inner.parent : "settings";
}

/**
 * The Settings sheet's two groups, in `NAV`'s order, and the screens in each. The
 * headings are `NAV`'s labels in the pixel voice's lower case; the navigation test
 * holds the rest -- every screen of both groups on exactly one tile, in order.
 */
export const SETTINGS_PANEL = [
  { heading: "settings", views: ["shortcuts", "meeting-settings", "vocabulary", "transcription", "intelligence", "local-llm", "providers", "calendar"] },
  { heading: "developer", views: ["api", "cli", "mcp", "diagnostics"] },
] as const satisfies readonly { heading: string; views: readonly ViewId[] }[];
