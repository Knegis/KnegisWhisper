#!/usr/bin/env node
/**
 * Measures a sidecar pin, rather than copying one from a release note.
 *
 * `config/sidecars.json` has said "run tools/sidecar-pin rather than editing by
 * hand" since the first sidecar was added. This is that tool. It did not exist,
 * which meant the one instruction protecting the digests was unfollowable — and
 * a digest nobody can reproduce is a digest nobody can check.
 *
 * What it does, per target:
 *
 *   1. Downloads the asset to a temporary file, hashing as it streams. Nothing
 *      is held in memory; a CUDA build of llama.cpp is 260 MB.
 *   2. Records the **observed** size, not the size the release page reports.
 *   3. Reads the archive's index and finds the executable, so
 *      `executableInArchive` is measured too. That path is guessed more often
 *      than the digest is, and a wrong one fails at first launch with an error
 *      about a missing file rather than about a wrong pin.
 *   4. Measures what the asset extracts to -- the sum of the entries inside the
 *      payload directory, which is what `install::extract_with_limit` actually
 *      writes, not the archive's whole uncompressed size. `maxExtractedSizeBytes`
 *      is that figure plus headroom; see `withHeadroom` for why the ceiling is a
 *      judgement while everything around it is a measurement.
 *   5. Prints a JSON fragment ready to paste, or writes it with `--write`.
 *
 * Archives are read with a central-directory reader for zip and a header walk
 * for tar.gz, tar.xz and tar.bz2, rather than with a dependency. This tool
 * runs in the contracts job, which has no `npm install`.
 *
 * Two shapes need no archive reader at all:
 *   - `--bare`: the release asset has no archive around it -- yt-dlp ships a
 *     bare executable on every platform. There is no index to read; the
 *     digest of the download already covers the whole executable.
 *   - tar.xz (ffmpeg on Linux) and tar.bz2 (sherpa-onnx, almost everywhere):
 *     Node's `zlib` has no XZ or bzip2 decoder the way it has one for gzip,
 *     so reading their entries shells out to the system `xz` or `bzip2`
 *     binary (both present on every CI image this project targets, and
 *     bundled with Git for Windows) rather than hand-rolling an LZMA2 or
 *     bzip2 decoder for a tool that never ships inside the application. The
 *     Rust installer, which does ship, uses `lzma-rs` and `bzip2-rs` instead
 *     -- see `crates/sidecar/src/install.rs`.
 *
 * Usage:
 *   node tools/sidecar-pin/pin.mjs \
 *     --repo ggml-org/llama.cpp --release b9763 \
 *     --asset llama-b9763-bin-win-cpu-x64.zip \
 *     --target windows-x86_64 --executable llama-server
 *
 *   node tools/sidecar-pin/pin.mjs \
 *     --repo yt-dlp/yt-dlp --release 2024.08.06 \
 *     --asset yt-dlp.exe --bare \
 *     --target windows-x86_64 --executable yt-dlp.exe
 *
 *   node tools/sidecar-pin/pin.mjs --verify        # re-measure every pinned target
 *
 *   node tools/sidecar-pin/pin.mjs --extracted            # report extracted sizes
 *   node tools/sidecar-pin/pin.mjs --extracted --write    # and set the ceilings
 *
 * `--verify` is the reason this is worth having beyond the first pin: it proves
 * the bytes at a URL still hash to what is committed. A release retagged in
 * place is otherwise invisible. It also re-measures the extracted size and
 * fails if a committed ceiling has become too tight to install through, which
 * is the failure mode that would otherwise reach a user as "the download
 * refused itself".
 */

import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { createWriteStream, readFileSync, writeFileSync, rmSync, statSync } from "node:fs";
import { open } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { pipeline } from "node:stream/promises";
import { gunzipSync, inflateRawSync } from "node:zlib";

import {
  HttpError,
  describe,
  discard,
  removePartial,
  retryLine,
  retrying,
} from "../model-pin/retry.mjs";

