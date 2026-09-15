import assert from "node:assert/strict";
import test from "node:test";
import { carriesFiles, installFileDropGuard } from "../src/lib/fileDrops.js";

/** Records what was installed, and lets a test fire it with a made-up event. */
function fakeWindow() {
  const listeners = new Map<string, (event: DragEvent) => void>();
  const target = {
    addEventListener(type: string, listener: (event: DragEvent) => void) {
      listeners.set(type, listener);
    },
  };
  const fire = (type: string, types: string[]) => {
    let prevented = false;
    const event = {
      dataTransfer: { types },
      preventDefault: () => {
        prevented = true;
      },
    } as unknown as DragEvent;
    listeners.get(type)?.(event);
    return prevented;
  };
  return { target, listeners, fire };
}

test("a drag carrying files is recognised, and a drag carrying page content is not", () => {
  assert.equal(carriesFiles(["Files"]), true);
  assert.equal(carriesFiles(["text/plain", "Files"]), true);
  // What @milkdown/plugin-block puts on the drag when a block is moved.
  assert.equal(carriesFiles(["text/html", "text/plain"]), false);
  assert.equal(carriesFiles([]), false);
  assert.equal(carriesFiles(undefined), false);
});

test("the guard covers both events, because preventing one still navigates", () => {
  const { target, listeners } = fakeWindow();
  installFileDropGuard(target);
  assert.deepEqual([...listeners.keys()].sort(), ["dragover", "drop"]);
});

test("a file drop is refused on both events", () => {
  const { target, fire } = fakeWindow();
  installFileDropGuard(target);
  assert.equal(fire("dragover", ["Files"]), true);
  assert.equal(fire("drop", ["Files"]), true);
});

test("a drag inside the page is left alone, which is the whole point", () => {
  // KNEG-258: the guard exists because the window gave Tauri's drag and drop
  // up so the editor's block handle could drag. A guard that prevented the
  // default on the editor's own drop would have taken the fix back out again.
  const { target, fire } = fakeWindow();
  installFileDropGuard(target);
  assert.equal(fire("dragover", ["text/html", "text/plain"]), false);
  assert.equal(fire("drop", ["text/html", "text/plain"]), false);
});
