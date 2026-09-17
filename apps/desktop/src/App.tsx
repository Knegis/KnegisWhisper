import { TitleBar } from "./components/TitleBar";
import { LanguagePopup } from "./components/LanguagePopup";
/**
 * The shell: a rail, a view, and an error boundary. Nothing else.
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
 * four top-level destinations plus the `Settings` and `Developer` groups — and
 * `components/Sidebar.tsx` renders it. Two things follow from that split:
 *
 *   - **`ViewId` is derived from the tree**, so the switch below is exhaustive
 *     over exactly the entries the rail can reach. Adding a nav entry without a
 *     branch here is a compile error rather than a rail item that opens a blank
 *     window, which is what the old chain of ternaries did.
 *   - **The rail holds no state.** There are two pieces of navigation state --
 *     the current view and which groups are open -- and both live here, so
 *     `select` below can open the group holding a screen whatever route
 *     arrived at it. `Sidebar.tsx` records why a group header discloses rather
 *     than navigates.
 *
 * # The error boundary
 *
 * A thrown render error in a Tauri webview is a blank window. There is no tab to
 * reload and no console in a bundled build, so the boundary is the only thing
 * standing between a mistake in one view and an application that appears to have
 * died.
 */

import { Component, useCallback, useEffect, useMemo, useReducer, useState, type ReactNode } from "react";

