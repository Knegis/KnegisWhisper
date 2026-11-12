import { SettingsDraftsProvider, useSettingsNavigation } from "./components/SettingsDrafts";
import { TitleBar } from "./components/TitleBar";
import type { SummarySetupTarget } from "./lib/ipc";
import { LanguagePopup } from "./components/LanguagePopup";
/**
 * The shell: a title bar carrying the navigation pills, the page under it --
 * with a list panel beside it on the pages that have one -- the Settings
 * sheet, and an error boundary. Nothing else.
 *
 * This file was 1037 lines holding every screen the application had, which is
 * how four UI surfaces came to live in one component. Splitting it was the
 * precondition for the nine that do not exist yet — `config/ui-surfaces.json`
 * names a component per surface and the parity gate checks it, so a screen that
 * is not its own file cannot honestly be claimed.
 *
 * # Navigation
 *
 * A `view` state and a switch, not a router. The URL fragment is already claimed
 * — `#overlay`, `#meeting`, and `#update` each select a window out of the same
 * bundle — and a router would be a dependency that makes one `switch` look
 * like a different `switch`. `currentSurface()` owns the fragment mapping, so
 * this module never repeats it for routing or root-element setup.
 * In-memory history tracks pages and individual notes within the main window.
 *
 * The list of screens is no longer here. `lib/navigation.ts` holds the tree —
 * three top-level destinations plus the `Settings` and `Developer` groups —
 * and the four pills drawn from it. `components/NavPills.tsx` renders the
 * pills and `components/SettingsSheet.tsx` shows both groups' screens as
 * tiles. Two things follow from that split:
 *
 *   - **`ViewId` is derived from the tree**, so the switch below is exhaustive
 *     over exactly the screens the pills and the Settings sheet can reach.
 *     Adding a nav entry without a branch here is a compile error rather than
 *     an item that opens a blank window, which is what the old chain of
 *     ternaries did.
 *   - **Neither the pills nor the sheet holds navigation state.** That is the
 *     history below, and which screen reads as current is derived from it:
 *     `railOf(view)` for the pills, `view` itself on the sheet's tiles. The
 *     sheet holds only whether it is open, so a sideways "go to Providers" or
 *     the error boundary's jump to Diagnostics marks its target without help.
 *
 * # The card
 *
 * Every page sits in one card under the title bar. A settings or developer
 * screen is a centred column with a "Settings" pill above its title that
 * reopens the sheet; it has no list panel. Dictation, Meetings and Notes each
 * render their own `SplitView` -- its toggle and the collapsed state reach it
 * through `PanelCollapsed` rather than props -- because they own their panel's
 * data -- the
 * history, the recent and interrupted recordings, the notes library -- and
 * each goes into the card bare, with no `.content` and no `PageHeader`: a
 * task hero says what Dictation and Meetings are for, and a note's own title
 * is the heading on Notes. Notes > Upload is the one page with no panel; it
 * keeps its `PageHeader`.
 * Providers keeps a `PageHeader` too, but draws it itself: its search field
 * and Test connections sit in the header, and what they hold is the page's
 * own state, so this file leaves its copy out for that one view.
 *
 * # The error boundary
 *
 * A thrown render error in a Tauri webview is a blank window. There is no tab to
 * reload and no console in a bundled build, so the boundary is the only thing
 * standing between a mistake in one view and an application that appears to have
 * died.
 */

import { Component, useCallback, useEffect, useMemo, useReducer, useRef, useState, type ReactNode } from "react";

