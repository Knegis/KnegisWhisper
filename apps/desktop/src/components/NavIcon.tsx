import { PATHS, SUPPLIED } from "../lib/navIcons";

/** Supplied SVG assets inherit the navigation's selected/hover colour as masks. */
export function NavIcon({ id }: { id: string }) {
  if (SUPPLIED[id]) return <span aria-hidden="true" className={`nav-icon nav-icon-${SUPPLIED[id]}`} />;
  return <svg className="nav-icon" aria-hidden="true" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round"><path d={PATHS[id]} /></svg>;
}
