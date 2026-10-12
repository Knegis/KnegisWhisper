import assert from "node:assert/strict";
import test from "node:test";
import {
  LONG_TRANSCRIPT_SECONDS,
  LONG_TRANSCRIPT_SEGMENTS,
  longTranscriptRows,
} from "../src/lib/fixtureTranscript.js";
import { findMatches } from "../src/lib/transcriptFind.js";
import { transcriptSections } from "../src/lib/transcriptLayout.js";
import { wordsMatchText } from "../src/lib/wordTiming.js";

const rows = longTranscriptRows();

test("the long fixture note makes three titled sections", () => {
  assert.deepEqual(
    transcriptSections(rows).map((section) => section.title),
    ["0:00 – 5:00", "5:00 – 10:00", "10:00 – 11:30"],
  );
});

test("the rows agree with the stored segments", () => {
  assert.equal(rows.length, LONG_TRANSCRIPT_SEGMENTS.length);
  rows.forEach((row, at) => {
    assert.equal(row.index, at);
    assert.equal(row.text, LONG_TRANSCRIPT_SEGMENTS[at]!.text);
  });
});

test("some turn has three lines or more, and some line has no speaker at all", () => {
  const turns = transcriptSections(rows).flatMap((section) => section.turns);
  assert.ok(turns.some((turn) => turn.rows.length >= 3));
  assert.ok(rows.some((row) => row.speaker === null && row.speakerId === null));
});

test("an unnamed diarised speaker shows as Speaker N with its label, as rows() has it", () => {
  assert.equal(rows.find((row) => row.speakerId === "speaker_2")?.speaker, "Speaker 3");
  assert.equal(rows.find((row) => row.speakerId === "speaker_0")?.speaker, "Maja");
  assert.equal(rows.find((row) => row.speakerId === "speaker_1")?.speaker, "Jonas");
});

test("one line has no start time, in the middle of a turn", () => {
  const untimed = rows.filter((row) => row.startSeconds === null);
  assert.equal(untimed.length, 1);
  const turn = transcriptSections(rows)
    .flatMap((section) => section.turns)
    .find((candidate) => candidate.rows.includes(untimed[0]!));
  assert.ok(turn !== undefined && turn.rows.length >= 3 && turn.rows[0] !== untimed[0]);
});

test("a pause of a minute between two lines of one speaker splits their turn", () => {
  const turns = transcriptSections(rows).flatMap((section) => section.turns);
  const before = rows.find((row) => row.startSeconds === 420)!;
  const after = rows.find((row) => row.startSeconds === 510)!;
  assert.equal(before.speakerId, after.speakerId);
  assert.equal(after.index, before.index + 1);
  const turnOf = (index: number) => turns.findIndex((turn) => turn.rows.some((row) => row.index === index));
  assert.notEqual(turnOf(before.index), turnOf(after.index));
});

test("every worded row's words spell its text, and sections one and three have some", () => {
  const worded = rows.filter((row) => row.words.length > 0);
  for (const row of worded) assert.ok(wordsMatchText(row.words, row.text), row.text);
  assert.ok(worded.some((row) => (row.startSeconds ?? 0) < 300));
  assert.ok(worded.some((row) => (row.startSeconds ?? 0) >= 600));
});

test("every row ends within the recording", () => {
  for (const row of rows) assert.ok((row.endSeconds ?? 0) <= LONG_TRANSCRIPT_SECONDS, row.text);
  assert.equal(Math.max(...rows.map((row) => row.endSeconds ?? 0)), LONG_TRANSCRIPT_SECONDS);
});

test("find: möte is in three rows, accents count, and nothing else says möte", () => {
  assert.equal(findMatches(rows, "möte").length, 3);
  assert.equal(findMatches(rows, "MÖTE").length, 3);
  assert.equal(findMatches(rows, "mote").length, 1);
  assert.equal(findMatches(rows, "remote").length, 1);
  assert.equal(findMatches(rows, "Mötet").length, 1);
});

test("none of the strings other checks count is in the note", () => {
  const all = JSON.stringify(LONG_TRANSCRIPT_SEGMENTS).toLowerCase();
  for (const avoided of ["honest", "thursday", "more days", "wasapi", "anna lindqvist"]) {
    assert.ok(!all.includes(avoided), avoided);
  }
});
