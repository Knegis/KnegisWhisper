import { enrollKnownVoice } from "../lib/ipc";
import { forwardRef, useCallback, useEffect, useImperativeHandle, useLayoutEffect, useMemo, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { attachNoteRecording, getNoteRecording, getNoteRecordingState, getNoteMetadataForPeople, restoreNote, retryNoteRecordingDeletion, type NoteRecordingState, getNoteSpeakerLabels, retryNoteSpeakerLabels, dismissNoteSpeakerLabels, onNoteSpeakerLabelsChanged, type SpeakerLabelState, getNoteSpeakerSuggestions, getNoteTranscriptRows, renameNoteSpeaker, updateNote, type Note, type NoteUpdate } from "../lib/ipc";
import { useAutosave, type AutosaveStatus } from "../lib/useAutosave";
import { workspaceSaveText } from "../lib/saveStatus";
import { transcriptToMarkdown } from "../lib/noteBody";
import { hasStoredTranscript, noteMeta, noteWhen } from "../lib/noteMeta";
import { lastStartedLine, NO_SELECTION, sectionMarks, selectionInterval, transcriptSections, type LineSelection } from "../lib/transcriptLayout";
import { message } from "../lib/useRefresh";
import { speakerLabelLine } from "../lib/speakerLabels";
import { askConfirm } from "./ConfirmDialog";
import { FailureLine } from "./FailureLine";
import { Icon } from "./Icon";
import { NoteEditor } from "./NoteEditor";
import { NotePeople } from "./NotePeople";
import { PlayerHeader } from "./PlayerHeader";
import { SummaryPane, type SummaryPaneHandle } from "./SummaryPane";
import type { SummarySetupTarget } from "../lib/ipc";
import { RecordingPlayer, type RecordingPlayerHandle } from "./RecordingPlayer";
import { TranscriptHeading, TranscriptPanel } from "./TranscriptPanel";

export type NoteWorkspaceHandle = { flush: () => Promise<void> };
const TABS = ["notes", "transcript", "replay"] as const;
type Tab = typeof TABS[number];
type Patch = { title: string; content: string };
const TAB_LABELS: Record<Tab, string> = { notes: "Notes", transcript: "Transcript", replay: "Replay" };

/**
 * The window width at and above which the transcript is a column beside the
 * note rather than a tab. Chosen beside the old rail, when at 1280 with the
 * list panel open the note kept about 590px beside the 340px column (1280
 * less the rail, the card's margin and border, and a narrower panel); any
 * narrower and the editor is squeezed below a readable measure to make room
 * for it. Without the rail the note has somewhat more room at the same width,
 * and the breakpoint has not been revisited.
 */
const WIDE = "(min-width: 1280px)";

/**
 * Whether the reader has hidden the transcript column, for as long as the app
 * is open. Held here rather than in the component because `Notes.tsx` mounts
 * a workspace per note: each note opens with the column as the last one left
 * it, as the list panel keeps its state from page to page (`App.tsx`).
 */
let columnHiddenChoice = false;

/**
 * What a switch between the two layouts carries across; see `NoteWorkspace`.
 * `anchor` is the reading position; `reading`, a plain offset down the note,
 * is kept for when the anchor itself was inside something that remounted.
 */
type Flip = {
  anchor: { element: Element; offset: number } | null;
  reading: number;
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
/**
 * What remounts when the width is crossed, so that focus inside it would fall to `<body>`. Not
 * `.note-full-player`: Replay's player is in the note column in both layouts, and stays mounted.
 */
const MOVING = '.note-side, .note-mini-player, [role="tabpanel"][id$="-panel-transcript"]';

/**
 * Where the one player is drawn. `note` is the note column -- in full over Replay, and otherwise a
 * full player in Replay. `inline` is the narrow mini card below the stable transcript wrapper;
 * `side` is the wide transcript column's card.
 */
type PlayerSlot = "note" | "side" | "inline";
const playerSlotOf = (wide: boolean, tab: Tab): PlayerSlot => tab === "replay" ? "note" : wide ? "side" : "inline";

/**
 * One open note: its title and meta row, and the Notes /
 * Transcript / Replay tabs.
 *
 * **Above the editor**, the tabs share their row with the save status and
 * two buttons that hand back to `Notes.tsx`, which owns both actions:
 * Export runs the same Export… the library's right-click menu does, and More
 * opens that menu under the button. At the width a third, Hide transcript or
 * Show transcript, is the transcript column's own.
 *
 * **Wide** (`WIDE`, tracked with a change listener): the transcript leaves
 * its tab for a column on the right, the Transcript tab is hidden, and the
 * player is a card floating at the foot of that column (KNEG-448) -- except
 * while Replay is open, which has the full player over it in the note column,
 * as it does in a narrower window. A Transcript tab that is open when the
 * window widens gives way to Notes; narrowing brings the tab back. The three tab ids stay the
 * same in both layouts -- the hidden tab keeps its button and its (empty)
 * panel, so no `aria-controls` points at nothing.
 *
 * **Hiding the column** (`columnHiddenChoice`) puts the note back in the
 * narrower layout at any width: one column, the Transcript tab back, the
 * player in the note. It is the same switch as crossing the width, and is
 * carried across the same way (below); while the column is hidden, crossing
 * the width moves nothing. Showing it again gives an open Transcript tab way
 * to Notes, as widening does.
 *
 * There is only ever one player, so one `<audio>`. It is drawn in one of
 * three slots (`playerSlotOf`) but rendered once, into an element that the
 * slot on screen holds (`holdPlayer`). A tab switch, a crossing of the width
 * or hiding the column moves that element and never mounts the player
 * afresh, so playback, its speed and an interval's end carry on (KNEG-488).
 *
 * **Crossing the width** -- maximising the 1000px window, or snapping it to
 * half a screen -- or hiding or showing the column moves the transcript and
 * the player to their places in the other layout. What the reader was doing
 * is carried across (`Flip`), taken in the media listener or the column's
 * button before React renders and put back in the new layout:
 *
 * - the reading position -- the element at the top of the scroller that is
 *   going (`.notes-main`, or the note's own column when wide), as recorded
 *   after its last scroll -- placed at the same offset in the one that takes
 *   over;
 * - focus, if it was on something that moves: the Transcript tab hands it
 *   to Notes, and anything inside the transcript or the player hands it to
 *   the selected tab.
 *
 * A summary source is shown once and then cleared (`onSourceShown`), so the
 * transcript does not scroll to it again. The selected lines are the note's
 * (`selection`, KNEG-448), so they survive the switch, and so does Follow.
 * The transcript itself stays mounted across it (KNEG-455), so a speaker
 * rename left open, Find and a closed section survive it too.
 *
 * **The player card's header** (`PlayerHeader`), while the transcript is on
 * screen -- the column at the width, or the Transcript tab below it -- names
 * who is speaking at the player's time, or offers to play the selected lines
 * and to clear them, and carries the Follow chip (`follow`): while it is on,
 * the transcript keeps the playing line in view, until the person scrolls it.
 * Below the width the mini player shows on the Transcript tab before anything
 * has played, so that offer is there to press; on Notes it waits for
 * playback, as before, and has no header, so no chip.
 */
export const NoteWorkspace = forwardRef<NoteWorkspaceHandle, {
  note: Note;
  /**
   * A transcript row to open at, from a search hit (KNEG-406): selected and in
   * view, and the player left alone. Shown once per object, so a request
   * repeated as a new object -- the same hit chosen again -- is shown again.
   */
  revealRow?: { index: number } | null;
  reload: () => Promise<void>;
  onSaved: (id: number, patch: { title?: string; content?: string }) => Promise<void>;
  onExport: () => void;
  onMore: (anchor: HTMLButtonElement) => void;
  /** Whether the menu More opens is open, for its `aria-expanded`. */
  moreOpen: boolean;
  onSummarySetup: (target: SummarySetupTarget) => Promise<void>;
  focusSummary: boolean;
}>(function NoteWorkspace({ note, revealRow = null, reload, onSaved, onExport, onMore, moreOpen, onSummarySetup, focusSummary }, ref) {
  const [blocked, setBlocked] = useState<"trashed" | "deleted" | "restored" | null>(null);
  const blockedRef = useRef(blocked); blockedRef.current = blocked;
  const [draftNotice, setDraftNotice] = useState<string | null>(null);
  useEffect(() => {
    let disposed = false; let inFlight = false;
    const read = async () => {
      if (disposed || inFlight || document.visibilityState === "hidden") return;
      inFlight = true;
      try {
        const metadata = await getNoteMetadataForPeople(note.id);
        if (disposed) return;
        if (!metadata || metadata.trashedAt !== null) {
          const next = metadata ? "trashed" : "deleted";
          blockedRef.current = next; setBlocked(next);
        } else if (blockedRef.current === "trashed") { blockedRef.current = "restored"; setBlocked("restored"); }
      } catch { /* A failed read is not proof of deletion; writes retain backend checks. */ }
      finally { inFlight = false; }
    };
    const timer = setInterval(() => { void read(); }, 2000);
    const focused = () => { void read(); };
    window.addEventListener("focus", focused); document.addEventListener("visibilitychange", focused);
    return () => { disposed = true; clearInterval(timer); window.removeEventListener("focus", focused); document.removeEventListener("visibilitychange", focused); };
  }, [note.id]);
  const saved = useRef({ title: note.title, content: note.content });
  const draft = useRef({ title: note.title, content: note.content });
  const [title, setTitle] = useState(note.title);
  const [empty, setEmpty] = useState(false);
  const [ready, setReady] = useState(false);
  const [seed, setSeed] = useState({ content: note.content, generation: 0 });
  const summaryRef = useRef<SummaryPaneHandle>(null);
  // The summary pane saves on its own; the toolbar's line covers both (KNEG-398).
  const [summarySave, setSummarySave] = useState<AutosaveStatus>("idle");
  const playerRef = useRef<RecordingPlayerHandle>(null);
  const [mediaReady, setMediaReady] = useState(false);
  const [tab, setTab] = useState<Tab>("notes");
  const [sourceRequest, setSourceRequest] = useState<{ index: number; nonce: number } | null>(null);
  // The transcript lines selected, here rather than in the panel so the player card can offer to
  // play them (KNEG-448).
  const [selection, setSelection] = useState<LineSelection>(NO_SELECTION);
  // Follow (KNEG-448): whether the transcript keeps the playing line in view. On as each note opens
  // (`Notes.tsx` mounts a workspace per note); the panel turns it off when the person scrolls the
  // transcript, and the card's chip, a line or word pressed, Play selection and a summary source
  // turn it back on. Here, beside the chip.
  const [follow, setFollow] = useState(true);
  // Set as the chip turns Follow on and taken once by the panel, which then centres the line the
  // player is at. Turning it on by pressing a line leaves the transcript where it is: that line is
  // in view already, and moving it would move the next press's target.
  const recentre = useRef(false);
  const takeRecentre = useCallback(() => {
    const due = recentre.current;
    recentre.current = false;
    return due;
  }, []);
  const releaseFollow = useCallback(() => setFollow(false), []);
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
  const [windowWide, setWindowWide] = useState(() => window.matchMedia(WIDE).matches);
  const [columnHidden, setColumnHidden] = useState(columnHiddenChoice);
  // Read by the media listener, which is subscribed once per note, and set as the button is pressed.
  const columnHiddenNow = useRef(columnHidden);
  // A note with no transcript has no column to show: a wide window gave a plain note a 340px one
  // saying "This note has no transcript." (KNEG-377). Read from the note, so it is right on the
  // first frame, and kept in a ref for the media listener. Parsed once per transcript, not on every
  // render: playback renders about four times a second, and an hour's transcript with its word
  // timings is over half a megabyte (KNEG-408).
  const hasTranscript = useMemo(() => hasStoredTranscript(note.transcript), [note.transcript]);
  const hasTranscriptNow = useRef(hasTranscript);
  useLayoutEffect(() => { hasTranscriptNow.current = hasTranscript; }, [hasTranscript]);
  // The layout, rather than the width: a window at the width with the column hidden lays the note
  // out as a narrower one does, Transcript tab and all, and so does a note with no transcript.
  const wide = windowWide && !columnHidden && hasTranscript;
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
  // What the reader was doing, taken before a switch of layout renders; see `Flip`.
  const takeFlip = useCallback((): Flip => {
    const article = articleRef.current;
    const active = document.activeElement;
    return {
      ...lastReading.current,
      focus: active?.id === `note-${note.id}-tab-transcript` ? "notes-tab"
        : active instanceof Element && article?.contains(active) && active.closest(MOVING) ? "selected-tab"
        : null,
    };
  }, [note.id]);
  useEffect(() => {
    const query = window.matchMedia(WIDE);
    const change = () => {
      const next = query.matches && !columnHiddenNow.current && hasTranscriptNow.current;
      // Taken on every change that moves the layout and cleared on one that does not, never kept
      // from an earlier one: two changes before a render leave the layout where it was, and the next
      // real switch must not carry a stale snapshot. With the column hidden, crossing the width
      // moves nothing.
      flip.current = next === renderedWide.current ? null : takeFlip();
      setWindowWide(query.matches);
      if (!next && articleRef.current?.querySelector(".transcript")?.contains(document.activeElement)) setTab("transcript");
      if (next) setTab(current => current === "transcript" ? "notes" : current);
    };
    query.addEventListener("change", change);
    return () => query.removeEventListener("change", change);
  }, [takeFlip]);
  // The column's button. The same switch of layout as crossing the width, carried across the same way.
  const toggleColumn = () => {
    const hide = !columnHidden;
    flip.current = takeFlip();
    columnHiddenChoice = hide; columnHiddenNow.current = hide;
    setColumnHidden(hide);
    if (!hide) setTab(current => current === "transcript" ? "notes" : current);
  };
  // The reading position and focus, before the new layout is painted.
  useLayoutEffect(() => {
    renderedWide.current = wide;
    const carried = flip.current;
    flip.current = null;
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
  const playerSlot = playerSlotOf(wide, tab);
  const shown: readonly Tab[] = wide ? TABS.filter(value => value !== "transcript") : TABS;

  const save = useCallback(async (patch: Partial<Patch>) => {
    if (blockedRef.current) throw new Error("This note is read-only. Restore it before saving.");
    const request: NoteUpdate = {};
    if (patch.content !== undefined && patch.content !== saved.current.content) request.content = patch.content;
    if (patch.title !== undefined && patch.title !== saved.current.title) request.title = patch.title;
    if (!Object.keys(request).length) return;
    if (!await updateNote(note.id, request)) throw new Error("This note no longer exists.");
    if (typeof request.content === "string") saved.current.content = request.content;
    if (typeof request.title === "string") saved.current.title = request.title;
    await onSaved(note.id, { ...(typeof request.title === "string" ? { title: request.title } : {}), ...(typeof request.content === "string" ? { content: request.content } : {}) });
  }, [note.id, onSaved]);
  const autosave = useAutosave<Patch>({ save, paused: blocked !== null });
  const normalizeTitle = useCallback(() => {
    if (blockedRef.current) return;
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
  const [recordingState, setRecordingState] = useState<NoteRecordingState | null>(null);
  const [retryingDeletion, setRetryingDeletion] = useState(false);
  const deletionPending = useRef(false);
  const retryDeletion = async () => {
    if (deletionPending.current) return;
    deletionPending.current = true; setRetryingDeletion(true); setAttachmentError(null);
    playerRef.current?.pause();
    try {
      await flush();
      const next = await retryNoteRecordingDeletion(note.id);
      if (!alive.current) return;
      setRecordingState(next);
      const recording = await getNoteRecording(note.id);
      if (alive.current) setTranscript(previous => previous ? { ...previous, recording: next.status === "deleted" || next.status === "deleted-update-failed" ? null : recording } : previous);
    } catch (cause) { if (alive.current) setAttachmentError(message(cause)); }
    finally { deletionPending.current = false; if (alive.current) setRetryingDeletion(false); }
  };
  const loadTranscript = useCallback(async (rethrow = false) => {
    setTranscriptError(null);
    try {
      const [rows, recording, suggestions, disposition] = await Promise.all([getNoteTranscriptRows(note.id), getNoteRecording(note.id), getNoteSpeakerSuggestions(note.id), getNoteRecordingState(note.id)]);
      if (alive.current) { setRecordingState(disposition); setTranscript({ rows, recording: disposition?.status === "deleted-update-failed" ? null : recording, suggestions }); }
    } catch (cause) {
      // A rename failure belongs in its open form; the initial load uses the tab error.
      if (rethrow) throw cause;
      if (alive.current) setTranscriptError(message(cause));
    }
  }, [note.id]);
  useEffect(() => { void loadTranscript(); }, [loadTranscript]);
  // Speaker labels are added after the note is saved, so they can still be
  // coming, or have failed, while the note is open. Each change reloads the
  // rows too: that is where the labels land, and where a deleted recording goes.
  const [speakerLabels, setSpeakerLabels] = useState<SpeakerLabelState | null>(null);
  const [speakerLabelsBusy, setSpeakerLabelsBusy] = useState(false);
  const [speakerLabelsError, setSpeakerLabelsError] = useState<string | null>(null);
  useEffect(() => {
    let current = true;
    const load = async (reload: boolean) => {
      try {
        const next = await getNoteSpeakerLabels(note.id);
        if (!current) return;
        setSpeakerLabels(next);
        if (reload) void loadTranscript();
      } catch { /* The line is extra; the transcript's own load reports a database that can't be read. */ }
    };
    void load(false);
    const subscription = onNoteSpeakerLabelsChanged(id => { if (id === note.id) void load(true); });
    return () => { current = false; void subscription.then(stop => stop()); };
  }, [note.id, loadTranscript]);
  const actOnSpeakerLabels = async (act: (id: number) => Promise<SpeakerLabelState | null>) => {
    if (speakerLabelsBusy) return;
    setSpeakerLabelsBusy(true); setSpeakerLabelsError(null);
    try {
      const next = await act(note.id);
      if (!alive.current) return;
      setSpeakerLabels(next);
      void loadTranscript();
    } catch (cause) { if (alive.current) setSpeakerLabelsError(message(cause)); }
    finally { if (alive.current) setSpeakerLabelsBusy(false); }
  };
  const renameSpeaker = useCallback(async (speakerId: string, name: string, remember = false, replaceId: number | null = null) => {
    if (remember && name.trim()) await enrollKnownVoice(note.id, speakerId, name, replaceId);
    else await renameNoteSpeaker(note.id, speakerId, name);
    // Rust resolves display names for both rows and export; reload those rows after each edit.
    await loadTranscript(true);
  }, [note.id, loadTranscript]);
  // The transcript's sections and turns, which the column draws, and where the player ticks its
  // scrubber: every section start after the first (KNEG-448). Computed once per rows, so a render
  // for playback, four a second, hands the panel and the player the same arrays.
  const transcriptRows = transcript?.rows;
  const sections = useMemo(() => transcriptSections(transcriptRows ?? []), [transcriptRows]);
  const marks = useMemo(() => sectionMarks(sections), [sections]);
  // Who the card names: the speaker of the last line to have started, which holds through the
  // gaps between lines (KNEG-448).
  const currentLine = useMemo(() => lastStartedLine(transcriptRows ?? [], currentTime), [transcriptRows, currentTime]);
  // What the card offers to play while two lines or more are selected: the first one's start to
  // the last one's end, or to the recording's end when that line runs to it.
  const duration = transcript?.recording?.durationSeconds ?? null;
  const toPlay = useMemo(() => selectionInterval(transcriptRows ?? [], selection.rows, duration), [transcriptRows, selection.rows, duration]);
  // The card's height as `--player-h` on the article, for the transcript beside or under it to
  // keep its last line clear of the card (KNEG-448). Written straight onto the element rather
  // than into state, so a resize renders nothing and cannot move the player (KNEG-377). Attached
  // only to the slot holding the floating card -- the column's foot, or the mini player -- and
  // the variable goes with that slot.
  const observePlayer = useCallback((slot: HTMLDivElement | null) => {
    if (slot === null) return;
    const observer = new ResizeObserver(([entry]) => {
      const height = entry?.borderBoxSize[0]?.blockSize ?? slot.offsetHeight;
      articleRef.current?.style.setProperty("--player-h", `${height}px`);
    });
    observer.observe(slot, { box: "border-box" });
    return () => {
      observer.disconnect();
      articleRef.current?.style.removeProperty("--player-h");
    };
  }, []);
  // The one player is rendered once, into an element of its own, and the slot on screen holds
  // that element (KNEG-488). Rendered in each slot instead, it was mounted afresh on every move --
  // a tab switch, a crossing of the width, hiding the column -- and its new `<audio>` loaded from
  // nothing, resuming from the old player's position after a gap: up to 150 ms between two
  // playing 50 ms samples in headless Edge, where a kept element shows about 60. A media element
  // moved from one parent to another within a task keeps playing; it is paused only if it is
  // still out of the document when the browser next looks.
  const [playerHost] = useState(() => {
    const host = document.createElement("div");
    host.className = "player-host";
    return host;
  });
  const holdPlayer = useCallback((slot: HTMLDivElement | null) => {
    if (slot !== null && playerHost.parentNode !== slot) slot.appendChild(playerHost);
  }, [playerHost]);
  const holdObservedPlayer = useCallback((slot: HTMLDivElement | null) => {
    holdPlayer(slot);
    return observePlayer(slot);
  }, [holdPlayer, observePlayer]);
  const attachRecording = async () => {
    if (attachmentPending.current) return;
    attachmentPending.current = true; setAttaching(true); setAttachmentError(null);
    try {
      const recording = await attachNoteRecording(note.id);
      if (alive.current && recording) { setRecordingState(null); setTranscript(previous => previous ? { ...previous, recording } : previous); }
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
  // A line or a word pressed: played from there to the end, with Follow on (KNEG-448).
  const playFrom = useCallback((start: number) => { setFollow(true); playInterval(start, null); }, [playInterval]);
  const playSelection = () => { if (toPlay !== null) { setFollow(true); playInterval(toPlay.start, toPlay.end); } };
  // The card's chip: off, or on and back to the line the player is at.
  const toggleFollow = () => {
    if (!follow) recentre.current = true;
    setFollow(!follow);
  };
  const clearSelection = useCallback(() => setSelection(NO_SELECTION), []);
  // For a transcript row highlighting word by word (KNEG-408). Read through the ref, so whichever
  // slot the one player is drawn in answers.
  const playerTime = useCallback(() => playerRef.current?.time() ?? null, []);
  // The Transcript at row `index`, selected and in view, and nothing else. False when the save
  // failed or the note has gone.
  const openRow = async (index: number): Promise<boolean> => {
    try {
      await flush();
      if (!alive.current) return false;
      // A wide window already shows the transcript, in its column; only a narrower one has a tab to
      // open. Read now, after the save, as `switchTab` reads it. The panel only scrolls to the row.
      setError(null); if (!renderedWide.current) setTab("transcript");
      setSelection({ rows: new Set([index]), anchor: index }); setSourceRequest({ index, nonce: Date.now() });
      return true;
    } catch (cause) { setError(message(cause)); return false; }
  };
  // A summary's source link: the row, played, with Follow on.
  const showSource = async (index: number) => {
    if (!(await openRow(index))) return;
    setFollow(true);
    const row = transcript?.rows.find(item => item.index === index);
    if (row?.startSeconds != null && transcript?.recording) playInterval(row.startSeconds, row.endSeconds);
  };
  // A search hit's row: only shown. Sound doesn't start because a search was answered -- the
  // owner's ruling of 2026-10-07 (KNEG-406). While something plays, Follow lets go, or it would
  // take the transcript straight back from the row to the playing line (KNEG-448).
  const revealed = useRef<object | null>(null);
  useEffect(() => {
    if (!revealRow || revealed.current === revealRow) return;
    revealed.current = revealRow;
    if (playing) setFollow(false);
    void openRow(revealRow.index);
    // Only the request: `openRow` is redefined each render and reads refs and setters, and
    // `playing` is read as it is when the request arrives.
  }, [revealRow]);
  const startFromTranscript = () => {
    if (!ready || draft.current.content.trim() || !transcript?.rows.length) return;
    const content = transcriptToMarkdown(transcript.rows);
    if (!content.trim()) return;
    draft.current.content = content; setEmpty(false); setReady(false);
    setSeed(previous => ({ content, generation: previous.generation + 1 }));
    autosave.schedule({ content });
  };

  const { date, minutes, transcribed } = noteMeta(note, []);
  const when = noteWhen(date, minutes);
  const dispositionLine = recordingState?.status === "deleted" ? "The recording was deleted after transcribing, as chosen for this meeting." : recordingState?.status === "deleted-update-failed" ? "The recording file is gone, but its attachment still needs cleanup." : recordingState?.status === "kept-incomplete" ? "This meeting was set to delete its recording after transcribing. It was kept because the saved transcript may be incomplete." : recordingState?.status === "kept-untranscribed" ? "This meeting was set to delete its recording after transcribing. It was kept because nothing was transcribed from it." : recordingState?.canRetry ? "The recording is still attached. Its requested deletion has not completed." : null;
  const disposition = dispositionLine && <div className="recording-disposition" role="status"><p>{dispositionLine}</p>{recordingState?.error && <p>{recordingState.error}</p>}{recordingState?.canRetry && <button type="button" disabled={blocked !== null || retryingDeletion} onClick={() => { void retryDeletion(); }}>{retryingDeletion ? "Retrying deletion…" : "Retry deletion"}</button>}</div>;
  const labelLine = speakerLabelLine(speakerLabels);
  const speakerLabelStatus = labelLine && <div className="speaker-label-status">
    {labelLine.kind === "failed"
      ? <FailureLine text={labelLine.text} detail={labelLine.detail} />
      : <p role="status" className="muted">{labelLine.text}</p>}
    <div className="speaker-label-actions">
      {labelLine.retry && <button type="button" className="secondary" disabled={blocked !== null || speakerLabelsBusy} onClick={() => { void actOnSpeakerLabels(retryNoteSpeakerLabels); }}>{labelLine.retry}</button>}
      {labelLine.dismiss && <button type="button" className="secondary" disabled={blocked !== null || speakerLabelsBusy} onClick={() => { void actOnSpeakerLabels(dismissNoteSpeakerLabels); }}>{labelLine.dismiss}</button>}
    </div>
    {speakerLabelsError && <p className="error" role="alert">{speakerLabelsError}</p>}
  </div>;
  const recordingDetails = note.recordingDetails && <details className="recording-details" open><summary>Recording details</summary><p>{note.recordingDetails}</p></details>;
  const transcriptBody = transcriptError ? <p className="error" role="alert">{transcriptError} <button type="button" onClick={() => { void loadTranscript(); }}>Retry</button></p> : !transcript ? <p role="status">Loading transcript and recording…</p> : transcript.rows.length ?
    <TranscriptPanel noteId={note.id} readOnly={blocked !== null} sourceRequest={sourceRequest} onSourceShown={clearSource} rows={transcript.rows} sections={sections}
      recording={transcript.recording} currentTime={currentTime} playing={playing} visible={wide || tab === "transcript"} wide={wide} playFrom={playFrom}
      time={playerTime} selection={selection} onSelectionChange={setSelection}
      suggestions={transcript.suggestions} onRename={renameSpeaker}
      follow={follow} onReleaseFollow={releaseFollow} takeRecentre={takeRecentre} currentIndex={currentLine?.index ?? null} />
    : <p className="muted">This note has no transcript.</p>;
  // The panel heads itself (`TranscriptHeading`, sticky in whatever scrolls it); the column's
  // loading, error and empty lines get the same label over them, so it is there before the rows.
  const transcriptShown = !transcriptError && transcript !== null && transcript.rows.length > 0;
  // The header follows transcript visibility, including Replay's wide full player.
  const transcriptOnScreen = wide || tab === "transcript";
  const header = transcriptOnScreen && transcriptShown
    ? <PlayerHeader speaker={currentLine === null ? null : currentLine.speaker ?? currentLine.speakerId} playing={playing}
      selection={toPlay?.label ?? null} hasSelection={selection.rows.size > 0} canPlaySelection={mediaReady} onPlaySelection={playSelection} onClearSelection={clearSelection}
      follow={follow} onToggleFollow={toggleFollow} />
    : null;
  // Drawn in one of three slots (`playerSlotOf`), never more than once: rendered into `playerHost`,
  // which that slot holds. The header is a prop, so it comes and goes without remounting the
  // player or moving it (KNEG-377).
  const player = transcript?.recording
    ? <RecordingPlayer ref={playerRef} noteId={note.id} onCurrentTime={setCurrentTime} onPlayingChange={onPlayingChange} onReadyChange={setMediaReady} marks={marks} header={header} />
    : null;

  return <article ref={articleRef} id={`note-detail-${note.id}`} className={wide ? "note-detail note-detail-wide" : "note-detail"} aria-label={title}>
    <div className="note-body" ref={bodyRef}>
      <div className="note-page">
        {autosave.status === "failed" && <p className="save-status error" role="alert">Couldn't save: {autosave.error} <button type="button" disabled={blocked !== null} onClick={() => { void autosave.retry().catch(() => undefined); }}>Retry save</button></p>}
        <header className="note-detail-header">
          {when === null ? null : <p className="note-when"><span className="note-when-mark" aria-hidden="true" />{when}</p>}
          {/* The title is the page's heading and a field. A textarea, because at this size a
              one-line field would cut off most titles; Enter stays out of it, and a line break
              pasted in becomes a space, since a title has one line to Rust. The heading is
              named outright, so what it is called does not depend on how a screen reader
              treats a field inside a heading. */}
          <h1 className="note-title-heading" aria-label={title}>
            <textarea className="note-title" rows={1} aria-label="Title" value={title} disabled={!ready || blocked !== null}
              onKeyDown={event => { if (event.key === "Enter") event.preventDefault(); }}
              onChange={event => {
                const value = event.target.value.replace(/\r?\n/g, " "); draft.current.title = value; setTitle(value);
                autosave.schedule({ title: value.trim() || "Untitled note", content: draft.current.content });
              }} onBlur={normalizeTitle} />
          </h1>
          {/* Always drawn: the people item is always there, so a note with nobody on it can be given someone (KNEG-362). */}
          <ul className="note-meta">
            <NotePeople disabled={blocked !== null} noteId={note.id} participants={note.participants} onSaved={reload} />
          </ul>
          {/* Where the transcript was made (KNEG-368). Notes saved before it was recorded say nothing. */}
          {transcribed === null ? null : <p className="note-where"><Icon name="computer" size={18} />{transcribed}</p>}
        </header>
        <div className="note-controls">
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
          <div className="note-actions">
            {/* Keep the live region mounted so save changes are announced reliably. */}
            <p className="save-status" role="status" aria-live="polite">{workspaceSaveText(autosave.status, summarySave)}</p>
            <button type="button" className="note-action-button" aria-label="Export…" title="Export…" onClick={onExport}><Icon name="export" size={16} /></button>
            <button type="button" className="note-action-button" disabled={blocked !== null} aria-label="More" title="More" aria-haspopup="dialog" aria-expanded={moreOpen} onClick={event => onMore(event.currentTarget)}><Icon name="more" /></button>
            {/* Only where there is a column to hide. `aria-controls` only while it is on screen, so it never names nothing. */}
            {windowWide && hasTranscript ? <button type="button" className="note-action-button" aria-label={columnHidden ? "Show transcript" : "Hide transcript"}
              title={columnHidden ? "Show transcript" : "Hide transcript"} aria-expanded={!columnHidden}
              aria-controls={wide ? `note-${note.id}-side` : undefined} onClick={toggleColumn}><Icon name="panelRight" /></button> : null}
          </div>
        </div>
        {blocked && <div className="caveat" role="status">
          <p>{blocked === "trashed" ? "This note was moved to Trash elsewhere. Your unsaved draft is retained here." : blocked === "deleted" ? "This note was deleted elsewhere. Your unsaved draft is retained here." : "The note was restored. Save the retained draft explicitly before continuing to edit."}</p>
          <button type="button" onClick={() => { void navigator.clipboard.writeText(`# ${draft.current.title}\n\n${draft.current.content}\n\n${summaryRef.current?.draft() ?? ""}`).then(() => setDraftNotice("Draft copied")).catch(cause => setError(message(cause))); }}>Copy draft</button>
          {blocked === "trashed" && <button type="button" onClick={() => { void restoreNote(note.id).then(restored => { if (!restored) throw new Error("The note could not be restored."); blockedRef.current = "restored"; setBlocked("restored"); }).catch(cause => setError(message(cause))); }}>Restore note</button>}
          {blocked === "restored" && <button type="button" onClick={() => { blockedRef.current = null; setBlocked(null); autosave.schedule({ ...draft.current }); setTimeout(() => { void flush().catch(cause => setError(message(cause))); }, 0); }}>Save retained draft</button>}
          <button type="button" onClick={() => { void askConfirm("Discard the unsaved draft? Copy it first if you want to keep it.", { confirm: "Discard draft", destructive: true }).then((yes) => { if (!yes) return; autosave.discard(); summaryRef.current?.discardDraft(); setDraftNotice("Unsaved draft discarded. You can open another note."); }); }}>Discard unsaved draft</button>
          {draftNotice && <p>{draftNotice}</p>}
        </div>}
        {error && <p className="error" role="alert">{error}</p>}
        <div className="note-workspace">
          <section id={`note-${note.id}-panel-notes`} role="tabpanel" aria-labelledby={`note-${note.id}-tab-notes`} hidden={tab !== "notes"}>
            <div className="note-prose">
              <h2 className="note-section-heading">your notes</h2>
              {ready && empty && transcript?.rows.some(row => row.text.trim()) && <button type="button" className="start-from-transcript" disabled={blocked !== null} onClick={startFromTranscript}>Start from transcript</button>}
              <NoteEditor readOnly={blocked !== null} key={seed.generation} noteId={note.id} initialContent={seed.content}
                {...(seed.generation === 0 ? { legacyTitle: note.title } : {})}
                onReady={content => { draft.current.content = content; setEmpty(!content.trim()); setReady(true); }}
                onChange={content => { draft.current.content = content; setEmpty(!content.trim()); autosave.schedule({ content }); }} />
            </div>
            <h2 className="note-section-heading note-summary-heading">ai summary</h2>
            <SummaryPane readOnly={blocked !== null} ref={summaryRef} noteId={note.id} visible={tab === "notes"}
              hasTranscript={transcript === null ? null : transcript.rows.some(row => row.text.trim())}
              focusOnReady={focusSummary}
              onSetup={onSummarySetup} onSource={index => { void showSource(index); }} onSaveStatus={setSummarySave} />
          </section>
          {/* The ref changes with the tab, the element does not: Replay's full player is not a card.
              The mini card waits for playback only on Notes; on Transcript it is there from the
              start, with its header (KNEG-448). */}
          {player && playerSlot === "note" ? <div ref={tab === "replay" ? holdPlayer : holdObservedPlayer} className={tab === "replay" ? "note-full-player" : "note-mini-player"} hidden={tab === "notes" && !playbackStarted} /> : null}
          <section id={`note-${note.id}-panel-replay`} role="tabpanel"
            aria-labelledby={`note-${note.id}-tab-replay`} hidden={tab !== "replay"}>
            {recordingDetails}{disposition}
            {transcriptError ? <p className="error" role="alert">{transcriptError} <button type="button" onClick={() => { void loadTranscript(); }}>Retry</button></p> : !transcript ? <p role="status">Loading recording…</p> : !transcript.recording && <div className="recording-empty">
              <p>{recordingState?.status === "deleted" || recordingState?.status === "deleted-update-failed" ? "If you have another copy, you can attach it to replay this note." : "This note has no recording attached. Select its original recording to replay it. A copy is saved; your original stays in place."}</p>
              <button type="button" disabled={blocked !== null || attaching || retryingDeletion} onClick={() => { void attachRecording(); }}>{attaching ? "Attaching…" : recordingState?.status === "deleted" || recordingState?.status === "deleted-update-failed" ? "Attach another recording…" : "Attach recording…"}</button>
              <p className="muted">16-bit PCM WAV, mono or stereo.</p>
              {attachmentError && <p className="error" role="alert">{attachmentError}</p>}
            </div>}
          </section>
        </div>
      </div>
    </div>
    {/* No recording details here: the Replay tab still has them, and beside it they would be on screen twice. */}
    <div id={wide ? `note-${note.id}-side` : `note-${note.id}-panel-transcript`}
      role={wide ? undefined : "tabpanel"} aria-labelledby={wide ? undefined : `note-${note.id}-tab-transcript`}
      className={wide ? "note-side" : "note-transcript-inline"} hidden={!wide && tab !== "transcript"}>
      {!wide && <>{recordingDetails}{disposition}</>}
      <div className="note-side-body">{speakerLabelStatus}{transcriptShown ? null : <TranscriptHeading />}{transcriptBody}</div>
      {player && playerSlot === "side" ? <div ref={holdObservedPlayer} className="note-side-player" /> : null}
    </div>
    {player && playerSlot === "inline" ? <div className="note-inline-player" hidden={tab === "notes" && !playbackStarted}>
      <div ref={holdObservedPlayer} className="note-mini-player" hidden={tab === "notes" && !playbackStarted} />
    </div> : null}
    {player ? createPortal(player, playerHost) : null}
  </article>;
});
