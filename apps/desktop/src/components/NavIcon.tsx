/** Supplied SVG assets inherit the navigation's selected/hover colour as masks. */
export function NavIcon({ id }: { id: string }) {
  const supplied: Record<string, string> = {
    dictation: "voice", notes: "text", "notes-upload": "upload", developer: "code", cli: "terminal",
  };
  if (supplied[id]) return <span aria-hidden="true" className={`nav-icon nav-icon-${supplied[id]}`} />;
  const paths: Record<string, string> = {
    meetings: "M8 11a3 3 0 1 0 0-6 3 3 0 0 0 0 6ZM2 20v-2a6 6 0 0 1 12 0v2M16 5a3 3 0 0 1 0 6m1 3a5 5 0 0 1 5 5v1",
    settings: "M4 7h6m4 0h6M4 17h10m4 0h2M10 4v6m4 4v6",
    providers: "M8 3v5m8-5v5M5 8h14v3a7 7 0 0 1-14 0V8Zm7 10v3",
    transcription: "M5 5h14M5 10h14M5 15h8M5 20h8m4-6 4 3-4 3",
    intelligence: "m12 3 2.5 6.5L21 12l-6.5 2.5L12 21l-2.5-6.5L3 12l6.5-2.5L12 3Z",
    "local-llm": "M5 5h14v11H5V5Zm3 15h8m-4-4v4",
    calendar: "M5 5h14v16H5V5Zm3-3v6m8-6v6M5 10h14",
    api: "m9 6-6 6 6 6m6-12 6 6-6 6",
    mcp: "M4 4h6v6H4V4Zm10 10h6v6h-6v-6ZM7 10v7h7m3-3V7h-7",
    diagnostics: "M3 12h4l3-8 4 16 3-8h4",
  };
  return <svg className="nav-icon" aria-hidden="true" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round"><path d={paths[id]} /></svg>;
}
