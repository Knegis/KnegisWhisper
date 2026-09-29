import { useEffect, useRef, useState } from "react";
import { searchNotes, type SearchHit } from "../lib/ipc";
import { Icon } from "./Icon";

/**
 * The title bar's search field: always drawn, focused by Ctrl+K (Cmd+K), and
 * cleared and left by Escape. It searches notes -- meetings among them, since
 * a recorded meeting is a note -- and nothing else, which is why the field
 * says "notes and meetings" and not dictations. `open` is whether the results
 * popup shows, not whether the field does.
 */
export function TitleBarSearch({ onOpenNote }: { onOpenNote: (id: number) => void }) {
  const [query, setQuery] = useState("");
  const [open, setOpen] = useState(false);
  const [hits, setHits] = useState<SearchHit[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [active, setActive] = useState(0);
  const root = useRef<HTMLDivElement>(null);
  const input = useRef<HTMLInputElement>(null);
  useEffect(() => {
    const outside = (event: PointerEvent) => { if (event.target instanceof Node && !root.current?.contains(event.target)) setOpen(false); };
    const shortcut = (event: KeyboardEvent) => {
      if ((event.ctrlKey || event.metaKey) && !event.altKey && event.key.toLowerCase() === "k") {
        event.preventDefault(); input.current?.focus(); input.current?.select(); setOpen(true);
      }
    };
    document.addEventListener("pointerdown", outside);
    document.addEventListener("keydown", shortcut);
    return () => { document.removeEventListener("pointerdown", outside); document.removeEventListener("keydown", shortcut); };
  }, []);
  useEffect(() => {
    let cancelled = false;
    setHits([]); setActive(0); setError(null);
    if (!open || !query.trim()) { setLoading(false); return; }
    setLoading(true);
    const timer = window.setTimeout(() => {
      void searchNotes(query.trim(), 8).then(value => { if (!cancelled) setHits(value); })
        .catch(e => { if (!cancelled) setError(String(e)); })
        .finally(() => { if (!cancelled) setLoading(false); });
    }, 180);
    return () => { cancelled = true; window.clearTimeout(timer); };
  }, [query, open]);
  const choose = (hit: SearchHit) => { onOpenNote(hit.id); setOpen(false); input.current?.blur(); };
  // No `title` on the field: the trailing hint already says Ctrl+K, and the
  // window's tooltip would open under the field exactly where the results do.
  return <div className="titlebar-search" ref={root} onBlur={event => { if (!event.currentTarget.contains(event.relatedTarget)) setOpen(false); }}>
    <Icon name="search" size={15} />
    <input ref={input} type="search" role="combobox" aria-label="Search notes and meetings" placeholder="Search notes and meetings" aria-keyshortcuts="Control+K Meta+K"
      value={query} aria-expanded={open} aria-controls="titlebar-search-results" aria-autocomplete="list"
      aria-activedescendant={open && hits[active] ? `titlebar-hit-${hits[active].id}` : undefined}
      onFocus={() => setOpen(true)} onChange={event => { setQuery(event.target.value); setHits([]); setLoading(!!event.target.value.trim()); setOpen(true); }}
      onKeyDown={event => {
        if (event.key === "Escape") { event.preventDefault(); setQuery(""); setOpen(false); input.current?.blur(); }
        if (event.key === "ArrowDown" || event.key === "ArrowUp") {
          event.preventDefault(); setOpen(true);
          setActive(value => hits.length ? (value + (event.key === "ArrowDown" ? 1 : -1) + hits.length) % hits.length : 0);
        }
        if (event.key === "Enter" && open && hits[active]) { event.preventDefault(); choose(hits[active]); }
      }} />
    <span className="titlebar-search-key" aria-hidden="true">CTRL K</span>
    {open && <div className="titlebar-search-popup">
      <div className="titlebar-search-status" role="status">{!query.trim() ? "Search note titles and content" : loading ? "Searching?" : error ? "Search unavailable" : hits.length ? `${hits.length} result${hits.length === 1 ? "" : "s"}` : "No matching notes"}</div>
      {error && <p className="error" role="alert">{error}</p>}
      <div id="titlebar-search-results" role="listbox" aria-label="Matching notes">
        {hits.map((hit, index) => <button type="button" role="option" aria-selected={index === active} id={`titlebar-hit-${hit.id}`} key={hit.id}
          onMouseDown={event => event.preventDefault()} onMouseEnter={() => setActive(index)} onClick={() => choose(hit)}>
          <strong>{hit.title || "Untitled note"}</strong><span>{hit.snippet}</span>
        </button>)}
      </div>
    </div>}
  </div>;
}