const REPO_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..", "..");
const MANIFEST = join(REPO_ROOT, "config/sidecars.json");

function die(message) {
  console.error(`sidecar-pin: ${message}`);
  process.exit(1);
}

/** Parse `--flag value` pairs and bare `--flag` switches. */
function args(argv) {
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

/* ----------------------------------------------------------------- download */

/**
 * Download `url`, streaming to disk, and return its digest and observed size.
 *
 * Redirects are followed explicitly and counted. GitHub's release download
 * endpoint always redirects to object storage, so refusing redirects outright
 * would make this tool unable to do its job — but a redirect chain that does not
 * terminate is how a download ends up somewhere unintended, so it is bounded.
 *
 * A dropped connection is retried twice, from byte 0 and from `url` rather
 * than the URL a redirect led to (KNEG-413, `tools/model-pin/retry.mjs`). On
 * 2026-10-05 one socket GitHub closed ended the weekly check at its 19th of 21
 * targets. Anything else throws, having removed the partial file. `options`
 * replaces the retry's delays and sleep, for tests.
 */
export async function download(url, label, options = {}) {
  const target = join(tmpdir(), `sidecar-pin-${process.pid}-${label}`);
  return retrying(() => downloadOnce(url, target), {
    ...options,
    onRetry: (retry) => console.error(`  ${retryLine(url, retry, { fromZero: true })}`),
  });
}

/** One attempt at `download`. Throws on any failure, having removed what it wrote. */
async function downloadOnce(url, target) {
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

      const hash = createHash("sha256");
      out = createWriteStream(target);
      await pipeline(response.body, async function* (chunks) {
        for await (const chunk of chunks) {
          hash.update(chunk);
          yield chunk;
        }
      }, out);

      return {
        path: target,
        sha256: hash.digest("hex"),
        // The size on disk, not `content-length`: a truncated body would agree
        // with the header and disagree with itself.
        sizeBytes: statSync(target).size,
        resolvedFrom: current === url ? null : current,
      };
    }

    throw new Error(`${url} redirected more than five times`);
  } catch (error) {
    await removePartial(out, target);
    throw error;
  }
}

/* ------------------------------------------------------------ archive index */

/**
 * Entry names in a zip, from its central directory.
 *
 * Read from the end of the file rather than by decompressing anything, so a
 * 400 MB archive costs a few kilobytes of reads.
 */
async function zipEntries(path) {
  const handle = await open(path, "r");
  try {
    const { size } = await handle.stat();
    // The end-of-central-directory record is within the last 64 KiB + 22 bytes,
    // because the comment field it may carry is capped at 64 KiB.
    const tailLength = Math.min(size, 65_557);
    const tail = Buffer.alloc(tailLength);
    await handle.read(tail, 0, tailLength, size - tailLength);

    let eocd = -1;
    for (let i = tail.length - 22; i >= 0; i -= 1) {
      if (tail.readUInt32LE(i) === 0x0605_4b50) {
        eocd = i;
        break;
      }
    }
    if (eocd === -1) return null;

    let count = tail.readUInt16LE(eocd + 10);
    let directoryStart = tail.readUInt32LE(eocd + 16);
    const directorySize = tail.readUInt32LE(eocd + 12);

    // Zip64: the 32-bit fields saturate and the real values live in a separate
    // record. Not expected for these archives, but a silent wrong answer here
    // would be a wrong `executableInArchive`.
    if (directoryStart === 0xffff_ffff || count === 0xffff) {
      return null;
    }

    const directory = Buffer.alloc(directorySize);
    await handle.read(directory, 0, directorySize, directoryStart);

    const entries = [];
    let offset = 0;
    while (offset < directory.length && entries.length < count) {
      if (directory.readUInt32LE(offset) !== 0x0201_4b50) break;
      const nameLength = directory.readUInt16LE(offset + 28);
      const extraLength = directory.readUInt16LE(offset + 30);
      const commentLength = directory.readUInt16LE(offset + 32);
      entries.push({
        name: directory.toString("utf8", offset + 46, offset + 46 + nameLength),
        method: directory.readUInt16LE(offset + 10),
        compressedSize: directory.readUInt32LE(offset + 20),
        localOffset: directory.readUInt32LE(offset + 42),
      });
      offset += 46 + nameLength + extraLength + commentLength;
    }

    for (const entry of entries) {
      entry.size = entry.name.endsWith("/") ? 0 : await inflatedLength(handle, entry);
    }
    return entries;
  } finally {
    await handle.close();
  }
}

