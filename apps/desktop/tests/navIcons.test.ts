import assert from "node:assert/strict";
import test from "node:test";
import { NAV, INNER_VIEWS, childrenOf, isGroup } from "../src/lib/navigation.js";
import { NAV_ICON } from "../src/lib/navIcons.js";
import { ICONS } from "../src/lib/icons.js";

test("every leaf, group and inner-view id maps to a real icon", () => {
  const ids = [
    ...NAV.map((entry) => entry.id),
    ...NAV.filter(isGroup).flatMap((group) => childrenOf(group).map((child) => child.id)),
    ...INNER_VIEWS.map((view) => view.id),
  ];
  const byId: Record<string, string> = NAV_ICON;
  for (const id of ids) {
    assert.ok(id in NAV_ICON, `${id} has no entry in NAV_ICON`);
    const icon = byId[id];
    assert.ok(icon !== undefined && icon in ICONS, `${id} maps to ${icon}, not a key of ICONS`);
  }
});
