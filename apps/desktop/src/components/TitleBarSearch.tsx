import { useEffect, useRef, useState } from "react";
import { searchNotes, type SearchHit } from "../lib/ipc";
import { hitMeta } from "../lib/searchHit";
import { Icon } from "./Icon";

/**
 * The title bar's search field: always drawn, focused by Ctrl+K (Cmd+K), and
 * cleared and left by Escape. It searches notes -- meetings among them, since
 * a recorded meeting is a note -- and nothing else, which is why the field
 * says "notes and meetings" and not dictations. `open` is whether the results
 * popup shows, not whether the field does.
 *
 * The popup opens when something is asked of it -- typing, ArrowDown, Ctrl+K,
 * or a click in the field -- and not when the field gains focus. The field is
 * always in the title bar now, on the way from the window buttons to the
 * page, so opening on focus dropped a popup over the page for everyone who
 * only tabbed past it. Ctrl+K focuses and selects the field; it and a click
 * show the results for a query already in it, and with the field empty leave
 * the popup to the first key, since there is nothing yet to show.
 * `aria-controls` names the results only while they exist: the listbox is
 * rendered inside the popup, and an id that points at nothing is an error
 * to anything reading it.
 */
export function TitleBarSearch({ onOpenNote }: { onOpenNote: (id: number, transcriptRow?: number) => void }) {
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
        event.preventDefault(); input.current?.focus(); input.current?.select();
        // The field's own value, not `query`: this listener is registered once, and would read the first render's.
        if (input.current?.value.trim()) setOpen(true);
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
  // A hit in a meeting transcript opens the note at its row (KNEG-406).
  const choose = (hit: SearchHit) => { onOpenNote(hit.id, hit.transcriptRow?.index); setOpen(false); input.current?.blur(); };
  // No `title` on the field: the trailing hint already says Ctrl+K, and the
  // window's tooltip would open under the field exactly where the results do.
  return <div className="titlebar-search" ref={root} onBlur={event => { if (!event.currentTarget.contains(event.relatedTarget)) setOpen(false); }}>
    <Icon name="search" size={16} />
    <input ref={input} type="search" role="combobox" aria-label="Search notes and meetings" placeholder="Search notes and meetings" aria-keyshortcuts="Control+K Meta+K"
      value={query} aria-expanded={open} aria-controls={open ? "titlebar-search-results" : undefined} aria-autocomplete="list"
      aria-activedescendant={open && hits[active] ? `titlebar-hit-${hits[active].id}` : undefined}
      onClick={() => { if (query.trim()) setOpen(true); }}
      onChange={event => { setQuery(event.target.value); setHits([]); setLoading(!!event.target.value.trim()); setOpen(true); }}
      onKeyDown={event => {
        if (event.key === "Escape") { event.preventDefault(); setQuery(""); setOpen(false); input.current?.blur(); }
        // ArrowDown opens a closed popup, as a combobox's does; the arrows move through the
        // results once it is open. ArrowUp on a closed one opens nothing.
        if (event.key === "ArrowDown" && !open) { event.preventDefault(); setOpen(true); }
        else if ((event.key === "ArrowDown" || event.key === "ArrowUp") && open) {
          event.preventDefault();
          setActive(value => hits.length ? (value + (event.key === "ArrowDown" ? 1 : -1) + hits.length) % hits.length : 0);
        }
        if (event.key === "Enter" && open && hits[active]) { event.preventDefault(); choose(hits[active]); }
      }} />
    <span className="titlebar-search-key" aria-hidden="true">ctrl k</span>
    {open && <div className="titlebar-search-popup">
      <div className="titlebar-search-status" role="status">{!query.trim() ? "Search note titles and content" : loading ? "Searching…" : error ? "Search unavailable" : hits.length ? `${hits.length} result${hits.length === 1 ? "" : "s"}` : "No matching notes"}</div>
      {error && <p className="error" role="alert">{error}</p>}
      <div id="titlebar-search-results" role="listbox" aria-label="Matching notes">
        {hits.map((hit, index) => <button type="button" role="option" aria-selected={index === active} id={`titlebar-hit-${hit.id}`} key={hit.id}
          onMouseDown={event => event.preventDefault()} onMouseEnter={() => setActive(index)} onClick={() => choose(hit)}>
          <strong>{hit.title || "Untitled note"}</strong><span>{hitMeta(hit)}</span>
        </button>)}
      </div>
    </div>}
  </div>;
}
