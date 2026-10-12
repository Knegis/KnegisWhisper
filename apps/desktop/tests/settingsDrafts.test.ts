import assert from "node:assert/strict";
import test from "node:test";
import { SettingsDraftRegistry } from "../src/lib/settingsDrafts.js";

test("navigation waits for an automatic write and observes its acknowledgment", async () => {
  const registry = new SettingsDraftRegistry(); let dirty = true; let release!: () => void;
  const write = new Promise<void>(resolve => { release = () => { dirty = false; resolve(); }; });
  registry.register("form", { label: "Language", dirty: () => dirty, settle: () => write, save: async () => { throw new Error("Shouldn't submit twice"); }, discard: () => { dirty = false; } });
  let settled = false; const waiting = registry.settle().then(() => { settled = true; });
  await Promise.resolve(); assert.equal(settled, false); release(); await waiting;
  assert.equal(registry.dirty().length, 0);
});
test("partial Save preserves failed drafts without resubmitting committed groups", async () => {
  const registry = new SettingsDraftRegistry(); let first = true, second = true, attempts = 0, fail = true;
  registry.register("first", { label: "Summary", dirty: () => first, settle: async () => {}, save: async () => { attempts++; first = false; }, discard: () => { first = false; } });
  registry.register("second", { label: "Key", dirty: () => second, settle: async () => {}, save: async () => { if (fail) throw new Error("write failed"); second = false; }, discard: () => { second = false; } });
  await assert.rejects(registry.save(), /write failed/); assert.equal(first, false); assert.equal(second, true);
  fail = false; await registry.save(); assert.equal(attempts, 1); assert.equal(registry.dirty().length, 0);
});
test("a new edit during Save keeps navigation blocked", async () => {
  const registry = new SettingsDraftRegistry(); let dirty = true;
  registry.register("form", { label: "Summary", dirty: () => dirty, settle: async () => {}, save: async () => { dirty = true; }, discard: () => { dirty = false; } });
  await assert.rejects(registry.save(), /still have unsaved/);
  await registry.discard(); assert.equal(registry.dirty().length, 0);
});
test("discard waits for writes and only discards remaining drafts; unmounted forms unregister", async () => {
  const registry = new SettingsDraftRegistry(); const events: string[] = []; let dirty = true;
  const unregister = registry.register("form", { label: "Calendar", dirty: () => dirty, settle: async () => { events.push("settle"); }, save: async () => {}, discard: () => { events.push("discard"); dirty = false; } });
  await registry.discard(); assert.deepEqual(events, ["settle", "discard"]);
  unregister(); assert.equal(registry.dirty().length, 0);
});

test("Save follows screen order even when child effects register first", async () => {
  const registry = new SettingsDraftRegistry(); const events: string[] = [];
  const dirty = new Set(["company", "cleanup", "summary"]);
  for (const [label, order] of [["company", 20], ["cleanup", 10], ["summary", 0]] as const) registry.register(label, {
    label, order, dirty: () => dirty.has(label), settle: async () => {},
    save: async () => { events.push(label); dirty.delete(label); }, discard: () => { dirty.delete(label); },
  });
  await registry.save(); assert.deepEqual(events, ["summary", "cleanup", "company"]);
});
