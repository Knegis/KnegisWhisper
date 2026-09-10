/** Pure presentation shared by the floating button and its regression tests. */
export function microphoneState(state: string | null, unavailable: boolean) {
  const label = state === "recording" ? "Stop dictation" : state === "transcribing" ? "Transcribing" : state === "inserting" ? "Inserting" : state === "idle" ? "Start dictation" : unavailable ? "Status unavailable" : "Loading status";
  return { label, recording: state === "recording", disabled: state !== "idle" && state !== "recording", busy: state === "transcribing" || state === "inserting" };
}
export function isMicrophoneDrag(x: number, y: number, startX: number, startY: number) {
  return Math.hypot(x - startX, y - startY) >= 6;
}