import { PageHeader } from "./components/PageHeader";
import { BackToSettings, SettingsSheet } from "./components/SettingsSheet";
import { ConfirmHost } from "./components/ConfirmDialog";
import { PanelCollapsed } from "./components/SplitView";
import { getDiagnostics, getMeetingStatus, onMeetingState, type Diagnostics, type MeetingState } from "./lib/ipc";
import { eyebrowOf, railOf, type ViewId } from "./lib/navigation";
import { defaultDestination, initialHistory, navigationHistory, settingsReturnDestination } from "./lib/navigationHistory";
import { PAGES } from "./lib/pages";
import { currentSurface } from "./lib/surface";
import { installIdleScrollbars } from "./lib/idleScrollbars";
import { installWindowActivity } from "./lib/windowActivity";
import { useRefresh } from "./lib/useRefresh";
import { MeetingNotification } from "./MeetingNotification";
import { Overlay } from "./Overlay";
import { Tooltips } from "./components/Tooltip";
import { UpdateNotification } from "./UpdateNotification";
import { DiagnosticsView } from "./views/Diagnostics";
import { DictationView } from "./views/Dictation";
import { IntegrationsApi } from "./views/IntegrationsApi";
import { IntegrationsCalendar } from "./views/IntegrationsCalendar";
import { IntegrationsCli } from "./views/IntegrationsCli";
import { IntegrationsMcp } from "./views/IntegrationsMcp";
import { Meetings } from "./views/Meetings";
import { Vocabulary } from "./views/Vocabulary";
import { Notes } from "./views/Notes";
import { NotesUpload } from "./views/NotesUpload";
import { SettingsMeetings } from "./views/SettingsMeetings";
import { SettingsIntelligence } from "./views/SettingsIntelligence";
import { SettingsIntelligenceLocal } from "./views/SettingsIntelligenceLocal";
import { SettingsProviders } from "./views/SettingsProviders";
import { SettingsShortcuts } from "./views/SettingsShortcuts";
import { SettingsTranscription } from "./views/SettingsTranscription";

/**
 * Catch a render error and say what it was.
 *
 * Deliberately a class: `componentDidCatch` has no hook equivalent. It shows the
 * message rather than a generic apology, because the message is the only thing
 * anyone can act on, and it offers a way out to a screen that was working.
 */
class ViewBoundary extends Component<
  { children: ReactNode; view: ViewId; onReset: () => void },
  { failed: string | null }
> {
  constructor(props: {
    children: ReactNode;
    view: ViewId;
    onReset: () => void;
  }) {
    super(props);
    this.state = { failed: null };
  }

  static getDerivedStateFromError(error: unknown) {
    return { failed: error instanceof Error ? error.message : String(error) };
  }

  override componentDidUpdate(prev: { view: ViewId }) {
    // Leaving a broken screen clears the failure. Without this, one thrown
    // render would leave the boundary latched for the rest of the session.
    if (prev.view !== this.props.view && this.state.failed !== null) {
      this.setState({ failed: null });
    }
  }

  override render() {
    if (this.state.failed === null) return this.props.children;
    return (
      <div className="view">
        <section>
          <h2>This screen failed to render</h2>
          <p className="error">{this.state.failed}</p>
          <p className="muted">
            Nothing was changed by the failure. Every screen reads from Rust and
            writes only when you press something, so the rest of the application
            is still working.
          </p>
          <button type="button" onClick={this.props.onReset}>
            Go to Diagnostics
          </button>
        </section>
      </div>
    );
  }
}

/**
 * One screen per `ViewId`, with the `never` guard the repository already uses at
 * `lib/labels.ts:952`.
 *
 * A `switch` rather than the chain of `{view === "x" ? <X /> : null}` ternaries
 * this replaced. The ternaries were not exhaustive over anything: a nav entry
 * with no matching line rendered an empty content pane and no error, and nesting
 * the entries under two groups makes that mismatch easier to introduce, not
 * harder. The `default` arm is what turns it into a type error instead.
 */
