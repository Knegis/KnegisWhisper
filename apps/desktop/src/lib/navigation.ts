/**
 * The navigation tree, and the `ViewId` union derived from it.
 *
 * # Three jobs, not thirteen screens
 *
 * The rail answers one question: what is this application for. That is three
 * things -- `Dictation`, `Meetings`, `Notes` -- and everything else is either
 * configuring one of those three (`Settings`) or exposing the application to
 * another program (`Developer`). A flat list of thirteen peers, which is what
 * this was before grouping, cannot say that; every screen read as equally
 * important because every screen was one indistinguishable row in the same
 * column.
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
 *   under `Settings > Intelligence`. A rail that disagreed with the config
 *   describing the same screen would be a second, quieter answer to "where
 *   does this live". It is also a user action -- install a model, pick one --
 *   rather than a developer one.
 * - **Vocabulary is Settings, not top-level.** It is a dictionary that
 *   Transcription and Meetings both read, not a job of its own the way
 *   Dictation, Meetings and Notes are -- a colleague reaches it to fix how a
 *   name gets spelled, which is configuration, not a destination.
 * - **Developer is the four machine-facing surfaces**: an HTTP API on
 *   loopback, a CLI binary, an MCP stdio server, and the diagnostics dump.
 *   What they have in common is that their audience is another program.
 * - **Import is an inner view of Notes, not a rail entry.** It has no job of
 *   its own -- it exists to put a recording into the writing space Notes
 *   already is -- so it is reached from Notes' own toolbar and empty state
 *   (`INNER_VIEWS` below) rather than sitting beside it in the rail as a
 *   fourth top-level peer.
 *
 * # The Settings order (Design decision 7)
 *
 * Shortcuts and Meetings settings come first, ahead of the three provider/
 * transcription/model screens, because the shortcut editor moved out of
 * Dictation and the meeting configuration out of Meetings -- a colleague
 * who came from either task screen to change a setting should find it near
 * the top, not after screens that configure a different job entirely.
 * Vocabulary follows, then Transcription, Intelligence and Local text models
 * keep the old setup-path order ("a key is useless without a shortcut to
 * dictate with, and both are useless without knowing where the audio goes"),
 * and Providers moves down next to Calendar -- both are account-level
 * connections a colleague visits rarely, once either is already configured
 * once.
 *
 * # The ids are mostly unchanged
 *
 * Every destination the flat array had keeps the id it used there --
 * `meetings` is still the rail's Meetings, which now opens the recording task
 * rather than its configuration. `shortcuts` and `meeting-settings` are new,
 * because the shortcut editor and the meeting configuration had no screen of
 * their own until they moved out of Dictation and Meetings; the second is not
 * called `meetings` because that id is taken, and the unique-id test is the
 * only thing that would catch the clash. `notes-upload` is the one id that
 * left `NAV` entirely, for `INNER_VIEWS` below -- it is still a `ViewId` and
 * still the id `App.tsx`'s switch renders, but it is no longer a destination
 * the rail lists on its own. The ids are internal -- the names that have to
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
      // reader "Meetings settings", so two buttons in one list do not share a
      // name. The suffix follows the visible label, so the name still
      // contains it (WCAG 2.5.3, label in name).
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
      { id: "cli", label: "CLI" },
      { id: "mcp", label: "MCP" },
      { id: "diagnostics", label: "Diagnostics" },
    ],
  },
] as const;

/**
 * A screen reached from inside another screen rather than from the rail.
 *
 * `notes-upload` is still a `ViewId` -- `App.tsx`'s switch still has a case
 * for it, and `PAGES` still carries its title -- but it is not one of `NAV`'s
 * destinations, so `Sidebar.tsx` never lists it and `groupOf` never opens a
 * group for it. `parent` is the rail entry that should read as current while
 * an inner view is open; `railIdOf` below is what reads it.
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
 * a compile error instead of a rail item that opens a blank window. The union
 * includes `INNER_VIEWS`' ids too -- `notes-upload` is a screen the switch must
 * render even though the rail never lists it as a destination.
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
 * This exists so that arriving at a screen opens the group holding it, however
 * you arrived. Screens navigate sideways -- Dictation's Change buttons open
 * Shortcuts and Transcription, Meetings' opens its settings under Settings,
 * and Transcription and Intelligence both offer
 * "go to Providers" -- and the error boundary sends you to Diagnostics, which
 * lives under `Developer`. Without this, any of those would leave the rail
 * showing a collapsed group and no marked item at all.
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
 * The rail entry that should read as current while `view` is showing.
 *
 * An inner view is not in `NAV`, so comparing it against the rail directly
 * would mark nothing -- `Sidebar.tsx` compares `railIdOf(view)` instead of
 * `view` for exactly this reason. For every other `ViewId` this is the
 * identity function: a screen that is already a rail entry reads as itself.
 */
export function railIdOf(view: ViewId): ViewId {
  const inner = INNER_VIEWS.find((entry) => entry.id === view);
  return inner ? inner.parent : view;
}
