/** Covers request correlation and clip-coordinate arithmetic in probe helpers. */
import test from "node:test";
import assert from "node:assert/strict";
import { CdpClient } from "./cdp.mjs";
import { clipRail } from "./shell-rail.mjs";

class FakeWebSocket {
  static OPEN = 1;

  constructor() {
    this.readyState = FakeWebSocket.OPEN;
    this.listeners = new Map();
    this.sent = [];
    queueMicrotask(() => this.emit("open", {}));
  }

  addEventListener(type, listener, options = {}) {
    const list = this.listeners.get(type) ?? [];
    list.push({ listener, once: options.once });
    this.listeners.set(type, list);
  }

  emit(type, event) {
    const list = this.listeners.get(type) ?? [];
    this.listeners.set(type, list.filter((entry) => !entry.once));
    for (const entry of list) entry.listener(event);
  }

  send(value) {
    this.sent.push(JSON.parse(value));
  }

  close() {
    this.readyState = 3;
    this.emit("close", {});
  }
}

async function client() {
  const cdp = new CdpClient("ws://loopback", { WebSocketImpl: FakeWebSocket });
  await cdp.connect();
  return cdp;
}

function reply(cdp, message) {
  cdp.socket.emit("message", { data: JSON.stringify(message) });
}

test("correlates replies by id and dispatches events", async () => {
  const cdp = await client();
  let event;
  cdp.on("Page.loadEventFired", (params) => {
    event = params;
  });
  const first = cdp.send("One");
  const second = cdp.send("Two");
  reply(cdp, { id: 2, result: { two: true } });
  reply(cdp, {
    method: "Page.loadEventFired",
    params: { loaded: true },
  });
  reply(cdp, { id: 1, result: { one: true } });
  assert.deepEqual(await first, { one: true });
  assert.deepEqual(await second, { two: true });
  assert.deepEqual(event, { loaded: true });
});

test("evaluate rejects exception details", async () => {
  const cdp = await client();
  const value = cdp.evaluate("throw new Error()");
  reply(cdp, { id: 1, result: { exceptionDetails: { text: "renderer boom" } } });
  await assert.rejects(value, /renderer boom/);
});

test("a close rejects every pending request", async () => {
  const cdp = await client();
  const one = cdp.send("One");
  const two = cdp.send("Two");
  cdp.socket.close();
  await assert.rejects(one, /closed/);
  await assert.rejects(two, /closed/);
});

test("rail clips translate viewport geometry into page coordinates", () => {
  assert.deepEqual(
    clipRail({
      rect: { left: 0, top: 12, width: 182, height: 708 },
      scrollX: 40,
      scrollY: 600,
    }),
    { x: 40, y: 612, width: 182, height: 708 },
  );
});
