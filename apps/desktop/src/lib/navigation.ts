/**
 * The navigation tree, and the `ViewId` union derived from it.
 *
 * This was a flat `VIEWS` array of thirteen entries inside `App.tsx`. Thirteen
 * peers in one list is a list with no shape: `Providers`, `Transcription` and
 * `Intelligence` configure the application, `Local API`, `CLI` and `MCP` expose
 * it to other programs, and `Dictation` is what it is actually for -- and none
 * of that was visible, because every one of them was one indistinguishable row
 * in the same column.
 *
 * So there are two groups now, `Settings` and `Developer`, and four top-level
 * destinations. Every one of the thirteen screens survives with the label it
 * already shipped; the only two new user-visible strings in the change are the
 * group names.
 *
 * # Where each screen went, and why
 *
 * - **Calendar is Settings, not Developer.** It is an OAuth client-ID paste, a
 *   connect and a sync -- configuring an account, which is what the rest of
 *   Settings is.
 * - **Local models is Settings, not Developer**, which is not the obvious
 *   answer: it sits beside `CLI` and `MCP` in the request that prompted this,
 *   and it is the screen that installs a llama.cpp build. It is filed here
 *   because `config/ui-surfaces.json` calls its surface
 *   `settings.intelligence.local` and files it directly under
 *   `Settings > Intelligence`. A rail that disagreed with the config
 *   describing the same screen would be a second, quieter answer to "where
 *   does this live". It is also a user action -- install a model, pick one --
 *   rather than a developer one.
 * - **Developer is the four machine-facing surfaces**: an HTTP API on
 *   loopback, a CLI binary, an MCP stdio server, and the diagnostics dump.
 *   What they have in common is that their audience is another program.
 * - **Notes and Upload stay top-level peers** rather than becoming a third
 *   group. `Notes.tsx`'s own heading is `Notes`, so a group would have had to
 *   contain a child of the same name, and neither screen is a setting or a
 *   developer surface.
 *
 * # The order, which is the old order
 *
 * The flat list carried this argument for its order, and it is still the
 * argument: "what someone needs first: a key is useless without a shortcut to
 * dictate with, and both are useless without knowing where the audio goes."
 * That was an ordering of the *setup* path, and the setup path is now the
 * `Settings` group -- in the same relative order, Providers before
 * Transcription before Intelligence, with Shortcuts ahead of all three since
 * the shortcut editor moved there out of Dictation. Grouping did not overturn
 * the reasoning; it moved the place where it applies.
 *
 * # The ids are unchanged
 *
 * Every screen the flat array had keeps the id it used there; `shortcuts` is
 * new, because the shortcut editor had no screen of its own until it moved
 * out of Dictation. The ids are internal -- the names that have to stay
 * stable are the surface ids in `config/ui-surfaces.json`,
 * and those live on the components, not here -- but keeping them identical
 * means `onGoToProviders` and the error boundary's reset target keep working
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
  { id: "vocabulary", label: "Vocabulary" },
  { id: "notes-upload", label: "Upload" },
  {
    id: "settings",
    label: "Settings",
    children: [
      { id: "shortcuts", label: "Shortcuts" },
      { id: "providers", label: "Providers" },
      { id: "transcription", label: "Transcription" },
      { id: "intelligence", label: "Intelligence" },
      { id: "local-llm", label: "Local models" },
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
 * a compile error instead of a rail item that opens a blank window.
 */
export type ViewId = LeavesOf<NavEntry>["id"];

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
 * Shortcuts and Transcription, and Transcription and Intelligence both offer
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