function renderView(
  view: ViewId,
  diagnostics: Diagnostics,
  select: (id: ViewId) => void,
  searchNoteRequest: { id: number; transcriptRow: number | null; summaryFocus?: boolean } | null,
  openNote: (id: number, transcriptRow?: number) => void,
  openSummarySetup: (id: number, target: SummarySetupTarget) => void,
  summaryFocus: number | undefined,
): ReactNode {
  switch (view) {
    case "dictation":
      return <DictationView onGoTo={select} />;
    case "shortcuts":
      return <SettingsShortcuts hook={diagnostics.modifierOnlyShortcuts} />;
    case "meetings":
      return <Meetings onGoTo={select} onOpenNote={openNote} />;
    case "meeting-settings":
      return <SettingsMeetings onGoToVocabulary={() => select("vocabulary")} />;
    case "notes":
      return <Notes openRequest={searchNoteRequest} onOpenNote={openNote} onImport={() => select("notes-upload")} onSummarySetup={openSummarySetup} />;
    case "vocabulary":
      return <Vocabulary />;
    case "notes-upload":
      return <NotesUpload onOpenNote={openNote} />;
    case "providers":
      return <SettingsProviders onGoTo={select} />;
    case "transcription":
      return (
        <SettingsTranscription onGoToProviders={() => select("providers")} onGoToVocabulary={() => select("vocabulary")} />
      );
    case "intelligence":
      return (
        <SettingsIntelligence onGoToProviders={() => select("providers")} onGoToLocalModels={() => select("local-llm")} summaryFocus={summaryFocus} />
      );
    case "local-llm":
      return <SettingsIntelligenceLocal />;
    case "calendar":
      return <IntegrationsCalendar />;
    case "api":
      return <IntegrationsApi />;
    case "cli":
      return <IntegrationsCli onGoToApi={() => select("api")} />;
    case "mcp":
      return <IntegrationsMcp onGoToApi={() => select("api")} />;
    case "diagnostics":
      return <DiagnosticsView diagnostics={diagnostics} />;
    default: {
      const exhaustive: never = view;
      return exhaustive;
    }
  }
}

