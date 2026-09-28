/** Supplied SVG assets inherit the navigation's selected/hover colour as masks. */
export function NavIcon({ id }: { id: string }) {
  const supplied: Record<string, string> = {
    dictation: "voice", notes: "text", "notes-upload": "upload", developer: "code", cli: "terminal",
    providers: "git-branch", intelligence: "flare", calendar: "calendar-arrow-up",
    transcription: "align-left", diagnostics: "square-wave",
    meetings: "user-plus", "meeting-settings": "user-plus", "local-llm": "tv",
  };
  if (supplied[id]) return <span aria-hidden="true" className={`nav-icon nav-icon-${supplied[id]}`} />;
  const paths: Record<string, string> = {
    vocabulary: "M12 5v15M3 4c4-1 6 0 9 1 3-1 5-2 9-1v15c-4-1-6 0-9 1-3-1-5-2-9-1V4Z",
    shortcuts: "M3 7h18v10H3zM7 11h.01M11 11h.01M15 11h.01M8 14h8",
    settings: "M4 7h6m4 0h6M4 17h10m4 0h2M10 4v6m4 4v6",
    api: "m9 6-6 6 6 6m6-12 6 6-6 6",
    mcp: "M4 4h6v6H4V4Zm10 10h6v6h-6v-6ZM7 10v7h7m3-3V7h-7",
  };
  return <svg className="nav-icon" aria-hidden="true" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round"><path d={paths[id]} /></svg>;
}
