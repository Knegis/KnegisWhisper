import assert from "node:assert/strict";
import test from "node:test";
import { NAV, INNER_VIEWS, childrenOf, isGroup } from "../src/lib/navigation.js";
import { SUPPLIED, PATHS } from "../src/lib/navIcons.js";

test("every leaf, group and inner-view id has an icon", () => {
  const ids = [
    ...NAV.map((entry) => entry.id),
    ...NAV.filter(isGroup).flatMap((group) => childrenOf(group).map((child) => child.id)),
    ...INNER_VIEWS.map((view) => view.id),
  ];
  for (const id of ids) {
    assert.ok(SUPPLIED[id] || PATHS[id], `${id} has no icon in SUPPLIED or PATHS`);
  }
});
