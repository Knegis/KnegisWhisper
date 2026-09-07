/**
 * The rail: the application's name, what version is running, and every screen.
 *
 * It renders `lib/navigation.ts`'s tree and holds no state -- `App.tsx` owns
 * both the current view and the set of open groups, and passes them down. That
 * keeps every piece of navigation state in one component, which is what makes
 * "arriving at a screen opens its group" a single line there rather than an
 * effect here reacting to a prop change.
 *
 * # A group header toggles; it does not navigate
 *
 * The first version of this derived the open group from the current view: the
 * open group was whichever one held the current screen, there was no state, and
 * a header selected its group's first child. It was smaller and it was wrong,
 * for two reasons that only appear once you draw it:
 *
 *   - **A group could not be collapsed**, so `aria-expanded` and any `+`/`-`
 *     marker on the header promised a press that would do nothing. An
 *     affordance that lies is worse than no affordance -- and with no marker at
 *     all, a collapsed `Settings` renders identically to `Dictation`, so nothing
 *     distinguished a section from a screen.
 *   - **Looking inside a group cost you your place.** Pressing `Developer` from
 *     Dictation landed you on Local API. The request this rail answers was to
 *     put the list of options on the left where it can be seen; a list you have
 *     to leave your work to read is not that.
 *
 * So a header is a disclosure control and nothing else, and the two states are
 * marked with a trailing `+` and `-` in `styles.css`. Collapsing the group you
 * are standing in is allowed: the marked item disappears from view, which is
 * what every sidebar that has sections does, and `view` is untouched by it.
 */

import { childrenOf, isGroup, NAV, type ViewId } from "../lib/navigation";

export function Sidebar({
  expanded,
  onToggle,
  view,
  openGroups,
  onSelect,
  onToggleGroup,
  appVersion,
  os,
  arch,
}: {
  expanded: boolean;
  onToggle: () => void;
  view: ViewId;
  openGroups: ReadonlySet<string>;
  onSelect: (id: ViewId) => void;
  onToggleGroup: (id: string) => void;
  appVersion: string;
  os: string;
  arch: string;
}) {
  return (
    <nav className="sidebar" aria-label="Main navigation">
      <button
        type="button"
        className="sidebar-toggle"
        aria-label={expanded ? "Retract sidebar" : "Expand sidebar"}
        title={expanded ? "Retract sidebar" : "Expand sidebar"}
        aria-expanded={expanded}
        aria-controls="sidebar-navigation"
        onClick={onToggle}
      >
        <svg
          width="24"
          height="24"
          viewBox="0 0 24 24"
          fill="none"
          xmlns="http://www.w3.org/2000/svg"
          stroke="currentColor"
          strokeWidth="1.5"
          aria-hidden="true"
          focusable="false"
        >
          <path d="M3 3H21V21H3V3Z" strokeLinecap="square" strokeLinejoin="miter" />
          <path d="M9.5 21V3" strokeLinecap="round" strokeLinejoin="round" />
          <path
            d={expanded ? "M7.25 10L5.5 12L7.25 14" : "M5.5 10L7.25 12L5.5 14"}
            strokeLinecap="square"
            strokeLinejoin="miter"
          />
        </svg>
      </button>
      <div id="sidebar-navigation" hidden={!expanded}>
        <h1>KnegisWhisper</h1>
        <p className="muted">
          v{appVersion} · {os} {arch}
        </p>
        <ul>
          {NAV.map((entry) => {
            if (!isGroup(entry)) {
              return (
                <li key={entry.id}>
                  <button
                    type="button"
                    className={entry.id === view ? "nav-current" : undefined}
                    aria-current={entry.id === view ? "page" : undefined}
                    onClick={() => onSelect(entry.id)}
                  >
                    {entry.label}
                  </button>
                </li>
              );
            }

            const children = childrenOf(entry);
            const open = openGroups.has(entry.id);

            return (
              <li key={entry.id}>
                {/*
                 * No `aria-current` on a header: the child is the page, not the
                 * group. `aria-expanded` is honest here because pressing this is
                 * the only thing that opens or closes the list below it.
                 */}
                <button
                  type="button"
                  className={open ? "nav-group nav-group-open" : "nav-group"}
                  aria-expanded={open}
                  onClick={() => onToggleGroup(entry.id)}
                >
                  {entry.label}
                </button>
                {open ? (
                  <ul className="nav-children">
                    {children.map((child) => (
                      <li key={child.id}>
                        <button
                          type="button"
                          className={
                            child.id === view
                              ? "nav-child nav-current"
                              : "nav-child"
                          }
                          aria-current={child.id === view ? "page" : undefined}
                          onClick={() => onSelect(child.id)}
                        >
                          {child.label}
                        </button>
                      </li>
                    ))}
                  </ul>
                ) : null}
              </li>
            );
          })}
        </ul>
      </div>
    </nav>
  );
}
