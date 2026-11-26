import { test } from "node:test";
import assert from "node:assert/strict";
import { deletedLeftOutLabel, importReportSummary, leftBehindTableLabels } from "../src/lib/labels.js";
import type { ImportReport } from "../src/lib/ipc.js";

const report: ImportReport = {
  source: "source.db", tables: [], absentTables: [], leftBehindTables: [],
  relocatedCredentials: [], ftsRows: 0,
};

test("tables with rows come first, with singular and plural counts; empty tables share a line", () => {
  const input: ImportReport = { ...report, leftBehindTables: [
    { table: "z_empty", rows: 0 }, { table: "spaces", rows: 1 },
    { table: "a_empty", rows: 0 }, { table: "other", rows: 2 },
  ] };
  const before = structuredClone(input);
  assert.deepEqual(leftBehindTableLabels(input), [
    "other: 2 rows", "spaces: 1 row",
    "Empty, or could not be counted: a_empty, z_empty",
  ]);
  assert.deepEqual(input, before);
});

test("nothing left behind produces no table lines", () => {
  assert.deepEqual(leftBehindTableLabels(report), []);
  assert.deepEqual(leftBehindTableLabels({ ...report, leftBehindTables: [{ table: "empty", rows: 0 }] }),
    ["Empty, or could not be counted: empty"]);
});

test("deleted rows get a separate suffix only when present", () => {
  assert.equal(deletedLeftOutLabel(0), "");
  assert.equal(deletedLeftOutLabel(1), ", 1 deleted in OpenWhispr and not copied");
  assert.equal(deletedLeftOutLabel(3), ", 3 deleted in OpenWhispr and not copied");
});

test("the import summary keeps its copied and skipped meanings", () => {
  assert.equal(importReportSummary(report), "0 rows copied.");
  const table = { table: "notes", copied: 1, skipped: 0, deletedLeftOut: 9,
    absentColumns: [], droppedColumns: [], skipReason: null };
  assert.equal(importReportSummary({ ...report, tables: [table] }), "1 row copied.");
  assert.equal(importReportSummary({ ...report, tables: [table, { ...table, table: "folders", copied: 2, skipped: 1 }] }),
    "3 rows copied, 1 skipped.");
});
