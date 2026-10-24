import assert from "node:assert/strict";
import test from "node:test";
import {
  CARD_CLEARANCE,
  APP_SCROLL_IDLE_MS,
  RevealQueue,
  ScrollOwnership,
  followBand,
  inBand,
  keyReleasesFollow,
} from "../src/lib/followScroll.js";

test("only a reveal owns scrolls, renewed until 150 ms of inactivity", () => {
  const ownership = new ScrollOwnership();
  assert.equal(ownership.scroll(0), false);
  ownership.begin(10);
  assert.equal(ownership.scroll(100), true);
  assert.equal(ownership.scroll(200), true);
  assert.equal(ownership.scroll(200 + APP_SCROLL_IDLE_MS), false);
});

test("scrollend or user input ends a reveal immediately, including during its glide", () => {
  const ownership = new ScrollOwnership();
  ownership.begin(10);
  ownership.ended();
  assert.equal(ownership.scroll(11), false);
  ownership.begin(20);
  assert.equal(ownership.scroll(21), true);
  ownership.interrupted();
  assert.equal(ownership.scroll(22), false);
  ownership.begin(30);
  assert.equal(ownership.scroll(31), true);
});

test("one reveal waits at a time: the latest, taken once its line can be shown", () => {
  const queue = new RevealQueue();
  assert.equal(queue.take(() => true), null);
  queue.push({ index: 3, block: "center" });
  queue.push({ index: 7, block: "nearest", behavior: "instant" });
  assert.equal(queue.count, 2);
  assert.equal(queue.take(() => false), null, "a hidden line waits");
  const shown = queue.take((index) => index === 7);
  assert.equal(shown?.index, 7);
  assert.equal(shown?.block, "nearest");
  assert.equal(shown?.behavior, "instant");
  assert.equal(queue.take(() => true), null, "taken once");
  assert.equal(queue.count, 2, "the count is of reveals asked for, not shown");
});

test("a reveal that overtakes one still waiting runs that one's then first, once", () => {
  const queue = new RevealQueue();
  const ran: string[] = [];
  queue.push({ index: 1, block: "nearest", then: () => ran.push("source") });
  queue.push({ index: 2, block: "center" });
  queue.push({ index: 3, block: "center", then: () => ran.push("follow") });
  queue.take(() => true)?.then?.();
  assert.deepEqual(ran, ["source", "follow"]);
  queue.push({ index: 4, block: "center", then: () => ran.push("later") });
  queue.take(() => true)?.then?.();
  assert.deepEqual(ran, ["source", "follow", "later"], "a shown reveal's then is not run again");
});

test("with focus on the page or the scroller, every scrolling key lets go, and no other key does", () => {
  for (const key of ["PageUp", "PageDown", "Home", "End", "ArrowUp", "ArrowDown", " "]) {
    assert.equal(keyReleasesFollow(key, "page", false), true, key);
  }
  for (const key of ["ArrowLeft", "ArrowRight", "Enter", "Tab", "Escape", "a"]) {
    assert.equal(keyReleasesFollow(key, "page", false), false, key);
  }
});

test("from a button or line inside the scroller, the scrolling keys let go but Space, which presses the button", () => {
  for (const key of ["PageUp", "PageDown", "Home", "End", "ArrowUp", "ArrowDown"]) {
    assert.equal(keyReleasesFollow(key, "inside", false), true, key);
  }
  assert.equal(keyReleasesFollow(" ", "inside", false), false);
});

test("a key the page handled itself, or one pressed elsewhere, never lets go", () => {
  assert.equal(keyReleasesFollow("PageDown", "page", true), false);
  assert.equal(keyReleasesFollow("ArrowDown", "inside", true), false);
  assert.equal(keyReleasesFollow("PageDown", "elsewhere", false), false);
  assert.equal(keyReleasesFollow(" ", "elsewhere", false), false);
});

test("followBand is the scroller less the head at the top and the card plus CARD_CLEARANCE at the bottom", () => {
  assert.equal(CARD_CLEARANCE, 24);
  assert.deepEqual(followBand({ top: 39, bottom: 900 }, 52, 104), { top: 91, bottom: 772 });
  // No card (Replay holds the player, or the note has no recording): only the clearance.
  assert.deepEqual(followBand({ top: 0, bottom: 500 }, 52, 0), { top: 52, bottom: 476 });
});

test("inBand: wholly inside, within a pixel of either edge for rounding; a line past an edge or taller than the band is not", () => {
  const band = { top: 91, bottom: 772 };
  assert.equal(inBand({ top: 300, bottom: 328 }, band), true);
  assert.equal(inBand({ top: 91, bottom: 772 }, band), true);
  assert.equal(inBand({ top: 90.5, bottom: 772.5 }, band), true);
  assert.equal(inBand({ top: 89, bottom: 117 }, band), false);
  assert.equal(inBand({ top: 750, bottom: 778 }, band), false);
  assert.equal(inBand({ top: 80, bottom: 800 }, band), false);
  // A hidden line -- in a closed section -- measures as an empty box at the origin.
  assert.equal(inBand({ top: 0, bottom: 0 }, band), false);
});
