/**
 * `hitMeta`: a search hit in a meeting transcript says when its words were
 * said, on the recording's clock, before its snippet (KNEG-406).
 */
import assert from "node:assert/strict";
import test from "node:test";

import type { SearchHit, SearchHitRow } from "../src/generated/ipc-types.js";
import { hitMeta } from "../src/lib/searchHit.js";

const hit = (transcriptRow: SearchHitRow | null): SearchHit => ({ id: 1, title: "Weekly", snippet: "the [budget] was agreed", transcriptRow });

test("a transcript hit's time is the player's clock, before the snippet", () => {
  assert.equal(hitMeta(hit({ index: 3, startSeconds: 760.4 })), "12:40 · the [budget] was agreed");
  assert.equal(hitMeta(hit({ index: 3, startSeconds: 3725 })), "1:02:05 · the [budget] was agreed");
});

test("a row with no time, or no row, is the snippet alone", () => {
  assert.equal(hitMeta(hit({ index: 3, startSeconds: null })), "the [budget] was agreed");
  assert.equal(hitMeta(hit(null)), "the [budget] was agreed");
});
