#!/usr/bin/env node
/**
 * Measures a model's SHA-256 and exact byte size, rather than inventing one.
 *
 * `crates/transcription-core/src/models.rs` used to say, of the six whisper
 * models:
 *
 *   > Pinning all of them means downloading roughly seven gigabytes once,
 *   > which is a job for a tool rather than something to invent digests for
 *   > here: `tiny` is pinned because it was actually downloaded and hashed
 *   > while this was written, and the rest say plainly that they were not.
 *
 * This is that tool, and on 2026-08-31 it did that: all six downloaded and
 * hashed in one run, `tiny` reproducing the digest `models.rs` had carried
 * since it was written. It exists so the difference between a measured digest
 * and a copied one stays visible: every row it prints was produced by bytes
 * moving through this process, and nothing else it prints claims to have been.
 *
 * ## Why not tools/sidecar-pin
 *
 * `sidecar-pin` measures the same two numbers but cannot measure a model. It
 * builds its URL from `--repo/--release/--asset`, so it can only reach a
 * GitHub release; and once the bytes are down it demands exactly one
 * executable inside the archive, dying on zero and on two. A model is neither
 * of those things. It lives on Hugging Face, it is not an archive, and it
 * contains no executable — it is data, and this tool hashes a file.
 *
 * The parts worth copying were copied: no dependencies, streaming, manual
 * redirects, a temp file removed in a `finally`, progress on stderr and
 * pasteable JSON on stdout.
 *
 * ## The size comes off the disk
 *
 * `sizeBytes` is `statSync(target).size` after the stream has closed, never
 * the `content-length` header, for the reason `sidecar-pin` gives: **a
 * truncated body would agree with the header and disagree with itself.** That
 * is also the whole reason this tool downloads rather than sending a HEAD
 * request. A HEAD would return both numbers in a millisecond and neither of
 * them would be evidence: the digest would be absent and the size would be a
 * claim by the server about bytes nobody counted. Everything this tool reports
 * is measured downstream of the hash, from the file that was actually written.
 *
 * ## Size
 *
 * Nothing is ever held in memory — chunks are hashed as they pass through to
 * disk. That is not a nicety here. The largest asset in scope is the Qwen3 32B
 * Q4_K_M GGUF in `config/provider-registry.json`'s `localLlm`, at
 * 19,762,149,024 bytes (about 18.4 GiB — the figure here used to be
 * 21,260,251,955, which was upstream's declaration and wrong by 2^30 ÷ 10^9;
 * see `98e836b`); the largest whisper model is `ggml-large-v3.bin` at
 * 3,095,033,483 bytes, and the pinned sherpa archives are about 500 MB each.
 * Peak memory is one chunk regardless.
 *
 * A download that dies at 18 GiB is not resumed: a partially-fetched file
 * cannot be hashed into a whole-file digest without trusting the part already
 * on disk, and this tool's only job is to not trust things. `--file` is the
 * escape hatch — if the bytes are already on the machine, measure those and
 * skip the network entirely.
 *
 * A dropped connection is retried instead, twice, from byte 0 (KNEG-413,
 * `retry.mjs`). Only the transport is retried: bytes that arrive whole and
 * disagree with a pin are a disagreement, never a reason to ask again.
 *
 * ## Usage
 *
 *   node tools/model-pin/pin.mjs --url <url> [--id <model-id>] [--keep <path>]
 *   node tools/model-pin/pin.mjs --file <path> [--id <model-id>]
 *   node tools/model-pin/pin.mjs --verify [--only <model-id>]
 *   node tools/model-pin/pin.mjs --verify-stated [--only <model-id>]
 *   node tools/model-pin/pin.mjs --revisions
 *
 *   # Measure and assert in one step — this is the acceptance run:
 *   node tools/model-pin/pin.mjs \
 *     --url https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-tiny.bin \
 *     --id tiny \
 *     --expect-sha256 be07e048e1e599ad46341c8d2a135645097a538221678b7acdd1b1919c6e1b21 \
 *     --expect-size 77691713
 *
 * ## What `--verify` reads
 *
 * **One file.** `config/provider-registry.json` → `localAsr.whisper[]` and
 * `localAsr.sherpa[]`. It is generated, so the URL, `sha256` and
 * `expectedSizeBytes` are read as data, with no parsing and no second copy.
 * Their source is `WHISPER_PINS` and `SHERPA_PINS` in
 * `tools/integration-audit/generate-provider-registry.mjs`; a model with a
 * null `sha256` is skipped, because there is nothing committed to re-measure.
 *
 * ## What `--verify-stated` reads, and why it is a different verb
 *
 * The same file, `localLlm[].models[]` and their drafters -- the 39 GGUF
 * objects. It does **not** download them: 202 GB is why they were never
 * digest-pinned in the first place. It reads Hugging Face's
 * `/api/models/{repo}/tree/{revision}?expand=true`, which returns every
 * object's `lfs.oid` and `lfs.size` in one response per repository, and
 * compares those against what is committed.
 *
 * **The revision is the committed one, not `main`, and that changed what this
 * mode is for.** Until KNEG-103 it asked "has the publisher re-uploaded", and
 * a yes was an emergency: the bytes behind our URL had moved out from under
 * the digest. Now the URL names a commit, so a re-upload cannot move them, and
 * the question is a better one -- **do the digest and the revision committed
 * beside each other describe the same object?** Nothing else in the repository
 * checks that pair, and nothing else would notice them drifting apart.
 *
 * It also reads `/api/models/{repo}` for each repository and says where `main`
 * has moved past the pinned commit. That is reported and never failed on:
 * upstream publishing is normal, and a weekly job that exited non-zero for it
 * would be ignored by the second month. It is news about pins worth revisiting,
 * not a defect.
 *
 * **And the six k2-fsa assets, which are not on Hugging Face at all.** The
 * four sherpa ASR archives are in k2-fsa's rolling `asr-models` GitHub
 * release, and the two diarization models in `speaker-segmentation-models`
 * and `speaker-recongition-models` (k2-fsa's spelling). GitHub's release API
 * states a `size` per asset and, for assets uploaded from about mid-2025, a
 * `digest` -- the same shape as `lfs.oid`, from a different host, one request
 * per release. All six are *also* downloaded and re-hashed by `--verify`, so
 * this is not how they are checked; it is an earlier and far cheaper signal
 * that tells a swapped asset from a damaged download before the re-download
 * runs. Measured 2026-08-31: the two Parakeet archives' stated digests and
 * sizes match the committed pins exactly (KNEG-112).
 *
 * **The two diarization models have no stated digest, and never will.** Both
 * were uploaded in October 2024, before GitHub computed digests: 0 of 7
 * assets in `speaker-segmentation-models` have one, and nothing uploaded to
 * `speaker-recongition-models` before 2025-08-14 does (measured 2026-09-29).
 * For them the size is compared and the missing digest is printed as a note,
 * not a failure. From 2026-09-07 to 2026-09-29 it was a failure, which turned
 * the weekly job red for a reason that could not clear, and a job that is
 * always red is not read (KNEG-358). A replaced asset would be a new upload,
 * and new uploads carry a digest, so the note cannot hide a swap -- a swap
 * would arrive with a digest that disagrees.
 *
 * `--verify` re-measures bytes. `--verify-stated` re-reads a claim. Both catch
 * a silent upstream re-upload, which is the failure that matters most here;
 * only the first can catch bytes that disagree with the claim. They are two
 * commands rather than one flag because a tool that reports "verified" should
 * never be vague about which of those it did.
 *
 * ## What `--revisions` measures
 *
 * Which commit each Hugging Face repository's `main` points at right now, for
 * all 37 the registry names -- one request each, no bodies. It prints a
 * pasteable `HF_REVISIONS` table, and it exists for the same reason every
 * other mode here does: **a revision that was typed rather than read is not a
 * pin.**
 *
 * This is a measuring verb, not a checking one. It reports what upstream says
 * today and never compares that to what is committed; `--verify-stated` is
 * where a disagreement becomes an exit code. Keeping the two apart is the same
 * separation `--verify` and `--verify-stated` already have, and for the same
 * reason: a run that both measures and asserts can always be misread as having
 * asserted the thing it measured.
 *
 * This used to be two files. The whisper pins lived in a `PINNED` table in
 * `crates/transcription-core/src/models.rs`, so this tool parsed a Rust
 * literal — narrowly, with the array's declared arity as a checksum, and
 * dying rather than returning a short list, because a verifier that silently
 * checks nothing reports success. The header here proposed moving them into
 * the generator; that is what happened, and `parseWhisperPins` and its five
 * tests went with it. They were a tripwire for exactly this change and they
 * fired once, which is all a tripwire owes anybody.
 */

