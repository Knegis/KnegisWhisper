#!/usr/bin/env node
/**
 * Runs a crate's tests into the ground and reports which one is not reliable.
 *
 * This exists because of one line in the carry-forward: *"a `kw-storage` test
 * failed once and passed on re-run"* — and the report did not record **which**
 * test, which is half of why it took a session to answer (KNEG-88).
 *
 * The stakes are not the one test. `cargo test --workspace` at 1475 passing is
 * quoted as evidence in four documents. A test that lies in either direction —
 * a false red that gets re-run away, or a false green — undermines every one of
 * those claims, and "it passed the second time" is exactly how a real
 * intermittent defect gets discarded.
 *
 * ## What it does
 *
 *   1. Builds the test binaries and asks **cargo** which they are, rather than
 *      globbing `target/debug/deps` — a glob picks up binaries from older
 *      builds, and running a stale one silently answers a different question.
 *   2. Runs a control: one clean pass. If that fails, it stops and says so.
 *   3. Runs the binaries `--rounds` times, cycling the thread count, and
 *      records the name of every test that ever failed.
 *
 * ## The control is not ceremony
 *
 * The first version of this reported **100% failures with no output**, for
 * every thread count, including what should have been the control. The cause
 * was a trailing carriage return on each path — the tooling that produced the
 * list translated newlines, and the shell that consumed it stripped only the
 * line feed — so nothing was ever executed. A harness that fails everything is
 * measuring itself, and a harness that *passes* everything for the same class
 * of reason would have been reported as a clean result. So: the control runs
 * first, and a clean report is refused unless it passed.
 *
 * ## The thread count is the variable, deliberately
 *
 * A shared-temp-directory or connection-lifetime race is a function of how many
 * tests are in flight at once. Running only the default tests one point on that
 * curve and calls it the curve.
 *
 * ## What a clean run does and does not prove
 *
 * It does not prove the crate has no intermittent test. It bounds how common
 * one can be: 400 rounds of a 56-test binary is 22,400 executions, so a failure
 * rate above roughly 1 in 20,000 would very likely have shown. Anything rarer,
 * or anything that needs conditions this loop does not create — a full
 * workspace run, a machine under load, an antivirus scanner holding a temporary
 * file — is still possible, and the honest report says so rather than closing
 * the question.
 *
 * Usage:
 *   node tools/flake-hunt/hunt.mjs --crate kw-storage [--rounds 400] [--json]
 *   node tools/flake-hunt/hunt.mjs --workspace --rounds 20
 *
 * Exit: 0 if every round passed, 1 if any round failed or the control did not.
 */

import { spawnSync } from "node:child_process";
import { basename } from "node:path";

/** Thread counts to cycle through, smallest first. */
const THREAD_COUNTS = [1, 2, 4, 8, 16];

function parseArgs(argv) {
  const args = { crate: null, workspace: false, rounds: 100, json: false };
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === "--crate") {
      args.crate = argv[i + 1];
      i += 1;
    } else if (arg === "--workspace") {
      args.workspace = true;
    } else if (arg === "--rounds") {
      args.rounds = Number.parseInt(argv[i + 1] ?? "", 10);
      i += 1;
    } else if (arg === "--json") {
      args.json = true;
    } else {
      throw new Error(`unknown argument ${JSON.stringify(arg)}`);
    }
  }
  if (!args.crate && !args.workspace) {
    throw new Error("pass --crate <name> or --workspace");
  }
  if (!Number.isInteger(args.rounds) || args.rounds < 1) {
    throw new Error("--rounds needs a positive whole number");
  }
  return args;
}

/** The scope flags to hand cargo. */
function scope(args) {
  return args.workspace ? ["--workspace"] : ["-p", args.crate];
}

/**
 * The test binaries cargo just built, from cargo's own JSON.
 *
 * Not a glob over `target/debug/deps`: that directory keeps binaries from
 * earlier builds, and running one of those would answer a question about code
 * that is no longer in the tree.
 */
