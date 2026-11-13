import { test } from "node:test";
import assert from "node:assert/strict";
import { updateCheckLabel } from "../src/lib/labels.js";

test("unreadable and empty releases have honest neutral labels", () => {
  assert.deepEqual(updateCheckLabel({ state: "releases-not-public" }), {
    text: "No public release could be found",
    detail: "GitHub answered 404: the repository is private, renamed or removed, so its releases can't be read without signing in.",
  });
  assert.deepEqual(updateCheckLabel({ state: "no-published-release" }), {
    text: "No release has been published yet",
    detail: "GitHub lists no release whose tag reads as a version.",
  });
});

test("failures retain their reason and an unreadable build retains its version", () => {
  assert.deepEqual(updateCheckLabel({ state: "check-failed", error: "offline" }), {
    text: "Could not check for an update", detail: "offline",
  });
  assert.deepEqual(updateCheckLabel({ state: "current-version-unparseable", version: "bad" }), {
    text: "This build's own version could not be read", detail: "unparseable version: bad",
  });
});

test("version labels name the installed and available versions without displaying a URL", () => {
  assert.deepEqual(updateCheckLabel({ state: "up-to-date", current: "0.2.0", latest: "0.2.0" }), {
    text: "Up to date", detail: "You have v0.2.0, the newest published release.",
  });
  assert.deepEqual(updateCheckLabel({ state: "update-available", current: "0.2.0", latest: "0.2.1", url: "https://github.com/Knegis/KnegisWhisper/releases/tag/v0.2.1" }), {
    text: "Update available: v0.2.1", detail: "You have v0.2.0.",
  });
});
