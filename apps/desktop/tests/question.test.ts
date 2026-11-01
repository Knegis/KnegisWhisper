import test from "node:test";
import assert from "node:assert/strict";
import { splitQuestion } from "../src/lib/question.js";
import { discardQuestion } from "../src/lib/meetingView.js";

test("a question splits into what is asked and what it costs", () => {
  assert.deepEqual(splitQuestion("Delete folder “Q3”? Its notes are kept and become unfiled."), {
    title: "Delete folder “Q3”?",
    detail: "Its notes are kept and become unfiled.",
  });
});

test("the meeting discard question keeps its detail out of the title", () => {
  const { title, detail } = splitQuestion(discardQuestion(90));
  assert.equal(title, "Discard this meeting?");
  assert.match(detail, /can't be recovered/);
});

test("a message with no question mark is all title", () => {
  assert.deepEqual(splitQuestion("Replace the summary."), { title: "Replace the summary.", detail: "" });
});
