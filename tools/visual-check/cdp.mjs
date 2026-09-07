/** Provides the small dependency-free CDP client used by visual probes. */
import { writeFile } from "node:fs/promises";

export class CdpClient {
  constructor(url, { WebSocketImpl = WebSocket } = {}) {
    this.url = url;
    this.WebSocketImpl = WebSocketImpl;
    this.socket = null;
    this.nextId = 1;
    this.pending = new Map();
    this.handlers = new Map();
  }

  async connect() {
    if (this.socket) return this;
    const socket = new this.WebSocketImpl(this.url);
    this.socket = socket;
    // Chromium may close an unavailable target immediately after opening it.
    // Install terminal listeners before waiting so that race rejects the caller
    // instead of leaving an orphaned request or a hanging probe.
    socket.addEventListener("message", (event) => this.#message(event));
    socket.addEventListener("close", () => {
      this.#rejectPending(new Error("CDP connection closed"));
    });
    await new Promise((resolveConnect, reject) => {
      socket.addEventListener("open", resolveConnect, { once: true });
      socket.addEventListener(
        "error",
        () => reject(new Error(`CDP connection failed: ${this.url}`)),
        { once: true },
      );
    });
    return this;
  }

  #message(event) {
    const message = JSON.parse(
      typeof event.data === "string" ? event.data : String(event.data),
    );
    if (message.id !== undefined) {
      const pending = this.pending.get(message.id);
      if (!pending) return;
      this.pending.delete(message.id);
      if (message.error) {
        pending.reject(
          new Error(
            `CDP ${pending.method}: ${message.error.message ?? JSON.stringify(message.error)}`,
          ),
        );
      } else {
        pending.resolve(message.result);
      }
      return;
    }
    if (message.method) {
      for (const handler of this.handlers.get(message.method) ?? []) {
        handler(message.params);
      }
    }
  }

  #rejectPending(error) {
    for (const { reject } of this.pending.values()) reject(error);
    this.pending.clear();
  }

  send(method, params = {}) {
    if (!this.socket || this.socket.readyState !== (this.WebSocketImpl.OPEN ?? 1)) {
      return Promise.reject(new Error("CDP is not connected"));
    }
    const id = this.nextId++;
    return new Promise((resolveRequest, reject) => {
      this.pending.set(id, { resolve: resolveRequest, reject, method });
      this.socket.send(JSON.stringify({ id, method, params }));
    });
  }

  on(method, handler) {
    const handlers = this.handlers.get(method) ?? [];
    handlers.push(handler);
    this.handlers.set(method, handlers);
    return () => {
      this.handlers.set(method, handlers.filter((item) => item !== handler));
    };
  }

  async evaluate(expression) {
    const result = await this.send("Runtime.evaluate", {
      expression,
      awaitPromise: true,
      returnByValue: true,
    });
    if (result.exceptionDetails) {
      const text = result.exceptionDetails.exception?.description
        ?? result.exceptionDetails.text
        ?? "Runtime evaluation failed";
      throw new Error(text);
    }
    return result.result?.value;
  }

  async screenshot(path, { clip, scale = 1 } = {}) {
    const result = await this.send("Page.captureScreenshot", {
      format: "png",
      ...(clip ? { clip: { ...clip, scale } } : {}),
      captureBeyondViewport: false,
    });
    await writeFile(path, Buffer.from(result.data, "base64"));
    return path;
  }

  async close() {
    if (!this.socket) return;
    this.#rejectPending(new Error("CDP client closed"));
    this.socket.close();
    this.socket = null;
  }
}

export async function connectCdp(url, options) {
  const client = new CdpClient(url, options);
  await client.connect();
  return client;
}