import { createHash } from "node:crypto";
import { createWriteStream, readFileSync, createReadStream, renameSync, rmSync, statSync } from "node:fs";
import { mkdir } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { pipeline } from "node:stream/promises";

import { HttpError, describe, discard, removePartial, retryLine, retrying } from "./retry.mjs";

const REPO_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..", "..");
const REGISTRY = join(REPO_ROOT, "config/provider-registry.json");

export function die(message) {
  console.error(`model-pin: ${message}`);
  process.exit(1);
}

/* -------------------------------------------------------------------- args */

/**
 * Parse `--flag value` pairs and bare `--flag` switches.
 *
 * Same shape as `sidecar-pin`'s, deliberately: two pin tools that disagree
 * about how a command line works is a papercut nobody needs. Exported so the
 * tests can reach it — the CLI below only runs when this file is the entry
 * point.
 */
export function args(argv) {
  const out = {};
  for (let i = 0; i < argv.length; i += 1) {
    const token = argv[i];
    if (!token.startsWith("--")) continue;
    const name = token.slice(2);
    const next = argv[i + 1];
    if (next === undefined || next.startsWith("--")) {
      out[name] = true;
    } else {
      out[name] = next;
      i += 1;
    }
  }
  return out;
}

/* ---------------------------------------------------------------- progress */

/** Human-readable bytes, for the stderr channel only. Never for a pin. */
function human(bytes) {
  if (bytes >= 1024 ** 3) return `${(bytes / 1024 ** 3).toFixed(2)} GiB`;
  if (bytes >= 1024 ** 2) return `${(bytes / 1024 ** 2).toFixed(1)} MiB`;
  if (bytes >= 1024) return `${(bytes / 1024).toFixed(1)} KiB`;
  return `${bytes} B`;
}

/**
 * Periodic progress on stderr, so stdout stays a clean JSON document.
 *
 * Throttled on both time and share: a 20 GiB download is millions of chunks,
 * and a CI log with millions of lines in it is not a log. Five seconds keeps
 * a slow link talking; one percent keeps a fast one from printing the same
 * number forty times. Together they cap a download of any size at about a
 * hundred lines. Plain lines rather than a carriage-return spinner, because
 * this runs unattended more often than not.
 */
function progress(label, totalBytes) {
  const started = Date.now();
  let seen = 0;
  let lastPrint = started;
  let lastShare = 0;

  return {
    tick(length) {
      seen += length;
      const now = Date.now();
      if (now - lastPrint < 5000) return;
      const share = totalBytes ? (seen / totalBytes) * 100 : null;
      if (share !== null && share - lastShare < 1) return;
      lastPrint = now;
      if (share !== null) lastShare = share;
      const rate = seen / Math.max(1, (now - started) / 1000);
      const of = share === null ? "" : ` of ${human(totalBytes)} (${share.toFixed(1)}%)`;
      console.error(`  ${label}: ${human(seen)}${of} at ${human(rate)}/s`);
    },
    done() {
      const seconds = Math.max(0.001, (Date.now() - started) / 1000);
      console.error(`  ${label}: ${human(seen)} in ${seconds.toFixed(1)}s`);
    },
  };
}

