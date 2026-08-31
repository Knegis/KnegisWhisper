#!/usr/bin/env node
/**
 * Measures a model's SHA-256 and exact byte size, rather than inventing one.
 *
 * `crates/transcription-core/src/models.rs` says, of the six whisper models:
 *
 *   > Pinning all of them means downloading roughly seven gigabytes once,
 *   > which is a job for a tool rather than something to invent digests for
 *   > here: `tiny` is pinned because it was actually downloaded and hashed
 *   > while this was written, and the rest say plainly that they were not.
 *
 * This is that tool. It exists so the difference between a measured digest and
 * a copied one stays visible: every row it prints was produced by bytes moving
 * through this process, and nothing else it prints claims to have been.
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
 * 21,260,251,955 bytes (about 19.8 GiB); the largest whisper model is
 * `ggml-large-v3.bin` at roughly 3 GB, and the pinned sherpa archives are
 * about 500 MB each. Peak memory is one chunk regardless.
 *
 * A download that dies at 19 GiB is not resumed: a partially-fetched file
 * cannot be hashed into a whole-file digest without trusting the part already
 * on disk, and this tool's only job is to not trust things. `--file` is the
 * escape hatch — if the bytes are already on the machine, measure those and
 * skip the network entirely.
 *
 * ## Usage
 *
 *   node tools/model-pin/pin.mjs --url <url> [--id <model-id>] [--keep <path>]
 *   node tools/model-pin/pin.mjs --file <path> [--id <model-id>]
 *   node tools/model-pin/pin.mjs --verify [--only <model-id>]
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
 * Committed model pins live in two places today, and `--verify` reads both
 * rather than keeping a third copy that nobody would maintain:
 *
 *   1. `config/provider-registry.json` → `localAsr.sherpa[]`. Generated, so
 *      the URL, `sha256` and `expectedSizeBytes` are read as data with no
 *      parsing. (Their source is `SHERPA_PINS` in
 *      `tools/integration-audit/generate-provider-registry.mjs`.)
 *   2. `crates/transcription-core/src/models.rs` → the `PINNED` table, joined
 *      to `localAsr.whisper[]` by model id for the URL. That table is a Rust
 *      literal, so it is parsed — narrowly, and with the array's own declared
 *      arity used as a checksum on the parse. If the table's shape changes,
 *      this dies loudly rather than verifying nothing and reporting success.
 *
 * Reading Rust from a Node tool is the ugly half of that, and it is ugly
 * because the pins are in two places, not because parsing is hard. The clean
 * fix is a `WHISPER_PINS` table beside `SHERPA_PINS` in the registry
 * generator, so `config/provider-registry.json` carries `sha256` for whisper
 * too and `models.rs` reads it from `kw_provider_registry::WhisperModel`
 * instead of holding its own list. Then step 2 above deletes itself and this
 * tool reads one generated file. That change spans crates and the generator,
 * so it is a proposal here rather than something this file does.
 */

import { createHash } from "node:crypto";
import { createWriteStream, readFileSync, createReadStream, rmSync, statSync } from "node:fs";
import { mkdir } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { pipeline } from "node:stream/promises";

const REPO_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..", "..");
const REGISTRY = join(REPO_ROOT, "config/provider-registry.json");
const WHISPER_PIN_SOURCE = join(REPO_ROOT, "crates/transcription-core/src/models.rs");

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
  // time, which is the property that matters at 20 GiB.
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
 */
export async function download(url, label, keepAt = null) {
  const target = keepAt ?? join(tmpdir(), `model-pin-${process.pid}-${label.replace(/[^\w.-]/g, "_")}`);
  if (keepAt) await mkdir(dirname(target), { recursive: true });
  let current = url;

  for (let hop = 0; hop < 6; hop += 1) {
    const response = await fetch(current, { redirect: "manual" });

    if (response.status >= 300 && response.status < 400) {
      const next = response.headers.get("location");
      if (!next) die(`${current} redirected with no Location header`);
      current = new URL(next, current).toString();
      continue;
    }

    if (!response.ok) {
      die(`${current} returned HTTP ${response.status}`);
    }

    // Read only to size the progress bar. It is never reported as the pin.
    const declared = Number(response.headers.get("content-length")) || null;

    const hash = createHash("sha256");
    const reporter = progress(label, declared);
    const out = createWriteStream(target);
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

  return die(`${url} redirected more than five times`);
}

/* ---------------------------------------------------------- pin discovery */

function registry() {
  try {
    return JSON.parse(readFileSync(REGISTRY, "utf8"));
  } catch (error) {
    return die(`cannot read ${REGISTRY}: ${error.message}`);
  }
}

/**
 * The `PINNED` rows in `crates/transcription-core/src/models.rs`.
 *
 * A deliberately narrow reader for one literal of one known shape:
 *
 *   const PINNED: [(&str, &str, u64); 1] = [("tiny", "be07…", 77_691_713)];
 *
 * The array's declared length is used as a checksum on the parse. If the table
 * grows a field, becomes a slice of structs, or moves, the count will not
 * match — or the header will not match at all — and this throws rather than
 * returning a short list. A verifier that silently checks nothing is worse
 * than no verifier, because it reports success.
 *
 * Exported for the tests, which is where the shape assumptions are asserted
 * against strings instead of against the network.
 */
export function parseWhisperPins(source) {
  const table = source.match(
    /const\s+PINNED\s*:\s*\[\s*\(\s*&str\s*,\s*&str\s*,\s*u64\s*\)\s*;\s*(\d+)\s*\]\s*=\s*\[([\s\S]*?)\]\s*;/
  );
  if (!table) {
    throw new Error(
      "no `const PINNED: [(&str, &str, u64); N]` table found. It moved or changed shape — " +
        "update this reader, or move the whisper pins into the provider registry (see the " +
        "header of this file)."
    );
  }

  const declared = Number(table[1]);
  const rows = [];
  const tuple = /\(\s*"([^"]+)"\s*,\s*"([0-9a-fA-F]{64})"\s*,\s*([0-9_]+)\s*,?\s*\)/g;
  for (const match of table[2].matchAll(tuple)) {
    rows.push({
      id: match[1],
      sha256: match[2].toLowerCase(),
      sizeBytes: Number(match[3].replace(/_/g, "")),
    });
  }

  if (rows.length !== declared) {
    throw new Error(
      `the PINNED table declares ${declared} entries and ${rows.length} parsed. One of them is ` +
        "not a plain (id, 64-hex digest, size) tuple; this reader will not guess at the rest."
    );
  }
  return rows;
}

