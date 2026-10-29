import { NAV_ICON, type NavIconId } from "../lib/navIcons";
import { Icon } from "./Icon";

/**
 * The icon for a nav id, looked up in `NAV_ICON` and drawn by the shared
 * `Icon` component. `size` defaults to 18, the size of every other icon in
 * the window; the rail passed 22. Nothing renders this now: the Settings
 * sheet's tiles look `NAV_ICON` up and draw it through `Icon` directly.
 */
export function NavIcon({ id, size = 18 }: { id: NavIconId; size?: number }) {
  return <Icon name={NAV_ICON[id]} size={size} />;
}
