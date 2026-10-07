/**
 * The local model screens in plain words (KNEG-343): Settings › Transcription
 * and Local text models.
 *
 * The copy rule: what a person must know or do stays in `text`; digests, ids,
 * file names, ports and the mechanism go to `more` or `detail`, for Learn
 * more. Nothing true is dropped, so each test checks both halves.
 */
import { test } from "node:test";
import assert from "node:assert/strict";
import {
  downloadCheckPlain,
  freshnessLabel,
  ggufIntegrityLabel,
  llmProgressLabel,
  modelLanguage,
  progressLabel,
  recheckOutcomeLabel,
  refreshOutcomeLabel,
  useForDetail,
  verifiedPlain,
} from "../src/lib/labels.js";

const INSTALLED = "1f3a9c2b7d4e5f60718293a4b5c6d7e8f90a1b2c3d4e5f60718293a4b5c6d7e8";
const PINNED = "9e8d7c6b5a4f3e2d1c0b9a8f7e6d5c4b3a2f1e0d9c8b7a6f5e4d3c2b1a0f9e8d";
const HEX = /[0-9a-f]{12}/;
const names: Record<string, string> = { "kb-whisper-base": "KB Whisper Base", "qwen2.5-7b-instruct": "Qwen2.5 7B Instruct" };
const nameOf = (id: string) => names[id] ?? id;

test("freshness says whether the engine is the version the app expects, with the builds behind Learn more", () => {
  assert.equal(freshnessLabel({ state: "not-installed" }), undefined);
  assert.deepEqual(freshnessLabel({ state: "current" }), {
    text: "Up to date.",
    more: "This is the build this version of the app pins.",
  });

  const stale = freshnessLabel({ state: "stale", installed: INSTALLED, expected: PINNED });
  assert.equal(stale?.text, "Not the version this app expects.");
  assert.doesNotMatch(stale?.text ?? "", HEX);
  assert.match(stale?.more ?? "", /1f3a9c2b7d4e/);
  assert.match(stale?.more ?? "", /9e8d7c6b5a4f/);

  const unknown = freshnessLabel({ state: "unknown", why: "no install record was written" });
  assert.equal(unknown?.text, "It isn't known whether this is the version this app expects.");
  assert.equal(unknown?.more, "no install record was written");
});

test("a refresh says what happened in words, and says plainly when the model did not come back", () => {
  const failed = refreshOutcomeLabel({ files: 14, restarted: null, restartError: "exited with code 3" });
  assert.equal(failed.problem, true);
  assert.equal(failed.text, "Updated, but the model that was running could not start again. Choose it again to retry.");
  assert.match(failed.more ?? "", /14 files/);
  assert.match(failed.more ?? "", /exited with code 3/);

  const restarted = refreshOutcomeLabel({ files: 14, restarted: "kb-whisper-base", restartError: null });
  assert.equal(restarted.problem, false);
  assert.equal(restarted.text, "Updated. The model that was running has started again.");
  assert.match(restarted.more ?? "", /kb-whisper-base/);

  assert.deepEqual(refreshOutcomeLabel({ files: 14, restarted: null, restartError: null }), {
    text: "Updated.",
    problem: false,
    more: "14 files were replaced.",
  });
});

test("install progress names a model by its name, never its id", () => {
  const line = progressLabel({ stage: "model", id: "kb-whisper-base", written: 12_300_000, total: 55_300_000 }, nameOf);
  assert.equal(line, "Downloading KB Whisper Base — 12.3 MB of 55.3 MB");
  assert.equal(progressLabel({ stage: "server", written: 1_000_000, total: null }), "Downloading the speech engine — 1.0 MB");
  assert.equal(progressLabel({ stage: "verifying" }), "Checking the download…");
  assert.equal(progressLabel({ stage: "done", verifiedBy: "digest" }), "Installed and verified.");
  assert.equal(progressLabel({ stage: "done", verifiedBy: "size" }), "Installed. Only its size was checked.");
});

test("how a download was checked, in words a person reads", () => {
  assert.equal(verifiedPlain("digest"), "verified");
  assert.equal(verifiedPlain("stated-digest"), "verified");
  assert.equal(verifiedPlain("size"), "only its size was checked");
  assert.equal(verifiedPlain("nothing"), "not verified");
});