/**
 * Every committed model pin, as one list of things to re-measure.
 *
 * Sherpa comes from the generated registry, whisper from the Rust table joined
 * to the registry for its URL. A pin whose id is in neither list is an error,
 * following the same rule `generate-provider-registry.mjs` applies to
 * `SHERPA_PINS`: a pin for a model that does not exist is a typo that would
 * otherwise go unnoticed until somebody wondered why verification never ran.
 */
export function pinnedTargets(reg = registry(), rustSource = null) {
  const targets = [];

  const whisper = reg.localAsr?.whisper ?? [];
  const source = rustSource ?? readFileSync(WHISPER_PIN_SOURCE, "utf8");
  let rows;
  try {
    rows = parseWhisperPins(source);
  } catch (error) {
    return die(`${WHISPER_PIN_SOURCE}: ${error.message}`);
  }
  for (const row of rows) {
    const model = whisper.find((m) => m.id === row.id);
    if (!model) {
      die(
        `the PINNED table in models.rs pins "${row.id}", which is not a model in the registry's ` +
          "localAsr.whisper. Fix the id, or remove the pin."
      );
    }
    targets.push({
      kind: "whisper",
      id: row.id,
      url: model.downloadUrl,
      sha256: row.sha256,
      // The Rust table's size, not the registry's `expectedSizeBytes`: the
      // registry figure is rounded for display (78,000,000 for tiny, which is
      // really 77,691,713) and would never match a measurement.
      sizeBytes: row.sizeBytes,
      from: "crates/transcription-core/src/models.rs PINNED",
    });
  }

  for (const model of reg.localAsr?.sherpa ?? []) {
    if (!model.sha256) continue;
    targets.push({
      kind: "sherpa",
      id: model.id,
      url: model.downloadUrl,
      sha256: model.sha256.toLowerCase(),
      // Measured, unlike whisper's: SHERPA_PINS carries the exact size beside
      // the digest, so this one is the real figure.
      sizeBytes: model.expectedSizeBytes ?? null,
      from: "config/provider-registry.json localAsr.sherpa",
    });
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
  for (const target of targets) {
    console.error(`${target.kind} / ${target.id}`);
    const measured = await download(target.url, `${target.kind}-${target.id}`);
    try {
      if (measured.sha256 !== target.sha256) {
        wrong.push(
          `${target.kind}/${target.id}: committed ${target.sha256}, measured ${measured.sha256}`
        );
      } else if (target.sizeBytes !== null && measured.sizeBytes !== target.sizeBytes) {
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
    process.exit(1);
  }
  console.log(`model-pin --verify: ${targets.length} model(s) match the committed pins.`);
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

  if (options.verify) {
    await verify(typeof options.only === "string" ? options.only : null);
    return;
  }

  const hasUrl = typeof options.url === "string";
  const hasFile = typeof options.file === "string";
  if (hasUrl === hasFile) {
    die(
      "exactly one of --url <url> or --file <path> is required. See the header of this file " +
        "for examples, or --verify to re-measure the committed pins."
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

    // The two places a model pin actually goes, spelled out on the human
    // channel. stdout stays a single JSON document so it can be piped.
    const underscored = String(measured.sizeBytes).replace(/\B(?=(\d{3})+(?!\d))/g, "_");
    console.error(`\nAs a row in models.rs PINNED:`);
    console.error(`    ("${label}", "${measured.sha256}", ${underscored}),`);
    console.error(`\nAs an entry in SHERPA_PINS (generate-provider-registry.mjs):`);
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
  await main(process.argv.slice(2));
}