/**
 * How many bytes a zip entry actually produces, by decompressing it.
 *
 * **Not** `uncompressed_size` from the central directory, which is a
 * *declaration*, not a measurement. The `zip` crate the Rust installer reads
 * these archives with bounds an entry's reader by the *compressed* size
 * (zip-2.4.2/src/read.rs, `make_reader`), not by that field, so a zip can
 * declare one byte and hand over a megabyte -- and
 * `crates/sidecar/src/install.rs` therefore refuses on the bytes that arrive.
 * A ceiling measured off the declaration would be measured against a number
 * the installer does not use.
 */
async function inflatedLength(handle, entry) {
  if (entry.compressedSize === 0) return 0;

  const header = Buffer.alloc(30);
  await handle.read(header, 0, 30, entry.localOffset);
  if (header.readUInt32LE(0) !== 0x0403_4b50) {
    die(`${entry.name}: no local file header at offset ${entry.localOffset}`);
  }
  // The local header repeats the name and may carry different extra data, so
  // the payload starts after *its* lengths, not the central directory's.
  const dataStart = entry.localOffset + 30 + header.readUInt16LE(26) + header.readUInt16LE(28);

  const compressed = Buffer.alloc(entry.compressedSize);
  await handle.read(compressed, 0, entry.compressedSize, dataStart);

  if (entry.method === 0) return compressed.length; // stored
  if (entry.method === 8) return inflateRawSync(compressed).length; // deflate
  return die(
    `${entry.name} uses zip compression method ${entry.method}. The installer builds the ` +
      `zip crate with the "deflate" feature only, so it could not read this entry either.`
  );
}

/** Entries in a tar, by walking 512-byte headers. Shared by the tar.gz and
 * tar.xz readers below, which differ only in how they get from a compressed
 * file on disk to this uncompressed `Buffer`.
 *
 * A tar entry's body length *is* the size in its header -- the `tar` crate
 * reads the body through an `io::Take` over that field -- so unlike a zip there
 * is nothing to decompress and compare it against. */
function tarEntriesFromBuffer(tar) {
  const entries = [];

  for (let offset = 0; offset + 512 <= tar.length; ) {
    const name = tar.toString("utf8", offset, offset + 100).replace(/\0.*$/, "");
    if (name === "") break; // Two zero blocks end the archive.

    const sizeField = tar.toString("utf8", offset + 124, offset + 136).replace(/\0.*$/, "").trim();
    const size = Number.parseInt(sizeField, 8) || 0;

    // A `pax` or `gnu` long-name record describes the *next* entry rather than
    // being one, so it is skipped instead of listed.
    const typeFlag = tar.toString("utf8", offset + 156, offset + 157);
    if (!["x", "g", "L", "K"].includes(typeFlag)) {
      // Only a regular file contributes to what gets written: directories,
      // symlinks and hard links carry a size of 0 in the header anyway, and
      // `install::extract_tar` skips anything `entry_type().is_file()` is
      // false for.
      const isFile = typeFlag === "0" || typeFlag === "\0" || typeFlag === "";
      entries.push({ name, size: isFile ? size : 0 });
    }

    offset += 512 + Math.ceil(size / 512) * 512;
  }
  return entries;
}