function Shell() {
  const load = useCallback(() => getDiagnostics(), []);
  const { data: diagnostics, error } = useRefresh(load);

  // Not persisted, and starting on Dictation because that is what the
  // application is for. Reopening on the screen you last used sounds helpful
  // until the screen you last used is the one that failed.
  const [history, dispatchNavigate] = useReducer(navigationHistory, initialHistory);
  const requestNavigation = useSettingsNavigation();
  const [headingRequest, setHeadingRequest] = useState(0);
  const focusHeading = useRef(false);
  const navigate = useCallback((action: Parameters<typeof navigationHistory>[1], focus = false) => {
    void requestNavigation(() => {
      focusHeading.current = focus;
      setHeadingRequest(value => value + 1);
      if (action.type === "visit" && railOf(action.destination.view) !== "settings") setSummarySetup(null);
      dispatchNavigate(action);
    });
  }, [requestNavigation]);
  const [summarySetup, setSummarySetup] = useState<{ noteId: number; nonce: number } | null>(null);
  const destination = history.entries[history.index] ?? defaultDestination;
  const view = destination.view;
  // A local variable, because TypeScript does not narrow a repeated
  // `PAGES[view]`.
  const page = PAGES[view];
  // A new request for each destination, so a row chosen again is shown again (KNEG-406).
  const searchNoteRequest = useMemo(() => destination.noteId === undefined ? null : { id: destination.noteId, transcriptRow: destination.transcriptRow ?? null, ...(destination.summaryFocus ? { summaryFocus: true } : {}) }, [destination]);
  const openNote = useCallback((id: number, transcriptRow?: number) => navigate({ type: "visit", destination: { view: "notes", noteId: id, ...(transcriptRow === undefined ? {} : { transcriptRow }) } }), [navigate]);

  /*
   * Go to a screen.
   *
   * Every route to a screen comes through here -- a pill press, a Settings
   * sheet tile, Dictation's, Meetings' and Providers' `onGoTo`, either
   * `onGoToProviders`, both `onGoToVocabulary`, and the error boundary's
   * reset. Nothing else has to happen on arrival: the pills' mark and the
   * sheet's are both derived from `view`.
   */
  const select = useCallback((id: ViewId) => {
    navigate({ type: "visit", destination: { view: id } }, true);
  }, [navigate]);
  const openSummarySetup = useCallback((noteId: number, _target: SummarySetupTarget) => {
    void requestNavigation(() => {
      setSummarySetup({ noteId, nonce: Date.now() });
      dispatchNavigate({ type: "visit", destination: { view: "intelligence" } });
    });
  }, [requestNavigation]);
  const returnToSummary = () => {
    if (summarySetup === null) return;
    const noteId = summarySetup.noteId;
    void requestNavigation(() => {
      setSummarySetup(null);
      dispatchNavigate({ type: "visit", destination: { view: "notes", noteId, summaryFocus: true } });
    });
  };

  // Whether the Settings sheet is over the window. Nothing else about it is state.
  const [sheetOpen, setSheetOpen] = useState(false);
  const openSheet = useCallback(() => setSheetOpen(true), []);

  const selectFromSheet = useCallback((id: ViewId) => {
    navigate({ type: "visit", destination: { view: id } }, true);
  }, [navigate]);
  const dismissSheet = () => {
    setSheetOpen(false);
    if (railOf(view) === "settings") {
      navigate({ type: "visit", destination: settingsReturnDestination(history) }, true);
    }
  };
  useEffect(() => {
    if (!focusHeading.current) return;
    focusHeading.current = false;
    // Notes may still be loading. Wait for the actual heading, but stop if
    // the person acts or another destination replaces this one.
    const stop = () => {
      observer.disconnect();
      document.removeEventListener("pointerdown", stop, true);
      document.removeEventListener("keydown", stop, true);
    };
    const focus = () => {
      const heading = document.querySelector<HTMLElement>(".split-main h1");
      if (!heading || heading.closest('[aria-busy="true"]')) return;
      heading.tabIndex = -1;
      if (document.activeElement !== heading) heading.focus({ preventScroll: true });
    };
    const observer = new MutationObserver(focus);
    observer.observe(document.querySelector("main.card") ?? document.body, { childList: true, subtree: true, attributes: true, attributeFilter: ["aria-busy"] });
    document.addEventListener("pointerdown", stop, true);
    document.addEventListener("keydown", stop, true);
    focus();
    return stop;
  }, [destination, diagnostics, headingRequest]);

  const ownsPanel = view === "dictation" || view === "meetings" || view === "notes";

  // Rust announces the meeting's state wherever the window is, so the pill can
  // say so from any page. The first read covers a window opened mid-meeting.
  // Only while audio is being captured: stalled and finishing record nothing.
  const capturing = (state: MeetingState) => state === "recording" || state === "recording-without-transcript";
  const [meetingRecording, setMeetingRecording] = useState(false);
  useEffect(() => {
    let disposed = false;
    let stop: (() => void) | undefined;
    void getMeetingStatus().then((status) => { if (!disposed) setMeetingRecording(capturing(status.state)); }).catch(() => {});
    void onMeetingState((state) => { if (!disposed) setMeetingRecording(capturing(state)); }).then((unlisten) => {
      if (disposed) unlisten(); else stop = unlisten;
    });
    return () => { disposed = true; stop?.(); };
  }, []);

  const titlebar = <TitleBar view={view} onSelect={select} sheetOpen={sheetOpen} onOpenSheet={openSheet}
    onOpenNote={openNote} meetingRecording={meetingRecording}
    canGoBack={history.index > 0} canGoForward={history.index < history.entries.length - 1}
    onBack={() => navigate({ type: "back" })} onForward={() => navigate({ type: "forward" })} />;

  // The pills need nothing from diagnostics, so they are drawn from the first
  // frame and do not jump in when the first read lands; only the card waits.
  const frame = (card: ReactNode) => (
    <>{titlebar}<div className="frame">
      <main className="card">{card}</main>
    </div>
    <SettingsSheet open={sheetOpen} view={view} version={diagnostics?.appVersion ?? null} onSelect={selectFromSheet} onHide={() => setSheetOpen(false)} onDismiss={dismissSheet} /></>
  );

  if (!diagnostics) {
    return frame(
      <div className="split-main">
        <div className="content">
          <h1>KnegisWhisper</h1>
          {error ? (
            <p className="error">{error}</p>
          ) : (
            <p className="muted">Loading…</p>
          )}
        </div>
      </div>,
    );
  }

  const boundary = (
    <ViewBoundary view={view} onReset={() => select("diagnostics")}>
      {renderView(view, diagnostics, select, searchNoteRequest, openNote, openSummarySetup, summarySetup?.nonce)}
    </ViewBoundary>
  );

  // Dictation, Meetings and Notes draw their own `SplitView` -- Dictation's
  // panel is its history, Meetings' its recent and interrupted recordings,
  // Notes' its library -- so each goes into the card bare, with no `.content`
  // and no `PageHeader`.
  if (ownsPanel) return frame(boundary);

  // A settings or developer screen leads with the way back to the sheet.
  const content = (
    <>
      {railOf(view) === "settings" ? <div className="page-back"><BackToSettings onOpen={openSheet} /></div> : null}
      <div className={`content content-${view}`}>
      {page ? <PageHeader page={page} eyebrow={eyebrowOf(view)} /> : null}
      {summarySetup !== null && railOf(view) === "settings" && <div className="controls summary-setup-return"><button type="button" onClick={returnToSummary}>Return to note</button></div>}
      {boundary}
      </div>
    </>
  );

  // Every other screen -- the settings and developer screens, and
  // `notes-upload` -- scrolls in `.split-main` with no panel beside it: the
  // card clips its overflow, so the page cannot scroll the card itself.
  return frame(<div className="split-main">{content}</div>);
}

