import { forwardRef, useCallback, useEffect, useImperativeHandle, useLayoutEffect, useRef, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { attachNoteRecording, getNoteRecording, getNoteSpeakerSuggestions, getNoteTranscriptRows, renameNoteSpeaker, updateNote, type Note, type NoteUpdate } from "../lib/ipc";
import { useAutosave } from "../lib/useAutosave";
import { saveStatusText } from "../lib/saveStatus";
import { transcriptToMarkdown } from "../lib/noteBody";
import { timeOfDay } from "../lib/dayGroups";
import { noteMeta } from "../lib/noteMeta";
import { message } from "../lib/useRefresh";
import { Icon } from "./Icon";
import { NoteEditor } from "./NoteEditor";
import { SummaryPane, type SummaryPaneHandle } from "./SummaryPane";
import { RecordingPlayer, type PlayerPosition, type RecordingPlayerHandle } from "./RecordingPlayer";
import { TranscriptPanel } from "./TranscriptPanel";

export type NoteWorkspaceHandle = { flush: () => Promise<void> };
const TABS = ["notes", "transcript", "replay"] as const;
type Tab = typeof TABS[number];
type Patch = { title: string; content: string };
const TAB_LABELS: Record<Tab, string> = { notes: "Notes", transcript: "Transcript", replay: "Replay" };

/**
 * The window width at and above which the transcript is a column beside the
 * note rather than a tab. At 1280 with the list panel open the note keeps
 * about 590px beside the 340px column (1280 less the 68px rail, the card's
 * 10px of margin and border, and the 272px panel); any narrower and the
 * editor is squeezed below a readable measure to make room for it.
 */
const WIDE = "(min-width: 1280px)";

/**
 * What a switch between the two layouts carries across; see `NoteWorkspace`.
 * `anchor` is the reading position; `reading`, a plain offset down the note,
 * is kept for when the anchor itself was inside something that remounted.
 */
type Flip = {
  anchor: { element: Element; offset: number } | null;
  reading: number;
  player: PlayerPosition | null;
  focus: "notes-tab" | "selected-tab" | null;
};

/**
 * What the reader is looking at: the element at `edge`, the top of the note's
 * scroller, and how far below the edge it starts. Found the way scroll
 * anchoring finds its anchor -- the first child that reaches past the edge,
 * then the first of that one's children that does -- so hidden tab panels,
 * which have no height, are passed over. It stops at a block whose children
 * are inline: `<code>` inside a paragraph moves to another line when the
 * column changes width, and anchoring on it would move the paragraph.
 *
 * An element rather than a pixel offset, because the note does not remount
 * when the layout switches, it reflows: the same offset lands on different
 * text in a column of a different width, and by the time the media listener
 * runs the old layout has already reflowed at the new one.
 */
function readingAnchor(body: HTMLElement, edge: number): Flip["anchor"] {
  let element: Element = body;
  for (;;) {
    const next: Element | undefined = [...element.children].find(child => {
      if (getComputedStyle(child).display === "inline") return false;
      const rect = child.getBoundingClientRect();
      return rect.height > 0 && rect.bottom > edge;
    });
    if (!next) break;
    element = next;
  }
  return element === body ? null : { element, offset: element.getBoundingClientRect().top - edge };
}
/** What remounts when the width is crossed, so that focus inside it would fall to `<body>`. */
const MOVING = '.note-side, .note-mini-player, .note-full-player, [role="tabpanel"][id$="-panel-transcript"]';

/**
 * One open note: its top bar, its title and meta row, and the Notes /
 * Transcript / Replay tabs.
 *
 * **The top bar** is the breadcrumb (folder, then title), the save status,
 * and two buttons that hand back to `Notes.tsx`, which owns both actions:
 * Export runs the same Export… the library's right-click menu does, and More
 * opens that menu under the button.
 *
 * **Wide** (`WIDE`, tracked with a change listener): the transcript leaves
 * its tab for a column on the right, the Transcript tab is hidden, and the
 * player sits at the foot of that column. A Transcript tab that is open when
 * the window widens gives way to Notes; narrowing brings the tab back. The
 * three tab ids stay the same in both layouts -- the hidden tab keeps its
 * button and its (empty) panel, so no `aria-controls` points at nothing.
 *
 * **Crossing the width** -- maximising the 1000px window, or snapping it to
 * half a screen -- mounts the player and the transcript afresh, because each
 * has one parent per layout. What the reader was doing is carried across
 * (`Flip`), taken in the media listener before React renders and put back in
 * the new layout:
 *
 * - the reading position -- the element at the top of the scroller that is
 *   going (`.notes-main`, or the note's own column when wide), as recorded
 *   after its last scroll -- placed at the same offset in the one that takes
 *   over;
 * - the player's time, speed and interval end, playing again if it was
 *   (`RecordingPlayer`'s `position` and `resume`);
 * - focus, if it was on something that goes: the Transcript tab hands it
 *   to Notes, and anything inside the transcript or the player hands it to
 *   the selected tab.
 *
 * A summary source is shown once and then cleared (`onSourceShown`), so the
 * new transcript does not select it again. A selection of rows, or a speaker
 * rename left open, does not survive the switch.
 */
export const NoteWorkspace = forwardRef<NoteWorkspaceHandle, {
  note: Note;
  reload: () => Promise<void>;
  folderName: string | null;
  onExport: () => void;
  onMore: (anchor: HTMLButtonElement) => void;
}>(function NoteWorkspace({ note, reload, folderName, onExport, onMore }, ref) {
  const saved = useRef({ title: note.title, content: note.content });
  const draft = useRef({ title: note.title, content: note.content });
  const [title, setTitle] = useState(note.title);
  const [empty, setEmpty] = useState(false);
  const [ready, setReady] = useState(false);
  const [seed, setSeed] = useState({ content: note.content, generation: 0 });
  const summaryRef = useRef<SummaryPaneHandle>(null);
  const playerRef = useRef<RecordingPlayerHandle>(null);
  const [tab, setTab] = useState<Tab>("notes");
  const [sourceRequest, setSourceRequest] = useState<{ index: number; nonce: number } | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [currentTime, setCurrentTime] = useState(0);
  const [playing, setPlaying] = useState(false);
  const [playbackStarted, setPlaybackStarted] = useState(false);
  const [transcript, setTranscript] = useState<{
    rows: Awaited<ReturnType<typeof getNoteTranscriptRows>>;
    recording: Awaited<ReturnType<typeof getNoteRecording>>;
    suggestions: string[];
  } | null>(null);
  const [transcriptError, setTranscriptError] = useState<string | null>(null);
  const [attaching, setAttaching] = useState(false);
  const [attachmentError, setAttachmentError] = useState<string | null>(null);
  const alive = useRef(true);
  const attachmentPending = useRef(false);
  useEffect(() => { alive.current = true; return () => { alive.current = false; }; }, []);

  const articleRef = useRef<HTMLElement>(null);
  const bodyRef = useRef<HTMLDivElement>(null);
  const [wide, setWide] = useState(() => window.matchMedia(WIDE).matches);
  // The layout on screen, which the media listener reads: by the time it runs, `query.matches`
  // already describes the width being moved to.
  const renderedWide = useRef(wide);
  const flip = useRef<Flip | null>(null);
  // How far down the note the reader is, in either layout. Narrower than the width the note
  // scrolls in `Notes.tsx`' `.notes-main`, below any notice above it; wide, in its own column.
  const readingOffset = useCallback((inWide: boolean) => {
    const body = bodyRef.current;
    if (!body) return 0;
    if (inWide) return body.scrollTop;
    const main = body.closest<HTMLElement>(".notes-main");
    return main ? main.getBoundingClientRect().top - body.getBoundingClientRect().top : 0;
  }, []);
  // The reading position as of the note's last scroll, kept up to date rather than measured when
  // the width is crossed: by the time the media listener runs, the old layout has already reflowed
  // at the new width, and Chromium's own scroll anchoring has moved it to suit that half-way
  // layout. An ordinary resize that reflows the note scrolls it too, through that anchoring, so it
  // is recorded like any other scroll.
  const lastReading = useRef<Pick<Flip, "anchor" | "reading">>({ anchor: null, reading: 0 });
  useEffect(() => {
    const main = articleRef.current?.closest<HTMLElement>(".notes-main");
    if (!main) return;
    let frame = 0;
    const record = () => {
      frame = 0;
      const body = bodyRef.current;
      if (!body) return;
      const scroller = renderedWide.current ? body : main;
      lastReading.current = { anchor: readingAnchor(body, scroller.getBoundingClientRect().top), reading: readingOffset(renderedWide.current) };
    };
    // Recorded a frame later, never in the event. Animation-frame callbacks run after media-query
    // change events in a rendering update, so a scroll that the crossing itself caused is recorded
    // after the listener has already taken the reading from before it.
    const scrolled = (event: Event) => {
      if (event.target !== main && event.target !== bodyRef.current) return;
      if (!frame) frame = requestAnimationFrame(record);
    };
    main.addEventListener("scroll", scrolled, { capture: true, passive: true });
    return () => { main.removeEventListener("scroll", scrolled, { capture: true }); cancelAnimationFrame(frame); };
  }, [readingOffset]);
  useEffect(() => {
    const query = window.matchMedia(WIDE);
    const change = () => {
      // Overwritten on every change, not only the first: two changes before a render leave the
      // layout where it was, and the next real switch must not carry a stale snapshot.
      const article = articleRef.current;
      const active = document.activeElement;
      flip.current = {
        ...lastReading.current,
        player: playerRef.current?.position() ?? null,
        focus: active?.id === `note-${note.id}-tab-transcript` ? "notes-tab"
          : active instanceof Element && article?.contains(active) && active.closest(MOVING) ? "selected-tab"
          : null,
      };
      setWide(query.matches);
      if (query.matches) setTab(current => current === "transcript" ? "notes" : current);
    };
    query.addEventListener("change", change);
    return () => query.removeEventListener("change", change);
  }, [note.id, readingOffset]);
  // The reading position and focus, before the new layout is painted.
  useLayoutEffect(() => {
    renderedWide.current = wide;
    const carried = flip.current;
    const body = bodyRef.current;
    const main = body?.closest<HTMLElement>(".notes-main");
    if (!carried || !body || !main) return;
    // `.notes-main` stops scrolling in the wide layout, but an element with `overflow: hidden`
    // keeps an offset it was given, and would hold the note up out of sight.
    if (wide) main.scrollTop = 0;
    const scroller = wide ? body : main;
    const anchor = carried.anchor;
    if (anchor && body.contains(anchor.element) && anchor.element.getClientRects().length > 0) {
      scroller.scrollTop += anchor.element.getBoundingClientRect().top - scroller.getBoundingClientRect().top - anchor.offset;
    } else if (wide) {
      body.scrollTop = carried.reading;
    } else {
      main.scrollTop += body.getBoundingClientRect().top - main.getBoundingClientRect().top + carried.reading;
    }
    const article = articleRef.current;
    const active = document.activeElement;
    // The Transcript tab goes to `display: none`, which does not always move focus off it at
    // once; anything else that remounted has already dropped focus to `<body>`.
    if (carried.focus === "notes-tab") document.getElementById(`note-${note.id}-tab-notes`)?.focus();
    else if (carried.focus === "selected-tab" && (active === document.body || !article?.contains(active))) {
      article?.querySelector<HTMLElement>('[role="tab"][aria-selected="true"]')?.focus();
    }
  }, [wide, note.id]);
  // Playback, once the new player has mounted and subscribed to its audio, so that its play and
  // pause events reach `onPlayingChange`. A task later rather than in this effect: in development,
  // StrictMode mounts the new player a second time, and the unmount in between pauses its audio,
  // which in a headless probe rejected a resume made here with "The play() request was
  // interrupted by a call to pause()". In a production build the wait is one task.
  useEffect(() => {
    const carried = flip.current;
    if (!carried) return;
    flip.current = null;
    setPlaying(false);
    const from = carried.player;
    if (!from) return;
    const timer = setTimeout(() => playerRef.current?.resume(from), 0);
    return () => clearTimeout(timer);
  }, [wide]);
  const shown: readonly Tab[] = wide ? TABS.filter(value => value !== "transcript") : TABS;

  const save = useCallback(async (patch: Partial<Patch>) => {
    const request: NoteUpdate = {};
    if (patch.content !== undefined && patch.content !== saved.current.content) request.content = patch.content;
    if (patch.title !== undefined && patch.title !== saved.current.title) request.title = patch.title;
    if (!Object.keys(request).length) return;
    if (!await updateNote(note.id, request)) throw new Error("This note no longer exists.");
    if (typeof request.content === "string") saved.current.content = request.content;
    if (typeof request.title === "string") saved.current.title = request.title;
    await reload();
  }, [note.id, reload]);
  const autosave = useAutosave<Patch>({ save });
  const normalizeTitle = useCallback(() => {
    const normalized = draft.current.title.trim() || "Untitled note";
    if (normalized !== draft.current.title) {
      draft.current.title = normalized; setTitle(normalized);
      autosave.schedule({ title: normalized, content: draft.current.content });
    }
  }, [autosave.schedule]);
  const flush = useCallback(async () => {
    normalizeTitle(); await autosave.flush(); await summaryRef.current?.flush();
  }, [normalizeTitle, autosave.flush]);
  useImperativeHandle(ref, () => ({ flush }), [flush]);
  useEffect(() => {
    if (!("__TAURI_INTERNALS__" in window)) return;
    let closing = false;
    const registered = getCurrentWindow().onCloseRequested(async event => {
      if (closing) return;
      event.preventDefault();
      try { await flush(); closing = true; await getCurrentWindow().close(); }
      catch (cause) { setError(message(cause)); }
    });
    return () => { void registered.then(stop => stop()); };
  }, [flush]);
  const loadTranscript = useCallback(async (rethrow = false) => {
    setTranscriptError(null);
    try {
      const [rows, recording, suggestions] = await Promise.all([getNoteTranscriptRows(note.id), getNoteRecording(note.id), getNoteSpeakerSuggestions(note.id)]);
      if (alive.current) setTranscript({ rows, recording, suggestions });
    } catch (cause) {
      // A rename failure belongs in its open form; the initial load uses the tab error.
      if (rethrow) throw cause;
      if (alive.current) setTranscriptError(message(cause));
    }
  }, [note.id]);
  useEffect(() => { void loadTranscript(); }, [loadTranscript]);
  const attachRecording = async () => {
    if (attachmentPending.current) return;
    attachmentPending.current = true; setAttaching(true); setAttachmentError(null);
    try {
      const recording = await attachNoteRecording(note.id);
      if (alive.current && recording) setTranscript(previous => previous ? { ...previous, recording } : previous);
    } catch (cause) { if (alive.current) setAttachmentError(message(cause)); }
    finally { if (alive.current) { attachmentPending.current = false; setAttaching(false); } }
  };
  const switchTab = async (next: Tab, focus = false) => {
    try {
      await flush();
      if (!alive.current) return;
      setError(null); setTab(next);
      if (focus) document.getElementById(`note-${note.id}-tab-${next}`)?.focus();
    } catch (cause) { setError(message(cause)); }
  };
  const clearSource = useCallback(() => setSourceRequest(null), []);
  const playInterval = useCallback((start: number, end: number | null) => {
    setPlaybackStarted(true); playerRef.current?.playInterval(start, end);
  }, []);
  const onPlayingChange = useCallback((value: boolean) => { setPlaying(value); if (value) setPlaybackStarted(true); }, []);
  const showSource = async (index: number) => {
    try {
      await flush();
      if (!alive.current) return;
      // A wide window already shows the transcript, in its column; only a narrower one has a tab to open.
      setError(null); if (!wide) setTab("transcript"); setSourceRequest({ index, nonce: Date.now() });
      const row = transcript?.rows.find(item => item.index === index);
      if (row?.startSeconds != null && transcript?.recording) playInterval(row.startSeconds, row.endSeconds);
    } catch (cause) { setError(message(cause)); }
  };
  const startFromTranscript = () => {
    if (!ready || draft.current.content.trim() || !transcript?.rows.length) return;
    const content = transcriptToMarkdown(transcript.rows);
    if (!content.trim()) return;
    draft.current.content = content; setEmpty(false); setReady(false);
    setSeed(previous => ({ content, generation: previous.generation + 1 }));
    autosave.schedule({ content });
  };

  // The folder half of `noteMeta` is Notes' to resolve -- it holds the folder list -- and arrives as `folderName`.
  const { date, minutes, participants } = noteMeta(note, []);
  const recordingDetails = note.recordingDetails && <details className="recording-details" open><summary>Recording details</summary><p>{note.recordingDetails}</p></details>;
  const transcriptBody = transcriptError ? <p className="error" role="alert">{transcriptError} <button type="button" onClick={() => { void loadTranscript(); }}>Retry</button></p> : !transcript ? <p role="status">Loading transcript and recording…</p> : transcript.rows.length ?
    <TranscriptPanel sourceRequest={sourceRequest} onSourceShown={clearSource} rows={transcript.rows}
      recording={transcript.recording} currentTime={currentTime} playing={playing} visible={wide || tab === "transcript"} playInterval={playInterval}
      suggestions={transcript.suggestions} onRename={async (speakerId, name) => {
        await renameNoteSpeaker(note.id, speakerId, name);
        // Rust resolves display names for both rows and export; reload those rows after each edit.
        await loadTranscript(true);
      }} />
    : <p className="muted">This note has no transcript.</p>;
  const player = transcript?.recording
    ? <RecordingPlayer ref={playerRef} noteId={note.id} onCurrentTime={setCurrentTime} onPlayingChange={onPlayingChange} compact={wide || tab !== "replay"} />
    : null;

  return <article ref={articleRef} id={`note-detail-${note.id}`} className={wide ? "note-detail note-detail-wide" : "note-detail"} aria-label={title}>
    <div className="note-body" ref={bodyRef}>
      <div className="note-topbar">
        <p className="note-crumb">
          {folderName === null ? null : <><span>{folderName}</span><span className="note-crumb-rule" aria-hidden="true">/</span></>}
          <span>{title.trim() || "Untitled note"}</span>
        </p>
        {/* Always rendered, and holding its place when it has nothing to say: a live region that is already in the page is the one screen readers announce reliably, and one that came and went would shift the buttons beside it. */}
        <p className="save-status" role="status" aria-live="polite">{saveStatusText(autosave.status)}</p>
        <button type="button" className="note-topbar-button" aria-label="Export…" title="Export…" onClick={onExport}><Icon name="export" /></button>
        <button type="button" className="note-topbar-button" aria-label="More" title="More" aria-haspopup="dialog" onClick={event => onMore(event.currentTarget)}><Icon name="more" /></button>
      </div>
      <div className="note-page">
        {autosave.status === "failed" && <p className="save-status error" role="alert">Couldn't save: {autosave.error} <button type="button" onClick={() => { void autosave.retry().catch(() => undefined); }}>Retry save</button></p>}
        <header className="note-detail-header">
          <input className="note-title" aria-label="Title" value={title} disabled={!ready} onChange={event => {
            const value = event.target.value; draft.current.title = value; setTitle(value);
            autosave.schedule({ title: value.trim() || "Untitled note", content: draft.current.content });
          }} onBlur={normalizeTitle} />
          {date !== null || minutes !== null || participants.length > 0 ? <ul className="note-meta">
            {date === null ? null : <li><Icon name="calendar" size={15} />{date.toLocaleDateString(undefined, { day: "numeric", month: "short", year: "numeric" })}, {timeOfDay(date)}</li>}
            {/* At least a minute, as the Meetings list rounds it: a 20-second recording is not "0 min". */}
            {minutes === null ? null : <li><Icon name="clock" size={15} />{Math.max(1, minutes)} min</li>}
            {participants.length === 0 ? null : <li><Icon name="meetings" size={15} /><span className="sr-only">Participants: </span>{participants.join(", ")}</li>}
          </ul> : null}
        </header>
        <div className="summary-tabs note-tabs" role="tablist" aria-label="Note view">
          {TABS.map(value => <button key={value} id={`note-${note.id}-tab-${value}`} type="button" role="tab" hidden={!shown.includes(value)}
            aria-controls={`note-${note.id}-panel-${value}`} aria-selected={tab === value} tabIndex={tab === value ? 0 : -1}
            onClick={() => { void switchTab(value); }} onKeyDown={event => {
              if (!["ArrowLeft", "ArrowRight", "Home", "End"].includes(event.key)) return;
              event.preventDefault();
              // Over the tabs on screen: a wide window's hidden Transcript tab is skipped, not landed on.
              const at = shown.indexOf(value);
              const next = event.key === "Home" ? 0 : event.key === "End" ? shown.length - 1 : (at + (event.key === "ArrowRight" ? 1 : shown.length - 1)) % shown.length;
              void switchTab(shown[next]!, true);
            }}>{TAB_LABELS[value]}</button>)}
        </div>
        {error && <p className="error" role="alert">{error}</p>}
        <div className="note-workspace">
          <section id={`note-${note.id}-panel-notes`} role="tabpanel" aria-labelledby={`note-${note.id}-tab-notes`} hidden={tab !== "notes"}>
            <div className="note-prose">
              <h2 className="note-section-heading">Your notes</h2>
              {ready && empty && transcript?.rows.some(row => row.text.trim()) && <button type="button" className="start-from-transcript" onClick={startFromTranscript}>Start from transcript</button>}
              <NoteEditor key={seed.generation} noteId={note.id} initialContent={seed.content}
                {...(seed.generation === 0 ? { legacyTitle: note.title } : {})}
                onReady={content => { draft.current.content = content; setEmpty(!content.trim()); setReady(true); }}
                onChange={content => { draft.current.content = content; setEmpty(!content.trim()); autosave.schedule({ content }); }} />
            </div>
            <h2 className="note-section-heading">AI summary</h2>
            <SummaryPane ref={summaryRef} noteId={note.id} visible={tab === "notes"} onSource={index => { void showSource(index); }} />
          </section>
          {!wide && player ? <div className={tab === "replay" ? "note-full-player" : "note-mini-player"} hidden={tab !== "replay" && !playbackStarted}>
            {player}
          </div> : null}
          <section id={`note-${note.id}-panel-transcript`} role="tabpanel"
            aria-labelledby={`note-${note.id}-tab-transcript`} hidden={tab !== "transcript"}>
            {wide ? null : <>{recordingDetails}{transcriptBody}</>}
          </section>
          <section id={`note-${note.id}-panel-replay`} role="tabpanel"
            aria-labelledby={`note-${note.id}-tab-replay`} hidden={tab !== "replay"}>
            {recordingDetails}
            {transcriptError ? <p className="error" role="alert">{transcriptError} <button type="button" onClick={() => { void loadTranscript(); }}>Retry</button></p> : !transcript ? <p role="status">Loading recording…</p> : !transcript.recording && <div className="recording-empty">
              <p>This note has no recording attached. Select its original recording to replay it. A copy is saved; your original stays in place.</p>
              <button type="button" disabled={attaching} onClick={() => { void attachRecording(); }}>{attaching ? "Attaching…" : "Attach recording…"}</button>
              <p className="muted">16-bit PCM WAV, mono or stereo.</p>
              {attachmentError && <p className="error" role="alert">{attachmentError}</p>}
            </div>}
          </section>
        </div>
      </div>
    </div>
    {/* No recording details here: the Replay tab still has them, and beside it they would be on screen twice. */}
    {wide ? <div className="note-side">
      <h2 className="note-side-label">Transcript</h2>
      <div className="note-side-body">{transcriptBody}</div>
      {player ? <div className="note-side-player">{player}</div> : null}
    </div> : null}
  </article>;
});
