export function sharingActive(stage: string): boolean {
  return !["succeeded", "failed", "cancelled", "interrupted"].includes(stage);
}
export function sharingStage(stage: string): string {
  const labels: Record<string, string> = { preparing: "Preparing sharing…", checking: "Checking shared folder…", copying: "Copying notes and recordings…", saving: "Finishing…", "awaiting-confirmation": "Ready to import", cancelling: "Cancelling…", succeeded: "Finished", failed: "Sharing failed", cancelled: "Sharing cancelled", interrupted: "Sharing was interrupted. Please try again." };
  return labels[stage] ?? "Working…";
}
export function sharingBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  const power = Math.min(Math.floor(Math.log(bytes) / Math.log(1024)), 3);
  return `${(bytes / 1024 ** power).toFixed(1)} ${["B", "KiB", "MiB", "GiB"][power]}`;
}
