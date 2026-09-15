/**
 * Tests for the model pin measurer.
 *
 * Three things are worth asserting without a network: that the argument parser
 * agrees with the documented command lines, that hashing a file on disk
 * produces the digest and the size the rest of the world would produce for the
 * same bytes, and that `pinnedTargets` finds every committed pin and no
 * phantoms.
 *
 * The last is the one that earns its keep, and the reason has not changed even
 * though the reader has: the failure mode of a loose discovery step is not a
 * crash, it is `--verify` checking zero models and printing a pass.
 *
 * **What used to be here.** Five tests covered `parseWhisperPins`, which read
 * the `PINNED` array out of `crates/transcription-core/src/models.rs` — a
 * table gone missing, a declared arity disagreeing with the rows, a digest
 * that was not 64 hex characters. They were written as a tripwire for the day
 * the whisper pins moved into the registry generator beside `SHERPA_PINS`.
 * That day was 2026-08-31; the pins are `WHISPER_PINS` now, this tool reads
 * one generated JSON document, and the tripwire went with the thing it was
 * guarding. Retiring it is the outcome it existed for.
 *
 * Run: node --test tools/model-pin/pin.test.mjs
 */

import { test } from "node:test";
import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { mkdtempSync, writeFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

import {
  args,
  githubAssetTargets,
  hfRepos,
  measureFile,
  parseGithubAssetUrl,
  parseHfUrl,
  pinnedTargets,
  registry,
  statedTargets,
} from "./pin.mjs";

/* -------------------------------------------------------------------- args */

test("args parses --flag value pairs", () => {
  assert.deepEqual(
    args(["--url", "https://example.invalid/m.bin", "--id", "tiny"]),
    { url: "https://example.invalid/m.bin", id: "tiny" }
  );
});

test("args parses a bare switch as true", () => {
  assert.deepEqual(args(["--verify"]), { verify: true });
});

test("args treats a flag followed by another flag as a switch", () => {
  // `--verify --only tiny` must not swallow `--only` as --verify's value.
  assert.deepEqual(args(["--verify", "--only", "tiny"]), { verify: true, only: "tiny" });
});

test("args keeps hyphenated names verbatim so --expect-sha256 is reachable", () => {
  const parsed = args(["--expect-sha256", "ab".repeat(32), "--expect-size", "77691713"]);
  assert.equal(parsed["expect-sha256"], "ab".repeat(32));
  assert.equal(parsed["expect-size"], "77691713");
});

test("args ignores positional tokens", () => {
  assert.deepEqual(args(["measure", "--file", "m.bin", "trailing"]), { file: "m.bin" });
});

test("args lets a later flag win, rather than silently keeping the first", () => {
  assert.deepEqual(args(["--id", "a", "--id", "b"]), { id: "b" });
});

test("args accepts a URL that itself starts with no dashes but has query separators", () => {
  const url = "https://example.invalid/a.bin?download=true&x=1";
  assert.deepEqual(args(["--url", url]), { url });
});

/* --------------------------------------------------------------- measuring */

test("measureFile reproduces a known digest and the exact byte size", async () => {
  const dir = mkdtempSync(join(tmpdir(), "model-pin-test-"));
  try {
    const path = join(dir, "fixture.bin");
    // Not text: a model is binary, and a hash of bytes must not go through any
    // encoding on the way in.
    const bytes = Buffer.alloc(200_003);
    for (let i = 0; i < bytes.length; i += 1) bytes[i] = (i * 31 + 7) % 256;
    writeFileSync(path, bytes);

    const expected = createHash("sha256").update(bytes).digest("hex");
    const measured = await measureFile(path, "fixture");

    assert.equal(measured.sha256, expected);
    assert.equal(measured.sizeBytes, 200_003);
    assert.equal(measured.resolvedFrom, null);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("measureFile hashes an empty file rather than skipping it", async () => {
  const dir = mkdtempSync(join(tmpdir(), "model-pin-test-"));
  try {
    const path = join(dir, "empty.bin");
    writeFileSync(path, Buffer.alloc(0));
    const measured = await measureFile(path, "empty");
    // The digest of nothing, which is a real answer and not an error.
    assert.equal(
      measured.sha256,
      "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
    assert.equal(measured.sizeBytes, 0);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("measureFile streams a file larger than one chunk without changing the answer", async () => {
  const dir = mkdtempSync(join(tmpdir(), "model-pin-test-"));
  try {
    // Larger than the 64 KiB default highWaterMark, so the loop runs many
    // times. A hash that is only ever fed one chunk proves nothing about the
    // path a real model takes.
    const path = join(dir, "multi.bin");
    const bytes = Buffer.alloc(1_000_000, 0xab);
    writeFileSync(path, bytes);
    const measured = await measureFile(path, "multi");
    assert.equal(measured.sha256, createHash("sha256").update(bytes).digest("hex"));
    assert.equal(measured.sizeBytes, 1_000_000);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

/* --------------------------------------------------------- pinned targets */

/** A registry shaped like the real one, with both families and both states. */
function fixtureRegistry() {
  return {
    localAsr: {
      whisper: [
        {
          id: "tiny",
          downloadUrl: "https://example.invalid/ggml-tiny.bin",
          sha256: "a".repeat(64),
          // The measured length. `sizeMb` beside it is the rounded display
          // figure, and a target must never be built from that one.
          sizeMb: 75,
          expectedSizeBytes: 77_691_713,
        },
      ],
      sherpa: [
        {
          id: "pinned-model",
          downloadUrl: "https://example.invalid/pinned.tar.bz2",
          sha256: "c".repeat(64),
          expectedSizeBytes: 487_170_055,
        },
        {
          id: "unpinned-model",
          downloadUrl: "https://example.invalid/unpinned.tar.bz2",
          sha256: null,
          expectedSizeBytes: null,
        },
      ],
    },
  };
}

test("pinnedTargets reads both families out of the registry", () => {
  const targets = pinnedTargets(fixtureRegistry());
  assert.deepEqual(
    targets.map((t) => `${t.kind}/${t.id}`),
    ["whisper/tiny", "sherpa/pinned-model"]
  );
  assert.equal(targets[0].url, "https://example.invalid/ggml-tiny.bin");
  assert.equal(targets[0].sizeBytes, 77_691_713);
  assert.equal(targets[1].sizeBytes, 487_170_055);
});

test("pinnedTargets skips a model with no committed digest", () => {
  // The two Nemotron streaming models are in the registry and pinned nowhere.
  // Reporting them as unverifiable would be noise; there is nothing committed
  // to disagree with.
  const targets = pinnedTargets(fixtureRegistry());
  assert.ok(!targets.some((t) => t.id === "unpinned-model"));
});

test("pinnedTargets lowercases a digest so the comparison cannot miss", () => {
  const reg = fixtureRegistry();
  reg.localAsr.whisper[0].sha256 = "A".repeat(64);
  assert.equal(pinnedTargets(reg)[0].sha256, "a".repeat(64));
});

test("pinnedTargets survives a registry with no localAsr at all", () => {
  assert.deepEqual(pinnedTargets({}), []);
});

test("the committed registry still carries pins for all eight whisper models", () => {
  // Not a unit test of anything — an assertion that the real file this tool
  // reads still has something in it, so `--verify` would have something to
  // measure. A pin dropped from WHISPER_PINS would otherwise make `--verify`
  // quietly check five models and pass.
  //
  // The sherpa count went 2 -> 3 -> 4 on 2026-09-01: first
  // `nemotron-speech-streaming-en-0.6b` for KNEG-84, then
  // `nemotron-3.5-asr-streaming-0.6b` for KNEG-118 the same day. Raised each
  // time rather than loosened to `>= 2`: a count is the only thing here that
  // notices a pin going *missing*, and a lower bound would notice nothing.
  //
  // Four is now every sherpa model the registry declares, so the next change to
  // this number is either a new model or a regression.
  const targets = pinnedTargets();
  const whisper = targets.filter((t) => t.kind === "whisper");
  const sherpa = targets.filter((t) => t.kind === "sherpa");
  assert.deepEqual(
    whisper.map((t) => t.id).sort(),
    ["base", "kb-whisper-base", "kb-whisper-tiny", "large", "medium", "small", "tiny", "turbo"],
    "every standard and KB Whisper model is pinned exactly once"
  );
  for (const id of ["kb-whisper-base", "kb-whisper-tiny"]) {
    const pin = whisper.find((t) => t.id === id);
    const source = parseHfUrl(pin.url);
    assert.equal(source.repo, `KBLab/${id}`);
    assert.equal(source.path, "ggml-model-q5_0.bin");
    assert.match(source.revision, /^[0-9a-f]{40}$/, `${id} is not pinned to a commit`);
  }
  assert.equal(sherpa.length, 4, "both Parakeet archives and both Nemotron archives are pinned");
  for (const t of targets) {
    assert.match(t.sha256, /^[0-9a-f]{64}$/, `${t.id} has no usable digest`);
    assert.ok(t.url?.startsWith("https://"), `${t.id} has no https URL`);
    assert.ok(Number.isInteger(t.sizeBytes) && t.sizeBytes > 0, `${t.id} has no measured size`);
    // A measured length, never a rounded one. Every figure the baseline
    // declared was a round million; no real file is.
    assert.notEqual(t.sizeBytes % 1_000_000, 0, `${t.id}'s size looks rounded`);
  }
});

/* --------------------------------------------------------- stated targets */

/** A `localLlm` shaped like the real one: a drafter, and both digest standards. */
function fixtureLocalLlm() {
  return {
    localLlm: [
      {
        id: "gemma",
        models: [
          {
            id: "gemma-with-a-drafter",
            hfRepo: "unsloth/gemma-4-E2B-it-qat-GGUF",
            revision: "a".repeat(40),
            fileName: "gemma.gguf",
            sizeBytes: 3_349_516_256,
            sizeMeasured: true,
            sha256: "d".repeat(64),
            sha256Measured: true,
            draft: {
              hfRepo: "unsloth/gemma-4-E2B-it-qat-GGUF",
              revision: "a".repeat(40),
              fileName: "mtp-gemma.gguf",
              sizeBytes: 59_235_648,
              sizeMeasured: true,
              sha256: "e".repeat(64),
              sha256Measured: false,
            },
          },
          {
            id: "stated-only",
            hfRepo: "bartowski/whatever-GGUF",
            revision: "b".repeat(40),
            fileName: "stated.gguf",
            sizeBytes: 806_058_496,
            sizeMeasured: true,
            sha256: "f".repeat(64),
            sha256Measured: false,
            draft: null,
          },
        ],
      },
    ],
  };
}

test("statedTargets reads models and their drafters as separate objects", () => {
  const targets = statedTargets(fixtureLocalLlm());
  assert.deepEqual(
    targets.map((t) => t.id),
    ["gemma-with-a-drafter", "gemma-with-a-drafter (drafter)", "stated-only"]
  );
  assert.equal(targets[1].file, "mtp-gemma.gguf");
  assert.equal(targets[1].sizeBytes, 59_235_648);
});

test("statedTargets keeps which standard each pin already met", () => {
  // The distinction this whole mode has to preserve: agreement with a stated
  // oid means "upstream has not re-uploaded", and for the seven measured pins
  // that is weaker than how they were pinned. A caller that cannot tell them
  // apart cannot say so.
  const targets = statedTargets(fixtureLocalLlm());
  assert.equal(targets[0].measured, true);
  assert.equal(targets[1].measured, false);
  assert.equal(targets[2].measured, false);
});

test("statedTargets carries the commit each object is pinned to", () => {
  // Without it verifyStated would read the tree at `main`, which since
  // KNEG-103 is not where the installer fetches from -- it would answer
  // confidently about different bytes.
  const targets = statedTargets(fixtureLocalLlm());
  assert.equal(targets[0].revision, "a".repeat(40));
  assert.equal(targets[1].revision, "a".repeat(40), "a drafter carries its own");
  assert.equal(targets[2].revision, "b".repeat(40));
});

test("statedTargets ignores the whisper and sherpa pins entirely", () => {
  // Those are bytes this project hashed. Re-reading a publisher's claim is not
  // how they are checked, and including them would make a weaker check look
  // like it covered them.
  assert.deepEqual(statedTargets(fixtureRegistry()), []);
});

test("statedTargets survives a registry with no localLlm at all", () => {
  assert.deepEqual(statedTargets({}), []);
});

test("statedTargets lowercases a digest so the comparison cannot miss", () => {
  const reg = fixtureLocalLlm();
  reg.localLlm[0].models[0].sha256 = "D".repeat(64);
  assert.equal(statedTargets(reg)[0].sha256, "d".repeat(64));
});

test("the committed registry still carries a digest for all 39 GGUF objects", () => {
  // The same argument as the whisper assertion above: a pin dropped from
  // LOCAL_LLM_PINS would otherwise make --verify-stated quietly check 38
  // objects and pass. 35 models plus 4 drafters.
  const targets = statedTargets();
  assert.equal(targets.length, 39, "every model and drafter is pinned");
  for (const target of targets) {
    assert.match(target.sha256, /^[0-9a-f]{64}$/, `${target.id} has no usable digest`);
    assert.ok(
      Number.isInteger(target.sizeBytes) && target.sizeBytes > 0,
      `${target.id} has no measured size`
    );
    // A measured length, never a rounded one -- every figure upstream declared
    // was a round million, and no real file is.
    assert.notEqual(target.sizeBytes % 1_000_000, 0, `${target.id}'s size looks rounded`);
    assert.ok(target.repo?.includes("/"), `${target.id} has no owner/name repo`);
    assert.ok(target.file?.endsWith(".gguf"), `${target.id} does not name a .gguf`);
    // A branch name here would make --verify-stated read a tree the installer
    // never fetches from, and report a pass about it.
    assert.match(target.revision, /^[0-9a-f]{40}$/, `${target.id} is not pinned to a commit`);
  }
});

/* ------------------------------------------------------------- revisions */

test("parseHfUrl splits a Hugging Face resolve URL into its three parts", () => {
  assert.deepEqual(
    parseHfUrl("https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-tiny.bin"),
    { repo: "ggerganov/whisper.cpp", revision: "main", path: "ggml-tiny.bin" }
  );
});

test("parseHfUrl reads a commit sha as the revision, not just a branch name", () => {
  const sha = "5359861c739e955e79d9a303bcbc70fb988958b1";
  const parsed = parseHfUrl(
    `https://huggingface.co/ggerganov/whisper.cpp/resolve/${sha}/ggml-base.bin`
  );
  assert.equal(parsed.revision, sha);
  assert.equal(parsed.path, "ggml-base.bin");
});

test("parseHfUrl keeps a path with directories in it whole", () => {
  // Several publishers put a multi-part quantisation in a subdirectory, and a
  // parser that stopped at the first slash would name the wrong object.
  const parsed = parseHfUrl(
    "https://huggingface.co/unsloth/gemma-4-31B-it-qat-GGUF/resolve/main/Q4_0/part-1.gguf"
  );
  assert.equal(parsed.path, "Q4_0/part-1.gguf");
});

test("parseHfUrl returns null for a URL hosted somewhere else", () => {
  // A caller has to handle this rather than assume it away. Nothing in the
  // registry is hosted off Hugging Face today, and that is a fact about today.
  assert.equal(parseHfUrl("https://github.com/k2-fsa/sherpa-onnx/releases/x.tar.bz2"), null);
  assert.equal(parseHfUrl("https://huggingface.co/some-repo/blob/main/x.bin"), null);
});

test("hfRepos finds both families and counts a shared repository once", () => {
  const repos = hfRepos({
    localAsr: {
      whisper: [
        { id: "tiny", downloadUrl: "https://huggingface.co/one/two/resolve/main/a.bin" },
        { id: "base", downloadUrl: "https://huggingface.co/one/two/resolve/main/b.bin" },
      ],
    },
    localLlm: [
      {
        models: [
          { id: "m", hfRepo: "three/four", draft: { hfRepo: "five/six" } },
          { id: "n", hfRepo: "three/four" },
        ],
      },
    ],
  });
  assert.deepEqual(
    repos.map((r) => r.repo),
    ["five/six", "one/two", "three/four"]
  );
  // Who would stop installing if that repository could not be read. A
  // repository that names only itself understates what a failure costs.
  assert.deepEqual(repos[1].from, ["whisper/tiny", "whisper/base"]);
  assert.deepEqual(repos[2].from, ["localLlm/m", "localLlm/n"]);
});

test("hfRepos skips a whisper model hosted off Hugging Face rather than guessing", () => {
  const repos = hfRepos({
    localAsr: { whisper: [{ id: "x", downloadUrl: "https://example.invalid/x.bin" }] },
  });
  assert.deepEqual(repos, []);
});

test("hfRepos reads every repository the committed registry downloads from", () => {
  // 36 GGUF repositories plus ggerganov/whisper.cpp and two KB repositories.
  // The count is asserted for
  // the reason the 39-object count is: a repository silently dropping out of
  // the registry would make --revisions read fewer and still report success.
  const repos = hfRepos();
  assert.equal(repos.length, 39);
  assert.ok(repos.some((r) => r.repo === "ggerganov/whisper.cpp"));
  assert.deepEqual(
    repos.filter((r) => r.repo.startsWith("KBLab/")),
    [
      { repo: "KBLab/kb-whisper-base", from: ["whisper/kb-whisper-base"] },
      { repo: "KBLab/kb-whisper-tiny", from: ["whisper/kb-whisper-tiny"] },
    ]
  );
  for (const { repo, from } of repos) {
    assert.match(repo, /^[^/]+\/[^/]+$/, `${repo} is not an owner/name pair`);
    assert.ok(from.length > 0, `${repo} is named by nothing`);
  }
});

/* -------------------------------------------- github release assets */

test("parseGithubAssetUrl splits a release-asset URL into its three parts", () => {
  assert.deepEqual(
    parseGithubAssetUrl(
      "https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/a-model.tar.bz2"
    ),
    { repo: "k2-fsa/sherpa-onnx", tag: "asr-models", name: "a-model.tar.bz2" }
  );
});

test("parseGithubAssetUrl returns null for anything that is not one", () => {
  // A caller has to handle this rather than assume it away -- `githubAssetTargets`
  // dies on it, because a pinned model whose host this cannot read would
  // otherwise be skipped and reported as covered.
  assert.equal(parseGithubAssetUrl("https://huggingface.co/a/b/resolve/main/c.bin"), null);
  assert.equal(parseGithubAssetUrl("https://github.com/k2-fsa/sherpa-onnx/releases"), null);
});

test("githubAssetTargets reads the committed sherpa pins", () => {
  const targets = githubAssetTargets({
    localAsr: {
      sherpa: [
        {
          id: "pinned",
          downloadUrl: "https://github.com/o/r/releases/download/tag/x.tar.bz2",
          sha256: "A".repeat(64),
          expectedSizeBytes: 123,
        },
        { id: "unpinned", downloadUrl: "https://github.com/o/r/releases/download/tag/y.tar.bz2" },
      ],
    },
  });
  assert.equal(targets.length, 1, "a model with no digest has nothing to re-read");
  assert.equal(targets[0].id, "pinned");
  assert.equal(targets[0].repo, "o/r");
  assert.equal(targets[0].tag, "tag");
  assert.equal(targets[0].name, "x.tar.bz2");
  assert.equal(targets[0].sha256, "a".repeat(64), "lowercased so the comparison cannot miss");
});

test("githubAssetTargets survives a registry with no models at all", () => {
  assert.deepEqual(githubAssetTargets({}), []);
});

test("the committed registry still carries every k2-fsa asset as a GitHub asset", () => {
  // The same argument as the 39-object and six-whisper counts: a pin dropped
  // from its table would otherwise make --verify-stated check one asset, or
  // none, and report success.
  //
  // By id rather than by count, since 2026-09-01. It asserted `length === 4`
  // and there are six; bumping the literal would have kept it green and
  // stopped it saying anything, because a count cannot tell a model that was
  // replaced from one that is still there. The four sherpa ASR archives were
  // measured earlier that day (KNEG-84 needed a streaming model to measure the
  // protocol against, KNEG-118 because somebody tried to install the other
  // one); the two diarization models were measured the same day for KNEG-73a.
  const targets = githubAssetTargets();
  assert.deepEqual(
    targets.map((t) => t.id).sort(),
    [
      "3dspeaker-campplus-en",
      "nemotron-3.5-asr-streaming-0.6b",
      "nemotron-speech-streaming-en-0.6b",
      "parakeet-tdt-0.6b-v3",
      "parakeet-unified-en-0.6b",
      "sherpa-onnx-pyannote-segmentation-3-0",
    ],
    "every pinned k2-fsa model is re-readable from its release"
  );
  for (const target of targets) {
    assert.match(target.sha256, /^[0-9a-f]{64}$/, `${target.id} has no usable digest`);
    assert.ok(target.sizeBytes > 0, `${target.id} has no measured size`);
    assert.equal(target.repo, "k2-fsa/sherpa-onnx");
  }

  // Not every asset is an archive any more, and that is a real difference
  // rather than an exception: the speaker embedding model is a single `.onnx`
  // that nothing extracts, which is why it carries no extraction ceiling.
  const embedding = targets.find((t) => t.id === "3dspeaker-campplus-en");
  assert.ok(embedding.name.endsWith(".onnx"), embedding.name);
  for (const target of targets.filter((t) => t.id !== "3dspeaker-campplus-en")) {
    assert.ok(target.name.endsWith(".tar.bz2"), `${target.id} does not name an archive`);
  }
});

test("the two diarization models are one segmentation and one embedding", () => {
  // A run passes one of each, so two of the same kind is a registry that
  // describes an install which cannot work. The generator refuses to emit
  // that; this is the check that the committed file is what the generator
  // would emit.
  const { localAsr } = registry();
  assert.deepEqual(
    localAsr.diarization.map((m) => m.role).sort(),
    ["embedding", "segmentation"]
  );
  for (const model of localAsr.diarization) {
    assert.match(model.sha256, /^[0-9a-f]{64}$/, `${model.id} is unpinned`);
    assert.ok(model.expectedSizeBytes > 0, `${model.id} has no measured size`);
  }

  // Only the archive carries an extraction ceiling. A ceiling on the bare
  // `.onnx` would describe a step that does not happen to it, and a missing
  // one on the archive would be the decompression-bomb hole KNEG-100 closed.
  const byRole = Object.fromEntries(localAsr.diarization.map((m) => [m.role, m]));
  assert.ok(byRole.segmentation.maxExtractedSizeBytes > 0);
  assert.equal(byRole.embedding.maxExtractedSizeBytes, null);
  assert.equal(byRole.embedding.extractDir, null);
});