/**
 * The archive-internal directory whose contents get installed. Mirrors
 * `Asset::payload_dir` in `crates/sidecar/src/manifest.rs`.
 */
function payloadDirOf(executableInArchive) {
  const index = Math.max(
    executableInArchive.lastIndexOf("/"),
    executableInArchive.lastIndexOf("\\")
  );
  return index === -1 ? "" : executableInArchive.slice(0, index);
}

/**
 * How many bytes `install::extract_with_limit` would write for this asset.
 *
 * Mirrors `install::payload_relative`: names are normalised to forward slashes,
 * the payload directory prefix is stripped, and anything outside it is skipped.
 * That last part is why this is not simply "the archive's uncompressed size" --
 * the Windows whisper archive carries `main.exe`, `SDL2.dll`, `llama.dll` and a
 * dozen test binaries that are never written, and counting them would inflate
 * the ceiling into meaninglessness.
 */
function extractedBytes(entries, payloadDir) {
  let total = 0;
  for (const { name, size } of entries) {
    const normalised = name.replace(/\\/g, "/");
    let relative = normalised;
    if (payloadDir !== "") {
      const prefix = `${payloadDir.replace(/\\/g, "/")}/`;
      if (!normalised.startsWith(prefix)) continue;
      relative = normalised.slice(prefix.length);
    }
    if (relative === "" || relative.endsWith("/")) continue;
    total += size;
  }
  return total;
}

/** Entry names in a tar.gz, by gunzipping (Node's `zlib` does this natively)
 * and walking 512-byte headers. */
function tarGzEntries(path) {
  return tarEntriesFromBuffer(gunzipSync(readFileSync(path)));
}

/**
 * Entry names in a tar.xz, by decompressing with the system `xz` binary and
 * walking 512-byte headers the same way `tarGzEntries` does.
 *
 * Node's `zlib` module decodes gzip natively but has no XZ/LZMA decoder, and
 * writing one for a tool that only ever runs on a maintainer's machine or in
 * CI -- never inside the shipped application -- is not worth it: the
 * application's own installer uses `lzma-rs` (pure Rust) precisely so it does
 * not need a system binary. This tool is not the application, so shelling out
 * to `xz` here is the pragmatic choice.
 */
function tarXzEntries(path) {
  const result = spawnSync("xz", ["--decompress", "--stdout", "--keep", path], {
    maxBuffer: 1024 * 1024 * 1024,
  });
  if (result.error) {
    die(
      `could not run the system "xz" binary to read ${path}: ${result.error.message}. Install ` +
        `xz (e.g. "apt install xz-utils", "brew install xz"; it ships with Git for Windows) to ` +
        "pin a tar.xz asset."
    );
  }
  if (result.status !== 0) {
    die(`"xz --decompress" exited ${result.status} on ${path}:\n${result.stderr.toString("utf8")}`);
  }
  return tarEntriesFromBuffer(result.stdout);
}

/**
 * Entry names in a tar.bz2, by decompressing with the system `bzip2` binary
 * and walking 512-byte headers the same way `tarGzEntries` does.
 *
 * Same reasoning as `tarXzEntries` above: Node's `zlib` has no bzip2 decoder,
 * and this tool never ships inside the application, so shelling out to the
 * system `bzip2` binary (present on every CI image this project targets, and
 * bundled with Git for Windows) is the pragmatic choice here too. The Rust
 * installer uses `bzip2-rs` (pure Rust) instead -- see
 * `crates/sidecar/src/install.rs`. sherpa-onnx is the reason this exists: 276
 * of its 303 release assets are `.tar.bz2`, including every desktop binary
 * bundle and all four model archives the provider registry points at.
 */