/* -------------------------------------------------------------- measuring */

/**
 * Hash a file already on disk, streaming it past a hash.
 *
 * The `--file` path, and the only path that involves no network. Reports the
 * same two numbers from the same two places as the download path does: the
 * digest from the bytes that went through the hash, the size from `stat`.
 */
export async function measureFile(path, label = "file") {
  let stat;
  try {
    stat = statSync(path);
  } catch (error) {
    return die(`cannot read ${path}: ${error.message}`);
  }
  if (!stat.isFile()) die(`${path} is not a file`);

  const hash = createHash("sha256");
  const reporter = progress(label, stat.size);
  // Iterated rather than piped: there is no destination here, so the chunks
  // are consumed for the hash and dropped. Still one chunk in memory at a
  // time, which is the property that matters at 18 GiB.
  for await (const chunk of createReadStream(path)) {
    hash.update(chunk);
    reporter.tick(chunk.length);
  }
  reporter.done();

  return {
    path,
    sha256: hash.digest("hex"),
    // From `stat`, for the same reason the download path does not read
    // `content-length`: the number reported is the number of bytes that were
    // there, not a number something else said would be there.
    sizeBytes: statSync(path).size,
    resolvedFrom: null,
  };
}

/**
 * Download `url`, streaming to disk, and return its digest and observed size.
 *
 * Redirects are followed explicitly and counted. Hugging Face always redirects
 * a `resolve/` URL to a CDN host, so refusing redirects outright would make
 * this tool unable to do its job — but a redirect chain that does not
 * terminate is how a download ends up somewhere unintended, so it is bounded.
 *
 * Note what a `resolve/main` URL is: a pointer at a branch, not at content.
 * The bytes behind it can change without the URL changing, which is precisely
 * the case `--verify` exists to notice.
 *
 * **A dropped connection is retried, twice** (KNEG-413, `retry.mjs`), each
 * time from byte 0 and from `url` rather than the signed URL it redirected to.
 * Anything else throws, and nothing is left behind: a failed attempt's partial
 * file is removed, and `keepAt` is written as `<keepAt>.partial` and renamed
 * only once whole, so a path that exists after this returns holds every byte
 * that was hashed. `options` replaces the retry's delays and sleep, for tests.
 */
export async function download(url, label, keepAt = null, options = {}) {
  const target = keepAt
    ? `${keepAt}.partial`
    : join(tmpdir(), `model-pin-${process.pid}-${label.replace(/[^\w.-]/g, "_")}`);
  if (keepAt) await mkdir(dirname(keepAt), { recursive: true });

  const measured = await retrying(() => downloadOnce(url, label, target), {
    ...options,
    onRetry: (retry) => console.error(`  ${retryLine(url, retry, { fromZero: true })}`),
  });
  if (!keepAt) return measured;
  renameSync(target, keepAt);
  return { ...measured, path: keepAt };
}

/** One attempt at `download`. Throws on any failure, having removed what it wrote. */
async function downloadOnce(url, label, target) {
  let current = url;
  let out = null;

  try {
    for (let hop = 0; hop < 6; hop += 1) {
      const response = await fetch(current, { redirect: "manual" });

      if (response.status >= 300 && response.status < 400) {
        const next = response.headers.get("location");
        await discard(response);
        if (!next) throw new Error(`${current} redirected with no Location header`);
        current = new URL(next, current).toString();
        continue;
      }

      if (!response.ok) {
        await discard(response);
        throw new HttpError(response.status, current);
      }

      // Read only to size the progress bar. It is never reported as the pin.
      const declared = Number(response.headers.get("content-length")) || null;

      const hash = createHash("sha256");
      const reporter = progress(label, declared);
      out = createWriteStream(target);
      await pipeline(
        response.body,
        async function* (chunks) {
          for await (const chunk of chunks) {
            hash.update(chunk);
            reporter.tick(chunk.length);
            yield chunk;
          }
        },
        out
      );
      reporter.done();

      const sizeBytes = statSync(target).size;
      // The size on disk, not `content-length`: a truncated body would agree
      // with the header and disagree with itself. When the two differ, say so —
      // the pin is still the measured number, but a mismatch is worth a line in
      // the log rather than a silent shrug.
      if (declared !== null && declared !== sizeBytes) {
        console.error(
          `  warning: content-length said ${declared} and ${sizeBytes} bytes arrived. ` +
            "The pin below is the measured size."
        );
      }

      return {
        path: target,
        sha256: hash.digest("hex"),
        sizeBytes,
        resolvedFrom: current === url ? null : current,
      };
    }

    throw new Error(`${url} redirected more than five times`);
  } catch (error) {
    await removePartial(out, target);
    throw error;
  }
}

/* ---------------------------------------------------------- pin discovery */

export function registry() {
  try {
    return JSON.parse(readFileSync(REGISTRY, "utf8"));
  } catch (error) {
    return die(`cannot read ${REGISTRY}: ${error.message}`);
  }
}

/**
 * Every committed model pin, as one list of things to re-measure.
 *
 * All three families come out of the generated registry now, read as data. A
 * model with no `sha256` is skipped rather than reported, and **as of
 * 2026-09-01 nothing is skipped**: this used to say the exceptions were
 * `localAsr.sherpa`'s two Nemotron streaming models, which nothing could
 * install and nobody had pinned. Both were measured that day, and the two
 * diarization models were measured and added beside them, so every model this
 * registry describes is now pinned and re-measurable. The generator writes an
 * explicit `null` for an unpinned model rather than omitting the key, which is
 * what makes this a data check and not a guess.
 *
 * A pin with a digest and no `expectedSizeBytes` is a `die()`, not a skip. The
 * generator cannot emit that pair — both `WHISPER_PINS` and `SHERPA_PINS`
 * carry the two numbers together — so seeing it here means the registry was
 * hand-edited, and half a pin is the one input that would let this tool report
 * a pass over a length nobody checked.
 */
