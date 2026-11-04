import { test } from "node:test";
import assert from "node:assert/strict";
import { importActive, importProgressText, importStages, pollImport } from "../src/lib/importProgress.js";
import type { ImportSnapshot } from "../src/generated/ipc-types.js";
const snapshot = (stage: ImportSnapshot["stage"]): ImportSnapshot => ({ operationId: "a", stage, elapsedSeconds: 65, provider: "local", model: "base", result: null, error: null });
const flush = async () => { await Promise.resolve(); await Promise.resolve(); };
test("stages expose elapsed time without a fabricated percentage", () => {
  assert.equal(importActive(null), false);
  for (const stage of ["preparing", "converting", "transcribing", "saving"] as const) assert.equal(importActive(snapshot(stage)), true);
  for (const stage of ["succeeded", "failed", "cancelled", "interrupted"] as const) assert.equal(importActive(snapshot(stage)), false);
  assert.equal(importProgressText(snapshot("interrupted")), "Import interrupted");
  assert.equal(importProgressText(snapshot("transcribing")), "Transcribing · 1:05 elapsed");
});
test("navigation discards the old response and a new view restores the server snapshot", async () => {
  let resolve!: (value: ImportSnapshot) => void;
  const received: ImportSnapshot[] = [];
  let schedules = 0;
  const stop = pollImport(() => new Promise<ImportSnapshot>(r => { resolve = r; }), v => received.push(v), () => assert.fail(), () => { schedules++; return () => {}; });
  stop(); resolve(snapshot("transcribing")); await flush();
  assert.equal(received.length, 0); assert.equal(schedules, 0);
  const stopNew = pollImport(async () => snapshot("succeeded"), v => received.push(v), () => assert.fail(), () => () => {});
  await flush(); stopNew();
  assert.equal(received[0]?.stage, "succeeded");
});
test("polling serializes reads and retries a read failure", async () => {
  let next: (() => void) | undefined;
  let reads = 0, failures = 0;
  const values: string[] = [];
  const stop = pollImport(async () => { reads++; if (reads === 1) throw Error("IPC unavailable"); return "new operation"; }, v => values.push(v), () => { failures++; }, fn => { next = fn; return () => { next = undefined; }; });
  await flush(); assert.equal(reads, 1); assert.equal(failures, 1);
  next?.(); await flush(); assert.equal(reads, 2); assert.deepEqual(values, ["new operation"]);
  stop(); assert.equal(next, undefined);
});

test("the four stages are drawn only while an import runs", () => {
  assert.equal(importStages(null), null);
  for (const stage of ["succeeded", "failed", "cancelled", "interrupted"] as const) {
    assert.equal(importStages(snapshot(stage)), null, stage);
  }
});

test("each stage before the current one is done, and each after it still to come", () => {
  const stages = importStages(snapshot("transcribing"));
  assert.deepEqual(stages?.map((s) => s.label), ["Preparing import", "Converting audio", "Transcribing", "Saving note and recording"]);
  assert.deepEqual(stages?.map((s) => s.state), ["done", "done", "current", "todo"]);
  assert.deepEqual(stages?.map((s) => s.number), ["01", "02", "03", "04"]);
  assert.deepEqual(importStages(snapshot("preparing"))?.map((s) => s.state), ["current", "todo", "todo", "todo"]);
  assert.deepEqual(importStages(snapshot("saving"))?.map((s) => s.state), ["done", "done", "done", "current"]);
});
