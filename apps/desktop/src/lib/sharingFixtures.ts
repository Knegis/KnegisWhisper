/** Browser-only invented sharing data, composed below the real IPC wrappers. */
import type { Folder, Note } from "./ipc";
import type { ShareOptions, SharePreview, ShareStatus } from "../generated/sharing-types";

export function installSharingFixtures(): void {
  const internals = (window as unknown as { __TAURI_INTERNALS__: { invoke: (command: string, args?: Record<string, unknown>) => Promise<unknown> } }).__TAURI_INTERNALS__;
  const original = internals.invoke.bind(internals);
  let status: ShareStatus | null = null;
  let sequence = 0;
  internals.invoke = async (command, args = {}) => {
    if (!command.startsWith("folder_share_")) return original(command, args);
    if (command === "folder_share_status") return status;
    if (command === "folder_share_preview") {
      const folders = await original("folders_list") as Folder[];
      const folder = folders.find(value => value.id === args.folderId);
      const notes = await original("notes_list", { folderId: args.folderId }) as Note[];
      return { folderName: folder?.name ?? "Shared folder", notes: notes.map(note => ({ id: note.id, title: note.title, recordingBytes: 320044, recordingMissing: false, textBytes: 1024 })), options: { personalNotes: true, summaries: true, transcripts: true, recordings: true }, totalBytes: notes.length * 321068 } satisfies SharePreview;
    }
    if (command === "folder_share_export" || command === "folder_share_pick_import") {
      const preview = command === "folder_share_pick_import" ? { folderName: "Shared project", notes: [{ id: 0, title: "Project meeting Åäö", recordingBytes: 320044, recordingMissing: false, textBytes: 956 }], options: { personalNotes: true, summaries: true, transcripts: true, recordings: true }, totalBytes: 321000 } satisfies SharePreview : null;
      const operationId = `fixture-sharing-${++sequence}`;
      const slow = !preview && new URLSearchParams(window.location.search).has("sharingSlow");
      status = { operationId, direction: preview ? "import" : "export", stage: preview ? "awaiting-confirmation" : slow ? "copying" : "succeeded", bytesProcessed: 320044, preview, folderId: null, savedTo: preview || slow ? null : "C:\\Exports\\Project.knegis", error: null };
      if (!preview) status.preview = { folderName: "Fixture export", notes: [], options: args.options as ShareOptions, totalBytes: 0 };
      if (slow) setTimeout(() => {
        if (status?.operationId === operationId && status.stage === "copying") status = { ...status, stage: "succeeded", savedTo: "C:\\Exports\\Project.knegis" };
      }, 4000);
      return status.operationId;
    }
    if (command === "folder_share_import") {
      const folderId = await original("folder_create", { name: args.folderName }) as number;
      await original("notes_create", { request: { title: "Project meeting Åäö", content: "Invented shared personal notes", folderId, noteType: "personal" } });
      if (status) { status = { ...status, stage: "succeeded", folderId }; }
      return null;
    }
    if (command === "folder_share_cancel") { if (status) status = { ...status, stage: "cancelled" }; return null; }
    if (command === "folder_share_dismiss") { status = null; return null; }
    if (command === "folder_share_show_export") return null;
    throw new Error(`No sharing fixture for ${command}`);
  };
}
