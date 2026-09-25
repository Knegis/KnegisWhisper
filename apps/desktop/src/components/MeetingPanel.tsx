import { useCallback, useLayoutEffect, useRef, useState } from "react";
import { clock } from "../MeetingNotification";
import { stopMeeting, updateMeetingNotes } from "../lib/ipc";
import type { LiveRow } from "../lib/liveTranscript";
import { useAutosave } from "../lib/useAutosave";
import { NoteEditor } from "./NoteEditor";

type MeetingPanelProps = { session: number; elapsed: number; detail: string; alert: boolean; transcriptAllowed: boolean; rows: LiveRow[]; initialNotes: string | null; onProblem: (message: string) => void };
const TABS = ["notes", "transcript"] as const;
type Tab = typeof TABS[number];

function LiveTranscriptList({ rows, container, active, nearBottom }: { rows: LiveRow[]; container: React.RefObject<HTMLDivElement | null>; active: boolean; nearBottom: React.RefObject<boolean> }) {
  useLayoutEffect(() => {
    const element = container.current;
    if (!element || !active) return;
    if (nearBottom.current) element.scrollTop = element.scrollHeight;
  }, [rows, container, active]);
  return <ol className="transcript-rows">{rows.map(row => <li key={row.id} className={row.committed ? undefined : "transcript-row-muted"}>
    <span className="transcript-clock">{clock(row.startSeconds)}</span>
    {row.speaker && <span className="transcript-speaker">{row.speaker}</span>}
    <span dir="auto">{row.text}</span>
  </li>)}</ol>;
}

export function MeetingPanel({ session, elapsed, detail, alert, transcriptAllowed, rows, initialNotes, onProblem }: MeetingPanelProps) {
  const [tab, setTab] = useState<Tab>("notes");
  const [stopProblem, setStopProblem] = useState<string | null>(null);
  const tabs = useRef<Partial<Record<Tab, HTMLButtonElement | null>>>({});
  const body = useRef<HTMLDivElement>(null);
  const nearBottom = useRef(true);
  const save = useCallback(async (patch: Partial<{ content: string }>) => {
    if (patch.content !== undefined) await updateMeetingNotes(patch.content);
  }, []);
  const autosave = useAutosave<{ content: string }>({ save });
  const switchTab = (next: Tab, focus = false) => {
    setTab(next);
    if (focus) tabs.current[next]?.focus();
  };
  const stop = async () => {
    try {
      await autosave.flush();
      await stopMeeting();
      setStopProblem(null);
    } catch (err) {
      const message = String(err);
      setStopProblem(message);
      onProblem(message);
    }
  };

  return <div className="notify notify-recording notify-panel" data-tauri-drag-region="deep">
    <div className="notify-header" title="Drag to move">
      <span className="indicator indicator-recording" aria-hidden="true" />
      <span className="notify-body"><strong>Recording {clock(elapsed)}</strong><span className={alert ? "notify-detail notify-alert" : "notify-detail"}>{detail}</span></span>
      <button type="button" onClick={() => { void stop(); }}>Stop</button>
    </div>
    <div className="notify-panel-body" data-tauri-drag-region="false" ref={body} onScroll={() => {
      const element = body.current;
      if (element && tab === "transcript") nearBottom.current = element.scrollHeight - element.scrollTop - element.clientHeight <= 8;
    }}>
      {autosave.status === "failed" && <p className="save-status error" role="alert">Couldn't save: {autosave.error} <button type="button" onClick={() => { void autosave.retry().catch(() => undefined); }}>Retry save</button></p>}
      {stopProblem && autosave.status !== "failed" && <p className="save-status error" role="alert">{stopProblem}</p>}
      <section id={`meeting-${session}-panel-notes`} role="tabpanel" aria-labelledby={`meeting-${session}-tab-notes`} hidden={tab !== "notes"}>
        {initialNotes === null ? <p role="status">Loading your notes…</p> : <div className="note-editor-host"><NoteEditor key={session} noteId={session} initialContent={initialNotes} onChange={content => autosave.schedule({ content })} /></div>}
      </section>
      <section id={`meeting-${session}-panel-transcript`} role="tabpanel" aria-labelledby={`meeting-${session}-tab-transcript`} hidden={tab !== "transcript"}>
        {transcriptAllowed ? <LiveTranscriptList rows={rows} container={body} active={tab === "transcript"} nearBottom={nearBottom} /> : <p className="muted">The live transcript is hidden because this window could not be excluded from screen capture.</p>}
      </section>
    </div>
    <div className="summary-tabs note-tabs notify-tabs" role="tablist" aria-label="Meeting view" data-tauri-drag-region="false">
      {TABS.map((value, index) => <button key={value} ref={element => { tabs.current[value] = element; }} id={`meeting-${session}-tab-${value}`} type="button" role="tab"
        aria-controls={`meeting-${session}-panel-${value}`} aria-selected={tab === value} tabIndex={tab === value ? 0 : -1}
        onClick={() => switchTab(value)} onKeyDown={event => {
          if (!["ArrowLeft", "ArrowRight", "Home", "End"].includes(event.key)) return;
          event.preventDefault();
          const next = event.key === "Home" ? 0 : event.key === "End" ? 1 : (index + 1) % 2;
          switchTab(TABS[next]!, true);
        }}>{value === "notes" ? "My notes" : "Transcript"}</button>)}
    </div>
  </div>;
}
