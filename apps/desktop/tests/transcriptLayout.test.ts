import assert from "node:assert/strict";
import test from "node:test";
import type { TranscriptRow } from "../src/generated/ipc-types.js";
import {
  TURN_GAP_SECONDS,
  blockSecondsFor,
  lastStartedLine,
  lineAt,
  rangeSelection,
  sectionMarks,
  selectionInterval,
  transcriptExtent,
  transcriptSections,
} from "../src/lib/transcriptLayout.js";

type Opts = { speaker?: string | null; speakerId?: string | null; text?: string };

function row(index: number, start: number | null, end: number | null, opts: Opts = {}): TranscriptRow {
  return {
    index,
    startSeconds: start,
    endSeconds: end,
    speaker: opts.speaker ?? null,
    speakerId: opts.speakerId ?? null,
    text: opts.text ?? `line ${index}`,
    words: [],
  };
}

/** Rows one after another, 10 s each, from `start`, all by one speaker id. */
function run(first: number, count: number, start: number, speakerId = "a"): TranscriptRow[] {
  return Array.from({ length: count }, (_, i) =>
    row(first + i, start + i * 10, start + i * 10 + 10, { speakerId }),
  );
}

test("blockSecondsFor picks the block size by extent, boundaries inclusive", () => {
  assert.equal(blockSecondsFor(0), 300);
  assert.equal(blockSecondsFor(3600), 300);
  assert.equal(blockSecondsFor(3600.5), 600);
  assert.equal(blockSecondsFor(10800), 600);
  assert.equal(blockSecondsFor(10800.5), 900);
});

test("transcriptExtent is the max of end ?? start ?? 0", () => {
  assert.equal(transcriptExtent([]), 0);
  assert.equal(transcriptExtent([row(0, null, null)]), 0);
  assert.equal(transcriptExtent([row(0, 5, null), row(1, 3, 9)]), 9);
  assert.equal(transcriptExtent([row(0, 40, null), row(1, 3, 9)]), 40);
});

test("empty input has no sections; all-untimed input is one titleless section", () => {
  assert.deepEqual(transcriptSections([]), []);
  const sections = transcriptSections([row(0, null, null), row(1, null, null)]);
  assert.equal(sections.length, 1);
  const s = sections[0]!;
  assert.equal(s.key, "s0");
  assert.equal(s.startSeconds, null);
  assert.equal(s.endSeconds, null);
  assert.equal(s.title, "");
  assert.equal(s.lineCount, 2);
  assert.equal(s.turns.length, 1);
});

test("a row at 299.9 s stays in the first block; exactly 300.0 s opens the second", () => {
  const rows = [row(0, 0, 100), row(1, 299.9, 300), row(2, 300, 310), row(3, 400, 420)];
  const sections = transcriptSections(rows);
  assert.equal(sections.length, 2);
  assert.equal(sections[0]!.lineCount, 2);
  assert.equal(sections[1]!.lineCount, 2);
  assert.equal(sections[0]!.key, "s0");
  assert.equal(sections[1]!.key, "s300");
  assert.equal(sections[0]!.startSeconds, 0);
  assert.equal(sections[0]!.endSeconds, 300);
  assert.equal(sections[1]!.startSeconds, 300);
});

test("empty blocks produce no section", () => {
  const rows = [row(0, 10, 20), row(1, 1000, 1010)];
  const sections = transcriptSections(rows);
  assert.deepEqual(sections.map((s) => s.key), ["s0", "s900"]);
});

test("a row that goes backwards stays in the current section", () => {
  const rows = [row(0, 10, 20), row(1, 310, 320), row(2, 50, 60), row(3, 330, 340)];
  const sections = transcriptSections(rows);
  assert.equal(sections.length, 2);
  assert.deepEqual(
    sections[1]!.turns.flatMap((t) => t.rows.map((r) => r.index)),
    [1, 2, 3],
  );
});

test("rows are never reordered", () => {
  const rows = [row(0, 10, 20), row(1, 310, 320), row(2, 50, 60), row(3, 700, 710)];
  const flat = transcriptSections(rows).flatMap((s) => s.turns.flatMap((t) => t.rows.map((r) => r.index)));
  assert.deepEqual(flat, [0, 1, 2, 3]);
});

