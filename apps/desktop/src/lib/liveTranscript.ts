/**
 * The local provider emits overlapping windows, so consecutive live rows can
 * repeat words. The saved note uses those same partials only if the whole-file
 * pass fails; its normal transcript comes from that pass. This list is a live
 * aid, not the record.
 */
import type { AudioSource, TranscriptEvent } from "../generated/ipc-types.js";

export type LiveRow = { id: number; source: AudioSource; startSeconds: number; speaker: string | null; text: string; committed: boolean };
export type LiveTranscript = { rows: LiveRow[]; partial: Partial<Record<AudioSource, LiveRow>>; nextId: number };
export const LIVE_ROW_LIMIT = 500;

export function emptyLiveTranscript(): LiveTranscript {
  return { rows: [], partial: {}, nextId: 0 };
}

export function foldTranscriptEvent(state: LiveTranscript, event: TranscriptEvent): LiveTranscript {
  if (!event.text.trim()) return state;
  const open = state.partial[event.source];
  const extendsOpen = open !== undefined && event.text.trim().startsWith(open.text.trim());
  const partial = { ...state.partial };
  const rows = [...state.rows];
  let nextId = state.nextId;
  if (open !== undefined && !extendsOpen) rows.push({ ...open, committed: true });
  const current: LiveRow = extendsOpen && open !== undefined
    ? { ...open, text: event.text, speaker: event.speaker }
    : { id: nextId++, source: event.source, startSeconds: event.timestamp, speaker: event.speaker, text: event.text, committed: false };
  if (event.isFinal) {
    rows.push({ ...current, committed: true });
    delete partial[event.source];
  } else {
    partial[event.source] = current;
  }
  return { rows: rows.slice(-LIVE_ROW_LIMIT), partial, nextId };
}

export function liveRows(state: LiveTranscript): LiveRow[] {
  return [...state.rows, ...Object.values(state.partial)].sort((a, b) => a.startSeconds - b.startSeconds || a.id - b.id);
}