function tarBz2Entries(path) {
  // sherpa-onnx's static (self-contained, no separate .so/.dylib) desktop
  // bundles decompress to more than 1 GiB -- comfortably over the 1 GiB
  // `tarXzEntries` uses for ffmpeg's much smaller tar.xz -- so this needs a
  // larger ceiling.
  const result = spawnSync("bzip2", ["--decompress", "--stdout", "--keep", path], {
    maxBuffer: 2 * 1024 * 1024 * 1024,
  });
  if (result.error) {
    die(
      `could not run the system "bzip2" binary to read ${path}: ${result.error.message}. Install ` +
        `bzip2 (e.g. "apt install bzip2", "brew install bzip2"; it ships with Git for Windows) ` +
        "to pin a tar.bz2 asset."
    );
  }
  if (result.status !== 0) {
    die(
      `"bzip2 --decompress" exited ${result.status} on ${path}:\n${result.stderr.toString("utf8")}`
    );
  }
  return tarEntriesFromBuffer(result.stdout);
}

/**
 * Find the one entry that is the executable we asked for.
 *
 * Ambiguity is an error, not a first match. `llama-server` and
 * `llama-server.exe` in one archive would be a real question about which to run,
 * and picking silently is how the wrong one gets pinned.
 */
function findExecutable(entries, executable) {
  const wanted = [executable, `${executable}.exe`];
  const matches = entries
    .map((e) => e.name)
    .filter((name) => wanted.includes(name.split("/").pop()));

  if (matches.length === 0) {
    die(
      `no entry named ${wanted.join(" or ")} in the archive. It contains:\n  ` +
        entries
          .slice(0, 40)
          .map((e) => e.name)
          .join("\n  ")
    );
  }
  if (matches.length > 1) {
    die(`the archive has ${matches.length} candidates and no way to choose: ${matches.join(", ")}`);
  }
  return matches[0];
}

/**
 * `zip`, `tar-gz`, `tar-xz` or `tar-bz2`, from the asset name.
 *
 * Never guesses `bare`: an asset with no recognised archive extension is far
 * more likely a typo than a bare executable, so that shape is only ever
 * chosen by the explicit `--bare` flag, not inferred here.
 */
function archiveKind(asset) {
  if (asset.endsWith(".zip")) return "zip";
  if (asset.endsWith(".tar.gz") || asset.endsWith(".tgz")) return "tar-gz";
  if (asset.endsWith(".tar.xz") || asset.endsWith(".txz")) return "tar-xz";
  if (asset.endsWith(".tar.bz2") || asset.endsWith(".tbz2")) return "tar-bz2";
  return die(
    `${asset} is neither a .zip, .tar.gz, .tar.xz nor .tar.bz2, so its index cannot be read. If ` +
      "it ships with no archive at all, pass --bare."
  );
}

async function entriesOf(path, kind) {
  const entries =
    kind === "zip"
      ? await zipEntries(path)
      : kind === "tar-gz"
        ? tarGzEntries(path)
        : kind === "tar-xz"
          ? tarXzEntries(path)
          : tarBz2Entries(path);
  if (entries === null) {
    die("the archive index could not be read (zip64 archives are not supported)");
  }
  return entries;
}

/* ------------------------------------------------------- the output ceiling */

/**
 * A decompression-bomb ceiling, from a measured extracted size.
 *
 * **This is the one judgement in this tool.** Everything else it emits is
 * measured: the digest is hashed from the bytes, the size comes from `stat`,
 * the executable path is read out of the archive's index, and the extracted
 * total is the sum of what `install::extract_with_limit` would actually write.
 * The ceiling is that measurement plus 25%, rounded up to the next MiB.
 *
 * One stated rule applied to every target, rather than 21 numbers chosen one at
 * a time. That makes the headroom reproducible and arguable as a rule instead
 * of as a list.
 *
 * **Why 25%.** The drift a pinned asset sees between one upstream build and the
 * next is a few percent -- a rebuilt binary, an added microarchitecture DLL, a
 * change in debug symbols -- not a multiple. 25% absorbs that without leaving
 * the ceiling meaningless: something that decompresses to several times what
 * was measured is still refused. Being wrong high costs protection; being wrong
 * low refuses a legitimate install, so it errs high -- the same reasoning
 * `SHERPA_PINS` in `tools/integration-audit/generate-provider-registry.mjs`
 * gives for its two hand-chosen ceilings (663 MB measured, 750 MiB pinned).
 *
 * The headroom is not a substitute for re-pinning. An upstream change large
 * enough to matter changes the digest, and `--verify` fails on that first.
 */
