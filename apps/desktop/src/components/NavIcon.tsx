import { NAV_ICON, type NavIconId } from "../lib/navIcons";
import { Icon } from "./Icon";

/**
 * The rail's icon for a nav id, looked up in `NAV_ICON` and drawn by the
 * shared `Icon` component. `size` defaults to 18, the old sidebar's icon
 * size; the new rail (a later task) passes 22.
 */
export function NavIcon({ id, size = 18 }: { id: NavIconId; size?: number }) {
  return <Icon name={NAV_ICON[id]} size={size} />;
}