export function pinnedTargets(reg = registry()) {
  const targets = [];

  for (const [kind, models] of [
    ["whisper", reg.localAsr?.whisper ?? []],
    ["sherpa", reg.localAsr?.sherpa ?? []],
    ["diarization", reg.localAsr?.diarization ?? []],
  ]) {
    for (const model of models) {
      if (!model.sha256) continue;
      if (typeof model.expectedSizeBytes !== "number") {
        die(
          `${kind}/${model.id} in the registry has a sha256 and no expectedSizeBytes. The ` +
            "generator emits both or neither, so config/provider-registry.json has been " +
            "hand-edited -- fix the pin in generate-provider-registry.mjs and regenerate."
        );
      }
      targets.push({
        kind,
        id: model.id,
        url: model.downloadUrl,
        sha256: model.sha256.toLowerCase(),
        // The exact measured length, not a rounded display figure. `sizeMb` is
        // the rounded one and this tool never reads it.
        sizeBytes: model.expectedSizeBytes,
        from: `config/provider-registry.json localAsr.${kind}`,
      });
    }
  }

  return targets;
}

/* ------------------------------------------------------------------ verify */

/**
 * Re-measure every committed pin and report disagreements.
 *
 * Same contract as `sidecar-pin --verify`: non-zero exit on any disagreement,
 * a line naming what was committed against what was measured. For a model the
 * stakes differ from a binary — wrong weights make bad transcripts, not a
 * compromised machine — but the failure this catches is the same one: bytes
 * that changed at a URL that did not.
 *
 * A model that cannot be downloaded, after the retries `download` makes, is
 * reported and the run goes on to the rest; the exit is still non-zero. One
 * unreachable host used to end the run where it stood, and every model after
 * it went unchecked that week (KNEG-413).
 */
export async function verify(only) {
  const targets = pinnedTargets().filter((t) => !only || t.id === only);

  if (targets.length === 0) {
    die(
      only
        ? `no committed pin for a model with id "${only}".`
        : "no committed model pins were found at all. That is not a pass — see the header of " +
            "this file for where pins are read from."
    );
  }

  const wrong = [];
  const unreachable = [];
  for (const target of targets) {
    console.error(`${target.kind} / ${target.id}`);
    let measured;
    try {
      measured = await download(target.url, `${target.kind}-${target.id}`);
    } catch (error) {
      unreachable.push(`${target.kind}/${target.id}: ${describe(error)}`);
      continue;
    }
    try {
      if (measured.sha256 !== target.sha256) {
        wrong.push(
          `${target.kind}/${target.id}: committed ${target.sha256}, measured ${measured.sha256}`
        );
      } else if (measured.sizeBytes !== target.sizeBytes) {
        // Reachable even with a matching digest only if the pin's two halves
        // disagree with each other, which is worth saying out loud.
        wrong.push(
          `${target.kind}/${target.id}: committed ${target.sizeBytes} bytes, measured ${measured.sizeBytes}`
        );
      }
    } finally {
      rmSync(measured.path, { force: true });
    }
  }

  if (wrong.length > 0) {
    console.error(`\nmodel-pin --verify: ${wrong.length} of ${targets.length} model(s) disagree`);
    for (const line of wrong) console.error(`  ${line}`);
  }
  if (unreachable.length > 0) {
    console.error(
      `\nmodel-pin --verify: ${unreachable.length} of ${targets.length} model(s) could not be ` +
        "downloaded, so they were not checked"
    );
    for (const line of unreachable) console.error(`  ${line}`);
  }
  if (wrong.length > 0 || unreachable.length > 0) process.exit(1);
  console.log(`model-pin --verify: ${targets.length} model(s) match the committed pins.`);
}


/**
 * Every committed GGUF pin, as things to re-read rather than re-download.
 *
 * `localLlm` only. The whisper and sherpa pins are bytes this project hashed,
 * so re-reading a publisher's claim is not how they are checked -- `--verify`
 * is. Included here would make a weaker check look like it covered them.
 *
 * A digest with no size, or a size with no digest, is a `die()` and not a
 * skip, for the reason `pinnedTargets` gives: the generator emits `sha256`,
 * `sha256Measured` and `sizeBytes` together, so half a pin means
 * `config/provider-registry.json` was hand-edited, and half a pin is the one
 * input that would let this report a pass over something nobody checked. A
 * missing `revision` is the same failure and dies the same way -- reading the
 * tree at `main` instead would silently answer a question about different
 * bytes than the ones the installer fetches.
 */
export function statedTargets(reg = registry()) {
  const targets = [];

  const add = (id, object) => {
    if (!object.sha256 && object.sizeBytes === null) return;
    if (!object.sha256 || typeof object.sizeBytes !== "number") {
      die(
        `localLlm/${id} has ${object.sha256 ? "a sha256 and no sizeBytes" : "a sizeBytes and no sha256"}. ` +
          "The generator emits both or neither, so config/provider-registry.json has been " +
          "hand-edited -- fix the pin in generate-provider-registry.mjs and regenerate."
      );
    }
    if (!/^[0-9a-f]{40}$/.test(String(object.revision))) {
      die(
        `localLlm/${id} has no usable revision (got ${JSON.stringify(object.revision)}). The ` +
          "generator refuses to emit a model without one, so config/provider-registry.json " +
          "has been hand-edited -- fix HF_REVISIONS in generate-provider-registry.mjs and " +
          "regenerate."
      );
    }
    targets.push({
      id,
      repo: object.hfRepo,
      revision: object.revision,
      file: object.fileName,
      sha256: object.sha256.toLowerCase(),
      sizeBytes: object.sizeBytes,
      // Which standard this pin already met. A measured pin is *stronger* than
      // what this mode can check, so agreement here means "upstream has not
      // re-uploaded" and not "the bytes are still what we hashed".
      measured: object.sha256Measured === true,
    });
  };

  for (const provider of reg.localLlm ?? []) {
    for (const model of provider.models ?? []) {
      add(model.id, model);
      if (model.draft) add(`${model.id} (drafter)`, model.draft);
    }
  }

  return targets;
}