test("before an install, how the download will be checked reads as a phrase", () => {
  // It was " · will be " + verifiedPlain, which read "will be only its size
  // was checked" (branch review).
  assert.equal(downloadCheckPlain("digest"), "verified on download");
  assert.equal(downloadCheckPlain("stated-digest"), "verified on download");
  assert.equal(downloadCheckPlain("size"), "only its size is checked on download");
  assert.equal(downloadCheckPlain("nothing"), "not checked on download");
});

test("a local model's language is a word, not a code", () => {
  assert.equal(modelLanguage("en"), "English");
  assert.equal(modelLanguage("sv"), "Swedish");
  assert.equal(modelLanguage("multilingual"), "Many languages");
  // A code with no name here is shown as itself rather than dropped.
  assert.equal(modelLanguage("xx"), "xx");
});

test("a local model says what it is for, not what it runs on", () => {
  assert.equal(useForDetail("dictation"), "For dictation");
  assert.equal(useForDetail("meetings"), "For meetings, word by word");
});

test("a text model's state leads with what matters, and keeps sizes and checksums for Details", () => {
  for (const state of ["unchanged-since-install", "unrecorded"] as const) {
    const label = ggufIntegrityLabel(state === "unrecorded" ? { state } : { state, verifiedBy: "digest" });
    assert.equal(label.text, "Installed", state);
  }

  const changed = ggufIntegrityLabel({ state: "changed", recordedBytes: 5_900_000_000, bytes: 5_800_000_000 });
  assert.equal(changed.text, "Changed since it was installed. Check the file to be sure it is intact.");
  assert.doesNotMatch(changed.text, /\d/);
  assert.ok(changed.detail.includes((5_900_000_000).toLocaleString()), changed.detail);

  const failed = ggufIntegrityLabel({ state: "failed-recheck", expected: PINNED, actual: INSTALLED });
  assert.equal(failed.text, "This file doesn't match what this app expects. Reinstall the model.");
  assert.doesNotMatch(failed.text, HEX);
  assert.match(failed.detail, new RegExp(PINNED));
  assert.match(failed.detail, new RegExp(INSTALLED));
});

test("a re-check says what it found in words, with the file name and checksums behind it", () => {
  const mismatched = recheckOutcomeLabel({ outcome: "mismatched", fileName: "qwen2.5-7b-instruct-q4_k_m.gguf", expected: PINNED, actual: INSTALLED });
  assert.equal(mismatched.text, "This file doesn't match what this app expects. Reinstall the model.");
  assert.doesNotMatch(mismatched.text, /gguf|[0-9a-f]{12}/);
  assert.match(mismatched.detail, /qwen2\.5-7b-instruct-q4_k_m\.gguf/);
  assert.match(mismatched.detail, new RegExp(PINNED));

  assert.equal(recheckOutcomeLabel({ outcome: "matched", verifiedBy: "digest" }).text, "Checked: the file is as it was downloaded.");
  assert.equal(recheckOutcomeLabel({ outcome: "nothing-pinned" }).text, "There is nothing to check it against.");
});

test("text-model progress names the model and its helper by name", () => {
  const of = (written: number, total: number | null) => ({ written, total });
  assert.equal(
    llmProgressLabel({ stage: "model", id: "qwen2.5-7b-instruct", ...of(1_000_000_000, 5_900_000_000) }, nameOf),
    "Downloading Qwen2.5 7B Instruct — 1.0 GB of 5.9 GB",
  );
  assert.equal(
    llmProgressLabel({ stage: "draft", id: "qwen2.5-7b-instruct", ...of(100_000_000, null) }, nameOf),
    "Downloading Qwen2.5 7B Instruct's helper model — 100 MB",
  );
  assert.equal(llmProgressLabel({ stage: "server", ...of(5_000_000, 20_000_000) }, nameOf), "Downloading the text engine — 5 MB of 20 MB");
  assert.equal(llmProgressLabel({ stage: "verifying", id: "qwen2.5-7b-instruct" }, nameOf), "Checking Qwen2.5 7B Instruct…");
  assert.equal(
    llmProgressLabel({ stage: "done", id: "qwen2.5-7b-instruct", verifiedBy: "stated-digest" }, nameOf),
    "Qwen2.5 7B Instruct is installed and verified.",
  );
  assert.equal(
    llmProgressLabel({ stage: "done", id: "qwen2.5-7b-instruct", verifiedBy: null }, nameOf),
    "Qwen2.5 7B Instruct was already installed.",
  );
  assert.equal(
    llmProgressLabel({ stage: "stopped-when-idle" }, nameOf),
    "The model was unloaded after a while unused. The next request loads it again.",
  );
});