import { Sidebar } from "./components/Sidebar";
import { getDiagnostics, type Diagnostics } from "./lib/ipc";
import { groupOf, type ViewId } from "./lib/navigation";
import { defaultDestination, initialHistory, navigationHistory } from "./lib/navigationHistory";
import { currentSurface } from "./lib/surface";
import { installIdleScrollbars } from "./lib/idleScrollbars";
import { useRefresh } from "./lib/useRefresh";
import { MeetingNotification } from "./MeetingNotification";
import { Overlay } from "./Overlay";
import { UpdateNotification } from "./UpdateNotification";
import { DiagnosticsView } from "./views/Diagnostics";
import { DictationView } from "./views/Dictation";
import { IntegrationsApi } from "./views/IntegrationsApi";
import { IntegrationsCalendar } from "./views/IntegrationsCalendar";
import { IntegrationsCli } from "./views/IntegrationsCli";
import { IntegrationsMcp } from "./views/IntegrationsMcp";
import { Vocabulary } from "./views/Vocabulary";
import { Notes } from "./views/Notes";
import { NotesUpload } from "./views/NotesUpload";
import { SettingsMeetings } from "./views/SettingsMeetings";
import { SettingsIntelligence } from "./views/SettingsIntelligence";
import { SettingsIntelligenceLocal } from "./views/SettingsIntelligenceLocal";
import { SettingsProviders } from "./views/SettingsProviders";
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
  searchNoteRequest: { id: number } | null,
  openImportedNote: (id: number) => void,
): ReactNode {
  switch (view) {
    case "dictation":
      return <DictationView hook={diagnostics.modifierOnlyShortcuts} />;
    case "meetings":
      return <SettingsMeetings onGoToVocabulary={() => select("vocabulary")} />;
    case "notes":
      return <Notes openRequest={searchNoteRequest} onOpenNote={openImportedNote} />;
    case "vocabulary":
      return <Vocabulary />;
    case "notes-upload":
      return <NotesUpload onOpenNote={openImportedNote} />;
    case "providers":
      return <SettingsProviders />;
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

function Shell({ sidebarExpanded, onExpandSidebar, onToggleSidebar }: { sidebarExpanded: boolean; onExpandSidebar: () => void; onToggleSidebar: () => void }) {
  const load = useCallback(() => getDiagnostics(), []);
  const { data: diagnostics, error } = useRefresh(load);

  // Not persisted, and starting on Dictation because that is what the
  // application is for. Reopening on the screen you last used sounds helpful
  // until the screen you last used is the one that failed.
  const [history, navigate] = useReducer(navigationHistory, initialHistory);
  const destination = history.entries[history.index] ?? defaultDestination;
  const view = destination.view;
  const searchNoteRequest = useMemo(() => destination.noteId === undefined ? null : { id: destination.noteId }, [destination]);
  const openNote = useCallback((id: number) => navigate({ type: "visit", destination: { view: "notes", noteId: id } }), []);

  // Both groups start closed. Dictation is a top-level destination, so the
  // opening rail is four screens and two sections -- shorter than the thirteen
  // flat rows it replaced, which was the point.
  const [openGroups, setOpenGroups] = useState<ReadonlySet<string>>(
    () => new Set(),
  );

  /*
   * Go to a screen, and make sure it is visible in the rail.
   *
   * Every route to a screen comes through here -- a rail press, either
   * `onGoToProviders`, and the error boundary's reset -- so a screen inside a
   * collapsed group can never be current-but-hidden. Opening is one-way on
   * purpose: it never closes the group you just left, because a rail that
   * reshuffled itself on every navigation is harder to use than one that only
   * grows.
   */
  const select = useCallback((id: ViewId) => {
    navigate({ type: "visit", destination: { view: id } });
  }, []);

  useEffect(() => {
    const group = groupOf(view);
    if (group !== null) {
      setOpenGroups((prev) => (prev.has(group) ? prev : new Set(prev).add(group)));
    }
  }, [view]);

  const toggleGroup = useCallback((id: string) => {
    setOpenGroups((prev) => {
      const next = new Set(prev);
      // `delete` reports whether it removed anything, so this is one lookup
      // rather than a `has` followed by a branch.
      if (!next.delete(id)) next.add(id);
      return next;
    });
  }, []);

  const titlebar = <TitleBar sidebarExpanded={sidebarExpanded} onToggleSidebar={onToggleSidebar} onOpenNote={openNote}
    canGoBack={history.index > 0} canGoForward={history.index < history.entries.length - 1}
    onBack={() => navigate({ type: "back" })} onForward={() => navigate({ type: "forward" })} />;

  if (!diagnostics) {
    // No rail yet, so no rail column to reserve: `.shell-bare` cancels the
    // `padding-left` that holds the fixed rail's width, and the message goes
    // inside `.content` so it keeps the page's own padding in both states.
    // Without that it would sit 13rem in from the left with nothing beside it.
    return (
      <>{titlebar}<main className="shell shell-bare">
        <div className="content">
          <h1>KnegisWhisper</h1>
          {error ? (
            <p className="error">{error}</p>
          ) : (
            <p className="muted">Loading…</p>
          )}
        </div>
      </main></>
    );
  }

  return (
    <>{titlebar}<main className={sidebarExpanded ? "shell" : "shell shell-sidebar-retracted"}>
      <Sidebar
        expanded={sidebarExpanded}
        onExpand={onExpandSidebar}
        view={view}
        openGroups={openGroups}
        onSelect={select}
        onToggleGroup={toggleGroup}
      />

      <div className="content">
        {/* `select` rather than `setView`, so landing on Diagnostics opens the
            Developer group that holds it. */}
        <ViewBoundary view={view} onReset={() => select("diagnostics")}>
          {renderView(view, diagnostics, select, searchNoteRequest, openNote)}
        </ViewBoundary>
      </div>
    </main></>
  );
}

/**
 * Four windows are served by one bundle, routed through `currentSurface()`. One
 * bundle means one CSP and one build.
 */
function MainWindow() {
  useEffect(installIdleScrollbars, []);
  const [sidebarExpanded, setSidebarExpanded] = useState(true);
  const toggleSidebar = () => setSidebarExpanded(expanded => !expanded);
  return <Shell sidebarExpanded={sidebarExpanded} onExpandSidebar={() => setSidebarExpanded(true)} onToggleSidebar={toggleSidebar} />;
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