/**
 * One Hugging Face API document, as JSON. Bounded redirects and the same retry
 * as `download`; throws rather than exiting, so a caller decides what a failed
 * read means.
 */
async function apiJson(url) {
  return retrying(
    async () => {
      let current = url;
      for (let hop = 0; hop < 6; hop += 1) {
        const response = await fetch(current, { redirect: "manual" });
        if (response.status >= 300 && response.status < 400) {
          const next = response.headers.get("location");
          await discard(response);
          if (!next) throw new Error(`${current} redirected with no Location header`);
          current = new URL(next, current).toString();
          continue;
        }
        if (!response.ok) {
          await discard(response);
          throw new HttpError(response.status, current);
        }
        return await response.json();
      }
      throw new Error(`${url} redirected more than five times`);
    },
    { onRetry: (retry) => console.error(`  ${retryLine(url, retry)}`) }
  );
}

/**
 * A GitHub release-asset download URL, split into repository / tag / name.
 *
 * Exported beside [`parseHfUrl`] and for the same reason: one parser per URL
 * shape, so nothing has to keep two copies of it in step.
 */
export function parseGithubAssetUrl(url) {
  const match =
    /^https:\/\/github\.com\/([^/]+\/[^/]+)\/releases\/download\/([^/]+)\/(.+)$/.exec(
      String(url)
    );
  if (!match) return null;
  return { repo: match[1], tag: match[2], name: match[3] };
}

/**
 * Every committed pin that lives in a GitHub release, as an asset to look up.
 *
 * `localAsr.sherpa` **and** `localAsr.diarization`. It read only the first
 * until 2026-09-01, which was correct while sherpa held the only such pins and
 * became a hole the moment the two diarization models were committed: the
 * expensive check covered them and the cheap one did not, so a swapped asset
 * would have gone unnoticed by every run short of a full re-download. All four
 * sherpa archives sit under one rolling `asr-models` tag; the diarization pair
 * sits under two others in the same repository, which the tag comes from the
 * URL rather than being assumed.
 *
 * A model with no `sha256` is skipped for the reason [`pinnedTargets`] gives.
 * As of 2026-09-01 nothing is skipped.
 *
 * **These are bytes this project hashed**, unlike the GGUF pins. Re-reading
 * what GitHub states about them is strictly weaker than the download
 * `--verify` does, and is included here anyway because it is the difference
 * between knowing within seconds and knowing after a gigabyte. The output says
 * which, rather than letting a reader average the two.
 */
export function githubAssetTargets(reg = registry()) {
  const targets = [];
  for (const [kind, models] of [
    ["sherpa", reg.localAsr?.sherpa ?? []],
    ["diarization", reg.localAsr?.diarization ?? []],
  ]) {
    for (const model of models) {
      if (!model.sha256) continue;
      const parsed = parseGithubAssetUrl(model.downloadUrl);
      if (!parsed) {
        die(
          `${kind}/${model.id} is pinned but its downloadUrl is not a GitHub release asset: ` +
            `${model.downloadUrl}. This mode does not know how to re-read that host.`
        );
      }
      if (typeof model.expectedSizeBytes !== "number") {
        die(
          `${kind}/${model.id} has a sha256 and no expectedSizeBytes. The generator emits both ` +
            "or neither, so config/provider-registry.json has been hand-edited."
        );
      }
      targets.push({
        id: model.id,
        repo: parsed.repo,
        tag: parsed.tag,
        name: parsed.name,
        sha256: model.sha256.toLowerCase(),
        sizeBytes: model.expectedSizeBytes,
      });
    }
  }
  return targets;
}

/**
 * One pinned GitHub release asset against what the release API states about it.
 *
 * Returns `{ wrong, note }`. Every line in `wrong` is a disagreement and fails
 * the run; `note` is a line to print without failing, or `null`. The split is
 * the point of this being its own function: which facts fail is the whole
 * contract of `--verify-stated`, and inline beside the network calls nothing
 * could test it (KNEG-358).
 *
 * A missing digest is the note. GitHub states one only for assets uploaded
 * from about mid-2025, so for an older asset its absence says nothing about
 * the bytes -- and every asset here is re-hashed by `--verify` in the same
 * job. The size is compared either way: GitHub states that for every asset.
 */
export function compareAsset(target, found) {
  if (!found) {
    return { wrong: [`${target.id}: ${target.name} is no longer in ${target.repo} ${target.tag}`], note: null };
  }
  const wrong = [];
  let note = null;
  // GitHub states `sha256:<hex>`; the committed pin is bare hex.
  const stated = String(found.digest ?? "").replace(/^sha256:/, "").toLowerCase();
  if (!stated) {
    const uploaded = typeof found.updated_at === "string" ? `, uploaded ${found.updated_at.slice(0, 10)}` : "";
    note =
      `${target.id}: the release API states no digest for this asset${uploaded} -- size compared ` +
      "only; --verify re-hashes it";
  } else if (stated !== target.sha256) {
    wrong.push(`${target.id}: committed ${target.sha256}, stated ${stated}`);
  }
  if (typeof found.size === "number" && found.size !== target.sizeBytes) {
    wrong.push(`${target.id}: committed ${target.sizeBytes} bytes, stated ${found.size}`);
  }
  return { wrong, note };
}

/**
 * One GitHub release, by tag, as JSON.
 *
 * Sends `GITHUB_TOKEN` when the environment has one. The rate-limit message
 * below has always told the reader to set it, and until KNEG-413 nothing read
 * it. A 429 is retried with everything else transient; a 403 is not, because
 * waiting twenty seconds does not outlast an hourly limit.
 */
