/** Covers side-effect-free parsing of both visual-check command contracts. */
import test from "node:test";
import assert from "node:assert/strict";
import {
  appUsage,
  parseAppArgs,
  parseShellArgs,
  shellUsage,
} from "./args.mjs";

test("shell defaults and fixture flag parse without I/O", () => {
  const args = parseShellArgs(["--port", "9230", "--keep-fixture-banner"]);
  assert.equal(args.port, 9230);
  assert.equal(args.keepFixtureBanner, true);
  assert.match(args.out, /^tools\/visual-check\/out\//);
});

test("application command defaults and boolean values parse", () => {
  const args = parseAppArgs([
    "watch",
    "--system-audio",
    "false",
    "--via",
    "invoke",
  ]);
  assert.equal(args.command, "watch");
  assert.equal(args.systemAudio, false);
  assert.equal(args.via, "invoke");
});

test("invalid options fail early and help is text", () => {
  assert.throws(() => parseAppArgs(["bogus"]));
  assert.throws(() => parseShellArgs(["--wat"]));
  assert.match(shellUsage(), /Usage:/);
  assert.match(appUsage(), /run-all/);
});
