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
 *   4. Prints a JSON fragment ready to paste, or writes it with `--write`.
 *
 * Archives are read with a central-directory reader for zip and a header walk
 * for tar.gz, both about thirty lines, rather than with a dependency. This tool
 * runs in the contracts job, which has no `npm install`.
 *
 * Usage:
 *   node tools/sidecar-pin/pin.mjs \
 *     --repo ggml-org/llama.cpp --release b9763 \
 *     --asset llama-b9763-bin-win-cpu-x64.zip \
 *     --target windows-x86_64 --executable llama-server
 *
 *   node tools/sidecar-pin/pin.mjs --verify        # re-measure every pinned target
 *
 * `--verify` is the reason this is worth having beyond the first pin: it proves
 * the bytes at a URL still hash to what is committed. A release retagged in
 * place is otherwise invisible.
 */

import { createHash } from "node:crypto";
import { createWriteStream, readFileSync, writeFileSync, rmSync, statSync } from "node:fs";
import { open } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { pipeline } from "node:stream/promises";
import { gunzipSync } from "node:zlib";

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
 */
async function download(url, label) {
  const target = join(tmpdir(), `sidecar-pin-${process.pid}-${label}`);
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

    const hash = createHash("sha256");
    const out = createWriteStream(target);
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

  die(`${url} redirected more than five times`);
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

    const names = [];
    let offset = 0;
    while (offset < directory.length && names.length < count) {
      if (directory.readUInt32LE(offset) !== 0x0201_4b50) break;
      const nameLength = directory.readUInt16LE(offset + 28);
      const extraLength = directory.readUInt16LE(offset + 30);
      const commentLength = directory.readUInt16LE(offset + 32);
      names.push(directory.toString("utf8", offset + 46, offset + 46 + nameLength));
      offset += 46 + nameLength + extraLength + commentLength;
    }
    return names;
  } finally {
    await handle.close();
  }
}

/** Entry names in a tar.gz, by walking 512-byte headers. */
function tarGzEntries(path) {
  const tar = gunzipSync(readFileSync(path));
  const names = [];

  for (let offset = 0; offset + 512 <= tar.length; ) {
    const name = tar.toString("utf8", offset, offset + 100).replace(/\0.*$/, "");
    if (name === "") break; // Two zero blocks end the archive.

    const sizeField = tar.toString("utf8", offset + 124, offset + 136).replace(/\0.*$/, "").trim();
    const size = Number.parseInt(sizeField, 8) || 0;

    // A `pax` or `gnu` long-name record describes the *next* entry rather than
    // being one, so it is skipped instead of listed.
    const typeFlag = tar.toString("utf8", offset + 156, offset + 157);
    if (!["x", "g", "L", "K"].includes(typeFlag)) {
      names.push(name);
    }

    offset += 512 + Math.ceil(size / 512) * 512;
  }
  return names;
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
  const matches = entries.filter((e) => {
    const base = e.split("/").pop();
    return wanted.includes(base);
  });

  if (matches.length === 0) {
    die(
      `no entry named ${wanted.join(" or ")} in the archive. It contains:\n  ` +
        entries.slice(0, 40).join("\n  ")
    );
  }
  if (matches.length > 1) {
    die(`the archive has ${matches.length} candidates and no way to choose: ${matches.join(", ")}`);
  }
  return matches[0];
}

/** `zip` or `tar-gz`, from the asset name. */
function archiveKind(asset) {
  if (asset.endsWith(".zip")) return "zip";
  if (asset.endsWith(".tar.gz") || asset.endsWith(".tgz")) return "tar-gz";
  return die(`${asset} is neither a .zip nor a .tar.gz, so its index cannot be read`);
}

async function entriesOf(path, kind) {
  const entries = kind === "zip" ? await zipEntries(path) : tarGzEntries(path);
  if (entries === null) {
    die("the archive index could not be read (zip64 archives are not supported)");
  }
  return entries;
}

/* ---------------------------------------------------------------- measuring */

async function measure({ repo, release, asset, executable }) {
  const url = `https://github.com/${repo}/releases/download/${release}/${asset}`;
  const kind = archiveKind(asset);

  console.error(`  downloading ${asset} …`);
  const measured = await download(url, asset);

  try {
    const entries = await entriesOf(measured.path, kind);
    const executableInArchive = findExecutable(entries, executable);

    return {
      availability: "published",
      url,
      archive: kind,
      sha256: measured.sha256,
      sizeBytes: measured.sizeBytes,
      executableInArchive,
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
 */
async function verify(only) {
  const manifest = JSON.parse(readFileSync(MANIFEST, "utf8"));
  let checked = 0;
  const wrong = [];

  for (const sidecar of manifest.sidecars) {
    if (only && sidecar.name !== only) continue;
    for (const [target, spec] of Object.entries(sidecar.targets)) {
      if (spec.availability !== "published") continue;
      console.error(`${sidecar.name} / ${target}`);

      const measured = await download(spec.url, `${sidecar.name}-${target}`);
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
        } else {
          const entries = await entriesOf(measured.path, spec.archive);
          if (!entries.includes(spec.executableInArchive)) {
            wrong.push(
              `${sidecar.name}/${target}: the archive has no ${spec.executableInArchive}`
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
    process.exit(1);
  }
  console.log(`sidecar-pin --verify: ${checked} target(s) match the committed pins.`);
}

/* --------------------------------------------------------------------- main */

const options = args(process.argv.slice(2));

if (options.verify) {
  await verify(typeof options.sidecar === "string" ? options.sidecar : null);
} else {
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
