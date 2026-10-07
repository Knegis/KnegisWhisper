/**
 * Settings › Developer in plain words (KNEG-343).
 *
 * On these screens an address, a path or a command is what a developer needs
 * and stays in view; a failure is a sentence, with Rust's reason for Learn
 * more.
 */
import { test } from "node:test";
import assert from "node:assert/strict";
import { blockedLine } from "../src/lib/labels.js";

test("the local API's refusal is a sentence, and a revoked token says what to do", () => {
  // Rust's sentence, from `local_api.rs` `blocked_reason`.
  assert.equal(
    blockedLine(
      "Local API access was revoked: the discovery file is readable by 2 other accounts. Retry to create a new token after permissions are secured.",
    ),
    "Local API access was revoked. Start it again once the discovery file's permissions are secured.",
  );
  assert.equal(blockedLine("could not bind 127.0.0.1:0: access denied"), "The local API couldn't start.");
});
