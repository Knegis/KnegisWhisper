/**
 * A bounded retry for the pin tools' network reads (KNEG-413).
 *
 * On 2026-10-05 the weekly pin check on `main` died on one dropped socket:
 * `TypeError: fetch failed`, cause `SocketError: other side closed`, while
 * `sidecar-pin --verify` was fetching its 19th of 21 targets. Nothing retried,
 * the error was never caught, and the two steps after it were skipped, so a
 * week of checking was lost to one connection GitHub closed.
 *
 * **What is retried is the transport, never the answer.** A retry runs only
 * when the request or its body failed to arrive: a socket closed, a timeout, a
 * connection reset, or a status that means "try again" (408, 429, 5xx). A
 * download that arrives whole is hashed and compared by the caller after this
 * returns, so bytes that disagree with a pin are never retried into agreeing.
 * Each attempt starts again from byte 0; see `model-pin`'s header for why a
 * partial download is never resumed.
 *
 * **What is not retried:** a 403 or 404, a redirect with no Location, a
 * redirect loop, a refused connection, a name that does not resolve, and any
 * error from the disk -- `ENOSPC` and `EACCES` come out of the same `pipeline`
 * as a dropped socket, and retrying a full disk only makes the log longer.
 * Codes are matched from an allowlist, walked down the `cause` chain, because
 * undici reports a dropped socket two levels down and some of its own codes
 * (`UND_ERR_INVALID_ARG`) are as permanent as errors get.
 *
 * Shared by `tools/model-pin/pin.mjs` and `tools/sidecar-pin/pin.mjs`, which
 * imports it from here.
 */

import { once } from "node:events";
import { rmSync } from "node:fs";

/** An HTTP status that was not a success, with the URL that answered it. */
export class HttpError extends Error {
  constructor(status, url, message = `${url} returned HTTP ${status}`) {
    super(message);
    this.name = "HttpError";
    this.status = status;
    this.url = url;
  }
}

/**
 * Network failures that a second attempt can cure. Measured on Node 22: a socket
 * closed before the headers is `fetch failed` with a `UND_ERR_SOCKET` cause, and
 * one closed mid-body is `terminated` with the same cause.
 */
const TRANSIENT_CODES = new Set([
  "UND_ERR_SOCKET",
  "UND_ERR_CONNECT_TIMEOUT",
  "UND_ERR_HEADERS_TIMEOUT",
  "UND_ERR_BODY_TIMEOUT",
  "UND_ERR_CLOSED",
  "ECONNRESET",
  "ETIMEDOUT",
  "EPIPE",
  "EAI_AGAIN",
]);

const TRANSIENT_STATUSES = new Set([408, 429, 500, 502, 503, 504]);

/** Two retries, five and fifteen seconds after the failures they follow. */
export const DELAYS = [5_000, 15_000];

/** Every error reachable from `error` through `cause` and `AggregateError.errors`, once each. */
function* chain(error) {
  const seen = new Set();
  const pending = [error];
  while (pending.length > 0) {
    const next = pending.shift();
    if (next === null || typeof next !== "object" || seen.has(next)) continue;
    seen.add(next);
    yield next;
    if (next.cause !== undefined) pending.push(next.cause);
    if (Array.isArray(next.errors)) pending.push(...next.errors);
  }
}

/** Whether another attempt could succeed where this one failed. */
export function isTransient(error) {
  for (const link of chain(error)) {
    if (link instanceof HttpError) return TRANSIENT_STATUSES.has(link.status);
    if (typeof link.code === "string" && TRANSIENT_CODES.has(link.code)) return true;
  }
  return false;
}

/**
 * One line for a log: the outer message, and the innermost cause with its code.
 * `fetch failed` on its own says nothing; `fetch failed (UND_ERR_SOCKET: other
 * side closed)` says what the runner saw.
 */
export function describe(error) {
  const links = [...chain(error)];
  const outer = links[0];
  if (!outer) return String(error);
  const inner = links.length > 1 ? links[links.length - 1] : null;
  if (!inner) return outer.message;
  const code = typeof inner.code === "string" ? `${inner.code}: ` : "";
  return `${outer.message} (${code}${inner.message})`;
}

const wait = (ms) => new Promise((done) => setTimeout(done, ms));

/**
 * Run `attempt` until it succeeds, it fails permanently, or the delays run out.
 *
 * One attempt per delay, plus the first: `[5000, 15000]` is three attempts.
 * `onRetry` hears about each retry before its sleep, with the attempt that comes
 * `next`, the number there will be `of` in all, the `error` being retried and
 * the `delay`. The last error is thrown as it was, so a caller reports the real
 * failure rather than a wrapper around it.
 */
export async function retrying(attempt, { delays = DELAYS, sleep = wait, onRetry = () => {} } = {}) {
  for (let made = 1; ; made += 1) {
    try {
      return await attempt(made);
    } catch (error) {
      const delay = delays[made - 1];
      if (delay === undefined || !isTransient(error)) throw error;
      onRetry({ next: made + 1, of: delays.length + 1, error, delay });
      await sleep(delay);
    }
  }
}

/**
 * The log line for one retry. Names the URL the caller asked for, never the
 * signed URL a redirect led to: `model-pin` cuts that down to its host on
 * purpose, and a retry line is no reason to print an expiring signature.
 */
export function retryLine(url, { next, of, error, delay }, { fromZero = false } = {}) {
  const restart = fromZero ? ", restarting from byte 0" : "";
  return `retrying ${url} in ${delay / 1000} s (attempt ${next} of ${of}${restart}): ${describe(error)}`;
}

/* ------------------------------------------- for the attempts that use this */

/** Let go of a response whose body will not be read, so its socket is freed. */
export async function discard(response) {
  await response.body?.cancel().catch(() => {});
}

/**
 * Remove what a failed download attempt wrote, so the next attempt starts from
 * nothing and a run that gives up leaves nothing behind. The stream is closed
 * first: Windows refuses to delete a file that still has a handle open on it.
 */
export async function removePartial(out, path) {
  if (out && !out.closed) {
    out.destroy();
    await once(out, "close").catch(() => {});
  }
  rmSync(path, { force: true });
}