test("section titles: m:ss and h:mm:ss with spaced en dash; last section ends at the extent", () => {
  const rows = [row(0, 10, 20), row(1, 310, 400)];
  const sections = transcriptSections(rows);
  assert.equal(sections[0]!.title, "0:00 – 5:00");
  assert.equal(sections[1]!.title, "5:00 – 6:40");
  assert.equal(sections[1]!.endSeconds, 400);

  // Extent beyond an hour uses 10-minute blocks.
  const long = [row(0, 0, 10), row(1, 3600, 3700), row(2, 3700, 4000)];
  const ls = transcriptSections(long);
  assert.equal(ls[0]!.title, "0:00 – 10:00");
  assert.equal(ls[1]!.title, "1:00:00 – 1:06:40");
});

test("hour title for a non-last section uses h:mm:ss on both sides", () => {
  // Past an hour the block is 10 minutes, so 1:00:00 - 1:05:00 cannot occur:
  // a transcript that reaches 3600 s only has 300 s blocks if it ends there.
  const rows = [row(0, 3600, 3610), row(1, 4200, 4210)];
  const sections = transcriptSections(rows);
  assert.equal(sections[0]!.title, "1:00:00 – 1:10:00");
  assert.equal(sections[1]!.title, "1:10:00 – 1:10:10");
});

test("untimed rows join the previous row's section and turn", () => {
  const rows = [
    row(0, 10, 20, { speakerId: "a" }),
    row(1, null, null, { speakerId: "b" }),
    row(2, 310, 320, { speakerId: "a" }),
    row(3, null, null),
  ];
  const sections = transcriptSections(rows);
  assert.equal(sections.length, 2);
  assert.equal(sections[0]!.turns.length, 1);
  assert.deepEqual(sections[0]!.turns[0]!.rows.map((r) => r.index), [0, 1]);
  assert.equal(sections[0]!.lineCount, 2);
  assert.deepEqual(sections[1]!.turns[0]!.rows.map((r) => r.index), [2, 3]);
});

test("leading untimed rows go into the first section, as their own turn", () => {
  const rows = [row(0, null, null, { speakerId: "a" }), row(1, 10, 20, { speakerId: "a" })];
  const sections = transcriptSections(rows);
  assert.equal(sections.length, 1);
  assert.equal(sections[0]!.lineCount, 2);
  assert.equal(sections[0]!.turns[0]!.key, "t0");
  assert.equal(sections[0]!.turns[0]!.startSeconds, 10);
});

test("the same speakerId merges consecutive rows into one turn", () => {
  const rows = [
    row(0, 0, 5, { speakerId: "a", speaker: "Ann" }),
    row(1, 5, 10, { speakerId: "a", speaker: "Ann" }),
  ];
  const turns = transcriptSections(rows)[0]!.turns;
  assert.equal(turns.length, 1);
  assert.equal(turns[0]!.key, "t0");
  assert.equal(turns[0]!.speaker, "Ann");
  assert.equal(turns[0]!.speakerId, "a");
  assert.equal(turns[0]!.startSeconds, 0);
});

test("the same name with different ids splits", () => {
  const rows = [
    row(0, 0, 5, { speakerId: "a", speaker: "Sam" }),
    row(1, 5, 10, { speakerId: "b", speaker: "Sam" }),
  ];
  assert.equal(transcriptSections(rows)[0]!.turns.length, 2);
});

test("ids and names are separate namespaces", () => {
  const rows = [
    row(0, 0, 5, { speakerId: "speaker_0" }),
    row(1, 5, 10, { speaker: "speaker_0" }),
  ];
  assert.equal(transcriptSections(rows)[0]!.turns.length, 2);
});

test("rows with no id merge by name", () => {
  const rows = [row(0, 0, 5, { speaker: "Ann" }), row(1, 5, 10, { speaker: "Ann" })];
  assert.equal(transcriptSections(rows)[0]!.turns.length, 1);
});

test("unnamed rows never merge", () => {
  const rows = [row(0, 0, 5), row(1, 5, 10), row(2, 10, 15)];
  const turns = transcriptSections(rows)[0]!.turns;
  assert.deepEqual(turns.map((t) => t.key), ["t0", "t1", "t2"]);
});

test("a pause of at least TURN_GAP_SECONDS splits; just under does not", () => {
  assert.equal(TURN_GAP_SECONDS, 60);
  const under = [row(0, 0, 10, { speakerId: "a" }), row(1, 69.9, 80, { speakerId: "a" })];
  assert.equal(transcriptSections(under)[0]!.turns.length, 1);
  const exact = [row(0, 0, 10, { speakerId: "a" }), row(1, 70, 80, { speakerId: "a" })];
  assert.equal(transcriptSections(exact)[0]!.turns.length, 2);
});

