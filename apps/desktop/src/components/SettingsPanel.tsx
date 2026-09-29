import { childrenOf, isGroup, NAV, SETTINGS_PANEL, srSuffixOf, type ViewId } from "../lib/navigation";
import { NAV_ICON } from "../lib/navIcons";
import { PanelHeader, PanelItem, PanelSection } from "./Panel";

/**
 * The list panel for every Settings and Developer screen.
 *
 * One section per `SETTINGS_PANEL` entry, each listing its `NAV` group's
 * children in `NAV`'s order: the Settings group first with no heading of its
 * own, then Developer under a "Developer" heading. Rendered by `App.tsx`
 * rather than by each settings screen, because every one of those screens
 * shows the same panel and none of them owns its data -- unlike Dictation,
 * Meetings and Notes, whose panels list their own history.
 *
 * Holds no state. Which item is current is `view`, compared directly: every
 * id listed here is a `NAV` leaf, so there is no inner view to translate the
 * way `railOf` has to.
 */
export function SettingsPanel({ view, onSelect }: { view: ViewId; onSelect: (id: ViewId) => void }) {
  const groups = NAV.filter(isGroup);
  return (
    <>
      <PanelHeader title="Settings" />
      {SETTINGS_PANEL.map((section) => {
        const group = groups.find((entry) => entry.id === section.group);
        if (!group) return null;
        return (
          <PanelSection key={section.group} label={section.heading}>
            {childrenOf(group).map((child) => (
              <PanelItem
                key={child.id}
                title={child.label}
                srSuffix={srSuffixOf(child)}
                icon={NAV_ICON[child.id]}
                current={view === child.id}
                onClick={() => onSelect(child.id)}
              />
            ))}
          </PanelSection>
        );
      })}
    </>
  );
}