/** The width at and below which the list panel starts, and falls, collapsed. */
const NARROW = "(max-width: 900px)";

/**
 * Four windows are served by one bundle, routed through `currentSurface()`. One
 * bundle means one CSP and one build.
 */
function MainWindow() {
  useEffect(installIdleScrollbars, []);
  useEffect(installWindowActivity, []);
  // Collapsed from the start at 900px and below, and collapsed again whenever
  // the window narrows past that -- the rule Notes' own library followed
  // before it became this panel. At the 720x520 minimum the panel would take a large share of the
  // window from the page. Widening never reopens it: the panel only reappears when
  // someone asks for it.
  const [panelCollapsed, setPanelCollapsed] = useState(() => window.matchMedia(NARROW).matches);
  useEffect(() => {
    const narrow = window.matchMedia(NARROW);
    const collapse = () => { if (narrow.matches) setPanelCollapsed(true); };
    narrow.addEventListener("change", collapse);
    return () => narrow.removeEventListener("change", collapse);
  }, []);
  const toggle = useCallback(() => setPanelCollapsed(collapsed => !collapsed), []);
  const panel = useMemo(() => ({ collapsed: panelCollapsed, toggle }), [panelCollapsed, toggle]);
  return <PanelCollapsed.Provider value={panel}><Shell /><Tooltips /><ConfirmHost /></PanelCollapsed.Provider>;
}

export default function App() {
  // One bundle, multiple surfaces. The closed union and no `default` make a new
  // surface a compile-visible addition rather than a quiet fall-through.
  switch (currentSurface()) {
    case "main":
      return <SettingsDraftsProvider><MainWindow /></SettingsDraftsProvider>;
    case "dictation-languages":
      return <LanguagePopup />;
    case "overlay":
      return <Overlay />;
    case "meeting":
      return <MeetingNotification />;
    case "update":
      return <UpdateNotification />;
  }
}