function testBinaries(args) {
  const built = spawnSync(
    "cargo",
    ["test", ...scope(args), "--no-run", "--message-format=json"],
    { encoding: "utf8", maxBuffer: 256 * 1024 * 1024 }
  );
  if (built.status !== 0) {
    throw new Error(`the tests did not build:\n${built.stderr ?? ""}`);
  }

  const found = [];
  for (const line of (built.stdout ?? "").split("\n")) {
    if (!line.trim()) continue;
    let message;
    try {
      message = JSON.parse(line);
    } catch {
      continue;
    }
    if (message?.profile?.test && message.executable) {
      found.push(message.executable);
    }
  }
  return found;
}

/**
 * Which tests failed in one run, by name.
 *
 * Returns an empty array for a clean run. Names come from libtest's own
 * `test <name> ... FAILED` lines, which is the thing the original report was
 * missing.
 */
function failuresIn(binary, threads) {
  const run = spawnSync(binary, [`--test-threads=${threads}`], {
    encoding: "utf8",
    maxBuffer: 64 * 1024 * 1024,
  });
  if (run.status === 0) return [];

  const output = `${run.stdout ?? ""}${run.stderr ?? ""}`;
  const named = [...output.matchAll(/^test (\S+) \.\.\. FAILED$/gm)].map((m) => m[1]);
  if (named.length > 0) return named;

  // Non-zero with no named failure: a crash, a hang killed by the runner, or a
  // binary that could not start. Reported rather than counted as clean, and
  // labelled so it is not mistaken for a test name.
  return [`<the binary exited ${run.status} without naming a test>`];
}

function main() {
  const args = parseArgs(process.argv.slice(2));
  const binaries = testBinaries(args);

  if (binaries.length === 0) {
    console.error("no test binaries were built, so there is nothing to run");
    process.exit(1);
  }

  // The control. Everything below is meaningless if this does not pass.
  const controlFailures = failuresIn(binaries[0], 4);
  if (controlFailures.length > 0) {
    console.error(
      `control FAILED on ${basename(binaries[0])}: ${controlFailures.join(", ")}`
    );
    console.error("nothing below would be a finding, so the hunt did not run.");
    process.exit(1);
  }

  const seen = new Map();
  let rounds = 0;
  let failedRounds = 0;

  for (let round = 1; round <= args.rounds; round += 1) {
    const threads = THREAD_COUNTS[round % THREAD_COUNTS.length];
    for (const binary of binaries) {
      rounds += 1;
      const failures = failuresIn(binary, threads);
      if (failures.length === 0) continue;
      failedRounds += 1;
      for (const name of failures) {
        const key = `${basename(binary)} :: ${name}`;
        const record = seen.get(key) ?? { count: 0, threadCounts: new Set() };
        record.count += 1;
        record.threadCounts.add(threads);
        seen.set(key, record);
      }
    }
    if (!args.json && round % 25 === 0) {
      console.log(`round ${round}/${args.rounds}, ${failedRounds} failed run(s) so far`);
    }
  }

  const findings = [...seen.entries()].map(([test, record]) => ({
    test,
    failures: record.count,
    threadCounts: [...record.threadCounts].sort((a, b) => a - b),
  }));

  if (args.json) {
    console.log(JSON.stringify({ rounds, failedRounds, findings }, null, 2));
  } else if (findings.length === 0) {
    console.log(
      `flake hunt clean — ${rounds} run(s) of ${binaries.length} binary/binaries, ` +
        `thread counts ${THREAD_COUNTS.join(", ")}, control passed.`
    );
    console.log(
      "This bounds how common an intermittent failure can be. It does not prove there is none."
    );
  } else {
    console.log(`flake hunt FOUND ${findings.length} unreliable test(s) in ${rounds} run(s):\n`);
    for (const finding of findings) {
      console.log(
        `  ${finding.test}\n    failed ${finding.failures} time(s), at ` +
          `--test-threads=${finding.threadCounts.join(", ")}`
      );
    }
  }

  process.exit(findings.length === 0 ? 0 : 1);
}

try {
  main();
} catch (err) {
  console.error(String(err instanceof Error ? err.message : err));
  process.exit(1);
}