test("the gap is measured from the previous end, or its start when the end is null", () => {
  const rows = [row(0, 0, null, { speakerId: "a" }), row(1, 61, 70, { speakerId: "a" })];
  assert.equal(transcriptSections(rows)[0]!.turns.length, 2);
});

test("a section boundary splits a turn", () => {
  const rows = [row(0, 290, 299, { speakerId: "a" }), row(1, 300, 310, { speakerId: "a" })];
  const sections = transcriptSections(rows);
  assert.equal(sections.length, 2);
  assert.equal(sections[0]!.turns.length, 1);
  assert.equal(sections[1]!.turns.length, 1);
});

test("turn keys derive from the row index, so a rename does not change them", () => {
  const before = [row(0, 0, 5, { speakerId: "a", speaker: "Ann" }), row(1, 5, 10, { speakerId: "b", speaker: "Bo" })];
  const after = [row(0, 0, 5, { speakerId: "a", speaker: "Anna" }), row(1, 5, 10, { speakerId: "b", speaker: "Bob" })];
  const k = (rs: TranscriptRow[]) => transcriptSections(rs)[0]!.turns.map((t) => t.key);
  assert.deepEqual(k(before), ["t0", "t1"]);
  assert.deepEqual(k(after), k(before));
});

test("lineCount counts every row in the section", () => {
  const rows = [...run(0, 3, 0), row(3, null, null), ...run(4, 2, 300)];
  const sections = transcriptSections(rows);
  assert.deepEqual(sections.map((s) => s.lineCount), [4, 2]);
});

test("sectionMarks are the starts after the first; one section gives none", () => {
  const rows = [row(0, 0, 10), row(1, 310, 320), row(2, 910, 920)];
  const sections = transcriptSections(rows);
  assert.deepEqual(sectionMarks(sections), [300, 900]);
  assert.deepEqual(sectionMarks(transcriptSections([row(0, 0, 10)])), []);
  assert.deepEqual(sectionMarks([]), []);
});

test("lineAt is today's playing-row predicate; lastStartedLine holds through gaps", () => {
  const rows = [row(0, 0, 10), row(1, 20, 30), row(2, 30, null), row(3, null, null)];
  assert.equal(lineAt(rows, 5)?.index, 0);
  assert.equal(lineAt(rows, 10), null); // end is exclusive
  assert.equal(lineAt(rows, 15), null); // in the gap
  assert.equal(lastStartedLine(rows, 15)?.index, 0);
  assert.equal(lineAt(rows, 30)?.index, 2);
  assert.equal(lineAt(rows, 9999)?.index, 2); // null end means to the end
  assert.equal(lastStartedLine(rows, 9999)?.index, 2);
  assert.equal(lastStartedLine(rows, -1), null);
  assert.equal(lineAt([], 1), null);
});

test("rangeSelection returns playable indexes between anchor and target, both directions", () => {
  const rows = [row(10, 0, 5), row(11, null, null), row(12, 5, 10), row(13, 10, 15)];
  assert.deepEqual(rangeSelection(rows, 10, 13), [10, 12, 13]);
  assert.deepEqual(rangeSelection(rows, 13, 10), [10, 12, 13]);
  assert.deepEqual(rangeSelection(rows, 12, 12), [12]);
  assert.deepEqual(rangeSelection(rows, 10, 99), []);
  assert.deepEqual(rangeSelection(rows, 99, 10), []);
});

test("selectionInterval needs two playable selected rows", () => {
  const rows = [row(0, 72, 80), row(1, null, null), row(2, 80, 95), row(3, 95, null)];
  assert.equal(selectionInterval(rows, new Set([0]), 200), null);
  assert.equal(selectionInterval(rows, new Set([0, 1]), 200), null);
  assert.deepEqual(selectionInterval(rows, new Set([2, 0]), 200), {
    start: 72,
    end: 95,
    label: "1:12–1:35",
  });
  assert.deepEqual(selectionInterval(rows, new Set([0, 3]), 200), {
    start: 72,
    end: 200,
    label: "1:12–3:20",
  });
  assert.deepEqual(selectionInterval(rows, new Set([0, 3]), null), {
    start: 72,
    end: null,
    label: "1:12–1:12",
  });
});