async function release(repo, tag) {
  const url = `https://api.github.com/repos/${repo}/releases/tags/${tag}`;
  const headers = { accept: "application/vnd.github+json", "user-agent": "knegiswhisper-model-pin" };
  if (process.env.GITHUB_TOKEN) headers.authorization = `Bearer ${process.env.GITHUB_TOKEN}`;

  return retrying(
    async () => {
      const response = await fetch(url, { headers });
      if (response.status === 403 || response.status === 429) {
        await discard(response);
        throw new HttpError(
          response.status,
          url,
          `${url} returned HTTP ${response.status}. GitHub rate-limits unauthenticated API calls to ` +
            "60 an hour per address; this mode needs one. Wait, or set GITHUB_TOKEN in the " +
            "environment if a run needs more."
        );
      }
      if (!response.ok) {
        await discard(response);
        throw new HttpError(response.status, url);
      }
      return await response.json();
    },
    { onRetry: (retry) => console.error(`  ${retryLine(url, retry)}`) }
  );
}

/**
 * One repository tree at a named revision, as JSON.
 *
 * The revision is a parameter rather than the constant `main` it used to be,
 * because the answer to "what does this repository hold" is only meaningful
 * once you say *when*. `--verify-stated` passes the committed commit; nothing
 * asks for `main` any more.
 */
async function tree(repo, revision) {
  return apiJson(
    `https://huggingface.co/api/models/${repo}/tree/${revision}?expand=true&recursive=true`
  );
}

/**
 * What a repository's `main` points at right now, and when it last changed.
 *
 * `sha` is the commit that goes into `HF_REVISIONS`. `lastModified` is context
 * rather than part of the pin: it is the difference between "nothing has
 * touched this since 2024" and "this moved last week", which is what tells a
 * reader whether a pin one commit behind is stale or simply older.
 */
async function modelInfo(repo) {
  return apiJson(`https://huggingface.co/api/models/${repo}`);
}

/**
 * Re-read every committed GGUF pin at its pinned commit and report disagreements.
 *
 * Non-zero exit on any disagreement, the same contract `--verify` has. Two
 * requests per repository -- the tree at the pinned commit, and what `main`
 * points at now -- so 39 objects cost about 72 requests and a few seconds,
 * which is the point: this can run weekly where re-downloading 202 GB cannot.
 *
 * The two halves answer different questions and only the first can fail.
 * Reading the tree at the committed revision asks whether the digest and the
 * revision beside it describe the same object, and whether that commit still
 * resolves at all -- a repository going private or being force-pushed would
 * break every install of its models, and nothing else here would notice.
 * Reading `main` asks whether upstream has published since, which is normal
 * and is reported as news.
 */
export async function verifyStated(only) {
  const targets = statedTargets().filter((t) => !only || t.id === only);

  if (targets.length === 0) {
    die(
      only
        ? `no committed GGUF pin for a model with id "${only}".`
        : "no committed GGUF pins were found at all. That is not a pass -- see the header of " +
            "this file for where pins are read from."
    );
  }

  // Keyed on repository *and* revision rather than repository alone. One
  // revision per repository is how HF_REVISIONS is built, but a cache that
  // assumed it would answer from the wrong tree the day that stopped being
  // true, and would do it silently.
  const trees = new Map();
  for (const target of targets) {
    const key = `${target.repo}@${target.revision}`;
    if (trees.has(key)) continue;
    console.error(`reading ${target.repo} at ${target.revision.slice(0, 12)} …`);
    trees.set(key, await tree(target.repo, target.revision));
  }

  const wrong = [];
  for (const target of targets) {
    const entries = trees.get(`${target.repo}@${target.revision}`);
    const entry = Array.isArray(entries)
      ? entries.find((e) => e.path === target.file)
      : undefined;

    if (!entry) {
      wrong.push(
        `${target.id}: ${target.file} is not in ${target.repo} at ${target.revision}`
      );
      continue;
    }
    // `lfs.oid` is the object's SHA-256, because git-lfs is content-addressed
    // on it. No `lfs` block at all on a multi-gigabyte GGUF would itself be
    // the news, so it is reported rather than skipped.
    const oid = entry.lfs?.oid;
    if (!oid) {
      wrong.push(`${target.id}: no lfs.oid in its tree entry — it may no longer be an LFS object`);
    } else if (oid.toLowerCase() !== target.sha256) {
      wrong.push(`${target.id}: committed ${target.sha256}, stated ${oid}`);
    }
    if (typeof entry.lfs?.size === "number" && entry.lfs.size !== target.sizeBytes) {
      wrong.push(`${target.id}: committed ${target.sizeBytes} bytes, stated ${entry.lfs.size}`);
    }
  }

  if (wrong.length > 0) {
    console.error(
      `\nmodel-pin --verify-stated: ${wrong.length} disagreement(s) over ${targets.length} object(s)`
    );
    for (const line of wrong) console.error(`  ${line}`);
    process.exit(1);
  }

  // Everything below this line is reporting, not checking. `wrong` is already
  // empty or the process has already exited. So a repository whose `main`
  // cannot be read is a line in the report, not a failure: the check it would
  // have informed has already passed (KNEG-413).
  const moved = [];
  const unread = [];
  const repos = [...new Set(targets.map((t) => t.repo))];
  for (const repo of repos) {
    const pinned = targets.find((t) => t.repo === repo).revision;
    let info;
    try {
      info = await modelInfo(repo);
    } catch (error) {
      unread.push(`  ${repo}: ${describe(error)}`);
      continue;
    }
    if (typeof info.sha === "string" && info.sha !== pinned) {
      moved.push(
        `  ${repo}: pinned ${pinned.slice(0, 12)}, main is now ${info.sha.slice(0, 12)}` +
          `${info.lastModified ? ` (pushed ${String(info.lastModified).slice(0, 10)})` : ""}`
      );
    }
  }

  // The k2-fsa assets, from the other host. One request per release: three
  // today, `asr-models` and the two diarization releases.
  const assets = only ? [] : githubAssetTargets();
  const assetWrong = [];
  const assetNotes = [];
  const releases = new Map();
  for (const target of assets) {
    const key = `${target.repo}@${target.tag}`;
    if (!releases.has(key)) {
      console.error(`reading ${target.repo} release ${target.tag} …`);
      releases.set(key, await release(target.repo, target.tag));
    }
    const found = (releases.get(key).assets ?? []).find((a) => a.name === target.name);
    const { wrong: disagreements, note } = compareAsset(target, found);
    assetWrong.push(...disagreements);
    if (note) assetNotes.push(note);
  }

  // Printed whether or not the run fails, so a red run still says which
  // assets were compared by size alone.
  if (assetNotes.length > 0) {
    console.log(
      `\n${assetNotes.length} of ${assets.length} GitHub release asset(s) have no stated digest. ` +
        "That is not a failure -- GitHub states none for assets uploaded before about mid-2025:"
    );
    for (const line of assetNotes) console.log(`  ${line}`);
  }

  if (assetWrong.length > 0) {
    console.error(
      `\nmodel-pin --verify-stated: ${assetWrong.length} disagreement(s) over ${assets.length} ` +
        "GitHub release asset(s)"
    );
    for (const line of assetWrong) console.error(`  ${line}`);
    process.exit(1);
  }

  const measured = targets.filter((t) => t.measured).length;
  console.log(
    `model-pin --verify-stated: ${targets.length} object(s) still match their committed ` +
      `digest and length at the commit they are pinned to (${trees.size} tree request(s); ` +
      `${measured} pinned to bytes hashed here, which this mode does not re-hash).`
  );
  if (assets.length > 0) {
    console.log(
      `model-pin --verify-stated: ${assets.length} GitHub release asset(s) match what the ` +
        `release API states (${assetNotes.length} by size alone, noted above). All of those ` +
        "are pinned to bytes hashed here, so agreement means " +
        "the asset has not been swapped -- not that the bytes are still what we hashed. Only " +
        "--verify proves the second thing."
    );
  }

  if (moved.length > 0) {
    // Deliberately not a failure. A publisher pushing is not a defect, and the
    // whole point of pinning a commit is that it costs us nothing when they do.
    // It is worth reading, because a pin far behind a busy repository is a pin
    // somebody should decide about rather than one that has decided for itself.
    console.log(
      `\n${moved.length} of ${repos.length} repositories have moved past the pinned commit. ` +
        `That is not a failure -- the pinned commit is what gets downloaded -- but it is ` +
        `what to re-read with --revisions when these pins are next revisited:`
    );
    for (const line of moved) console.log(line);
  } else if (unread.length === 0) {
    console.log(`No repository has moved past its pinned commit.`);
  }
  if (unread.length > 0) {
    console.log(
      `\n${unread.length} of ${repos.length} repositories' main could not be read, so whether ` +
        "they have moved is unknown this run. Not a failure -- this half only reports:"
    );
    for (const line of unread) console.log(line);
  }
}

