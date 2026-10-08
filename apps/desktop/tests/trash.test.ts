import { test } from "node:test";
import assert from "node:assert/strict";
import { deleteForeverQuestion, emptyTrashQuestion, movedToTrash, recordingLength, trashedMeta, trashSize, trashSpaceLine } from "../src/lib/trash.js";
import { recentMeta } from "../src/lib/meetingView.js";
import type { TrashedNote, TrashSummary } from "../src/generated/ipc-types.js";

const NOW = new Date(2026, 9, 8, 12, 0, 0);

function trashed(given: Partial<TrashedNote> & Pick<TrashedNote, "id">): TrashedNote {
  return {
    title: `Note ${given.id}`,
    trashedAt: "2026-10-03 09:00:00",
    hasRecording: false,
    recordingSeconds: null,
    recordingBytes: 0,
    originalFilename: null,
    ...given,
  };
}

const summary = (notes: TrashedNote[]): TrashSummary => ({ notes, recordingBytes: notes.reduce((sum, note) => sum + note.recordingBytes, 0) });

test("a size reads at the scale a person reads it, in the units a disk is sold in", () => {
  assert.equal(trashSize(412_000_000), "412 MB");
  assert.equal(trashSize(412_499_999), "412 MB");
  assert.equal(trashSize(10_000_000), "10 MB");
  assert.equal(trashSize(3_840_044), "3.8 MB");
  assert.equal(trashSize(1_000_000), "1.0 MB");
  assert.equal(trashSize(1_250_000_000), "1.3 GB");
  assert.equal(trashSize(640_000), "640 KB");
  assert.equal(trashSize(1), "1 KB", "a byte is not nothing");
  assert.equal(trashSize(0), "0 KB");
});

test("a recording's length is minutes, at least one, as the library rounds it", () => {
  assert.equal(recordingLength(1860), "31 min");
  assert.equal(recordingLength(20), "1 min");
  assert.equal(recordingLength(5400), "90 min");
});

test("a trashed note says when it was deleted and how long its recording runs", () => {
  const date = recentMeta("2026-10-03 09:00:00", null, NOW);
  assert.equal(trashedMeta(trashed({ id: 1, hasRecording: true, recordingSeconds: 1860 }), NOW), `Deleted ${date} · 31 min`);
  assert.equal(trashedMeta(trashed({ id: 2 }), NOW), `Deleted ${date}`);
  assert.equal(trashedMeta(trashed({ id: 3, trashedAt: "never", recordingSeconds: 60 }), NOW), "Deleted · 1 min");
});

test("the Trash says what its recordings take on this computer", () => {
  assert.equal(trashSpaceLine(summary([])), "Trash is empty.");
  assert.equal(trashSpaceLine(summary([trashed({ id: 1 }), trashed({ id: 2 })])), "Nothing in Trash has a recording.");
  assert.equal(
    trashSpaceLine(summary([
      trashed({ id: 1, hasRecording: true, recordingBytes: 400_000_000 }),
      trashed({ id: 2, hasRecording: true, recordingBytes: 12_000_000 }),
      trashed({ id: 3 }),
    ])),
    "Recordings in Trash take 412 MB on this computer.",
  );
});

test("Delete forever asks with the note's title and its recording's length, and never a file name", () => {
  const recorded = trashed({ id: 1, title: "Weekly sync", hasRecording: true, recordingSeconds: 1860, recordingBytes: 400_000_000 });
  assert.equal(
    deleteForeverQuestion(recorded),
    "Delete “Weekly sync” forever? Its recording (31 min) is deleted with it. This can't be undone.",
  );
  assert.equal(
    deleteForeverQuestion({ ...recorded, originalFilename: "standup.m4a" }),
    "Delete “Weekly sync” forever? Its recording (31 min) is deleted with it. Your original file isn't touched. This can't be undone.",
  );
  assert.equal(
    deleteForeverQuestion({ ...recorded, recordingSeconds: null }),
    "Delete “Weekly sync” forever? Its recording is deleted with it. This can't be undone.",
  );
  assert.equal(deleteForeverQuestion(trashed({ id: 2, title: "Scratch" })), "Delete “Scratch” forever? This can't be undone.");
  for (const question of [deleteForeverQuestion(recorded), deleteForeverQuestion({ ...recorded, originalFilename: "standup.m4a" })]) {
    assert.doesNotMatch(question, /\.wav|\.m4a|standup/);
  }
});

test("Empty trash asks with how many notes and what their recordings take", () => {
  const three = [
    trashed({ id: 1, hasRecording: true, recordingBytes: 400_000_000 }),
    trashed({ id: 2, hasRecording: true, recordingBytes: 12_000_000 }),
    trashed({ id: 3 }),
  ];
  assert.equal(
    emptyTrashQuestion(summary(three)),
    "Delete 3 notes forever? Their recordings (412 MB) are deleted with them. This can't be undone.",
  );
  assert.equal(
    emptyTrashQuestion(summary([trashed({ id: 1, hasRecording: true, recordingBytes: 2_400_000 })])),
    "Delete 1 note forever? Its recording (2.4 MB) is deleted with it. This can't be undone.",
  );
  assert.equal(emptyTrashQuestion(summary([trashed({ id: 1 }), trashed({ id: 2 })])), "Delete 2 notes forever? This can't be undone.");
  assert.equal(
    emptyTrashQuestion(summary([trashed({ id: 1, hasRecording: true, originalFilename: "call.mp4", recordingBytes: 5_000_000 }), trashed({ id: 2, hasRecording: true, recordingBytes: 0 })])),
    "Delete 2 notes forever? Their recordings (5.0 MB) are deleted with them. Your original files aren't touched. This can't be undone.",
  );
});

test("Delete says where the note went", () => {
  assert.equal(movedToTrash("Weekly sync"), "Moved “Weekly sync” to Trash.");
});
