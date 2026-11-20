import test from "node:test";
import assert from "node:assert/strict";
import { ICONS, FILLED, iconStrokeWidth } from "../src/lib/icons.js";

test("every icon is a non-empty path made only of path commands and numbers", () => {
  for (const [name, d] of Object.entries({ ...ICONS, ...FILLED })) {
    assert.ok(d.length > 0, name);
    assert.match(d, /^[MmLlHhVvAaZz0-9.,\s-]+$/, name);
  }
});

test("pause is two closed bars", () => {
  assert.match(FILLED.pause, /^M[^M]*zM[^M]*z$/);
});

test("chevronUp exists and is chevronDown mirrored vertically", () => {
  assert.ok(ICONS.chevronUp.length > 0);
  assert.equal(ICONS.chevronUp, "M7 14l5-5 5 5");
});

test("a name belongs to one family, so Icon never has to choose between two drawings", () => {
  for (const name of Object.keys(FILLED)) assert.ok(!(name in ICONS), name);
});

test("a folder is line, New folder's outline without its plus; Trash is line too, and its own bin", () => {
  assert.ok("folder" in ICONS && !("folder" in FILLED));
  assert.ok(ICONS.folderPlus.startsWith(ICONS.folder), "folderPlus is the folder with a plus added");
  assert.ok("trash" in ICONS && !("trash" in FILLED));
  assert.notEqual(ICONS.trash, ICONS.discard);
  for (const action of ["folderPlus", "discard"]) assert.ok(action in ICONS, action);
});

test("an icon under 18px holds the same weight on screen", () => {
  for (const size of [12, 14, 16]) {
    const onScreen = ((iconStrokeWidth("close", size) ?? 0) * size) / 24;
    assert.ok(Math.abs(onScreen - 1.15) < 0.01, `${size}px draws ${onScreen}px`);
  }
});

test("from 18px up the stroke is left to the stylesheet, so its overrides still apply", () => {
  for (const size of [18, 24, 52]) assert.equal(iconStrokeWidth("dictation", size), undefined, String(size));
});

test("more's dots keep their width at any size", () => {
  for (const size of [14, 18, 24]) assert.equal(iconStrokeWidth("more", size), 3, String(size));
});
