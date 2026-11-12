import assert from "node:assert/strict";
import { test } from "node:test";
import { get } from "node:http";
import { mkdtemp, mkdir, writeFile, rm, symlink } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, resolve, dirname } from "node:path";
import { serve } from "./design-server.mjs";

function request(port, path) {
  return new Promise((done, reject) => {
    get({ hostname: "127.0.0.1", port, path }, (response) => {
      let body = "";
      response.setEncoding("utf8");
      response.on("data", (chunk) => { body += chunk; });
      response.on("end", () => done({ status: response.statusCode, body }));
    }).on("error", reject);
  });
}

test("the comparison server contains decoded paths and symlinks while serving chosen files", async () => {
  const root = await mkdtemp(join(tmpdir(), "kw-design-server-"));
  const boards = join(root, "boards");
  const out = join(root, "shots");
  const outside = join(root, "boards-other");
  const runtime = join(root, "runtime.js");
  let server;
  try {
    for (const dir of [boards, out, outside, join(boards, "nested")]) await mkdir(dir);
    await writeFile(runtime, "selected runtime");
    await writeFile(join(root, "secret.txt"), "outside marker");
    await writeFile(join(outside, "secret.txt"), "outside marker");
    await writeFile(join(boards, "Dictation.dc.html"), "board");
    await writeFile(join(boards, "nested", "Å space.js"), "nested asset");
    await writeFile(join(out, "view.png"), "screenshot");
    // Windows junction creation requires no symlink privilege.
    for (const dir of [boards, out]) await symlink(outside, join(dir, "escape"), process.platform === "win32" ? "junction" : "dir");
    const started = await serve({ boards, runtime, out });
    server = started.server;
    const { port } = started;
    for (const [path, body] of [
      ["/Dictation.dc.html", "board"],
      ["/nested/%C3%85%20space.js", "nested asset"],
      ["/shots/view.png", "screenshot"],
      ["/support.js", "selected runtime"],
      ["/nested/support.js", "selected runtime"],
    ]) assert.deepEqual(await request(port, path), { status: 200, body });
    for (const path of [
      "/..%2fsecret.txt", "/%2e%2e%2Fsecret.txt", "/..%5csecret.txt",
      "/shots/..%2fsecret.txt", "/shots/..%2f..%2fsecret.txt",
      "/shots%2f..%2f..%2fsecret.txt", "/..%2fboards-other%2fsecret.txt",
      "/shots/%2fsecret.txt", "/C:%5csecret.txt", "/%5c%5cserver%5cshare",
      "/escape/secret.txt", "/shots/escape/secret.txt", "/bad%00name", "/bad%ZZ",
    ]) {
      const reply = await request(port, path);
      assert.equal(reply.status, 404, path);
      assert.ok(!reply.body.includes("outside marker"), path);
    }
    const pair = await request(port, "/pair?a=view.png&b=%22%3E%3Cscript%3E");
    assert.equal(pair.status, 200);
    assert.ok(pair.body.includes('/shots/view.png'));
    assert.ok(!pair.body.includes("<script>"));
  } finally {
    if (server) await new Promise((done, reject) => server.close((err) => err ? reject(err) : done()));
    assert.equal(dirname(resolve(root)), resolve(tmpdir()));
    await rm(root, { recursive: true, force: true });
  }
});