const HEADROOM = 1.25;
const MIB = 1024 * 1024;

function withHeadroom(measured) {
  return Math.ceil((measured * HEADROOM) / MIB) * MIB;
}

/**
 * What one target extracts to, measured off the bytes the manifest pins.
 *
 * Refuses to measure an asset whose digest has moved: a ceiling derived from
 * bytes that are not the pinned bytes pins the wrong number, and quietly.
 */
async function measureExtracted(name, target, spec) {
  const measured = await download(spec.url, `${name}-${target}`);
  try {
    if (measured.sha256 !== spec.sha256) {
      die(
        `${name}/${target}: committed digest ${spec.sha256}, measured ${measured.sha256}. ` +
          "Re-pin the asset before measuring what it extracts to."
      );
    }

    // `Bare` is copied verbatim by `install::extract_bare`, so its extracted
    // size is its download size -- and there is no header that could claim
    // otherwise, which makes it the one kind whose ceiling cannot be lied to.
    if (spec.archive === "bare") {
      return { extracted: measured.sizeBytes, ceiling: measured.sizeBytes };
    }

    const entries = await entriesOf(measured.path, spec.archive);
    const extracted = extractedBytes(entries, payloadDirOf(spec.executableInArchive));
    return { extracted, ceiling: withHeadroom(extracted) };
  } finally {
    rmSync(measured.path, { force: true });
  }
}

/**
 * Measure every published target's extracted size and report it against the
 * committed ceiling. With `--write`, set the ceiling from the measurement.
 *
 * Separate from `--verify` because it is what runs *before* a ceiling exists;
 * `--verify` then re-runs the same measurement as a check.
 */
async function extracted(only, write) {
  const manifest = JSON.parse(readFileSync(MANIFEST, "utf8"));
  const rows = [];

  for (const sidecar of manifest.sidecars) {
    if (only && sidecar.name !== only) continue;
    for (const [target, spec] of Object.entries(sidecar.targets)) {
      if (spec.availability !== "published") continue;
      console.error(`${sidecar.name} / ${target}`);

      const { extracted: size, ceiling } = await measureExtracted(sidecar.name, target, spec);
      rows.push({
        target: `${sidecar.name}/${target}`,
        archive: spec.archive,
        download: spec.sizeBytes,
        extracted: size,
        ceiling,
        committed: spec.maxExtractedSizeBytes ?? null,
      });
      if (write) spec.maxExtractedSizeBytes = ceiling;
    }
  }

  const number = (n) => (n === null ? "-" : n.toLocaleString("en-US"));
  const width = (key) => Math.max(...rows.map((r) => number(r[key]).length));
  console.log(
    `\n${"target".padEnd(Math.max(...rows.map((r) => r.target.length)))}  ` +
      `${"archive".padEnd(7)}  ${"download".padStart(width("download"))}  ` +
      `${"extracted".padStart(width("extracted"))}  ${"ceiling".padStart(width("ceiling"))}  ` +
      "committed"
  );
  for (const row of rows) {
    console.log(
      `${row.target.padEnd(Math.max(...rows.map((r) => r.target.length)))}  ` +
        `${row.archive.padEnd(7)}  ${number(row.download).padStart(width("download"))}  ` +
        `${number(row.extracted).padStart(width("extracted"))}  ` +
        `${number(row.ceiling).padStart(width("ceiling"))}  ${number(row.committed)}`
    );
  }
  console.log(
    `\n${rows.length} target(s). "extracted" is measured; "ceiling" is that figure plus ` +
      `${Math.round((HEADROOM - 1) * 100)}% rounded up to the next MiB, which is a judgement.`
  );

  if (write) {
    writeFileSync(MANIFEST, `${JSON.stringify(manifest, null, 2)}\n`, "utf8");
    console.error(`\nwrote maxExtractedSizeBytes for ${rows.length} target(s) to ${MANIFEST}`);
  }
}

