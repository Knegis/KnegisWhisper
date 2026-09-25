import assert from "node:assert/strict";
import test from "node:test";
import { transcriptToMarkdown } from "../src/lib/noteBody.js";

test("transcript copy retains speakers, order, Unicode and literal Markdown", () => {
  assert.equal(transcriptToMarkdown([
    { text: "# **beslut** [länk](url)", speaker: "Åsa" },
    { text: "", speaker: null },
    { text: "1. next\n> quoted", speaker: null },
  ]), "Åsa: \\# \\*\\*beslut\\*\\* \\[länk\\]\\(url\\)\n\n1\\. next  \n\\> quoted");
});

test("empty transcripts stay empty", () => {
  assert.equal(transcriptToMarkdown([]), "");
  assert.equal(transcriptToMarkdown([{ text: " \n", speaker: "Person" }]), "");
});

test("speech cannot introduce entities or indented code", () => {
  assert.equal(transcriptToMarkdown([{ text: "    &copy;", speaker: null }]), "&#32;&#32;&#32;&#32;\\&copy;");
});
