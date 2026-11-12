import { createServer } from "node:http";
import { readFile, realpath } from "node:fs/promises";
import { isAbsolute, relative, resolve } from "node:path";

function contained(root, file) {
  const path = relative(root, file);
  return path !== ".." && !path.startsWith("../") && !path.startsWith("..\\") && !isAbsolute(path);
}

async function contentFile(root, name) {
  // Reject Windows separators, drive names, streams and NUL on every host.
  if (/[\\:\0]/u.test(name)) throw new Error("invalid file name");
  const candidate = resolve(root, name);
  if (!contained(root, candidate)) throw new Error("outside content root");
  const canonical = await realpath(candidate);
  if (!contained(root, canonical)) throw new Error("outside content root");
  return canonical;
}

/** Serve files inside the chosen roots, plus the explicitly chosen runtime. */
export async function serve({ boards, runtime, out }) {
  const boardRoot = await realpath(boards);
  const shotRoot = await realpath(out);
  const runtimeFile = await realpath(runtime);
  const types = { ".html": "text/html; charset=utf-8", ".js": "text/javascript", ".png": "image/png" };
  const server = createServer(async (request, response) => {
    try {
      const url = new URL(request.url, "http://x");
      const path = decodeURIComponent(url.pathname);
      if (path === "/pair") {
        // Encode each complete file name so query text cannot become HTML.
        const a = encodeURIComponent(url.searchParams.get("a") ?? "");
        const b = encodeURIComponent(url.searchParams.get("b") ?? "");
        response.writeHead(200, { "content-type": types[".html"] });
        response.end('<!doctype html><meta charset="utf-8"><style>' +
          'body{margin:0;background:#2a2a2e;color:#fff;font:14px Arial}' +
          '.p{display:flex;gap:16px;padding:12px;align-items:flex-start}' +
          'figure{margin:0}figcaption{padding:0 0 8px}img{display:block}' +
          '</style><div class="p">' +
          '<figure><figcaption>Approved design</figcaption><img src="/shots/' + a + '"></figure>' +
          '<figure><figcaption>App</figcaption><img src="/shots/' + b + '"></figure></div>');
        return;
      }
      let file;
      if (path.endsWith("/support.js")) file = runtimeFile;
      else if (path.startsWith("/shots/")) file = await contentFile(shotRoot, path.slice("/shots/".length));
      else file = await contentFile(boardRoot, path.slice(1));
      const body = await readFile(file);
      const type = types[file.slice(file.lastIndexOf("."))] ?? "application/octet-stream";
      response.writeHead(200, { "content-type": type });
      response.end(body);
    } catch {
      response.writeHead(404);
      response.end("not found");
    }
  });
  return new Promise((done, reject) => {
    server.once("error", reject);
    server.listen(0, "127.0.0.1", () => done({ server, port: server.address().port }));
  });
}