/* ---------------------------------------------------------------- measuring */

async function measure({ repo, release, asset, executable, bare }) {
  const url = `https://github.com/${repo}/releases/download/${release}/${asset}`;

  console.error(`  downloading ${asset}${bare ? " (bare, no archive)" : ""} …`);
  const measured = await download(url, asset);

  try {
    if (bare) {
      // There is no archive to index, so `--executable` is taken as the
      // on-disk name verbatim rather than searched for. The digest above
      // already covers the whole file -- the strongest row in the table, not
      // a weaker one, since there is no container to have extracted wrong.
      return {
        availability: "published",
        url,
        archive: "bare",
        sha256: measured.sha256,
        sizeBytes: measured.sizeBytes,
        executableInArchive: executable,
        // `Bare` is copied verbatim, so the extracted size *is* the download
        // size. Headroom would be meaningless here: a ceiling above the
        // measured length could only ever be reached by a different file, and
        // the digest already rules that out.
        maxExtractedSizeBytes: measured.sizeBytes,
      };
    }

    const kind = archiveKind(asset);
    const entries = await entriesOf(measured.path, kind);
    const executableInArchive = findExecutable(entries, executable);
    const extracted = extractedBytes(entries, payloadDirOf(executableInArchive));

    return {
      availability: "published",
      url,
      archive: kind,
      sha256: measured.sha256,
      sizeBytes: measured.sizeBytes,
      executableInArchive,
      // A judgement, not a measurement -- see `withHeadroom`. The measured
      // figure is printed beside it so the headroom is visible.
      maxExtractedSizeBytes: withHeadroom(extracted),
    };
  } finally {
    rmSync(measured.path, { force: true });
  }
}

/* ------------------------------------------------------------------- verify */

/**
 * Re-measure every published target in the manifest and report disagreements.
 *
 * This is what makes a pin more than a comment: a release retagged in place
 * changes the bytes and nothing else, and only re-hashing notices.
 *
 * A target that cannot be downloaded, after `download`'s retries, is reported
 * and the run goes on to the rest; the exit is still non-zero (KNEG-413).
 */