/* --------------------------------------------------------------- revisions */

/**
 * A Hugging Face download URL, split into repository / revision / path.
 *
 * Exported because the same three parts are what
 * `tools/integration-audit/generate-provider-registry.mjs` rewrites when it
 * pins a whisper URL, and two regexes for one URL shape is how they drift.
 * Returns `null` for anything that is not a Hugging Face `resolve` URL, which
 * a caller must handle rather than assume away -- nothing in the registry is
 * hosted elsewhere today, and that is a fact about today.
 */
export function parseHfUrl(url) {
  const match = /^https:\/\/huggingface\.co\/([^/]+\/[^/]+)\/resolve\/([^/]+)\/(.+)$/.exec(
    String(url)
  );
  if (!match) return null;
  return { repo: match[1], revision: match[2], path: match[3] };
}

/**
 * Every Hugging Face repository the registry downloads from, once each.
 *
 * Both families, because both are served by the same host under the same
 * branch pointer: whisper URLs are stored whole and parsed back apart, GGUF
 * URLs are composed from `hfRepo` and so are read directly. The `from` string
 * is carried so a repository that cannot be read names the models that would
 * stop installing, rather than only itself.
 */
export function hfRepos(reg = registry()) {
  const found = new Map();
  const add = (repo, from) => {
    const seen = found.get(repo);
    if (seen) {
      if (!seen.includes(from)) seen.push(from);
      return;
    }
    found.set(repo, [from]);
  };

  for (const model of reg.localAsr?.whisper ?? []) {
    const parsed = parseHfUrl(model.downloadUrl);
    if (parsed) add(parsed.repo, `whisper/${model.id}`);
  }
  for (const provider of reg.localLlm ?? []) {
    for (const model of provider.models ?? []) {
      if (model.hfRepo) add(model.hfRepo, `localLlm/${model.id}`);
      if (model.draft?.hfRepo) add(model.draft.hfRepo, `localLlm/${model.id} (drafter)`);
    }
  }

  return [...found.entries()]
    .map(([repo, from]) => ({ repo, from }))
    .sort((a, b) => (a.repo < b.repo ? -1 : a.repo > b.repo ? 1 : 0));
}

/**
 * Read every repository's current `main` commit and print a pasteable table.
 *
 * Measures; never asserts. A repository that has moved past the committed
 * revision is not this mode's news to break on -- upstream publishing is
 * normal, and a tool that exits non-zero for it would be crying wolf weekly.
 * `--verify-stated` is where a real disagreement becomes an exit code.
 */
