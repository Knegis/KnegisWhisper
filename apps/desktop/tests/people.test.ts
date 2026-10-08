import { test } from "node:test";
import assert from "node:assert/strict";
import { addParticipant, noteCount, participantLabel, personLabel, readParticipants, removeParticipant } from "../src/lib/people.js";

// Fixture 101's participants, exactly as `lib/devFixtures.ts` stores them.
const FIXTURE_101 = JSON.stringify([
  { displayName: "Anna Lindqvist", email: "anna@example.com" },
  { displayName: "Ravi Menon", email: "ravi@example.com" },
]);

test("a note's people are its stored names and emails, in stored order", () => {
  assert.deepEqual(readParticipants(FIXTURE_101), [
    { displayName: "Anna Lindqvist", email: "anna@example.com" },
    { displayName: "Ravi Menon", email: "ravi@example.com" },
  ]);
});

test("no people, or people that will not read, are none", () => {
  for (const json of [null, "", "not json", "{}", JSON.stringify({ email: "anna@example.com" }), "null"]) {
    assert.deepEqual(readParticipants(json), [], String(json));
  }
});

test("an entry with neither a name nor an email is left out, and blanks are none", () => {
  assert.deepEqual(
    readParticipants(JSON.stringify([
      "Axel",
      null,
      { displayName: "  ", email: "" },
      { displayName: 7, email: { at: "x" } },
      { displayName: " Jonas Berg ", email: null },
      { email: " jonas@nordbank.example " },
    ])),
    [
      { displayName: "Jonas Berg", email: null },
      { displayName: null, email: "jonas@nordbank.example" },
    ],
  );
});

test("a person is shown by their name, or by their email where there is none", () => {
  assert.equal(participantLabel({ displayName: "Anna Lindqvist", email: "anna@example.com" }), "Anna Lindqvist");
  assert.equal(participantLabel({ displayName: null, email: "jonas@nordbank.example" }), "jonas@nordbank.example");
  assert.equal(personLabel({ email: "anna@example.com", name: "Anna Lindqvist", noteCount: 2 }), "Anna Lindqvist");
  assert.equal(personLabel({ email: "jonas@nordbank.example", name: null, noteCount: 1 }), "jonas@nordbank.example");
});

test("adding a person appends them, trimmed, with a name only when one was typed", () => {
  const list = readParticipants(FIXTURE_101);
  assert.deepEqual(addParticipant(list, " maja@example.com ", "  Maja Holm "), {
    list: [...list, { displayName: "Maja Holm", email: "maja@example.com" }],
  });
  assert.deepEqual(addParticipant([], "maja@example.com", "   "), {
    list: [{ displayName: null, email: "maja@example.com" }],
  });
  // The list it was given is left as it was.
  assert.equal(list.length, 2);
});

test("adding someone already on the note, in any case, says so instead", () => {
  assert.deepEqual(addParticipant(readParticipants(FIXTURE_101), " ANNA@example.com", "Anna"), {
    error: "ANNA@example.com is already on this note.",
  });
});

test("adding needs an email; whether it is one is Rust's to say", () => {
  assert.deepEqual(addParticipant([], "   ", "Maja"), { error: "Enter an email address." });
  // Not refused here: Rust's check answers it in plain words.
  assert.deepEqual(addParticipant([], "maja@", ""), { list: [{ displayName: null, email: "maja@" }] });
});

test("removing a person leaves the rest in order", () => {
  const list = readParticipants(FIXTURE_101);
  assert.deepEqual(removeParticipant(list, 0), [{ displayName: "Ravi Menon", email: "ravi@example.com" }]);
  assert.deepEqual(removeParticipant(list, 5), list);
  assert.equal(list.length, 2);
});

test("a count is read out with its noun", () => {
  assert.equal(noteCount(1), "1 note");
  assert.equal(noteCount(3), "3 notes");
});
