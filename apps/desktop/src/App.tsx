/**
 * The shell: a sidebar, a view, and an error boundary. Nothing else.
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
 * like a different `switch`.
 * There is nothing to link to from outside and no history worth preserving: this
 * is a settings window, not a site.
 *
 * # The error boundary
 *
 * A thrown render error in a Tauri webview is a blank window. There is no tab to
 * reload and no console in a bundled build, so the boundary is the only thing
 * standing between a mistake in one view and an application that appears to have
 * died.
 */

import { Component, useCallback, useState, type ReactNode } from "react";

import { getDiagnostics } from "./lib/ipc";
import { useRefresh } from "./lib/useRefresh";
import { MeetingNotification } from "./MeetingNotification";
import { Overlay } from "./Overlay";
import { UpdateNotification } from "./UpdateNotification";
import { DiagnosticsView } from "./views/Diagnostics";
import { DictationView } from "./views/Dictation";
import { IntegrationsApi } from "./views/IntegrationsApi";
import { Notes } from "./views/Notes";
import { SettingsMeetings } from "./views/SettingsMeetings";
import { SettingsIntelligence } from "./views/SettingsIntelligence";
import { SettingsIntelligenceLocal } from "./views/SettingsIntelligenceLocal";
import { SettingsProviders } from "./views/SettingsProviders";
import { SettingsTranscription } from "./views/SettingsTranscription";

/**
 * The screens, in sidebar order.
 *
 * Ordered by what someone needs first: a key is useless without a shortcut to
 * dictate with, and both are useless without knowing where the audio goes. These
 * ids are internal — the surface ids in `config/ui-surfaces.json` are the names
 * that have to stay stable, and those live on the components.
 */
const VIEWS = [
  { id: "dictation", label: "Dictation" },
  { id: "providers", label: "Providers" },
  { id: "transcription", label: "Transcription" },
  { id: "intelligence", label: "Intelligence" },
  { id: "local-llm", label: "Local models" },
  { id: "meetings", label: "Meetings" },
  { id: "notes", label: "Notes" },
  { id: "api", label: "Local API" },
  { id: "diagnostics", label: "Diagnostics" },
] as const;

type ViewId = (typeof VIEWS)[number]["id"];

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

function Shell() {
  const load = useCallback(() => getDiagnostics(), []);
  const { data: diagnostics, error } = useRefresh(load);

  // Not persisted, and starting on Dictation because that is what the
  // application is for. Reopening on the screen you last used sounds helpful
  // until the screen you last used is the one that failed.
  const [view, setView] = useState<ViewId>("dictation");

  if (!diagnostics) {
    return (
      <main className="shell">
        <h1>KnegisWhisper</h1>
        {error ? (
          <p className="error">{error}</p>
        ) : (
          <p className="muted">Loading…</p>
        )}
      </main>
    );
  }

  return (
    <main className="shell">
      <nav className="sidebar">
        <h1>KnegisWhisper</h1>
        <p className="muted">
          v{diagnostics.appVersion} · {diagnostics.os} {diagnostics.arch}
        </p>
        <ul>
          {VIEWS.map((v) => (
            <li key={v.id}>
              <button
                type="button"
                className={v.id === view ? "nav-current" : undefined}
                aria-current={v.id === view ? "page" : undefined}
                onClick={() => setView(v.id)}
              >
                {v.label}
              </button>
            </li>
          ))}
        </ul>
      </nav>

      <div className="content">
        <ViewBoundary view={view} onReset={() => setView("diagnostics")}>
          {view === "dictation" ? (
            <DictationView hook={diagnostics.modifierOnlyShortcuts} />
          ) : null}
          {view === "providers" ? <SettingsProviders /> : null}
          {view === "transcription" ? (
            <SettingsTranscription
              onGoToProviders={() => setView("providers")}
            />
          ) : null}
          {view === "intelligence" ? (
            <SettingsIntelligence onGoToProviders={() => setView("providers")} />
          ) : null}
          {view === "local-llm" ? <SettingsIntelligenceLocal /> : null}
          {view === "meetings" ? <SettingsMeetings /> : null}
          {view === "notes" ? <Notes /> : null}
          {view === "api" ? <IntegrationsApi /> : null}
          {view === "diagnostics" ? (
            <DiagnosticsView diagnostics={diagnostics} />
          ) : null}
        </ViewBoundary>
      </div>
    </main>
  );
}

/**
 * Both windows are served by one bundle, routed on the URL fragment. One bundle
 * means one CSP and one build.
 */
export default function App() {
  // One bundle, four surfaces, routed on the fragment. A `switch` rather than
  // a chain of ternaries so a fifth window is a compile-visible addition.
  switch (window.location.hash) {
    case "#overlay":
      return <Overlay />;
    case "#meeting":
      return <MeetingNotification />;
    case "#update":
      return <UpdateNotification />;
    default:
      return <Shell />;
  }
}