export async function revisions() {
  const repos = hfRepos();
  if (repos.length === 0) {
    die(
      "no Hugging Face repositories were found in the registry at all. That is not a pass -- " +
        "see the header of this file for where the registry is read from."
    );
  }

  const rows = [];
  for (const { repo, from } of repos) {
    console.error(`reading ${repo} …`);
    const info = await modelInfo(repo);
    if (typeof info.sha !== "string" || !/^[0-9a-f]{40}$/.test(info.sha)) {
      die(`${repo} returned no usable commit sha (got ${JSON.stringify(info.sha)}).`);
    }
    rows.push({
      repo,
      revision: info.sha,
      // Date only. The time of day a publisher pushed is not context anybody
      // reading a pin table needs, and a shorter string keeps the table
      // scannable down the column that matters.
      lastModified: String(info.lastModified ?? "").slice(0, 10) || null,
      usedBy: from,
    });
  }

  console.error("");
  console.error("As the HF_REVISIONS table in tools/integration-audit/generate-provider-registry.mjs:");
  console.error("");
  for (const row of rows) {
    console.error(`  "${row.repo}": {`);
    console.error(`    revision: "${row.revision}",`);
    console.error(`    lastModified: ${row.lastModified ? `"${row.lastModified}"` : "null"},`);
    console.error(`  },`);
  }
  console.error("");
  console.error(
    `model-pin --revisions: read ${rows.length} repository/repositories. These are what ` +
      `upstream points at today; nothing here was compared to what is committed.`
  );

  console.log(JSON.stringify(rows, null, 2));
}

/* -------------------------------------------------------------------- main */

/** `ggml-tiny.bin` from a whisper URL — a label, never part of a pin. */
function labelFor(options) {
  if (typeof options.id === "string") return options.id;
  const from = typeof options.url === "string" ? options.url : String(options.file ?? "model");
  const last = from.split(/[/\\]/).pop() ?? "model";
  return last.split("?")[0] || "model";
}

async function main(argv) {
  const options = args(argv);

  const only = typeof options.only === "string" ? options.only : null;

  if (options.verify) {
    await verify(only);
    return;
  }

  if (options["verify-stated"]) {
    await verifyStated(only);
    return;
  }

  if (options.revisions) {
    await revisions();
    return;
  }

  const hasUrl = typeof options.url === "string";
  const hasFile = typeof options.file === "string";
  if (hasUrl === hasFile) {
    die(
      "exactly one of --url <url> or --file <path> is required. See the header of this file " +
        "for examples, --verify to re-measure the committed pins, --verify-stated to " +
        "re-read the GGUF pins from Hugging Face without downloading them, or --revisions " +
        "to read what each repository's main points at today."
    );
  }

  const label = labelFor(options);
  const keep = typeof options.keep === "string" ? resolve(options.keep) : null;

  console.error(hasUrl ? `downloading ${options.url} …` : `hashing ${options.file} …`);
  const measured = hasUrl
    ? await download(options.url, label, keep)
    : await measureFile(options.file, label);

  try {
    // Deterministic on purpose: four fields, all of them stable across runs.
    // `resolvedFrom` is deliberately not in here — Hugging Face resolves to a
    // per-request presigned CDN URL carrying an expiry and a signature, which
    // is a fact about this download rather than about the model. It goes on
    // the human channel, reduced to the host that actually served the bytes.
    const pin = {
      id: label,
      ...(hasUrl ? { url: options.url } : { file: resolve(options.file) }),
      sha256: measured.sha256,
      sizeBytes: measured.sizeBytes,
    };

    if (measured.resolvedFrom) {
      console.error(`  served by ${new URL(measured.resolvedFrom).host}`);
    }

    console.error("");
    console.log(JSON.stringify(pin, null, 2));

    // Where a model pin actually goes, spelled out on the human channel.
    // stdout stays a single JSON document so it can be piped. One shape for
    // both tables, because there is one place now: WHISPER_PINS and
    // SHERPA_PINS in tools/integration-audit/generate-provider-registry.mjs.
    // A sherpa entry needs a `maxExtractedSizeBytes` alongside, which is a
    // judgement about a decompression bomb and not something to measure.
    const underscored = String(measured.sizeBytes).replace(/\B(?=(\d{3})+(?!\d))/g, "_");
    console.error(`\nAs an entry in WHISPER_PINS / SHERPA_PINS:`);
    console.error(`  "${label}": {`);
    console.error(`    sha256: "${measured.sha256}",`);
    console.error(`    expectedSizeBytes: ${underscored},`);
    console.error(`  },`);

    if (keep) console.error(`\nkept the download at ${keep}`);

    // Measure-and-assert, which is what makes a first pin checkable in the
    // same run that produces it rather than in a later one.
    const failures = [];
    if (typeof options["expect-sha256"] === "string") {
      const expected = options["expect-sha256"].toLowerCase();
      if (expected !== measured.sha256) {
        failures.push(`expected sha256 ${expected}, measured ${measured.sha256}`);
      }
    }
    if (typeof options["expect-size"] === "string") {
      const expected = Number(options["expect-size"].replace(/[_,]/g, ""));
      if (expected !== measured.sizeBytes) {
        failures.push(`expected ${expected} bytes, measured ${measured.sizeBytes}`);
      }
    }
    if (failures.length > 0) {
      console.error("");
      for (const line of failures) console.error(`model-pin: ${line}`);
      process.exitCode = 1;
    } else if (options["expect-sha256"] || options["expect-size"]) {
      console.error(`\nmodel-pin: ${label} matches what was expected.`);
    }
  } finally {
    // The temp file always goes. A `--keep` destination is not a temp file and
    // is left where it was asked for.
    if (!keep && hasUrl) rmSync(measured.path, { force: true });
  }
}

const isMain =
  process.argv[1] && resolve(process.argv[1]) === resolve(fileURLToPath(import.meta.url));
if (isMain) {
  // The network paths throw rather than exit, so one place turns a failure
  // into the tool's own one-line message and a non-zero exit, as `die` always
  // did, instead of a stack trace.
  try {
    await main(process.argv.slice(2));
  } catch (error) {
    die(describe(error));
  }
}
