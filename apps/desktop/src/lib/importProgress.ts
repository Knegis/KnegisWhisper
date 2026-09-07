import type { ImportSnapshot } from "../generated/ipc-types.js";

export function importActive(snapshot: ImportSnapshot | null): boolean {
  return snapshot !== null && snapshot.stage !== "succeeded" && snapshot.stage !== "failed";
}
export function importProgressText(snapshot: ImportSnapshot): string {
  const labels = { preparing: "Preparing import", converting: "Converting audio", transcribing: "Transcribing", saving: "Saving note and recording", succeeded: "Import complete", failed: "Import failed" };
  const seconds = Math.max(0, Math.floor(snapshot.elapsedSeconds));
  return `${labels[snapshot.stage]} · ${Math.floor(seconds / 60)}:${String(seconds % 60).padStart(2, "0")} elapsed`;
}
// One request at a time; a disposed view can never apply a late response.
export function pollImport<T>(read: () => Promise<T>, receive: (value: T) => void, fail: (error: unknown) => void,
  schedule: (fn: () => void) => () => void = (fn) => { const id = setTimeout(fn, 1000); return () => clearTimeout(id); },
): () => void {
  let alive = true;
  let cancel: (() => void) | undefined;
  const tick = async () => {
    try { const value = await read(); if (alive) receive(value); }
    catch (error) { if (alive) fail(error); }
    finally { if (alive) cancel = schedule(() => { void tick(); }); }
  };
  void tick();
  return () => { alive = false; cancel?.(); };
}
