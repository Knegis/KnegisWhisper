import { TitleBar } from "./components/TitleBar";
import { LanguagePopup } from "./components/LanguagePopup";
/**
 * The shell: a title bar, a rail, and a card holding the page -- with a list
 * panel beside it on the pages that have one -- plus an error boundary.
 * Nothing else.
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
 * and the four-entry rail drawn from it. `components/Rail.tsx` renders the
 * rail and `components/SettingsPanel.tsx` lists both groups' screens. Two
 * things follow from that split:
 *
 *   - **`ViewId` is derived from the tree**, so the switch below is exhaustive
 *     over exactly the screens the rail and the Settings panel can reach.
 *     Adding a nav entry without a branch here is a compile error rather than
 *     an item that opens a blank window, which is what the old chain of
 *     ternaries did.
 *   - **Neither the rail nor the panel holds state.** Navigation state is the
 *     history below, and which screen reads as current is derived from it:
 *     `railOf(view)` for the rail, `view` itself in the panel. Nothing has to
 *     be opened to show where you are -- every settings screen is always
 *     listed in the panel -- so a sideways "go to Providers" or the error
 *     boundary's jump to Diagnostics marks its target without any help.
 *
 * # The card
 *
 * Every page sits in one card beside the rail. A settings or developer screen
 * gets `SplitView` with the shared `SettingsPanel`; the title bar's toggle
 * collapses that panel, and the collapsed state reaches it through
 * `PanelCollapsed` rather than props. Dictation, Meetings and Notes each
 * render their own `SplitView`, because they own their panel's data -- the
 * history, the recent and interrupted recordings, the notes library -- and
 * each goes into the card bare, with no `.content` and no `PageHeader`: a
 * task hero says what Dictation and Meetings are for, and a note's own title
 * is the heading on Notes. Notes > Upload is the one page with no panel; it
 * keeps its `PageHeader`, and the title bar draws no panel toggle for it.
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
import { Rail } from "./components/Rail";
import { SettingsPanel } from "./components/SettingsPanel";
import { PanelCollapsed, SplitView } from "./components/SplitView";
import { getDiagnostics, type Diagnostics } from "./lib/ipc";
import { railOf, SETTINGS_HOME, type ViewId } from "./lib/navigation";
import { defaultDestination, initialHistory, navigationHistory } from "./lib/navigationHistory";
import { PAGES } from "./lib/pages";
import { currentSurface } from "./lib/surface";
import { installIdleScrollbars } from "./lib/idleScrollbars";
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
  { children: ReactNode; view: ViewId; onReset: () => void; onFailedChange: (failed: boolean) => void },
  { failed: string | null }
> {
  constructor(props: {
    children: ReactNode;
    view: ViewId;
    onReset: () => void;
    onFailedChange: (failed: boolean) => void;
  }) {
    super(props);
    this.state = { failed: null };
  }

  static getDerivedStateFromError(error: unknown) {
    return { failed: error instanceof Error ? error.message : String(error) };
  }

  // `onFailedChange` tells the shell whether the page's own markup is on
  // screen: a task page that failed has no list panel for the title bar's
  // toggle to name.
  override componentDidCatch() {
    this.props.onFailedChange(true);
  }

  override componentDidUpdate(prev: { view: ViewId }) {
    // Leaving a broken screen clears the failure. Without this, one thrown
    // render would leave the boundary latched for the rest of the session.
    if (prev.view !== this.props.view && this.state.failed !== null) {
      this.setState({ failed: null });
      this.props.onFailedChange(false);
    }
  }

  override componentWillUnmount() {
    // A task page's boundary and a settings page's sit in different places,
    // so moving between them replaces the boundary rather than updating it.
    if (this.state.failed !== null) this.props.onFailedChange(false);
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
  searchNoteRequest: { id: number; transcriptRow: number | null } | null,
  openNote: (id: number, transcriptRow?: number) => void,
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
      return <Notes openRequest={searchNoteRequest} onOpenNote={openNote} onImport={() => select("notes-upload")} />;
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
        <SettingsIntelligence onGoToProviders={() => select("providers")} />
      );
    case "local-llm":
      return <SettingsIntelligenceLocal />;
    case "calendar":
      return <IntegrationsCalendar />;
    case "api":
      return <IntegrationsApi />;
    case "cli":
      return <IntegrationsCli />;
    case "mcp":
      return <IntegrationsMcp />;
    case "diagnostics":
      return <DiagnosticsView diagnostics={diagnostics} />;
    default: {
      const exhaustive: never = view;
      return exhaustive;
    }
  }
}

function Shell({ panelCollapsed, onTogglePanel }: { panelCollapsed: boolean; onTogglePanel: () => void }) {
  const load = useCallback(() => getDiagnostics(), []);
  const { data: diagnostics, error } = useRefresh(load);

  // Not persisted, and starting on Dictation because that is what the
  // application is for. Reopening on the screen you last used sounds helpful
  // until the screen you last used is the one that failed.
  const [history, navigate] = useReducer(navigationHistory, initialHistory);
  const destination = history.entries[history.index] ?? defaultDestination;
  const view = destination.view;
  // A local variable, because TypeScript does not narrow a repeated
  // `PAGES[view]`.
  const page = PAGES[view];
  // A new request for each destination, so a row chosen again is shown again (KNEG-406).
  const searchNoteRequest = useMemo(() => destination.noteId === undefined ? null : { id: destination.noteId, transcriptRow: destination.transcriptRow ?? null }, [destination]);
  const openNote = useCallback((id: number, transcriptRow?: number) => navigate({ type: "visit", destination: { view: "notes", noteId: id, ...(transcriptRow === undefined ? {} : { transcriptRow }) } }), []);

  /*
   * Go to a screen.
   *
   * Every route to a screen comes through here -- a rail press, a Settings
   * panel press, Dictation's, Meetings' and Providers' `onGoTo`, either
   * `onGoToProviders`, both `onGoToVocabulary`, and the error boundary's
   * reset. Nothing else has to happen on arrival: the rail's mark and the
   * panel's are both derived from `view`.
   */
  const select = useCallback((id: ViewId) => {
    navigate({ type: "visit", destination: { view: id } });
  }, []);

  /*
   * The settings screen the rail's Settings reopens: the last one visited
   * this session, or Shortcuts before any has been. A ref, not state -- only
   * the rail's click reads it, so a change has nothing to re-render -- and
   * written after each navigation rather than during render, whatever route
   * the navigation took (back and forward included).
   */
  const lastSettings = useRef<ViewId>(SETTINGS_HOME);
  useEffect(() => {
    if (railOf(view) === "settings") lastSettings.current = view;
  }, [view]);

  // Whether the page that failed to render is one that draws its own panel.
  const [boundaryFailed, setBoundaryFailed] = useState(false);
  const ownsPanel = view === "dictation" || view === "meetings" || view === "notes";

  // Every page draws a list panel but Notes > Upload -- see `# The card`
  // above -- and the title bar's toggle is drawn only where there is one to
  // toggle. Not while the card is still loading or failed to load, which
  // draws no page at all, and not while Dictation, Meetings or Notes shows
  // the error boundary in place of its `SplitView`; a settings page's
  // boundary is inside the shared `SplitView`, so its panel stays.
  const hasListPanel = diagnostics !== null && view !== "notes-upload" && !(ownsPanel && boundaryFailed);
  const titlebar = <TitleBar panelCollapsed={panelCollapsed} hasListPanel={hasListPanel} onTogglePanel={onTogglePanel} onOpenNote={openNote}
    canGoBack={history.index > 0} canGoForward={history.index < history.entries.length - 1}
    onBack={() => navigate({ type: "back" })} onForward={() => navigate({ type: "forward" })} />;

  // The rail needs nothing from diagnostics, so it is drawn from the first
  // frame and does not jump in when the first read lands; only the card waits.
  const frame = (card: ReactNode) => (
    <>{titlebar}<PanelCollapsed.Provider value={panelCollapsed}>
      <div className="frame">
        <Rail view={view} lastSettings={lastSettings} onSelect={select} />
        <main className="card">{card}</main>
      </div>
    </PanelCollapsed.Provider></>
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
    <ViewBoundary view={view} onReset={() => select("diagnostics")} onFailedChange={setBoundaryFailed}>
      {renderView(view, diagnostics, select, searchNoteRequest, openNote)}
    </ViewBoundary>
  );

  // Dictation, Meetings and Notes draw their own `SplitView` -- Dictation's
  // panel is its history, Meetings' its recent and interrupted recordings,
  // Notes' its library -- so each goes into the card bare, with no `.content`
  // and no `PageHeader`.
  if (ownsPanel) return frame(boundary);

  // Providers draws its own header, with its search and Test connections in it.
  const content = (
    <div className="content">
      {page && view !== "providers" ? <PageHeader page={page} /> : null}
      {boundary}
    </div>
  );

  // Every settings and developer screen shares one panel. The rest --
  // `notes-upload` -- scrolls in `.split-main` as a split with no panel
  // beside it: the card clips its overflow, so the page cannot scroll the
  // card itself. The settings panel's landmark is "Settings list", as
  // Meetings' is "Meetings list": "Settings sections" is the `nav` inside it,
  // and a landmark named the same as the one it holds says nothing more.
  if (railOf(view) === "settings") {
    return frame(
      <SplitView label="Settings list" panel={<SettingsPanel view={view} onSelect={select} />}>
        {content}
      </SplitView>,
    );
  }
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
  // Collapsed from the start at 900px and below, and collapsed again whenever
  // the window narrows past that -- the rule Notes' own library followed
  // before it became this panel. At the 720x520 minimum, rail and panel together would leave the
  // page about 372px. Widening never reopens it: the panel only reappears when
  // someone asks for it.
  const [panelCollapsed, setPanelCollapsed] = useState(() => window.matchMedia(NARROW).matches);
  useEffect(() => {
    const narrow = window.matchMedia(NARROW);
    const collapse = () => { if (narrow.matches) setPanelCollapsed(true); };
    narrow.addEventListener("change", collapse);
    return () => narrow.removeEventListener("change", collapse);
  }, []);
  const togglePanel = useCallback(() => setPanelCollapsed(collapsed => !collapsed), []);
  return <><Shell panelCollapsed={panelCollapsed} onTogglePanel={togglePanel} /><Tooltips /></>;
}

export default function App() {
  // One bundle, multiple surfaces. The closed union and no `default` make a new
  // surface a compile-visible addition rather than a quiet fall-through.
  switch (currentSurface()) {
    case "main":
      return <MainWindow />;
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
