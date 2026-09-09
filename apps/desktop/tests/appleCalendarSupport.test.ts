/**
 * KNEG-81: the one part of Apple Calendar that is checkable without a Mac.
 *
 * The EventKit binding needs a macOS machine and does not exist. What does not
 * need one is the claim `Integrations > Calendar` makes about it: the Apple row
 * must say it is unsupported *and why*, rather than showing an empty calendar
 * list that reads as "you have none" or hiding the row so the reader cannot
 * tell the difference between "unsupported" and "not asked about".
 *
 * That rule is written twice in prose — `kw_calendar_core::apple`'s module doc
 * and `appleCalendarSupportLabel`'s — and until now was asserted only on the
 * Rust side of the boundary, where `support_is_unsupported_on_every_platform`
 * checks the reason names EventKit and macOS. Nothing checked that the string
 * survived the trip to the screen. These are the frontend half.
 *
 * Written as regression coverage, not as a fix: it passed on first run, which
 * is the right outcome for a verification task and is recorded on the issue as
 * such.
 */
import assert from "node:assert/strict";
import test from "node:test";
import { appleCalendarSupportLabel } from "../src/lib/labels.js";
import type { AppleCalendarSupport } from "../src/generated/ipc-types.js";

/** The real reason, as `kw_calendar_core::apple::REASON` sends it. */
const real: AppleCalendarSupport = {
  state: "unsupported",
  reason:
    "Apple Calendar sync uses EventKit, native macOS API with no Rust binding anywhere in this " +
    "project. This is unimplemented on every platform, including macOS -- not only unavailable " +
    "elsewhere -- and unverifiable on this development machine even once written, since there is " +
    "no macOS machine here to run it on.",
};

test("the Apple row states it is unsupported rather than showing nothing", () => {
  const label = appleCalendarSupportLabel(real);
  assert.notEqual(label.text.trim(), "", "an empty state reads as a row nobody filled in");
  assert.match(label.text, /not supported/i);
});

test("the row carries the backend's reason, so EventKit is named on screen", () => {
  const label = appleCalendarSupportLabel(real);
  // The point of the whole row: not merely "no", but the reason for the no,
  // which is what distinguishes this from an empty calendar list.
  assert.match(label.detail, /EventKit/);
  assert.match(label.detail, /macOS/);
});

test("the reason is passed through whole rather than summarised", () => {
  // A truncation here would be the same defect one layer up: the row would
  // still say "unsupported" and would stop saying why on macOS specifically,
  // which is the half a reader is most likely to be surprised by.
  assert.equal(appleCalendarSupportLabel(real).detail, real.reason);
});