async function verify(only) {
  const manifest = JSON.parse(readFileSync(MANIFEST, "utf8"));
  let checked = 0;
  const wrong = [];
  const unreachable = [];

  for (const sidecar of manifest.sidecars) {
    if (only && sidecar.name !== only) continue;
    for (const [target, spec] of Object.entries(sidecar.targets)) {
      if (spec.availability !== "published") continue;
      console.error(`${sidecar.name} / ${target}`);

      let measured;
      try {
        measured = await download(spec.url, `${sidecar.name}-${target}`);
      } catch (error) {
        unreachable.push(`${sidecar.name}/${target}: ${describe(error)}`);
        continue;
      }
      checked += 1;
      try {
        if (measured.sha256 !== spec.sha256) {
          wrong.push(
            `${sidecar.name}/${target}: committed ${spec.sha256}, measured ${measured.sha256}`
          );
        } else if (measured.sizeBytes !== spec.sizeBytes) {
          wrong.push(
            `${sidecar.name}/${target}: committed ${spec.sizeBytes} bytes, measured ${measured.sizeBytes}`
          );
        } else if (spec.archive === "bare") {
          // No archive index to disagree with the pin: the digest and size
          // checks above already cover the entire file. The ceiling is the
          // download size, so it is checked against the same figure.
          if (spec.maxExtractedSizeBytes < measured.sizeBytes) {
            wrong.push(
              `${sidecar.name}/${target}: maxExtractedSizeBytes is ` +
                `${spec.maxExtractedSizeBytes} but the file it copies verbatim is ` +
                `${measured.sizeBytes} bytes, so no install could ever succeed`
            );
          }
        } else {
          const entries = await entriesOf(measured.path, spec.archive);
          if (!entries.some((e) => e.name === spec.executableInArchive)) {
            wrong.push(
              `${sidecar.name}/${target}: the archive has no ${spec.executableInArchive}`
            );
          }
          // A ceiling below what the asset really extracts to refuses every
          // install of it -- the failure mode `docs/PLATFORM_SUPPORT.md`
          // records for the sherpa `sizeMb` figure, which was the *extracted*
          // footprint used as a download bound. Caught here rather than by a
          // user.
          const size = extractedBytes(entries, payloadDirOf(spec.executableInArchive));
          if (size > spec.maxExtractedSizeBytes) {
            wrong.push(
              `${sidecar.name}/${target}: extracts to ${size} bytes, over its ` +
                `maxExtractedSizeBytes of ${spec.maxExtractedSizeBytes}. Re-run ` +
                "--extracted --write."
            );
          }
        }
      } finally {
        rmSync(measured.path, { force: true });
      }
    }
  }

  if (wrong.length > 0) {
    console.error(`\nsidecar-pin --verify: ${wrong.length} of ${checked} targets disagree`);
    for (const line of wrong) console.error(`  ${line}`);
  }
  if (unreachable.length > 0) {
    console.error(
      `\nsidecar-pin --verify: ${unreachable.length} target(s) could not be downloaded, so they ` +
        "were not checked"
    );
    for (const line of unreachable) console.error(`  ${line}`);
  }
  if (wrong.length > 0 || unreachable.length > 0) process.exit(1);
  console.log(`sidecar-pin --verify: ${checked} target(s) match the committed pins.`);
}

/* --------------------------------------------------------------------- main */

async function main(argv) {
  const options = args(argv);

  if (options.verify) {
    await verify(typeof options.sidecar === "string" ? options.sidecar : null);
    return;
  }
  if (options.extracted) {
    await extracted(
      typeof options.sidecar === "string" ? options.sidecar : null,
      Boolean(options.write)
    );
    return;
  }

  for (const required of ["repo", "release", "asset", "target"]) {
    if (typeof options[required] !== "string") {
      die(`--${required} is required. See the header of this file for an example.`);
    }
  }
  const executable = typeof options.executable === "string" ? options.executable : null;
  if (!executable) die("--executable is required: the path in the archive is measured, not guessed");

  const spec = await measure({
    repo: options.repo,
    release: options.release,
    asset: options.asset,
    executable,
    bare: Boolean(options.bare),
  });

  console.error(`\n${options.target}:`);
  console.log(JSON.stringify({ [options.target]: spec }, null, 2));

  if (options.write) {
    if (typeof options.sidecar !== "string") {
      die("--write needs --sidecar <name> so it knows which entry to update");
    }
    const manifest = JSON.parse(readFileSync(MANIFEST, "utf8"));
    const sidecar = manifest.sidecars.find((s) => s.name === options.sidecar);
    if (!sidecar) die(`no sidecar named ${options.sidecar} in config/sidecars.json`);
    sidecar.targets[options.target] = spec;
    writeFileSync(MANIFEST, `${JSON.stringify(manifest, null, 2)}\n`, "utf8");
    console.error(`wrote ${options.sidecar}/${options.target} to config/sidecars.json`);
  }
}

// Only as the entry point, so the tests can import `download` without starting
// a measurement (KNEG-413). The network paths throw rather than exit; this is
// the one place a failure becomes the tool's one-line message and exit 1.
const isMain =
  process.argv[1] && resolve(process.argv[1]) === resolve(fileURLToPath(import.meta.url));
if (isMain) {
  try {
    await main(process.argv.slice(2));
  } catch (error) {
    die(describe(error));
  }
}
