/**
 * A scripted HTTP server on 127.0.0.1, for testing the pin tools' downloads
 * against real `fetch` rather than a stub of it (KNEG-413).
 *
 * The failure these tests exist for came out of undici on a GitHub runner:
 * `TypeError: fetch failed`, cause `SocketError: other side closed`. A stubbed
 * `fetch` would only ever throw the error its author imagined. A socket this
 * server destroys makes Node throw the real one, in the shape the retry has to
 * recognise, on whatever Node version runs the test.
 *
 * Each path has a list of handlers. The nth request to a path gets the nth
 * handler, and the last one repeats, so `[drop(), body(bytes)]` is "fail once,
 * then serve". Not a test file itself: `node --test tools/*\/*.test.mjs` only
 * picks up `*.test.mjs`.
 */

import { createServer } from "node:http";

export async function scriptedServer(routes) {
  const counts = new Map();
  const server = createServer((req, res) => {
    const path = req.url ?? "/";
    const list = routes[path];
    const n = counts.get(path) ?? 0;
    counts.set(path, n + 1);
    if (!list || list.length === 0) {
      res.statusCode = 404;
      res.end();
      return;
    }
    list[Math.min(n, list.length - 1)](req, res);
  });
  await new Promise((done) => server.listen(0, "127.0.0.1", done));
  const { port } = server.address();
  return {
    url: (path) => `http://127.0.0.1:${port}${path}`,
    requests: (path) => counts.get(path) ?? 0,
    close: () =>
      new Promise((done) => {
        server.closeAllConnections();
        server.close(() => done());
      }),
  };
}

/** Serve `bytes` whole. */
export const body = (bytes) => (_req, res) => {
  res.writeHead(200, { "content-length": bytes.length });
  res.end(bytes);
};

/** Close the connection before any response headers: the 2026-10-05 shape. */
export const dropBeforeHeaders = () => (req) => {
  req.socket.destroy();
};

/** Promise `bytes.length` bytes, send half, then close the connection. */
export const dropMidBody = (bytes) => (_req, res) => {
  res.writeHead(200, { "content-length": bytes.length });
  res.write(bytes.subarray(0, Math.floor(bytes.length / 2)), () => res.socket.destroy());
};

export const status = (code) => (_req, res) => {
  res.statusCode = code;
  res.end();
};

export const redirect = (location) => (_req, res) => {
  res.writeHead(302, { location });
  res.end();
};

/** Deterministic binary bytes. A pin hashes bytes, so no text encoding is involved. */
export function bytesOf(length) {
  const bytes = Buffer.alloc(length);
  for (let i = 0; i < length; i += 1) bytes[i] = (i * 31 + 7) % 256;
  return bytes;
}

/** A port nothing listens on: bound once, then released. */
export async function closedPort() {
  const server = createServer();
  await new Promise((done) => server.listen(0, "127.0.0.1", done));
  const { port } = server.address();
  await new Promise((done) => server.close(() => done()));
  return port;
}
