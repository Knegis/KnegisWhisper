import type { ImportSnapshot } from "../generated/ipc-types.js";

export function importActive(snapshot: ImportSnapshot | null): boolean {
  return snapshot !== null && snapshot.stage !== "succeeded" && snapshot.stage !== "failed" && snapshot.stage !== "cancelled" && snapshot.stage !== "interrupted";
}
export function importProgressText(snapshot: ImportSnapshot): string {
  if (snapshot.stage === "interrupted") return "Import interrupted";
  const labels = { preparing: "Preparing import", converting: "Converting audio", transcribing: "Transcribing", saving: "Saving note and recording", succeeded: "Import complete", failed: "Import failed", cancelled: "Import cancelled" };
  const seconds = Math.max(0, Math.floor(snapshot.elapsedSeconds));
  return `${labels[snapshot.stage]} · ${Math.floor(seconds / 60)}:${String(seconds % 60).padStart(2, "0")} elapsed`;
}
const STAGES = [
  ["preparing", "Preparing import"],
  ["converting", "Converting audio"],
  ["transcribing", "Transcribing"],
  ["saving", "Saving note and recording"],
] as const;

export type ImportStage = { number: string; label: string; state: "done" | "current" | "todo" };

/**
 * The import's four stages, as a row of pills, while one runs. `null` otherwise: a
 * failed import does not say which stage it reached, so it draws none rather than
 * a guess, and a finished one has its own line.
 */
export function importStages(snapshot: ImportSnapshot | null): ImportStage[] | null {
  if (snapshot === null || !importActive(snapshot)) return null;
  const at = STAGES.findIndex(([stage]) => stage === snapshot.stage);
  return STAGES.map(([, label], index) => ({
    number: String(index + 1).padStart(2, "0"),
    label,
    state: index < at ? "done" : index === at ? "current" : "todo",
  }));
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
